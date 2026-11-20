//! The erosion in the Inspector (docs/plans/auto_detail_2.md, D; `wce_tile::erosion`, `tile_document::erosion`): opened from Terrain
//! layers > Height's "Erosion" row. On or off, a preset, how much is kept and the seed; Advanced: every number. An edit is one undo entry:
//! the panel works on a copy while a field is being dragged or typed in, and gives it to the tile when no field is active. The tile's
//! layer follows by itself.

use wce_editor::editor::Editor;
use wce_editor::{gui, icons, theme};
use wce_scene::render_scene::DocumentId;
use wce_tile::erosion::{PRESETS, Settings};

use crate::styled;
use crate::tool::{TileTool, document, document_id};

/// The panel's state.
#[derive(Default)]
pub struct ErosionUi {
    pub open: bool,

    /// Every number (advanced) or the plain choices.
    pub advanced: bool,

    /// The settings being edited, for a tile, and whether they differ from the tile's by an edit not yet given to it.
    draft: Option<(DocumentId, Option<Settings>)>,
    draft_edited: bool,
}

/// Whether two settings are the same but for their seed: a preset's, whatever its seed.
fn same_but_seed(a: &Settings, b: &Settings) -> bool {
    *a == Settings { seed: a.seed, ..b.clone() }
}

impl TileTool {
    /// Shows the tile's erosion in the Inspector (Terrain layers > Height, "Erosion").
    pub fn open_erosion(&mut self) {
        self.close_detail_panels();
        self.erosion_ui.open = true;
    }

    /// The tile's erosion in the Inspector: on or off, the preset, how much, the seed; every number in Advanced.
    pub(crate) fn inspect_erosion(&mut self, e: &mut Editor) {
        if !self.erosion_ui.open {
            return;
        }

        let Some(id) = document_id(e) else {
            self.erosion_ui.open = false;
            return;
        };

        let current = document(e).and_then(|d| d.erosion().cloned());
        let ui = &mut self.erosion_ui;
        if ui.draft.as_ref().is_none_or(|(d, _)| *d != id) {
            ui.draft = Some((id, current.clone()));
            ui.draft_edited = false;
        } else if ui.draft_edited && !gui::is_any_item_active() {
            let settings = ui.draft.as_ref().and_then(|d| d.1.clone());
            ui.draft_edited = false;
            if let Some(d) = document(e) {
                d.set_erosion(settings);
            }
        } else if !ui.draft_edited {
            ui.draft = Some((id, current.clone()));
        }

        if styled::inspector_header(
            &format!("{}  Erosion", icons::MOUNTAIN),
            "Weathers the ground you shaped: rain wears channels down the slopes and lays what it carries where it slows, loose ground \
             slides off what is too steep, and fine bumps break up smooth surfaces. It is a height layer of its own over yours, under \
             the roads, walls and prefabs' ground, made again by itself when the ground under it changes; buildings keep their ground. \
             Where the water ran feeds the texture rules' Water flow and Deposit.",
        ) {
            self.erosion_ui.open = false;
            return;
        }

        let on = current.is_some();
        let half = (gui::content_region_avail().x - gui::item_spacing().x) * 0.5;
        let toggled = styled::toggle_chip("erosionon", "Erosion on this tile", on, half, "Erosion on this tile, or none");
        gui::same_line();
        if let Some(i) = styled::segmented("mode", &["Simple", "Advanced"], Some(usize::from(self.erosion_ui.advanced)), half) {
            self.erosion_ui.advanced = i == 1;
        }

        if toggled {
            if let Some(d) = document(e) {
                d.set_erosion((!on).then(Settings::default));
            }

            self.erosion_ui.draft = None;
            return;
        }

        let advanced = self.erosion_ui.advanced;
        let Some((_, Some(mut s))) = self.erosion_ui.draft.clone() else {
            gui::spacing();
            gui::text_disabled("No erosion: the ground is as you shaped it.");
            return;
        };

        let before = s.clone();
        gui::spacing();
        settings_rows(&mut s, advanced);
        if s != before {
            self.erosion_ui.draft = Some((id, Some(s)));
            self.erosion_ui.draft_edited = true;
        }

        gui::spacing();
        if let Some(d) = document(e) {
            let status = if d.erosion_busy() {
                "Being made...".to_owned()
            } else {
                d.erosion_summary().unwrap_or_else(|| "Not made yet.".to_owned())
            };
            gui::push_text_wrap_pos(0.0);
            gui::text_colored(theme::dim(), &status);
            gui::pop_text_wrap_pos();
        }
    }
}

/// A slider in a row, its value shown with `format`.
fn slider(id: &str, label: &str, tip: &str, v: &mut f32, (lo, hi): (f32, f32), format: &str) {
    let row = styled::row(label, tip);
    gui::set_next_item_width(row.width);
    gui::slider_float(&format!("##{id}"), v, lo, hi, format);
}

/// The same for a whole number.
fn slider_int(id: &str, label: &str, tip: &str, v: &mut u32, (lo, hi): (u32, u32)) {
    let row = styled::row(label, tip);
    gui::set_next_item_width(row.width);
    let mut i = *v as i32;
    if gui::slider_int(&format!("##{id}"), &mut i, lo as i32, hi as i32) {
        *v = i.max(0) as u32;
    }
}

/// The settings' rows: the preset, how much, the seed; in Advanced every number.
fn settings_rows(s: &mut Settings, advanced: bool) {
    let row = styled::row("Preset", "The built-in settings: a lightly weathered ground, a weathered one, a rugged one.");
    let names = PRESETS.iter().map(|p| p.0).collect::<Vec<_>>();
    let current = PRESETS.iter().position(|(_, p)| same_but_seed(s, p));
    if let Some(i) = styled::segmented("preset", &names, current, row.width) {
        *s = Settings { seed: s.seed, ..PRESETS[i].1.clone() };
    }

    let mut pct = s.strength * 100.0;
    slider("strength", "Strength", "How much of the change is kept: 0 % leaves the ground as it is.", &mut pct, (0.0, 100.0), "%.0f %%");
    s.strength = pct / 100.0;

    let row = styled::row("Seed", "Another seed: other drops, other channels, other bumps; the same seed always gives the same ground.");
    let reroll_w = gui::button_width(icons::REFRESH_CW) + gui::item_spacing().x;
    gui::set_next_item_width((row.width - reroll_w).max(40.0));
    let mut seed = s.seed.min(i32::MAX as u64) as i32;
    if gui::input_int("##seed", &mut seed) {
        s.seed = seed.max(0) as u64;
    }

    gui::same_line();
    if styled::icon_button(icons::REFRESH_CW, "reroll", "Another seed") {
        s.seed = (s.seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407) >> 33) % 100_000;
    }

    if !advanced {
        return;
    }

    gui::spacing();
    gui::separator_text("Rain");
    slider(
        "rain",
        "Drops",
        "Rain drops per 100 m² of ground: more wears deeper channels (and takes longer).",
        &mut s.rain,
        (0.0, 30.0),
        "%.1f / 100 m²",
    );
    slider_int("steps", "Run", "The most steps a drop runs (a step is one height sample).", &mut s.steps, (4, 200));
    slider(
        "inertia",
        "Inertia",
        "How much a drop keeps its way: low follows every dip, high cuts straighter.",
        &mut s.inertia,
        (0.0, 0.9),
        "%.2f",
    );
    slider(
        "capacity",
        "Load",
        "How much a drop carries (metres of ground) on a 45° slope: more digs deeper.",
        &mut s.capacity,
        (0.0, 0.5),
        "%.3f m",
    );
    slider("erode", "Erode", "How fast a drop takes ground it could carry.", &mut s.erode, (0.0, 1.0), "%.2f");
    slider("deposit", "Deposit", "How fast a drop lays what it carries past its load.", &mut s.deposit, (0.0, 1.0), "%.2f");
    slider(
        "evaporate",
        "Evaporate",
        "How much of a drop's water goes a step: more gives shorter channels.",
        &mut s.evaporate,
        (0.0, 0.2),
        "%.3f",
    );
    slider(
        "radius",
        "Width",
        "How far round its path a drop wears the ground: wider gives smoother channels.",
        &mut s.radius,
        (0.5, 8.0),
        "%.1f m",
    );

    gui::spacing();
    gui::separator_text("Loose ground");
    slider("talus", "Stands up to", "The steepest loose ground stands; steeper slides down.", &mut s.talus, (15.0, 80.0), "%.0f°");
    slider_int(
        "thermal",
        "Passes",
        "How many times loose ground slides: more flattens steep faces further.",
        &mut s.thermal_passes,
        (0, 30),
    );

    gui::spacing();
    gui::separator_text("Fine detail");
    slider("detail", "Height", "Fine bumps' height on steep ground (a third of it on flat ground).", &mut s.detail, (0.0, 2.0), "%.2f m");
    slider("detail_scale", "Size", "From bump to bump.", &mut s.detail_scale, (1.0, 40.0), "%.1f m");
}
