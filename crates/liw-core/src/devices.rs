//! User-session PulseAudio routing and metadata-only V4L2 discovery.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, path::PathBuf, time::Duration};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeviceSettings {
    pub audio_sink: Option<String>,
    pub audio_source: Option<String>,
    pub camera: String,
}
impl Default for DeviceSettings {
    fn default() -> Self {
        Self {
            audio_sink: None,
            audio_source: None,
            camera: "auto".into(),
        }
    }
}
impl DeviceSettings {
    pub fn path() -> PathBuf {
        crate::Config::path().with_file_name("devices.json")
    }
    pub fn load() -> Result<Self, String> {
        match std::fs::read_to_string(Self::path()) {
            Ok(text) => {
                let settings: Self =
                    serde_json::from_str(&text).map_err(|e| format!("devices.json: {e}"))?;
                settings.validate()?;
                Ok(settings)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.to_string()),
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        for name in [&self.audio_sink, &self.audio_source].into_iter().flatten() {
            if name.is_empty() || name.len() > 512 || name.chars().any(char::is_control) {
                return Err("Invalid audio device name".into());
            }
        }
        if !valid_camera_id(&self.camera) {
            return Err("Invalid camera identifier".into());
        }
        Ok(())
    }
}
pub fn valid_camera_id(id: &str) -> bool {
    if matches!(id, "auto" | "off") {
        return true;
    }
    if let Some(name) = id.strip_prefix("by-id:") {
        return !name.is_empty()
            && name.len() <= 255
            && !matches!(name, "." | "..")
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.:+-".contains(&b));
    }
    id.strip_prefix("video").is_some_and(|n| {
        !n.is_empty()
            && n.bytes().all(|b| b.is_ascii_digit())
            && n.parse::<u32>().is_ok_and(|n| n < 256)
    })
}

#[derive(Clone, Debug, Default)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub index: u32,
}
#[derive(Clone, Debug)]
pub struct Camera {
    pub id: String,
    pub name: String,
    pub node: PathBuf,
}
#[derive(Clone, Debug, Default)]
pub struct Inventory {
    pub sinks: Vec<AudioDevice>,
    pub sources: Vec<AudioDevice>,
    pub cameras: Vec<Camera>,
    pub audio_error: String,
}
pub async fn inventory() -> Inventory {
    let mut result = Inventory {
        cameras: cameras(),
        ..Default::default()
    };
    match audio_devices("sinks").await {
        Ok(v) => result.sinks = v,
        Err(e) => result.audio_error = e,
    }
    match audio_devices("sources").await {
        Ok(v) => result.sources = v,
        Err(e) => result.audio_error = e,
    }
    result
}
async fn pactl(args: &[&str]) -> Result<String, String> {
    let mut command = tokio::process::Command::new("pactl");
    command.args(args).kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(5), command.output())
        .await
        .map_err(|_| "PulseAudio request timed out".to_owned())?
        .map_err(|e| format!("pactl: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr)
            .trim()
            .chars()
            .take(600)
            .collect());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
async fn pulse_list(kind: &str) -> Result<Vec<Value>, String> {
    serde_json::from_str(&pactl(&["-f", "json", "list", kind]).await?).map_err(|e| e.to_string())
}
async fn audio_devices(kind: &str) -> Result<Vec<AudioDevice>, String> {
    let entries = pulse_list(kind).await?;
    Ok(entries
        .iter()
        .filter_map(|entry| {
            let id = entry["name"].as_str()?.to_owned();
            let index = u32::try_from(entry["index"].as_u64()?).ok()?;
            let name = entry["description"]
                .as_str()
                .or_else(|| entry["properties"]["device.description"].as_str())
                .unwrap_or(&id)
                .to_owned();
            Some(AudioDevice { id, name, index })
        })
        .collect())
}
fn is_waydroid(stream: &Value) -> bool {
    let p = &stream["properties"];
    if p["application.process.host"].as_str() != Some("waydroid") {
        return false;
    }
    let name = p["application.name"].as_str().unwrap_or_default();
    let binary = p["application.process.binary"]
        .as_str()
        .unwrap_or_default()
        .rsplit('/')
        .next()
        .unwrap_or_default();
    (name == "Waydroid"
        && p["application.process.machine_id"] == "waydroid"
        && matches!(
            p["media.name"].as_str(),
            Some("ALSA Playback" | "ALSA Capture")
        ))
        || (matches!(
            binary,
            "android.hardware.audio.service" | "android.hardware.audio@2.0-service" | "audioserver"
        ) && (name.starts_with("ALSA") || name == binary || name == "AudioFlinger"))
}
async fn owned_session() -> Result<bool, String> {
    let conn = zbus::Connection::system()
        .await
        .map_err(|e| e.to_string())?;
    let proxy = zbus::Proxy::new(
        &conn,
        "id.waydro.Container",
        "/ContainerManager",
        "id.waydro.ContainerManager",
    )
    .await
    .map_err(|e| e.to_string())?;
    let session: HashMap<String, String> =
        tokio::time::timeout(Duration::from_secs(3), proxy.call("GetSession", &()))
            .await
            .map_err(|_| "Waydroid session query timed out".to_owned())?
            .map_err(|e| e.to_string())?;
    // SAFETY: getuid has no preconditions.
    let uid = unsafe { libc::getuid() }.to_string();
    Ok(session.get("state").is_some_and(|s| s == "RUNNING")
        && session.get("user_id") == Some(&uid)
        && session
            .get("wayland_display")
            .is_some_and(|s| s == "wayland-liw"))
}
/// Moves only this user's Waydroid streams, including newly recreated streams.
pub async fn route(settings: &DeviceSettings) -> Result<u32, String> {
    settings.validate()?;
    if !owned_session().await? {
        return Ok(0);
    }
    let mut moved = 0;
    for (selected, devices, streams, field, default, action) in [
        (
            &settings.audio_sink,
            "sinks",
            "sink-inputs",
            "sink",
            "get-default-sink",
            "move-sink-input",
        ),
        (
            &settings.audio_source,
            "sources",
            "source-outputs",
            "source",
            "get-default-source",
            "move-source-output",
        ),
    ] {
        let inventory = audio_devices(devices).await?;
        let fallback = pactl(&[default]).await?;
        let target = selected
            .as_ref()
            .and_then(|id| inventory.iter().find(|d| &d.id == id))
            .or_else(|| inventory.iter().find(|d| d.id == fallback.trim()));
        let Some(target) = target else {
            continue;
        };
        for stream in pulse_list(streams).await?.iter().filter(|s| is_waydroid(s)) {
            if stream[field].as_u64() == Some(target.index as u64) {
                continue;
            }
            if let Some(index) = stream["index"].as_u64() {
                pactl(&[action, &index.to_string(), &target.index.to_string()]).await?;
                moved += 1;
            }
        }
    }
    Ok(moved)
}

pub fn cameras() -> Vec<Camera> {
    use std::os::unix::fs::OpenOptionsExt;
    #[repr(C)]
    #[derive(Default)]
    struct Capability {
        driver: [u8; 16],
        card: [u8; 32],
        bus: [u8; 32],
        version: u32,
        caps: u32,
        device_caps: u32,
        reserved: [u32; 3],
    }
    let mut aliases = HashMap::new();
    if let Ok(entries) = std::fs::read_dir("/dev/v4l/by-id") {
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            if let Ok(node) = entry.path().canonicalize() {
                let id = format!("by-id:{}", entry.file_name().to_string_lossy());
                if valid_camera_id(&id) {
                    aliases.entry(node).or_insert(id);
                }
            }
        }
    }
    let Ok(entries) = std::fs::read_dir("/sys/class/video4linux") else {
        return vec![];
    };
    let mut result = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !valid_camera_id(&name) || !name.starts_with("video") {
            continue;
        }
        let node = PathBuf::from("/dev").join(&name);
        let Ok(file) = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(&node)
        else {
            continue;
        };
        let mut cap = Capability::default();
        use std::os::fd::AsRawFd;
        // SAFETY: VIDIOC_QUERYCAP writes one initialized 104-byte v4l2_capability.
        // This metadata ioctl does not start camera streaming.
        if unsafe { libc::ioctl(file.as_raw_fd(), 0x80685600u64 as libc::c_ulong, &mut cap) } < 0 {
            continue;
        }
        let caps = if cap.caps & 0x80000000 != 0 {
            cap.device_caps
        } else {
            cap.caps
        };
        if caps & (1 | 0x1000) == 0 {
            continue;
        }
        let model = String::from_utf8_lossy(cap.card.split(|b| *b == 0).next().unwrap_or_default())
            .into_owned();
        let id = aliases.remove(&node).unwrap_or(name);
        result.push(Camera {
            id,
            name: model,
            node,
        });
    }
    result.sort_by(|a, b| a.id.cmp(&b.id));
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn routing_does_not_match_other_linux_applications() {
        let mut stream = serde_json::json!({"properties":{"application.name":"Waydroid","application.process.host":"waydroid","application.process.machine_id":"waydroid","media.name":"ALSA Playback"}});
        assert!(is_waydroid(&stream));
        stream["properties"]["application.process.host"] = "desktop".into();
        assert!(!is_waydroid(&stream));
        assert!(!is_waydroid(
            &serde_json::json!({"properties":{"application.name":"Firefox"}})
        ));
    }
    #[test]
    fn camera_ids_reject_paths_and_non_capture_index_ranges() {
        for id in [
            "auto",
            "off",
            "video0",
            "video255",
            "by-id:usb-Webcam-video-index0",
        ] {
            assert!(valid_camera_id(id));
        }
        for id in [
            "/dev/video0",
            "video256",
            "by-id:../../etc/passwd",
            "by-id:..",
            "video-1",
        ] {
            assert!(!valid_camera_id(id));
        }
    }
}
