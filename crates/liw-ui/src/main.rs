//! liw-ui — the liwinux desktop front end.
//!
//! # Not in the render path
//!
//! This window never sits on top of the game. KWin can hand a fullscreen
//! window straight to the display; any always-on-top overlay closes that path
//! and forces full composition, which costs frames in the one place they
//! matter. So: a normal window, and it stays out of the way while you play.
//!
//! It is also disposable. All state lives in `liwd`, so closing this changes
//! nothing about a running session — the daemon is the product, this is a
//! viewport onto it.

mod library;
mod tint;
mod android;
mod keys;
mod game;
mod mapper;
mod touch;
mod shell;
mod diagnostics;
mod keymap;
mod settings;
mod state;
mod theme;
mod instance;
mod pointer_lock;
mod keyboard;

use gpui::{App, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};

/// Re-exec before threads/graphics exist, with per-process GLVND selection.
/// The desktop/Codex may export a Mesa-only filter; importing NVK dmabufs on
/// Intel then fails even though Android itself renders correctly on NVIDIA.
fn prepare_graphics_environment() -> anyhow::Result<()> {
    use std::os::unix::process::CommandExt;
    let cfg = std::fs::read_to_string("/var/lib/waydroid/waydroid.cfg").unwrap_or_default();
    if !liw_core::nvidia::source_native_selected(&cfg) { return Ok(()); }
    let nvidia = "/usr/share/glvnd/egl_vendor.d/10_nvidia.json";
    let mesa = "/usr/share/glvnd/egl_vendor.d/50_mesa.json";
    anyhow::ensure!(std::path::Path::new(nvidia).is_file(),"NVIDIA EGL userspace is missing");
    // WGPU probes GL as well as Vulkan. Keep Mesa available for the host
    // display while preferring the proprietary EGL device for guest import.
    let vendors = if std::path::Path::new(mesa).is_file() { format!("{nvidia}:{mesa}") } else { nvidia.into() };
    let mut nodes: Vec<_> = std::fs::read_dir("/sys/class/drm")?.filter_map(Result::ok)
        .filter(|p|p.file_name().to_string_lossy().starts_with("renderD")).collect();
    nodes.sort_by_key(|p|p.file_name());
    let device = nodes.into_iter().find(|p|std::fs::read_to_string(p.path().join("device/vendor")).is_ok_and(|s|s.trim()=="0x10de"))
        .ok_or_else(||anyhow::anyhow!("NVIDIA render node is unavailable"))?;
    let id = std::fs::read_to_string(device.path().join("device/device"))?.trim().to_owned();
    let node = format!("/dev/dri/{}",device.file_name().to_string_lossy());
    let requested = [("__EGL_VENDOR_LIBRARY_FILENAMES",vendors),("ZED_DEVICE_ID",id),("LIW_EGL_RENDER_NODE",node)];
    if requested.iter().all(|(k,v)|std::env::var(k).as_deref()==Ok(v.as_str())) { return Ok(()); }
    let error = std::process::Command::new(std::env::current_exe()?)
        .args(std::env::args_os().skip(1)).envs(requested).exec();
    Err(error.into())
}

fn main() {
    if let Err(error) = prepare_graphics_environment() {
        eprintln!("Cannot prepare NVIDIA graphics: {error}");
        std::process::exit(1);
    }
    let mut args=std::env::args().skip(1);
    let mut requested=None;
    while let Some(arg)=args.next() { if arg=="--game" { requested=args.next(); } }
    let runtime=std::env::var_os("XDG_RUNTIME_DIR").expect("Run in your logged-in Wayland desktop");
    let (_instance,requests)=match instance::acquire(std::path::Path::new(&runtime),instance::Request { package:requested.clone() }) {
        Ok(Some(owner)) => owner,
        Ok(None) => return,
        Err(error) => { eprintln!("Cannot start Rust interface: {error}"); std::process::exit(1); }
    };
    keyboard::clear_export(std::path::Path::new(&runtime));
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("LIW_UI_LOG").unwrap_or_else(|_| "warn".into()))
        .init();

    // A desktop launch must bring its user daemon up too. The unit remains
    // disabled at login: opening this Rust application owns the transition.
    match std::process::Command::new("systemctl").args(["--user","start","liwd.service"]).status() {
        Ok(status) if status.success() => {},
        result => tracing::warn!(?result,"could not start liwd; the window will show connection diagnostics"),
    }

    // Pinned-rev API: the platform crate builds the application, there is no
    // `Application::new()`.
    gpui_platform::application().run(move |cx: &mut App| {
        gpui_tokio::init(cx);
        let bounds = Bounds::centered(None, size(px(1120.), px(720.)), cx);
        let state = state::build(cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(880.), px(560.))),
                // The nav strip IS the titlebar: transparent so the system
                // does not draw a second one above ours. Under CSD we draw
                // our own caption buttons (see `shell::caption`); under SSD
                // the compositor still draws real ones.
                titlebar: Some(TitlebarOptions {
                    title: Some("NexDroid".into()),
                    appears_transparent: true,
                    traffic_light_position: None,
                }),
                // Wayland hands a window no picture: the compositor looks
                // the app_id up in the desktop entries and takes the icon
                // from there. This must match StartupWMClass in
                // dist/desktop/liwinux.desktop.
                app_id: Some("liwinux".into()),
                ..Default::default()
            },
            |_, _| state,
        )
        .expect("could not open the window");

        // `liw-ui --game <package>` opens the game window straight away.
        // Useful on its own, and it is the only way to reach that window
        // without a mouse — which matters when the point is to test it.
        let mut args = std::env::args().skip(1);
        while let Some(a) = args.next() {
            if a == "--game" {
                if let Some(pkg) = args.next() {
                    let title = pkg.clone();
                    if let Err(e) = game::open(pkg, title, None, cx) {
                        tracing::error!(error = %e, "could not open the game window");
                    }
                }
            }
        }
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor().timer(std::time::Duration::from_millis(50)).await;
                while let Ok(request)=requests.try_recv() {
                    cx.update(|cx| {
                        if let Some(package)=request.package {
                            if let Err(error)=game::open(package.clone(),package,None,cx) {
                                tracing::error!(%error,"cannot route desktop launch");
                            }
                        } else if let Some(window)=cx.windows().first() {
                            let _=window.update(cx,|_,window,_|window.activate_window());
                        }
                    });
                }
            }
        }).detach();
        // With no windows there is nothing left to do and nothing to show.
        // gpui does not assume that — an app can legitimately live in a tray —
        // so it has to be said. Without it, closing the launcher leaves a
        // resident process and the next start from KDE adds another beside it.
        cx.on_window_closed(|cx, _id| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        cx.activate(true);
    });
}
