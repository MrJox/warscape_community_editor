//! Keyboard shortcuts the user can rebind (Settings > Shortcuts). Every action that a key does has an id, a scope, a label and up to two
//! default keys ([`Action`]); the editor's are here ([`EDITOR`]), each tool declares its own (`Tool::shortcuts`). The settings keep only the
//! actions whose keys differ from their defaults (`[shortcuts]` in settings.toml: `id = ["Ctrl+S", ""]`), published with [`set_overrides`].
//! Code asks [`pressed`] (a key press) or [`down`] (a key held: the camera's moves) instead of reading keys, and menus show [`label`].
//!
//! A chord matches when its key is pressed and Ctrl, Shift and Alt are exactly its own; a held action ignores Shift unless its chord has it
//! (Shift is the camera's "faster"), and a modifier key bound alone (Left Shift) matches whatever else is held. Dialog keys (Esc cancels,
//! Enter confirms) and text editing are not actions.

use std::collections::{BTreeMap, HashMap};
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::gui;

/// A key with its modifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    /// Dear ImGui's key (`ImGuiKey_*`).
    pub key: i32,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// The keyboard's keys in Dear ImGui's numbering (no mouse or gamepad keys).
const KEYS: std::ops::Range<i32> = 512..632;

/// The modifier keys (Left / Right Ctrl, Shift, Alt, Super).
const MODIFIER_KEYS: std::ops::RangeInclusive<i32> = 527..=534;

/// Whether a key is a modifier key.
pub fn is_modifier(key: i32) -> bool {
    MODIFIER_KEYS.contains(&key)
}

impl Chord {
    /// `Ctrl+Shift+Z`, `F2`, `LeftShift`: the modifiers, then Dear ImGui's name of the key (case aside). None when it does not read.
    pub fn parse(text: &str) -> Option<Self> {
        let parts = text.split('+').map(str::trim).filter(|p| !p.is_empty()).collect::<Vec<_>>();
        let (name, mods) = parts.split_last()?;
        let mut c = Self { key: key_by_name(name)?, ctrl: false, shift: false, alt: false };
        for m in mods {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" => c.ctrl = true,
                "shift" => c.shift = true,
                "alt" => c.alt = true,
                _ => return None,
            }
        }

        Some(c)
    }

    /// As [`Self::parse`] reads it.
    pub fn text(&self) -> String {
        let mut s = String::new();
        for (on, name) in [(self.ctrl, "Ctrl+"), (self.shift, "Shift+"), (self.alt, "Alt+")] {
            if on {
                s.push_str(name);
            }
        }

        s.push_str(&gui::key_name(self.key));
        s
    }

    /// Whether the key was pressed this frame with exactly these modifiers.
    fn pressed(&self, repeat: bool) -> bool {
        let io = gui::io();
        gui::is_key_pressed(self.key, repeat) && (is_modifier(self.key) || (io.KeyCtrl == self.ctrl && io.KeyShift == self.shift && io.KeyAlt == self.alt))
    }

    /// Whether the key is held with these Ctrl and Alt (Shift too when the chord has it).
    fn down(&self) -> bool {
        let io = gui::io();
        gui::is_key_down(self.key) && (is_modifier(self.key) || (io.KeyCtrl == self.ctrl && io.KeyAlt == self.alt && (!self.shift || io.KeyShift)))
    }
}

fn key_by_name(name: &str) -> Option<i32> {
    KEYS.into_iter().find(|&k| gui::key_name(k).eq_ignore_ascii_case(name))
}

/// Where an action's keys work, for the clashes: the editor's work everywhere, the viewport's while the mouse is over it, a tool's while it is
/// the active tool (by its id).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Global,
    Viewport,
    Tool(&'static str),
}

impl Scope {
    /// Whether keys of the two scopes can be read at the same time (one key may then do two things).
    pub fn overlaps(self, other: Scope) -> bool {
        match (self, other) {
            (Scope::Global, _) | (_, Scope::Global) | (Scope::Viewport, Scope::Viewport) | (Scope::Viewport, Scope::Tool(_)) | (Scope::Tool(_), Scope::Viewport) => true,
            (Scope::Tool(a), Scope::Tool(b)) => a == b,
        }
    }
}

/// Something a key does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Action {
    pub id: &'static str,
    pub scope: Scope,

    /// The group it is listed under (Settings > Shortcuts) and what it does.
    pub group: &'static str,
    pub label: &'static str,

    /// Its default keys (`Chord::parse`'s text), at most two.
    pub defaults: &'static [&'static str],

    /// Held, not pressed (the camera's moves): `down`.
    pub held: bool,
}

const fn act(id: &'static str, scope: Scope, group: &'static str, label: &'static str, defaults: &'static [&'static str]) -> Action {
    Action { id, scope, group, label, defaults, held: false }
}

const fn held(id: &'static str, label: &'static str, defaults: &'static [&'static str]) -> Action {
    Action { id, scope: Scope::Viewport, group: "Camera", label, defaults, held: true }
}

/// The editor's own actions.
pub const EDITOR: &[Action] = &[
    act("save", Scope::Global, "File", "Save", &["Ctrl+S"]),
    act("save_all", Scope::Global, "File", "Save all", &["Ctrl+Shift+S"]),
    act("undo", Scope::Global, "Edit", "Undo", &["Ctrl+Z"]),
    act("redo", Scope::Global, "Edit", "Redo", &["Ctrl+Y", "Ctrl+Shift+Z"]),
    act("tool_1", Scope::Global, "Tools", "Switch to the 1st tool", &["Ctrl+1"]),
    act("tool_2", Scope::Global, "Tools", "Switch to the 2nd tool", &["Ctrl+2"]),
    act("tool_3", Scope::Global, "Tools", "Switch to the 3rd tool", &["Ctrl+3"]),
    act("tool_4", Scope::Global, "Tools", "Switch to the 4th tool", &["Ctrl+4"]),
    act("tool_5", Scope::Global, "Tools", "Switch to the 5th tool", &["Ctrl+5"]),
    act("tool_6", Scope::Global, "Tools", "Switch to the 6th tool", &["Ctrl+6"]),
    act("tool_7", Scope::Global, "Tools", "Switch to the 7th tool", &["Ctrl+7"]),
    act("tool_8", Scope::Global, "Tools", "Switch to the 8th tool", &["Ctrl+8"]),
    act("tool_9", Scope::Global, "Tools", "Switch to the 9th tool", &["Ctrl+9"]),
    act("gizmo_none", Scope::Viewport, "Viewport", "Gizmo: none", &["1"]),
    act("gizmo_move", Scope::Viewport, "Viewport", "Gizmo: move", &["2"]),
    act("gizmo_rotate", Scope::Viewport, "Viewport", "Gizmo: rotate", &["3"]),
    act("gizmo_scale", Scope::Viewport, "Viewport", "Gizmo: scale", &["4"]),
    act("focus", Scope::Viewport, "Viewport", "Focus the selection", &["F"]),
    act("local_coordinates", Scope::Viewport, "Viewport", "Global / local coordinates", &["L"]),
    held("camera_forward", "Move forward", &["W"]),
    held("camera_back", "Move back", &["S"]),
    held("camera_left", "Move left", &["A"]),
    held("camera_right", "Move right", &["D"]),
    held("camera_turn_left", "Turn left (fly camera: down)", &["Q"]),
    held("camera_turn_right", "Turn right (fly camera: up)", &["E"]),
    held("camera_fast", "Faster (held with the moves)", &["LeftShift", "RightShift"]),
];

/// The actions known (the editor's and the tools'), in their order.
static ACTIONS: RwLock<Vec<Action>> = RwLock::new(Vec::new());

/// The user's keys of the actions that differ from their defaults.
static OVERRIDES: RwLock<Option<HashMap<String, [Option<Chord>; 2]>>> = RwLock::new(None);

/// The Settings page is reading a key: no action fires.
static CAPTURING: AtomicBool = AtomicBool::new(false);

/// Adds actions (the editor's, a tool's); one already known by its id is left as it is.
pub fn register(actions: &[Action]) {
    if let Ok(mut all) = ACTIONS.write() {
        for a in actions {
            if !all.iter().any(|b| b.id == a.id) {
                all.push(*a);
            }
        }
    }
}

/// Every action known.
pub fn actions() -> Vec<Action> {
    if ACTIONS.read().is_ok_and(|a| a.is_empty()) {
        register(EDITOR);
    }

    ACTIONS.read().map(|a| a.clone()).unwrap_or_default()
}

fn action(id: &str) -> Option<Action> {
    actions().into_iter().find(|a| a.id == id)
}

/// An action's default keys.
pub fn defaults(a: &Action) -> [Option<Chord>; 2] {
    [a.defaults.first().and_then(|t| Chord::parse(t)), a.defaults.get(1).and_then(|t| Chord::parse(t))]
}

/// The keys of an action in a settings table (`[shortcuts]`): its entry, else its defaults.
pub fn keys_in(a: &Action, table: &BTreeMap<String, Vec<String>>) -> [Option<Chord>; 2] {
    match table.get(a.id) {
        Some(v) => [v.first().and_then(|t| Chord::parse(t)), v.get(1).and_then(|t| Chord::parse(t))],
        None => defaults(a),
    }
}

/// Sets an action's keys in a settings table; keys that are its defaults take its entry out.
pub fn set_keys_in(a: &Action, table: &mut BTreeMap<String, Vec<String>>, keys: [Option<Chord>; 2]) {
    if keys == defaults(a) {
        table.remove(a.id);
    } else {
        table.insert(a.id.to_owned(), keys.iter().map(|k| k.map_or_else(String::new, |c| c.text())).collect());
    }
}

/// Uses the settings' table (`Settings::shortcuts`).
pub fn set_overrides(table: &BTreeMap<String, Vec<String>>) {
    let map = actions().iter().filter(|a| table.contains_key(a.id)).map(|a| (a.id.to_owned(), keys_in(a, table))).collect::<HashMap<_, _>>();
    if let Ok(mut o) = OVERRIDES.write() {
        *o = Some(map);
    }
}

/// An action's keys now.
pub fn keys(id: &str) -> [Option<Chord>; 2] {
    if let Some(k) = OVERRIDES.read().ok().and_then(|o| o.as_ref().and_then(|m| m.get(id).copied())) {
        return k;
    }

    action(id).map_or([None, None], |a| defaults(&a))
}

/// Whether one of the action's keys was pressed this frame (`repeat`: and while it is held, at the key repeat's rate).
pub fn pressed(id: &str, repeat: bool) -> bool {
    !CAPTURING.load(Ordering::Relaxed) && keys(id).iter().flatten().any(|c| c.pressed(repeat))
}

/// Whether one of the action's keys is held.
pub fn down(id: &str) -> bool {
    !CAPTURING.load(Ordering::Relaxed) && keys(id).iter().flatten().any(Chord::down)
}

/// The action's first key, for a menu item.
pub fn label(id: &str) -> Option<String> {
    keys(id).iter().flatten().next().map(Chord::text)
}

/// The Settings page reads a key (`true`) or stops: meanwhile no action fires.
pub fn set_capturing(on: bool) {
    CAPTURING.store(on, Ordering::Relaxed);
}

/// The key pressed this frame with the modifiers held, for the Settings page. A modifier key alone counts only for a held action.
pub fn captured(held: bool) -> Option<Chord> {
    let io = gui::io();
    let key = KEYS.into_iter().find(|&k| gui::is_key_pressed(k, false) && (held || !is_modifier(k)))?;
    if is_modifier(key) {
        return Some(Chord { key, ctrl: false, shift: false, alt: false });
    }

    Some(Chord { key, ctrl: io.KeyCtrl, shift: io.KeyShift, alt: io.KeyAlt })
}

/// The clashes of a settings table: pairs of actions whose scopes overlap with a key in common, as (action, other action, key).
pub fn clashes(table: &BTreeMap<String, Vec<String>>) -> Vec<(Action, Action, Chord)> {
    let all = actions();
    let keys = all.iter().map(|a| keys_in(a, table)).collect::<Vec<_>>();
    let mut out = vec![];
    for i in 0..all.len() {
        for j in i + 1..all.len() {
            if !all[i].scope.overlaps(all[j].scope) {
                continue;
            }

            for c in keys[i].iter().flatten() {
                if keys[j].iter().flatten().any(|d| d == c) {
                    out.push((all[i], all[j], *c));
                }
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_overlap_as_the_keys_are_read() {
        assert!(Scope::Global.overlaps(Scope::Tool("tile")));
        assert!(Scope::Viewport.overlaps(Scope::Tool("tile")));
        assert!(Scope::Tool("tile").overlaps(Scope::Tool("tile")));
        assert!(!Scope::Tool("tile").overlaps(Scope::Tool("ui")));
    }
}
