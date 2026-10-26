//! Tile > New tile...: TEd's New Tile dialog (name, size, terrain height, density; `wce_tile::new_tile`), written as a project in the
//! projects folder and opened.

use wce_editor::editor::Editor;
use wce_editor::{gui, theme};
use wce_tile::new_tile::{self, NewTile};

use crate::tool::TileTool;

/// What the dialog holds while it is open.
#[derive(Clone, Debug)]
pub(crate) struct NewTileDialog {
    name: String,
    cells: [i32; 2],
    height: f32,

    /// Index in `new_tile::DENSITIES`, and whether the user picked it (else it follows the size, as TEd's does).
    density: i32,
    density_chosen: bool,
    error: String,
}

impl Default for NewTileDialog {
    fn default() -> Self {
        let cells = new_tile::DEFAULT_CELLS as i32;
        Self { name: String::new(), cells: [cells; 2], height: new_tile::DEFAULT_HEIGHT as f32, density: density_index(new_tile::DEFAULT_DENSITY), density_chosen: false, error: String::new() }
    }
}

fn density_index(density: u32) -> i32 {
    new_tile::DENSITIES.iter().position(|&d| d == density).unwrap_or(0) as i32
}

impl NewTileDialog {
    fn tile(&self) -> NewTile {
        NewTile {
            name: self.name.trim().to_lowercase(),
            cells_x: self.cells[0].max(0) as u32,
            cells_y: self.cells[1].max(0) as u32,
            height: f64::from(self.height),
            density: new_tile::DENSITIES[self.density as usize],
            seed: new_tile::clock_seed(),
        }
    }
}

impl TileTool {
    /// Opens the dialog on the next frame.
    pub fn open_new_tile_dialog(&mut self) {
        self.new_tile = Some(NewTileDialog::default());
        self.new_tile_open = true;
    }

    pub(crate) fn draw_new_tile_dialog(&mut self, e: &mut Editor) {
        const TITLE: &str = "New tile";
        if std::mem::take(&mut self.new_tile_open) {
            gui::open_popup(TITLE);
        }

        if !gui::begin_popup_modal(TITLE, None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            return;
        }

        let Some(mut d) = self.new_tile.take() else {
            gui::close_current_popup();
            gui::end_popup();
            return;
        };

        gui::text_wrapped("A battle tile in the projects folder (tile set assembly_kit, variation battlefield), flat at the terrain height.");
        gui::set_next_item_width(320.0);
        if gui::is_window_appearing() {
            gui::set_keyboard_focus_here(0);
        }

        let mut create = gui::input_text("Name", Some("no spaces"), &mut d.name, 128, gui::INPUT_ENTER_RETURNS_TRUE);
        let sizes = (new_tile::MIN_CELLS..=new_tile::MAX_CELLS).map(|n| n.to_string()).collect::<Vec<_>>();
        let sizes = sizes.iter().map(String::as_str).collect::<Vec<_>>();
        let mut size_changed = false;
        for (axis, label) in [(0, "Size X (cells)"), (1, "Size Y (cells)")] {
            let mut i = d.cells[axis] - new_tile::MIN_CELLS as i32;
            gui::set_next_item_width(120.0);
            if gui::combo(label, &mut i, &sizes) {
                d.cells[axis] = i + new_tile::MIN_CELLS as i32;
                size_changed = true;
            }
        }

        gui::set_next_item_width(120.0);
        gui::drag_float("Terrain height", &mut d.height, 0.5, -10_000.0, 10_000.0, "%.2f");
        let names = new_tile::DENSITIES.map(|n| n.to_string());
        let names = names.iter().map(String::as_str).collect::<Vec<_>>();
        gui::set_next_item_width(120.0);
        if gui::combo("Terrain density", &mut d.density, &names) {
            d.density_chosen = true;
        }

        // As TEd's: a large tile gets the low density, unless one was chosen.
        if size_changed {
            let t = d.tile();
            d.density = density_index(new_tile::suggested_density(t.cells_x, t.cells_y, t.density, d.density_chosen));
        }

        let t = d.tile();
        let (w, h) = t.map_size();
        gui::text_disabled(&format!("{} x {} m, maps of {w} x {h} pixels, blend_map.tif {}", t.cells_x * 128, t.cells_y * 128, size_text(t.blend_map_bytes())));
        if !d.error.is_empty() {
            gui::text_colored(theme::warning(), &d.error);
        }

        gui::separator();
        create |= gui::button("Create");
        gui::same_line();
        let cancel = gui::button("Cancel") || gui::is_key_pressed(gui::KEY_ESCAPE, true);
        if create {
            match self.create_tile(e, &t) {
                Ok(msg) => {
                    e.status(&msg, false);
                    gui::close_current_popup();
                    gui::end_popup();
                    return;
                }

                Err(err) => d.error = err,
            }
        }

        if cancel {
            gui::close_current_popup();
        } else {
            self.new_tile = Some(d);
        }

        gui::end_popup();
    }

    /// Writes a new tile into the projects (`wce_tile::new_tile::create`) and opens it.
    ///
    /// # Errors
    ///
    /// The values are out of the dialog's limits, the tile exists, or it can't be written or opened.
    pub fn create_tile(&mut self, e: &mut Editor, t: &NewTile) -> Result<String, String> {
        let projects = Self::workspace_dir(e).join("raw_data");
        let kit = self.kit_for(e, "battle").and_then(|raw| raw.parent().map(std::path::Path::to_path_buf));
        let dir = new_tile::create(t, &projects, kit.as_deref())?;
        self.open_tile(e, &dir.display().to_string(), "")?;
        Ok(format!("new tile {} ({} x {} cells, {} m, density {}) in {}", t.name, t.cells_x, t.cells_y, t.height, t.density, dir.display()))
    }
}

fn size_text(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / f64::from(1u32 << 30)),
        b => format!("{:.0} MB", (b as f64 / f64::from(1u32 << 20)).max(1.0)),
    }
}
