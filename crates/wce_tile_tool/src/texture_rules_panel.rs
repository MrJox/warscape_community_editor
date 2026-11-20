//! The texture rules' window (docs/plans/auto_detail.md, A; `wce_tile::texture_rules`, `tile_document::texture_rules`): opened from the
//! Tile panel. The tile's rules on or off, the preset they came from (Save, Save as, Delete; `wce_tile::texture_rules::Preset`), the list
//! of rules and the selected one's texture and conditions. An edit is one undo entry: the window works on a copy while a field is being
//! dragged or typed in, and gives it to the tile when no field is active. The tile's layer follows by itself. "Show its area" outlines
//! where the selected rule lays its texture (its weight at least a half, as though it were alone) in the viewport.

use std::collections::HashMap;
use std::sync::Arc;

use wce_editor::editor::Editor;
use wce_editor::{gui, icons, theme};
use wce_editor_core::document::Document;
use wce_scene::render_scene::{DocumentId, Overlay, OverlayVertex, overlay_colour};
use wce_tile::ground_coverage::Grid;
use wce_tile::texture_rules::{Breakup, Preset, Range, Rule, RuleSet, presets_root, scan_presets};
use wce_tile_editor::tile_document::TileDocument;

use crate::styled;
use crate::tool::{TileTool, document, document_id};

/// The window's state.
#[derive(Default)]
pub struct RulesUi {
    pub open: bool,
    pub selected: usize,

    /// The rules being edited, for a tile, and whether they differ from the tile's by an edit not yet given to it.
    draft: Option<(DocumentId, Option<RuleSet>)>,
    draft_edited: bool,

    presets: Option<Arc<Vec<Preset>>>,
    save_as: Option<String>,
    delete_ask: bool,

    /// Whether the viewport outlines the selected rule's area, and the outline: the tile and what it was made from.
    pub show_area: bool,
    area: Option<(DocumentId, u64, Vec<OverlayVertex>)>,

    /// Every number of a rule (advanced) or its plain choices.
    pub advanced: bool,
}

/// The outline's colour.
fn area_colour() -> u32 {
    overlay_colour(1.0, 0.55, 0.1, 1.0)
}

impl TileTool {
    /// Shows the tile's texture rules in the Inspector (Terrain layers > Blend, "Texture rules"; Paint mode's Edit).
    pub fn open_texture_rules(&mut self) {
        self.close_detail_panels();
        self.rules_ui.open = true;
    }

    fn rules_presets_folder(e: &mut Editor) -> std::path::PathBuf {
        presets_root(&Self::workspace_dir(e).join("raw_data"))
    }

    /// The presets (the built-in one first), read on first use and again after a change.
    pub(crate) fn rules_presets(&mut self, e: &mut Editor) -> Arc<Vec<Preset>> {
        if let Some(p) = &self.rules_ui.presets {
            return p.clone();
        }

        let p = Arc::new(scan_presets(&Self::rules_presets_folder(e)));
        self.rules_ui.presets = Some(p.clone());
        p
    }

    /// Gives the tile a preset's rules (by name; `Default`: the built-in one).
    ///
    /// # Errors
    ///
    /// No tile, or no such preset.
    pub(crate) fn load_rules_preset(&mut self, e: &mut Editor, name: &str) -> Result<String, String> {
        let p = self.rules_presets(e).iter().find(|p| p.name == name).cloned().ok_or_else(|| format!("no texture rule preset '{name}'"))?;
        let d = document(e).ok_or("no tile is open")?;
        d.set_texture_rules(Some(RuleSet { preset: p.name.clone(), rules: p.rules.clone() }));
        self.rules_ui.draft_edited = false;
        Ok(format!("texture rules: the preset '{}' ({} rules)", p.name, p.rules.len()))
    }

    /// The tile's texture rules in the Inspector: on or off, the preset, the rules and the selected one.
    pub(crate) fn inspect_texture_rules(&mut self, e: &mut Editor) {
        if !self.rules_ui.open {
            return;
        }

        let Some(id) = document_id(e) else {
            self.rules_ui.open = false;
            return;
        };

        // The copy: the tile's own unless an edit waits; an edit is given to the tile once no field is being edited.
        let current = document(e).and_then(|d| d.texture_rules().cloned());
        let ui = &mut self.rules_ui;
        if ui.draft.as_ref().is_none_or(|(d, _)| *d != id) {
            ui.draft = Some((id, current.clone()));
            ui.draft_edited = false;
        } else if ui.draft_edited && !gui::is_any_item_active() {
            let rules = ui.draft.as_ref().and_then(|d| d.1.clone());
            ui.draft_edited = false;
            if let Some(d) = document(e) {
                d.set_texture_rules(rules);
            }
        } else if !ui.draft_edited {
            ui.draft = Some((id, current.clone()));
        }

        if styled::inspector_header(
            &format!("{}  Texture rules (experimental)", icons::LAYERS),
            "Textures laid by the terrain: each rule puts a texture where its conditions hold (height, slope, hollows, distance to water, \
             roads and buildings), a later rule over an earlier one. They go into a terrain layer of their own under yours: what you \
             paint stays over them. The layer is made again by itself when the terrain or the rules change.",
        ) {
            self.rules_ui.open = false;
            return;
        }

        // On or off, and how the rules show, on one line.
        let on = current.is_some();
        let half = (gui::content_region_avail().x - gui::item_spacing().x) * 0.5;
        let toggled = styled::toggle_chip("ruleson", "Rules on this tile", on, half, "Texture rules on this tile, or none");
        gui::same_line();
        if let Some(i) = styled::segmented("mode", &["Simple", "Advanced"], Some(usize::from(self.rules_ui.advanced)), half) {
            self.rules_ui.advanced = i == 1;
        }

        if toggled {
            let rules = (!on).then(|| RuleSet { preset: Preset::builtin().name, rules: Preset::builtin().rules });
            if let Some(d) = document(e) {
                d.set_texture_rules(rules);
            }

            self.rules_ui.draft = None;
            return;
        }

        if current.is_some() {
            self.rules_preset_rows(e);
            self.rules_groups_note(e);
            self.rules_list_and_editor(e);
            if let Some(summary) = document(e).and_then(|d| d.texture_rules_summary()) {
                gui::spacing();
                gui::text_disabled(&summary);
            }
        }

        self.update_rules_area(e);
    }

    /// The preset combo and its buttons.
    fn rules_preset_rows(&mut self, e: &mut Editor) {
        let presets = self.rules_presets(e);
        let Some((_, Some(set))) = self.rules_ui.draft.clone() else { return };
        let current = presets.iter().find(|p| p.name == set.preset).cloned();
        let label = |p: &Preset| {
            if p.is_builtin() {
                format!("{} (built in)", p.name)
            } else if p.folder.is_empty() {
                p.name.clone()
            } else {
                format!("{}   {}", p.name, p.folder)
            }
        };
        let shown = match &current {
            Some(p) if p.rules == set.rules => label(p),
            Some(p) => format!("{} (changed)", label(p)),
            None if set.preset.is_empty() => "none".to_owned(),
            None => format!("{} (not found)", set.preset),
        };

        let mut pick = None;
        let buttons =
            [icons::SAVE, icons::FILE_PLUS, icons::TRASH_2].iter().map(|i| gui::button_width(i)).sum::<f32>() + 3.0 * gui::item_spacing().x;
        gui::set_next_item_width((gui::content_region_avail().x - buttons).max(120.0));
        if gui::begin_combo("##preset", &shown) {
            for p in presets.iter() {
                if gui::selectable(&label(p), p.name == set.preset, 0) {
                    pick = Some(p.name.clone());
                }
            }

            gui::end_combo();
        }

        let file = current.as_ref().is_some_and(|p| !p.is_builtin());
        let mut result: Option<Result<String, String>> = None;
        // The preset's buttons, after its combo.
        if self.rules_ui.save_as.is_none() && !self.rules_ui.delete_ask {
            gui::same_line();
            gui::begin_disabled(!file);
            if gui::button(&format!("{}##savepreset", icons::SAVE)) {
                result = Some(self.save_rules_preset(e, &set, None));
            }

            gui::end_disabled();
            if gui::is_item_hovered_allow_disabled() {
                gui::set_tooltip("Save the rules into their preset");
            }

            gui::same_line();
            if gui::button(&format!("{}##saveas", icons::FILE_PLUS)) {
                self.rules_ui.save_as = Some(String::new());
            }

            if gui::is_item_hovered() {
                gui::set_tooltip("Save as a new preset...");
            }

            gui::same_line();
            gui::begin_disabled(!file);
            if gui::button(&format!("{}##deletepreset", icons::TRASH_2)) {
                self.rules_ui.delete_ask = true;
            }

            gui::end_disabled();
            if gui::is_item_hovered_allow_disabled() {
                gui::set_tooltip("Delete the preset's file");
            }
        }

        if let Some(name) = pick {
            self.status = self.load_rules_preset(e, &name).unwrap_or_else(|err| err);
        }

        if let Some(mut name) = self.rules_ui.save_as.take() {
            gui::set_next_item_width(gui::content_region_avail().x - gui::button_width("Save") - gui::button_width("Cancel") - 24.0);
            let enter = gui::input_text("##rulespreset", Some("the new preset's name"), &mut name, 128, gui::INPUT_ENTER_RETURNS_TRUE);
            gui::same_line();
            let ok = gui::small_button("Save") || enter;
            gui::same_line();
            if ok && !name.trim().is_empty() {
                result = Some(self.save_rules_preset(e, &set, Some(name.trim())));
            } else if !gui::small_button("Cancel") {
                self.rules_ui.save_as = Some(name);
            }
        } else if self.rules_ui.delete_ask {
            gui::text_colored(theme::warning(), &format!("Delete the preset '{}'?", set.preset));
            gui::same_line();
            if gui::small_button("Delete") {
                result = Some(self.delete_rules_preset(e, &set.preset));
                self.rules_ui.delete_ask = false;
            }

            gui::same_line();
            if gui::small_button("Cancel") {
                self.rules_ui.delete_ask = false;
            }
        }

        if let Some(r) = result {
            self.status = r.unwrap_or_else(|err| err);
        }
    }

    /// Writes the tile's rules into a preset's file: its own (`None`) or a new one, which the tile then names.
    fn save_rules_preset(&mut self, e: &mut Editor, set: &RuleSet, new_name: Option<&str>) -> Result<String, String> {
        let root = Self::rules_presets_folder(e);
        let presets = self.rules_presets(e);
        let preset = match new_name {
            Some(name) => {
                if name.contains(['/', '\\']) || name == wce_tile::texture_rules::BUILTIN {
                    return Err(format!("'{name}' can't be a preset's name"));
                }

                Preset {
                    name: name.to_owned(),
                    folder: String::new(),
                    path: Some(root.join(format!("{name}.wce"))),
                    rules: set.rules.clone(),
                }
            }

            None => {
                let p = presets.iter().find(|p| p.name == set.preset && !p.is_builtin()).ok_or("the built-in preset has no file")?;
                Preset { rules: set.rules.clone(), ..p.clone() }
            }
        };

        preset.save()?;
        self.rules_ui.presets = None;
        if preset.name != set.preset
            && let Some(d) = document(e)
        {
            d.set_texture_rules(Some(RuleSet { preset: preset.name.clone(), rules: set.rules.clone() }));
        }

        Ok(format!("texture rule preset '{}' saved", preset.name))
    }

    fn delete_rules_preset(&mut self, e: &mut Editor, name: &str) -> Result<String, String> {
        let presets = self.rules_presets(e);
        let p = presets.iter().find(|p| p.name == name && !p.is_builtin()).ok_or("the built-in preset can't be deleted")?;
        let path = p.path.clone().ok_or("no file")?;
        std::fs::remove_file(&path).map_err(|err| format!("{}: {err}", path.display()))?;
        self.rules_ui.presets = None;
        Ok(format!("texture rule preset '{name}' deleted"))
    }

    /// The tile's texture groups, and the rules whose group it does not have.
    fn rules_groups_note(&mut self, e: &mut Editor) {
        let Some(d) = document(e) else { return };
        let groups = d.texture_group_slots();
        gui::push_text_wrap_pos(0.0);
        if groups.is_empty() {
            gui::text_colored(theme::warning(), "The tile has no database entry: no texture groups, the rules lay nothing.");
        } else {
            gui::text_disabled(&format!("This tile's texture groups: {}", groups.join(", ")));
        }

        let missing = d.texture_rules_missing_groups();
        if !missing.is_empty() {
            gui::text_colored(theme::warning(), &format!("Not in this tile, laying nothing: {}", missing.join(", ")));
        }

        gui::pop_text_wrap_pos();
    }

    /// The list of rules (each with its texture's swatch) and the selected one's fields.
    fn rules_list_and_editor(&mut self, e: &mut Editor) {
        let swatches = self.ground_swatches(e);
        let (groups, heights) = document(e).map(|d| (d.texture_group_slots(), tile_heights(d))).unwrap_or((vec![], (0.0, 10.0)));
        let ui = &mut self.rules_ui;
        let advanced = ui.advanced;
        let Some((_, Some(set))) = ui.draft.as_mut() else { return };
        let mut edited = false;

        edited |= rule_list("rules", "Rules (later over earlier)", &mut set.rules, &mut ui.selected, &groups, &swatches, "New rule");
        if let Some(r) = set.rules.get_mut(ui.selected) {
            gui::separator_text("The selected rule");
            let row = styled::row("Viewport", "");
            if styled::toggle_chip(
                "showarea",
                "Outline where it lays its texture",
                ui.show_area,
                row.width,
                "Outlines in the viewport (orange) where the selected rule lays its texture (at least half of it), as though it were the \
                 only rule",
            ) {
                ui.show_area = !ui.show_area;
            }

            let view = RuleView { groups: &groups, swatches: &swatches, heights, advanced };
            edited |= rule_fields("selected", r, &view);
        }

        if edited {
            ui.draft_edited = true;
        }
    }

    /// The selected rule's outline, into the tool's overlay (only over its tile).
    pub(crate) fn rules_overlay(&self, d: &TileDocument, o: &mut Overlay) {
        let ui = &self.rules_ui;
        if let Some((id, _, lines)) = ui.area.as_ref().filter(|_| ui.open && ui.show_area)
            && *id == d.id()
        {
            o.lines.extend_from_slice(lines);
        }
    }

    /// Makes the selected rule's outline again when the rule or the tile changed.
    fn update_rules_area(&mut self, e: &mut Editor) {
        let ui = &self.rules_ui;
        if !ui.open || !ui.show_area {
            return;
        }

        let Some((id, Some(set))) = ui.draft.clone() else { return };
        let Some(d) = document(e).filter(|d| d.id() == id) else { return };

        let Some(rule) = set.rules.get(ui.selected).cloned() else { return };
        let key = {
            let mut h = 0xcbf2_9ce4_8422_2325u64;
            let text = wce_tile::texture_rules::text(&RuleSet { preset: String::new(), rules: vec![rule] });
            for b in text.iter().copied().chain(d.history().version().to_le_bytes()).chain((ui.selected as u64).to_le_bytes()) {
                h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
            }

            h
        };

        if self.rules_ui.area.as_ref().is_none_or(|(i, k, _)| *i != id || *k != key) {
            let rules = set.rules.clone();
            let selected = ui.selected;
            let (ex, ez) = (d.project().extent_x() as f32, d.project().extent_z() as f32);
            let cell = (ex.max(ez) / 256.0).max(1.0);
            let lines = match d.texture_rule_area(&rules, selected, cell) {
                Ok(area) => {
                    let t = d.tile();
                    let grid = Grid::sample(
                        area.x0,
                        area.z0,
                        area.x0 + (area.nx - 1) as f32 * area.cell,
                        area.z0 + (area.nz - 1) as f32 * area.cell,
                        area.cell,
                        |x, z| t.terrain_height(x, z),
                    );
                    let cells = area.weights.iter().enumerate().filter(|(_, w)| **w >= 0.5).map(|(i, _)| i).collect::<Vec<_>>();
                    let mut out = Overlay::default();
                    crate::ground_coverage_panel::outline(&grid, &cells, area_colour(), &mut out);
                    out.lines
                }

                Err(err) => {
                    self.status = format!("texture rules: {err}");
                    vec![]
                }
            };
            self.rules_ui.area = Some((id, key, lines));
        }
    }
}

/// A list of rules under a title whose buttons add, duplicate, remove and move the selected one: each rule on or off, its texture's
/// swatch, its name and what it lays (or that the tile has not its texture); a click selects one. New rules are called `new_name`.
/// Whether the rules changed.
#[allow(clippy::too_many_arguments)]
pub(crate) fn rule_list(
    id: &str,
    title: &str,
    rules: &mut Vec<Rule>,
    selected: &mut usize,
    groups: &[String],
    swatches: &HashMap<(String, u8), u64>,
    new_name: &str,
) -> bool {
    let mut edited = false;
    gui::push_id(id);
    let n = rules.len();
    *selected = (*selected).min(n.saturating_sub(1));
    let at = *selected;
    let clicked = styled::section(
        title,
        &[
            (icons::PLUS, "add", "Add a rule (laid over the others)", true),
            (icons::COPY, "dup", "Duplicate the selected rule", n > 0),
            (icons::TRASH_2, "remove", "Remove the selected rule", n > 0),
            (icons::ARROW_BIG_UP, "up", "Earlier: under the rules after it", at > 0),
            (icons::ARROW_BIG_DOWN, "down", "Later: over the rules before it", at + 1 < n),
        ],
    );
    match clicked {
        Some(0) => {
            rules.push(Rule {
                name: new_name.to_owned(),
                group: groups.first().cloned().unwrap_or_default(),
                seed: n as u32 + 1,
                ..Rule::default()
            });
            *selected = rules.len() - 1;
            edited = true;
        }
        Some(1) if n > 0 => {
            let mut r = rules[at].clone();
            r.name = format!("{} (copy)", r.name);
            rules.insert(at + 1, r);
            *selected = at + 1;
            edited = true;
        }
        Some(2) if n > 0 => {
            rules.remove(at);
            *selected = at.saturating_sub(1);
            edited = true;
        }
        Some(3) if at > 0 => {
            rules.swap(at, at - 1);
            *selected = at - 1;
            edited = true;
        }
        Some(4) if at + 1 < n => {
            rules.swap(at, at + 1);
            *selected = at + 1;
            edited = true;
        }
        _ => {}
    }

    if rules.is_empty() {
        gui::text_disabled("No rules: + adds one.");
        gui::pop_id();
        return edited;
    }

    let line = gui::frame_height();
    let list_h = (line + gui::item_spacing().y) * (rules.len().clamp(2, 7) as f32) + 8.0;
    gui::begin_child("##list", gui::v2(0.0, list_h), gui::CHILD_BORDERS, 0);
    for (i, r) in rules.iter_mut().enumerate() {
        gui::push_id_int(i as i32);
        edited |= gui::checkbox("##on", &mut r.enabled);
        if gui::is_item_hovered() {
            gui::set_tooltip("On or off");
        }

        gui::same_line();
        let slot = group_slot(groups, &r.group);
        let thumb = slot.and_then(|g| swatches.get(&(groups[g].clone(), r.variation.round().clamp(0.0, 3.0) as u8)).copied());
        match thumb {
            Some(t) => gui::image(t, gui::v2(line, line)),
            None => gui::dummy(gui::v2(line, line)),
        }

        gui::same_line();
        let name = if r.name.is_empty() { format!("(rule {})", i + 1) } else { r.name.clone() };
        gui::align_text_to_frame_padding();
        if gui::selectable(&name, *selected == i, 0) {
            *selected = i;
        }

        // The texture it lays, at the row's right.
        let (lo, hi) = gui::item_rect();
        let what = slot.map_or_else(|| "not in this tile".to_owned(), |g| groups[g].clone());
        let colour = if slot.is_some() { theme::dim() } else { theme::warning() };
        let w = gui::calc_text_width(&what);
        let y = lo.y + (hi.y - lo.y - gui::text_line_height()) * 0.5;
        gui::window_draw_list().text(gui::v2(hi.x - w - 6.0, y), theme::u32(colour), &what);
        gui::pop_id();
    }

    gui::end_child();
    gui::pop_id();
    edited
}

/// What a rule's fields show besides the rule: the tile's texture groups and their swatches (by group and variation), the tile's
/// heights (the height bar's ends, metres) and whether every number shows (advanced) or the plain choices.
pub(crate) struct RuleView<'a> {
    pub groups: &'a [String],
    pub swatches: &'a HashMap<(String, u8), u64>,
    pub heights: (f32, f32),
    pub advanced: bool,
}

/// The tile's lowest and highest ground, metres (the database entry's heights), at least 10 m apart.
pub(crate) fn tile_heights(d: &TileDocument) -> (f32, f32) {
    let info = &d.project().info;
    let lo = info.min_height.floor() as f32;
    let hi = (info.min_height + info.scale).ceil() as f32;
    (lo, hi.max(lo + 10.0))
}

/// The slot of the first of a rule's group names the tile has (as the rules lay it), if any.
pub(crate) fn group_slot(groups: &[String], names: &str) -> Option<usize> {
    names.split('|').map(str::trim).filter(|n| !n.is_empty()).find_map(|n| groups.iter().take(4).position(|g| g.eq_ignore_ascii_case(n)))
}

/// A condition of a rule: how it shows and its plain choices.
struct Condition {
    key: &'static str,
    label: &'static str,
    tip: &'static str,
    unit: &'static str,
    step: f32,
    domain: (f32, f32),

    /// The plain choices, and which one a condition switched on starts as.
    presets: Vec<(&'static str, Range)>,
    first: usize,
}

const fn below(hi: f32, fade: f32) -> Range {
    Range { lo: None, hi: Some(hi), fade }
}

const fn above(lo: f32, fade: f32) -> Range {
    Range { lo: Some(lo), hi: None, fade }
}

/// The conditions in the order they show, the height's choices from the tile's heights.
fn conditions(heights: (f32, f32)) -> Vec<Condition> {
    let (lo, hi) = heights;
    let span = hi - lo;
    let at = |t: f32| ((lo + span * t) * 2.0).round() / 2.0;
    let soft = ((span * 0.08) * 2.0).round().max(1.0) / 2.0;
    vec![
        Condition {
            key: "height",
            label: "Height",
            tip: "The ground's height in metres (the sea at 0)",
            unit: "m",
            step: 0.5,
            domain: heights,
            presets: vec![
                ("Low", below(at(0.25), soft)),
                ("Middle", Range { lo: Some(at(0.3)), hi: Some(at(0.7)), fade: soft }),
                ("High", above(at(0.75), soft)),
            ],
            first: 2,
        },
        Condition {
            key: "slope",
            label: "Slope",
            tip: "The ground's slope in degrees",
            unit: "deg",
            step: 1.0,
            domain: (0.0, 90.0),
            presets: vec![
                ("Flat", below(10.0, 6.0)),
                ("Gentle", below(25.0, 8.0)),
                ("Steep", above(35.0, 12.0)),
                ("Cliffs", above(50.0, 10.0)),
            ],
            first: 2,
        },
        Condition {
            key: "hollow",
            label: "Hollows",
            tip: "How far the ground lies under the mean ground around it, metres: above 0 a hollow, below 0 a ridge",
            unit: "m",
            step: 0.05,
            domain: (-3.0, 3.0),
            presets: vec![("Ridges", below(-0.4, 0.4)), ("Hollows", above(0.4, 0.4)), ("Deep hollows", above(1.5, 0.8))],
            first: 1,
        },
        Condition {
            key: "water",
            label: "Water",
            tip: "Metres to the nearest water: rivers, lakes, the sea",
            unit: "m",
            step: 0.5,
            domain: (0.0, 30.0),
            presets: vec![
                ("At the edge", below(2.0, 2.0)),
                ("Near", below(4.0, 4.0)),
                ("Around", below(12.0, 8.0)),
                ("Away", above(12.0, 8.0)),
            ],
            first: 1,
        },
        Condition {
            key: "road",
            label: "Roads",
            tip: "Metres to the nearest procedural road",
            unit: "m",
            step: 0.5,
            domain: (0.0, 20.0),
            presets: vec![
                ("On the edge", below(1.0, 1.5)),
                ("Near", below(2.0, 3.0)),
                ("Around", below(6.0, 6.0)),
                ("Away", above(6.0, 6.0)),
            ],
            first: 1,
        },
        Condition {
            key: "building",
            label: "Buildings",
            tip: "Metres to the nearest real building (walls, towers, gates, houses)",
            unit: "m",
            step: 0.5,
            domain: (0.0, 20.0),
            presets: vec![("Touching", below(1.0, 1.5)), ("Near", below(3.0, 3.0)), ("Around", below(8.0, 6.0)), ("Away", above(8.0, 6.0))],
            first: 1,
        },
    ]
}

fn range_of<'a>(r: &'a mut Rule, key: &str) -> &'a mut Option<Range> {
    match key {
        "height" => &mut r.height,
        "slope" => &mut r.slope,
        "hollow" => &mut r.hollow,
        "water" => &mut r.water,
        "road" => &mut r.road,
        _ => &mut r.building,
    }
}

/// The strength's plain choices.
const STRENGTHS: [(&str, f32); 4] = [("Faint", 0.3), ("Some", 0.6), ("Strong", 0.85), ("Full", 1.0)];

/// The edges' noise's plain choices.
const BREAKUPS: [(&str, Option<Breakup>); 4] = [
    ("Clean", None),
    ("Soft", Some(Breakup { scale: 25.0, amount: 0.4 })),
    ("Patchy", Some(Breakup { scale: 15.0, amount: 0.6 })),
    ("Very patchy", Some(Breakup { scale: 8.0, amount: 0.9 })),
];

/// A rule's fields in rows (its name, its texture as swatches, its strength, where it lays it, its edges and its ground type); whether
/// any changed. `id` keeps two rules' fields apart in one window.
pub(crate) fn rule_fields(id: &str, r: &mut Rule, view: &RuleView) -> bool {
    let mut edited = false;
    gui::push_id(id);

    let row = styled::row("Name", "");
    gui::set_next_item_width(row.width);
    edited |= gui::input_text("##name", None, &mut r.name, 128, 0);

    edited |= texture_rows(r, view);

    let row = styled::row("Strength", "The most the rule's texture covers, where all its conditions hold");
    if view.advanced {
        gui::set_next_item_width(row.width);
        edited |= gui::slider_float("##strength", &mut r.strength, 0.0, 1.0, "%.2f");
    } else {
        let names = STRENGTHS.map(|s| s.0);
        let current = STRENGTHS.iter().position(|s| (s.1 - r.strength).abs() < 1e-4);
        if let Some(i) = styled::segmented("strength", &names, current, row.width) {
            r.strength = STRENGTHS[i].1;
            edited = true;
        }
    }

    gui::separator_text("Where");
    for c in conditions(view.heights) {
        edited |= condition_rows(r, &c, view.advanced);
    }

    gui::separator_text("Edges and ground");
    edited |= breakup_rows(r, view.advanced);
    edited |= ground_rows(r, view.advanced);
    gui::pop_id();
    edited
}

/// The texture: the tile's groups as swatches, then the chosen group's four textures; advanced: the names it may go by and a mix of two
/// textures.
fn texture_rows(r: &mut Rule, view: &RuleView) -> bool {
    let mut edited = false;
    let groups = view.groups;
    let slot = group_slot(groups, &r.group);
    let row = styled::row("Texture", "The tile's texture groups: the rule lays the one picked");
    if groups.is_empty() {
        gui::text_colored(theme::warning(), "the tile has no texture groups");
    } else {
        let size = styled::swatch_size(groups.len(), row.width, 64.0);
        for (i, g) in groups.iter().enumerate() {
            if i > 0 {
                styled::swatch_gap();
            }

            let image = view.swatches.get(&(g.clone(), 0)).copied();
            if styled::swatch(&format!("group{i}"), image, size, slot == Some(i), g, g) && slot != Some(i) {
                r.group.clone_from(g);
                edited = true;
            }
        }

        if slot.is_none() {
            row.next_line();
            gui::text_colored(theme::warning(), &format!("none of {} is in this tile: it lays nothing", r.group));
        }
    }

    if let Some(g) = slot.map(|i| groups[i].clone()) {
        let row = styled::row("Variation", "Which of the group's four textures");
        let size = styled::swatch_size(4, row.width, 64.0);
        let whole = (r.variation.fract() == 0.0).then_some(r.variation as usize);
        for v in 0..4u8 {
            if v > 0 {
                styled::swatch_gap();
            }

            let image = view.swatches.get(&(g.clone(), v)).copied();
            let tip = format!("{g}, texture {}", v + 1);
            if styled::swatch(&format!("var{v}"), image, size, whole == Some(usize::from(v)), &format!("{}", v + 1), &tip) {
                r.variation = f32::from(v);
                edited = true;
            }
        }
    }

    if view.advanced {
        let row = styled::row(
            "Names",
            "The texture group by name; several separated by |, the first this tile has (kit tiles name the same ground differently)",
        );
        gui::set_next_item_width(row.width);
        edited |= gui::input_text("##group", Some("rock|rock_b|mountain"), &mut r.group, 128, 0);

        let row = styled::row("Mix", "Between two textures, a mix of both (1.3: 70 % of the second, 30 % of the third)");
        gui::set_next_item_width(row.width);
        let mut shown = r.variation + 1.0;
        if gui::slider_float("##variation", &mut shown, 1.0, 4.0, "texture %.2f") {
            r.variation = (shown - 1.0).clamp(0.0, 3.0);
            edited = true;
        }
    }

    edited
}

/// A condition: its toggle and its bar; under them while it is on, its plain choices, or (advanced) its numbers.
fn condition_rows(r: &mut Rule, c: &Condition, advanced: bool) -> bool {
    let mut edited = false;
    let bar = styled::Bar { domain: c.domain, step: c.step, unit: c.unit, open_ends: true, fade: true };
    let first = c.presets[c.first].1;
    let range = range_of(r, c.key);
    let mut on = range.is_some();
    let (row, clicked) = styled::toggle_row(c.key, c.label, &mut on, c.tip);
    if clicked {
        *range = on.then_some(first);
        edited = true;
    }

    let mut shown = range.unwrap_or(first);
    let (changed, ask) = styled::range_bar(&format!("{}bar", c.key), &mut shown, &bar, row.width, range.is_some());
    if ask || (changed && range.is_some()) {
        *range = Some(shown);
        edited = true;
    }

    let Some(rg) = range.as_mut() else { return edited };
    row.next_line();
    if advanced {
        edited |= range_fields(c.key, rg, &bar, row.width);
    } else {
        let names = c.presets.iter().map(|p| p.0).collect::<Vec<_>>();
        let current = c.presets.iter().position(|p| p.1 == *rg);
        if let Some(i) = styled::segmented(&format!("{}choice", c.key), &names, current, row.width) {
            *rg = c.presets[i].1;
            edited = true;
        }
    }

    if c.key == "hollow" && advanced {
        row.next_line();
        gui::set_next_item_width(row.width);
        edited |= gui::slider_float("##around", &mut r.hollow_radius, 1.0, 100.0, "the mean ground of %.0f m around");
        if gui::is_item_hovered() {
            gui::set_tooltip("How far around each place the mean ground is taken (Ctrl + click to type)");
        }
    }

    edited
}

/// A condition's numbers on one line: its low end, its high end (each a number, or "no limit"), how softly they blend.
fn range_fields(key: &str, r: &mut Range, bar: &styled::Bar, width: f32) -> bool {
    let mut edited = false;
    let spacing = gui::item_spacing().x;
    let field = (width - 2.0 * spacing) / 3.0;
    let x_w = gui::button_width(icons::X);
    let decimals = if bar.step >= 1.0 {
        0
    } else if bar.step >= 0.1 {
        1
    } else {
        2
    };
    let first = [bar.domain.0 + (bar.domain.1 - bar.domain.0) * 0.25, bar.domain.0 + (bar.domain.1 - bar.domain.0) * 0.75];
    for (i, (name, end)) in [("from", &mut r.lo), ("to", &mut r.hi)].into_iter().enumerate() {
        if i > 0 {
            gui::same_line();
        }

        match end {
            Some(v) => {
                gui::set_next_item_width(field - x_w - 2.0);
                let format = format!("{name} %.{decimals}f {}", bar.unit);
                edited |= gui::drag_float(&format!("##{key}{name}"), v, bar.step * 0.5, -10_000.0, 10_000.0, &format);
                gui::same_line_spacing(2.0);
                if styled::icon_button(icons::X, &format!("{key}{name}open"), "No limit at this end") {
                    *end = None;
                    edited = true;
                }
            }

            None => {
                if gui::button_sized(&format!("{name}: no limit##{key}{name}"), gui::v2(field, 0.0)) {
                    *end = Some(first[i]);
                    edited = true;
                }

                if gui::is_item_hovered() {
                    gui::set_tooltip("Click to give this end a value");
                }
            }
        }
    }

    gui::same_line();
    gui::set_next_item_width(field);
    edited |=
        gui::drag_float(&format!("##{key}fade"), &mut r.fade, bar.step * 0.5, 0.0, 1000.0, &format!("soft %.{decimals}f {}", bar.unit));
    if gui::is_item_hovered() {
        gui::set_tooltip("How far each end blends, centred on it");
    }

    if let (Some(lo), Some(hi)) = (r.lo, r.hi)
        && lo > hi
    {
        r.hi = Some(lo);
    }

    edited
}

/// The edges' noise: plain choices and a new pattern; advanced: its size, amount and seed.
fn breakup_rows(r: &mut Rule, advanced: bool) -> bool {
    let mut edited = false;
    let row = styled::row("Edges", "Moves the rule's edges by noise, so that they are not contour lines; the inside of each range stays");
    let reroll_w = gui::button_width(icons::REFRESH_CW) + gui::item_spacing().x;
    if advanced {
        let spacing = gui::item_spacing().x;
        let field = (row.width - reroll_w - 2.0 * spacing) / 3.0;
        let mut b = r.breakup.unwrap_or(Breakup { scale: 15.0, amount: 0.0 });
        gui::set_next_item_width(field);
        let mut changed = gui::drag_float("##bsize", &mut b.scale, 0.1, 1.0, 500.0, "size %.1f m");
        gui::same_line();
        gui::set_next_item_width(field);
        changed |= gui::drag_float("##bamount", &mut b.amount, 0.01, 0.0, 2.0, "amount %.2f");
        gui::same_line();
        let mut seed = r.seed.min(i32::MAX as u32) as i32;
        gui::set_next_item_width(field);
        if gui::input_int("##seed", &mut seed) {
            r.seed = seed.max(0) as u32;
            edited = true;
        }

        if changed {
            r.breakup = (b.amount > 0.0).then_some(b);
            edited = true;
        }
    } else {
        let names = BREAKUPS.map(|b| b.0);
        let current = BREAKUPS.iter().position(|b| b.1 == r.breakup);
        if let Some(i) = styled::segmented("breakup", &names, current, row.width - reroll_w) {
            r.breakup = BREAKUPS[i].1;
            edited = true;
        }
    }

    gui::same_line();
    if styled::icon_button(icons::REFRESH_CW, "reroll", "Another pattern of the noise") {
        r.seed = r.seed.wrapping_mul(2_654_435_761).wrapping_add(1) % 1_000_000;
        edited = true;
    }

    edited
}

/// The ground type the rule lays too (none by default: it is gameplay); advanced: from what weight.
fn ground_rows(r: &mut Rule, advanced: bool) -> bool {
    let mut edited = false;
    let row = styled::row(
        "Ground type",
        "Also paints a ground type where the rule's weight is high enough: it changes sounds and how units move, so only when the \
         texture means it (rock, mud, sand)",
    );
    let names = wce_tile::ground_types::GROUND_TYPES;
    let shown = r.ground.and_then(|g| names.get(usize::from(g.kind))).map_or("none (gameplay stays as it is)", |(n, _)| n);
    gui::set_next_item_width(row.width);
    if gui::begin_combo("##groundtype", shown) {
        if gui::selectable("none", r.ground.is_none(), 0) {
            r.ground = None;
            edited = true;
        }

        for (i, (n, _)) in names.iter().enumerate() {
            if gui::selectable(n, r.ground.is_some_and(|g| usize::from(g.kind) == i), 0) {
                r.ground = Some(wce_tile::texture_rules::GroundRule { kind: i as u8, threshold: r.ground.map_or(0.5, |g| g.threshold) });
                edited = true;
            }
        }

        gui::end_combo();
    }

    if advanced && let Some(g) = r.ground.as_mut() {
        let row = styled::row(
            "From weight",
            "Where the rule's weight is at least this; the latest rule that lays a ground type at a place wins there",
        );
        gui::set_next_item_width(row.width);
        edited |= gui::slider_float("##threshold", &mut g.threshold, 0.01, 1.0, "%.2f");
    }

    edited
}
