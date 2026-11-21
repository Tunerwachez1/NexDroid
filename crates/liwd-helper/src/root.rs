//! Optional, reversible Magisk runtime. Private policies/keys are never copied.
use serde::{Deserialize, Serialize};
use std::{
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Mutex,
};
const DIR: &str = "/var/lib/nexdroid/root";
const OVERLAY: &str = "/var/lib/waydroid/overlay/system";
const BOOT: &str = "etc/nexdroid-root/bootstrap.sh";
const INIT: &str = "etc/init/nexdroid-root.rc";
static LOCK: Mutex<()> = Mutex::new(());
const FILES: [&str; 18] = [
    "magisk",
    "magisk32",
    "busybox",
    "magiskpolicy",
    "magiskinit",
    "magiskboot",
    "bootctl",
    "init-ld",
    "libinit-ld.so",
    "libinit-ld32.so",
    "addon.d.sh",
    "app_functions.sh",
    "boot_patch.sh",
    "module_installer.sh",
    "uninstaller.sh",
    "util_functions.sh",
    "stub.apk",
    "magisk.apk",
];
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    enabled: bool,
}
#[derive(Serialize)]
pub struct State {
    available: bool,
    active: bool,
    requested: bool,
    pending: bool,
    manager_package: String,
}
fn trusted(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .is_ok_and(|m| !m.file_type().is_symlink() && m.uid() == 0 && m.mode() & 0o022 == 0)
}
fn native_binary(bytes: &[u8]) -> bool {
    let machine = match std::env::consts::ARCH {
        "x86_64" => 62,
        "aarch64" => 183,
        _ => return false,
    };
    bytes.starts_with(b"\x7fELF\x02\x01") && bytes.get(18..20) == Some(&[machine, 0])
}
fn payload(path: &Path) -> bool {
    trusted(path)
        && ["magisk", "busybox", "stub.apk"]
            .iter()
            .all(|f| trusted(&path.join(f)))
        && std::fs::read(path.join("magisk")).is_ok_and(|b| native_binary(&b))
}
fn manager(path: &Path) -> String {
    let value = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
    value
        .as_ref()
        .and_then(|v| v["manager_package"].as_str())
        .filter(|s| valid_package(s))
        .unwrap_or("com.topjohnwu.magisk")
        .into()
}
fn valid_package(s: &str) -> bool {
    s.len() <= 160
        && s.contains('.')
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._".contains(&b))
}
fn source() -> Option<(PathBuf, String)> {
    let own = Path::new(DIR).join("runtime");
    if payload(&own) {
        return Some((own, manager(&Path::new(DIR).join("runtime.json"))));
    }
    let mut dirs = std::fs::read_dir("/var/lib/waydroid")
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("magisk-disabled-"))
        })
        .collect::<Vec<_>>();
    dirs.sort();
    for dir in dirs.into_iter().rev() {
        let p = dir.join("adb/magisk");
        if trusted(&dir) && payload(&p) && trusted(&dir.join("manifest.json")) {
            return Some((p, manager(&dir.join("manifest.json"))));
        }
    }
    None
}
fn requested() -> anyhow::Result<bool> {
    match std::fs::read_to_string(Path::new(DIR).join("settings.json")) {
        Ok(s) => Ok(serde_json::from_str::<Settings>(&s)?.enabled),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
fn atomic(path: &Path, bytes: &[u8], mode: u32) -> anyhow::Result<()> {
    use std::io::Write;
    anyhow::ensure!(
        !std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()),
        "Refusing symlink {}",
        path.display()
    );
    let parent = path.parent().unwrap();
    std::fs::create_dir_all(parent)?;
    anyhow::ensure!(trusted(parent), "Untrusted root runtime directory");
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.as_file()
        .set_permissions(std::fs::Permissions::from_mode(mode))?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}
fn ensure_runtime() -> anyhow::Result<(PathBuf, String)> {
    let (src, package) = source().ok_or_else(|| {
        anyhow::anyhow!(
            "Prepare the optional Magisk runtime first (scripts/prepare-root-runtime.py)"
        )
    })?;
    let dest = Path::new(DIR).join("runtime");
    std::fs::create_dir_all(&dest)?;
    std::fs::set_permissions(Path::new(DIR), std::fs::Permissions::from_mode(0o700))?;
    std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o700))?;
    if src != dest {
        for name in FILES {
            let p = src.join(name);
            if p.exists() {
                anyhow::ensure!(trusted(&p), "Untrusted Magisk payload file");
                atomic(
                    &dest.join(name),
                    &std::fs::read(p)?,
                    if name.ends_with(".apk") { 0o600 } else { 0o700 },
                )?;
            }
        }
    }
    atomic(
        &Path::new(DIR).join("runtime.json"),
        &serde_json::to_vec(&serde_json::json!({"manager_package":package}))?,
        0o600,
    )?;
    Ok((dest, package))
}
pub async fn state() -> anyhow::Result<State> {
    let requested = requested()?;
    let active = crate::run_dumpsys(&["pidof", "magiskd"])
        .await
        .is_ok_and(|s| !s.trim().is_empty());
    let package = source();
    Ok(State {
        available: package.is_some(),
        active,
        requested,
        pending: active != requested,
        manager_package: package.map(|(_, p)| p).unwrap_or_default(),
    })
}
pub fn stage(enabled: bool) -> anyhow::Result<()> {
    let _lock = LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("Root settings lock failed"))?;
    if enabled {
        ensure_runtime()?;
    }
    if requested()? == enabled {
        return Ok(());
    }
    atomic(
        &Path::new(DIR).join("settings.json"),
        &serde_json::to_vec_pretty(&Settings { enabled })?,
        0o600,
    )
}
pub async fn prepare() -> anyhow::Result<()> {
    if !Path::new(DIR).join("settings.json").exists() {
        return Ok(());
    }
    let out = tokio::process::Command::new("/usr/bin/lxc-info")
        .args(["-P", "/var/lib/waydroid/lxc", "-n", "waydroid", "-sH"])
        .output()
        .await?;
    anyhow::ensure!(
        out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "STOPPED",
        "Stop Android before applying root settings"
    );
    let _lock = LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("Root settings lock failed"))?;
    let enabled = requested()?;
    let package = manager(&Path::new(DIR).join("runtime.json"));
    let bootstrap = include_str!("root-bootstrap.sh").replace("@MANAGER@", &package);
    let overlay = Path::new(OVERLAY);
    for old in [
        "etc/init/waydroid-magisk.rc",
        "etc/waydroid-magisk/bootstrap.sh",
    ] {
        anyhow::ensure!(
            !overlay.join(old).exists(),
            "Another Magisk installation owns the Android hooks; manage it separately"
        );
    }
    // Touch only our two exact hooks. Keep all data and other overlays.
    for (name, expected) in [
        (BOOT, bootstrap.as_bytes()),
        (INIT, include_bytes!("root-init.rc").as_slice()),
    ] {
        let p = overlay.join(name);
        if p.exists() {
            anyhow::ensure!(
                trusted(&p) && std::fs::read(&p)? == expected,
                "Root hook changed outside NexDroid: {name}"
            );
        }
    }
    if !enabled {
        for name in [INIT, BOOT] {
            let p = overlay.join(name);
            if p.exists() {
                std::fs::remove_file(p)?;
            }
        }
        return Ok(());
    }
    let (runtime, _) = ensure_runtime()?;
    let target = overlay.join("etc/nexdroid-root/payload");
    std::fs::create_dir_all(&target)?;
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700))?;
    for name in FILES {
        let src = runtime.join(name);
        if src.exists() {
            atomic(
                &target.join(name),
                &std::fs::read(src)?,
                if name.ends_with(".apk") { 0o600 } else { 0o700 },
            )?;
        }
    }
    atomic(&overlay.join(BOOT), bootstrap.as_bytes(), 0o700)?;
    atomic(&overlay.join(INIT), include_bytes!("root-init.rc"), 0o644)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn package_names_cannot_inject_into_the_bootstrap_sql() {
        assert!(super::valid_package("com.topjohnwu.magisk"));
        assert!(!super::valid_package("a.b';DROP TABLE policies;"));
    }
    #[test]
    fn foreign_or_non_elf_payloads_are_rejected() {
        assert!(!super::native_binary(b"a shell script"));
        let mut bytes = vec![0u8; 20];
        bytes[..6].copy_from_slice(b"\x7fELF\x02\x01");
        bytes[18] = if std::env::consts::ARCH == "x86_64" {
            183
        } else {
            62
        };
        assert!(!super::native_binary(&bytes));
    }
}
