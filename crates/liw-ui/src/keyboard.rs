//! Physical key events for the game surface, including keys GPUI normalizes
//! or handles as modifier state. GPUI remains the reader of the display fd.

use anyhow::{Context, Result};
use gpui::Window;
use std::io::Write;
use std::path::{Path, PathBuf};
use wayland_client::backend::ObjectId;
use wayland_client::globals::{GlobalList, GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_keyboard, wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum};
use xkbcommon::xkb;

const STATE_FILE: &str = "liwinux-keyboard.json";

pub fn clear_export(runtime: &Path) {
    let _ = std::fs::remove_file(runtime.join(STATE_FILE));
}

#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize)]
struct KeyboardState {
    layout: &'static str,
    caps: bool,
}

impl KeyboardState {
    fn read(xkb: &xkb::State) -> Option<Self> {
        let group = xkb.serialize_layout(xkb::STATE_LAYOUT_EFFECTIVE);
        let map = xkb.get_keymap();
        if group >= map.num_layouts() {
            return None;
        }
        let layout = match map.layout_get_name(group) {
            "English (US)" => "us",
            "Russian" => "ru",
            _ => return None,
        };
        let caps = xkb.mod_name_is_active(xkb::MOD_NAME_CAPS, xkb::STATE_MODS_LOCKED);
        Some(Self { layout, caps })
    }

    fn publish(self, runtime: &Path) -> Result<()> {
        let mut file = tempfile::NamedTempFile::new_in(runtime)?;
        serde_json::to_writer(file.as_file_mut(), &self)?;
        file.as_file_mut().flush()?;
        file.persist(runtime.join(STATE_FILE))?;
        Ok(())
    }
}

pub enum KeyEvent {
    Key { code: u16, pressed: bool },
    FocusLost,
}

struct Events {
    surface: ObjectId,
    focused: bool,
    events: Vec<KeyEvent>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    xkb: Option<xkb::State>,
    runtime: PathBuf,
    exported: Option<KeyboardState>,
}

impl Events {
    fn export(&mut self, state: Option<KeyboardState>) {
        if self.exported == state {
            return;
        }
        if let Some(state) = state {
            if let Err(error) = state.publish(&self.runtime) {
                tracing::warn!(%error, "could not share Android keyboard state");
                clear_export(&self.runtime);
                self.exported = None;
                return;
            }
        } else {
            clear_export(&self.runtime);
        }
        self.exported = state;
    }
}

pub struct Keyboard {
    conn: Connection,
    globals: Option<GlobalList>,
    queue: EventQueue<Events>,
    seat: wl_seat::WlSeat,
    events: Events,
}

impl Keyboard {
    pub fn new(window: &Window) -> Result<Self> {
        let conn = crate::pointer_lock::host_connection(window)?;
        let surface = crate::pointer_lock::surface(window, &conn)?.id();
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is missing")?;
        let (globals, queue) = registry_queue_init(&conn)?;
        let seat = globals
            .bind(&queue.handle(), 5..=5, ())
            .context("Wayland не предоставил устройство ввода")?;
        conn.flush()?;
        Ok(Self {
            conn,
            globals: Some(globals),
            queue,
            seat,
            events: Events {
                surface,
                focused: false,
                events: Vec::new(),
                keyboard: None,
                xkb: None,
                runtime: runtime.into(),
                exported: None,
            },
        })
    }

    pub fn poll(&mut self) -> Result<Vec<KeyEvent>> {
        self.queue.dispatch_pending(&mut self.events)?;
        Ok(std::mem::take(&mut self.events.events))
    }
}

impl Drop for Keyboard {
    fn drop(&mut self) {
        self.events.export(None);
        if let Some(globals) = self.globals.take() {
            globals.destroy();
        }
        // The keyboard proxy is kept with the queue state so the seat
        // capabilities callback can recreate it on hotplug.
        if let Some(keyboard) = self.events.keyboard.take() {
            keyboard.release();
        }
        self.seat.release();
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

impl Dispatch<wl_seat::WlSeat, ()> for Events {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
        else {
            return;
        };
        if capabilities.contains(wl_seat::Capability::Keyboard) {
            if state.keyboard.is_none() {
                state.keyboard = Some(seat.get_keyboard(qh, ()));
            }
        } else if let Some(keyboard) = state.keyboard.take() {
            keyboard.release();
            state.focused = false;
            state.xkb = None;
            state.export(None);
            state.events.push(KeyEvent::FocusLost);
        }
        let _ = conn.flush();
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for Events {
    fn event(
        state: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Keymap {
                format: WEnum::Value(wl_keyboard::KeymapFormat::XkbV1),
                fd,
                size,
            } => {
                let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
                // SAFETY: Wayland supplies a valid keymap file of the advertised size.
                let keymap = unsafe {
                    xkb::Keymap::new_from_fd(
                        &context,
                        fd,
                        size as usize,
                        xkb::KEYMAP_FORMAT_TEXT_V1,
                        xkb::KEYMAP_COMPILE_NO_FLAGS,
                    )
                };
                state.export(None);
                state.xkb = keymap.ok().flatten().map(|map| xkb::State::new(&map));
            }
            wl_keyboard::Event::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
                ..
            } if state.focused => {
                let keyboard = state.xkb.as_mut().and_then(|xkb| {
                    xkb.update_mask(mods_depressed, mods_latched, mods_locked, 0, 0, group);
                    KeyboardState::read(xkb)
                });
                state.export(keyboard);
            }
            wl_keyboard::Event::Enter { surface, .. } => {
                state.focused = surface.id() == state.surface;
            }
            wl_keyboard::Event::Leave { surface, .. } if surface.id() == state.surface => {
                state.focused = false;
                state.export(None);
                state.events.push(KeyEvent::FocusLost);
            }
            wl_keyboard::Event::Key {
                key,
                state: WEnum::Value(key_state),
                ..
            } if state.focused => {
                if let Ok(code) = u16::try_from(key) {
                    match key_state {
                        wl_keyboard::KeyState::Pressed => state.events.push(KeyEvent::Key {
                            code,
                            pressed: true,
                        }),
                        wl_keyboard::KeyState::Released => state.events.push(KeyEvent::Key {
                            code,
                            pressed: false,
                        }),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_the_wayland_caps_lock_and_active_layout() {
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap = xkb::Keymap::new_from_names(
            &context,
            "evdev",
            "pc105",
            "us,ru",
            "",
            None,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .unwrap();
        let mut keyboard = xkb::State::new(&keymap);
        let caps = 1 << keymap.mod_get_index(xkb::MOD_NAME_CAPS);
        let directory = tempfile::tempdir().unwrap();
        for (group, locked, layout) in
            [(0, 0, "us"), (0, caps, "us"), (1, caps, "ru"), (1, 0, "ru")]
        {
            keyboard.update_mask(0, 0, locked, 0, 0, group);
            KeyboardState::read(&keyboard)
                .unwrap()
                .publish(directory.path())
                .unwrap();
            let contents = std::fs::read(directory.path().join(STATE_FILE)).unwrap();
            let state: serde_json::Value = serde_json::from_slice(&contents).unwrap();
            assert_eq!(
                state,
                serde_json::json!({"layout": layout, "caps": locked != 0})
            );
        }
    }
}
