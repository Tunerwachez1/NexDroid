//! Host window shortcuts. Physical evdev keys keep bindings layout-independent.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Fullscreen,
    Settings,
    Editor,
    TextInput,
    Screenshot,
    CaptureMouse,
}
pub const ACTIONS: [Action; 6] = [
    Action::Fullscreen,
    Action::Settings,
    Action::Editor,
    Action::TextInput,
    Action::Screenshot,
    Action::CaptureMouse,
];
impl Action {
    pub fn text(self) -> &'static str {
        match self {
            Self::Fullscreen => "shortcut_fullscreen",
            Self::Settings => "shortcut_settings",
            Self::Editor => "shortcut_editor",
            Self::TextInput => "shortcut_text_input",
            Self::Screenshot => "shortcut_screenshot",
            Self::CaptureMouse => "shortcut_capture",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chord {
    pub key: u16,
    pub modifiers: u8,
}
pub const CTRL: u8 = 1;
pub const SHIFT: u8 = 2;
pub const ALT: u8 = 4;
pub const SUPER: u8 = 8;
impl Chord {
    pub fn valid(self) -> bool {
        self.modifiers < 16
            && (2..=255).contains(&self.key)
            && !matches!(self.key, 29 | 42 | 54 | 56 | 97 | 100 | 125 | 126)
    }
    pub fn label(self) -> String {
        let mut parts = Vec::new();
        for (mask, label) in [
            (CTRL, "Ctrl"),
            (SHIFT, "Shift"),
            (ALT, "Alt"),
            (SUPER, "Super"),
        ] {
            if self.modifiers & mask != 0 {
                parts.push(label.into());
            }
        }
        parts.push(crate::keys::label(self.key));
        parts.join(" + ")
    }
    pub fn from_event(event: &gpui::KeyDownEvent) -> Option<Self> {
        if event.is_held {
            return None;
        }
        let key = crate::keys::evdev_code(&event.keystroke.key)?;
        let chord = Self {
            key,
            modifiers: event_modifiers(event),
        };
        chord.valid().then_some(chord)
    }
}
pub fn event_modifiers(event: &gpui::KeyDownEvent) -> u8 {
    let m = event.keystroke.modifiers;
    u8::from(m.control) * CTRL
        | u8::from(m.shift) * SHIFT
        | u8::from(m.alt) * ALT
        | u8::from(m.platform) * SUPER
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub enabled: bool,
    pub chord: Option<Chord>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shortcuts {
    pub version: u32,
    pub bindings: BTreeMap<Action, Binding>,
}
impl Default for Shortcuts {
    fn default() -> Self {
        Self {
            version: 1,
            bindings: [
                (
                    Action::Fullscreen,
                    Some(Chord {
                        key: 87,
                        modifiers: 0,
                    }),
                ),
                (
                    Action::Settings,
                    Some(Chord {
                        key: 23,
                        modifiers: CTRL | SHIFT,
                    }),
                ),
                (
                    Action::Editor,
                    Some(Chord {
                        key: 30,
                        modifiers: CTRL | SHIFT,
                    }),
                ),
                (
                    Action::TextInput,
                    Some(Chord {
                        key: 66,
                        modifiers: 0,
                    }),
                ),
                (
                    Action::Screenshot,
                    Some(Chord {
                        key: 31,
                        modifiers: CTRL | SHIFT,
                    }),
                ),
                (Action::CaptureMouse, None),
            ]
            .into_iter()
            .map(|(a, chord)| {
                (
                    a,
                    Binding {
                        enabled: true,
                        chord,
                    },
                )
            })
            .collect(),
        }
    }
}
impl Shortcuts {
    pub fn path() -> std::path::PathBuf {
        crate::performance::directory().join("shortcuts.json")
    }
    pub fn load() -> anyhow::Result<Self> {
        Self::read(&Self::path())
    }
    fn read(path: &std::path::Path) -> anyhow::Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };
        let mut config: Self = serde_json::from_str(&text)?;
        for (action, binding) in Self::default().bindings {
            config.bindings.entry(action).or_insert(binding);
        }
        config.validate()?;
        Ok(config)
    }
    pub fn conflicts(&self) -> Option<(Action, Action)> {
        self.conflicts_with_capture(None)
    }
    pub fn conflicts_with_capture(&self, capture_key: Option<u16>) -> Option<(Action, Action)> {
        let mut seen = BTreeMap::new();
        for (&action, binding) in &self.bindings {
            if binding.enabled {
                if let Some(chord) = binding.chord.or_else(|| {
                    (action == Action::CaptureMouse)
                        .then_some(capture_key)
                        .flatten()
                        .map(|key| Chord { key, modifiers: 0 })
                }) {
                    if let Some(previous) = seen.insert((chord.key, chord.modifiers), action) {
                        return Some((previous, action));
                    }
                }
            }
        }
        None
    }
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(self.version == 1, "Unsupported shortcuts.json version");
        for (action, binding) in &self.bindings {
            anyhow::ensure!(
                binding.chord.is_none_or(Chord::valid),
                "Invalid shortcut for {action:?}; Esc and standalone modifiers are reserved"
            );
            anyhow::ensure!(
                !binding.enabled || binding.chord.is_some() || *action == Action::CaptureMouse,
                "Missing shortcut for {action:?}"
            );
        }
        anyhow::ensure!(
            self.conflicts().is_none(),
            "Two enabled actions use the same shortcut"
        );
        Ok(())
    }
    pub fn save(&self) -> anyhow::Result<()> {
        self.validate()?;
        std::fs::create_dir_all(crate::performance::directory())?;
        crate::performance::atomic_json(&Self::path(), self)
    }
    pub fn resolve(&self, key: u16, modifiers: u8, capture_key: Option<u16>) -> Option<Action> {
        self.bindings.iter().find_map(|(&action, binding)| {
            let chord = binding.chord.or_else(|| {
                (action == Action::CaptureMouse)
                    .then_some(capture_key)
                    .flatten()
                    .map(|key| Chord { key, modifiers: 0 })
            });
            (binding.enabled && chord == Some(Chord { key, modifiers })).then_some(action)
        })
    }
    pub fn label(&self, action: Action, capture_key: Option<u16>) -> Option<String> {
        let binding = self.bindings.get(&action)?;
        if !binding.enabled {
            return None;
        }
        binding
            .chord
            .or_else(|| {
                (action == Action::CaptureMouse)
                    .then_some(capture_key)
                    .flatten()
                    .map(|key| Chord { key, modifiers: 0 })
            })
            .map(Chord::label)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changing_or_disabling_releases_the_old_key() {
        let mut c = Shortcuts::default();
        assert_eq!(c.resolve(87, 0, None), Some(Action::Fullscreen));
        c.bindings.get_mut(&Action::Fullscreen).unwrap().chord = Some(Chord {
            key: 88,
            modifiers: CTRL,
        });
        assert_eq!(c.resolve(87, 0, None), None);
        assert_eq!(c.resolve(88, CTRL, None), Some(Action::Fullscreen));
        c.bindings.get_mut(&Action::Fullscreen).unwrap().enabled = false;
        assert_eq!(c.resolve(88, CTRL, None), None);
        assert_eq!(c.resolve(31, CTRL | SHIFT | ALT, None), None);
    }
    #[test]
    fn conflicts_block_saving_but_disabled_duplicates_are_allowed() {
        let mut c = Shortcuts::default();
        let chord = c.bindings[&Action::Fullscreen].chord;
        c.bindings.get_mut(&Action::Editor).unwrap().chord = chord;
        assert!(c.validate().is_err());
        c.bindings.get_mut(&Action::Editor).unwrap().enabled = false;
        assert!(c.validate().is_ok());
        c.bindings.get_mut(&Action::Settings).unwrap().chord = Some(Chord {
            key: 1,
            modifiers: 0,
        });
        assert!(c.validate().is_err());
    }
    #[test]
    fn mouse_capture_follows_the_profile_until_overridden_and_can_be_disabled() {
        let mut c = Shortcuts::default();
        assert_eq!(c.resolve(59, 0, Some(59)), Some(Action::CaptureMouse));
        c.bindings.get_mut(&Action::CaptureMouse).unwrap().chord = Some(Chord {
            key: 60,
            modifiers: CTRL,
        });
        assert_eq!(c.resolve(59, 0, Some(59)), None);
        assert_eq!(c.resolve(60, CTRL, Some(59)), Some(Action::CaptureMouse));
        c.bindings.get_mut(&Action::CaptureMouse).unwrap().enabled = false;
        assert_eq!(c.resolve(60, CTRL, Some(59)), None);
    }
    #[test]
    fn json_roundtrip_preserves_disabled_bindings_and_rejects_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shortcuts.json");
        let mut c = Shortcuts::default();
        c.bindings.get_mut(&Action::Screenshot).unwrap().enabled = false;
        std::fs::write(&path, serde_json::to_vec(&c).unwrap()).unwrap();
        assert!(!Shortcuts::read(&path).unwrap().bindings[&Action::Screenshot].enabled);
        std::fs::write(
            &path,
            r#"{"version":1,"bindings":{"fullscreen":{"enabled":false,"chord":null}}}"#,
        )
        .unwrap();
        let c = Shortcuts::read(&path).unwrap();
        assert!(!c.bindings[&Action::Fullscreen].enabled);
        assert!(c.bindings.contains_key(&Action::Editor));
        std::fs::write(&path, "broken").unwrap();
        assert!(Shortcuts::read(&path).is_err());
    }
    #[test]
    fn russian_and_english_chords_use_the_same_physical_key() {
        let event = |key| gpui::KeyDownEvent {
            keystroke: gpui::Keystroke::parse(key).unwrap(),
            is_held: false,
            prefer_character_input: false,
        };
        assert_eq!(
            Chord::from_event(&event("ctrl-shift-a")),
            Chord::from_event(&event("ctrl-shift-ф"))
        );
    }
    #[test]
    fn profile_capture_conflicts_are_detected_in_the_current_game() {
        let mut c = Shortcuts::default();
        c.bindings.get_mut(&Action::Fullscreen).unwrap().chord = Some(Chord {
            key: 59,
            modifiers: 0,
        });
        assert!(c.conflicts_with_capture(Some(59)).is_some());
        c.bindings.get_mut(&Action::CaptureMouse).unwrap().enabled = false;
        assert!(c.conflicts_with_capture(Some(59)).is_none());
    }
}
