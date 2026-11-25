//! Metadata-only IME events. No shell calls or socket IO on the UI thread.
use std::{
    io::{BufRead, BufReader, Write},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{FileTypeExt, MetadataExt, PermissionsExt},
            net::UnixListener,
        },
    },
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[derive(Clone, Debug, serde::Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub package: String,
    pub active: bool,
}
impl Event {
    fn parse(line: &str) -> Option<Self> {
        let event: Self = serde_json::from_str(line).ok()?;
        (event.package.len() <= 200
            && !event.package.is_empty()
            && event
                .package
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_'))
        .then_some(event)
    }
}
pub fn effective(
    event: Option<&Event>,
    package: &str,
    enabled: bool,
    manual: Option<bool>,
) -> bool {
    manual.unwrap_or_else(|| enabled && event.is_some_and(|e| e.active && e.package == package))
}
pub struct Monitor {
    state: Arc<Mutex<Option<Arc<Event>>>>,
    stopped: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
    path: std::path::PathBuf,
    client: Arc<Mutex<Option<std::os::unix::net::UnixStream>>>,
}
impl Monitor {
    pub fn start() -> anyhow::Result<Self> {
        let dir = std::path::PathBuf::from(
            std::env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("HOME missing"))?,
        )
        .join(".local/share/waydroid/data/local/tmp/waydroid-hardware-keyboard");
        let info = std::fs::symlink_metadata(&dir)?;
        let uid = unsafe { libc::getuid() };
        anyhow::ensure!(
            info.is_dir() && info.uid() == uid && info.gid() >= 10000 && info.mode() & 0o027 == 0,
            "Untrusted IME bridge directory"
        );
        let app_uid = info.gid();
        let path = dir.join("focus.sock");
        if let Ok(old) = std::fs::symlink_metadata(&path) {
            anyhow::ensure!(
                old.file_type().is_socket() && old.uid() == uid,
                "Untrusted focus socket"
            );
            // Only replace a stale socket, never another live owner.
            anyhow::ensure!(
                std::os::unix::net::UnixStream::connect(&path).is_err(),
                "IME bridge already running"
            );
            std::fs::remove_file(&path)?;
        }
        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666))?;
        listener.set_nonblocking(true)?;
        let state = Arc::new(Mutex::new(None));
        let stopped = Arc::new(AtomicBool::new(false));
        let shared = state.clone();
        let stop = stopped.clone();
        let client = Arc::new(Mutex::new(None));
        let connected = client.clone();
        let worker = std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let Ok((stream, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                };
                let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
                let mut len = std::mem::size_of_val(&cred) as libc::socklen_t;
                let result = unsafe {
                    libc::getsockopt(
                        stream.as_raw_fd(),
                        libc::SOL_SOCKET,
                        libc::SO_PEERCRED,
                        &mut cred as *mut _ as *mut _,
                        &mut len,
                    )
                };
                if result != 0 || cred.uid != app_uid {
                    continue;
                }
                *connected.lock().unwrap() = stream.try_clone().ok();
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));
                let mut reader = BufReader::new(stream);
                while !stop.load(Ordering::Relaxed) {
                    // Bound allocation even for a malformed client.
                    let mut line = Vec::new();
                    let mut too_long = false;
                    loop {
                        let Ok(bytes) = reader.fill_buf() else {
                            too_long = true;
                            break;
                        };
                        if bytes.is_empty() {
                            too_long = true;
                            break;
                        }
                        let count = bytes
                            .iter()
                            .position(|b| *b == b'\n')
                            .map(|n| n + 1)
                            .unwrap_or(bytes.len());
                        if line.len() + count > 512 {
                            too_long = true;
                            break;
                        }
                        line.extend_from_slice(&bytes[..count]);
                        reader.consume(count);
                        if line.last() == Some(&b'\n') {
                            break;
                        }
                    }
                    if too_long {
                        break;
                    }
                    let Some(event) = std::str::from_utf8(&line).ok().and_then(Event::parse) else {
                        break;
                    };
                    *shared.lock().unwrap() = Some(Arc::new(event));
                    // ACK guarantees the host has the state before the IME callback returns.
                    if reader.get_mut().write_all(b"1\n").is_err() {
                        break;
                    }
                }
                // Keep the last known mode on disconnect; do not enable game binds in a chat.
                connected.lock().unwrap().take();
            }
        });
        Ok(Self {
            state,
            stopped,
            worker: Some(worker),
            path,
            client,
        })
    }
    pub fn snapshot(&self) -> Option<Arc<Event>> {
        self.state.lock().unwrap().clone()
    }
}
impl Drop for Monitor {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        if let Some(client) = self.client.lock().unwrap().as_ref() {
            let _ = client.shutdown(std::net::Shutdown::Both);
        }
        // Unblock any read before joining, so a replacement can bind immediately.
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_only_focus_metadata() {
        assert!(
            Event::parse(r#"{"package":"com.axlebolt.standoff2","active":true}"#)
                .unwrap()
                .active
        );
        assert!(Event::parse(r#"{"package":"com.x","active":false,"text":"secret"}"#).is_none());
        assert!(Event::parse(r#"{"package":"../bad","active":true}"#).is_none());
    }
    #[test]
    fn routes_only_the_active_game_and_honors_manual_override() {
        let mut e = Event {
            package: "com.game".into(),
            active: true,
        };
        assert!(effective(Some(&e), "com.game", true, None));
        assert!(!effective(Some(&e), "com.other", true, None));
        assert!(!effective(Some(&e), "com.game", false, None));
        assert!(!effective(Some(&e), "com.game", true, Some(false)));
        e.active = false;
        assert!(!effective(Some(&e), "com.game", true, None));
        assert!(effective(None, "com.game", true, Some(true)));
    }
}
