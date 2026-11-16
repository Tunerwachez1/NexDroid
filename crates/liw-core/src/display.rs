//! Persistent native Android mode, independent of the host view's render size.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GuestDisplay {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

impl Default for GuestDisplay {
    fn default() -> Self { Self { width: 1280, height: 720, fps: 60 } }
}

impl GuestDisplay {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mode: Self = serde_json::from_str(text).map_err(|e| format!("display.json: {e}"))?;
        if !(320..=7680).contains(&mode.width) || !(320..=7680).contains(&mode.height)
            || !(24..=360).contains(&mode.fps) {
            return Err("display.json: dimensions must be 320..7680, refresh 24..360 Hz".into());
        }
        Ok(mode)
    }

    pub fn load() -> Result<Self, String> {
        let path = crate::Config::path().with_file_name("display.json");
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("{}: {e}",path.display())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_exported_host_profile_without_resetting_native_mode() {
        assert_eq!(GuestDisplay::parse(r#"{"width":1920,"height":1080,"fps":240,"dpi":400,"cpus":0,"memory_mib":0}"#).unwrap(),
                   GuestDisplay { width:1920,height:1080,fps:240 });
    }
    #[test]
    fn invalid_or_partial_numbers_are_not_silently_defaulted() {
        for text in [r#"{"width":0}"#,r#"{"fps":10000}"#,r#"{"fps":"240"}"#,"broken"] {
            assert!(GuestDisplay::parse(text).is_err());
        }
    }
    #[test]
    fn portrait_is_a_real_native_mode() {
        assert_eq!(GuestDisplay::parse(r#"{"width":1080,"height":1920,"fps":144}"#).unwrap().height,1920);
    }
}
