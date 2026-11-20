//! A field area's settings in the Inspector (docs/plans/auto_detail_2.md, G; `wce_tile::fields`, `tile_document::fields`): under the
//! selected area's properties. Simple: how wide the strips are, how much is orchard, the fence, the props, the ground under it, Reroll;
//! Advanced: every number and what each lays (picked from the game's). An edit is one undo entry (the area's `wce_fields` attribute): the
//! panel works on a copy while a field is being dragged or typed in, and gives it to the area when no field is active.

use wce_editor::editor::Editor;
use wce_editor::{gui, icons};
use wce_tile::fields::{FIELDS_ATTR, FieldSettings, SEED_ATTR};

use crate::styled;
use crate::tool::{TileTool, document, primary};

/// The panel's state: the settings being edited, of an area, and whether they differ from the area's by an edit not yet given to it.
#[derive(Default)]
pub struct FieldsUi {
    draft: Option<(usize, FieldSettings)>,
    edited: bool,
    pub advanced: bool,
}

const WIDTHS: [(&str, f32); 4] = [("Narrow", 6.0), ("Medium", 12.0), ("Wide", 20.0), ("Broad", 30.0)];
const ORCHARDS: [(&str, f32); 4] = [("None", 0.0), ("Some", 0.2), ("Half", 0.5), ("All", 1.0)];
const PROPS: [(&str, f32); 4] = [("None", 0.0), ("Few", 2.0), ("Some", 4.0), ("Many", 10.0)];
const GROUNDS: [(&str, f32); 4] = [("None", 0.0), ("Light", 0.3), ("Worn", 0.5), ("Bare", 0.8)];

/// A row of plain choices; whether one was picked.
fn choices(id: &str, label: &str, tip: &str, v: &mut f32, options: &[(&str, f32)]) -> bool {
    let row = styled::row(label, tip);
    let names = options.iter().map(|o| o.0).collect::<Vec<_>>();
    let current = options.iter().position(|o| (o.1 - *v).abs() < 1e-4);
    match styled::segmented(id, &names, current, row.width) {
        Some(i) => {
            *v = options[i].1;
            true
        }
        None => false,
    }
}

fn slider(id: &str, label: &str, tip: &str, v: &mut f32, (lo, hi): (f32, f32), format: &str) -> bool {
    let row = styled::row(label, tip);
    gui::set_next_item_width(row.width);
    gui::slider_float(&format!("##{id}"), v, lo, hi, format)
}

impl TileTool {
    /// The selected field area, when one alone is selected: its element and settings.
    pub(crate) fn selected_field_area(&self, e: &mut Editor) -> Option<wce_tile_editor::tile_document::fields::FieldArea> {
        let sel = primary(e)?;
        (e.selection.items().len() == 1).then_some(())?;
        document(e)?.tile().field_areas().into_iter().find(|a| a.elem == sel)
    }

    /// Below a field area's properties in the Inspector: its settings, Reroll, what it lays.
    pub(crate) fn inspect_field_area(&mut self, e: &mut Editor) {
        let Some(area) = self.selected_field_area(e) else {
            self.fields_ui.draft = None;
            return;
        };

        // The copy: the area's own unless an edit waits; an edit is given to the area once no field is being edited.
        let ui = &mut self.fields_ui;
        if ui.draft.as_ref().is_none_or(|(elem, _)| *elem != area.elem) {
            ui.draft = Some((area.elem, area.settings.clone()));
            ui.edited = false;
        } else if ui.edited && !gui::is_any_item_active() {
            ui.edited = false;
            if let (Some(d), Some((elem, s))) = (document(e), ui.draft.clone()) {
                d.set_attribute(elem, FIELDS_ATTR, &s.to_text());
            }
        } else if !ui.edited {
            ui.draft = Some((area.elem, area.settings.clone()));
        }

        gui::separator_text(&format!("{}  Fields", icons::LEAF));
        let half = (gui::content_region_avail().x - gui::item_spacing().x) * 0.5;
        if let Some(i) = styled::segmented("fieldsmode", &["Simple", "Advanced"], Some(usize::from(self.fields_ui.advanced)), half) {
            self.fields_ui.advanced = i == 1;
        }

        gui::same_line();
        if gui::button_sized(&format!("{}  Reroll", icons::REFRESH_CW), gui::v2(half, 0.0)) {
            let seed = ((self.random.unit() * 1.0e6) as u32).to_string();
            if let Some(d) = document(e) {
                d.set_attribute(area.elem, SEED_ATTR, &seed);
            }
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Another layout: other crops on the strips, other orchards, props elsewhere");
        }

        let Some((elem, mut s)) = self.fields_ui.draft.clone() else { return };
        let advanced = self.fields_ui.advanced;
        let before = s.clone();
        choices("width", "Strips", "How wide each strip of crops is", &mut s.strip_width, &WIDTHS);
        choices("orchard", "Orchards", "How many of the strips are orchards: rows of trees instead of crops", &mut s.orchard, &ORCHARDS);
        choices("props", "Haystacks", "Haystacks and carts on the crops, how many a hectare", &mut s.props_per_ha, &PROPS);
        choices("ground", "Ground", "How worn the ground under the fields is (the texture rules' layer)", &mut s.ground_strength, &GROUNDS);
        let mut fenced = !s.fence.is_empty();
        let (_, clicked) = styled::toggle_row("fence", "Fence", &mut fenced, "A fence round the area, as a building spline lays it");
        gui::text_disabled(if fenced { "round the area" } else { "none" });
        if clicked {
            s.fence = if fenced { FieldSettings::default().fence } else { vec![] };
        }

        if advanced {
            gui::separator_text("Strips");
            slider("verge", "Verge", "The grass between two strips", &mut s.verge, (0.0, 10.0), "%.1f m");
            let mut auto = s.angle.is_none();
            let (row, clicked) =
                styled::toggle_row("auto", "Longest side", &mut auto, "The strips run along the area's longest side (else at the angle)");
            if clicked {
                s.angle = if auto { None } else { Some(0.0) };
            }

            if let Some(a) = s.angle.as_mut() {
                row.next_line();
                gui::set_next_item_width(row.width);
                gui::slider_float("##angle", a, -180.0, 180.0, "%.0f deg");
            } else {
                gui::text_disabled("along the longest side");
            }

            slider("croplen", "Crop pieces", "How long each crop decal is along its strip", &mut s.crop_length, (5.0, 60.0), "%.0f m");
            let mut rows = s.rows as i32;
            let row = styled::row("Tree rows", "Rows of trees in an orchard strip");
            gui::set_next_item_width(row.width);
            if gui::slider_int("##rows", &mut rows, 1, 6) {
                s.rows = rows.max(1) as u32;
            }

            slider("treespace", "Tree spacing", "Between two trees of a row", &mut s.tree_spacing, (2.0, 20.0), "%.1f m");
            gui::separator_text("What it lays");
            let row = styled::row("Crops", "The field decals a strip is laid with, one picked a strip");
            let mut crops = s.crops.iter().map(|k| wce_tile::decal_rules::key_of(k)).collect::<Vec<_>>();
            if self.key_list(e, "crops", "ProjectedDecalInstance", &mut crops, row.width, "") {
                s.crops = crops.iter().map(|k| wce_tile::decal_rules::name_of(k)).collect();
            }

            let row = styled::row("Trees", "The orchards' trees");
            self.key_list(e, "orchardtrees", "VEGETATION_INSTANCE", &mut s.orchard_trees, row.width, "");
            let row = styled::row("Fence", "The fence's pieces, laid end to end round the area");
            self.key_list(e, "fencekeys", "BUILDING_INSTANCE", &mut s.fence, row.width, "");
            let row = styled::row("Props", "The haystacks and carts");
            self.key_list(e, "propkeys", "BUILDING_INSTANCE", &mut s.props, row.width, "incidental");
            let row =
                styled::row("Ground", "The texture under the fields: a texture group (several separated by |, the first the tile has)");
            gui::set_next_item_width(row.width);
            gui::input_text("##groundgroup", Some("mud|mud_a"), &mut s.ground, 128, 0);
            slider("groundvar", "Its texture", "Which of the group's four textures (1 to 4)", &mut s.ground_variation, (0.0, 3.0), "%.1f");
        }

        // Given to the area once no field is being edited (a plain choice, at the next frame).
        if s != before {
            self.fields_ui.draft = Some((elem, s));
            self.fields_ui.edited = true;
        }

        if let Some(d) = document(e) {
            let (decals, trees, buildings) = d.field_counts();
            gui::push_text_wrap_pos(0.0);
            gui::text_disabled(&format!("The tile's fields: {decals} crop decals, {trees} trees, {buildings} props and fence pieces."));
            gui::pop_text_wrap_pos();
        }
    }
}
