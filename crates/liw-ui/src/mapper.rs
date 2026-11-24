//! Laying controls onto the game.
//!
//! The shape is GameLoop's, and it is the right one: pick a kind of control
//! from the panel, click where it goes, then set it up. Nothing is named. A
//! player thinks of it as "the fire button", not as a string they have to
//! invent — so the engine's binding key is generated and never shown.
//!
//! A generated key is not cosmetic. `liw_input` allocates one touch pointer
//! per binding name; two controls sharing a name would fight over the same
//! finger. Making the name the machine's business removes a way for a person
//! to break that without knowing they did.
//!
//! What is stored is an evdev code and a NORMALISED position, so a control
//! survives a resized window and a different keyboard layout. Turning a click
//! into that position uses `liw_compositor::fit` — the same function the
//! compositor renders with, because two copies would drift and the symptom
//! would be taps landing away from where they were placed.

use liw_input::profile::{Binding, Profile, Trigger};
use liw_input::touch::Norm;

/// The kinds of control that can be placed.
///
/// Only what the input engine actually implements. A palette entry with no
/// binding behind it is a promise the engine cannot keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    /// Finger down while the key is held.
    #[default]
    Tap,
    /// One tap per press, holding does not repeat.
    Toggle,
    /// Four keys moving one finger around a centre.
    Joystick,
    /// Mouse look.
    Aim,
    Fire,
}

impl Kind {
    pub const ALL: [Kind; 5] = [Kind::Tap, Kind::Toggle, Kind::Joystick, Kind::Aim, Kind::Fire];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Tap => "Точка нажатия",
            Kind::Toggle => "Одиночный тап",
            Kind::Joystick => "Джойстик",
            Kind::Aim => "Обзор и прицел",
            Kind::Fire => "Стрельба · ЛКМ",
        }
    }

    /// Whether this kind has a size the user can change.
    ///
    /// Only the joystick does — its radius is in the model. Offering a size
    /// for a button would be a control that changes nothing, which is worse
    /// than not offering one.
    pub fn has_size(self) -> bool {
        matches!(self, Kind::Joystick)
    }
}

/// How far one arrow-key press moves a control, in normalised units.
///
/// Small enough that a control can be put exactly on a game button, which is
/// the whole reason for nudging rather than clicking again — a click is only
/// as accurate as the pointer, and game buttons are small.
pub const NUDGE: f32 = 0.002;

#[derive(Default)]
pub struct Mapper {
    pub on: bool,
    pub profile: Option<Profile>,
    /// What the next click will place.
    pub palette: Kind,
    /// The control being edited, by its generated key.
    pub selected: Option<String>,
    /// True while waiting for a key press to bind to the selection.
    pub binding: bool,
    pub error: Option<String>,
    pub saved: bool,
}

impl Mapper {
    pub fn is_on(&self) -> bool {
        self.on
    }

    /// Starts editing the profile for a package, creating one if there is none.
    pub fn begin(&mut self, package: &str, name: &str) {
        let store = liw_input::store::Store::discover();
        self.profile = Some(match store.for_package(package) {
            Some(entry) => entry.profile.clone(),
            None => Profile { auto_text: true,
                name: name.to_string(),
                package: package.to_string(),
                bindings: Default::default(),
            },
        });
        self.on = true;
        self.selected = None;
        self.binding = false;
        self.error = None;
        self.saved = false;
    }

    pub fn end(&mut self) {
        self.on = false;
        self.profile = None;
        self.selected = None;
        self.binding = false;
    }

    /// Places a control of the selected kind.
    ///
    /// `point` is relative to the picture's top-left corner and `shown` is the
    /// picture's size, both in view pixels. A click in the letterboxing is
    /// ignored rather than clamped: clamping would silently put a control at
    /// the edge, which is not where it was aimed.
    pub fn place(&mut self, point: (f32, f32), shown: (f32, f32)) -> Option<String> {
        if !self.on || shown.0 <= 0.0 || shown.1 <= 0.0 {
            return None;
        }
        let (x, y) = (point.0 / shown.0, point.1 / shown.1);
        if !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) {
            return None;
        }
        let at = Norm::new(x, y);
        let kind = self.palette;
        let profile = self.profile.as_mut()?;
        if kind == Kind::Fire && profile.bindings.values().any(|b| b.triggers().contains(&Trigger::MouseLeft)) {
            self.error = Some("ЛКМ уже назначена. Переместите существующий маркер или освободите ЛКМ.".into());
            return None;
        }
        let key = next_key(profile);
        profile.bindings.insert(key.clone(), blank(kind, at));
        self.selected = Some(key.clone());
        // Buttons need a key; joysticks start with WASD and aim uses the mouse.
        self.binding = matches!(kind, Kind::Tap | Kind::Toggle);
        self.saved = false;
        self.error = None;
        Some(key)
    }

    /// Selects an existing control.
    pub fn select(&mut self, key: &str) {
        if self.profile.as_ref().is_some_and(|p| p.bindings.contains_key(key)) {
            self.selected = Some(key.to_string());
            self.binding = self.binding_prompt().is_some();
        }
    }

    /// The next missing assignment, in the same order used by `take_key`.
    pub fn binding_prompt(&self) -> Option<&'static str> {
        let b = self.profile.as_ref()?.bindings.get(self.selected.as_ref()?)?;
        match b {
            Binding::Tap { trigger: Trigger::Key(0), .. }
            | Binding::Toggle { trigger: Trigger::Key(0), .. } => Some("Нажмите клавишу или кнопку мыши"),
            Binding::Joystick { up, left, down, right, .. } => {
                [(up, "Вверх: нажмите W"), (left, "Влево: нажмите A"),
                 (down, "Вниз: нажмите S"), (right, "Вправо: нажмите D")]
                    .into_iter().find_map(|(key, prompt)| (*key == Trigger::Key(0)).then_some(prompt))
            }
            _ => None,
        }
    }

    /// Restart key assignment without losing a control's position or size.
    pub fn rebind(&mut self) {
        let Some(b) = self.profile.as_mut().and_then(|p| p.bindings.get_mut(self.selected.as_ref()?)) else { return };
        match b {
            Binding::Tap { trigger, .. } | Binding::Toggle { trigger, .. } => *trigger = Trigger::Key(0),
            Binding::Joystick { up, left, down, right, .. } => {
                *up = Trigger::Key(0);
                *left = Trigger::Key(0);
                *down = Trigger::Key(0);
                *right = Trigger::Key(0);
            }
            Binding::Aim { .. } => {}
            _ => return,
        }
        self.binding = true;
        self.saved = false;
        self.error = None;
    }

    /// Apply the standard movement keys without changing geometry.
    pub fn use_wasd(&mut self) {
        let Some(Binding::Joystick { up, left, down, right, .. }) =
            self.profile.as_mut().and_then(|p| p.bindings.get_mut(self.selected.as_ref()?)) else { return };
        *up = Trigger::Key(17);
        *left = Trigger::Key(30);
        *down = Trigger::Key(31);
        *right = Trigger::Key(32);
        self.binding = false;
        self.saved = false;
        self.error = None;
    }

    /// Binds a key press to the selected control.
    ///
    /// For a joystick the four directions are filled in order, so pressing
    /// W A S D in turn sets up the whole stick without four separate steps.
    pub fn take_key(&mut self, key_name: &str) -> bool {
        let Some(code) = crate::keys::evdev_code(key_name) else { return false };
        self.take_trigger(Trigger::Key(code))
    }

    pub fn take_trigger(&mut self, trigger: Trigger) -> bool {
        if !self.binding {
            return false;
        }
        if matches!(trigger, Trigger::Key(0 | 1)) {
            self.error = Some("Esc зарезервирована для отмены редактирования.".into());
            return true;
        }
        let Some(sel) = &self.selected else { return false };
        let Some(profile) = self.profile.as_mut() else { return false };
        if profile.bindings.iter().any(|(name, b)| name != sel && b.triggers().contains(&trigger)) {
            self.error = Some(format!("{} уже назначена другому элементу.", trigger_label(&trigger)));
            return true;
        }
        let Some(binding) = profile.bindings.get_mut(sel) else { return false };

        match binding {
            Binding::Tap { trigger: t, .. } | Binding::Toggle { trigger: t, .. } => {
                *t = trigger;
                self.binding = false;
            }
            Binding::Joystick { up, down, left, right, .. } => {
                if !matches!(trigger, Trigger::Key(_)) {
                    return false;
                }
                if [&*up, &*left, &*down, &*right].contains(&&trigger) {
                    self.error = Some("Эта клавиша уже занята другим направлением. Нажмите другую.".into());
                    return true;
                }
                // Unset directions are Key(0); fill the first one still unset,
                // in the order a person expects to press them.
                const UNSET: Trigger = Trigger::Key(0);
                if let Some(slot) = [&mut *up, &mut *left, &mut *down, &mut *right]
                    .into_iter()
                    .find(|s| **s == UNSET)
                {
                    *slot = trigger;
                }
                let still_empty = [&*up, &*left, &*down, &*right]
                    .iter()
                    .any(|s| **s == UNSET);
                self.binding = still_empty;
            }
            Binding::Aim { toggle, .. } => {
                let Trigger::Key(code) = trigger else { return false };
                if !crate::keys::can_capture_with(code) {
                    self.error = Some("Эта клавиша зарезервирована. Выберите другую.".into());
                    return true;
                }
                *toggle = Some(trigger);
                self.binding = false;
            }
            _ => return false,
        }
        self.saved = false;
        self.error = None;
        true
    }

    pub fn move_selected(&mut self, at: Norm) -> bool {
        let Some(key) = &self.selected else { return false };
        let Some(binding) = self.profile.as_mut().and_then(|p| p.bindings.get_mut(key)) else {
            return false;
        };
        let Some(position) = position_mut(binding) else { return false };
        if *position == at {
            return false;
        }
        *position = at;
        self.saved = false;
        true
    }

    pub fn nudge(&mut self, dx: f32, dy: f32) -> bool {
        let Some(control) = self.selection() else { return false };
        self.move_selected(Norm::new(control.at.x + dx, control.at.y + dy))
    }

    /// Changes the selected control's size, where it has one.
    pub fn resize(&mut self, delta: f32) -> bool {
        let Some(sel) = self.selected.clone() else { return false };
        let Some(profile) = self.profile.as_mut() else { return false };
        match profile.bindings.get_mut(&sel) {
            Some(Binding::Joystick { radius, .. }) => {
                *radius = (*radius + delta).clamp(0.02, 0.5);
                self.saved = false;
                true
            }
            _ => false,
        }
    }

    pub fn remove_selected(&mut self) {
        let Some(sel) = self.selected.take() else { return };
        if let Some(p) = self.profile.as_mut() {
            p.bindings.remove(&sel);
            self.saved = false;
        }
        self.binding = false;
    }

    pub fn save(&mut self) {
        let Some(p) = &self.profile else { return };
        if let Some((key, b)) = p.bindings.iter().find(|(_, b)| b.triggers().contains(&Trigger::Key(0))) {
            let message = if matches!(b, Binding::Joystick { .. }) {
                "Назначены не все клавиши. Завершите ввод или нажмите «Назначить WASD»."
            } else {
                "Назначены не все клавиши. Нажмите клавишу для выделенного элемента."
            };
            let key = key.clone();
            self.select(&key);
            self.saved = false;
            self.error = Some(message.into());
            return;
        }
        match liw_input::store::Store::discover().save(p) {
            Ok(_) => {
                self.saved = true;
                self.error = None;
            }
            Err(e) => {
                self.saved = false;
                self.error = Some(format!("Не удалось сохранить: {e}"));
            }
        }
    }

    /// Everything to draw over the picture.
    pub fn controls(&self) -> Vec<Control> {
        let Some(p) = &self.profile else { return Vec::new() };
        p.bindings
            .iter()
            .filter_map(|(key, b)| {
                let (kind, at, radius) = describe(b)?;
                Some(Control {
                    key: key.clone(),
                    kind,
                    at,
                    radius,
                    label: face(b),
                    directions: match b {
                        Binding::Joystick { up, left, down, right, .. } =>
                            Some([up, left, down, right].map(trigger_label)),
                        _ => None,
                    },
                    selected: self.selected.as_deref() == Some(key.as_str()),
                })
            })
            .collect()
    }

    /// The selected control, if there is one.
    pub fn selection(&self) -> Option<Control> {
        let sel = self.selected.as_deref()?;
        self.controls().into_iter().find(|c| c.key == sel)
    }
}

/// One control, as the view needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct Control {
    pub key: String,
    pub kind: Kind,
    pub at: Norm,
    /// Normalised radius, for the kinds that have one.
    pub radius: Option<f32>,
    /// What to write on it — the key or keys it is bound to.
    pub label: String,
    pub directions: Option<[String; 4]>,
    pub selected: bool,
}

/// A control of this kind with nothing bound yet.
fn blank(kind: Kind, at: Norm) -> Binding {
    // Key(0) means "not set". evdev reserves 0 as KEY_RESERVED, so it can
    // never be a real key, which makes it safe as the empty marker.
    const UNSET: Trigger = Trigger::Key(0);
    match kind {
        Kind::Tap => Binding::Tap { trigger: UNSET, at },
        Kind::Toggle => Binding::Toggle { trigger: UNSET, at },
        Kind::Fire => Binding::Fire { at },
        Kind::Joystick => Binding::Joystick {
            up: Trigger::Key(17), down: Trigger::Key(31),
            left: Trigger::Key(30), right: Trigger::Key(32),
            center: at,
            radius: 0.12,
        },
        Kind::Aim => toml::from_str(&format!(
            "type = \"aim\"\norigin = {{ x = {}, y = {} }}\n\
             sensitivity = 0.0016\ndeadzone = 0.0\n",
            at.x, at.y
        ))
        .expect("an aim binding with its required fields"),
    }
}

fn describe(b: &Binding) -> Option<(Kind, Norm, Option<f32>)> {
    match b {
        Binding::Tap { at, .. } => Some((Kind::Tap, *at, None)),
        Binding::Fire { at } => Some((Kind::Fire, *at, None)),
        Binding::Toggle { at, .. } => Some((Kind::Toggle, *at, None)),
        Binding::Joystick { center, radius, .. } => {
            Some((Kind::Joystick, *center, Some(*radius)))
        }
        Binding::Aim { origin, .. } => Some((Kind::Aim, *origin, None)),
        _ => None,
    }
}

fn position_mut(b: &mut Binding) -> Option<&mut Norm> {
    match b {
        Binding::Tap { at, .. } | Binding::Toggle { at, .. } | Binding::Fire { at } => Some(at),
        Binding::Joystick { center, .. } => Some(center),
        Binding::Aim { origin, .. } => Some(origin),
        _ => None,
    }
}

/// What is written on a control.
fn trigger_label(t: &Trigger) -> String {
    match t {
        Trigger::Key(0) => "—".to_string(),
        Trigger::Key(c) => crate::keys::label(*c),
        Trigger::MouseLeft => "ЛКМ".into(),
        Trigger::MouseRight => "ПКМ".into(),
        Trigger::MouseMiddle => "СКМ".into(),
        Trigger::WheelUp => "W↑".into(),
        Trigger::WheelDown => "W↓".into(),
    }
}

fn face(b: &Binding) -> String {
    let one = trigger_label;
    match b {
        Binding::Tap { trigger, .. } | Binding::Toggle { trigger, .. } => one(trigger),
        Binding::Fire { .. } => "ЛКМ".into(),
        Binding::Joystick { up, down, left, right, .. } => {
            format!("{}{}{}{}", one(up), one(left), one(down), one(right))
        }
        Binding::Aim { toggle, .. } => {
            toggle.as_ref().map(one).unwrap_or_else(|| "F1".into())
        }
        _ => String::new(),
    }
}

/// A binding key nobody has to think about.
///
/// The engine needs one per control for pointer allocation. Asking a person
/// to invent it would be asking them to name something they think of as
/// "that button", and a duplicate would silently make two controls share a
/// finger.
fn next_key(p: &Profile) -> String {
    (1..).map(|n| format!("c{n}")).find(|k| !p.bindings.contains_key(k)).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editing() -> Mapper {
        let mut m = Mapper::default();
        m.on = true;
        m.profile = Some(Profile { auto_text: true,
            name: "t".into(),
            package: "com.x".into(),
            bindings: Default::default(),
        });
        m
    }

    #[test]
    fn physical_keys_and_mouse_buttons_survive_profile_round_trip() {
        let mut mapper = editing();
        let triggers = [Trigger::Key(29), Trigger::Key(110), Trigger::Key(111),
                        Trigger::Key(82), Trigger::MouseRight];
        for (index, trigger) in triggers.iter().enumerate() {
            mapper.place((10.0 + index as f32, 10.0), (100.0, 100.0)).unwrap();
            assert!(mapper.take_trigger(trigger.clone()));
            assert!(!mapper.binding);
        }
        mapper.palette = Kind::Fire;
        mapper.place((70.0, 80.0), (100.0, 100.0)).unwrap();
        assert!(!mapper.binding);
        assert_eq!(mapper.selection().unwrap().label, "ЛКМ");

        let profile = mapper.profile.as_ref().unwrap();
        let rendered = liw_input::store::render(None, profile).unwrap();
        let saved = Profile::from_toml(&rendered).unwrap();
        assert_eq!(saved.bindings, profile.bindings);
        assert!(matches!(saved.bindings["c6"], Binding::Fire { .. }));
        assert_eq!(saved.bindings["c6"].triggers(), vec![Trigger::MouseLeft]);
    }

    #[test]
    fn a_click_places_the_selected_kind() {
        let mut m = editing();
        m.palette = Kind::Joystick;
        let key = m.place((320.0, 180.0), (1280.0, 720.0)).unwrap();
        let c = m.controls();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].kind, Kind::Joystick);
        assert_eq!(c[0].at, Norm::new(0.25, 0.25));
        assert_eq!(m.selected.as_deref(), Some(key.as_str()));
    }

    /// A click in the letterboxing is not on the game. Clamping would put a
    /// control at the edge without saying so.
    #[test]
    fn a_click_outside_the_picture_places_nothing() {
        let mut m = editing();
        assert!(m.place((1400.0, 10.0), (1280.0, 720.0)).is_none());
        assert!(m.controls().is_empty());
    }

    /// The generated key is the engine's, not the user's: one touch pointer
    /// is allocated per key, so a duplicate would make two controls share a
    /// finger.
    #[test]
    fn every_control_gets_its_own_key() {
        let mut m = editing();
        let a = m.place((10.0, 10.0), (100.0, 100.0)).unwrap();
        let b = m.place((20.0, 20.0), (100.0, 100.0)).unwrap();
        assert_ne!(a, b);
        assert_eq!(m.profile.unwrap().bindings.len(), 2);
    }

    #[test]
    fn a_new_button_waits_for_its_key() {
        let mut m = editing();
        m.place((10.0, 10.0), (100.0, 100.0));
        assert!(m.binding, "a button with no key is not useful yet");
        assert!(m.take_key("w"));
        assert!(!m.binding);
        assert_eq!(m.controls()[0].label, "W");
    }

    /// Aim needs no key: mouse look is on by default and the trigger is only
    /// an optional toggle.
    #[test]
    fn aim_does_not_demand_a_key() {
        let mut m = editing();
        m.palette = Kind::Aim;
        m.place((10.0, 10.0), (100.0, 100.0));
        assert!(!m.binding);
        assert_eq!(m.controls()[0].label, "F1");
    }

    /// A joystick takes four keys in a row, so WASD sets up the whole stick
    /// without four separate steps.
    #[test]
    fn a_joystick_fills_all_four_directions_in_turn() {
        let mut m = editing();
        m.palette = Kind::Joystick;
        m.place((10.0, 10.0), (100.0, 100.0));
        m.rebind();
        for k in ["w", "a", "s", "d"] {
            assert!(m.binding, "still collecting directions");
            assert!(m.take_key(k));
        }
        assert!(!m.binding, "four keys is a full stick");
        assert_eq!(m.controls()[0].label, "WASD");
    }

    #[test]
    fn repeated_direction_does_not_fill_the_next_slot() {
        let mut m = editing();
        m.palette = Kind::Joystick;
        m.place((10.0, 10.0), (100.0, 100.0));
        m.rebind();
        m.take_key("w");
        m.take_key("w");
        assert_eq!(m.controls()[0].label, "W———");
        assert!(m.error.is_some());
        assert_eq!(m.binding_prompt(), Some("Влево: нажмите A"));
        for k in ["a", "s", "d"] { m.take_key(k); }
        assert!(m.error.is_none());
        assert!(m.profile.as_ref().unwrap().validate().is_ok());
    }

    #[test]
    fn incomplete_save_resumes_assignment_without_writing() {
        let mut m = editing();
        m.palette = Kind::Joystick;
        let key = m.place((10.0, 10.0), (100.0, 100.0)).unwrap();
        m.rebind();
        m.take_key("w");
        m.selected = None;
        m.binding = false;
        m.save();
        assert!(!m.saved);
        assert_eq!(m.selected.as_deref(), Some(key.as_str()));
        assert!(m.binding);
        assert!(m.error.as_ref().unwrap().contains("Назначены не все"));
    }

    #[test]
    fn rebinding_preserves_geometry_and_replaces_old_keys() {
        let mut m = editing();
        m.palette = Kind::Joystick;
        m.place((10.0, 20.0), (100.0, 100.0));
        m.rebind();
        for k in ["w", "a", "s", "d"] { m.take_key(k); }
        m.resize(0.03);
        let before = m.selection().unwrap();
        m.rebind();
        assert_eq!(m.controls()[0].label, "————");
        assert_eq!(m.selection().unwrap().at, before.at);
        assert_eq!(m.selection().unwrap().radius, before.radius);
        for k in ["i", "j", "k", "l"] { m.take_key(k); }
        assert_eq!(m.controls()[0].label, "IJKL");
        assert!(m.profile.as_ref().unwrap().validate().is_ok());
    }

    #[test]
    fn a_new_joystick_is_ready_to_save_with_wasd() {
        let mut m = editing();
        m.palette = Kind::Joystick;
        m.place((10.0, 20.0), (100.0, 100.0));
        assert!(!m.binding);
        assert_eq!(m.controls()[0].directions, Some(["W", "A", "S", "D"].map(str::to_string)));
        assert!(m.profile.as_ref().unwrap().validate().is_ok());
        m.rebind();
        m.take_key("i");
        let geometry = m.selection().unwrap();
        m.use_wasd();
        assert_eq!(m.controls()[0].label, "WASD");
        assert_eq!(m.selection().unwrap().at, geometry.at);
        assert_eq!(m.selection().unwrap().radius, geometry.radius);
        assert!(!m.binding);
        assert!(m.profile.as_ref().unwrap().validate().is_ok());
    }

    #[test]
    fn an_unmappable_key_changes_nothing() {
        let mut m = editing();
        m.place((10.0, 10.0), (100.0, 100.0));
        assert!(!m.take_key("printscreen"));
        assert!(m.binding, "still waiting for a key it can use");
    }

    /// Nudging is why this is usable at all: a click is only as accurate as
    /// the pointer and game buttons are small.
    #[test]
    fn nudging_moves_the_selection_by_one_step() {
        let mut m = editing();
        m.place((500.0, 500.0), (1000.0, 1000.0));
        assert!(m.nudge(NUDGE, 0.0));
        let at = m.controls()[0].at;
        assert!((at.x - (0.5 + NUDGE)).abs() < 1e-6, "{at:?}");
    }

    #[test]
    fn nudging_stops_at_the_edge() {
        let mut m = editing();
        m.place((0.0, 0.0), (1000.0, 1000.0));
        m.nudge(-1.0, -1.0);
        let at = m.controls()[0].at;
        assert_eq!((at.x, at.y), (0.0, 0.0));
    }

    #[test]
    fn nudging_without_a_selection_does_nothing() {
        let mut m = editing();
        assert!(!m.nudge(NUDGE, 0.0));
    }

    /// Only the joystick has a size in the model. Offering one for a button
    /// would be a control that changes nothing.
    #[test]
    fn only_a_joystick_can_be_resized() {
        let mut m = editing();
        m.palette = Kind::Tap;
        m.place((10.0, 10.0), (100.0, 100.0));
        assert!(!m.resize(0.01));
        assert!(!Kind::Tap.has_size());

        let mut m = editing();
        m.palette = Kind::Joystick;
        m.place((10.0, 10.0), (100.0, 100.0));
        assert!(m.resize(0.01));
        assert!(Kind::Joystick.has_size());
        assert_eq!(m.controls()[0].radius, Some(0.13));
    }

    #[test]
    fn a_radius_cannot_be_made_absurd() {
        let mut m = editing();
        m.palette = Kind::Joystick;
        m.place((10.0, 10.0), (100.0, 100.0));
        m.resize(-10.0);
        assert_eq!(m.controls()[0].radius, Some(0.02));
        m.resize(10.0);
        assert_eq!(m.controls()[0].radius, Some(0.5));
    }

    #[test]
    fn removing_takes_the_selection_with_it() {
        let mut m = editing();
        m.place((10.0, 10.0), (100.0, 100.0));
        m.remove_selected();
        assert!(m.controls().is_empty());
        assert!(m.selected.is_none());
    }

    #[test]
    fn selecting_marks_exactly_one() {
        let mut m = editing();
        let a = m.place((10.0, 10.0), (100.0, 100.0)).unwrap();
        let b = m.place((20.0, 20.0), (100.0, 100.0)).unwrap();
        m.select(&a);
        let c = m.controls();
        assert!(c.iter().find(|x| x.key == a).unwrap().selected);
        assert!(!c.iter().find(|x| x.key == b).unwrap().selected);
    }

    #[test]
    fn selecting_something_that_is_not_there_changes_nothing() {
        let mut m = editing();
        let a = m.place((10.0, 10.0), (100.0, 100.0)).unwrap();
        m.select("nope");
        assert_eq!(m.selected.as_deref(), Some(a.as_str()));
    }

    /// Key(0) is KEY_RESERVED and can never be a real key, which is what
    /// makes it safe as the "not set yet" marker.
    #[test]
    fn an_unbound_control_shows_that_it_is_unbound() {
        let mut m = editing();
        m.place((10.0, 10.0), (100.0, 100.0));
        assert_eq!(m.controls()[0].label, "—");
    }

    #[test]
    fn nothing_is_editable_before_it_begins() {
        let mut m = Mapper::default();
        assert!(!m.is_on());
        assert!(m.place((1.0, 1.0), (10.0, 10.0)).is_none());
        assert!(m.controls().is_empty());
        assert!(m.selection().is_none());
    }
}
