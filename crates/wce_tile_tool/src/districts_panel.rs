//! A district's settings in the Inspector (docs/plans/auto_detail_2.md, I; `wce_tile::districts`, `tile_document::districts`): under the
//! selected district's properties. Its buildings (picked from the game's; none: the tile's own houses), how far they stand back from the
//! roads and from each other, the most slope, whether the inside is filled, which way they face; Fill, Reroll and fill. An edit of the
//! settings is one undo entry (the district's `wce_district` attribute); filling is one more.

use wce_editor::editor::Editor;
use wce_editor::{gui, icons};
use wce_tile::districts::{DISTRICT_ATTR, DistrictSettings, SEED_ATTR};

use crate::styled;
use crate::tool::{TileTool, document, primary};

/// The panel's state: the settings being edited, of a district, and whether they differ from its own by an edit not yet given to it.
#[derive(Default)]
pub struct DistrictsUi {
    draft: Option<(usize, DistrictSettings)>,
    edited: bool,
}

/// Which way a building's front is turned against facing its road.
const FACINGS: [(&str, f32); 4] = [("Front", 0.0), ("Back", 180.0), ("Left", 90.0), ("Right", -90.0)];

fn slider(id: &str, label: &str, tip: &str, v: &mut f32, (lo, hi): (f32, f32), format: &str) {
    let row = styled::row(label, tip);
    gui::set_next_item_width(row.width);
    gui::slider_float(&format!("##{id}"), v, lo, hi, format);
}

impl TileTool {
    /// The selected district, when one alone is selected.
    pub(crate) fn selected_district(&self, e: &mut Editor) -> Option<wce_tile_editor::tile_document::districts::DistrictArea> {
        let sel = primary(e)?;
        (e.selection.items().len() == 1).then_some(())?;
        document(e)?.tile().district_areas().into_iter().find(|a| a.elem == sel)
    }

    /// Below a district's properties in the Inspector: its settings, Fill, Reroll and fill.
    pub(crate) fn inspect_district(&mut self, e: &mut Editor) {
        let Some(area) = self.selected_district(e) else {
            self.districts_ui.draft = None;
            return;
        };

        let ui = &mut self.districts_ui;
        if ui.draft.as_ref().is_none_or(|(elem, _)| *elem != area.elem) {
            ui.draft = Some((area.elem, area.settings.clone()));
            ui.edited = false;
        } else if ui.edited && !gui::is_any_item_active() {
            ui.edited = false;
            if let (Some(d), Some((elem, s))) = (document(e), ui.draft.clone()) {
                d.set_attribute(elem, DISTRICT_ATTR, &s.to_text());
            }
        } else if !ui.edited {
            ui.draft = Some((area.elem, area.settings.clone()));
        }

        gui::separator_text(&format!("{}  District", icons::BOX));
        let half = (gui::content_region_avail().x - gui::item_spacing().x) * 0.5;
        let mut fill = gui::button_sized(&format!("{}  Fill", icons::SPARKLES), gui::v2(half, 0.0));
        if gui::is_item_hovered() {
            gui::set_tooltip("Fills the district with buildings, replacing those it was filled with before (one undo step)");
        }

        gui::same_line();
        if gui::button_sized(&format!("{}  Reroll and fill", icons::REFRESH_CW), gui::v2(half, 0.0)) {
            let seed = ((self.random.unit() * 1.0e6) as u32).to_string();
            if let Some(d) = document(e) {
                d.set_attribute(area.elem, SEED_ATTR, &seed);
            }

            fill = true;
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Another layout of the same buildings");
        }

        let Some((elem, mut s)) = self.districts_ui.draft.clone() else { return };
        let before = s.clone();
        let row = styled::row("Buildings", "What the district is filled with, each as likely; none: the tile's own houses");
        if s.keys.is_empty() {
            let own = document(e).map(|d| d.tile().district_default_keys()).unwrap_or_default();
            gui::text_disabled(&format!("the tile's own houses ({})", own.len()));
            row.next_line();
            if gui::small_button("Pick them...") {
                s.keys = own;
            }
        } else {
            self.key_list(e, "districtkeys", "BUILDING_INSTANCE", &mut s.keys, row.width, "generic");
        }

        slider("setback", "Setback", "From a road's edge to a building's front", &mut s.setback, (0.0, 15.0), "%.1f m");
        slider("spacing", "Spacing", "The least distance between two buildings", &mut s.spacing, (0.0, 15.0), "%.1f m");
        slider("slope", "Most slope", "No building stands on steeper ground", &mut s.max_slope, (0.0, 40.0), "%.0f deg");
        let (_, _) = styled::toggle_row("inside", "Inside", &mut s.inside, "Fill the inside too, each building facing the nearest road");
        gui::text_disabled(if s.inside { "filled too" } else { "only along the roads" });
        let row = styled::row("Front", "Which side of a building faces its road: its front, unless the kit's buildings face another way");
        let names = FACINGS.map(|f| f.0);
        let current = FACINGS.iter().position(|f| (f.1 - s.facing).abs() < 1e-3);
        if let Some(i) = styled::segmented("facing", &names, current, row.width) {
            s.facing = FACINGS[i].1;
        }

        slider(
            "jitter",
            "Turned off",
            "How far a building may be turned off facing its road, each way",
            &mut s.jitter,
            (0.0, 30.0),
            "%.0f deg",
        );

        if s != before {
            self.districts_ui.draft = Some((elem, s.clone()));
            self.districts_ui.edited = true;
        }

        // Filling takes the settings as they are now.
        if fill && let Some(d) = document(e) {
            if s != area.settings {
                d.set_attribute(elem, DISTRICT_ATTR, &s.to_text());
                self.districts_ui.edited = false;
            }

            self.status = match d.fill_district(elem) {
                Ok(n) => format!("the district filled with {n} buildings"),
                Err(err) => err,
            };
        }

        if let Some(d) = document(e) {
            gui::push_text_wrap_pos(0.0);
            gui::text_disabled(&format!("{} buildings in it.", d.district_buildings(elem)));
            let keys = if s.keys.is_empty() { d.tile().district_default_keys() } else { s.keys.clone() };
            let unknown = keys.iter().filter(|k| d.building_footprint(k).is_none()).count();
            if unknown > 0 {
                gui::text_colored(
                    wce_editor::theme::warning(),
                    &format!(
                        "{unknown} of its buildings' sizes are not known yet ({0} x {0} m guessed): Fill again once they show in the viewport.",
                        wce_tile_editor::tile_document::districts::GUESS
                    ),
                );
            }

            gui::pop_text_wrap_pos();
        }
    }
}
