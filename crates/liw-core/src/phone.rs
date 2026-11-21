//! Device identity presets. Hardware, ABI and verified-boot properties are untouched.
#[derive(Clone, Copy, Debug)]
pub struct PhoneProfile {
    pub id: &'static str,
    pub model: &'static str,
    pub device: &'static str,
}
// Model/codename source: https://source.android.com/docs/setup/reference/build-numbers
pub const PROFILES: [PhoneProfile; 3] = [
    PhoneProfile {
        id: "pixel6",
        model: "Pixel 6",
        device: "oriole",
    },
    PhoneProfile {
        id: "pixel7",
        model: "Pixel 7",
        device: "panther",
    },
    PhoneProfile {
        id: "pixel8",
        model: "Pixel 8",
        device: "shiba",
    },
];
pub fn find(id: &str) -> Option<PhoneProfile> {
    PROFILES.iter().copied().find(|p| p.id == id)
}
pub fn valid(id: &str) -> bool {
    id == "auto" || find(id).is_some()
}
pub const KEYS: [&str; 5] = [
    "ro.product.brand",
    "ro.product.manufacturer",
    "ro.product.model",
    "ro.product.name",
    "ro.product.device",
];
pub fn properties(id: &str) -> Option<std::collections::BTreeMap<String, Option<String>>> {
    let p = find(id)?;
    Some(
        KEYS.into_iter()
            .zip(["google", "Google", p.model, p.device, p.device])
            .map(|(k, v)| (k.into(), Some(v.into())))
            .collect(),
    )
}
#[cfg(test)]
mod tests {
    #[test]
    fn only_known_identities_are_allowed() {
        assert!(super::valid("auto"));
        assert!(!super::valid("../../pixel7"));
        assert!(!super::valid("pixel7\nro.debuggable=1"));
        for p in super::PROFILES {
            let values = super::properties(p.id).unwrap();
            assert_eq!(values.len(), 5);
            assert_eq!(values["ro.product.model"].as_deref(), Some(p.model));
            assert!(values.keys().all(|k| k.starts_with("ro.product.")));
        }
    }
}
