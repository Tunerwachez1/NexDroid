use std::os::unix::net::UnixStream;
use std::sync::Arc;

use smithay::reexports::wayland_server::Display;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_compositor, wl_keyboard, wl_registry, wl_seat, wl_surface};
use wayland_client::{Connection, Dispatch, QueueHandle, WEnum, delegate_noop};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

use super::Compositor;
use crate::ClientState;

#[derive(Default)]
struct GuestKeys {
    focused: bool,
    events: Vec<(u32, wl_keyboard::KeyState)>,
}

#[test]
fn embedded_seat_delivers_physical_keys_to_the_guest() {
    let (client, server) = UnixStream::pair().unwrap();
    let (commands, rx) = std::sync::mpsc::channel::<Option<(u16, bool)>>();
    let server_thread = std::thread::spawn(move || {
        let mut display = Display::<Compositor>::new().unwrap();
        let mut handle = display.handle();
        let mut compositor = Compositor::new(&handle, (1280, 720), 60_000);
        handle
            .insert_client(server, Arc::new(ClientState::default()))
            .unwrap();
        loop {
            display.dispatch_clients(&mut compositor).unwrap();
            match rx.try_recv() {
                Ok(Some((code, pressed))) => compositor.key(code, pressed, 0),
                Ok(None) | Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
            display.flush_clients().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    });

    let conn = Connection::from_socket(client).unwrap();
    let (globals, mut queue) = registry_queue_init::<GuestKeys>(&conn).unwrap();
    let qh = queue.handle();
    let compositor: wl_compositor::WlCompositor = globals.bind(&qh, 1..=1, ()).unwrap();
    let seat: wl_seat::WlSeat = globals.bind(&qh, 5..=5, ()).unwrap();
    let shell: xdg_wm_base::XdgWmBase = globals.bind(&qh, 1..=1, ()).unwrap();
    let _keyboard = seat.get_keyboard(&qh, ());
    let surface = compositor.create_surface(&qh, ());
    let xdg = shell.get_xdg_surface(&surface, &qh, ());
    let _toplevel = xdg.get_toplevel(&qh, ());
    surface.commit();

    let mut guest = GuestKeys::default();
    queue.roundtrip(&mut guest).unwrap();
    assert!(guest.focused, "the guest must receive keyboard enter");
    let expected = [
        (29, wl_keyboard::KeyState::Pressed),
        (29, wl_keyboard::KeyState::Released),
        (30, wl_keyboard::KeyState::Pressed),
        (30, wl_keyboard::KeyState::Released),
        (82, wl_keyboard::KeyState::Pressed),
        (82, wl_keyboard::KeyState::Released),
    ];
    for (code, state) in expected {
        commands
            .send(Some((code as u16, state == wl_keyboard::KeyState::Pressed)))
            .unwrap();
    }
    // Round trips let the real server dispatch its seat queue, with a bound
    // so a missing key fails the test rather than waiting indefinitely.
    for _ in 0..30 {
        if guest.events.len() >= expected.len() {
            break;
        }
        queue.roundtrip(&mut guest).unwrap();
    }
    commands.send(None).unwrap();
    server_thread.join().unwrap();
    assert_eq!(guest.events, expected);
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for GuestKeys {
    fn event(
        guest: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Enter { .. } => guest.focused = true,
            wl_keyboard::Event::Key {
                key,
                state: WEnum::Value(state),
                ..
            } => guest.events.push((key, state)),
            _ => {}
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for GuestKeys {
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

delegate_noop!(GuestKeys: ignore wl_compositor::WlCompositor);
delegate_noop!(GuestKeys: ignore wl_seat::WlSeat);
delegate_noop!(GuestKeys: ignore wl_surface::WlSurface);
delegate_noop!(GuestKeys: ignore xdg_wm_base::XdgWmBase);
delegate_noop!(GuestKeys: ignore xdg_surface::XdgSurface);
delegate_noop!(GuestKeys: ignore xdg_toplevel::XdgToplevel);
