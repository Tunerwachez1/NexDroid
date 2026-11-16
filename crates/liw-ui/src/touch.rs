//! The mouse, as a finger.
//!
//! A click in the game window has to reach Android, and the compositor is not
//! the way to do it. Waydroid's `EventHub` is patched to read a FIFO at
//! `/dev/input/wl_touch_events` and the whole input engine already writes
//! there — that path is measured, it bypasses the compositor entirely, and it
//! is what the key mapper uses. Sending `wl_pointer` events instead would mean
//! a second, unproven route into the same place.
//!
//! So a press becomes a finger down, a drag becomes a move, and a release
//! becomes a finger up.

use std::collections::VecDeque;
use std::time::Instant;

use liw_input::engine::{Engine, InputEvent, TriggerKind};
use liw_input::profile::{Binding, Profile, Trigger};
use liw_input::touch::{MAX_POINTERS, Norm, TouchAction};
use liw_input::wl_touch::WlTouchBackend;

/// Reserve the highest slot for normal clicks alongside mapped keys.
const MOUSE_ID: u8 = MAX_POINTERS as u8 - 1;
const MAX_PENDING_FRAMES: usize = 32;

/// Sends mouse presses into Android as touches.
#[derive(Default)]
pub struct Touch {
    backend: Option<WlTouchBackend>,
    engine: Option<Engine>,
    pending: VecDeque<Vec<TouchAction>>,
    clock: Option<Instant>,
    display: Option<(u32, u32)>,
    pub aim_key: Option<u16>,
    /// A finger is currently down.
    down: bool,
    /// Why it is not working, if it is not. Shown rather than logged: a click
    /// that silently goes nowhere is the worst outcome here.
    pub error: Option<String>,
}

impl Touch {
    pub fn is_ready(&self) -> bool {
        self.backend.is_some()
    }

    /// Called by the reconnect task, so an idle pipe can be replaced before
    /// the next click or mapped key tries to write to it.
    pub fn check_connection(&mut self) -> bool {
        if self.backend.as_ref().is_some_and(WlTouchBackend::reader_disconnected) {
            tracing::debug!("Android closed the touch pipe");
            self.disconnect();
        }
        self.is_ready()
    }

    fn disconnect(&mut self) {
        self.backend = None;
        self.release_all();
        self.error = Some("Переподключение ввода Android…".into());
    }

    /// Takes an already-opened pipe.
    ///
    /// Opening it needs root, so the daemon's helper does that and passes the
    /// descriptor over D-Bus; this only wraps it.
    pub fn attach(&mut self, pipe: std::fs::File, w: u32, h: u32) {
        match WlTouchBackend::from_pipe(pipe, w, h) {
            Ok(b) => {
                // Worth a line: without the pipe every click is silently
                // swallowed, and "the mouse does nothing" gives no clue
                // whether it never opened or opened and then broke.
                tracing::info!(width = w, height = h, "touch pipe attached");
                self.backend = Some(b);
                self.display = Some((w, h));
                if let Some(engine) = self.engine.as_mut() {
                    engine.set_aspect(w, h);
                }
                self.error = None;
            }
            Err(e) => {
                tracing::warn!(error = %e, "touch pipe unusable");
                self.error = Some(e.to_string());
            }
        }
    }

    /// Press at a normalised position.
    pub fn press(&mut self, at: Norm) {
        // A press while a finger is already down would leave the old one
        // stuck: Android holds a slot until it is lifted, and a stuck finger
        // is a game that stops responding.
        if self.down {
            self.release();
        }
        let sent = self.send(&[TouchAction::Down { id: MOUSE_ID, at }]);
        tracing::debug!(x = at.x, y = at.y, sent, "press");
        if sent {
            self.down = true;
        }
    }

    /// Drag. Ignored when nothing is pressed — Android has no hover.
    pub fn drag(&mut self, at: Norm) {
        if !self.down {
            return;
        }
        self.send(&[TouchAction::Move { id: MOUSE_ID, at }]);
    }

    pub fn release(&mut self) {
        if !self.down {
            return;
        }
        self.send(&[TouchAction::Up { id: MOUSE_ID }]);
        self.down = false;
    }

    /// Load the saved bindings for the foreground game window.
    pub fn load_profile(&mut self, mut profile: Option<Profile>) {
        self.release_all();
        self.aim_key = profile.as_mut().and_then(|profile| {
            let assigned = profile
                .bindings
                .values()
                .find_map(|binding| match binding {
                    Binding::Aim { toggle, .. } => Some(toggle.clone()),
                    _ => None,
                })?;
            let key = match assigned {
                Some(Trigger::Key(code)) if crate::keys::can_capture_with(code) => Some(code),
                _ => (59..=68).chain([88]).find(|code| {
                    crate::keys::can_capture_with(*code) && !profile
                        .bindings
                        .values()
                        .any(|binding| binding.triggers().contains(&Trigger::Key(*code)))
                }),
            };
            // The window toggles capture. The engine receives relative motion
            // only while captured, so its aim no longer needs a held key.
            for binding in profile.bindings.values_mut() {
                if let Binding::Aim { toggle, .. } = binding {
                    *toggle = None;
                }
            }
            key
        });
        self.engine = profile.map(|profile| {
            let mut engine = Engine::with_pointer_limit(profile, usize::from(MOUSE_ID));
            if let Some((w, h)) = self.display {
                engine.set_aspect(w, h);
            }
            engine.set_offscreen_ok(true);
            engine
        });
    }

    pub fn input(&mut self, event: InputEvent) {
        if self.backend.is_none() {
            return;
        }
        if let Some(engine) = self.engine.as_mut() {
            let actions = engine.handle(event);
            self.send(&actions);
        }
    }

    pub fn has_binding(&self, trigger: &Trigger) -> bool {
        self.engine.as_ref().is_some_and(|engine| {
            engine.profile().bindings.values().any(|binding| binding.triggers().contains(trigger))
        })
    }

    pub fn button(&mut self, button: Trigger, down: bool) -> bool {
        if !self.has_binding(&button) {
            return false;
        }
        let trigger = TriggerKind::from(&button);
        self.input(if down { InputEvent::Press(trigger) } else { InputEvent::Release(trigger) });
        true
    }

    pub fn tick(&mut self) {
        if let Some(engine) = self.engine.as_mut().filter(|e| e.has_pending()) {
            let now = self
                .clock
                .get_or_insert_with(Instant::now)
                .elapsed()
                .as_millis() as u64;
            let actions = engine.tick(now);
            self.send(&actions);
        }
        self.flush();
    }

    pub fn release_aim(&mut self) {
        if let Some(engine) = self.engine.as_mut() {
            let actions = engine.release_aim();
            self.send(&actions);
        }
    }

    /// Discard unsent input and lift each finger already delivered to Android.
    pub fn release_all(&mut self) {
        self.pending.clear();
        if let Some(engine) = self.engine.as_mut() {
            let _ = engine.set_enabled(false);
            let _ = engine.set_enabled(true);
        }
        self.down = false;
        if let Some(backend) = &self.backend {
            let actions = backend.release_actions();
            if !actions.is_empty() {
                self.pending.push_back(actions);
                self.flush();
            }
        }
    }

    fn send(&mut self, actions: &[TouchAction]) -> bool {
        if self.backend.is_none() {
            self.error
                .get_or_insert_with(|| "Ввод Android ещё не подключён.".into());
            return false;
        }
        if actions.is_empty() {
            return true;
        }
        if self.pending.len() == MAX_PENDING_FRAMES {
            self.release_all();
            self.error =
                Some("Android не успевает принимать касания. Нажмите клавиши снова.".into());
            return false;
        }
        // Preserve frame boundaries for Down/Up, and keep only the latest
        // position per slot when consecutive movement frames queue up.
        if let Some(previous) = self.pending.back_mut().filter(|previous| {
            previous
                .iter()
                .chain(actions)
                .all(|action| matches!(action, TouchAction::Move { .. }))
        }) {
            for action in actions {
                let TouchAction::Move { id, .. } = action else {
                    unreachable!()
                };
                if let Some(old) = previous
                    .iter_mut()
                    .find(|old| matches!(old, TouchAction::Move { id: old_id, .. } if old_id == id))
                {
                    *old = *action;
                } else {
                    previous.push(*action);
                }
            }
        } else {
            self.pending.push_back(actions.to_vec());
        }
        self.flush();
        self.backend.is_some()
    }

    fn flush(&mut self) {
        while let (Some(backend), Some(actions)) = (&mut self.backend, self.pending.front()) {
            match backend.try_dispatch(actions) {
                Ok(true) => {
                    self.pending.pop_front();
                    self.error = None;
                }
                Ok(false) => return,
                Err(error) => {
                    tracing::warn!(%error, "touch pipe write failed; reconnecting");
                    self.disconnect();
                    return;
                }
            }
        }
    }
}

/// Turns a click in the picture into a position on the Android screen.
///
/// `point` is relative to the picture's top-left corner and `shown` is how
/// large the guest is drawn. Returns `None` for a click in the letterboxing:
/// that is beside the game, not on it, and clamping would send a touch to the
/// edge of the screen instead.
pub fn to_norm(point: (f32, f32), shown: (f32, f32)) -> Option<Norm> {
    if shown.0 <= 0.0 || shown.1 <= 0.0 {
        return None;
    }
    let (x, y) = (point.0 / shown.0, point.1 / shown.1);
    if !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) {
        return None;
    }
    Some(Norm::new(x, y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_joystick_and_camera_share_the_pipe_and_release_independently() {
        use std::io::Read;
        use std::os::fd::OwnedFd;
        use std::os::unix::net::UnixStream;

        let (writer, mut reader) = UnixStream::pair().unwrap();
        writer.set_nonblocking(true).unwrap();
        reader.set_nonblocking(true).unwrap();

        let profile = Profile::from_toml(
            r#"
name = "Test"
package = "org.example.game"
[bindings.move]
type = "joystick"
up = { Key = 17 }
left = { Key = 30 }
down = { Key = 31 }
right = { Key = 32 }
center = { x = 0.2, y = 0.7 }
radius = 0.1
[bindings.look]
type = "aim"
toggle = { Key = 60 }
origin = { x = 0.7, y = 0.5 }
sensitivity = 0.0016
deadzone = 0.0
"#,
        )
        .unwrap();

        let mut touch = Touch::default();
        touch.load_profile(Some(profile));
        assert_eq!(touch.aim_key, Some(60));
        touch.attach(std::fs::File::from(OwnedFd::from(writer)), 1920, 1080);

        let mut frames = || {
            let mut bytes = [0u8; 4096];
            let mut result = Vec::new();
            loop {
                match reader.read(&mut bytes) {
                    Ok(0) => break,
                    Ok(n) => result.extend_from_slice(&bytes[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) => panic!("{e}"),
                }
            }
            assert_eq!(result.len() % 24, 0);
            result
                .chunks_exact(24)
                .map(|event| {
                    (
                        u16::from_ne_bytes(event[16..18].try_into().unwrap()),
                        u16::from_ne_bytes(event[18..20].try_into().unwrap()),
                        i32::from_ne_bytes(event[20..24].try_into().unwrap()),
                    )
                })
                .collect::<Vec<_>>()
        };

        touch.input(InputEvent::Press(TriggerKind::Key(17)));
        touch.tick();
        let walk = frames();
        assert_eq!(walk.iter().filter(|event| **event == (0, 0, 0)).count(), 2);
        assert!(walk.contains(&(3, 0x36, 756)));
        assert!(walk.contains(&(3, 0x36, 564)));

        touch.input(InputEvent::MouseMove { dx: 20.0, dy: 0.0 });
        touch.tick();
        touch.tick();
        let look = frames();
        assert!(look.contains(&(3, 0x2f, 1)));
        assert!(look.iter().filter(|event| **event == (0, 0, 0)).count() >= 2);

        touch.release_aim();
        let release = frames();
        assert_eq!(release, vec![(3, 0x2f, 1), (3, 0x39, -1), (0, 0, 0)]);

        touch.input(InputEvent::Release(TriggerKind::Key(17)));
        assert_eq!(frames(), vec![(3, 0x2f, 0), (3, 0x39, -1), (0, 0, 0)]);
    }

    #[test]
    fn fire_holds_the_configured_touch_until_left_click_releases() {
        let profile = Profile::from_toml(r#"
name = "Test"
package = "org.example.game"
[bindings.fire]
type = "fire"
at = { x = 0.75, y = 0.5 }
"#).unwrap();

        let mut engine = Engine::new(profile);
        assert_eq!(
            engine.handle(InputEvent::Press(TriggerKind::MouseLeft)),
            vec![TouchAction::Down { id: 0, at: Norm::new(0.75, 0.5) }],
        );
        assert!(engine.handle(InputEvent::Press(TriggerKind::MouseLeft)).is_empty());
        assert_eq!(
            engine.handle(InputEvent::Release(TriggerKind::MouseLeft)),
            vec![TouchAction::Up { id: 0 }],
        );
    }

    #[test]
    fn a_click_in_the_picture_becomes_a_position() {
        let n = to_norm((640.0, 360.0), (1280.0, 720.0)).unwrap();
        assert_eq!(n, Norm::new(0.5, 0.5));
    }

    /// The letterboxing is beside the game, not on it. Clamping would send a
    /// touch to the screen edge, which is a real tap somewhere unintended.
    #[test]
    fn a_click_in_the_letterboxing_is_not_a_touch() {
        assert!(to_norm((1400.0, 10.0), (1280.0, 720.0)).is_none());
        assert!(to_norm((10.0, -5.0), (1280.0, 720.0)).is_none());
    }

    #[test]
    fn a_picture_with_no_size_yields_nothing() {
        assert!(to_norm((1.0, 1.0), (0.0, 0.0)).is_none());
    }

    /// The corners are on the game, so they must map rather than be refused.
    #[test]
    fn the_corners_are_inside() {
        assert_eq!(
            to_norm((0.0, 0.0), (100.0, 100.0)),
            Some(Norm::new(0.0, 0.0))
        );
        assert_eq!(
            to_norm((100.0, 100.0), (100.0, 100.0)),
            Some(Norm::new(1.0, 1.0))
        );
    }

    /// The pool hands out from 0 upward, so the mouse takes the last id.
    #[test]
    fn the_mouse_uses_the_id_the_pool_reaches_last() {
        assert_eq!(MOUSE_ID as usize, MAX_POINTERS - 1);
    }

    /// Without a pipe nothing is sent, and the reason is recorded rather than
    /// dropped — a click that goes nowhere in silence is the worst outcome.
    #[test]
    fn a_press_without_a_pipe_reports_why() {
        let mut t = Touch::default();
        t.press(Norm::new(0.5, 0.5));
        assert!(!t.down, "no finger can be down without a pipe");
        assert!(t.error.is_some());
    }

    #[test]
    fn dragging_without_a_press_does_nothing() {
        let mut t = Touch::default();
        t.drag(Norm::new(0.5, 0.5));
        assert!(!t.down);
    }

    #[test]
    fn releasing_what_was_never_pressed_is_harmless() {
        let mut t = Touch::default();
        t.release();
        assert!(!t.down);
    }

    #[test]
    fn nothing_is_ready_before_a_pipe_arrives() {
        assert!(!Touch::default().is_ready());
    }

    #[test]
    fn a_closed_pipe_is_replaced_without_waiting_for_a_click() {
        use std::io::Read;
        use std::os::fd::OwnedFd;

        let (reader, writer) = std::io::pipe().unwrap();
        let mut touch = Touch::default();
        touch.attach(std::fs::File::from(OwnedFd::from(writer)), 1920, 1080);
        assert!(touch.check_connection());
        touch.press(Norm::new(0.5, 0.5));
        assert!(touch.down);

        drop(reader);
        assert!(!touch.check_connection());
        assert!(!touch.down);

        let (mut reader, writer) = std::io::pipe().unwrap();
        touch.attach(std::fs::File::from(OwnedFd::from(writer)), 1920, 1080);
        touch.press(Norm::new(0.5, 0.5));
        touch.release();
        drop(touch);
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes.len(), (6 + 3) * 24);
    }
}
