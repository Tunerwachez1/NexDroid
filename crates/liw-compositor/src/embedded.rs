//! Running the compositor inside another application.
//!
//! The whole Wayland side — socket, event loop, EGL context, rendering —
//! lives on a thread of its own. The host application never touches any of
//! it: it reads the newest frame out of a slot and writes the size it wants
//! into another. Those two handles are the entire surface between them, and
//! keeping it that small is what stops the compositor from leaking into a UI
//! that has no business knowing what a `wl_surface` is.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::element::surface::{
    render_elements_from_surface_tree, WaylandSurfaceRenderElement,
};
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::renderer::utils::with_renderer_surface_state;
use smithay::desktop::utils::send_frames_surface_tree;
use smithay::output::{Mode, Output, PhysicalProperties, Scale, Subpixel};
use smithay::reexports::calloop::{EventLoop, channel};
use smithay::reexports::wayland_server::Display;
use smithay::wayland::socket::ListeningSocketSource;

use crate::headless::{Frame, FrameSlot, Headless, OFFSCREEN_TRANSFORM};
use crate::{ClientState, Compositor, GuestHandle};

/// Handles the host application holds onto.
pub struct Embedded {
    /// The newest frame, or `None` before the first one.
    pub frames: FrameSlot,
    /// What the guest is doing, for a status line.
    pub guest: GuestHandle,
    width: Arc<AtomicI32>,
    height: Arc<AtomicI32>,
    running: Arc<AtomicBool>,
    keyboard: channel::Sender<(u16, bool)>,
}

impl Embedded {
    /// Asks for a new render size. Takes effect on the next frame.
    ///
    /// This is what makes the picture follow the window instead of being
    /// fixed at start-up.
    pub fn request_size(&self, width: i32, height: i32) {
        self.width.store(width.max(1), Ordering::Relaxed);
        self.height.store(height.max(1), Ordering::Relaxed);
    }

    pub fn requested_size(&self) -> (i32, i32) {
        (self.width.load(Ordering::Relaxed), self.height.load(Ordering::Relaxed))
    }

    /// Forward an evdev key to the embedded Wayland seat.
    pub fn key(&self, code: u16, pressed: bool) {
        let _ = self.keyboard.send((code, pressed));
    }

    /// Stops the thread. The compositor's socket goes with it, so anything
    /// still connected is disconnected.
    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

/// Starts the compositor on its own thread.
///
/// Returns as soon as the socket is bound, so the caller can put
/// `WAYLAND_DISPLAY` in front of Waydroid without racing it.
pub fn spawn(socket_name: &str, width: i32, height: i32) -> Result<Embedded, String> {
    spawn_with_mode(socket_name,width,height,60_000)
}

pub fn spawn_with_mode(socket_name: &str, width: i32, height: i32, refresh_mhz: i32) -> Result<Embedded, String> {
    if width < 1 || height < 1 || !(1_000..=1_000_000).contains(&refresh_mhz) {
        return Err("Invalid native display mode".into());
    }
    let frames: FrameSlot = Arc::new(Mutex::new(None));
    let w = Arc::new(AtomicI32::new(width.max(1)));
    let h = Arc::new(AtomicI32::new(height.max(1)));
    let running = Arc::new(AtomicBool::new(true));

    // The socket is bound HERE rather than on the thread, so a name already
    // in use is reported to the caller instead of disappearing into a log it
    // may never read.
    let socket = ListeningSocketSource::with_name(socket_name)
        .map_err(|e| format!("could not bind the socket {socket_name}: {e}"))?;

    let (keyboard, key_events) = channel::channel();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<GuestHandle, String>>();

    let (tf, tw, th, trunning) = (frames.clone(), w.clone(), h.clone(), running.clone());
    std::thread::Builder::new()
        .name("liw-compositor".into())
        .spawn(move || {
            match run(socket, tf, tw, th, trunning.clone(), ready_tx.clone(), refresh_mhz, key_events) {
                Ok(()) => tracing::info!("compositor thread finished"),
                Err(e) => {
                    tracing::error!(error = %e, "compositor thread failed");
                    // If it died before signalling readiness the caller is
                    // still waiting; tell it rather than leaving it blocked.
                    let _ = ready_tx.send(Err(e));
                }
            }
            trunning.store(false,Ordering::Relaxed);
        })
        .map_err(|e| format!("could not start the compositor thread: {e}"))?;

    match ready_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(guest)) => Ok(Embedded { frames, guest, width: w, height: h, running, keyboard }),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("the compositor did not come up within ten seconds".into()),
    }
}

fn run(
    socket: ListeningSocketSource,
    frames: FrameSlot,
    want_w: Arc<AtomicI32>,
    want_h: Arc<AtomicI32>,
    running: Arc<AtomicBool>,
    ready: std::sync::mpsc::Sender<Result<GuestHandle, String>>,
    refresh_mhz: i32,
    key_events: channel::Channel<(u16, bool)>,
) -> Result<(), String> {
    let (w0, h0) = (want_w.load(Ordering::Relaxed), want_h.load(Ordering::Relaxed));
    let mut headless = Headless::new(w0, h0)?;

    let mut event_loop: EventLoop<Compositor> =
        EventLoop::try_new().map_err(|e| format!("event loop: {e}"))?;
    let display: Display<Compositor> =
        Display::new().map_err(|e| format!("wayland display: {e}"))?;
    let dh = display.handle();

    let mut state = Compositor::new(&dh, (w0, h0), refresh_mhz);
    let _ = ready.send(Ok(state.guest.clone()));

    let output = Output::new(
        "liw-embedded".into(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "liwinux".into(),
            model: "embedded".into(),
        },
    );
    output.change_current_state(
        Some(Mode { size: (w0, h0).into(), refresh: refresh_mhz }),
        Some(OFFSCREEN_TRANSFORM),
        Some(Scale::Fractional(1.0)),
        Some((0, 0).into()),
    );
    let mut damage = OutputDamageTracker::new((w0,h0), 1.0, OFFSCREEN_TRANSFORM);

    let handle = event_loop.handle();
    let start = Instant::now();
    handle.insert_source(key_events, move |event, _, state| {
        if let channel::Event::Msg((code, pressed)) = event {
            state.key(code, pressed, start.elapsed().as_millis() as u32);
        }
    }).map_err(|e| format!("keyboard source: {e}"))?;
    handle
        .insert_source(socket, move |client, _, state: &mut Compositor| {
            match state.dh.insert_client(client, Arc::new(ClientState::default())) {
                Ok(_) => tracing::info!("guest connected"),
                Err(e) => tracing::error!(error = %e, "could not insert the guest"),
            }
        })
        .map_err(|e| format!("socket source: {e}"))?;

    handle
        .insert_source(
            smithay::reexports::calloop::generic::Generic::new(
                display,
                smithay::reexports::calloop::Interest::READ,
                smithay::reexports::calloop::Mode::Level,
            ),
            |_, display, state: &mut Compositor| {
                // SAFETY: the display is owned by this source and touched
                // only here, on this thread.
                unsafe { display.get_mut().dispatch_clients(state)? };
                Ok(smithay::reexports::calloop::PostAction::Continue)
            },
        )
        .map_err(|e| format!("display source: {e}"))?;

    let mut drawn_commits = u64::MAX;
    let mut applied = (0i32, 0i32, f64::NAN);
    let mut stats = Stats::new();

    while running.load(Ordering::Relaxed) && state.running {
        event_loop
            .dispatch(Some(Duration::from_millis(4)), &mut state)
            .map_err(|e| format!("dispatch: {e}"))?;

        let size = (want_w.load(Ordering::Relaxed), want_h.load(Ordering::Relaxed));
        headless.resize(size.0, size.1)?;

        let (commits, guest_size) = state
            .guest
            .lock()
            .map(|g| (g.commits, g.size))
            .unwrap_or((0, None));

        // Keep the advertised native output independent of the render target.
        // Window resizing scales pixels locally without hotplugging Android.
        let (gw, gh) = guest_size.unwrap_or((w0,h0));
        let (fit, _shown) = crate::headless::fit((gw, gh), size);

        if (size.0, size.1) != (applied.0, applied.1) || (fit - applied.2).abs() > 1e-9 {
            // Smithay computes surface dimensions from the damage tracker's
            // scale too. Passing 1.0 here crops the native buffer to the
            // smaller host framebuffer even when tree locations use `fit`.
            damage = OutputDamageTracker::new(size, fit, OFFSCREEN_TRANSFORM);
            // Resizing replaces the framebuffer, including for a static app.
            drawn_commits = u64::MAX;
            applied = (size.0, size.1, fit);
            tracing::info!(guest = ?(gw, gh), view = ?size, fit, "fitted");
        }

        let Some(surface) = state.surface.clone() else {
            state.dh.flush_clients().ok();
            continue;
        };
        // Nothing new to draw. Repainting an unchanged frame costs a full GPU
        // pass and a readback for a picture nobody asked for again.
        if commits == drawn_commits {
            state.dh.flush_clients().ok();
            continue;
        }
        drawn_commits = commits;

        let painted = Instant::now();
        let result = headless.capture(|renderer, fb| {
            let elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> =
                render_elements_from_surface_tree(
                    renderer,
                    &surface,
                    (0, 0),
                    fit,
                    1.0,
                    Kind::Unspecified,
                );
            // A new or switching Android window can commit before it has a
            // buffer. Keep the previous picture until a frame is drawable.
            if elements.is_empty() {
                return Ok(false);
            }
            damage
                .render_output(renderer, fb, 0, &elements, [0.02, 0.02, 0.03, 1.0])
                .map(|_| true)
                .map_err(|e| format!("render: {e}"))
        });

        match result {
            Ok(Some(frame)) => {
                if let Some(line) = stats.record(painted.elapsed()) {
                    tracing::info!(
                        fps = line.fps,
                        frame_ms = line.mean_ms,
                        size = ?headless.size(),
                        "embedded frame rate",
                    );
                }
                publish(&frames, frame);
            }
            Ok(None) => {
                let has_buffer = with_renderer_surface_state(&surface, |state| state.buffer().is_some());
                if has_buffer == Some(true) {
                    tracing::warn!(commits, "Android buffer produced no render elements");
                } else {
                    tracing::debug!(commits, "waiting for an Android surface buffer");
                }
            }
            Err(e) => tracing::warn!(error = %e, "frame dropped"),
        }

        send_frames_surface_tree(&surface, &output, start.elapsed(), None, |_, _| {
            Some(output.clone())
        });
        state.dh.flush_clients().ok();
    }
    Ok(())
}

/// Leaves a frame where the host will find it.
///
/// The lock is held for the swap alone. Holding it across the render would
/// stall the UI thread on the compositor, which is exactly what putting them
/// on separate threads was meant to avoid.
fn publish(slot: &FrameSlot, frame: Frame) {
    if let Ok(mut s) = slot.lock() {
        *s = Some(frame);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_requested_size_is_read_back() {
        let e = Embedded {
            frames: Arc::new(Mutex::new(None)),
            guest: Arc::new(Mutex::new(Default::default())),
            width: Arc::new(AtomicI32::new(1)),
            height: Arc::new(AtomicI32::new(1)),
            running: Arc::new(AtomicBool::new(true)),
            keyboard: channel::channel().0,
        };
        e.request_size(1600, 900);
        assert_eq!(e.requested_size(), (1600, 900));
    }

    /// A zero or negative size would make an invalid render target. The host
    /// can legitimately report one while a window is being laid out.
    #[test]
    fn a_size_is_never_allowed_to_reach_zero() {
        let e = Embedded {
            frames: Arc::new(Mutex::new(None)),
            guest: Arc::new(Mutex::new(Default::default())),
            width: Arc::new(AtomicI32::new(1)),
            height: Arc::new(AtomicI32::new(1)),
            running: Arc::new(AtomicBool::new(true)),
            keyboard: channel::channel().0,
        };
        e.request_size(0, -5);
        assert_eq!(e.requested_size(), (1, 1));
    }

    #[test]
    fn stopping_is_visible_to_the_thread() {
        let e = Embedded {
            frames: Arc::new(Mutex::new(None)),
            guest: Arc::new(Mutex::new(Default::default())),
            width: Arc::new(AtomicI32::new(1)),
            height: Arc::new(AtomicI32::new(1)),
            running: Arc::new(AtomicBool::new(true)),
            keyboard: channel::channel().0,
        };
        assert!(e.is_running());
        e.stop();
        assert!(!e.is_running());
    }

    /// Publishing replaces rather than queues: the host wants the current
    /// picture, not a backlog.
    #[test]
    fn publishing_replaces_the_previous_frame() {
        let slot: FrameSlot = Arc::new(Mutex::new(None));
        publish(&slot, Frame { width: 1, height: 1, bgra: vec![0; 4], serial: 1 });
        publish(&slot, Frame { width: 2, height: 2, bgra: vec![0; 16], serial: 2 });
        let g = slot.lock().unwrap();
        assert_eq!(g.as_ref().unwrap().serial, 2);
        assert_eq!(g.as_ref().unwrap().width, 2);
    }

    /// The placeholder guard is what keeps a 1x1 surface from producing an
    /// absurd output scale. Android stopped booting when it did.
    #[test]
    fn a_placeholder_surface_does_not_drive_the_scale() {
        assert_eq!(crate::headless::fit((1, 1), (1280, 720)).0, 1.0,
                   "1x1 must not scale");
        assert_eq!(crate::headless::fit((2560, 1440), (1280, 720)).0, 0.5,
                   "a real screen must");
    }
}

/// What one report says.
pub struct Report {
    pub fps: f32,
    pub mean_ms: f32,
}

/// Frame rate and cost, reported once a second.
///
/// Here because "it feels slower" cannot be answered by reading code. The
/// number that matters is not how fast this loop could spin — it only draws
/// when the guest commits — but how much of each frame is spent getting the
/// picture out of the GPU and into a buffer the host can draw.
struct Stats {
    frames: u32,
    total: Duration,
    since: Instant,
}

impl Stats {
    fn new() -> Self {
        Self { frames: 0, total: Duration::ZERO, since: Instant::now() }
    }

    /// Adds a frame; returns a report roughly once a second.
    fn record(&mut self, cost: Duration) -> Option<Report> {
        self.frames += 1;
        self.total += cost;
        let elapsed = self.since.elapsed();
        if elapsed < Duration::from_secs(1) {
            return None;
        }
        let report = Report {
            fps: self.frames as f32 / elapsed.as_secs_f32(),
            mean_ms: self.total.as_secs_f32() * 1000.0 / self.frames as f32,
        };
        self.frames = 0;
        self.total = Duration::ZERO;
        self.since = Instant::now();
        Some(report)
    }
}

#[cfg(test)]
mod stat_tests {
    use super::*;

    /// Nothing is reported before a second has passed: a report per frame
    /// would cost more than the thing it measures.
    #[test]
    fn a_report_waits_for_a_second() {
        let mut s = Stats::new();
        assert!(s.record(Duration::from_millis(5)).is_none());
    }

    #[test]
    fn the_mean_is_over_the_frames_in_the_window() {
        let mut s = Stats::new();
        s.record(Duration::from_millis(10));
        s.record(Duration::from_millis(20));
        // Pretend a second went by.
        s.since = Instant::now() - Duration::from_secs(1);
        let r = s.record(Duration::from_millis(30)).expect("a report is due");
        assert!((r.mean_ms - 20.0).abs() < 0.1, "mean was {}", r.mean_ms);
        assert!((r.fps - 3.0).abs() < 0.1, "fps was {}", r.fps);
    }

    /// The window resets, or a slow second would haunt every later report.
    #[test]
    fn a_report_starts_a_fresh_window() {
        let mut s = Stats::new();
        s.since = Instant::now() - Duration::from_secs(1);
        s.record(Duration::from_millis(50)).unwrap();
        assert_eq!(s.frames, 0);
        assert_eq!(s.total, Duration::ZERO);
    }
}
