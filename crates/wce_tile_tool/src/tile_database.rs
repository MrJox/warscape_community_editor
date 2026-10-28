//! Tile > Tile database... (Phase 6b of docs/plans/tile_ted_parity.md; TEd's Tile Database Management): the battle tile sets of the
//! Assembly Kit and of the projects, each with its colour and its tiles (a click opens one), and New tile set (TEd's: a name and a colour)
//! into the projects' `_settings.xml`; "Save for Assembly Kit" carries a set a tile uses into the kit's. A new tile goes into a set with
//! Tile > Save tile as....

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use wce_editor::editor::Editor;
use wce_editor::{gui, icons};
use wce_formats::tile_entry::TileEntry;
use wce_tile::tile_sets::{TileSetInfo, add, free_colour, read, settings_file};

use crate::tool::TileTool;

/// A tile of the list: where its entry is, its location, whether it is the projects'.
#[derive(Clone, Debug)]
pub(crate) struct DbTile {
    pub entry: PathBuf,
    pub location: String,
    pub size: (u32, u32),
    pub ours: bool,
}

/// The window's state.
#[derive(Clone, Debug, Default)]
pub(crate) struct DbUi {
    pub open: bool,

    /// The sets (the projects' second: theirs where both have one) and whether each is the projects' only, and the tiles by set.
    pub sets: Vec<(TileSetInfo, bool)>,
    pub tiles: BTreeMap<String, Vec<DbTile>>,
    pub filter: String,
    pub new_name: String,
    pub new_colour: Option<[f32; 3]>,
    pub status: String,
}

/// The entries of a `TILES` folder by tile set (`ours`: the projects').
fn tiles_of(raw: &Path, ours: bool, out: &mut BTreeMap<String, Vec<DbTile>>) {
    let Ok(dir) = std::fs::read_dir(raw.join("terrain/tiles/battle/_tile_database/TILES")) else { return };
    for f in dir.flatten().map(|f| f.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("xml"))) {
        let Ok(e) = TileEntry::load(&f) else { continue };
        let list = out.entry(e.tile_set()).or_default();
        // A project's copy of a kit tile stands for it.
        let location = e.location();
        list.retain(|t| !(ours && t.location.eq_ignore_ascii_case(&location)));
        list.push(DbTile { entry: f, location, size: (e.width(), e.height()), ours });
    }
}

impl TileTool {
    /// Reads the kit's and the projects' sets and tiles again.
    pub(crate) fn refresh_tile_database(&mut self, e: &mut Editor) {
        let projects = Self::workspace_dir(e).join("raw_data");
        let kit = self.kit_for(e, "battle");
        let ui = self.tile_db.get_or_insert_with(DbUi::default);
        ui.sets.clear();
        ui.tiles.clear();
        let theirs = kit.as_deref().map(|k| read(&settings_file(k))).unwrap_or_default();
        let ours = read(&settings_file(&projects));
        ui.sets = theirs.iter().cloned().map(|s| (s, false)).collect();
        ui.sets.extend(ours.into_iter().filter(|s| !theirs.iter().any(|t| t.name == s.name)).map(|s| (s, true)));
        ui.sets.sort_by(|a, b| a.0.name.cmp(&b.0.name));
        if let Some(k) = &kit {
            tiles_of(k, false, &mut ui.tiles);
        }

        tiles_of(&projects, true, &mut ui.tiles);
    }

    pub(crate) fn open_tile_database(&mut self, e: &mut Editor) {
        self.refresh_tile_database(e);
        if let Some(ui) = self.tile_db.as_mut() {
            ui.open = true;
        }
    }

    /// Adds a tile set to the projects' `_settings.xml` (copied from the kit's first when the projects have none).
    pub(crate) fn new_tile_set(&mut self, e: &mut Editor, name: &str, colour: Option<[u8; 3]>) -> Result<String, String> {
        let name = wce_tile::new_tile::check_name(name)?;
        let projects = Self::workspace_dir(e).join("raw_data");
        let ours = settings_file(&projects);
        if !ours.is_file() {
            let kit = self.kit_for(e, "battle").map(|k| settings_file(&k)).filter(|k| k.is_file()).ok_or("no tile database settings to start from (set the Assembly Kit in the settings)")?;
            if let Some(dir) = ours.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }

            std::fs::copy(&kit, &ours).map_err(|e| format!("{}: {e}", kit.display()))?;
        }

        self.refresh_tile_database(e);
        let all = self.tile_db.as_ref().map(|u| u.sets.iter().map(|s| s.0.clone()).collect::<Vec<_>>()).unwrap_or_default();
        if all.iter().any(|s| s.name == name) {
            return Err(format!("there is a tile set '{name}' already"));
        }

        let c = colour.unwrap_or_else(|| free_colour(&name, &all));
        if all.iter().any(|s| s.colour.map(|v| v.round() as u8) == c) {
            return Err(format!("the colour {} {} {} is another set's: a map picture tells the sets apart by it", c[0], c[1], c[2]));
        }

        add(&ours, &TileSetInfo { name: name.clone(), colour: c.map(f64::from), ..Default::default() })?;
        self.refresh_tile_database(e);
        Ok(format!("tile set '{name}' added to the projects' tile database (colour {} {} {})", c[0], c[1], c[2]))
    }

    pub(crate) fn draw_tile_database(&mut self, e: &mut Editor) {
        if !self.tile_db.as_ref().is_some_and(|u| u.open) {
            return;
        }

        let mut ui = self.tile_db.take().unwrap_or_default();
        gui::set_next_window_size(gui::v2(520.0, 560.0), gui::COND_FIRST_USE_EVER);
        let mut open = true;
        if gui::begin("Tile database", Some(&mut open), gui::WINDOW_NO_DOCKING) {
            gui::text_wrapped("The battle tile sets of the Assembly Kit and of the projects, with their tiles (a click opens one). A new set goes into the projects' tile database; Save for Assembly Kit takes it into the kit's when a tile there uses it. Tile > Save tile as... puts a tile into a set.");
            gui::separator();
            gui::set_next_item_width(160.0);
            gui::input_text("##newset", Some("new set's name"), &mut ui.new_name, 64, 0);
            gui::same_line();
            let mut c = ui.new_colour.unwrap_or([0.5; 3]);
            if gui::color_edit3("##newsetcolour", &mut c, gui::COLOR_NO_INPUTS) {
                ui.new_colour = Some(c);
            }

            if gui::is_item_hovered() {
                gui::set_tooltip("Its colour in the battle maps' pictures (a map paints a cell with a set's colour to ask for a tile of it): one no other set has. Left alone, one is picked.");
            }

            gui::same_line();
            if gui::small_button(&format!("{}  New tile set", icons::PLUS)) {
                let name = ui.new_name.clone();
                let colour = ui.new_colour.map(|c| c.map(|v| (v * 255.0).round() as u8));
                self.tile_db = Some(ui.clone());
                ui.status = match self.new_tile_set(e, &name, colour) {
                    Ok(m) => {
                        ui.new_name.clear();
                        ui.new_colour = None;
                        m
                    }

                    Err(err) => err,
                };

                let fresh = self.tile_db.take().unwrap_or_default();
                (ui.sets, ui.tiles) = (fresh.sets, fresh.tiles);
            }

            if !ui.status.is_empty() {
                gui::text_disabled(&ui.status);
            }

            gui::separator();
            gui::set_next_item_width(-1.0);
            gui::input_text("##dbfilter", Some(&format!("{}  filter sets and tiles", icons::SEARCH)), &mut ui.filter, 64, 0);
            let f = ui.filter.to_lowercase();
            let mut open_entry = None;
            if gui::begin_child("dbsets", gui::v2(0.0, 0.0), 0, 0) {
                for (s, ours) in &ui.sets {
                    let tiles = ui.tiles.get(&s.name).cloned().unwrap_or_default();
                    let hit = |t: &DbTile| t.location.to_lowercase().contains(&f);
                    if !f.is_empty() && !s.name.contains(&f) && !tiles.iter().any(hit) {
                        continue;
                    }

                    let col = s.colour.map(|v| (v / 255.0) as f32);
                    gui::color_swatch(&format!("##sw{}", s.name), [col[0], col[1], col[2], 1.0], gui::v2(12.0, 12.0));
                    gui::same_line();
                    let label = format!("{}  ({} tile(s)){}###set{}", s.name, tiles.len(), if *ours { "  [projects]" } else { "" }, s.name);
                    if gui::tree_node(&label) {
                        if !s.link_as_set.is_empty() || !s.linking_tile.is_empty() {
                            gui::text_disabled(&format!("links as {}; linking tile {}", if s.link_as_set.is_empty() { &s.name } else { &s.link_as_set }, s.linking_tile));
                        }

                        for t in tiles.iter().filter(|t| f.is_empty() || s.name.contains(&f) || hit(t)) {
                            let name = t.location.rsplit(['\\', '/']).next().unwrap_or_default();
                            if gui::selectable(&format!("{name}  {} x {}{}##{}", t.size.0, t.size.1, if t.ours { "  [projects]" } else { "" }, t.entry.display()), false, 0) {
                                open_entry = Some(t.entry.clone());
                            }
                        }

                        gui::tree_pop();
                    }
                }
            }

            gui::end_child();
            if let Some(p) = open_entry {
                let msg = self.open_tile(e, &p.display().to_string(), "").unwrap_or_else(|m| format!("open failed: {m}"));
                e.status(&msg, false);
            }
        }

        gui::end();
        ui.open &= open;
        if self.tile_db.is_none() {
            self.tile_db = Some(ui);
        }
    }
}
