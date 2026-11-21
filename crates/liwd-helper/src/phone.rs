//! Reversible, allowlisted Waydroid identity changes for the next container boot.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path, sync::Mutex};
type Values = BTreeMap<String, Option<String>>;
static LOCK: Mutex<()> = Mutex::new(());
const STATE: &str = "/var/lib/nexdroid/phone/state.json";
const CONFIG: &str = "/var/lib/waydroid/waydroid.cfg";
const BASE: &str = "/var/lib/waydroid/waydroid_base.prop";
#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
struct Snapshot {
    config: Values,
    base: Values,
}
#[derive(Serialize, Deserialize)]
struct State {
    original: Snapshot,
    last: Snapshot,
    profile: String,
}
pub fn selected() -> anyhow::Result<String> {
    let _lock = LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("Phone settings lock failed"))?;
    match std::fs::read_to_string(STATE) {
        Ok(s) => Ok(serde_json::from_str::<State>(&s)?.profile),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok("auto".into()),
        Err(e) => Err(e.into()),
    }
}
pub fn select(id: &str) -> anyhow::Result<()> {
    let _lock = LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("Phone settings lock failed"))?;
    apply(id, Path::new(CONFIG), Path::new(BASE), Path::new(STATE))
}
fn read(path: &Path) -> anyhow::Result<String> {
    anyhow::ensure!(
        !std::fs::symlink_metadata(path)?.file_type().is_symlink(),
        "Refusing symlink: {}",
        path.display()
    );
    Ok(std::fs::read_to_string(path)?)
}
fn atomic(path: &Path, text: &str, mode: u32) -> anyhow::Result<()> {
    use std::{io::Write, os::unix::fs::PermissionsExt};
    let mut f = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    f.as_file()
        .set_permissions(std::fs::Permissions::from_mode(mode))?;
    f.write_all(text.as_bytes())?;
    f.as_file().sync_all()?;
    f.persist(path)?;
    Ok(())
}
fn apply(id: &str, config: &Path, base: &Path, state_path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    anyhow::ensure!(liw_core::phone::valid(id), "Unknown phone profile");
    if id == "auto" && !state_path.exists() {
        return Ok(());
    }
    let cfg = read(config)?;
    let props = read(base)?;
    let cfg_mode = std::fs::metadata(config)?.permissions().mode() & 0o777;
    let base_mode = std::fs::metadata(base)?.permissions().mode() & 0o777;
    let current = Snapshot {
        config: values(&cfg, true)?,
        base: values(&props, false)?,
    };
    let mut state = if state_path.exists() {
        serde_json::from_str::<State>(&read(state_path)?)?
    } else {
        let s = State {
            original: current.clone(),
            last: current.clone(),
            profile: "auto".into(),
        };
        std::fs::create_dir_all(state_path.parent().unwrap())?;
        atomic(state_path, &serde_json::to_string_pretty(&s)?, 0o600)?;
        s
    };
    anyhow::ensure!(
        current == state.last,
        "Phone properties changed outside NexDroid. Original values are preserved in {}; reconcile the changes before applying.",
        state_path.display()
    );
    let desired = if id == "auto" {
        state.original.clone()
    } else {
        let values = liw_core::phone::properties(id).unwrap();
        Snapshot {
            config: values.clone(),
            base: values,
        }
    };
    let next_cfg = rewrite(&cfg, true, &desired.config)?;
    let next_base = rewrite(&props, false, &desired.base)?;
    atomic(config, &next_cfg, cfg_mode)?;
    if let Err(e) = atomic(base, &next_base, base_mode) {
        atomic(config, &cfg, cfg_mode).map_err(|rollback| {
            anyhow::anyhow!("{e}; configuration rollback failed: {rollback}")
        })?;
        return Err(e);
    }
    state.last = desired;
    state.profile = id.into();
    if let Err(e) = atomic(state_path, &serde_json::to_string_pretty(&state)?, 0o600) {
        let a = atomic(config, &cfg, cfg_mode);
        let b = atomic(base, &props, base_mode);
        anyhow::ensure!(a.is_ok() && b.is_ok(), "{e}; rollback failed: {a:?}, {b:?}");
        return Err(e);
    }
    Ok(())
}
fn key_value(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    if line.starts_with(['#', ';']) {
        return None;
    }
    line.split_once('=').map(|(k, v)| (k.trim(), v.trim()))
}
fn values(text: &str, ini: bool) -> anyhow::Result<Values> {
    let mut result: Values = liw_core::phone::KEYS
        .into_iter()
        .map(|k| (k.into(), None))
        .collect();
    let mut inside = !ini;
    let mut sections = 0;
    for line in text.lines() {
        let t = line.trim();
        if ini && t.starts_with('[') {
            inside = t == "[properties]";
            if inside {
                sections += 1;
            }
            continue;
        }
        if inside {
            if let Some((k, v)) = key_value(line) {
                if let Some(value) = result.get_mut(k) {
                    anyhow::ensure!(value.is_none(), "Duplicate phone property: {k}");
                    *value = Some(v.into());
                }
            }
        }
    }
    anyhow::ensure!(sections <= 1, "Duplicate properties sections");
    Ok(result)
}
fn rewrite(text: &str, ini: bool, desired: &Values) -> anyhow::Result<String> {
    values(text, ini)?;
    let mut out = Vec::new();
    let mut inside = !ini;
    let mut found = !ini;
    let append = |out: &mut Vec<String>| {
        for (k, v) in desired {
            if let Some(v) = v {
                out.push(if ini {
                    format!("{k} = {v}")
                } else {
                    format!("{k}={v}")
                });
            }
        }
    };
    for line in text.lines() {
        let t = line.trim();
        if ini && t.starts_with('[') {
            if inside {
                append(&mut out);
            }
            inside = t == "[properties]";
            found |= inside;
        }
        if inside && key_value(line).is_some_and(|(k, _)| desired.contains_key(k)) {
            continue;
        }
        out.push(line.into());
    }
    if inside {
        append(&mut out);
    } else if !found {
        out.push("[properties]".into());
        append(&mut out);
    }
    Ok(out.join("\n") + "\n")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presets_restore_original_and_preserve_hardware_and_other_sections() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("cfg");
        let base = dir.path().join("base");
        let state = dir.path().join("state");
        let original = "[waydroid]\narch = x86_64\n[properties]\nro.product.model = Waydroid\nro.hardware.egl = angle\nro.build.fingerprint = original\n[other]\nro.product.model = unrelated\n";
        std::fs::write(&cfg, original).unwrap();
        std::fs::write(
            &base,
            "ro.product.model=Original base\nro.product.cpu.abilist=x86_64,arm64-v8a\n",
        )
        .unwrap();
        for p in liw_core::phone::PROFILES {
            apply(p.id, &cfg, &base, &state).unwrap();
            let a = std::fs::read_to_string(&cfg).unwrap();
            assert_eq!(
                values(&a, true).unwrap()["ro.product.model"].as_deref(),
                Some(p.model)
            );
            assert!(a.contains("ro.hardware.egl = angle"));
            assert!(a.contains("ro.build.fingerprint = original"));
            assert!(a.contains("[other]\nro.product.model = unrelated"));
        }
        apply("auto", &cfg, &base, &state).unwrap();
        assert_eq!(
            values(&std::fs::read_to_string(&cfg).unwrap(), true).unwrap(),
            values(original, true).unwrap()
        );
        let b = std::fs::read_to_string(&base).unwrap();
        assert!(b.contains("ro.product.model=Original base"));
        assert!(b.contains("ro.product.cpu.abilist=x86_64,arm64-v8a"));
    }
    #[test]
    fn invalid_profiles_and_external_changes_are_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("cfg");
        let base = dir.path().join("base");
        let state = dir.path().join("state");
        std::fs::write(&cfg, "[properties]\n").unwrap();
        std::fs::write(&base, "x=y\n").unwrap();
        assert!(apply("pixel7\nro.debuggable=1", &cfg, &base, &state).is_err());
        apply("pixel7", &cfg, &base, &state).unwrap();
        let altered = std::fs::read_to_string(&cfg)
            .unwrap()
            .replace("Pixel 7", "External");
        std::fs::write(&cfg, &altered).unwrap();
        assert!(apply("pixel6", &cfg, &base, &state).is_err());
        assert_eq!(std::fs::read_to_string(cfg).unwrap(), altered);
        assert!(
            values(
                "[properties]\nro.product.model=a\nro.product.model=b\n",
                true
            )
            .is_err()
        );
    }
}
