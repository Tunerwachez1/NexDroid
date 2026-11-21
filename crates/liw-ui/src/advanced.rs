//! Readback model for the helper's debugging settings.
use serde::Deserialize;
#[derive(Clone, Copy, Default, Deserialize)]
pub struct Settings {
    pub adb: bool,
    pub show_touches: bool,
    pub pointer_location: bool,
}
#[derive(Deserialize)]
pub struct RootState {
    pub available: bool,
    pub active: bool,
    pub requested: bool,
    pub pending: bool,
    pub manager_package: String,
}
#[derive(Deserialize)]
pub struct State {
    pub root: RootState,
    pub settings: Settings,
    pub abis: String,
    pub adb_secure: bool,
    pub adb_address: String,
}
pub async fn read() -> Result<State, String> {
    let helper = liw_core::HelperClient::connect()
        .await
        .map_err(|e| e.to_string())?;
    let raw = helper
        .advanced_settings()
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}
pub async fn save(settings: Settings, root: bool) -> Result<State, String> {
    let helper = liw_core::HelperClient::connect()
        .await
        .map_err(|e| e.to_string())?;
    helper
        .set_advanced_settings(
            settings.adb,
            settings.show_touches,
            settings.pointer_location,
        )
        .await
        .map_err(|e| e.to_string())?;
    helper
        .set_root_enabled(root)
        .await
        .map_err(|e| e.to_string())?;
    let raw = helper
        .advanced_settings()
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}
