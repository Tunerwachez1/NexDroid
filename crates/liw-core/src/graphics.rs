//! Portable DRM discovery and process GPU selection.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Gpu {
    pub name: String,
    pub id: String,
    pub node: PathBuf,
    pub vendor: String,
    pub device: String,
    pub driver: String,
    pub version: String,
}
impl Gpu {
    pub fn label(&self) -> String {
        let vendor = match self.vendor.as_str() {
            "0x10de" => "NVIDIA",
            "0x1002" => "AMD",
            "0x8086" => "Intel",
            _ => &self.vendor,
        };
        let name = if self.name.is_empty() {
            format!("{vendor} {}", self.device)
        } else {
            self.name.clone()
        };
        format!("{name} · {}", self.id)
    }
}
pub fn discover() -> Vec<Gpu> {
    discover_at(Path::new("/sys/class/drm"), Path::new("/dev/dri"))
}
fn discover_at(sys: &Path, dev: &Path) -> Vec<Gpu> {
    let Ok(entries) = std::fs::read_dir(sys) else {
        return vec![];
    };
    let mut result = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name
            .strip_prefix("renderD")
            .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        {
            continue;
        }
        let device = entry.path().join("device");
        let Ok(vendor) = std::fs::read_to_string(device.join("vendor")) else {
            continue;
        };
        let Ok(device_id) = std::fs::read_to_string(device.join("device")) else {
            continue;
        };
        let Ok(real) = device.canonicalize() else {
            continue;
        };
        let Some(id) = real.file_name().and_then(|s| s.to_str()).map(str::to_owned) else {
            continue;
        };
        let driver = device
            .join("driver")
            .canonicalize()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let version =
            std::fs::read_to_string(Path::new("/sys/module").join(&driver).join("version"))
                .or_else(|_| std::fs::read_to_string("/proc/sys/kernel/osrelease"))
                .unwrap_or_default()
                .trim()
                .to_owned();
        // pciutils is optional. Numeric IDs remain usable when it is absent.
        let model = std::process::Command::new("lspci")
            .args(["-Dmm", "-s", &id])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .split('"')
                    .nth(5)
                    .map(str::to_owned)
            })
            .unwrap_or_default();
        result.push(Gpu {
            name: model,
            id,
            node: dev.join(name),
            vendor: vendor.trim().to_ascii_lowercase(),
            device: device_id.trim().to_ascii_lowercase(),
            driver,
            version,
        });
    }
    result.sort_by(|a, b| a.id.cmp(&b.id));
    result
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphicsSettings {
    pub gpu: Option<String>,
}
impl GraphicsSettings {
    pub fn path() -> PathBuf {
        crate::Config::path().with_file_name("graphics.json")
    }
    pub fn load() -> Result<Self, String> {
        match std::fs::read_to_string(Self::path()) {
            Ok(s) => serde_json::from_str(&s).map_err(|e| format!("graphics.json: {e}")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.to_string()),
        }
    }
    pub fn select<'a>(
        &self,
        devices: &'a [Gpu],
        requires_nvidia: bool,
    ) -> Result<Option<&'a Gpu>, String> {
        let selected = match &self.gpu {
            Some(id) => Some(devices.iter().find(|d| &d.id == id).ok_or_else(|| {
                format!("Selected GPU {id} is unavailable. Choose Auto in Graphics settings.")
            })?),
            None if requires_nvidia => Some(
                devices
                    .iter()
                    .find(|d| d.vendor == "0x10de")
                    .ok_or("The selected Android backend requires an NVIDIA GPU")?,
            ),
            None => None,
        };
        if requires_nvidia && selected.is_some_and(|d| d.vendor != "0x10de") {
            return Err("The selected Android backend requires an NVIDIA GPU. Other Android backends retain their own GPU compatibility.".into());
        }
        Ok(selected)
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AndroidGraphics {
    pub gles: String,
    pub egl: String,
    pub vulkan: String,
    pub astc: Option<bool>,
}
impl AndroidGraphics {
    pub fn from_dump(dump: &str, egl: String, vulkan: String) -> Self {
        let gles = dump
            .lines()
            .find_map(|l| l.trim().strip_prefix("GLES:"))
            .unwrap_or_default()
            .trim()
            .to_owned();
        let astc = if dump.contains("GL_KHR_texture_compression_astc")
            || dump.contains("GL_OES_texture_compression_astc")
        {
            Some(true)
        } else if dump.contains("GL_EXTENSIONS:") || dump.contains("GL_EXTENSIONS=") {
            Some(false)
        } else {
            None
        };
        Self {
            gles,
            egl,
            vulkan,
            astc,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn gpu(id: &str, vendor: &str) -> Gpu {
        Gpu {
            name: String::new(),
            id: id.into(),
            vendor: vendor.into(),
            node: "/dev/dri/renderD128".into(),
            device: "0x1234".into(),
            driver: String::new(),
            version: String::new(),
        }
    }
    #[test]
    fn selection_preserves_auto_and_enforces_the_installed_backend() {
        let devices = vec![
            gpu("intel", "0x8086"),
            gpu("amd", "0x1002"),
            gpu("nvidia", "0x10de"),
        ];
        assert!(
            GraphicsSettings::default()
                .select(&devices, false)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            GraphicsSettings::default()
                .select(&devices, true)
                .unwrap()
                .unwrap()
                .id,
            "nvidia"
        );
        let manual = GraphicsSettings {
            gpu: Some("amd".into()),
        };
        assert_eq!(manual.select(&devices, false).unwrap().unwrap().id, "amd");
        assert!(manual.select(&devices, true).is_err());
        assert!(manual.select(&[], false).is_err());
    }
    #[test]
    fn absent_extension_listing_is_unknown_instead_of_unsupported() {
        let info = AndroidGraphics::from_dump(
            "GLES: Vendor, Renderer, OpenGL ES 3.2\n",
            "angle".into(),
            "virtio".into(),
        );
        assert_eq!(info.astc, None);
        assert!(info.gles.contains("Renderer"));
        assert_eq!(
            AndroidGraphics::from_dump(
                "GL_EXTENSIONS: GL_KHR_texture_compression_astc_ldr",
                String::new(),
                String::new()
            )
            .astc,
            Some(true)
        );
    }
}
