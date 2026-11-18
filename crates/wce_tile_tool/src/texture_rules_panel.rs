//! The texture rules' window (docs/plans/auto_detail.md, A; `wce_tile::texture_rules`, `tile_document::texture_rules`): opened from the
//! Tile panel. The tile's rules on or off, the preset they came from (Save, Save as, Delete; `wce_tile::texture_rules::Preset`), the list
//! of rules and the selected one's texture and conditions. An edit is one undo entry: the window works on a copy while a field is being
//! dragged or typed in, and gives it to the tile when no field is active. The tile's layer follows by itself. "Show its area" outlines
//! where the selected rule lays its texture (its weight at least a half, as though it were alone) in the viewport.

use std::sync::Arc;

use wce_editor::editor::Editor;
use wce_editor::{gui, icons, theme};
use wce_editor_core::document::Document;
use wce_scene::render_scene::{DocumentId, Overlay, OverlayVertex, overlay_colour};
use wce_tile::ground_coverage::Grid;
use wce_tile::texture_rules::{Breakup, Preset, Range, Rule, RuleSet, presets_root, scan_presets};
use wce_tile_editor::tile_document::TileDocument;

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
}

/// The outline's colour.
fn area_colour() -> u32 {
    overlay_colour(1.0, 0.55, 0.1, 1.0)
}

impl TileTool {
    /// Opens the window (Tile panel, "Texture rules...").
    pub fn open_texture_rules(&mut self) {
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

    pub(crate) fn draw_texture_rules_window(&mut self, e: &mut Editor) {
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

        gui::set_next_window_size(gui::v2(460.0, 680.0), gui::COND_FIRST_USE_EVER);
        let mut open = true;
        if !gui::begin(&format!("{}  Texture rules###texturerules", icons::LAYERS), Some(&mut open), 0) {
            gui::end();
            self.rules_ui.open = open;
            return;
        }

        gui::text_wrapped(
            "Textures laid by the terrain: each rule puts a texture where its conditions hold (height, slope, hollows, distance to water, \
             roads and buildings), a later rule over an earlier one. They go into a terrain layer of their own under yours: what you \
             paint stays over them. The layer is made again by itself when the terrain or the rules change.",
        );
        gui::spacing();

        let mut on = current.is_some();
        if gui::checkbox("Texture rules on this tile", &mut on) {
            let rules = on.then(|| RuleSet { preset: Preset::builtin().name, rules: Preset::builtin().rules });
            if let Some(d) = document(e) {
                d.set_texture_rules(rules);
            }

            self.rules_ui.draft = None;
            gui::end();
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

        gui::end();
        self.rules_ui.open = open;
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
        gui::set_next_item_width(260.0);
        if gui::begin_combo("preset", &shown) {
            for p in presets.iter() {
                if gui::selectable(&label(p), p.name == set.preset, 0) {
                    pick = Some(p.name.clone());
                }
            }

            gui::end_combo();
        }

        if let Some(name) = pick {
            self.status = self.load_rules_preset(e, &name).unwrap_or_else(|err| err);
        }

        let file = current.as_ref().is_some_and(|p| !p.is_builtin());
        let mut result: Option<Result<String, String>> = None;
        if let Some(mut name) = self.rules_ui.save_as.take() {
            gui::set_next_item_width(200.0);
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
        } else {
            gui::begin_disabled(!file);
            if gui::small_button(&format!("{}  Save", icons::SAVE)) {
                result = Some(self.save_rules_preset(e, &set, None));
            }

            gui::end_disabled();
            gui::same_line();
            if gui::small_button("Save as...") {
                self.rules_ui.save_as = Some(String::new());
            }

            gui::same_line();
            gui::begin_disabled(!file);
            if gui::small_button(&format!("{}  Delete", icons::X)) {
                self.rules_ui.delete_ask = true;
            }

            gui::end_disabled();
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

    /// The list of rules and the selected one's fields.
    fn rules_list_and_editor(&mut self, e: &mut Editor) {
        let groups = document(e).map(|d| d.texture_group_slots()).unwrap_or_default();
        let ui = &mut self.rules_ui;
        let Some((_, Some(set))) = ui.draft.as_mut() else { return };
        let mut edited = false;

        gui::separator_text("Rules (later over earlier)");
        let n = set.rules.len();
        ui.selected = ui.selected.min(n.saturating_sub(1));
        gui::begin_child("##ruleslist", gui::v2(0.0, 150.0), gui::CHILD_BORDERS, 0);
        for (i, r) in set.rules.iter_mut().enumerate() {
            gui::push_id_int(i as i32);
            edited |= gui::checkbox("##on", &mut r.enabled);
            if gui::is_item_hovered() {
                gui::set_tooltip("On or off");
            }

            gui::same_line();
            let name = if r.name.is_empty() { format!("(rule {})", i + 1) } else { r.name.clone() };
            if gui::selectable(&format!("{name}   {}", r.group), ui.selected == i, 0) {
                ui.selected = i;
            }

            gui::pop_id();
        }

        gui::end_child();

        if gui::small_button(&format!("{}  Add", icons::PLUS)) {
            set.rules.push(Rule {
                name: "New rule".to_owned(),
                group: groups.first().cloned().unwrap_or_default(),
                seed: n as u32 + 1,
                ..Rule::default()
            });
            ui.selected = set.rules.len() - 1;
            edited = true;
        }

        gui::same_line();
        gui::begin_disabled(n == 0);
        if gui::small_button("Duplicate") && n > 0 {
            let mut r = set.rules[ui.selected].clone();
            r.name = format!("{} (copy)", r.name);
            set.rules.insert(ui.selected + 1, r);
            ui.selected += 1;
            edited = true;
        }

        gui::same_line();
        if gui::small_button(&format!("{}  Remove", icons::X)) && n > 0 {
            set.rules.remove(ui.selected);
            ui.selected = ui.selected.saturating_sub(1);
            edited = true;
        }

        gui::same_line();
        if gui::small_button("Up") && ui.selected > 0 {
            set.rules.swap(ui.selected, ui.selected - 1);
            ui.selected -= 1;
            edited = true;
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Earlier: under the rules after it");
        }

        gui::same_line();
        if gui::small_button("Down") && ui.selected + 1 < n {
            set.rules.swap(ui.selected, ui.selected + 1);
            ui.selected += 1;
            edited = true;
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Later: over the rules before it");
        }

        gui::end_disabled();
        if let Some(r) = set.rules.get_mut(ui.selected) {
            gui::separator_text("The selected rule");
            edited |= rule_fields(r, &groups);
        }

        gui::separator();
        gui::checkbox("Show its area in the viewport", &mut ui.show_area);
        if gui::is_item_hovered() {
            gui::set_tooltip("Outlines where the selected rule lays its texture (at least half of it), as though it were the only rule");
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

/// A rule's fields; whether any changed.
fn rule_fields(r: &mut Rule, groups: &[String]) -> bool {
    let mut edited = false;
    gui::set_next_item_width(240.0);
    edited |= gui::input_text("name", None, &mut r.name, 128, 0);
    gui::set_next_item_width(240.0);
    edited |= gui::input_text("texture group", Some("rock|rock_b|mountain"), &mut r.group, 128, 0);
    if gui::is_item_hovered() {
        gui::set_tooltip(
            "A texture group by name; several separated by |, the first this tile has (kit tiles name the same ground differently)",
        );
    }

    if !groups.is_empty() {
        gui::same_line();
        gui::set_next_item_width(70.0);
        if gui::begin_combo("##groups", "pick") {
            for g in groups {
                if gui::selectable(g, r.group == *g, 0) {
                    r.group.clone_from(g);
                    edited = true;
                }
            }

            gui::end_combo();
        }
    }

    gui::set_next_item_width(160.0);
    edited |= gui::drag_float("variation", &mut r.variation, 0.01, 0.0, 3.0, "%.2f");
    if gui::is_item_hovered() {
        gui::set_tooltip("Which of the group's four textures, 0 to 3; between two whole numbers a mix of both (1.3: 70 % of 1, 30 % of 2)");
    }

    gui::set_next_item_width(160.0);
    edited |= gui::drag_float("strength", &mut r.strength, 0.005, 0.0, 1.0, "%.2f");

    gui::separator_text("Where");
    edited |= range_row("height", "m", &mut r.height, (0.0, 50.0), 2.0, "The ground's height in metres (the sea at 0)");
    edited |= range_row("slope", "deg", &mut r.slope, (30.0, 90.0), 8.0, "The ground's slope in degrees");
    edited |= range_row(
        "hollow",
        "m",
        &mut r.hollow,
        (0.3, 100.0),
        0.3,
        "How far the ground lies under the mean ground around it, in metres: above 0 a hollow, below 0 a ridge",
    );
    if r.hollow.is_some() {
        gui::set_next_item_width(140.0);
        edited |= gui::drag_float("around", &mut r.hollow_radius, 0.1, 1.0, 200.0, "%.1f m");
        if gui::is_item_hovered() {
            gui::set_tooltip("How far around each place the mean ground is taken");
        }
    }

    edited |= range_row("water", "m", &mut r.water, (0.0, 5.0), 3.0, "Metres to the nearest water: rivers, lakes, the sea");
    edited |= range_row("road", "m", &mut r.road, (0.0, 2.0), 3.0, "Metres to the nearest procedural road");
    edited |=
        range_row("building", "m", &mut r.building, (0.0, 3.0), 3.0, "Metres to the nearest real building (walls, towers, gates, houses)");

    gui::separator_text("Ground type");
    let names = wce_tile::ground_types::GROUND_TYPES;
    let shown = r.ground.and_then(|g| names.get(usize::from(g.kind))).map_or("none (gameplay: off unless needed)", |(n, _)| n);
    gui::set_next_item_width(240.0);
    if gui::begin_combo("ground type", shown) {
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

    if gui::is_item_hovered() {
        gui::set_tooltip(
            "Also paints a ground type where the rule's weight is high enough: it changes sounds and how units move, so only when the \
             texture means it (rock, mud, sand)",
        );
    }

    if let Some(g) = r.ground.as_mut() {
        gui::set_next_item_width(140.0);
        edited |= gui::drag_float("from weight", &mut g.threshold, 0.01, 0.01, 1.0, "%.2f");
        if gui::is_item_hovered() {
            gui::set_tooltip("Where the rule's weight is at least this; the latest rule that lays a ground type at a place wins there");
        }
    }

    gui::separator_text("Breakup");
    let mut on = r.breakup.is_some();
    if gui::checkbox("noise on the edges", &mut on) {
        r.breakup = on.then_some(Breakup { scale: 15.0, amount: 0.6 });
        edited = true;
    }

    if gui::is_item_hovered() {
        gui::set_tooltip("Moves the rule's edges by noise, so that they are not contour lines; the inside of each range stays");
    }

    if let Some(b) = r.breakup.as_mut() {
        gui::set_next_item_width(140.0);
        edited |= gui::drag_float("size", &mut b.scale, 0.1, 1.0, 500.0, "%.1f m");
        gui::set_next_item_width(140.0);
        edited |= gui::drag_float("amount", &mut b.amount, 0.01, 0.0, 2.0, "%.2f");
        let mut seed = r.seed.min(i32::MAX as u32) as i32;
        gui::set_next_item_width(140.0);
        if gui::input_int("seed", &mut seed) {
            r.seed = seed.max(0) as u32;
            edited = true;
        }
    }

    edited
}

/// A condition's row: on or off, its ends (each may be open) and its fade; whether it changed.
fn range_row(label: &str, unit: &str, range: &mut Option<Range>, first: (f32, f32), fade: f32, tip: &str) -> bool {
    let mut edited = false;
    gui::push_id(label);
    let mut on = range.is_some();
    if gui::checkbox(label, &mut on) {
        *range = on.then_some(Range { lo: Some(first.0), hi: None, fade });
        edited = true;
    }

    if gui::is_item_hovered() {
        gui::set_tooltip(tip);
    }

    if let Some(r) = range.as_mut() {
        for (name, end, default) in [("from", &mut r.lo, first.0), ("to", &mut r.hi, first.1)] {
            let mut has = end.is_some();
            if gui::checkbox(&format!("{name}##has"), &mut has) {
                *end = has.then_some(default);
                edited = true;
            }

            gui::same_line();
            if let Some(v) = end.as_mut() {
                gui::set_next_item_width(120.0);
                edited |= gui::drag_float(&format!("##{name}"), v, 0.05, -10_000.0, 10_000.0, &format!("%.2f {unit}"));
            } else {
                gui::set_next_item_width(120.0);
                gui::text_disabled("no limit");
            }
            if name == "from" {
                gui::same_line();
            }
        }

        gui::set_next_item_width(120.0);
        edited |= gui::drag_float("fade", &mut r.fade, 0.05, 0.0, 1000.0, &format!("%.2f {unit}"));
        if gui::is_item_hovered() {
            gui::set_tooltip("How far each end blends, centred on it");
        }
    }

    gui::pop_id();
    edited
}
