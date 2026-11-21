//! Display draft independent of CPU/RAM and host window geometry.
use std::path::Path;
pub const RESOLUTIONS: [(u32,u32); 6] = [(960,540),(1280,720),(1600,900),(1920,1080),(2560,1440),(3840,2160)];
#[derive(Clone, Debug)]
pub struct DisplaySettings { pub width: u32, pub height: u32, pub dpi: u32 }
impl DisplaySettings {
    pub fn load() -> anyhow::Result<Self> { Self::load_from(&crate::performance::directory()) }
    fn load_from(dir: &Path) -> anyhow::Result<Self> {
        let text = match std::fs::read_to_string(dir.join("display.json")) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::to_string(&liw_core::display::GuestDisplay::default())?,
            Err(e) => return Err(e.into()),
        };
        let mode = liw_core::display::GuestDisplay::parse(&text).map_err(anyhow::Error::msg)?;
        let value: serde_json::Value = serde_json::from_str(&text)?;
        let dpi = match value.get("dpi") {
            Some(v) => v.as_u64().filter(|n| *n <= 640).ok_or_else(|| anyhow::anyhow!("Invalid DPI"))? as u32,
            None => 0,
        };
        let result = Self { width: mode.width, height: mode.height, dpi };
        result.validate()?;
        Ok(result)
    }
    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!((320..=7680).contains(&self.width) && (320..=7680).contains(&self.height), "Resolution must be 320–7680 pixels");
        anyhow::ensure!(self.dpi == 0 || (120..=640).contains(&self.dpi), "DPI must be Auto or 120–640");
        Ok(())
    }
    pub fn portrait(&self) -> bool { self.height > self.width }
    pub fn orient(&mut self, portrait: bool) {
        if self.portrait() != portrait { std::mem::swap(&mut self.width, &mut self.height); }
    }
    pub fn preset(&mut self, index: usize) {
        if let Some(&(w,h)) = RESOLUTIONS.get(index) {
            (self.width,self.height) = if self.portrait() { (h,w) } else { (w,h) };
        }
    }
    pub fn preset_index(&self) -> Option<usize> {
        RESOLUTIONS.iter().position(|&(w,h)| (self.width,self.height) == (w,h) || (self.width,self.height) == (h,w))
    }
    pub fn save(&self) -> anyhow::Result<()> { self.save_in(&crate::performance::directory()) }
    fn save_in(&self, dir: &Path) -> anyhow::Result<()> {
        self.validate()?;
        std::fs::create_dir_all(dir)?;
        let path = dir.join("display.json");
        let mut value = match std::fs::read_to_string(&path) {
            Ok(text) => {
                liw_core::display::GuestDisplay::parse(&text).map_err(anyhow::Error::msg)?;
                serde_json::from_str::<serde_json::Value>(&text)?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::to_value(liw_core::display::GuestDisplay::default())?,
            Err(e) => return Err(e.into()),
        };
        value["width"] = self.width.into(); value["height"] = self.height.into(); value["dpi"] = self.dpi.into();
        crate::performance::atomic_json(&path, &value)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portrait_save_preserves_performance_and_rejects_invalid_dpi() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("display.json");
        std::fs::write(&path, r#"{"width":1920,"height":1080,"dpi":400,"fps":240,"cpus":6,"memory_mib":8192,"future":true}"#).unwrap();
        let mut draft = DisplaySettings::load_from(dir.path()).unwrap();
        draft.orient(true); draft.preset(1); draft.dpi = 240;
        draft.save_in(dir.path()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["width"],720); assert_eq!(v["height"],1280);
        assert_eq!(v["fps"],240); assert_eq!(v["cpus"],6); assert_eq!(v["memory_mib"],8192); assert_eq!(v["future"],true);
        let before = std::fs::read_to_string(&path).unwrap();
        draft.dpi = 1; assert!(draft.save_in(dir.path()).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(),before);
    }
}
