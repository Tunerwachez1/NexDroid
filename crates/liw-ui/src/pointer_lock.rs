//! Wayland pointer lock for the GPUI game surface. GPUI owns and reads the
//! display connection. We dispatch only the events queued for our objects.

use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use gpui::Window;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use wayland_client::backend::{Backend, ObjectId};
use wayland_client::globals::{GlobalList, GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_pointer, wl_registry, wl_seat, wl_surface};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum, delegate_noop};
use wayland_protocols::wp::{
    cursor_shape::v1::client::{
        wp_cursor_shape_device_v1 as shape, wp_cursor_shape_manager_v1 as shapes,
    },
    pointer_constraints::zv1::client::{
        zwp_locked_pointer_v1 as lock, zwp_pointer_constraints_v1 as constraints,
    },
    relative_pointer::zv1::client::{
        zwp_relative_pointer_manager_v1 as relatives, zwp_relative_pointer_v1 as relative,
    },
};

#[derive(Default)]
struct Events {
    entered: Option<(ObjectId, u32)>,
    request: Option<ObjectId>,
    locked: bool,
    unlocked: bool,
    delta: (f32, f32),
}

pub struct PointerLock {
    conn: Connection,
    globals: Option<GlobalList>,
    queue: EventQueue<Events>,
    seat: wl_seat::WlSeat,
    pointer: wl_pointer::WlPointer,
    constraints: constraints::ZwpPointerConstraintsV1,
    relative: relative::ZwpRelativePointerV1,
    shape: Option<shape::WpCursorShapeDeviceV1>,
    lock: Option<lock::ZwpLockedPointerV1>,
    requested: Option<Instant>,
    events: Events,
}

pub enum CaptureEvent {
    Idle,
    Motion(f32, f32),
    Released,
    TimedOut,
}

pub(crate) fn surface(window: &Window, conn: &Connection) -> Result<wl_surface::WlSurface> {
    let RawWindowHandle::Wayland(handle) = HasWindowHandle::window_handle(window)?.as_raw() else {
        bail!("Захват мыши доступен в Wayland.");
    };
    // GPUI owns this proxy and keeps it alive for the request. from_id
    // borrows the existing object without installing a second handler.
    let id = unsafe {
        ObjectId::from_ptr(
            wl_surface::WlSurface::interface(),
            handle.surface.as_ptr().cast(),
        )
    }?;
    Ok(wl_surface::WlSurface::from_id(conn, id)?)
}

pub(crate) fn host_connection(window: &Window) -> Result<Connection> {
    let RawDisplayHandle::Wayland(handle) = window.display_handle()?.as_raw() else {
        bail!("Окно использует X11.");
    };
    // GPUI owns the display for the life of the application. This backend
    // manages only the objects on its own queue and never closes the display.
    Ok(Connection::from_backend(unsafe {
        Backend::from_foreign_display(handle.display.as_ptr().cast())
    }))
}

impl PointerLock {
    pub fn new(window: &Window) -> Result<Self> {
        let conn = host_connection(window)?;
        let (globals, queue) = registry_queue_init::<Events>(&conn)?;
        let qh = queue.handle();
        let seat: wl_seat::WlSeat = globals
            .bind(&qh, 5..=5, ())
            .context("Wayland не предоставил устройство ввода")?;
        let constraints = globals
            .bind(&qh, 1..=1, ())
            .context("Wayland не поддерживает захват курсора")?;
        let manager: relatives::ZwpRelativePointerManagerV1 = globals
            .bind(&qh, 1..=1, ())
            .context("Wayland не передаёт относительное движение мыши")?;
        let pointer = seat.get_pointer(&qh, ());
        let relative = manager.get_relative_pointer(&pointer, &qh, ());
        manager.destroy();
        let shape = globals
            .bind::<shapes::WpCursorShapeManagerV1, _, _>(&qh, 1..=1, ())
            .ok()
            .map(|manager| {
                let device = manager.get_pointer(&pointer, &qh, ());
                manager.destroy();
                device
            });
        conn.flush()?;
        Ok(Self {
            conn,
            globals: Some(globals),
            queue,
            seat,
            pointer,
            constraints,
            relative,
            shape,
            lock: None,
            requested: None,
            events: Events::default(),
        })
    }

    pub fn request(&mut self, window: &Window) -> Result<()> {
        let surface = surface(window, &self.conn)?;
        self.events.unlocked = false;
        self.events.delta = (0.0, 0.0);
        let lock = self.constraints.lock_pointer(
            &surface,
            &self.pointer,
            None,
            constraints::Lifetime::Oneshot,
            &self.queue.handle(),
            (),
        );
        self.events.request = Some(lock.id());
        self.lock = Some(lock);
        self.requested = Some(Instant::now());
        self.conn.flush()?;
        Ok(())
    }

    pub fn requested(&self) -> bool {
        self.lock.is_some()
    }

    pub fn active(&self) -> bool {
        self.events.locked
    }

    pub fn release(&mut self) {
        if let Some(lock) = self.lock.take() {
            lock.destroy();
        }
        if let (Some(shape), Some((_, serial))) = (&self.shape, &self.events.entered) {
            if self.events.locked || self.events.unlocked {
                shape.set_shape(*serial, shape::Shape::Default);
            }
        }
        self.requested = None;
        self.events.request = None;
        self.events.locked = false;
        self.events.unlocked = false;
        self.events.delta = (0.0, 0.0);
        let _ = self.conn.flush();
    }

    pub fn poll(&mut self) -> Result<CaptureEvent> {
        let was_locked = self.events.locked;
        self.queue.dispatch_pending(&mut self.events)?;
        if self.events.unlocked {
            self.release();
            return Ok(CaptureEvent::Released);
        }
        if self.events.locked && !was_locked {
            // Restore is available through cursor-shape-v1. Without it,
            // leave the compositor's cursor visible so it can be used again
            // immediately after releasing the lock.
            if let (Some(_), Some((_, serial))) = (&self.shape, &self.events.entered) {
                self.pointer.set_cursor(*serial, None, 0, 0);
                self.conn.flush()?;
            }
        }
        if self.events.locked {
            let (dx, dy) = std::mem::take(&mut self.events.delta);
            return Ok(CaptureEvent::Motion(dx, dy));
        }
        if self
            .requested
            .is_some_and(|at| at.elapsed() > Duration::from_secs(2))
        {
            self.release();
            return Ok(CaptureEvent::TimedOut);
        }
        Ok(CaptureEvent::Idle)
    }
}

impl Drop for PointerLock {
    fn drop(&mut self) {
        self.release();
        if let Some(shape) = self.shape.take() {
            shape.destroy();
        }
        self.relative.destroy();
        self.constraints.destroy();
        self.pointer.release();
        self.seat.release();
        if let Some(globals) = self.globals.take() {
            globals.destroy();
        }
        let _ = self.conn.flush();
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Events {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for Events {
    fn event(
        state: &mut Self,
        _: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter {
                serial, surface, ..
            } => state.entered = Some((surface.id(), serial)),
            wl_pointer::Event::Leave { .. } => state.entered = None,
            _ => {}
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for Events {
    fn event(
        state: &mut Self,
        _: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
        {
            if !capabilities.contains(wl_seat::Capability::Pointer) && state.locked {
                state.unlocked = true;
            }
        }
    }
}

impl Dispatch<lock::ZwpLockedPointerV1, ()> for Events {
    fn event(
        state: &mut Self,
        lock: &lock::ZwpLockedPointerV1,
        event: lock::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if state.request.as_ref() != Some(&lock.id()) {
            return;
        }
        match event {
            lock::Event::Locked => state.locked = true,
            lock::Event::Unlocked => {
                state.locked = false;
                state.unlocked = true;
            }
            _ => {}
        }
    }
}

impl Dispatch<relative::ZwpRelativePointerV1, ()> for Events {
    fn event(
        state: &mut Self,
        _: &relative::ZwpRelativePointerV1,
        event: relative::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if state.locked {
            let relative::Event::RelativeMotion {
                dx_unaccel,
                dy_unaccel,
                ..
            } = event
            else {
                return;
            };
            state.delta.0 += dx_unaccel as f32;
            state.delta.1 += dy_unaccel as f32;
        }
    }
}

delegate_noop!(Events: ignore constraints::ZwpPointerConstraintsV1);
delegate_noop!(Events: ignore relatives::ZwpRelativePointerManagerV1);
delegate_noop!(Events: ignore shapes::WpCursorShapeManagerV1);
delegate_noop!(Events: ignore shape::WpCursorShapeDeviceV1);
