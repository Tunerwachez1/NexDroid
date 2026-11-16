//! One compositor owner across desktop launches. Requests stay on the local user socket.
use std::{fs::{File, OpenOptions}, os::unix::{fs::{FileTypeExt,PermissionsExt},net::UnixDatagram},path::{Path,PathBuf},sync::{Arc,atomic::{AtomicBool,Ordering},mpsc},time::Duration};

#[derive(Debug,Clone,serde::Serialize,serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request { pub package: Option<String> }

/// Serialize app launches; the view owns the latest requested package.
#[derive(Default)]
pub struct LaunchGate { sending: Option<String> }
impl LaunchGate {
    pub fn begin(&mut self, package: &str) -> bool {
        if self.sending.is_some() { return false; }
        self.sending=Some(package.to_owned());
        true
    }
    pub fn finish(&mut self, latest: &str) -> bool {
        self.sending.take().is_some_and(|sent|sent!=latest)
    }
}
impl Request {
    fn valid(&self) -> bool {
        self.package.as_ref().is_none_or(|p|!p.is_empty() && p.len()<=255 && p.contains('.')
            && p.bytes().all(|b|b.is_ascii_alphanumeric() || b==b'.' || b==b'_'))
    }
}

pub struct Instance { _lock: File, path: PathBuf, stop: Arc<AtomicBool> }
impl Drop for Instance {
    fn drop(&mut self) {
        self.stop.store(true,Ordering::Relaxed);
        let _=std::fs::remove_file(&self.path);
    }
}

pub fn acquire(dir: &Path, request: Request) -> anyhow::Result<Option<(Instance,mpsc::Receiver<Request>)>> {
    anyhow::ensure!(request.valid(),"Invalid Android package");
    let path=dir.join("liwinux-ui.commands");
    let lock=OpenOptions::new().create(true).truncate(false).read(true).write(true).open(dir.join("liwinux-ui.lock"))?;
    if let Err(error)=lock.try_lock() {
        if !matches!(error,std::fs::TryLockError::WouldBlock) { return Err(error.into()); }
        let bytes=serde_json::to_vec(&request)?;
        let client=UnixDatagram::unbound()?;
        for _ in 0..20 {
            if client.send_to(&bytes,&path).is_ok() {
                println!("Request sent to the existing Rust window");
                return Ok(None);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        anyhow::bail!("The Rust interface is already running, but its command socket is unavailable");
    }
    if let Ok(meta)=std::fs::symlink_metadata(&path) {
        anyhow::ensure!(meta.file_type().is_socket(),"Refusing to replace a non-socket command path");
        std::fs::remove_file(&path)?;
    }
    let socket=UnixDatagram::bind(&path)?;
    std::fs::set_permissions(&path,std::fs::Permissions::from_mode(0o600))?;
    socket.set_read_timeout(Some(Duration::from_millis(100)))?;
    let stop=Arc::new(AtomicBool::new(false));
    let instance=Instance { _lock:lock,path,stop:stop.clone() };
    let (tx,rx)=mpsc::sync_channel(16);
    std::thread::Builder::new().name("liw-ui-commands".into()).spawn(move|| {
        let mut buffer=[0u8;1024];
        while !stop.load(Ordering::Relaxed) {
            if let Ok(n)=socket.recv(&mut buffer) {
                if let Ok(request)=serde_json::from_slice::<Request>(&buffer[..n]) {
                    if request.valid() { let _=tx.try_send(request); }
                }
            }
        }
    })?;
    Ok(Some((instance,rx)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rapid_app_switches_cannot_launch_an_older_request_last() {
        let mut gate=LaunchGate::default();
        assert!(gate.begin("com.first"));
        assert!(!gate.begin("com.middle"));
        assert!(!gate.begin("com.latest"));
        assert!(gate.finish("com.latest"));
        assert!(gate.begin("com.latest"));
        assert!(!gate.finish("com.latest"));
        assert!(gate.begin("com.latest"));
    }
    #[test]
    fn second_launch_forwards_instead_of_acquiring_another_compositor() {
        let dir=std::env::temp_dir().join(format!("liw-instance-test-{}",std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (owner,rx)=acquire(&dir,Request { package:None }).unwrap().unwrap();
        let request=Request { package:Some("com.android.settings".into()) };
        assert!(acquire(&dir,request).unwrap().is_none());
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap().package.as_deref(),Some("com.android.settings"));
        drop(owner);
        assert!(!dir.join("liwinux-ui.commands").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn rejects_option_and_path_payloads() {
        for p in ["--help","../secret","com.game;bad",""] { assert!(!Request { package:Some(p.into()) }.valid()); }
    }
}
