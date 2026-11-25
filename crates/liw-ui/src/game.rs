//! The game window.
//!
//! A window of its own, the way GameLoop does it: the launcher stays a
//! launcher and the game gets its own frame, its own size and its own place on
//! the taskbar. Putting the game inside the launcher meant one of them always
//! had to be the wrong shape.
//!
//! It holds everything that belongs to a running game — the compositor
//! Waydroid draws into, the rail beside the picture, and the control editor —
//! and nothing that does not.

use gpui::{
    div, prelude::*, px, App, Context, FocusHandle, IntoElement, Render,
    SharedString, Window,
};

use liw_input::engine::{InputEvent, TriggerKind};
use liw_input::profile::Trigger;
use liw_input::store::Store;
use std::collections::BTreeMap;

use crate::keyboard::{KeyEvent, Keyboard};

use crate::android::Android;
use crate::mapper::{Control, Kind, Mapper, NUDGE};
use crate::pointer_lock::{CaptureEvent, PointerLock};
use crate::theme::{Theme, RADIUS, S2, S3};

/// Width of the strip that is always there.
pub const RAIL_W: f32 = 44.0;
/// Width of the panel when it is open.
pub const PANEL_W: f32 = 300.0;

/// How much horizontal room the sidebar takes right now.
///
/// The picture's size is computed from this, so it lives in one place: a
/// second opinion about the rail's width would put every control a few pixels
/// from where it was clicked.
pub fn sidebar_width(open: bool) -> f32 {
    if open { RAIL_W + PANEL_W } else { RAIL_W }
}

fn window_size(mode: liw_core::display::GuestDisplay, screen: gpui::Size<gpui::Pixels>) -> gpui::Size<gpui::Pixels> {
    let ratio = mode.width as f32 / mode.height as f32;
    let height = (f32::from(screen.height) - 96.0).clamp(1.0, 720.0)
        .min((f32::from(screen.width) - 48.0 - RAIL_W).max(1.0) / ratio);
    gpui::size(px((height * ratio).round() + RAIL_W), px(height.round().max(1.0)))
}

#[derive(Clone, Copy)]
enum KeyRoute {
    Android,
    Binding,
    Shortcut,
}

pub struct GameView {
    settings: Option<gpui::Entity<crate::performance::Performance>>,
    settings_open: bool,
    pub android: Android,
    pub mapper: Mapper,
    pub sidebar_open: bool,
    drag_offset: Option<(f32, f32)>,
    pointer_lock: Option<PointerLock>,
    keyboard: Option<Keyboard>,
    held_keys: BTreeMap<u16, KeyRoute>,
    text_input: bool,
    editor_focus: Option<crate::editor_focus::Monitor>,
    editor_event: Option<std::sync::Arc<crate::editor_focus::Event>>,
    text_override: Option<bool>,
    auto_text: bool,
    resume_capture: bool,
    capture_message: Option<String>,
    shown: (f32, f32),
    /// The game window takes key events for the editor: arrows nudge, other
    /// keys bind. It has to hold focus for any of that to arrive.
    pub focus: FocusHandle,
    /// The package chosen from the Linux library, for launch requests.
    pub package: String,
    pub title: String,
    /// The game's own icon, drawn in the panel.
    ///
    /// Wayland has no per-window icon, so this is where the game's face
    /// actually appears — inside the window rather than on its frame.
    pub icon: Option<std::path::PathBuf>,
    /// Mouse presses, sent into Android as touches.
    pub touch: crate::touch::Touch,
    frame_pump: Option<gpui::Task<()>>,
    input_pump: Option<gpui::Task<()>>,
    _pipe: Option<gpui::Task<()>>,
    foreground: Option<String>,
    edit_requested: bool,
    /// The title has been put on the window.
    ///
    /// `TitlebarOptions.title` is not applied on Wayland in this gpui
    /// revision, so the name has to be set explicitly — otherwise the task
    /// switcher shows whatever the toolkit defaulted to.
    titled: bool,
    android_ready: bool,
    boot_in_flight: bool,
    display_owner: String,
    launches: crate::instance::LaunchGate,
    performance: crate::performance::Preferences,
    shortcuts:crate::shortcuts::Shortcuts,
    shortcut_error:String,
    preferences_checked: std::time::Instant,
}

impl GameView {
    pub fn new(
        package: String,
        title: String,
        icon: Option<std::path::PathBuf>,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut v = Self {
            settings:None,
            settings_open:false,
            android: Android::default(),
            mapper: Mapper::default(),
            sidebar_open: false,
            drag_offset: None,
            pointer_lock: None,
            keyboard: Keyboard::new(window).map_err(|error| {
                tracing::warn!(%error, "physical keyboard unavailable, using window key events");
            }).ok(),
            held_keys: BTreeMap::new(),
            text_input: false,
            editor_focus: crate::editor_focus::Monitor::start().map_err(|e| tracing::warn!(%e,"IME focus bridge unavailable; F8 remains available")).ok(),
            editor_event: None,
            text_override: None,
            auto_text: true,
            resume_capture: false,
            capture_message: None,
            shown: (0.0, 0.0),
            focus: cx.focus_handle(),
            package,
            title,
            icon,
            touch: Default::default(),
            frame_pump: None,
            input_pump: None,
            _pipe: None,
            foreground: None,
            edit_requested: false,
            titled: false,
            android_ready: false,
            boot_in_flight: false,
            display_owner: format!("{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()),
            launches: Default::default(),
            performance: crate::performance::Preferences::load(),
            shortcuts:crate::shortcuts::Shortcuts::load().unwrap_or_default(),
            shortcut_error:String::new(),
            preferences_checked: std::time::Instant::now(),
        };
        window.focus(&v.focus,cx);
        if v.performance.fullscreen_start && !window.is_fullscreen() { v.toggle_fullscreen(window,cx); }
        match liw_core::display::GuestDisplay::load() {
            Ok(mode) => v.android.start(mode),
            Err(error) => v.android.error = Some(error),
        }
        v.pump(window, cx);
        v.pump_input(window, cx);
        if v.android.running() {
            v.open_touch_pipe(cx);
            v.watch_foreground(cx);
            v.bring_up_android(cx);
            v.stop_android_on_close(cx);
        }
        v.report_focus(window, cx);
        v.watch_audio(cx);
        // Read-only QA of the frame delivered to GPUI; absent in normal runs.
        if let Some(path) = std::env::var_os("LIW_CAPTURE_PATH") {
            cx.spawn(async move |view,cx| {
                cx.background_executor().timer(std::time::Duration::from_secs(35)).await;
                let _ = view.update(cx,|view,_| {
                    match view.android.save_frame(std::path::Path::new(&path)) {
                        Ok(()) => tracing::info!(?path,"Android view frame saved"),
                        Err(error) => tracing::warn!(%error,"Android view capture failed"),
                    }
                });
            }).detach();
        }
        v
    }

    /// Tells the daemon when this window has the focus.
    ///
    /// The key mapper only maps while Android's screen is focused, and it
    /// learns that from the focused window's class. A KWin script reports
    /// those, which leaves two holes this closes:
    ///
    /// * The script samples focus when it LOADS and on changes after that.
    ///   Measured: this window already had the focus when the mapper started,
    ///   nothing changed afterwards, so nothing was ever reported. Game mode
    ///   turned on, said so, and mapped nothing.
    /// * The script is KWin's. On any other compositor there is no report at
    ///   all.
    ///
    /// This window knows the answer without asking anyone, so it says so —
    /// once at once, and on every change. It reports the same class KWin
    /// would, so the two agree rather than fight.
    fn report_focus(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        report(window.is_window_active(), cx);
        cx.observe_window_activation(window, |v, window, cx| {
            let active = window.is_window_active();
            if !active {
                v.resume_capture = false;
                v.release_mouse();
                v.release_keys();
                v.touch.release_all();
                v.drag_offset = None;
                cx.notify();
            }
            report(active && !v.settings_open, cx);
        })
        .detach();
    }

    /// Closing the window closes the game.
    ///
    /// The whole session goes, not just the package. Two reasons, and the
    /// second is the one that decides it:
    ///
    /// * `waydroid app` has no stop verb. Killing one package means
    ///   `am force-stop` through a root shell, which would mean a new
    ///   privileged method and a new polkit action for something the user can
    ///   already do by closing a window.
    /// * This window WAS Android's screen. With it gone the session is
    ///   drawing into a socket nobody is listening on — the same state the
    ///   embedded display is checked for before every start, because Android
    ///   boots happily into nothing and looks fine while doing it.
    ///
    /// It costs nothing either: the next game window would have had to stop
    /// and restart this session anyway to move it onto its own socket.
    fn stop_android_on_close(&mut self, cx: &mut Context<Self>) {
        cx.on_release(|v, cx| {
            // If the stop below fails — the daemon is gone, the call times out
            // — the session outlives the window. A finger left down in it is
            // held forever with nothing able to lift it.
            v.release_mouse();
            v.release_keys();
            v.touch.release_all();
            // The compositor thread owns the listening socket, and it outlives
            // the window unless it is told not to. Measured: after closing one
            // game the socket file was still bound, so the next game window
            // could not take the name and never got a picture. Stopping is a
            // flag the thread reads every 4ms, so this returns at once.
            if let Some(e) = v.android.embedded.take() {
                e.stop();
            }
            let owner=v.display_owner.clone();
            let task = gpui_tokio::Tokio::spawn(cx, async move {
                let Ok(m) = liw_core::manager::Manager::connect().await else { return };
                // A delayed close callback must not stop a replacement window.
                let _ = m.detach_embedded_display(&owner).await;
                tracing::info!("game window closed — Android stopped");
            });
            task.detach();
        })
        .detach();
    }

    /// Points the daemon at this window's socket and starts Android on it.
    ///
    /// The order is the whole trick: the socket exists as soon as the window
    /// does, the daemon is told about it, and only then is the session
    /// started. Starting first would send Android to the desktop compositor,
    /// and telling the daemon afterwards would not move a session already
    /// running.
    ///
    /// Telling the DAEMON rather than setting an environment variable is what
    /// makes it survive: liwd restarts the session when it looks unhealthy,
    /// and a restart that forgot the socket would take the game out of this
    /// window without explanation.
    fn bring_up_android(&mut self, cx: &mut Context<Self>) {
        if self.boot_in_flight { return; }
        self.boot_in_flight=true;
        self.android_ready=false;
        let previous_frame=self.android.image.as_ref().map(|(serial,_)|*serial);
        let owner=self.display_owner.clone();
        let task = gpui_tokio::Tokio::spawn(cx, async move {
            let m = liw_core::manager::Manager::connect().await
                .map_err(|e| e.to_string())?;
            let camera_warning = match liw_core::devices::DeviceSettings::load() {
                Ok(settings) => match liw_core::helper::HelperClient::connect().await {
                    Ok(helper) => helper.set_camera(&settings.camera).await.err().map(|e|e.to_string()),
                    Err(e) => Some(e.to_string()),
                },
                Err(e) => Some(e),
            };
            m.attach_embedded_display(crate::android::SOCKET,&owner).await
                .map_err(|e|e.to_string())?;
            tracing::debug!("bring-up: session started");

            // Wait for Android before launching. This is the ordering that
            // was wrong: the launcher used to fire the launch at the same
            // time as the window opened, so the app went into whatever
            // session was already running — which is the OLD one, on the
            // desktop's socket. The game then appeared in a window of
            // Waydroid's own while this one stayed empty.
            let mut booted = false;
            let helper=liw_core::HelperClient::connect().await.map_err(|e|e.to_string())?;
            for i in 0..90 {
                tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
                match helper.boot_completed().await {
                    Ok(true) => { booted = true; break; }
                    Ok(false) if i % 10 == 0 => tracing::debug!("bring-up: waiting for Android"),
                    Err(e) if i % 10 == 0 => tracing::warn!(
                        error = %e, "bring-up: cannot read the daemon"),
                    _ => {}
                }
            }
            if !booted {
                return Err("Android did not finish booting".to_string());
            }
            let display = crate::display_settings::DisplaySettings::load().map_err(|e| e.to_string())?;
            helper.set_display_density(display.dpi).await.map_err(|e| e.to_string())?;
            let debug_warning=helper.restore_advanced_settings().await.err().map(|e|format!("Debug: {e}"));
            let warnings=camera_warning.map(|e|format!("Camera: {e}")).into_iter().chain(debug_warning).collect::<Vec<_>>();
            Ok::<Option<String>,String>((!warnings.is_empty()).then(||warnings.join(" · ")))
        });
        cx.spawn(async move |this, cx| {
            let (r,camera_warning) = match task.await {
                Ok(Ok(warning)) => (None,warning),
                Ok(Err(e)) => (Some(e),None),
                Err(e) => (Some(e.to_string()),None),
            };
            if let Some(e) = r {
                let _ = this.update(cx, |v, cx| {
                    v.boot_in_flight=false;
                    v.android.error = Some(format!("could not start Android: {e}"));
                    cx.notify();
                });
            } else {
                // The launcher/app creates the first useful Android surface.
                // Waiting for that surface before launching it is a deadlock.
                let Ok(package)=this.update(cx,|view,_|view.package.clone()) else {return;};
                let launched=package.clone();
                let launch=gpui_tokio::Tokio::spawn(cx,async move {
                    let manager=liw_core::Manager::connect().await.map_err(|e|e.to_string())?;
                    manager.launch(&package).await.map_err(|e|e.to_string())
                }).await;
                if !matches!(launch,Ok(Ok(()))) {
                    let _=this.update(cx,|view,cx| {
                        view.boot_in_flight=false;
                        view.android.error=Some(format!("Не удалось открыть приложение Android: {launch:?}"));
                        cx.notify();
                    });
                    return;
                }
                // Boot completion alone does not prove Android reached THIS
                // compositor. Do not report a ready view with no surface.
                let mut connected=false;
                for _ in 0..120 {
                    match this.update(cx,|v,_|v.android.image.as_ref().map(|(serial,_)|*serial).is_some_and(|serial|Some(serial)!=previous_frame)
                        && v.android.guest_size().is_some() && v.touch.is_ready()) {
                        Ok(true)=>{connected=true;break;},Ok(false)=>{},Err(_)=>return,
                    }
                    cx.background_executor().timer(std::time::Duration::from_millis(250)).await;
                }
                let _=this.update(cx,|view,cx| {
                    view.boot_in_flight=false;
                    if !connected {
                        view.android.error=Some("Android загрузился, но не подключил изображение или ввод к этому окну. Нажмите «Переподключить».".into());
                        cx.notify();return;
                    }
                    view.android_ready=true;
                    tracing::info!(owner=%view.display_owner,"Android view ready: new frame and input connected");
                    if let Some(warning)=camera_warning { view.capture_message=Some(warning); }
                    view.restore_resource_limits(cx);
                    if view.package!=launched {view.launch_selected(cx);}
                });
            }
        })
        .detach();
    }

    fn launch_selected(&mut self,cx: &mut Context<Self>) {
        if !self.android_ready {
            self.bring_up_android(cx);
            return;
        }
        if !self.launches.begin(&self.package) { return; }
        let package=self.package.clone();
        let task=gpui_tokio::Tokio::spawn(cx,async move {
            let manager=liw_core::manager::Manager::connect().await.map_err(|e|e.to_string())?;
            if !manager.live_status().await.map_err(|e|e.to_string())?.session_running() {
                return Ok(false);
            }
            manager.launch(&package).await.map_err(|e|e.to_string())?;
            tracing::info!(%package,"game launched in existing Android window");
            Ok::<bool,String>(true)
        });
        cx.spawn(async move |view,cx| {
            let result=task.await.map_err(|error|error.to_string()).and_then(|value|value);
            let _=view.update(cx,|view,cx| {
                let newer=view.launches.finish(&view.package);
                match result {
                    Ok(false) => { view.android_ready=false; view.bring_up_android(cx); },
                    Err(error) if !newer => { view.android.error=Some(error); cx.notify(); },
                    _ if newer => view.launch_selected(cx),
                    _ => {},
                }
            });
        }).detach();
    }

    fn restore_resource_limits(&mut self, cx: &mut Context<Self>) {
        if !crate::performance::directory().join("performance.json").exists() { return; }
        let preferences = crate::performance::Preferences::load();
        cx.spawn(async move |view, cx| {
            let result = gpui_tokio::Tokio::spawn(cx, async move {
                let helper = liw_core::helper::HelperClient::connect().await.map_err(|e| e.to_string())?;
                helper.set_resources(preferences.cpus, preferences.memory_mib).await.map_err(|e| e.to_string())
            }).await;
            if !matches!(result, Ok(Ok(()))) {
                let _ = view.update(cx, |v, cx| {
                    v.capture_message = Some(format!("CPU/RAM: {result:?}"));
                    cx.notify();
                });
            }
        }).detach();
    }

    fn watch_audio(&mut self,cx: &mut Context<Self>) {
        cx.spawn(async move |view,cx| {
            let mut previous=None;
            let mut previous_error=String::new();
            loop {
                cx.background_executor().timer(std::time::Duration::from_secs(2)).await;
                match view.update(cx,|v,_|v.android_ready) { Ok(true)=>{},Ok(false)=>continue,Err(_)=>break }
                let last=previous.clone();
                let result=gpui_tokio::Tokio::spawn(cx,async move {
                    if !liw_core::devices::DeviceSettings::path().exists() { return Ok(None); }
                    let settings=liw_core::devices::DeviceSettings::load()?;
                    if settings.audio_sink.is_none() && settings.audio_source.is_none() && last.as_ref()==Some(&settings) { return Ok(Some(settings)); }
                    liw_core::devices::route(&settings).await?;
                    Ok::<_,String>(Some(settings))
                }).await;
                match result {
                    Ok(Ok(settings))=>{
                        previous=settings;
                        if !previous_error.is_empty() {
                            let recovered=std::mem::take(&mut previous_error);
                            let _=view.update(cx,|v,cx| {
                                if v.capture_message.as_ref()==Some(&recovered) { v.capture_message=None;cx.notify(); }
                            });
                        }
                    },
                    result=>{
                        let error=format!("Audio routing: {result:?}");
                        if error!=previous_error {
                            previous_error=error.clone();
                            let _=view.update(cx,|v,cx|{v.capture_message=Some(error);cx.notify();});
                        }
                    }
                }
            }
        }).detach();
    }

    /// Asks the daemon's helper for the touch pipe.
    ///
    /// Opening it needs root — the FIFO lives inside the container's mount
    /// namespace — so the helper opens it and passes the descriptor over
    /// D-Bus. Retried until the session is up, because the window is normally
    /// open before Android has finished booting.
    fn open_touch_pipe(&mut self, cx: &mut Context<Self>) {
        self._pipe = Some(cx.spawn(async move |this, cx| {
            let check_interval = std::time::Duration::from_millis(250);
            let retry_interval = std::time::Duration::from_secs(2);
            let mut delay = check_interval;
            loop {
                cx.background_executor().timer(delay).await;

                match this.update(cx, |v, cx| {
                    let was_ready = v.touch.is_ready();
                    let ready = v.touch.check_connection();
                    if was_ready && !ready {
                        cx.notify();
                    }
                    ready
                }) {
                    Ok(true) => { delay = check_interval; continue; }
                    Ok(false) => { delay = retry_interval; }
                    Err(_) => return, // the window is gone
                }

                let task = gpui_tokio::Tokio::spawn(cx, async move {
                    let h = liw_core::helper::HelperClient::connect().await.ok()?;
                    if !h.boot_completed().await.ok()? { return None; }
                    h.open_touch_pipe().await.ok()
                });
                let Ok(Some((pipe, w, h))) = task.await else { continue };
                match this.update(cx, |v, cx| {
                    // boot_completed can precede HWC's final mode update.
                    // Wait for the real surface and pipe axes to agree.
                    if v.android.guest_size() == Some((w as i32,h as i32)) {
                        v.touch.attach(pipe, w, h);
                        cx.notify();
                    }
                    v.touch.is_ready()
                }) {
                    Ok(true) => delay = check_interval,
                    Ok(false) => {}
                    Err(_) => return,
                }
            }
        }));
    }

    /// Repaints while frames are arriving.
    ///
    /// gpui redraws when something tells it to, and a frame produced on the
    /// compositor thread is not something it can see. This is that signal.
    fn pump(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.frame_pump.is_some() {
            return;
        }
        self.frame_pump = Some(cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor().timer(POLL).await;
            let keep = this.update_in(cx, |v, window, cx| {
                if !v.android.running() {
                    v.release_mouse();
                    v.touch.release_all();
                    v.frame_pump = None;
                    return false;
                }
                if v.preferences_checked.elapsed() >= std::time::Duration::from_secs(1) {
                    v.performance = crate::performance::Preferences::load();
                    v.reload_shortcuts();
                    v.preferences_checked = std::time::Instant::now();
                }
                if v.android.poll() {
                    cx.notify();
                }
                true
            });
            if !matches!(keep, Ok(true)) {
                break;
            }
        }));
    }

    /// Input cadence is independent from frame acquisition. Both tasks remain
    /// on GPUI's owning thread: no second Wayland display reader or input race.
    fn pump_input(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.input_pump.is_some() {return;}
        self.input_pump=Some(cx.spawn_in(window,async move |this,cx| {
            let mut delay=POLL;
            loop {
                cx.background_executor().timer(delay).await;
                let started=std::time::Instant::now();
                let keep=this.update_in(cx,|v,window,cx| {
                    if !v.android.running() {
                        v.release_mouse();v.release_keys();v.touch.release_all();
                        v.input_pump=None;
                        return None;
                    }
                    v.sync_editor_focus(window,cx);
                    v.poll_mouse(cx);
                    v.poll_keyboard(window,cx);
                    v.touch.tick();
                    Some(if v.pointer_lock.as_ref().is_some_and(PointerLock::requested) {
                        std::time::Duration::from_millis(1)
                    } else {POLL})
                });
                let Ok(Some(interval))=keep else {break};
                // Account for processing time, but never spin/catch up with a
                // burst when the desktop delayed us. Keep a yielding timer.
                delay=interval.saturating_sub(started.elapsed())
                    .max(std::time::Duration::from_micros(250));
            }
        }));
    }

    fn watch_foreground(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |view, cx| {
            let mut helper = None;
            loop {
                cx.background_executor().timer(std::time::Duration::from_millis(300)).await;
                match view.update(cx, |view, _| view.android.guest_size().is_some()) {
                    Err(_) => break,
                    Ok(false) => continue,
                    Ok(true) => {}
                }
                let previous: Option<liw_core::HelperClient> = helper.take();
                let request = gpui_tokio::Tokio::spawn(cx, async move {
                    let helper = match previous {
                        Some(helper) => helper,
                        None => liw_core::HelperClient::connect().await?,
                    };
                    let package = helper.foreground_package().await?;
                    Ok::<_, liw_core::helper::HelperError>((helper, package))
                });
                let package = match request.await {
                    Ok(Ok((client, package))) => {
                        helper = Some(client);
                        Some(package).filter(|package| control_target(package))
                    }
                    _ => None,
                };
                if view.update(cx, |view, cx| view.update_foreground(package, cx)).is_err() {
                    break;
                }
            }
        }).detach();
    }

    fn update_foreground(&mut self, package: Option<String>, cx: &mut Context<Self>) {
        if self.foreground != package {
            self.resume_capture = false;
            self.text_override = None;
            self.release_mouse();
            self.release_keys();
            self.touch.release_all();
            self.drag_offset = None;
            self.foreground = package;
            self.load_controls();
            tracing::info!(package = ?self.foreground, "foreground controls changed");
            cx.notify();
        }
        if std::mem::take(&mut self.edit_requested) {
            if let Some(package) = self.foreground.as_deref() {
                let name = liw_core::apps::discover().into_iter()
                    .find(|app| app.package == package).map(|app| app.name);
                self.mapper.begin(package, name.as_deref().unwrap_or(package));
                self.sidebar_open = true;
            } else {
                self.capture_message = Some("Откройте игру, чтобы настроить управление.".into());
            }
            cx.notify();
        }
    }

    fn editing_foreground(&self) -> bool {
        self.mapper.profile.as_ref().is_some_and(|profile| {
            self.foreground.as_deref() == Some(profile.package.as_str())
        })
    }

    fn load_controls(&mut self) {
        let store = Store::discover();
        let profile = self.foreground.as_deref()
            .and_then(|package| store.for_package(package))
            .map(|entry| entry.profile.clone());
        self.auto_text = profile.as_ref().map(|p|p.auto_text).unwrap_or(true);
        self.touch.load_profile(profile);
    }

    fn release_mouse(&mut self) {
        if let Some(lock) = self.pointer_lock.as_mut().filter(|lock| lock.requested()) {
            lock.release();
            self.touch.release_aim();
            self.touch.button(Trigger::MouseLeft, false);
        }
        self.capture_message = None;
    }

    fn screenshot(&mut self,cx: &mut Context<Self>) {
        let result=(|| -> Result<std::path::PathBuf,String> {
            let dir=self.performance.screenshot_directory();
            std::fs::create_dir_all(&dir).map_err(|e|e.to_string())?;
            let stamp=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e|e.to_string())?.as_nanos();
            let path=dir.join(format!("NexDroid-{stamp}.png"));
            self.android.save_frame(&path)?;
            Ok(path)
        })();
        let label=if self.performance.language=="ru" {"Снимок экрана"} else {"Screenshot"};
        self.capture_message=Some(match result {Ok(path)=>format!("{label}: {}",path.display()),Err(e)=>format!("{label}: {e}")});
        cx.notify();
    }

    fn mouse_captured(&self) -> bool {
        self.pointer_lock.as_ref().is_some_and(PointerLock::active)
    }

    fn toggle_mouse(&mut self, window: &Window) {
        if self.pointer_lock.as_ref().is_some_and(PointerLock::requested) {
            self.release_mouse();
            return;
        }
        let point = window.mouse_position();
        if crate::touch::to_norm((f32::from(point.x), f32::from(point.y)), self.shown).is_none() {
            self.capture_message = Some("Наведите курсор на игру и нажмите клавишу захвата.".into());
            return;
        }
        if !self.touch.is_ready() {
            self.capture_message = Some("Дождитесь подключения ввода Android.".into());
            return;
        }
        let result = (|| {
            if self.pointer_lock.is_none() {
                self.pointer_lock = Some(PointerLock::new(window)?);
            }
            self.pointer_lock.as_mut().unwrap().request(window)
        })();
        match result {
            Ok(()) => {
                self.touch.release();
                self.capture_message = Some("Ожидание захвата мыши…".into());
            }
            Err(error) => {
                self.release_mouse();
                self.capture_message = Some(error.to_string());
            }
        }
    }

    fn poll_mouse(&mut self, cx: &mut Context<Self>) {
        let Some(lock) = self.pointer_lock.as_mut() else { return };
        let event=lock.poll();
        if matches!(event,Ok(CaptureEvent::Captured(..))) {self.touch.begin_capture();}
        match event {
            Ok(CaptureEvent::Idle) => {}
            Ok(CaptureEvent::Motion(dx, dy) | CaptureEvent::Captured(dx,dy)) => {
                if self.capture_message.take().is_some() {
                    cx.notify();
                }
                if (dx, dy) != (0.0, 0.0) {
                    self.touch.input(InputEvent::MouseMove { dx, dy });
                    // Commit camera motion before key/button frames can report
                    // another pointer change at an older camera position.
                    self.touch.tick();
                }
            }
            Ok(CaptureEvent::Released) => {
                self.touch.release_aim();
                self.touch.button(Trigger::MouseLeft, false);
                self.capture_message = None;
                cx.notify();
            }
            Ok(CaptureEvent::TimedOut) => {
                self.capture_message = Some("Не удалось захватить мышь. Наведите курсор на игру и попробуйте снова.".into());
                cx.notify();
            }
            Err(error) => {
                self.release_mouse();
                self.pointer_lock = None;
                self.capture_message = Some(error.to_string());
                cx.notify();
            }
        }
    }

    fn close_editor(&mut self, cx: &mut Context<Self>) {
        self.mapper.end();
        self.drag_offset = None;
        self.load_controls();
        cx.notify();
    }

    fn toggle_fullscreen(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.release_mouse();
        self.release_keys();
        self.touch.release_all();
        self.drag_offset = None;
        window.toggle_fullscreen();
        cx.notify();
    }

    fn toggle_mapping(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.release_mouse();
        self.release_keys();
        self.touch.release_all();
        if window.is_fullscreen() {
            window.toggle_fullscreen();
        }
        if self.mapper.is_on() {
            self.close_editor(cx);
        } else {
            self.edit_requested = !self.edit_requested;
            if self.edit_requested && !self.android.running() {
                self.edit_requested = false;
                self.capture_message = Some("Android пока не подключён.".into());
            }
            cx.notify();
        }
    }

    fn release_keys(&mut self) {
        for (&code, route) in &mut self.held_keys {
            if matches!(route, KeyRoute::Android) {
                if let Some(embedded) = &self.android.embedded {
                    embedded.key(code, false);
                }
            }
            // Keep the physical press until its release so switching modes
            // cannot turn an already held key into a new press.
            *route = KeyRoute::Shortcut;
        }
    }

    fn toggle_text_input(&mut self, cx: &mut Context<Self>) {
        self.release_mouse();
        self.release_keys();
        self.touch.release_all();
        self.text_input = !self.text_input;
        self.text_override = Some(self.text_input);
        self.resume_capture = false;
        cx.notify();
    }

    fn sync_editor_focus(&mut self, window: &Window, cx: &mut Context<Self>) {
        let event = self.editor_focus.as_ref().and_then(|m|m.snapshot());
        if event != self.editor_event { self.text_override = None; self.editor_event = event.clone(); }
        let wanted = crate::editor_focus::effective(event.as_deref(),self.foreground.as_deref().unwrap_or(&self.package),self.auto_text,self.text_override);
        if wanted == self.text_input { return; }
        if wanted {
            self.resume_capture = self.pointer_lock.as_ref().is_some_and(PointerLock::requested);
        }
        self.release_mouse();
        self.release_keys();
        self.touch.release_all();
        self.text_input = wanted;
        if !wanted && std::mem::take(&mut self.resume_capture)
            && window.is_window_active() && !self.settings_open && !self.mapper.is_on() {
            self.toggle_mouse(window);
        }
        tracing::info!(active=wanted,"Android editor input mode changed");
        cx.notify();
    }

    fn toggle_auto_text(&mut self,cx:&mut Context<Self>) {
        let Some(package)=self.foreground.as_deref() else {return};
        let store=Store::discover();
        let Some(entry)=store.for_package(package) else {
            self.capture_message=Some("Сначала сохраните управление для этой игры.".into());cx.notify();return;
        };
        let mut profile=entry.profile.clone();
        profile.auto_text=!profile.auto_text;
        match store.save(&profile) {
            Ok(_)=>{self.auto_text=profile.auto_text;self.text_override=None;
                if let Some(draft)=self.mapper.profile.as_mut().filter(|p|p.package==profile.package) {draft.auto_text=profile.auto_text;}
            },
            Err(e)=>self.capture_message=Some(e.to_string()),
        }
        cx.notify();
    }

    fn poll_keyboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(keyboard) = &mut self.keyboard else { return };
        match keyboard.poll() {
            Ok(events) => {
                for event in events {
                    match event {
                        KeyEvent::Key {code,pressed} if self.settings_open => {
                            if !pressed {self.held_keys.remove(&code);}
                            if let Some(settings)=&self.settings {settings.update(cx,|s,cx|s.physical_shortcut_key(code,pressed,cx));}
                        }
                        KeyEvent::Key { code, pressed } if window.is_window_active() && !self.settings_open => {
                            self.route_key(code, pressed, window, cx);
                        }
                        KeyEvent::FocusLost => {
                            self.resume_capture = false;
                            self.release_mouse();
                            self.release_keys();
                            self.held_keys.clear();
                            if let Some(settings)=&self.settings {settings.update(cx,|s,_|s.use_physical_shortcuts(true));}
                            self.touch.release_all();
                            cx.notify();
                        }
                        _ => {}
                    }
                }
            }
            Err(error) => {
                tracing::warn!(%error, "physical keyboard connection lost");
                self.release_keys();
                self.held_keys.clear();
                self.touch.release_all();
                self.keyboard = None;
                if let Some(settings)=&self.settings {settings.update(cx,|s,_|s.use_physical_shortcuts(false));}
            }
        }
    }

    fn route_key(&mut self, code: u16, pressed: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.route_key_with_modifiers(code,pressed,None,window,cx);
    }
    fn reload_shortcuts(&mut self) {
        match crate::shortcuts::Shortcuts::load() {
            Ok(config)=>{self.shortcuts=config;self.shortcut_error.clear();},
            Err(e)=>{let error=e.to_string();if self.shortcut_error!=error {tracing::warn!(%error,"invalid shortcuts.json; retaining the last valid shortcuts");self.shortcut_error=error;}}
        }
    }
    fn shortcut_hint(&self,action:crate::shortcuts::Action)->String {
        self.shortcuts.label(action,self.touch.aim_key).map(|s|format!(" · {s}")).unwrap_or_default()
    }
    fn route_key_with_modifiers(&mut self, code:u16,pressed:bool,event_modifiers:Option<u8>,window:&mut Window,cx:&mut Context<Self>) {
        self.sync_editor_focus(window,cx);
        self.poll_mouse(cx);
        if self.settings_open { return; }
        let trigger = Trigger::Key(code);
        if !pressed {
            match self.held_keys.remove(&code) {
                Some(KeyRoute::Android) => {
                    if let Some(embedded) = &self.android.embedded {
                        embedded.key(code, false);
                    }
                }
                Some(KeyRoute::Binding) => self.touch.input(InputEvent::Release(TriggerKind::Key(code))),
                _ => {}
            }
            return;
        }
        if self.held_keys.contains_key(&code) {
            return;
        }
        use crate::shortcuts::{Action,CTRL,SHIFT,ALT,SUPER};
        let held=|codes:&[u16]|codes.iter().any(|key|self.held_keys.contains_key(key));
        let modifiers=event_modifiers.unwrap_or_else(||u8::from(held(&[29,97]))*CTRL | u8::from(held(&[42,54]))*SHIFT | u8::from(held(&[56,100]))*ALT | u8::from(held(&[125,126]))*SUPER);
        self.held_keys.insert(code, KeyRoute::Shortcut);
        if let Some(action)=self.shortcuts.resolve(code,modifiers,self.touch.aim_key) {
            let handled=match action {
                Action::Fullscreen=>{self.toggle_fullscreen(window,cx);true},
                Action::Settings=>{self.open_settings(window,cx);true},
                Action::Editor=>{self.toggle_mapping(window,cx);true},
                Action::Screenshot=>{self.screenshot(cx);true},
                Action::TextInput if !self.mapper.is_on()=>{self.toggle_text_input(cx);true},
                Action::CaptureMouse if !self.editing_foreground() && !self.text_input=>{self.toggle_mouse(window);cx.notify();true},
                _=>false,
            };
            if handled {return;}
        }
        match code {
            1 if self.performance.escape_mouse && self.pointer_lock.as_ref().is_some_and(PointerLock::requested) => {
                self.release_mouse();
                cx.notify();
            }
            1 if self.performance.escape_fullscreen && window.is_fullscreen() => {
                self.sidebar_open = true;
                self.toggle_fullscreen(window, cx);
            }
            _ if self.editing_foreground() => {
                self.editor_key(code);
                cx.notify();
            }
            _ if !self.text_input && self.touch.has_binding(&trigger) => {
                self.held_keys.insert(code, KeyRoute::Binding);
                self.touch.input(InputEvent::Press(TriggerKind::Key(code)));
            }
            _ => {
                self.held_keys.insert(code, KeyRoute::Android);
                if let Some(embedded) = &self.android.embedded {
                    embedded.key(code, true);
                }
            }
        }
    }

    fn editor_key(&mut self, code: u16) {
        // While assigning a binding, Delete and Backspace belong to the
        // binding. Outside that mode they delete the selected marker.
        if self.mapper.binding && code != 1 && self.mapper.take_trigger(Trigger::Key(code)) {
            return;
        }
        match code {
            103 => { self.mapper.nudge(0.0, -NUDGE); }
            108 => { self.mapper.nudge(0.0, NUDGE); }
            105 => { self.mapper.nudge(-NUDGE, 0.0); }
            106 => { self.mapper.nudge(NUDGE, 0.0); }
            26 => { self.mapper.resize(-0.01); }
            27 => { self.mapper.resize(0.01); }
            14 | 111 => self.mapper.remove_selected(),
            1 => {
                self.mapper.selected = None;
                self.mapper.binding = false;
            }
            _ => {}
        }
    }

    fn on_key_up(&mut self, ev: &gpui::KeyUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_open {
            if let Some(code)=crate::keys::evdev_code(&ev.keystroke.key) { self.held_keys.remove(&code); }
            cx.stop_propagation(); return;
        }
        if self.keyboard.is_none() && window.is_window_active() {
            if let Some(code) = crate::keys::evdev_code(&ev.keystroke.key) {
                self.route_key(code, false, window, cx);
            }
        }
    }

    fn on_key(&mut self, ev: &gpui::KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_open { cx.stop_propagation(); return; }
        if self.keyboard.is_none() && !ev.is_held && window.is_window_active() {
            if let Some(code) = crate::keys::evdev_code(&ev.keystroke.key) {
                self.route_key_with_modifiers(code,true,Some(crate::shortcuts::event_modifiers(ev)),window,cx);
            }
        }
        // Physical Wayland events already reach the input router.
        cx.stop_propagation();
    }

    fn open_settings(&mut self,window: &mut Window,cx: &mut Context<Self>) {
        self.release_mouse();
        self.release_keys();
        self.touch.release_all();
        self.drag_offset=None;
        if self.settings.is_none() {
            let settings=cx.new(crate::performance::Performance::embedded);
            cx.subscribe_in(&settings,window,|v,_,_: &crate::performance::SettingsClosed,window,cx| {
                v.settings_open=false;
                v.performance=crate::performance::Preferences::load();
                v.reload_shortcuts();
                if let Some(keyboard)=&mut v.keyboard {
                    for event in keyboard.poll().unwrap_or_default() {
                        if let KeyEvent::Key {code,pressed:false}=event { v.held_keys.remove(&code); }
                    }
                }
                window.focus(&v.focus,cx);
                report(window.is_window_active(),cx);
                cx.notify();
            }).detach();
            self.settings=Some(settings);
        }
        self.settings_open=true;
        if let Some(settings)=&self.settings { settings.update(cx,|s,cx|{s.set_capture_key(self.touch.aim_key);s.use_physical_shortcuts(self.keyboard.is_some());s.focus(window,cx);}); }
        report(false,cx);
        cx.notify();
    }
}

impl Render for GameView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(retired)=self.android.retire_previous_render() {
            if let Err(error)=window.drop_image(retired) {
                tracing::warn!(%error,"could not retire previous video frame from GPU atlas");
            }
        }
        let t = Theme::dark();

        if !self.titled {
            window.set_window_title(&self.title);
            self.titled = true;
        }

        // The picture is the window minus the rail. One piece of arithmetic
        // decides what the compositor renders AND where a click lands.
        let win = window.viewport_size();
        let fullscreen = window.is_fullscreen();
        let rail = if fullscreen { 0.0 } else { sidebar_width(self.sidebar_open) };
        let view = (f32::from(win.width) - rail, f32::from(win.height));
        self.android.resize(view.0, view.1);

        let guest = self
            .android
            .guest_size()
            .unwrap_or((view.0 as i32, view.1 as i32));
        let (_, shown) = liw_compositor::fit(guest, (view.0 as i32, view.1 as i32));
        self.shown = shown;

        div()
            .key_context("Game")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|v, ev: &gpui::KeyDownEvent, window, cx| v.on_key(ev, window, cx)))
            .on_key_up(cx.listener(|v, ev: &gpui::KeyUpEvent, window, cx| v.on_key_up(ev, window, cx)))
            .relative()
            .flex()
            .flex_row()
            .size_full()
            .bg(t.bg)
            .text_color(t.text)
            .font_family("sans-serif")
            .child(div().flex_1().h_full().child(picture(self, &t, shown, cx)))
            .when(self.performance.show_fps, |el| el.child(
                div().absolute().top(px(8.)).left(px(8.)).px(px(8.)).py(px(4.))
                    .bg(t.bg.opacity(0.85)).text_color(t.ok)
                    .child(SharedString::from(format!("{:.0} FPS", self.android.fps())))
            ))
            .when(!fullscreen, |el| el.child(sidebar(self, &t, cx)))
            .when(fullscreen && !self.mouse_captured(),|el| el.child(
                div().id("show-panel").absolute().top(px(8.)).left(px(8.))
                    .px(px(10.)).py(px(6.)).rounded(px(RADIUS)).bg(t.bg).text_size(px(12.)).cursor_pointer()
                    .child(SharedString::from(format!("Выйти из полного экрана{}",self.shortcut_hint(crate::shortcuts::Action::Fullscreen))))
                    .on_click(cx.listener(|v,_,window,cx| {
                        v.sidebar_open=true;
                        if window.is_fullscreen() { v.toggle_fullscreen(window, cx); }
                        window.focus(&v.focus,cx);
                    }))
            ))
            .when(self.text_input && !self.mapper.is_on(), |el| {
                el.child(capture_notice(&t, format!("Ввод текста{} — управление",self.shortcut_hint(crate::shortcuts::Action::TextInput))))
            })
            .when(self.mouse_captured(), |el| {
                let key = self.shortcuts.label(crate::shortcuts::Action::CaptureMouse,self.touch.aim_key).unwrap_or_default();
                let escape=if self.performance.escape_mouse {" / Esc"} else {""};
                el.child(capture_notice(&t, format!("Мышь захвачена · {key}{escape}")))
            })
            .when_some(self.capture_message.clone().or_else(|| self.touch.error.clone()), |el, message| {
                el.child(capture_notice(&t, message))
            })
            .when_some(self.settings.clone().filter(|_|self.settings_open),|el,settings| el.child(
                div().id("embedded-settings-overlay").absolute().inset_0().occlude()
                    .flex().items_center().justify_center().p(px(12.)).bg(t.bg.opacity(0.85))
                    .child(div().size_full().max_w(px(1000.)).max_h(px(760.)).overflow_hidden()
                        .rounded(px(8.)).border_1().border_color(t.border).child(settings))
            ))
    }
}

fn capture_notice(t: &Theme, message: String) -> impl IntoElement {
    div().absolute().top(px(12.0)).left(px(12.0)).p(px(S2))
        .rounded(px(6.0)).bg(t.surface.opacity(0.9)).text_size(px(12.0))
        .child(SharedString::from(message))
}

/// The Android picture, with the controls over it while editing.
fn picture(
    v: &GameView,
    t: &Theme,
    shown: (f32, f32),
    cx: &mut Context<GameView>,
) -> gpui::AnyElement {
    let body = match (&v.android.error, &v.android.image) {
        (Some(why), _) => div().size_full().flex().flex_col().items_center().justify_center().gap(px(12.))
            .child(message(t, "Не удалось подключить Android", why))
            .when(v.android.running(),|s|s.child(div().id("retry-android").px(px(14.)).py(px(9.)).rounded(px(6.)).bg(t.raised).cursor_pointer().child("Переподключить")
                .on_click(cx.listener(|v,_,_,cx| {
                    if v.android.running() {v.android.error=None;v.bring_up_android(cx);}
                    cx.notify();
                })))).into_any_element(),
        (None, Some((_, image))) => div()
            .size_full()
            .child(gpui::img(gpui::ImageSource::Render(image.clone())).size_full())
            .into_any_element(),
        (None, None) => message(
            t,
            "Ожидание Android",
            "Подключаем Android. Это может занять несколько секунд.",
        ),
    };

    let mut layer = div().relative().size_full().child(body);

    if !v.editing_foreground() {
        // Playing. The mouse is a finger: press, drag, release.
        //
        // Move is only listened for while something is down. Android has no
        // hover, so a stream of moves with no finger on the screen is work
        // for nothing — and on this path every one of them is a write into a
        // FIFO the guest has to read.
        return layer
            .id("play-surface")
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |v: &mut GameView, ev: &gpui::MouseDownEvent, window, cx| {
                    window.focus(&v.focus,cx);
                    v.poll_mouse(cx);
                    if v.mouse_captured() {
                        v.touch.button(Trigger::MouseLeft, true);
                        return;
                    }
                    if v.pointer_lock.as_ref().is_some_and(PointerLock::requested) {return;}
                    let p = (f32::from(ev.position.x), f32::from(ev.position.y));
                    let n = crate::touch::to_norm(p, shown);
                    // Both halves are logged because "the mouse does nothing"
                    // has two very different causes: the handler never fires,
                    // or it fires and the position falls outside the picture.
                    tracing::debug!(?p, ?shown, hit = n.is_some(), "mouse down");
                    if let Some(n) = n {
                        v.touch.press(n);
                        cx.notify();
                    }
                }),
            )
            .on_mouse_move(cx.listener(
                move |v: &mut GameView, ev: &gpui::MouseMoveEvent, _, _| {
                    if v.mouse_captured() { return; }
                    if ev.pressed_button == Some(gpui::MouseButton::Left) {
                        if let Some(n) = crate::touch::to_norm(
                            (f32::from(ev.position.x), f32::from(ev.position.y)), shown)
                        {
                            v.touch.drag(n);
                        }
                    } else {
                        // Drag off the WINDOW and release there and no mouse-up
                        // reaches us at all — the pointer left first. The next
                        // move back in is the first news of it, and it says the
                        // button is up, so lift then.
                        v.touch.release();
                    }
                },
            ))
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(move |v: &mut GameView, _: &gpui::MouseUpEvent, _, cx| {
                    v.touch.button(Trigger::MouseLeft, false);
                    v.touch.release();
                    cx.notify();
                }),
            )
            // Press on the game, drag onto the rail, let go there: `on_mouse_up`
            // only fires inside the element's own bounds, so that release never
            // arrives and the finger stays down — the game keeps walking, or
            // keeps firing, with nothing touching the mouse. This catches it.
            // Harmless when no finger is down: `release` returns immediately.
            .on_mouse_up_out(
                gpui::MouseButton::Left,
                cx.listener(move |v: &mut GameView, _: &gpui::MouseUpEvent, _, cx| {
                    v.touch.button(Trigger::MouseLeft, false);
                    v.touch.release();
                    cx.notify();
                }),
            )
            .on_mouse_down(gpui::MouseButton::Right, cx.listener(|v, _, _, _| {
                if !v.text_input {
                    v.touch.button(Trigger::MouseRight, true);
                }
            }))
            .on_mouse_up(gpui::MouseButton::Right, cx.listener(|v, _, _, _| {
                if !v.text_input {
                    v.touch.button(Trigger::MouseRight, false);
                }
            }))
            .on_mouse_up_out(gpui::MouseButton::Right, cx.listener(|v, _, _, _| {
                if !v.text_input {
                    v.touch.button(Trigger::MouseRight, false);
                }
            }))
            .on_mouse_down(gpui::MouseButton::Middle, cx.listener(|v, _, _, _| {
                if !v.text_input {
                    v.touch.button(Trigger::MouseMiddle, true);
                }
            }))
            .on_mouse_up(gpui::MouseButton::Middle, cx.listener(|v, _, _, _| {
                if !v.text_input {
                    v.touch.button(Trigger::MouseMiddle, false);
                }
            }))
            .on_mouse_up_out(gpui::MouseButton::Middle, cx.listener(|v, _, _, _| {
                if !v.text_input {
                    v.touch.button(Trigger::MouseMiddle, false);
                }
            }))
            .into_any_element();
    }

    let controls = v.mapper.controls();
    for c in &controls {
        layer = layer.child(control(c, t, shown));
    }
    // Resolve selection and placement in a single hit test. The topmost
    // visible marker wins over the background and over overlapping markers.
    layer.id("mapping-surface").cursor_crosshair()
        .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |v, ev: &gpui::MouseDownEvent, window, cx| {
            window.focus(&v.focus, cx);
            v.drag_offset = None;
            let point = (f32::from(ev.position.x), f32::from(ev.position.y));
            let Some(at) = crate::touch::to_norm(point, shown) else { return };
            if let Some(c) = marker_at(&controls, point, shown) {
                if v.mapper.selected.as_deref() != Some(&c.key) {
                    v.mapper.select(&c.key);
                }
                v.drag_offset = Some((c.at.x - at.x, c.at.y - at.y));
            } else {
                v.mapper.place(point, shown);
            }
            cx.stop_propagation();
            cx.notify();
        }))
        .on_mouse_down(gpui::MouseButton::Right, cx.listener(|v, _, _, cx| {
            if v.mapper.take_trigger(Trigger::MouseRight) {
                cx.stop_propagation();
                cx.notify();
            }
        }))
        .on_mouse_down(gpui::MouseButton::Middle, cx.listener(|v, _, _, cx| {
            if v.mapper.take_trigger(Trigger::MouseMiddle) {
                cx.stop_propagation();
                cx.notify();
            }
        }))
        .on_mouse_move(cx.listener(move |v, ev: &gpui::MouseMoveEvent, _, cx| {
            if ev.pressed_button != Some(gpui::MouseButton::Left) {
                v.drag_offset = None;
                return;
            }
            let Some((dx, dy)) = v.drag_offset else { return };
            if shown.0 <= 0.0 || shown.1 <= 0.0 { return; }
            let at = liw_input::touch::Norm::new(
                f32::from(ev.position.x) / shown.0 + dx,
                f32::from(ev.position.y) / shown.1 + dy,
            );
            if v.mapper.move_selected(at) {
                cx.notify();
            }
        }))
        .on_mouse_up(gpui::MouseButton::Left, cx.listener(|v, _, _, _| v.drag_offset = None))
        .on_mouse_up_out(gpui::MouseButton::Left, cx.listener(|v, _, _, _| v.drag_offset = None))
        .into_any_element()
}

fn marker_diameter(c: &Control, shown: (f32, f32)) -> f32 {
    c.radius.map_or(44.0, |r| (r * 2.0 * shown.0.min(shown.1)).max(90.0))
}

fn marker_at(controls: &[Control], point: (f32, f32), shown: (f32, f32)) -> Option<&Control> {
    controls.iter().rev().find(|c| {
        let r = marker_diameter(c, shown) * 0.5;
        let dx = point.0 - c.at.x * shown.0;
        let dy = point.1 - c.at.y * shown.1;
        dx * dx + dy * dy <= r * r
    })
}

/// One control drawn over the picture.
fn control(c: &Control, t: &Theme, shown: (f32, f32)) -> gpui::AnyElement {
    let d = marker_diameter(c, shown);
    div()
        .absolute()
        .left(px(c.at.x * shown.0 - d / 2.0))
        .top(px(c.at.y * shown.1 - d / 2.0))
        .size(px(d))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(d / 2.0))
        .border_2()
        .border_color(if c.selected { t.accent } else { t.border })
        .bg(if c.selected { t.accent.opacity(0.28) } else { t.raised.opacity(0.7) })
        .text_size(px(11.0))
        .text_color(t.text)
        .when_some(c.directions.clone(), |el, keys| {
            let keycap = |label: String| div().flex().items_center().justify_center()
                .size(px(25.0)).rounded(px(5.0)).border_1().border_color(t.accent.opacity(0.7))
                .bg(t.surface.opacity(0.9)).text_size(px(12.0)).child(SharedString::from(label));
            el.child(div().flex().flex_col().items_center().gap(px(3.0))
                .child(keycap(keys[0].clone()))
                .child(div().flex().gap(px(3.0))
                    .child(keycap(keys[1].clone()))
                    .child(div().size(px(25.0)).flex().items_center().justify_center().child("+"))
                    .child(keycap(keys[3].clone())))
                .child(keycap(keys[2].clone())))
        })
        .when(c.directions.is_none(), |el| el.child(SharedString::from(c.label.clone())))
        .into_any_element()
}

// ---------------------------------------------------------------------------
// The rail and its panel
// ---------------------------------------------------------------------------

fn sidebar(v: &GameView, t: &Theme, cx: &mut Context<GameView>) -> gpui::AnyElement {
    div()
        .flex()
        .flex_row()
        .h_full()
        .when(v.sidebar_open, |el| el.child(panel(v, t, cx)))
        .child(rail(v, t, cx))
        .into_any_element()
}

fn rail(v: &GameView, t: &Theme, cx: &mut Context<GameView>) -> gpui::AnyElement {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(S2))
        .w(px(RAIL_W))
        .h_full()
        .py(px(S3))
        .bg(t.surface)
        .border_l_1()
        .border_color(t.border)
        .child(tool(t, "r-open", if v.sidebar_open { "›" } else { "‹" }, false, cx,
                    |v, _, cx| { v.sidebar_open = !v.sidebar_open; cx.notify(); }))
        .child(tool(t, "r-map", "⌨", v.mapper.is_on() || v.edit_requested, cx,
                    |v, window, cx| v.toggle_mapping(window, cx)))
        .child(tool(t, "r-full", "⛶", false, cx, GameView::toggle_fullscreen))
        .child(div().flex_1())
        .child(div().id("r-settings").size(px(32.)).cursor_pointer()
            .child(crate::performance::gear(t.text_muted))
            .on_click(cx.listener(|v, _, window, cx|v.open_settings(window,cx))))
        .into_any_element()
}

/// One rail button. The glyph carries it: this gpui revision has no tooltip,
/// and a rail wide enough for a label would stop being a rail.
fn tool<F>(
    t: &Theme, id: &'static str, glyph: &str, on: bool,
    cx: &mut Context<GameView>, f: F,
) -> gpui::AnyElement
where
    F: Fn(&mut GameView, &Window, &mut Context<GameView>) + 'static,
{
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .size(px(32.0))
        .rounded(px(RADIUS))
        .text_size(px(15.0))
        .text_color(if on { t.accent } else { t.text_muted })
        .when(on, |e| e.bg(t.raised))
        .cursor_pointer()
        .hover(|x| x.bg(t.raised).text_color(t.text))
        .child(SharedString::from(glyph.to_string()))
        .on_click(cx.listener(move |v, _, window, cx| f(v, window, cx)))
        .into_any_element()
}

fn panel(v: &GameView, t: &Theme, cx: &mut Context<GameView>) -> gpui::AnyElement {
    let body = if v.mapper.is_on() { editor(v, t, cx) } else { idle(v, t, cx) };
    div()
        .id("panel")
        .flex()
        .flex_col()
        .w(px(PANEL_W))
        .h_full()
        .p(px(S3))
        .gap(px(S2))
        .bg(t.surface)
        .border_l_1()
        .border_color(t.border)
        .overflow_hidden()
        .child(body)
        .into_any_element()
}

fn idle(v: &GameView, t: &Theme, cx: &mut Context<GameView>) -> gpui::AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(S2))
        .child(title_row(v, t))
        .child(note(t, if v.android.running() {
            "Android подключён."
        } else {
            "Android пока не подключён."
        }))
        .child(head(t, "Управление"))
        .when_some(v.foreground.as_deref(), |el, package| el.child(note(t, package)))
        .child(action(t, "text-input", &format!("{}{}",if v.text_input {"Вернуться к управлению"} else {"Ввод текста"},v.shortcut_hint(crate::shortcuts::Action::TextInput)), cx, |v, cx| v.toggle_text_input(cx)))
        .child(action(t,"auto-text",if v.auto_text {"Автоввод текста: включён"} else {"Автоввод текста: выключен"},cx,|v,cx|v.toggle_auto_text(cx)))
        .when_some(v.touch.aim_key, |el, key| {
            el.child(row(t, "Захват мыши", &v.shortcuts.label(crate::shortcuts::Action::CaptureMouse,Some(key)).unwrap_or_else(||"Отключено".into())))
        })
        .child(note(t, &format!("Редактор управления{} — или нажмите значок клавиатуры.",v.shortcut_hint(crate::shortcuts::Action::Editor))))
        .into_any_element()
}

/// The editor panel: a palette, then the selected control's settings.
fn editor(v: &GameView, t: &Theme, cx: &mut Context<GameView>) -> gpui::AnyElement {
    let mut palette = div().flex().flex_col().gap(px(S2));
    for kinds in Kind::ALL.chunks(2) {
        let mut tiles = div().flex().gap(px(S2));
        for &kind in kinds {
            let on = v.mapper.palette == kind;
            tiles = tiles.child(
                div().id(SharedString::from(format!("k-{}", kind.label())))
                    .flex().flex_col().items_center().justify_center().flex_1()
                    .h(px(72.0)).gap(px(5.0)).rounded(px(8.0)).border_1()
                    .border_color(if on { t.accent } else { t.border })
                    .bg(if on { t.accent.opacity(0.12) } else { t.raised })
                    .text_size(px(12.0)).text_color(if on { t.accent } else { t.text })
                    .cursor_pointer().hover(|e| e.border_color(t.accent))
                    .child(control_icon(kind, if on { t.accent } else { t.text }))
                    .child(SharedString::from(kind.label()))
                    .on_click(cx.listener(move |v: &mut GameView, _, window, cx| {
                        v.mapper.palette = kind;
                        window.focus(&v.focus, cx);
                        cx.notify();
                    })),
            );
        }
        palette = palette.child(tiles);
    }

    let settings = match v.mapper.selection() {
        None => div().p(px(S3)).rounded(px(8.0)).bg(t.raised)
            .child(note(t, "Нажмите на маркер, чтобы настроить его, или выберите инструмент и добавьте новый."))
            .into_any_element(),
        Some(c) => {
            let key_text = if v.mapper.binding {
                v.mapper.binding_prompt().unwrap_or("Нажмите клавишу").to_string()
            } else { c.label.clone() };
            let mut b = div().flex().flex_col().gap(px(S2)).p(px(S3))
                .rounded(px(8.0)).bg(t.raised)
                .child(row(t, "Тип", c.kind.label()))
                .child(row(t, "Клавиши", &key_text));
            if c.kind == Kind::Joystick {
                b = b.child(action(t, "e-wasd", "Назначить WASD", cx, |v, cx| {
                    v.mapper.use_wasd(); cx.notify();
                }));
            }
            if c.kind != Kind::Fire {
                let rebind = if c.kind == Kind::Aim { "Клавиша захвата мыши" } else { "Изменить клавишу или кнопку" };
                b = b.child(action(t, "e-rebind", rebind, cx, |v, cx| {
                    v.mapper.rebind();
                    cx.notify();
                }));
            }
            if v.mapper.binding && matches!(c.kind, Kind::Tap | Kind::Toggle) {
                b = b.child(div().flex().gap(px(S2))
                    .child(action(t, "bind-lmb", "ЛКМ", cx, |v, cx| {
                        v.mapper.take_trigger(Trigger::MouseLeft);
                        cx.notify();
                    }))
                    .child(action(t, "bind-rmb", "ПКМ", cx, |v, cx| {
                        v.mapper.take_trigger(Trigger::MouseRight);
                        cx.notify();
                    }))
                    .child(action(t, "bind-mmb", "СКМ", cx, |v, cx| {
                        v.mapper.take_trigger(Trigger::MouseMiddle);
                        cx.notify();
                    })));
            }
            if c.kind == Kind::Fire {
                b = b.child(note(t, "Наведите маркер на кнопку огня. ЛКМ стреляет при захвате мыши."));
            }
            b = b.child(row(t, "Положение", &format!("{:.0}% · {:.0}%", c.at.x * 100.0, c.at.y * 100.0)));
            if let Some(r) = c.radius {
                b = b.child(row(t, "Размер", &format!("{:.0}%", r * 100.0)))
                    .child(div().flex().gap(px(S2))
                        .child(action(t, "e-smaller", "− Уменьшить", cx, |v, cx| { v.mapper.resize(-0.01); cx.notify(); }))
                        .child(action(t, "e-larger", "+ Увеличить", cx, |v, cx| { v.mapper.resize(0.01); cx.notify(); })));
            }
            b.child(action(t, "e-delete", "Удалить элемент", cx, |v, cx| {
                v.mapper.remove_selected(); cx.notify();
            })).into_any_element()
        }
    };

    div().flex().flex_col().h_full().min_h(px(0.0)).gap(px(S3))
        .child(div().flex().flex_shrink_0().items_center().justify_between()
            .child(div().text_size(px(17.0)).child("Редактор управления"))
            .child(action(t, "e-close", "×", cx, |v, cx| v.close_editor(cx))))
        .child(div().id("editor-content").flex().flex_col().flex_1().min_h(px(0.0))
            .overflow_y_scroll().gap(px(S3))
        .when_some(v.mapper.profile.as_ref(), |el, profile| el.child(note(t, &profile.package)))
        .when(v.editing_foreground(), |el| el
            .child(note(t, "Добавьте элементы поверх игровых кнопок."))
            .child(palette)
            .child(head(t, "Выбранный элемент"))
            .child(settings)
            .child(note(t, "Перетащите маркер мышью. Стрелки — точный сдвиг, Delete — удалить.")))
        .when(!v.editing_foreground(), |el| {
            el.child(note(t, "Вернитесь в эту игру, чтобы продолжить редактирование."))
        }))
        .when_some(v.mapper.error.clone(), |el, e| {
            el.child(div().p(px(S2)).rounded(px(6.0)).bg(t.bad.opacity(0.08))
                .text_size(px(12.0)).text_color(t.bad).child(SharedString::from(e)))
        })
        .when(v.mapper.saved, |el| el.child(
            div().text_size(px(12.0)).text_color(t.ok).child("Настройки сохранены")))
        .child(div().flex().flex_shrink_0().gap(px(S2))
            .child(action(t, "e-save", "Сохранить", cx, |v, cx| { v.mapper.save(); cx.notify(); }))
            .child(action(t, "e-done", "Готово", cx, |v, cx| v.close_editor(cx))))
        .into_any_element()
}

/// Vector tool icons, independent of the installed symbol font.
fn control_icon(kind: Kind, color: gpui::Hsla) -> impl IntoElement {
    gpui::canvas(|_, _, _| (), move |bounds, (), window, _| {
        let p = |x: f32, y: f32| bounds.origin + gpui::point(px(x), px(y));
        let mut path = gpui::PathBuilder::stroke(px(1.8));
        let mut circle = |cx: f32, cy: f32, r: f32| {
            for i in 0..=32 {
                let a = i as f32 * std::f32::consts::TAU / 32.0;
                let q = p(cx + r * a.cos(), cy + r * a.sin());
                if i == 0 { path.move_to(q); } else { path.line_to(q); }
            }
        };
        match kind {
            Kind::Tap => { circle(18., 18., 10.); circle(18., 18., 3.); }
            Kind::Toggle => { circle(18., 18., 13.); circle(18., 18., 8.); }
            Kind::Joystick => {
                circle(18., 18., 14.);
                for (x1,y1,x2,y2) in [(18.,7.,18.,13.),(18.,23.,18.,29.),(7.,18.,13.,18.),(23.,18.,29.,18.)] {
                    path.move_to(p(x1,y1)); path.line_to(p(x2,y2));
                }
            }
            Kind::Fire => {
                circle(18., 18., 13.);
                circle(18., 18., 7.);
                circle(18., 18., 2.);
            }
            Kind::Aim => {
                circle(18.,18.,10.);
                for (x1,y1,x2,y2) in [(18.,2.,18.,13.),(18.,23.,18.,34.),(2.,18.,13.,18.),(23.,18.,34.,18.)] {
                    path.move_to(p(x1,y1)); path.line_to(p(x2,y2));
                }
            }
        }
        if let Ok(path) = path.build() { window.paint_path(path, color); }
    }).size(px(36.0))
}

fn row(t: &Theme, k: &str, val: &str) -> gpui::AnyElement {
    div()
        .flex()
        .flex_row()
        .justify_between()
        .text_size(px(12.0))
        .child(div().text_color(t.text_faint).child(SharedString::from(k.to_string())))
        .child(div().text_color(t.text).child(SharedString::from(val.to_string())))
        .into_any_element()
}

fn action<F>(
    t: &Theme, id: &'static str, label: &str,
    cx: &mut Context<GameView>, f: F,
) -> gpui::AnyElement
where
    F: Fn(&mut GameView, &mut Context<GameView>) + 'static,
{
    div()
        .id(id)
        .flex().items_center().justify_center()
        .min_h(px(34.0)).px(px(S3)).py(px(S2))
        .rounded(px(6.0))
        .bg(if id == "e-save" { t.accent } else { t.surface })
        .text_color(if id == "e-save" { t.bg } else { t.text })
        .border_1()
        .border_color(t.border)
        .text_size(px(12.0))
        .cursor_pointer()
        .hover(|x| x.border_color(t.accent))
        .child(SharedString::from(label.to_string()))
        .on_click(cx.listener(move |v, _, window, cx| {
            window.focus(&v.focus, cx);
            f(v, cx);
        }))
        .into_any_element()
}

/// The game's icon and name, at the top of the panel.
fn title_row(v: &GameView, t: &Theme) -> gpui::AnyElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(S2))
        .when_some(v.icon.clone(), |el, p| {
            el.child(
                gpui::img(gpui::ImageSource::Resource(gpui::Resource::Path(
                    std::sync::Arc::from(p.as_path()),
                )))
                .w(px(28.0))
                .h(px(28.0)),
            )
        })
        .child(
            div()
                .text_size(px(13.0))
                .text_color(t.text)
                .child(SharedString::from(v.title.clone())),
        )
        .into_any_element()
}

fn head(t: &Theme, s: &str) -> gpui::AnyElement {
    div()
        .text_size(px(11.0))
        .text_color(t.text_faint)
        .child(SharedString::from(s.to_uppercase()))
        .into_any_element()
}

fn note(t: &Theme, s: &str) -> gpui::AnyElement {
    div()
        .text_size(px(12.0))
        .text_color(t.text_muted)
        .child(SharedString::from(s.to_string()))
        .into_any_element()
}

fn message(t: &Theme, title: &str, detail: &str) -> gpui::AnyElement {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(S2))
        .child(div().text_color(t.text).child(SharedString::from(title.to_string())))
        .child(
            div()
                .max_w(px(520.))
                .text_color(t.text_faint)
                .text_size(px(12.0))
                .child(SharedString::from(detail.to_string())),
        )
        .into_any_element()
}

/// Opens the game window.
///
/// `title` is the game's name rather than its package: a window called
/// `com.ForgeGames.SpecialForcesGroup2` in the task switcher is a package
/// manager's idea of a name, not a person's.
/// How often the view looks for a new frame.
///
/// Deliberately much shorter than a frame. At 16ms this polled at the same
/// rate the compositor produced, but the two clocks were not locked together:
/// measured, 60.0 frames a second were produced and 56.3 reached the window —
/// a tick would find nothing, and the tick after it would find a frame that
/// had already been overwritten by the next one. Four a second, lost to
/// beating.
///
/// At 4ms there are four looks per frame, so a frame can only be missed if
/// two arrive inside 4ms, which 60Hz cannot do. A look that finds nothing
/// costs one lock and one integer compare, and the window is only repainted
/// when a frame is actually taken.
const POLL: std::time::Duration = std::time::Duration::from_millis(4);

/// The window class this window carries.
///
/// It is NOT the launcher's. The key mapper decides whether Android has the
/// focus from the focused window's class, and this window is Android's screen
/// while the launcher is a list of games; one class for both would mean the
/// keyboard is grabbed over the library, where the user is trying to type.
///
/// Wayland also has no per-window icon, so a compositor looks this up in the
/// desktop entries — hence `dist/desktop/liwinux-game.desktop`, whose
/// StartupWMClass has to match.
pub const APP_ID: &str = "liwinux-game";

/// Sends the focus state to the daemon, as the class it would see from KWin.
///
/// Failures are ignored on purpose: no daemon means no key mapper either, and
/// a window that refused to draw because it could not report focus would be
/// worse than one that simply does not map keys.
fn report(active: bool, cx: &mut App) {
    let class = if active { APP_ID } else { "" };
    gpui_tokio::Tokio::spawn(cx, async move {
        if let Ok(m) = liw_core::manager::Manager::connect().await {
            let _ = m.set_active_window(class).await;
            tracing::debug!(class, "reported focus");
        }
    })
    .detach();
}

pub fn open(
    package: String,
    title: String,
    icon: Option<std::path::PathBuf>,
    cx: &mut App,
) -> anyhow::Result<()> {
    let (title,icon)=if title==package {
        match liw_core::apps::discover().into_iter().find(|app|app.package==package) {
            Some(app)=>(app.name,icon.or(app.icon)),
            None=>(title,icon),
        }
    } else { (title,icon) };
    if let Some(window)=cx.windows().iter().find_map(|w|w.downcast::<GameView>()) {
        window.update(cx,|view,window,cx| {
            window.activate_window();
            if view.package != package {
                view.release_mouse();
                view.release_keys();
                view.text_input = false;
                view.touch.release_all();
                view.package=package;
                view.title=title;
                view.icon=icon;
                view.titled=false;
                view.android.error=None;
                cx.notify();
            }
            view.launch_selected(cx);
        })?;
        return Ok(());
    }
    let mode = liw_core::display::GuestDisplay::load().map_err(anyhow::Error::msg)?;
    let screen = cx.primary_display().map(|display| display.bounds().size)
        .unwrap_or_else(|| gpui::size(px(1920.0), px(1080.0)));
    let size = window_size(mode, screen);
    let bounds = gpui::Bounds::centered(None, size, cx);
    cx.open_window(
        gpui::WindowOptions {
            window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
            is_resizable: false,
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(SharedString::from(title.clone())),
                ..Default::default()
            }),
            // Same family as the launcher, so the two group together and the
            // window is recognisable before its own icon is drawn inside it.
            // A class of its OWN, not the launcher's. The keymapper decides
            // whether Android has the focus from the focused window's class,
            // and this window is Android's screen while the launcher is a
            // list of games. One class for both would mean the keyboard is
            // grabbed over the library, where the user is trying to type.
            app_id: Some(APP_ID.into()),
            ..Default::default()
        },
        |window, cx| cx.new(|cx| {
            // The pinned GPUI Wayland backend does not enforce is_resizable.
            // Restore the regular content size after a compositor resize.
            cx.observe_window_bounds(window, move |_, window, _| {
                if matches!(window.window_bounds(), gpui::WindowBounds::Windowed(_))
                    && window.viewport_size() != size {
                    window.resize(size);
                }
            }).detach();
            GameView::new(package, title, icon, window, cx)
        }),
    )?;
    Ok(())
}

fn control_target(package: &str) -> bool {
    liw_core::apps::valid_package(package)
        && !matches!(package, "dev.waydroid.manager.home" | "com.android.launcher3"
            | "com.android.systemui" | "com.android.permissioncontroller" | "com.google.android.permissioncontroller")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_geometry_fits_the_display_with_the_rail() {
        let screen = gpui::size(px(1366.0), px(768.0));
        let landscape = liw_core::display::GuestDisplay { width: 1920, height: 1080, fps: 240 };
        let portrait = liw_core::display::GuestDisplay { width: 1080, height: 1920, ..landscape };
        assert_eq!(window_size(landscape, screen), gpui::size(px(1239.0), px(672.0)));
        assert_eq!(window_size(portrait, screen), gpui::size(px(422.0), px(672.0)));
    }

    #[test]
    fn launcher_and_system_overlays_do_not_load_game_controls() {
        assert!(control_target("com.axlebolt.standoff2"));
        assert!(!control_target("dev.waydroid.manager.home"));
        assert!(!control_target("com.android.launcher3"));
        assert!(!control_target("com.android.permissioncontroller"));
        assert!(!control_target(""));
    }

    /// The picture's size is computed from this. A second opinion about the
    /// rail's width would put every control a few pixels from its click.
    #[test]
    fn the_panel_widens_the_sidebar() {
        assert_eq!(sidebar_width(false), RAIL_W);
        assert_eq!(sidebar_width(true), RAIL_W + PANEL_W);
    }

    /// Collapsing hides the panel, never the way back to it.
    #[test]
    fn the_rail_never_disappears() {
        assert!(sidebar_width(false) > 0.0);
    }
}
