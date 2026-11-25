//! Actionable exact-version preflight; the system's full hardware gate remains authoritative.
fn selected(cfg: &str, name: &str) -> Option<String> {
    let mut section = "";
    for line in cfg.lines().map(str::trim) {
        if line.starts_with('[') && line.ends_with(']') {
            section = &line[1..line.len()-1];
        } else if section == "waydroid" {
            if let Some((key,value)) = line.split_once('=') {
                if key.trim() == name { return Some(value.trim().to_owned()); }
            }
        }
    }
    None
}

pub fn validate_versions(module: &str, manifest: &str) -> Result<(), String> {
    let line = module.lines().find(|s|s.starts_with("NVRM version:"))
        .ok_or("NVIDIA: cannot identify loaded module")?;
    let loaded = line.split_whitespace().find(|word| {
        word.contains('.') && word.bytes().all(|b|b.is_ascii_digit() || b==b'.')
    }).ok_or("NVIDIA: cannot identify loaded version")?;
    let json: serde_json::Value = serde_json::from_str(manifest).map_err(|e|format!("NVIDIA gate record: {e}"))?;
    let gated = json.get("kernel_module").and_then(|v|v.as_str())
        .ok_or("NVIDIA gate record has no kernel_module")?;
    if loaded != gated {
        return Err(format!("NVIDIA {loaded}: Android backend was gated on {gated}. Prepare and gate an exact-profile update before starting (nvidia-compat/README.md)."));
    }
    Ok(())
}

pub fn preflight() -> Result<(), String> {
    let cfg = match std::fs::read_to_string("/var/lib/waydroid/waydroid.cfg") {
        Ok(cfg) => cfg,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("Cannot read Waydroid configuration: {e}")),
    };
    if !source_native_selected(&cfg) {
        return Ok(()); // Other modes retain their own installed startup guards.
    }
    let module = std::fs::read_to_string("/proc/driver/nvidia/version").map_err(|e|e.to_string())?;
    let manifest = std::fs::read_to_string("/var/lib/waydroid/nvidia-prolapse/source-rm.json")
        .map_err(|e|format!("Cannot read NVIDIA source-root gate record: {e}"))?;
    validate_versions(&module,&manifest)
}

/// All NVIDIA Android backends need the same GPU for the host window/importer.
/// This does not broaden the exact-version preflight of the source RM backend.
pub fn graphics_requires_nvidia(cfg: &str) -> bool {
    matches!(selected(cfg, "nvidia_driver_mode").as_deref(),
        Some("nvidia-prolapse" | "glibc-shim" | "venus"))
}

pub fn source_native_selected(cfg: &str) -> bool {
    selected(cfg,"nvidia_driver_mode").as_deref() == Some("nvidia-prolapse")
        && selected(cfg,"nvidia_prolapse_install").as_deref() == Some("dev-root")
}

#[cfg(test)]
mod tests {
    use super::*;
    const MODULE: &str = "NVRM version: NVIDIA UNIX Open Kernel Module for x86_64  620.70.06 Release Build\nGCC version: 615.78.08";
    #[test]
    fn exact_620_gate_is_accepted() {
        assert!(validate_versions(MODULE,r#"{"kernel_module":"620.70.06"}"#).is_ok());
    }
    #[test]
    fn old_gate_fails_with_both_versions_in_message() {
        let error=validate_versions(MODULE,r#"{"kernel_module":"615.78.08"}"#).unwrap_err();
        assert!(error.contains("620.70.06") && error.contains("615.78.08"));
    }
    #[test]
    fn missing_evidence_is_an_error() {
        assert!(validate_versions(MODULE,"{}").is_err());
        assert!(validate_versions("GCC version: 620.70.06",r#"{"kernel_module":"620.70.06"}"#).is_err());
    }
    #[test]
    fn every_nvidia_backend_selects_nvidia_for_host_graphics() {
        for mode in ["nvidia-prolapse", "glibc-shim", "venus"] {
            assert!(graphics_requires_nvidia(&format!("[waydroid]\nnvidia_driver_mode={mode}")));
        }
        assert!(!graphics_requires_nvidia("[properties]\nnvidia_driver_mode=glibc-shim"));
        assert!(!graphics_requires_nvidia("[waydroid]\narch=x86_64"));
    }
    #[test]
    fn host_driver_mode_never_uses_exact_rm_preflight() {
        let cfg = "[waydroid]\nnvidia_driver_mode=glibc-shim\nnvidia_prolapse_install=dev-root";
        assert!(graphics_requires_nvidia(cfg));
        assert!(!source_native_selected(cfg));
    }
    #[test]
    fn reads_only_waydroid_keys() {
        assert_eq!(selected("[properties]\nnvidia_driver_mode=wrong\n[waydroid]\nnvidia_driver_mode = nvidia-prolapse","nvidia_driver_mode").as_deref(),Some("nvidia-prolapse"));
    }
}
