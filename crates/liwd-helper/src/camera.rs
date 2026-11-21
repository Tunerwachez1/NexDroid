//! External-camera HAL configuration, preserving the original image settings.
use serde::{Deserialize, Serialize};
use std::path::Path;
const STATE: &str = "/var/lib/nexdroid/camera/original.json";
const OVERLAY: &str = "/var/lib/waydroid/overlay/vendor/etc/external_camera_config.xml";
const CONFIG: &str = "/var/lib/waydroid/rootfs/vendor/etc/external_camera_config.xml";

#[derive(Serialize, Deserialize)]
struct State {
    original_xml: String,
    original_overlay: Option<String>,
    last_written: Option<String>,
    overlay_mode: u32,
}

pub fn select(selection: &str) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    anyhow::ensure!(
        liw_core::devices::valid_camera_id(selection),
        "Invalid camera identifier"
    );
    let state_path = Path::new(STATE);
    if selection == "auto" && !state_path.exists() {
        return Ok(());
    }
    let current = optional_text(Path::new(OVERLAY))?;
    let current_mode = std::fs::metadata(OVERLAY)
        .map(|m| m.permissions().mode() & 0o777)
        .unwrap_or(0o644);
    anyhow::ensure!(
        !std::fs::symlink_metadata(OVERLAY).is_ok_and(|m| m.file_type().is_symlink()),
        "Camera overlay is a symlink; preserve and configure it manually before using this setting"
    );
    let mut state = if state_path.exists() {
        serde_json::from_str::<State>(&std::fs::read_to_string(state_path)?)?
    } else {
        let original_xml = std::fs::read_to_string(CONFIG)
            .map_err(|e|anyhow::anyhow!("External camera HAL config is unavailable. Start Android once before choosing a camera: {e}"))?;
        // Parsing is required before any persistent state or overlay is written.
        camera_xml(&original_xml, None)?;
        let overlay_mode = std::fs::metadata(OVERLAY)
            .map(|m| m.permissions().mode() & 0o777)
            .unwrap_or(0o644);
        let state = State {
            original_xml,
            original_overlay: current.clone(),
            last_written: current.clone(),
            overlay_mode,
        };
        std::fs::create_dir_all(state_path.parent().unwrap())?;
        atomic(state_path, &serde_json::to_string_pretty(&state)?, 0o600)?;
        state
    };
    anyhow::ensure!(
        current == state.last_written,
        "Camera overlay changed outside NexDroid. Original configuration is preserved in {STATE}; reconcile the external changes before applying."
    );
    let desired = if selection == "auto" {
        state.original_overlay.clone()
    } else {
        let chosen = if selection == "off" {
            None
        } else {
            liw_core::devices::cameras()
                .into_iter()
                .find(|c| c.id == selection)
                .and_then(|c| {
                    c.node
                        .file_name()
                        .and_then(|n| n.to_str())
                        .and_then(|n| n.strip_prefix("video"))
                        .and_then(|n| n.parse::<u32>().ok())
                })
        };
        // An unplugged selection exposes no external camera, without blocking boot.
        Some(camera_xml(&state.original_xml, chosen)?)
    };
    let mode = if selection == "auto" {
        state.overlay_mode
    } else {
        0o644
    };
    write_overlay(desired.as_deref(), mode)?;
    state.last_written = desired;
    if let Err(e) = atomic(state_path, &serde_json::to_string_pretty(&state)?, 0o600) {
        write_overlay(current.as_deref(), current_mode)
            .map_err(|rollback| anyhow::anyhow!("{e}; rollback failed: {rollback}"))?;
        return Err(e);
    }
    Ok(())
}
fn optional_text(path: &Path) -> anyhow::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
fn atomic(path: &Path, text: &str, mode: u32) -> anyhow::Result<()> {
    use std::{io::Write, os::unix::fs::PermissionsExt};
    std::fs::create_dir_all(path.parent().unwrap())?;
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    file.write_all(text.as_bytes())?;
    file.as_file()
        .set_permissions(std::fs::Permissions::from_mode(mode))?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}
fn write_overlay(text: Option<&str>, mode: u32) -> anyhow::Result<()> {
    if let Some(text) = text {
        atomic(Path::new(OVERLAY), text, mode)
    } else {
        match std::fs::remove_file(OVERLAY) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}
fn camera_xml(source: &str, chosen: Option<u32>) -> anyhow::Result<String> {
    let doc = roxmltree::Document::parse(source)?;
    let provider = doc
        .descendants()
        .find(|n| n.has_tag_name("Provider"))
        .ok_or_else(|| anyhow::anyhow!("External camera config has no Provider"))?;
    let ignore = provider.children().find(|n| n.has_tag_name("ignore"));
    let mut ids = std::collections::BTreeSet::new();
    if let Some(ignore) = ignore {
        for id in ignore.children().filter(|n| n.has_tag_name("id")) {
            ids.insert(id.text().unwrap_or_default().trim().parse::<u32>()?);
        }
    }
    anyhow::ensure!(
        !chosen.is_some_and(|id| ids.contains(&id)),
        "This camera is excluded by the Android image configuration"
    );
    ids.extend((0..256).filter(|id| Some(*id) != chosen));
    let xml = format!(
        "<ignore>{}</ignore>",
        ids.into_iter()
            .map(|id| format!("<id>{id}</id>"))
            .collect::<String>()
    );
    let mut result = source.to_owned();
    if let Some(ignore) = ignore {
        result.replace_range(ignore.range(), &xml);
    } else {
        let range = provider.range();
        let offset = source[range.clone()]
            .rfind("</Provider>")
            .ok_or_else(|| anyhow::anyhow!("Unsupported Provider XML structure"))?;
        result.insert_str(range.start + offset, &xml);
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selecting_camera_preserves_device_tuning_and_existing_exclusions() {
        let source = "<ExternalCamera><Provider><ignore><id>4</id></ignore></Provider><Device><NumVideoBuffers count=\"4\"/></Device></ExternalCamera>";
        let changed = camera_xml(source, Some(2)).unwrap();
        assert!(changed.contains("<NumVideoBuffers count=\"4\"/>"));
        let doc = roxmltree::Document::parse(&changed).unwrap();
        let ids: Vec<_> = doc
            .descendants()
            .filter(|n| n.has_tag_name("id"))
            .map(|n| n.text().unwrap())
            .collect();
        assert!(!ids.contains(&"2"));
        assert!(ids.contains(&"4"));
        assert_eq!(ids.len(), 255);
        assert!(camera_xml(source, Some(4)).is_err());
        assert!(camera_xml("<ExternalCamera/>", None).is_err());
    }
}
