//! The decal rules in the Inspector (docs/plans/auto_detail_2.md, E; `wce_tile::decal_rules`, `tile_document::decal_rules`): opened from
//! the Create panel's Decals > Decal rules entry. The tile's rules on or off, the preset they came from (Save, Save as, Delete), the list of
//! rules (with how many decals each laid) and the selected one: its decals (picked from the game's), how many, how big, which way, and
//! where (a texture rule's conditions, and how far from the trees). An edit is one undo entry: the panel works on a copy while a field
//! is being dragged or typed in, and gives it to the tile when no field is active. The decals follow by themselves.

use std::sync::Arc;

use wce_editor::editor::Editor;
use wce_editor::{gui, icons, theme};
use wce_scene::render_scene::DocumentId;
use wce_tile::decal_rules::{Align, DecalRule, DecalRuleSet, Preset, key_of, name_of, presets_root, scan_presets};
use wce_tile::texture_rules::{Range, Rule};

use crate::styled;
use crate::texture_rules_panel::{tile_heights, where_rows};
use crate::tool::{TileTool, document, document_id};

/// The panel's state.
#[derive(Default)]
pub struct DecalsUi {
    pub open: bool,
    pub selected: usize,

    /// Every number (advanced) or the plain choices.
    pub advanced: bool,

    /// The rules being edited, for a tile, and whether they differ from the tile's by an edit not yet given to it.
    draft: Option<(DocumentId, Option<DecalRuleSet>)>,
    draft_edited: bool,

    presets: Option<Arc<Vec<Preset>>>,
    save_as: Option<String>,
    delete_ask: bool,
}

/// The density's plain choices (decals per 100 m²).
const DENSITIES: [(&str, f32); 4] = [("Few", 0.3), ("Some", 1.0), ("Many", 3.0), ("Thick", 8.0)];

/// The ways' names, as [`Align::ALL`].
const WAYS: [&str; 5] = ["Any", "Slope", "Roads", "Walls", "Water"];

impl TileTool {
    /// Shows the tile's decal rules in the Inspector (the Create panel's Decals > Decal rules); a tile without any gets the built-in ones.
    pub fn open_decal_rules(&mut self) {
        self.close_detail_panels();
        self.decals_ui.open = true;
    }

    fn decal_presets_folder(e: &mut Editor) -> std::path::PathBuf {
        presets_root(&Self::workspace_dir(e).join("raw_data"))
    }

    /// The presets (the built-in one first), read on first use and again after a change.
    pub(crate) fn decal_presets(&mut self, e: &mut Editor) -> Arc<Vec<Preset>> {
        if let Some(p) = &self.decals_ui.presets {
            return p.clone();
        }

        let p = Arc::new(scan_presets(&Self::decal_presets_folder(e)));
        self.decals_ui.presets = Some(p.clone());
        p
    }

    /// Gives the tile a preset's decal rules (by name; `Default`: the built-in ones).
    ///
    /// # Errors
    ///
    /// No tile, or no such preset.
    pub(crate) fn load_decal_preset(&mut self, e: &mut Editor, name: &str) -> Result<String, String> {
        let p = self.decal_presets(e).iter().find(|p| p.name == name).cloned().ok_or_else(|| format!("no decal rule preset '{name}'"))?;
        let d = document(e).ok_or("no tile is open")?;
        d.set_decal_rules(Some(DecalRuleSet { preset: p.name.clone(), rules: p.rules.clone() }));
        self.decals_ui.draft_edited = false;
        Ok(format!("decal rules: the preset '{}' ({} rules)", p.name, p.rules.len()))
    }

    /// The tile's decal rules in the Inspector.
    pub(crate) fn inspect_decal_rules(&mut self, e: &mut Editor) {
        if !self.decals_ui.open {
            return;
        }

        let Some(id) = document_id(e) else {
            self.decals_ui.open = false;
            return;
        };

        let current = document(e).and_then(|d| d.decal_rules().cloned());
        let ui = &mut self.decals_ui;
        if ui.draft.as_ref().is_none_or(|(d, _)| *d != id) {
            ui.draft = Some((id, current.clone()));
            ui.draft_edited = false;
        } else if ui.draft_edited && !gui::is_any_item_active() {
            let rules = ui.draft.as_ref().and_then(|d| d.1.clone());
            ui.draft_edited = false;
            if let Some(d) = document(e) {
                d.set_decal_rules(rules);
            }
        } else if !ui.draft_edited {
            ui.draft = Some((id, current.clone()));
        }

        if styled::inspector_header(
            &format!("{}  Decal rules", icons::STICKER),
            "Decals laid by the ground: each rule lays its decals where its conditions hold (height, slope, hollows, how far from water, \
             roads, buildings and trees, where water runs), as many as its density says, spaced, sized and turned its way. They are made \
             again by themselves when the ground or the scene changes, and go into the tile when it is exported or saved for the kit.",
        ) {
            self.decals_ui.open = false;
            return;
        }

        let on = current.is_some();
        let half = (gui::content_region_avail().x - gui::item_spacing().x) * 0.5;
        let toggled = styled::toggle_chip("decalson", "Decals on this tile", on, half, "Decal rules on this tile, or none");
        gui::same_line();
        if let Some(i) = styled::segmented("mode", &["Simple", "Advanced"], Some(usize::from(self.decals_ui.advanced)), half) {
            self.decals_ui.advanced = i == 1;
        }

        if toggled {
            let rules = (!on).then(|| DecalRuleSet { preset: Preset::builtin().name, rules: Preset::builtin().rules });
            if let Some(d) = document(e) {
                d.set_decal_rules(rules);
            }

            self.decals_ui.draft = None;
            return;
        }

        if current.is_none() {
            gui::spacing();
            gui::text_disabled("No decal rules on this tile.");
            return;
        }

        self.decal_preset_rows(e);
        self.decal_list_and_editor(e);
        if let Some(d) = document(e) {
            let missing = d.decal_rules_missing();
            gui::push_text_wrap_pos(0.0);
            if !missing.is_empty() {
                let names = missing.iter().map(|(r, k)| format!("{k} ({r})")).collect::<Vec<_>>().join(", ");
                gui::text_colored(theme::warning(), &format!("Not in the game's files, laid as nothing: {names}"));
            }

            let status = if d.decals_busy() {
                "Being laid...".to_owned()
            } else {
                format!("{} decals on the tile.", d.decal_counts().iter().sum::<usize>())
            };
            gui::text_disabled(&status);
            gui::pop_text_wrap_pos();
        }
    }

    /// The preset combo and its buttons.
    fn decal_preset_rows(&mut self, e: &mut Editor) {
        let presets = self.decal_presets(e);
        let Some((_, Some(set))) = self.decals_ui.draft.clone() else { return };
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
        if gui::begin_combo("##decalpreset", &shown) {
            for p in presets.iter() {
                if gui::selectable(&label(p), p.name == set.preset, 0) {
                    pick = Some(p.name.clone());
                }
            }

            gui::end_combo();
        }

        let file = current.as_ref().is_some_and(|p| !p.is_builtin());
        let mut result: Option<Result<String, String>> = None;
        if self.decals_ui.save_as.is_none() && !self.decals_ui.delete_ask {
            gui::same_line();
            gui::begin_disabled(!file);
            if gui::button(&format!("{}##savedecals", icons::SAVE)) {
                result = Some(self.save_decal_preset(e, &set, None));
            }

            gui::end_disabled();
            if gui::is_item_hovered_allow_disabled() {
                gui::set_tooltip("Save the rules into their preset");
            }

            gui::same_line();
            if gui::button(&format!("{}##decalssaveas", icons::FILE_PLUS)) {
                self.decals_ui.save_as = Some(String::new());
            }

            if gui::is_item_hovered() {
                gui::set_tooltip("Save as a new preset...");
            }

            gui::same_line();
            gui::begin_disabled(!file);
            if gui::button(&format!("{}##deletedecals", icons::TRASH_2)) {
                self.decals_ui.delete_ask = true;
            }

            gui::end_disabled();
            if gui::is_item_hovered_allow_disabled() {
                gui::set_tooltip("Delete the preset's file");
            }
        }

        if let Some(name) = pick {
            self.status = self.load_decal_preset(e, &name).unwrap_or_else(|err| err);
        }

        if let Some(mut name) = self.decals_ui.save_as.take() {
            gui::set_next_item_width(gui::content_region_avail().x - gui::button_width("Save") - gui::button_width("Cancel") - 24.0);
            let enter = gui::input_text("##decalspreset", Some("the new preset's name"), &mut name, 128, gui::INPUT_ENTER_RETURNS_TRUE);
            gui::same_line();
            let ok = gui::small_button("Save") || enter;
            gui::same_line();
            if ok && !name.trim().is_empty() {
                result = Some(self.save_decal_preset(e, &set, Some(name.trim())));
            } else if !gui::small_button("Cancel") {
                self.decals_ui.save_as = Some(name);
            }
        } else if self.decals_ui.delete_ask {
            gui::text_colored(theme::warning(), &format!("Delete the preset '{}'?", set.preset));
            gui::same_line();
            if gui::small_button("Delete") {
                result = Some(self.delete_decal_preset(e, &set.preset));
                self.decals_ui.delete_ask = false;
            }

            gui::same_line();
            if gui::small_button("Cancel") {
                self.decals_ui.delete_ask = false;
            }
        }

        if let Some(r) = result {
            self.status = r.unwrap_or_else(|err| err);
        }
    }

    fn save_decal_preset(&mut self, e: &mut Editor, set: &DecalRuleSet, new_name: Option<&str>) -> Result<String, String> {
        let root = Self::decal_presets_folder(e);
        let presets = self.decal_presets(e);
        let preset = match new_name {
            Some(name) => {
                if name.contains(['/', '\\']) || name == wce_tile::decal_rules::BUILTIN {
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
        self.decals_ui.presets = None;
        if preset.name != set.preset
            && let Some(d) = document(e)
        {
            d.set_decal_rules(Some(DecalRuleSet { preset: preset.name.clone(), rules: set.rules.clone() }));
        }

        Ok(format!("decal rule preset '{}' saved", preset.name))
    }

    fn delete_decal_preset(&mut self, e: &mut Editor, name: &str) -> Result<String, String> {
        let presets = self.decal_presets(e);
        let p = presets.iter().find(|p| p.name == name && !p.is_builtin()).ok_or("the built-in preset can't be deleted")?;
        let path = p.path.clone().ok_or("no file")?;
        std::fs::remove_file(&path).map_err(|err| format!("{}: {err}", path.display()))?;
        self.decals_ui.presets = None;
        Ok(format!("decal rule preset '{name}' deleted"))
    }

    /// The list of rules and the selected one's fields.
    fn decal_list_and_editor(&mut self, e: &mut Editor) {
        let (heights, counts) = document(e).map(|d| (tile_heights(d), d.decal_counts())).unwrap_or(((0.0, 10.0), vec![]));
        let Some((did, Some(mut set))) = self.decals_ui.draft.clone() else { return };
        let mut selected = self.decals_ui.selected;
        let advanced = self.decals_ui.advanced;
        let mut edited = decal_list(&mut set.rules, &mut selected, &counts);
        if let Some(r) = set.rules.get_mut(selected) {
            gui::spacing();
            gui::separator_text("The selected rule");
            edited |= self.decal_rule_fields(e, r, heights, advanced);
        }

        self.decals_ui.selected = selected;
        if edited {
            self.decals_ui.draft = Some((did, Some(set)));
            self.decals_ui.draft_edited = true;
        }
    }

    /// A rule's fields: its name, its decals, how many, how big, which way, and where.
    fn decal_rule_fields(&mut self, e: &mut Editor, r: &mut DecalRule, heights: (f32, f32), advanced: bool) -> bool {
        let mut edited = false;
        gui::push_id("decalrule");
        let row = styled::row("Name", "");
        gui::set_next_item_width(row.width);
        edited |= gui::input_text("##name", None, &mut r.name, 128, 0);

        // The decals, picked from the game's; their weights kept by name.
        let row = styled::row("Decals", "The decals the rule lays, one picked at random for each place (weighted, in Advanced)");
        let mut keys = r.keys.iter().map(|(k, _)| key_of(k)).collect::<Vec<_>>();
        if self.key_list(e, "decals", "ProjectedDecalInstance", &mut keys, row.width, "") {
            r.keys = keys
                .iter()
                .map(|k| {
                    let name = name_of(k);
                    let w = r.keys.iter().find(|(n, _)| key_of(n).eq_ignore_ascii_case(k)).map_or(1.0, |(_, w)| *w);
                    (name, w)
                })
                .collect();
            edited = true;
        }

        if advanced {
            for (i, (k, w)) in r.keys.iter_mut().enumerate() {
                let row = styled::row(if i == 0 { "Weights" } else { "" }, "How often each decal is picked against the others");
                gui::set_next_item_width(row.width);
                edited |= gui::slider_float(&format!("##w{i}"), w, 0.0, 10.0, &format!("{k}: %.1f"));
            }
        }

        let row = styled::row("How many", "Decals per 100 m² where the rule holds fully, fewer as it fades");
        if advanced {
            gui::set_next_item_width(row.width);
            edited |= gui::slider_float("##density", &mut r.density, 0.0, 20.0, "%.2f / 100 m²");
        } else {
            let names = DENSITIES.map(|d| d.0);
            let current = DENSITIES.iter().position(|d| (d.1 - r.density).abs() < 1e-4);
            if let Some(i) = styled::segmented("density", &names, current, row.width) {
                r.density = DENSITIES[i].1;
                edited = true;
            }
        }

        let row = styled::row("Size", "How wide a decal is, metres: each one between these");
        let mut size = Range { lo: Some(r.size.0), hi: Some(r.size.1), fade: 0.0 };
        let bar = styled::Bar { domain: (0.0, 20.0), step: 0.25, unit: "m", open_ends: false, fade: false };
        if styled::range_bar("size", &mut size, &bar, row.width, true).0 {
            r.size = (size.lo.unwrap_or(r.size.0).max(0.1), size.hi.unwrap_or(r.size.1).max(0.1));
            edited = true;
        }

        let row = styled::row(
            "Way",
            "Which way a decal's length runs: any way (at random), down the slope, or along the nearest road, building's side (Walls) or water's edge",
        );
        let current = Align::ALL.iter().position(|a| *a == r.align);
        if let Some(i) = styled::segmented("way", &WAYS, current, row.width) {
            r.align = Align::ALL[i];
            edited = true;
        }

        if advanced {
            let row = styled::row("Stretch", "How many times longer than wide a decal is");
            gui::set_next_item_width(row.width);
            edited |= gui::slider_float("##stretch", &mut r.stretch, 0.2, 6.0, "%.2f x");
            let row = styled::row("Spacing", "The least distance between two of the rule's decals");
            gui::set_next_item_width(row.width);
            edited |= gui::slider_float("##spacing", &mut r.spacing, 0.0, 30.0, "%.1f m");
            let row = styled::row("Strength", "Scales how many are laid everywhere");
            gui::set_next_item_width(row.width);
            edited |= gui::slider_float("##strength", &mut r.place.strength, 0.0, 1.0, "%.2f");
        }

        gui::separator_text("Where");
        edited |= trees_row(r, advanced);
        edited |= where_rows(&mut r.place, heights, advanced);
        gui::pop_id();
        edited
    }
}

/// How far from the trees: a check box and a bar, its plain choices under it while on.
fn trees_row(r: &mut DecalRule, advanced: bool) -> bool {
    let mut edited = false;
    let first = Range { lo: None, hi: Some(3.0), fade: 2.0 };
    let mut on = r.trees.is_some();
    let (row, clicked) =
        styled::toggle_row("trees", "Trees", &mut on, "Metres from the nearest tree's trunk (the tile's, the biomes', the forest shown)");
    if clicked {
        r.trees = on.then_some(first);
        edited = true;
    }

    let bar = styled::Bar { domain: (0.0, 20.0), step: 0.5, unit: "m", open_ends: true, fade: true };
    let mut shown = r.trees.unwrap_or(first);
    let (changed, ask) = styled::range_bar("treesbar", &mut shown, &bar, row.width, r.trees.is_some());
    if ask || (changed && r.trees.is_some()) {
        r.trees = Some(shown);
        edited = true;
    }

    if let Some(t) = r.trees.as_mut()
        && !advanced
    {
        row.next_line();
        let presets = [
            ("Under", Range { lo: None, hi: Some(3.0), fade: 2.0 }),
            ("Near", Range { lo: None, hi: Some(8.0), fade: 4.0 }),
            ("Away", Range { lo: Some(8.0), hi: None, fade: 4.0 }),
        ];
        let names = presets.map(|p| p.0);
        let current = presets.iter().position(|p| p.1 == *t);
        if let Some(i) = styled::segmented("treeschoice", &names, current, row.width) {
            *t = presets[i].1;
            edited = true;
        }
    }

    edited
}

/// The rules as a list (on or off, the name, how many decals it laid), with its buttons in its section's title.
fn decal_list(rules: &mut Vec<DecalRule>, selected: &mut usize, counts: &[usize]) -> bool {
    let mut edited = false;
    gui::push_id("decallist");
    let n = rules.len();
    *selected = (*selected).min(n.saturating_sub(1));
    let at = *selected;
    let clicked = styled::section(
        "Rules",
        &[
            (icons::PLUS, "add", "Add a rule", true),
            (icons::COPY, "dup", "Duplicate the selected rule", n > 0),
            (icons::TRASH_2, "remove", "Remove the selected rule", n > 0),
            (icons::ARROW_BIG_UP, "up", "Move it up", at > 0),
            (icons::ARROW_BIG_DOWN, "down", "Move it down", at + 1 < n),
        ],
    );
    match clicked {
        Some(0) => {
            rules.push(DecalRule {
                name: "New decals".to_owned(),
                place: Rule { seed: n as u32 + 21, ..Rule::default() },
                ..DecalRule::default()
            });
            *selected = rules.len() - 1;
            edited = true;
        }
        Some(1) if n > 0 => {
            let mut r = rules[at].clone();
            r.name = format!("{} (copy)", r.name);
            r.place.seed = r.place.seed.wrapping_add(101);
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
        let name = if r.name.is_empty() { format!("(rule {})", i + 1) } else { r.name.clone() };
        gui::align_text_to_frame_padding();
        if gui::selectable(&name, *selected == i, 0) {
            *selected = i;
        }

        let (lo, hi) = gui::item_rect();
        let what = counts.get(i).map_or_else(String::new, |n| format!("{n}"));
        let w = gui::calc_text_width(&what);
        let y = lo.y + (hi.y - lo.y - gui::text_line_height()) * 0.5;
        gui::window_draw_list().text(gui::v2(hi.x - w - 6.0, y), theme::u32(theme::dim()), &what);
        gui::pop_id();
    }

    gui::end_child();
    gui::pop_id();
    edited
}
