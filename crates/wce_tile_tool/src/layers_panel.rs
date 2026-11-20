//! The Tile panel's Terrain layers section and the `terrain_layer` command (docs/plans/procedural_roads.md). The list reads as a stack: the top
//! row applies last, and the base is always at the bottom. A click on a row makes it the working layer: brushes of its type then edit it.

use wce_editor::editor::Editor;
use wce_editor::{gui, icons, theme};
use wce_scene::render_scene::DocumentId;
use wce_tile_editor::tile_document::TileDocument;
use wce_tile_editor::tile_document::layers::{Dependency, LayerRow, LayerType};

use crate::tool::TileTool;

const SECTION_TIP: &str = "Terrain made by composition: what you paint goes into the working layer and leaves the base alone, so it can be hidden, faded, reordered or removed. \
    Height layers add to what is below, texture layers go over it, ground type layers replace it. Save for Assembly Kit writes the terrain they make, as TEd has no layers.";

const EXPORTS_WITH_TIP: &str = "A scene layer this one goes with: exports (Save for Assembly Kit, the game) leave it out when that layer is set \
    to Don't Export. The editor shows it either way.";

/// The section's state.
#[derive(Default)]
pub struct LayersUi {
    kind: usize,
    name: String,

    /// *New layer for...*: the variations ticked for the new layer.
    new_for: Vec<String>,
}

/// A combined project's variations as checkboxes, those of `set` ticked; what was ticked when one changed (all ticked: empty, every
/// variation).
fn variation_checks(all: &[String], set: &[String], id: &str) -> Option<Vec<String>> {
    let mut out = None;
    for v in all {
        let mut on = set.is_empty() || set.contains(v);
        if gui::checkbox(&format!("{v}##{id}"), &mut on) {
            let now = all.iter().filter(|x| if *x == v { on } else { set.is_empty() || set.contains(x) }).cloned().collect::<Vec<_>>();
            out = Some(if now.len() == all.len() { vec![] } else { now });
        }
    }

    out
}

impl TileTool {
    pub(crate) fn draw_layers_section(&mut self, e: &mut Editor, id: DocumentId) {
        if !gui::collapsing_header("Terrain layers", 0) {
            return;
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(SECTION_TIP);
        }

        if gui::begin_tab_bar("##layer_types") {
            for (k, kind) in LayerType::ALL.into_iter().enumerate() {
                if gui::begin_tab_item(kind.title(), 0) {
                    self.layers.kind = k;
                    let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
                    self.draw_layer_list(d, kind);
                    gui::end_tab_item();
                }
            }

            gui::end_tab_bar();
        }
    }

    fn draw_layer_list(&mut self, d: &mut TileDocument, kind: LayerType) {
        let (rows, working) = d.terrain_layers(kind);
        let all = d.tile().variations().to_vec();
        let can_add = gui::button(&format!("{} New layer", icons::PLUS));
        if gui::is_item_hovered() {
            gui::set_tooltip(if all.is_empty() {
                "Adds an empty layer of this type on top; it becomes the working layer."
            } else {
                "Adds an empty layer of this type on top, of every variation; it becomes the working layer."
            });
        }

        // A combined project: a layer of some variations only (a patch: `small, medium` under one of `medium` builds on it).
        let mut for_some = None;
        if !all.is_empty() {
            gui::same_line();
            if gui::button(&format!("{} New layer for...", icons::PLUS)) {
                self.layers.new_for.retain(|v| all.contains(v));
                gui::open_popup("new_layer_for");
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(
                    "Adds an empty layer that the variations you tick have (a patch of their terrain); it becomes the working layer.",
                );
            }

            if gui::begin_popup("new_layer_for") {
                gui::text_disabled("The new layer's variations:");
                if let Some(list) = variation_checks(&all, &self.layers.new_for, "new_for") {
                    self.layers.new_for = list;
                }

                let label = if self.layers.new_for.is_empty() { "all".to_owned() } else { self.layers.new_for.join(", ") };
                if gui::button(&format!("Add the layer of {label}")) {
                    for_some = Some(self.layers.new_for.clone());
                    gui::close_current_popup();
                }

                gui::end_popup();
            }
        }

        if can_add || for_some.is_some() {
            let name = format!("Layer {}", rows.len() + 1);
            if let Err(why) = d.add_terrain_layer_for(kind, &name, &for_some.unwrap_or_default()) {
                self.status = why;
            }

            return;
        }

        let selected = working.and_then(|id| rows.iter().find(|r| r.id == id)).cloned();
        if let Some(row) = &selected {
            self.draw_layer_actions(d, kind, row, &rows);
        }

        gui::spacing();
        for row in rows.iter().rev() {
            gui::push_id(&format!("layer{}", row.id));
            let eye = if row.visible { icons::EYE } else { icons::EYE_OFF };
            if gui::small_button(eye) {
                d.set_terrain_layer_visible(kind, row.id, !row.visible);
            }

            if gui::is_item_hovered() {
                gui::set_tooltip("Shows or hides the layer; a hidden layer leaves the terrain as if it weren't there.");
            }

            // A combined project's: the layer's variations, a click changes them.
            if !all.is_empty() {
                gui::same_line();
                let text = if row.variations.is_empty() { "all".to_owned() } else { row.variations.join(", ") };
                if gui::small_button(&text) {
                    gui::open_popup("layer_variations");
                }

                if gui::is_item_hovered() {
                    gui::set_tooltip("The variations this layer belongs to (all: every one). Click to change them.");
                }

                if gui::begin_popup("layer_variations") {
                    if let Some(list) = variation_checks(&all, &row.variations, "layer_var") {
                        d.set_terrain_layer_variations(kind, row.id, &list);
                    }

                    gui::end_popup();
                }
            }

            gui::same_line();
            let label = row.name.clone();
            if !row.in_view {
                gui::push_style_color(gui::COL_TEXT, theme::v4(theme::dim()));
            }

            if gui::selectable(&label, working == Some(row.id), 0) {
                d.set_working_layer(kind, Some(row.id));
            }

            if !row.in_view {
                gui::pop_style_color(1);
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(if row.in_view {
                    "Makes it the working layer: brushes of this type paint into it."
                } else {
                    "In no variation shown (it belongs to others, or exports with a scene layer they lack): it does not show. A click makes it the working layer."
                });
            }

            gui::pop_id();
        }

        // The texture rules' layer lies right over the base, under every layer of the user's: shown there, edited in the Inspector.
        if kind == LayerType::Blend {
            gui::push_id("layer_rules");
            let rules = d.texture_rules().map(|r| r.rules.iter().filter(|r| r.enabled).count());
            gui::small_button(icons::SPARKLES);
            if gui::is_item_hovered() {
                gui::set_tooltip("Texture rules: textures laid by the terrain, under your layers.");
            }

            gui::same_line();
            let label = match rules {
                Some(n) => format!("Texture rules ({n})"),
                None => "+ Texture rules".to_owned(),
            };
            if gui::selectable(&label, self.rules_ui.open, 0) {
                if rules.is_none() {
                    let p = wce_tile::texture_rules::Preset::builtin();
                    d.set_texture_rules(Some(wce_tile::texture_rules::RuleSet { preset: p.name, rules: p.rules }));
                }

                self.open_texture_rules();
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(if rules.is_some() {
                    "Shows the texture rules in the Inspector: rock on slopes, mud by the water and so on, laid under what you paint."
                } else {
                    "Adds texture rules (the built-in Default) and shows them in the Inspector: textures laid by the terrain, under what you paint."
                });
            }

            gui::pop_id();
        }

        gui::push_id("layer_base");
        gui::small_button(icons::LAYERS);
        if gui::is_item_hovered() {
            gui::set_tooltip("The base: the tile's own terrain, always at the bottom.");
        }

        gui::same_line();
        if gui::selectable("Base", working.is_none(), 0) {
            d.set_working_layer(kind, None);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Paint straight into the tile's own terrain, no layer.");
        }

        gui::pop_id();
        match &selected {
            Some(row) if !row.visible => {
                gui::push_style_color(gui::COL_TEXT, theme::v4(theme::warning()));
                gui::text_wrapped("The working layer is hidden: what you paint goes into it, but won't show until you show it.");
                gui::pop_style_color(1);
            }

            Some(row) if !row.in_view => {
                gui::push_style_color(gui::COL_TEXT, theme::v4(theme::warning()));
                gui::text_wrapped(
                    "The working layer is in no variation shown: what you paint goes into it, but won't show until one of its variations is shown.",
                );
                gui::pop_style_color(1);
            }

            _ => {}
        }
    }

    /// The working layer's name, opacity and actions.
    fn draw_layer_actions(&mut self, d: &mut TileDocument, kind: LayerType, row: &LayerRow, rows: &[LayerRow]) {
        gui::label_column();
        self.layers.name.clone_from(&row.name);
        gui::row("Name");
        if gui::input_text("##layer_name", None, &mut self.layers.name, 64, 0) {
            d.rename_terrain_layer(kind, row.id, &self.layers.name);
        }

        if gui::is_item_deactivated() {
            d.commit_terrain_layer("Rename layer", kind);
        }

        let mut opacity = row.opacity;
        gui::row("Opacity");
        if gui::slider_float("##layer_opacity", &mut opacity, 0.0, 1.0, "%.2f") {
            d.set_terrain_layer_opacity(kind, row.id, opacity);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("How much of the layer applies: 0 none, 1 all of it.");
        }

        if gui::is_item_deactivated() {
            d.commit_terrain_layer("Layer opacity", kind);
        }

        // A combined project's variations it belongs to (all ticked: every one).
        let all = d.tile().variations().to_vec();
        if !all.is_empty() {
            gui::row_tip("Variations", "The variations this layer belongs to (all ticked: every one, and the ones added later).");
            for (i, v) in all.iter().enumerate() {
                if i > 0 {
                    gui::same_line();
                }

                let mut on = row.variations.is_empty() || row.variations.contains(v);
                if gui::checkbox(&format!("{v}##working_layer_var"), &mut on) {
                    let list = all
                        .iter()
                        .filter(|x| if *x == v { on } else { row.variations.is_empty() || row.variations.contains(x) })
                        .cloned()
                        .collect::<Vec<_>>();
                    let list = if list.len() == all.len() { vec![] } else { list };
                    d.set_terrain_layer_variations(kind, row.id, &list);
                }
            }
        }

        // The scene layer it exports with.
        let scene = d.scene_layers();
        let dependency = d.terrain_layer_dependency(&row.depends);
        let mut names = vec!["Always".to_owned()];
        names.extend(scene.iter().map(|(_, n)| if n.is_empty() { "(unnamed)".to_owned() } else { n.clone() }));
        let mut chosen = match &dependency {
            Dependency::On { elem, .. } => scene.iter().position(|(e, _)| e == elem).map_or(0, |i| i as i32 + 1),
            _ => 0,
        };

        gui::row_tip("Exports with", EXPORTS_WITH_TIP);
        let items = names.iter().map(String::as_str).collect::<Vec<_>>();
        if gui::combo("##layer_depends", &mut chosen, &items) {
            d.set_terrain_layer_dependency(kind, row.id, (chosen > 0).then(|| scene[chosen as usize - 1].0));
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(EXPORTS_WITH_TIP);
        }

        let note = match &dependency {
            Dependency::Gone => Some("Its scene layer is gone: it always exports.".to_owned()),
            Dependency::On { name, export, .. } if export == "Off" => {
                Some(format!("Left out of exports: '{name}' is set to Don't Export."))
            }
            Dependency::On { export, .. } if export.starts_with("Escalation") => {
                Some("Escalation layers have no terrain of their own: this exports with the tile at every escalation level.".to_owned())
            }

            _ => None,
        };

        if let Some(note) = note {
            gui::push_style_color(gui::COL_TEXT, theme::v4(theme::warning()));
            gui::text_wrapped(&note);
            gui::pop_style_color(1);
        }

        let at = rows.iter().position(|r| r.id == row.id).unwrap_or(0);
        gui::begin_disabled(at + 1 >= rows.len());
        if gui::small_button(icons::ARROW_BIG_UP) {
            d.move_terrain_layer(kind, row.id, 1);
        }

        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip("Moves the layer up the stack (it applies later).");
        }

        gui::end_disabled();
        gui::same_line();
        gui::begin_disabled(at == 0);
        if gui::small_button(icons::ARROW_BIG_DOWN) {
            d.move_terrain_layer(kind, row.id, -1);
        }

        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip("Moves the layer down the stack (it applies earlier).");
        }

        gui::end_disabled();
        gui::same_line();
        if gui::small_button("Merge down") {
            d.merge_terrain_layer(kind, row.id);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Merges the layer into the one below it, or into the base.");
        }

        gui::same_line();
        if gui::small_button(icons::TRASH_2) {
            d.remove_terrain_layer(kind, row.id);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Removes the layer and what it did.");
        }
    }
}
