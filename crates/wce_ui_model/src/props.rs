//! What each field of a component, state, picture and mouse action means, in the game's words.
//!
//! The converter names fields the way the format's reverse engineers guessed them ("text xsize?", "uientry flag 7").
//! The names and meanings here come from the game's own code (`TWUI::Component`, `ComponentState`,
//! `ComponentImageMetrics` and their loaders in the decompiled executable): every member the loader fills in is named
//! after it, and every flag word or number that selects a mode gets a check box or a drop-down.
//! Nothing here changes the XML: a `Prop` points at a field of the tree and says how to show and edit it.

use crate::dom::{Element, Node};
use crate::model::{color_of, UiModel};

#[derive(Debug, Clone, PartialEq)]
pub enum Control {
    Text,
    Int,
    Float,
    /// A yes/no field.
    Check,
    /// An integer that selects one of a list.
    Choice(Vec<(i64, String)>),
    /// Text with common values to pick from.
    Suggest(&'static [&'static str]),
    /// One bit (or byte) of an integer word: `byte` fields are 0 or 1 in their own byte, the others are bits of a mask.
    Bit { mask: u32, byte: bool },
    /// A `<color>` element.
    Colour,
    /// Shown but not edited here.
    Info,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Prop {
    /// Index in the element's children (the field, or the `<color>` element).
    pub idx: usize,
    pub name: String,
    pub help: &'static str,
    pub group: &'static str,
    pub control: Control,
    /// The field's text (for a colour: `#rrggbbaa`).
    pub value: String,
    /// The converter's own name for the field, for scripts that know it.
    pub label: String,
    /// Rarely needed; the panel folds these away.
    pub advanced: bool,
}

#[derive(Debug, Clone)]
pub struct Props {
    pub kind: &'static str,
    pub groups: &'static [&'static str],
    pub items: Vec<Prop>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PropEdit {
    Value(String),
    /// A value being dragged: the same as `Value`, but consecutive drags are one undo step.
    Drag(String),
    Bit { mask: u32, byte: bool, on: bool },
    Colour([u8; 4]),
}

const COMPONENT_GROUPS: &[&str] = &["Identity", "Position & size", "Docking", "Behaviour", "Tooltip", "Sound & transitions", "Advanced"];
const STATE_GROUPS: &[&str] = &["State", "Text", "Font", "Shader", "Behaviour", "Advanced"];
const PICTURE_GROUPS: &[&str] = &["Picture", "Position & size", "Appearance", "Docking", "Rotation", "Nine-patch", "Advanced"];
const MOUSE_GROUPS: &[&str] = &["Mouse action", "Fade"];
const OTHER_GROUPS: &[&str] = &["Fields"];

pub const DOCK_POINTS: &[(i64, &str)] = &[
    (0, "None (use the offset)"),
    (1, "Top left"),
    (2, "Top centre"),
    (3, "Top right"),
    (4, "Centre left"),
    (5, "Centre"),
    (6, "Centre right"),
    (7, "Bottom left"),
    (8, "Bottom centre"),
    (9, "Bottom right"),
    (10, "Outside: top left"),
    (11, "Outside: top centre"),
    (12, "Outside: top right"),
    (13, "Outside: centre left"),
    (14, "Outside: centre"),
    (15, "Outside: centre right"),
    (16, "Outside: bottom left"),
    (17, "Outside: bottom centre"),
    (18, "Outside: bottom right"),
];

const MOVEABLE: &[(i64, &str)] = &[(0, "Not moveable"), (1, "Moveable (XP style)"), (2, "Moveable (Windows 7 style)")];
const DRAW_MODE: &[(i64, &str)] = &[(0, "Normal"), (1, "Unscaled"), (2, "Fit to screen")];
const H_ALIGN: &[(i64, &str)] = &[(2, "Left"), (3, "Right"), (4, "Centre"), (0, "Left (stored as 0)")];
const V_ALIGN: &[(i64, &str)] = &[(0, "Top"), (1, "Bottom"), (4, "Centre"), (2, "Top (stored as 2)")];
const FOCUS: &[(i64, &str)] = &[(0, "None"), (1, "Active (can take keyboard focus)")];
const WRAP: &[(i64, &str)] = &[(0, "Split by character"), (1, "Split by word"), (2, "Never split"), (3, "Shrink the string to fit")];
const MOUSE_WHEN: &[(i64, &str)] = &[
    (0, "Mouse enters"),
    (1, "Mouse leaves"),
    (2, "Left button down"),
    (3, "Left button up"),
    (4, "Right button down"),
    (5, "Right button up"),
    (6, "Middle button down"),
    (7, "Middle button up"),
    (8, "Left click released"),
    (9, "Right click released"),
    (10, "Middle click released"),
    (11, "Mouse wheel pulled"),
    (12, "Mouse wheel pushed"),
];

pub const SHADERS: &[&str] = &["normal_t0", "brighten_t0", "set_greyscale_t0", "glow_pulse_t0", "border_alpha_blend", "red_pulse_t0", "grey_over_time_t0", "drop_shadow_t0"];
pub const FONTS: &[&str] = &["la_gioconda", "la_gioconda_uppercase", "georgia_italic", "arialunicode", "candara_italic"];
pub const FONT_CATEGORIES: &[&str] = &[
    "Default Font Category",
    "ingame_text",
    "ingame_text_small",
    "ingame_text_small_grey",
    "ingame_text_red",
    "ingame_text_green",
    "ingame_tooltip",
    "ingame_tooltip_italic",
    "ingame_section_heading",
    "ingame_panel_heading",
    "ingame_paragraph_heading",
    "ingame_paragraph_heading_red",
    "fe_text",
    "fe_default_text",
    "fe_text_red",
    "fe_text_green",
    "fe_text_gold",
    "fe_text_blue",
    "fe_italic",
    "fe_button_text",
    "fe_input_text",
    "fe_hyperlink",
    "fe_hyperlink_small",
    "fe_page_heading",
    "fe_section_heading",
    "fe_section_heading_inactive",
    "fe_paragraph_heading",
    "fe_paragraph_heading_inactive",
    "fe_paragraph_heading_red",
    "fe_paragraph_heading_blue",
    "fe_tooltip_text",
    "fe_tooltip_green",
    "fe_tooltip_red",
    "parchment_text",
    "parchment_text_red",
    "parchment_text_inactive",
    "parchment italic",
    "parchment_header",
    "parchment_header_small",
    "parchment_header_large",
    "parchment_header_inactive",
    "chat",
    "subtitle",
    "scripted_objective",
    "name_unicode",
    "name_unicode_white",
    "name_unicode_parchment",
];
pub const SOUND_CATEGORIES: &[&str] = &[
    "grp_gw_generic_button_small",
    "grp_gw_generic_button",
    "grp_gw_generic_button_mouse_over_only",
    "grp_gw_generic_drop_down_list",
    "grp_gw_drop_down",
    "grp_gw_slider_handle",
    "grp_gw_tick_box",
    "grp_gw_accept_confirm",
    "grp_gw_cancel_close",
    "grp_gw_cycle_arrow",
    "grp_gw_tab",
    "grp_gw_unit_card",
    "grp_gw_encyclopedia",
    "grp_gw_generic_pop_up_open",
    "grp_opt_tab",
    "grp_opt_generic_button",
    "grp_cam_tab",
    "grp_cam_generic_button_small",
    "grp_fe_faction_select",
    "grp_fe_purchase",
    "grp_fe_hb_select_battle",
    "grp_bat_special_ability",
    "pop_up_bat_generic",
    "pop_up_cam_generic",
    "pop_up_cam_event",
    "pop_up_gw_option_menu",
    "ind_cam_tactical_map",
    "ind_cam_faction",
    "ind_fe_start_campaign",
    "ind_fe_start_battle",
    "ind_fe_start_prologue",
    "ind_bat_end_deployment",
    "ind_bat_end_battle",
    "ind_bat_first_person",
    "ind_bat_tactical_map",
    "placeholder_default",
];

/// A field with where it sits among the labelled ones: the last label before it and how many unlabelled fields came since.
pub(crate) struct Item {
    pub(crate) idx: usize,
    pub(crate) tag: String,
    pub(crate) value: String,
    pub(crate) label: String,
    pub(crate) after: String,
    pub(crate) since: usize,
    pub(crate) nth: usize,
}

pub(crate) struct Def {
    pub(crate) name: &'static str,
    help: &'static str,
    group: &'static str,
    control: Control,
    advanced: bool,
}

fn d(name: &'static str, group: &'static str, control: Control, help: &'static str) -> Option<Def> {
    Some(Def { name, help, group, control, advanced: false })
}

fn adv(name: &'static str, group: &'static str, control: Control, help: &'static str) -> Option<Def> {
    Some(Def { name, help, group, control, advanced: true })
}

fn choice(list: &[(i64, &str)]) -> Control {
    Control::Choice(list.iter().map(|(n, s)| (*n, s.to_string())).collect())
}

/// The first line of a converter label as a readable name ("text xsize?" -> "Text xsize").
fn tidy(label: &str) -> String {
    let t = label.trim().trim_end_matches('?').trim();
    let mut c = t.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

pub(crate) fn items(el: &Element) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    let mut after = String::new();
    let mut since = 0usize;
    let mut seen: std::collections::HashMap<String, usize> = Default::default();
    for (i, n) in el.children.iter().enumerate() {
        let Node::Elem(c) = n else { continue };
        if !(c.is_leaf() && matches!(c.name.as_str(), "u" | "i" | "u2" | "i2" | "byte" | "flt" | "s" | "unicode" | "uuid" | "yes" | "no")) {
            continue;
        }

        let label = el.label_of(i).unwrap_or("").to_string();
        let value = match c.name.as_str() {
            "yes" | "no" => c.name.clone(),
            _ => c.text(),
        };
        let nth = if label.is_empty() {
            0
        } else {
            let k = seen.entry(label.clone()).or_insert(0);
            let v = *k;
            *k += 1;
            v
        };
        out.push(Item { idx: i, tag: c.name.clone(), value, label: label.clone(), after: after.clone(), since, nth });
        if label.is_empty() {
            since += 1;
        } else {
            after = label;
            since = 0;
        }
    }
    out
}

impl UiModel {
    /// The names and controls for the fields of the element at `path`: a component, a state, a picture or a mouse action.
    pub fn properties(&self, path: &[usize]) -> Props {
        let Some(el) = self.dom.at(path) else { return Props { kind: "", groups: OTHER_GROUPS, items: vec![] } };
        let v = self.version;
        let (kind, groups): (&'static str, &'static [&'static str]) = match el.name.as_str() {
            "uientry" => ("component", COMPONENT_GROUPS),
            "state" => ("state", STATE_GROUPS),
            "image_use" => ("picture", PICTURE_GROUPS),
            "mouse_state" => ("mouse", MOUSE_GROUPS),
            _ => ("other", OTHER_GROUPS),
        };

        let states = || self.state_choices(path, kind);
        let mut out: Vec<Prop> = Vec::new();
        for it in items(el) {
            let def = match kind {
                "component" => component_def(&it, v),
                "state" => state_def(&it, v),
                "picture" => picture_def(&it, v),
                "mouse" => mouse_def(&it),
                _ => None,
            };

            let (mut name, help, group, mut control, advanced, known) = match def {
                Some(df) => (df.name.to_string(), df.help, df.group, df.control, df.advanced, true),
                None => (String::new(), "", groups[groups.len() - 1], Control::Info, true, false),
            };

            if hidden(kind, &it, v) {
                continue;
            }

            if !known {
                // Anything the tables do not know keeps the converter's name and an ordinary control for its type.
                name = if it.label.is_empty() { format!("Unknown {} ({})", it.tag, it.idx) } else { tidy(&it.label) };
                control = match it.tag.as_str() {
                    "yes" | "no" => Control::Check,
                    "u" | "i" | "u2" | "i2" | "byte" => Control::Int,
                    "flt" => Control::Float,
                    _ => Control::Text,
                };
            }

            if let Control::Choice(c) = &control {
                if c.is_empty() {
                    let signed = it.tag == "i";
                    let mut list = if kind == "picture" { self.picture_choices_of_use(path) } else { states() };
                    if it.label == "default state id" {
                        list.insert(0, (0, "First state".to_string()));
                    }

                    // a state id is unsigned; a field that stores it as a signed integer shows it as one
                    control = Control::Choice(list.into_iter().map(|(n, s)| (if signed { n as u32 as i32 as i64 } else { n }, s)).collect());
                }
            }

            // Several check boxes over one word: one prop per bit.
            if let Some(bits) = bits_for(kind, &it, v) {
                for (mask, byte, nm, hp) in bits {
                    out.push(Prop { idx: it.idx, name: nm.to_string(), help: hp, group: bits_group(kind), control: Control::Bit { mask, byte }, value: it.value.clone(), label: it.label.clone(), advanced: false });
                }
                continue;
            }

            let mut value = it.value.clone();
            if it.tag == "u" && matches!(control, Control::Int) {
                if let Ok(n) = value.trim().parse::<i64>() {
                    value = (n as u32 as i32).to_string();
                }
            }

            out.push(Prop { idx: it.idx, name, help, group, control, value, label: it.label.clone(), advanced });
        }

        // The tint or text colour is a `<color>` element, not a field.
        for (i, n) in el.children.iter().enumerate() {
            if let Node::Elem(c) = n {
                if c.name == "color" {
                    let [r, g, b, a] = color_of(c);
                    let (name, help, group): (&str, &'static str, &'static str) = match kind {
                        "state" => ("Text colour", "The colour the state's text is drawn in.", "Font"),
                        "picture" => ("Tint colour", "Multiplied with the picture; white leaves it as it is, the alpha is its opacity.", "Appearance"),
                        _ => ("Colour", "", groups[0]),
                    };
                    out.push(Prop { idx: i, name: name.into(), help, group, control: Control::Colour, value: format!("#{r:02x}{g:02x}{b:02x}{a:02x}"), label: "color".into(), advanced: false });
                }
            }
        }

        out.sort_by_key(|p| p.idx);
        Props { kind, groups, items: out }
    }

    /// The component that owns the state, picture or mouse action at `path`.
    fn owner_path(path: &[usize], kind: &str) -> Vec<usize> {
        let cut = match kind {
            "state" => 2,
            "picture" | "mouse" => 4,
            _ => 0,
        };
        path[..path.len().saturating_sub(cut)].to_vec()
    }

    /// (state id, name) of the states of the component that owns the element at `path`.
    fn state_choices(&self, path: &[usize], kind: &str) -> Vec<(i64, String)> {
        let owner = Self::owner_path(path, kind);
        let Some(el) = self.dom.at(&owner) else { return vec![] };
        let Some(states) = el.find("states") else { return vec![] };
        states
            .elems()
            .filter(|s| s.name == "state")
            .map(|s| {
                let id = s.find("u").and_then(|u| u.text().trim().parse::<i64>().ok()).unwrap_or(0);
                let name = s.find("s").map(|n| n.text()).unwrap_or_default();
                (id, name)
            })
            .collect()
    }

    /// (picture id, path) of the component's picture list.
    pub fn picture_choices(&self, owner: &[usize]) -> Vec<(i64, String)> {
        let Some(el) = self.dom.at(owner) else { return vec![] };
        let Some(images) = el.find("images") else { return vec![] };
        images
            .elems()
            .filter(|s| s.name == "image")
            .map(|s| {
                let id = s.find("u").and_then(|u| u.text().trim().parse::<i64>().ok()).unwrap_or(0);
                let path = s.find("s").map(|n| n.text()).unwrap_or_default();
                (id, path)
            })
            .collect()
    }

    /// Paths of the states of the component at `entry`, with their names.
    pub fn state_paths(&self, entry: &[usize]) -> Vec<(Vec<usize>, String)> {
        let Some(el) = self.dom.at(entry) else { return vec![] };
        let Some(si) = el.find_idx("states") else { return vec![] };
        let Some(states) = el.elem_at(si) else { return vec![] };
        states
            .children
            .iter()
            .enumerate()
            .filter_map(|(k, n)| match n {
                Node::Elem(s) if s.name == "state" => {
                    let mut p = entry.to_vec();
                    p.push(si);
                    p.push(k);
                    Some((p, s.find("s").map(|n| n.text()).unwrap_or_default()))
                }
                _ => None,
            })
            .collect()
    }

    /// Paths of the pictures (`image_use`) and mouse actions (`mouse_state`) of a state.
    pub fn state_children(&self, state: &[usize], tag: &str) -> Vec<Vec<usize>> {
        let Some(el) = self.dom.at(state) else { return vec![] };
        let Some(ci) = el.find_idx(if tag == "image_use" { "image_uses" } else { "mouse_states" }) else { return vec![] };
        let Some(list) = el.elem_at(ci) else { return vec![] };
        list.children
            .iter()
            .enumerate()
            .filter_map(|(k, n)| match n {
                Node::Elem(s) if s.name == tag => {
                    let mut p = state.to_vec();
                    p.push(ci);
                    p.push(k);
                    Some(p)
                }
                _ => None,
            })
            .collect()
    }

    /// The pictures of a component as (id, path), for the "Picture" drop-down of a picture use.
    pub fn picture_choices_of_use(&self, use_path: &[usize]) -> Vec<(i64, String)> {
        self.picture_choices(&Self::owner_path(use_path, "picture"))
    }

    /// A short name for a mouse action: its event and the state it goes to.
    pub fn mouse_label(&self, path: &[usize]) -> String {
        let Some(el) = self.dom.at(path) else { return String::new() };
        let when: i64 = el.find("i").and_then(|c| c.text().trim().parse().ok()).unwrap_or(-1);
        let when = MOUSE_WHEN.iter().find(|(n, _)| *n == when).map(|(_, s)| s.to_string()).unwrap_or_else(|| format!("event {when}"));
        let target: i64 = el.find("u").and_then(|c| c.text().trim().parse().ok()).unwrap_or(-1);
        let to = self.state_choices(path, "mouse").into_iter().find(|(n, _)| *n == target).map(|(_, s)| s).unwrap_or_else(|| "?".into());
        format!("{when} -> {to}")
    }

    /// A short name for a picture use: the picture it draws.
    pub fn picture_label(&self, use_path: &[usize]) -> String {
        let id = self.dom.at(use_path).and_then(|e| e.find("u")).and_then(|u| u.text().trim().parse::<i64>().ok());
        let path = id.and_then(|id| self.picture_choices_of_use(use_path).into_iter().find(|(n, _)| *n == id)).map(|(_, p)| p).unwrap_or_default();
        if path.is_empty() { "(no picture)".into() } else { path.rsplit('/').next().unwrap_or(&path).to_string() }
    }

    /// Changes a field through its property description.
    pub fn edit_prop(&mut self, path: &[usize], idx: usize, edit: PropEdit) -> Result<(), String> {
        match edit {
            PropEdit::Colour(rgba) => {
                let el = self.dom_mut().at_mut(path).ok_or("no such element")?;
                let c = el.elem_at_mut(idx).ok_or("no such colour")?;
                crate::model::set_color_pub(c, rgba);
                Ok(())
            }
            PropEdit::Value(s) | PropEdit::Drag(s) => {
                let tag = self.dom.at(path).and_then(|e| e.elem_at(idx)).map(|c| c.name.clone()).ok_or("no such field")?;
                if tag == "u" {
                    if let Ok(n) = s.trim().parse::<i64>() {
                        if n < 0 {
                            return self.set_field(path, idx, &(n as i32 as u32).to_string());
                        }
                    }
                }

                self.set_field(path, idx, &s)
            }
            PropEdit::Bit { mask, byte, on } => {
                let cur = self.dom.at(path).and_then(|e| e.elem_at(idx)).map(|c| c.text()).ok_or("no such field")?;
                let n: i64 = cur.trim().parse().map_err(|_| "not an integer".to_string())?;
                let mut w = n as u32;
                if byte {
                    // 0xFF over the byte the mask sits in
                    let lane = (mask.trailing_zeros() / 8) * 8;
                    w &= !(0xFFu32 << lane);
                    if on {
                        w |= mask;
                    }
                } else if on {
                    w |= mask;
                } else {
                    w &= !mask;
                }

                let signed = self.dom.at(path).and_then(|e| e.elem_at(idx)).map(|c| c.name == "i").unwrap_or(false);
                let text = if signed { (w as i32).to_string() } else { w.to_string() };
                self.set_field(path, idx, &text)
            }
        }
    }
}

impl Prop {
    /// The value as a person reads it: the choice's name, yes/no for a check box or bit, `#rrggbbaa` for a colour.
    pub fn shown(&self) -> String {
        match &self.control {
            Control::Choice(list) => {
                let cur: i64 = self.value.trim().parse().unwrap_or(i64::MIN);
                list.iter().find(|(n, _)| *n == cur).map(|(_, s)| s.clone()).unwrap_or_else(|| format!("Other ({})", self.value))
            }
            Control::Bit { mask, byte } => {
                let w = self.value.trim().parse::<i64>().unwrap_or(0) as u32;
                (if *byte { w & mask != 0 } else { w & mask == *mask }).then_some("yes").unwrap_or("no").to_string()
            }
            _ => self.value.clone(),
        }
    }

    /// The edit that sets this property from text typed by a person or a script: a choice by name or number, yes/no for check
    /// boxes and bits, `#rrggbb[aa]` for a colour, else the text itself.
    pub fn edit_from_text(&self, text: &str) -> Result<PropEdit, String> {
        let t = text.trim();
        let yes = || match t.to_ascii_lowercase().as_str() {
            "yes" | "true" | "1" | "on" => Ok(true),
            "no" | "false" | "0" | "off" => Ok(false),
            _ => Err(format!("{:?} is not yes or no", t)),
        };
        match &self.control {
            Control::Check => Ok(PropEdit::Value(if yes()? { "yes" } else { "no" }.into())),
            Control::Bit { mask, byte } => Ok(PropEdit::Bit { mask: *mask, byte: *byte, on: yes()? }),
            Control::Choice(list) => {
                if let Some((n, _)) = list.iter().find(|(_, s)| s.eq_ignore_ascii_case(t)) {
                    return Ok(PropEdit::Value(n.to_string()));
                }

                if t.parse::<i64>().is_ok() {
                    return Ok(PropEdit::Value(t.to_string()));
                }

                let names: Vec<&str> = list.iter().map(|(_, s)| s.as_str()).collect();
                Err(format!("{:?} is not one of: {}", t, names.join(", ")))
            }
            Control::Colour => {
                let h = t.trim_start_matches('#');
                if !(h.len() == 6 || h.len() == 8) || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err("a colour is RRGGBB or RRGGBBAA".into());
                }

                let ch = |i: usize| u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).unwrap_or(255);
                Ok(PropEdit::Colour([ch(0), ch(1), ch(2), if h.len() == 8 { ch(3) } else { 255 }]))
            }
            Control::Info => Err("this property cannot be edited".into()),
            _ => Ok(PropEdit::Value(t.to_string())),
        }
    }
}

/// The mask bits of an integer word are shown as check boxes.
pub(crate) fn bits_for(kind: &str, it: &Item, v: u32) -> Option<Vec<(u32, bool, &'static str, &'static str)>> {
    match kind {
        "state" if it.tag == "i" && it.since == 0 && (it.after == "bottom ?" || it.after == "y ?") => {
            let mut b = vec![
                (0x1, true, "Interactive", "The state reacts to the mouse."),
                (0x100, true, "Disabled", "Drawn as disabled; ignores clicks."),
                (0x1_0000, true, "Pixel-accurate hit test", "A click counts only where the picture is not transparent, instead of anywhere in the rectangle."),
            ];
            if v >= 80 {
                b.push((0x100_0000, true, "Dock pictures as a nine-patch", "Pictures dock to the neighbouring pictures (corners, edges, middle) instead of to the state's rectangle."));
            }

            Some(b)
        }
        "mouse" if it.tag == "i" && it.since == 2 && it.after == "type" => Some(vec![
            (0x01, false, "Fade colour", "Blend the colour into the next state over the transition time."),
            (0x02, false, "Fade position", "Blend the pictures' positions."),
            (0x04, false, "Fade size", "Blend the pictures' sizes."),
            (0x08, false, "Fade shader values", "Blend the shader values."),
            (0x10, false, "Fade rotation", "Blend the rotation."),
            (0x20, false, "Cross-fade picture", "Cross-fade to the other state's picture."),
            (0x40, false, "Fade opacity", "Blend the opacity."),
            (0x80, false, "Fade text colour", "Blend the text colour."),
        ]),
        _ => None,
    }
}

fn bits_group(kind: &str) -> &'static str {
    if kind == "mouse" { "Fade" } else { "Behaviour" }
}

/// Fields that are padding or only a part of another field.
fn hidden(kind: &str, it: &Item, v: u32) -> bool {
    match kind {
        // Before version 90 the converter's flag 7 is the extra flag (the visible one is its flag 8, see `component_def`).
        "component" => matches!(it.label.as_str(), "uientry flag 4" | "uientry flag 5" | "uientry flag 6")
            || (v < 90 && it.label == "uientry flag 7")
            || (it.tag == "u" && it.label.is_empty() && it.after.is_empty() && it.since == 0),
        "state" => v < 116 && matches!(it.label.as_str(), "text label?" | "state stuff 3?") || (it.tag == "u" && it.label.is_empty() && it.since == 0 && it.after.is_empty()),
        _ => false,
    }
}

pub(crate) fn component_def(it: &Item, v: u32) -> Option<Def> {
    // Before version 90 the entry has a thirteenth flag, before the visible one (Attila's and Rome 2's layouts: always off): the
    // converter's flags 8 .. 12 and the unlabelled one after them are the flags 7 .. 12.
    if v < 90 {
        let shifted = match (it.label.as_str(), it.after.as_str(), it.since, it.tag.as_str()) {
            ("", "uientry flag 12", 0, "yes" | "no") => Some("uientry flag 12".to_owned()),
            (label, ..) => label.strip_prefix("uientry flag ").and_then(|n| n.parse::<u32>().ok()).filter(|n| (8..=12).contains(n)).map(|n| format!("uientry flag {}", n - 1)),
        };
        if let Some(label) = shifted {
            let item = Item { idx: it.idx, tag: it.tag.clone(), value: it.value.clone(), label, after: String::new(), since: 0, nth: it.nth };
            return component_def(&item, 90);
        }
    }

    // The fields that have no label of their own are named by the labelled field before them.
    if it.label.is_empty() {
        return match (it.after.as_str(), it.since, it.tag.as_str()) {
            ("docking y?", 0, "yes" | "no") | ("docking?", 0, "yes" | "no") if v < 77 || it.after == "docking y?" => {
                d("Render last when focused", "Behaviour", Control::Check, "Drawn on top of everything when it has the focus.")
            }
            ("mask image?", 0, "i") if v < 110 => d("Picture draw mode", "Behaviour", choice(DRAW_MODE), "How the component's pictures are scaled."),
            ("end of uientry flag 6B?", 0, "yes" | "no") | ("end of uientry flag 5A?", 0, "yes" | "no") if v >= 94 => {
                d("Relative resize", "Behaviour", Control::Check, "When this component is resized, its children grow or shrink by the same amount instead of being scaled.")
            }
            ("end of uientry flag 6B?" | "end of uientry flag 5A?", _, "flt") if v >= 113 => adv("Unknown (extra values)", "Advanced", Control::Float, ""),
            _ => None,
        };
    }

    match it.label.as_str() {
        "title" => d("Name", "Identity", Control::Text, "The component's name. Scripts and templates find it by this."),
        "title2" => d("Callback name", "Identity", Control::Text, "Names the code that drives the component (it falls back to the name). The tree shows it as the class."),
        "x offset" => d("Offset X", "Position & size", Control::Int, "Distance from the parent's left edge. The game ignores it for a docked component and uses the dock point and dock offset."),
        "y offset" => d("Offset Y", "Position & size", Control::Int, "Distance from the parent's top edge. The game ignores it for a docked component."),
        "uientry flag 1" => d("Can resize horizontally", "Position & size", Control::Check, "The layout may change this component's width (screen resizing, resizing its parent)."),
        "uientry flag 2" => d("Can resize vertically", "Position & size", Control::Check, "The layout may change this component's height."),
        "uientry flag 3" => d("Moveable", "Behaviour", choice(MOVEABLE), "Whether the player can drag the component, and with which drag style."),
        "uientry flag 7" => d("Visible", "Behaviour", Control::Check, "Shown when the layout loads (scripts and states can show and hide it later)."),
        "uientry flag 8" => d("Clip children", "Behaviour", Control::Check, "Children are cut off at this component's edges; lists and scrolling panels need it."),
        "uientry flag 9" => d("Clip pictures", "Behaviour", Control::Check, "The component's own pictures are cut off at its edges."),
        "uientry flag 10" => d("Use global clicks", "Behaviour", Control::Check, "Also receives mouse clicks that land outside the component."),
        "uientry flag 11" => d("Render while dragged", "Behaviour", Control::Check, "Keeps drawing while the player drags it."),
        "uientry flag 12" => d("Tooltips are localised", "Tooltip", Control::Check, "The tooltip label is a localisation key and the text is looked up."),
        "tooltip text" => d("Tooltip text", "Tooltip", Control::Text, "Shown when the mouse rests on the component."),
        "tooltip id" => d("Tooltip label", "Tooltip", Control::Text, "Localisation key of the tooltip."),
        "docking?" => d("Dock point", "Docking", choice(DOCK_POINTS), "The point of the parent the component sticks to. The \"Outside\" points sit just outside the parent."),
        "docking x?" => d("Dock offset X", "Docking", Control::Int, "Distance from the dock point, to the right."),
        "docking y?" => d("Dock offset Y", "Docking", Control::Int, "Distance from the dock point, downwards."),
        "default state id" => d("Default state", "Behaviour", Control::Choice(vec![]), "The state the component starts in."),
        "mask image?" => d("Mask picture", "Behaviour", Control::Int, "A picture (by its id in the component's picture list) that masks what is drawn; 0 for none."),
        "priority?" => d("Draw priority", "Behaviour", Control::Int, "Higher draws on top of lower among siblings."),
        "end of uientry 1?" => d("Sound category", "Sound & transitions", Control::Suggest(SOUND_CATEGORIES), "The group of interface sounds the component plays on mouse events."),
        "end of uientry 2?" => d("Layout transition", "Sound & transitions", Control::Text, "Name of the layout transition the component takes part in."),
        "end of uientry flag 5B?" | "end of uientry flag 5A?" => d("Update when hidden", "Behaviour", Control::Check, "Keeps updating (scripts, animations) while the component is not visible."),
        "end of uientry flag 6B?" => d("Lock aspect ratio", "Position & size", Control::Check, "Resizing keeps the width to height ratio (both axes must be resizable)."),
        l if l.starts_with("has some extra data") => adv("Has text group", "Advanced", Control::Info, "Whether the component belongs to a group of components that share one font size."),
        l if l.starts_with("has some extra v97") => adv("Has focus group data", "Advanced", Control::Info, "Controller focus group data follows."),
        l if l.starts_with("has model") => adv("Has 3D model", "Advanced", Control::Info, "A 3D model is attached to the component."),
        "uientry id reference maybe?" | "extra int 1?" | "v100+ extra string?" => adv("Unknown", "Advanced", Control::Int, ""),
        _ => None,
    }
}

pub(crate) fn state_def(it: &Item, v: u32) -> Option<Def> {
    if it.label.is_empty() {
        let editor_pos = it.tag == "i" && matches!(it.after.as_str(), "text shader vars" | "text shader name") && it.since < 2 && it.nth == 0;
        if editor_pos {
            return adv(if it.since == 0 { "Editor position X" } else { "Editor position Y" }, "Advanced", Control::Int, "Only the layout editor uses this (where the state sits on its canvas).");
        }

        return None;
    }

    match it.label.as_str() {
        "title - NewState" => d("State name", "State", Control::Text, "Scripts, animations and mouse actions switch the component to a state by this name."),
        "x size" => d("Width", "State", Control::Int, "The component's width while in this state."),
        "y size" => d("Height", "State", Control::Int, "The component's height while in this state."),
        "state text" => d("Text", "Text", Control::Text, "The text the component shows."),
        "tooltip" => d("Tooltip text", "Text", Control::Text, "Tooltip while in this state."),
        "text xsize?" => d("Horizontal alignment", "Text", choice(H_ALIGN), "Where the text sits across the component."),
        "text ysize?" => d("Vertical alignment", "Text", choice(V_ALIGN), "Where the text sits down the component."),
        "text xalign?" => d("Focus type", "Behaviour", choice(FOCUS), "Whether the state can take the keyboard focus (text boxes do)."),
        "text yalign?" => d("Text wrapping", "Text", choice(WRAP), "How long lines are broken."),
        "text behavior?" if v < 116 => d("Shrink font to fit", "Text", Control::Check, "When the text is too tall the font shrinks; otherwise the text gets a scroll bar."),
        "state stuff 4?" if v < 116 => d("Text is localised", "Text", Control::Check, "The text is looked up from the localisation key below."),
        "localization id" if v < 116 => d("Text localisation key", "Text", Control::Text, "Key in the localisation files that gives this state's text."),
        "tooltip id" if v < 116 => d("Tooltip localisation key", "Text", Control::Text, "Key that gives this state's tooltip."),
        "state stuff before font?" => d("Blocked animations", "Behaviour", Control::Text, "Animations that must not play while the component is in this state."),
        "font" => d("Font", "Font", Control::Suggest(FONTS), "The game font the text is drawn with."),
        "font size? line height?" => d("Font size", "Font", Control::Int, "Size of the text in pixels."),
        "font leading?" => d("Line spacing", "Font", Control::Int, "Extra space between lines."),
        "font tracking?" => d("Letter spacing", "Font", Control::Int, "Extra space between letters."),
        "font category / twui" => d("Font category", "Font", Control::Suggest(FONT_CATEGORIES), "A named font style from the font category list; it can override the font above."),
        "left ?" | "x ?" => d("Text margin left", "Text", Control::Int, "Space between the left edge and the text."),
        "right ?" | "y ?" => d("Text margin right", "Text", Control::Int, "Space between the right edge and the text."),
        "top ?" => d("Text margin top", "Text", Control::Int, "Space between the top edge and the text."),
        "bottom ?" => d("Text margin bottom", "Text", Control::Int, "Space between the bottom edge and the text."),
        "shader name" => d("Shader", "Shader", Control::Suggest(SHADERS), "The shader pictures are drawn with."),
        "shader vars" => d(["Shader value 1", "Shader value 2", "Shader value 3", "Shader value 4"][it.nth.min(3)], "Shader", Control::Float, "Parameter the shader reads (glow speed, brightness, ...)."),
        "text shader name" => d("Text shader", "Shader", Control::Suggest(SHADERS), "The shader the text is drawn with."),
        "text shader vars" => d(["Text shader value 1", "Text shader value 2", "Text shader value 3", "Text shader value 4"][it.nth.min(3)], "Shader", Control::Float, "Parameter the text shader reads."),
        _ => None,
    }
}

pub(crate) fn picture_def(it: &Item, v: u32) -> Option<Def> {
    if it.label.is_empty() {
        return if it.tag == "u" && it.since == 0 && it.after.is_empty() {
            d("Picture", "Picture", Control::Choice(vec![]), "Which of the component's pictures (its picture list) this use draws.")
        } else {
            None
        };
    }

    match it.label.as_str() {
        "x offset" => d("Offset X", "Position & size", Control::Int, "Distance from the component's left edge."),
        "y offset" => d("Offset Y", "Position & size", Control::Int, "Distance from the component's top edge."),
        "x size" => d("Width", "Position & size", Control::Int, "Width the picture is drawn at."),
        "y size" => d("Height", "Position & size", Control::Int, "Height the picture is drawn at."),
        "tiled?" => d("Tile", "Appearance", Control::Check, "Repeat the picture instead of stretching it."),
        "flipped x?" => d("Flip horizontally", "Appearance", Control::Check, ""),
        "flipped y?" => d("Flip vertically", "Appearance", Control::Check, ""),
        "dock position (0-9)?" => d("Dock point", "Docking", choice(DOCK_POINTS), "The point of the component the picture sticks to."),
        "dock offset x?" => d("Dock offset X", "Docking", Control::Int, ""),
        "dock offset y?" => d("Dock offset Y", "Docking", Control::Int, ""),
        "stretch x?" => d("Stretch with width", "Appearance", Control::Check, "The picture gets wider when the component does."),
        "stretch y?" => d("Stretch with height", "Appearance", Control::Check, "The picture gets taller when the component does."),
        "rotation angle" => d("Rotation angle", "Rotation", Control::Float, "Degrees."),
        "rotation pivot x?" => d("Rotation pivot X", "Rotation", Control::Float, ""),
        "rotation pivot y?" => d("Rotation pivot Y", "Rotation", Control::Float, ""),
        "rotation axis x?" => adv("Rotation axis X", "Rotation", Control::Float, ""),
        "rotation axis y?" => adv("Rotation axis Y", "Rotation", Control::Float, ""),
        "rotation axis z?" => adv("Rotation axis Z", "Rotation", Control::Float, ""),
        "shader name" => d("Shader", "Appearance", Control::Suggest(SHADERS), "The shader this picture is drawn with."),
        "shader var?" => d(["Shader value 1", "Shader value 2", "Shader value 3", "Shader value 4"][it.nth.min(3)], "Appearance", Control::Float, ""),
        "margin top?" => d("Margin top", "Nine-patch", Control::Float, "Border kept unstretched when the picture is stretched; all four 0 means a plain stretch."),
        "margin right?" => d("Margin right", "Nine-patch", Control::Float, ""),
        "margin bottom?" => d("Margin bottom", "Nine-patch", Control::Float, ""),
        "margin left?" => d("Margin left", "Nine-patch", Control::Float, ""),
        "margin top-bottom?" if v < 92 => d("Margin top and bottom", "Nine-patch", Control::Float, ""),
        "margin left-right?" if v < 92 => d("Margin left and right", "Nine-patch", Control::Float, ""),
        _ => None,
    }
}

pub(crate) fn mouse_def(it: &Item) -> Option<Def> {
    if it.label.is_empty() {
        return match (it.after.as_str(), it.since, it.tag.as_str()) {
            ("type", 0, "u") => d("Go to state", "Mouse action", Control::Choice(vec![]), "The state the component changes to."),
            ("type", 2, "i") => None,
            ("type", 1, "i") => d("Transition time (ms)", "Mouse action", Control::Int, "How long the change takes; 0 is instant."),
            _ => None,
        };
    }

    match it.label.as_str() {
        "type" => d("When", "Mouse action", choice(MOUSE_WHEN), "The mouse event that triggers the change."),
        _ => None,
    }
}
