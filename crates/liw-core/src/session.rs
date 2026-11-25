//! Session lifecycle and health supervision.
//!
//! # Why this module exists
//!
//! `waydroid session start` runs in the foreground. The moment it dies the
//! chain is:
//!
//! ```text
//! composer HAL dies
//!   -> SurfaceFlinger SIGABRT ("HIDL return status ... DEAD_OBJECT")
//!   -> system_server dies
//!   -> every app gets DeadSystemException
//! ```
//!
//! Android never fully recovers: the system restarts but the **default route
//! does not come back**, leaving a network-less zombie. The visible symptom
//! ("Play Store has no internet") looks nothing like the root cause.
//!
//! That is why the session must never be tied to a terminal, and `liwd` must
//! own it.

use crate::helper::HelperClient;
use crate::waydroid::{Status, Waydroid, WaydroidError};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

/// Observed state of the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SessionState {
    Stopped,
    Starting,
    Running,
    /// Processes are up but health checks fail (the network-less zombie state).
    Degraded,
    Recovering,
}

impl SessionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stopped => "STOPPED",
            Self::Starting => "STARTING",
            Self::Running => "RUNNING",
            Self::Degraded => "DEGRADED",
            Self::Recovering => "RECOVERING",
        }
    }
}

/// Individual health signals. Each can break on its own, which is why we keep
/// the detail rather than a single flag — diagnostic experience showed a
/// "works / does not work" binary is not enough.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Health {
    pub session_running: bool,
    pub container_running: bool,
    pub has_ip: bool,
    pub boot_completed: bool,
    /// Is the Waydroid composer HAL process alive? Its death starts the chain.
    pub composer_alive: bool,
    /// Did composer restart AFTER the session?
    ///
    /// If so the session's binder connection is stale: processes look alive,
    /// there is an IP, boot completed — but no window appears and
    /// `waydroid app launch` says "Sending reply failed". A health check
    /// reporting "all good" would be misleading.
    pub composer_stale: bool,
}

impl Health {
    /// Is the game playable? Every signal is required.
    pub fn is_healthy(&self) -> bool {
        self.session_running && self.container_running
            && self.has_ip && self.boot_completed
            && self.composer_alive && !self.composer_stale
    }
    /// Network-only failures must not restart Android.
    pub fn requires_recovery(&self) -> bool {
        !self.session_running
            || !self.container_running
            || !self.boot_completed
            || !self.composer_alive
            || self.composer_stale
    }

    /// So a human can read what is missing.
    pub fn failures(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if !self.session_running { v.push("session not running"); }
        if !self.container_running { v.push("container not running"); }
        if !self.composer_alive { v.push("composer HAL dead (crash-chain risk)"); }
        if self.composer_stale {
            v.push("composer restarted after the session — stale binder connection                     (no window appears, app launch says 'Sending reply failed')");
        }
        if !self.boot_completed { v.push("Android boot did not complete"); }
        if !self.has_ip { v.push("no IP assigned"); }
        v
    }
}

/// Marker in the Android log for AudioFlinger failing to publish.
const AF_MARKER: &str = "AudioFlinger not published";

/// Startup is allowed to use boot_timeout; healthy-runtime failures are not.
#[derive(Default)]
pub struct BootGrace {
    began: Option<std::time::Instant>,
    booted: bool,
}
impl BootGrace {
    pub fn update(&mut self, health: &Health, now: std::time::Instant, timeout: Duration) -> bool {
        if !health.session_running { *self=Self::default(); return false; }
        if health.boot_completed { self.booted=true; return false; }
        if self.booted { return false; }
        let began=*self.began.get_or_insert(now);
        now.saturating_duration_since(began) < timeout
    }
}

/// Detects from the log whether the audio stack is wedged.
///
/// The chain works like this and all of it was measured: the audio HAL wedges
/// -> `audioserver` blocks in its `registerClient` call -> the `mediautils`
/// watchdog aborts the process after 5 seconds -> it restarts and wedges in
/// the same place. Result: `AudioFlinger` NEVER publishes to the service
/// manager and every app that touches audio waits forever.
///
/// The symptom the user sees is "the game freezes on its loading screen"; there
/// is nothing anywhere to suggest audio is involved. Hence the detection.
///
/// ONE line is NOT enough: during boot AudioFlinger genuinely is waited for a
/// few times and that is normal. Only persistent repetition indicates a fault.
pub fn audio_flinger_stalled(log: &str) -> bool {
    const MIN_REPEATS: usize = 5;
    log.lines().filter(|l| l.contains(AF_MARKER)).count() >= MIN_REPEATS
}

#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    pub poll_interval: Duration,
    pub boot_timeout: Duration,
    /// After how many consecutive unhealthy polls to attempt recovery.
    /// 1 is too aggressive: transient dips would trigger pointless restarts.
    pub unhealthy_threshold: u32,
    pub max_recovery_attempts: u32,
    pub auto_recover: bool,
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(5),
            boot_timeout: Duration::from_secs(180),
            unhealthy_threshold: 3,
            max_recovery_attempts: 3,
            auto_recover: true,
        }
    }
}

pub struct Supervisor {
    lifecycle: tokio::sync::Mutex<DisplayOwner>,
    start_generation: std::sync::atomic::AtomicU64,
    wd: Waydroid,
    cfg: SupervisorConfig,
    /// Wayland socket the session should attach to, when it is not the
    /// desktop's own.
    ///
    /// Held in memory rather than in the config on purpose. It names a socket
    /// owned by a window that is open right now; persisting it would outlive
    /// that window and send the next session to a socket nobody is listening
    /// on. It is also validated before every use, for the same reason.
    embedded_display: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    /// For privileged reads. Without it the boot state cannot be MEASURED, only
    /// inferred.
    helper: Option<HelperClient>,
}

impl Supervisor {
    pub fn new(cfg: SupervisorConfig) -> Self {
        Self { lifecycle:Default::default(),start_generation: std::sync::atomic::AtomicU64::new(0),wd: Waydroid::new(), cfg, helper: None,
               embedded_display: Default::default() }
    }

    /// Tries to connect to liwd-helper. Failure is not fatal: the supervisor
    /// works without it, only the boot state cannot be measured.
    pub async fn with_helper(mut self) -> Self {
        match HelperClient::connect().await {
            Ok(h) => { tracing::info!("liwd-helper connected — boot state will be measured"); self.helper = Some(h); }
            Err(e) => tracing::warn!(error = %e, "no liwd-helper — boot state will be inferred"),
        }
        self
    }

    pub fn has_helper(&self) -> bool { self.helper.is_some() }

    pub fn start_generation(&self) -> u64 {
        self.start_generation.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn config(&self) -> &SupervisorConfig { &self.cfg }

    /// Points new sessions at a window's socket. `None` restores the default.
    pub fn set_embedded_display(&self, name: Option<String>) {
        if let Ok(mut d) = self.embedded_display.lock() {
            *d = name;
        }
    }

    /// The socket to start against, if one is set AND still there.
    ///
    /// The check is the point. A window that has closed leaves a name behind
    /// with nothing listening on it, and starting Android against that gives
    /// a session that boots and draws into nothing — which looks like a
    /// broken compositor rather than a stale setting.
    fn display_to_use(&self) -> Option<String> {
        let name = self.embedded_display.lock().ok()?.clone()?;
        let dir = std::env::var("XDG_RUNTIME_DIR").ok()?;
        std::path::Path::new(&dir).join(&name).exists().then_some(name)
    }

    pub async fn status(&self) -> Result<Status, WaydroidError> {
        self.wd.status().await
    }

    /// Starts the session **detached from any terminal**.
    ///
    /// `setsid` makes it a new session leader and stdio is closed, so the
    /// session survives the calling shell dying (Ctrl+C, closing the window).
    pub async fn start_detached(&self) -> Result<(), WaydroidError> {
        let _guard=self.lifecycle.lock().await;
        self.start_inner().await
    }
    async fn start_inner(&self) -> Result<(), WaydroidError> {
        let st = self.wd.status().await?;
        if st.session_running() {
            tracing::info!("session already running");
            return Ok(());
        }
        crate::nvidia::preflight().map_err(WaydroidError::DriverCompatibility)?;
        if let Some(helper)=&self.helper {
            helper.prepare_root().await.map_err(|e|WaydroidError::Lifecycle(format!("Could not prepare Android root settings: {e}")))?;
        }
        let socket = self.display_to_use();
        if self.embedded_display.lock().map(|d|d.is_some()).unwrap_or(true) && socket.is_none() {
            return Err(WaydroidError::Lifecycle("The embedded display disappeared; refusing to start Android on another compositor".into()));
        }
        tracing::info!(?socket, "starting session (detached)");
        let mut cmd = Command::new("setsid");
        cmd.args(["--fork", "waydroid", "session", "start"]);
        cmd.env(
            "NEXDROID_HOST_WAYLAND_DISPLAY",
            std::env::var_os("WAYLAND_DISPLAY").unwrap_or_default(),
        );
        if let Some(d) = &socket {
            cmd.env("WAYLAND_DISPLAY", d);
            cmd.env_remove("WAYLAND_SOCKET");
            // Android 13+ uses the composer selection protocol in our nested display.
            cmd.env("NEXDROID_CLIPBOARD", "0");
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        self.start_generation.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
        let status=cmd.spawn()?.wait().await?;
        if !status.success() {return Err(WaydroidError::Lifecycle(format!("Detached Android launcher failed: {status}")));}
        // `setsid --fork` exits before Waydroid registers its session bus name.
        // Keep the lifecycle lock until the session is observable, otherwise
        // an immediate attach could stop nothing and inherit this late start.
        for _ in 0..120 {
            if self.wd.status().await?.session_running() {return Ok(());}
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        Err(WaydroidError::Lifecycle("Android did not register its session after launch".into()))
    }

    pub async fn stop(&self) -> Result<(), WaydroidError> {
        let _guard=self.lifecycle.lock().await;
        self.start_generation.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
        self.stop_inner().await
    }
    async fn stop_inner(&self) -> Result<(), WaydroidError> {
        tracing::info!("stopping the session");
        self.wd.session_stop().await?;
        for _ in 0..80 {
            if stopped_for_handoff(&self.wd.status().await?) {return Ok(());}
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        Err(WaydroidError::Lifecycle("The previous Android session did not stop; refusing to attach a second session".into()))
    }
    /// One serialized handoff. Never trust the health loop's cached state here.
    pub async fn attach_display(&self,name:&str,owner:&str) -> Result<(),WaydroidError> {
        if !valid_display_owner(name,owner) {return Err(WaydroidError::Lifecycle("Invalid embedded display or owner".into()));}
        let mut lease=self.lifecycle.lock().await;
        let dir=std::env::var_os("XDG_RUNTIME_DIR").ok_or_else(||WaydroidError::Lifecycle("XDG_RUNTIME_DIR is unavailable".into()))?;
        use std::os::unix::fs::FileTypeExt;
        if !std::fs::metadata(std::path::Path::new(&dir).join(name)).is_ok_and(|m|m.file_type().is_socket()) {
            return Err(WaydroidError::Lifecycle("The embedded compositor socket is unavailable".into()));
        }
        self.start_generation.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
        self.stop_inner().await?;
        self.set_embedded_display(Some(name.into()));
        lease.0=Some(owner.into());
        if let Err(e)=self.start_inner().await {
            lease.0=None;self.set_embedded_display(None);return Err(e);
        }
        tracing::info!(display=name,owner,"Android attached to embedded window");
        Ok(())
    }
    pub async fn detach_display(&self,owner:&str) -> Result<(),WaydroidError> {
        let mut lease=self.lifecycle.lock().await;
        if !lease.release(owner) {return Ok(());}
        self.start_generation.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
        self.set_embedded_display(None);
        self.stop_inner().await
    }

    /// Is the composer HAL process alive?
    ///
    /// This is the first link of the crash chain — SurfaceFlinger getting
    /// DEAD_OBJECT comes after it. The process name varies by vendor image, so
    /// we pattern-match rather than compare exactly.
    async fn composer_alive(&self) -> bool {
        Command::new("pgrep")
            .args(["-f", "hardware.graphics.composer"])
            .stdout(Stdio::null()).stderr(Stdio::null())
            .status().await
            .map(|s| s.success()).unwrap_or(false)
    }

    /// Reads a process start time in jiffies (/proc/PID/stat, field 22).
    async fn start_time(pid: &str) -> Option<u64> {
        let stat = tokio::fs::read_to_string(format!("/proc/{pid}/stat")).await.ok()?;
        // The comm field may contain spaces; count from after the ')'.
        let rest = stat.rsplit_once(')')?.1;
        rest.split_whitespace().nth(19)?.parse().ok()
    }

    async fn newest_start(pattern: &str) -> Option<u64> {
        let out = Command::new("pgrep").args(["-f", pattern])
            .stderr(Stdio::null()).output().await.ok()?;
        let mut newest = None;
        for pid in String::from_utf8_lossy(&out.stdout).split_whitespace() {
            if let Some(t) = Self::start_time(pid).await {
                newest = Some(newest.map_or(t, |n: u64| n.max(t)));
            }
        }
        newest
    }

    /// Did composer start noticeably later than the session process?
    ///
    /// A threshold is needed: on a normal start composer appears a few seconds
    /// after the session. The problem is it being reborn LONG AFTERWARDS.
    /// 60 seconds is far above the normal startup delay.
    async fn composer_stale(&self) -> bool {
        const HZ: u64 = 100; // kernel USER_HZ
        const THRESHOLD_SEC: u64 = 60;
        let (Some(sess), Some(comp)) = (
            Self::newest_start("waydroid session start").await,
            Self::newest_start("hardware.graphics.composer").await,
        ) else { return false };
        comp.saturating_sub(sess) > THRESHOLD_SEC * HZ
    }

    pub async fn health(&self) -> Health {
        let st = self.wd.status().await.unwrap_or_default();
        let composer = self.composer_alive().await;
        let stale = composer && self.composer_stale().await;
        // boot_completed needs root. Ask the helper first (a REAL measurement);
        // without a helper try directly (usually fails because liwd does not run
        // as root); failing that, INFER from the session state.
        // Inference is the last resort: a false negative causes a pointless restart.
        let boot = match &self.helper {
            Some(h) => match h.boot_completed().await {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(error = %e, "helper boot query failed — inferring");
                    st.session_running()
                }
            },
            None => match self.wd.getprop("sys.boot_completed").await {
                Ok(v) => v.trim() == "1",
                Err(_) => st.session_running(),
            },
        };
        Health {
            session_running: st.session_running(),
            container_running: st.container_running(),
            has_ip: st.has_ip(),
            boot_completed: boot,
            composer_alive: composer,
            composer_stale: stale,
        }
    }

    pub async fn state(&self) -> SessionState {
        let h = self.health().await;
        if !h.session_running { return SessionState::Stopped; }
        if h.is_healthy() { SessionState::Running } else { SessionState::Degraded }
    }

    /// Recovers a broken session with a full cycle.
    ///
    /// Partial recovery does not work: once composer dies Android cannot
    /// recover from the inside; the session must be rebuilt completely.
    pub async fn recover(&self) -> Result<(), WaydroidError> {
        let _guard=self.lifecycle.lock().await;
        self.recover_inner().await
    }
    pub async fn recover_if_generation(&self,expected:u64) -> Result<(),WaydroidError> {
        let _guard=self.lifecycle.lock().await;
        if self.start_generation()!=expected {
            tracing::info!("skipping recovery based on health from a previous Android session");return Ok(());
        }
        self.recover_inner().await
    }
    async fn recover_inner(&self) -> Result<(),WaydroidError> {
        tracing::warn!("recovering session (full restart)");
        self.start_generation.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
        self.stop_inner().await?;
        self.start_inner().await
    }
}

#[derive(Default)]
struct DisplayOwner(Option<String>);
impl DisplayOwner {
    fn release(&mut self,owner:&str)->bool {
        if self.0.as_deref()!=Some(owner) {return false;}
        self.0=None;true
    }
}
fn valid_display_owner(name:&str,owner:&str)->bool {
    name.starts_with("wayland-") && name.len()<=100 && name.bytes().all(|b|b.is_ascii_alphanumeric() || b"-_".contains(&b))
        && !owner.is_empty() && owner.len()<=100 && owner.bytes().all(|b|b.is_ascii_alphanumeric() || b"-_".contains(&b))
}
fn stopped_for_handoff(status:&Status)->bool {
    status.session.eq_ignore_ascii_case("STOPPED")
        && (status.container.is_empty() || status.container.eq_ignore_ascii_case("STOPPED"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn handoff_waits_for_the_container_not_just_the_session() {
        let mut status=crate::waydroid::Status {session:"STOPPED".into(),container:"RUNNING".into(),..Default::default()};
        assert!(!super::stopped_for_handoff(&status));
        status.container="FROZEN".into();assert!(!super::stopped_for_handoff(&status));
        status.container="STOPPED".into();assert!(super::stopped_for_handoff(&status));
        status.session="RUNNING".into();assert!(!super::stopped_for_handoff(&status));
        assert!(!super::stopped_for_handoff(&Default::default()));
    }
    #[tokio::test]
    async fn lifecycle_commands_wait_and_stale_recovery_does_not_run_waydroid() {
        let sup=super::Supervisor::new(Default::default());
        sup.start_generation.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
        let guard=sup.lifecycle.lock().await;
        assert!(tokio::time::timeout(std::time::Duration::from_millis(20),sup.recover_if_generation(0)).await.is_err());
        drop(guard);
        sup.recover_if_generation(0).await.unwrap();
        assert_eq!(sup.start_generation(),1);
    }
    #[test]
    fn closing_an_old_window_cannot_detach_its_replacement() {
        let mut lease=super::DisplayOwner(Some("new-window".into()));
        assert!(!lease.release("old-window"));assert_eq!(lease.0.as_deref(),Some("new-window"));
        assert!(lease.release("new-window"));assert!(!lease.release("new-window"));
    }
    #[test]
    fn display_names_cannot_escape_the_runtime_directory() {
        assert!(super::valid_display_owner("wayland-liw","123-456"));
        assert!(!super::valid_display_owner("wayland-../../other","123"));
        assert!(!super::valid_display_owner("wayland-liw",""));
    }
    #[test]
    fn startup_is_not_recovered_after_three_five_second_polls() {
        let mut grace=super::BootGrace::default();
        let health=super::Health { session_running:true,..Default::default() };
        let now=std::time::Instant::now();
        for seconds in [0,5,10,15,179] {
            assert!(grace.update(&health,now+std::time::Duration::from_secs(seconds),std::time::Duration::from_secs(180)));
        }
        assert!(!grace.update(&health,now+std::time::Duration::from_secs(180),std::time::Duration::from_secs(180)));
    }
    #[test]
    fn runtime_boot_loss_has_no_new_grace() {
        let mut grace=super::BootGrace::default();
        let now=std::time::Instant::now();
        let mut health=super::Health { session_running:true,boot_completed:true,..Default::default() };
        assert!(!grace.update(&health,now,std::time::Duration::from_secs(180)));
        health.boot_completed=false;
        assert!(!grace.update(&health,now,std::time::Duration::from_secs(180)));
    }
    #[test]
    fn stopped_session_resets_startup_grace() {
        let mut grace=super::BootGrace::default();
        let now=std::time::Instant::now();
        let live=super::Health { session_running:true,boot_completed:true,..Default::default() };
        grace.update(&live,now,std::time::Duration::from_secs(180));
        grace.update(&super::Health::default(),now,std::time::Duration::from_secs(180));
        assert!(grace.update(&super::Health { session_running:true,..Default::default() },now,std::time::Duration::from_secs(180)));
    }
    /// Real measurement: with the HAL wedged the log fills with this line.
    #[test]
    fn detects_wedged_audio_from_real_log() {
        let real = "08-27 17:26:45.329  1086 20805 W AudioSystem: AudioFlinger not published, waiting...\n\
                    08-27 17:26:50.830  1086 20805 W AudioSystem: AudioFlinger not published, waiting...\n\
                    08-27 17:26:56.331  1086 20805 W AudioSystem: AudioFlinger not published, waiting...\n\
                    08-27 17:27:01.832  1086 20805 W AudioSystem: AudioFlinger not published, waiting...\n\
                    08-27 17:27:07.333  1086 20805 W AudioSystem: AudioFlinger not published, waiting...\n";
        assert!(super::audio_flinger_stalled(real));
    }

    /// Waiting a few times at boot is NORMAL — it must not panic.
    #[test]
    fn a_few_waits_during_boot_are_normal() {
        let boot = "W AudioSystem: AudioFlinger not published, waiting...\n\
                    I boot: devam\n\
                    W AudioSystem: AudioFlinger not published, waiting...\n";
        assert!(!super::audio_flinger_stalled(boot));
    }

    #[test]
    fn healthy_log_is_not_flagged() {
        assert!(!super::audio_flinger_stalled("I ActivityManager: all is well"));
        assert!(!super::audio_flinger_stalled(""));
    }

    use super::*;

    #[test]
    fn healthy_needs_every_signal() {
        let full = Health {
            session_running: true, container_running: true,
            has_ip: true, boot_completed: true,
            composer_alive: true, composer_stale: false,
        };
        assert!(full.is_healthy());
        assert!(full.failures().is_empty());
    }

    /// A real failure seen today: everything up but no IP (route lost).
    #[test]
    fn missing_ip_is_degraded_not_healthy() {
        let h = Health { has_ip: false, ..Health {
            session_running: true, container_running: true,
            has_ip: true, boot_completed: true,
            composer_alive: true, composer_stale: false } };
        assert!(!h.is_healthy());
        assert_eq!(h.failures(), vec!["no IP assigned"]);
        assert!(!h.requires_recovery());
    }

    /// Composer death must be reported separately: it is the root of the chain.
    #[test]
    fn dead_composer_is_reported_explicitly() {
        let h = Health {
            session_running: true, container_running: true,
            has_ip: true, boot_completed: true,
            composer_alive: false, composer_stale: false,
        };
        assert!(!h.is_healthy());
        assert!(h.failures().iter().any(|f| f.contains("composer")));
    }

    /// Real case: everything up but composer restarted after the session.
    /// No window appears and app launch says "Sending reply failed".
    /// The health check MUST catch this, or it misleads by reporting "healthy".
    #[test]
    fn stale_composer_is_not_healthy() {
        let h = Health {
            session_running: true, container_running: true,
            has_ip: true, boot_completed: true,
            composer_alive: true, composer_stale: true,
        };
        assert!(!h.is_healthy(), "a stale composer must not count as healthy");
        assert!(h.failures().iter().any(|f| f.contains("stale")), "{:?}", h.failures());
    }

    #[test]
    fn critical_failures_require_recovery() {
        let healthy = Health {
            session_running: true,
            container_running: true,
            has_ip: true,
            boot_completed: true,
            composer_alive: true,
            composer_stale: false,
        };

        assert!(!healthy.requires_recovery());
        assert!(!Health { has_ip: false, ..healthy.clone() }.requires_recovery());

        assert!(Health { session_running: false, ..healthy.clone() }.requires_recovery());
        assert!(Health { container_running: false, ..healthy.clone() }.requires_recovery());
        assert!(Health { boot_completed: false, ..healthy.clone() }.requires_recovery());
        assert!(Health { composer_alive: false, ..healthy.clone() }.requires_recovery());
        assert!(Health { composer_stale: true, ..healthy }.requires_recovery());
    }

    #[test]
    fn threshold_avoids_restarting_on_a_single_blip() {
        assert!(SupervisorConfig::default().unhealthy_threshold > 1);
    }
}
