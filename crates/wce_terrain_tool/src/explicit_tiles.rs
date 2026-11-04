//! A kit map's explicit tiles (`explicit_tiles.txt`: tiles placed by hand, which BOB's fill places first, as they are: it tests no fit;
//! docs/re/battle_maps.md): listed in the Terrain panel, placed with a click in the Map panel or on the 3D view's ground (a tile or a
//! variation of the tile database, a quarter turn), turned and removed. The fit shown while placing is the map's edge and the other explicit
//! tiles (an overlap: the later one's cells win in the fill). Every edit is one step on the map's history and fills the map again.

use std::sync::Arc;

use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_scene::render_scene::DocumentId;
use wce_terrain::explicit::{ExplicitTile, ExplicitTiles};
use wce_terrain_editor::MapDocument;
use wce_terrain_editor::cells::ExplicitCommand;
use wce_tile::battle_map::tile_db::TileDatabase;

const ROTATIONS: [&str; 4] = ["0", "90", "180", "270"];

/// The most entries the tile list shows (the filter narrows it).
const LIST_MAX: usize = 400;

/// A tile's size in cells from its path in `explicit_tiles.txt` (a tile's location, or a variation's: `<tile>\<variation>`).
pub fn tile_size(db: &TileDatabase, path: &str) -> Option<(i32, i32)> {
    let p = path.replace('/', "\\");
    db.tiles.iter().find(|t| t.location.eq_ignore_ascii_case(&p) || t.variations.iter().any(|v| format!("{}\\{}", t.location, v.location).eq_ignore_ascii_case(&p)))
        .map(|t| (t.width, t.height))
}

/// The cells an explicit tile covers (x0, y0, x1, y1; end exclusive): its size turned by its rotation.
pub fn rect(t: &ExplicitTile, size: (i32, i32)) -> [i64; 4] {
    let (w, h) = if matches!(t.rotation, 90 | 270) { (size.1, size.0) } else { size };
    [i64::from(t.x), i64::from(t.y), i64::from(t.x + w), i64::from(t.y + h)]
}

fn overlap(a: [i64; 4], b: [i64; 4]) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

#[derive(Default)]
pub struct ExplicitPanel {
    /// A click places the chosen tile.
    pub placing: bool,
    choice: String,
    rotation: i32,
    filter: String,
    selected: Option<usize>,

    /// The tile under the cursor while placing: its cells and whether it fits.
    pub preview: Option<([i64; 4], bool)>,

    /// The list of tiles and variations to choose from (per tile database).
    entries: Option<(usize, Vec<String>)>,
}

impl ExplicitPanel {
    /// Where a tile at cell `at` would go and whether it fits (inside the map, on no other explicit tile).
    pub fn fits(&self, d: &MapDocument, at: (i64, i64)) -> Option<([i64; 4], bool)> {
        let size = tile_size(&d.db, &self.choice)?;
        let t = ExplicitTile { x: at.0 as i32, y: at.1 as i32, path: self.choice.clone(), rotation: self.rotation * 90 };
        let r = rect(&t, size);
        let inside = r[0] >= 0 && r[1] >= 0 && r[2] <= i64::from(d.placed.width) && r[3] <= i64::from(d.placed.height);
        let free = d.model().and_then(|m| m.explicit.as_ref()).is_none_or(|x| {
            x.get().tiles().all(|o| tile_size(&d.db, &o.path).is_none_or(|s| !overlap(rect(o, s), r)))
        });

        Some((r, inside && free))
    }

    /// The explicit tiles' cells for the Map panel to outline (the selected one brighter), and the preview.
    pub fn marks(&self, d: &MapDocument) -> Vec<([i64; 4], [u8; 4])> {
        let mut out: Vec<([i64; 4], [u8; 4])> = d.model().and_then(|m| m.explicit.as_ref()).map(|x| {
            x.get().tiles().enumerate().filter_map(|(i, t)| {
                let s = tile_size(&d.db, &t.path)?;
                Some((rect(t, s), if Some(i) == self.selected { [255, 255, 255, 255] } else { [80, 200, 255, 220] }))
            }).collect()
        }).unwrap_or_default();

        if let Some((r, ok)) = self.preview.filter(|_| self.placing) {
            out.push((r, if ok { [60, 230, 90, 255] } else { [240, 60, 50, 255] }));
        }

        out
    }

    /// An edit of the document's explicit tiles onto its history (a missing file is made).
    fn edit(e: &mut Editor, doc: DocumentId, name: &str, f: impl FnOnce(&mut ExplicitTiles) -> bool) -> bool {
        let Some(d) = e.docs.find_as_mut::<MapDocument>(doc) else { return false };
        let Some(m) = d.model() else { return false };
        let before = m.explicit.as_ref().map(|x| x.get().clone());
        let mut after = before.clone().unwrap_or_default();
        if !f(&mut after) {
            return false;
        }

        let watch = d.cells.clone();
        d.push_explicit(ExplicitCommand { name: name.to_owned(), before, after: Some(after), watch });
        true
    }

    /// The left button on cell `at` while placing: the tile added when it fits. What happened.
    pub fn place(&mut self, e: &mut Editor, doc: DocumentId, at: (i64, i64)) -> Result<String, String> {
        let d = e.docs.find_as::<MapDocument>(doc).ok_or("no map shown")?;
        if d.model().is_none() {
            return Err("a map of the game's packs has no explicit tiles to edit".to_owned());
        }

        let (_, ok) = self.fits(d, at).ok_or("choose a tile or variation to place")?;
        if !ok {
            return Err(format!("{} does not fit at {}, {} (the map's edge or another explicit tile)", self.choice, at.0, at.1));
        }

        let t = ExplicitTile { x: at.0 as i32, y: at.1 as i32, path: self.choice.clone(), rotation: self.rotation * 90 };
        let said = format!("explicit tile {} at {}, {} turned {}", t.path, t.x, t.y, t.rotation);
        Self::edit(e, doc, "place an explicit tile", |x| {
            x.push(t);
            true
        });

        Ok(said)
    }

    pub fn draw_section(&mut self, e: &mut Editor, doc: DocumentId) {
        if !gui::collapsing_header("Explicit tiles", 0) {
            return;
        }

        let Some(d) = e.docs.find_as::<MapDocument>(doc) else { return };
        let Some(m) = d.model() else {
            gui::text_disabled("Open a kit map (not one of the game's packs) to edit its explicit tiles.");
            self.placing = false;
            return;
        };

        if m.built {
            gui::text_wrapped("A map of a game without kit maps (Rome 2) is edited in its built form: its tiles in the Placements section, its LF with the LF brush.");
            self.placing = false;
            return;
        }

        // The list, a row each: select, turn, remove.
        let tiles: Vec<ExplicitTile> = m.explicit.as_ref().map(|x| x.get().tiles().cloned().collect()).unwrap_or_default();
        if tiles.is_empty() {
            gui::text_disabled("No explicit tiles.");
        }

        let mut action: Option<(usize, bool)> = None;
        for (i, t) in tiles.iter().enumerate() {
            gui::push_id_int(i as i32);
            if gui::selectable(&format!("{}, {}  {}  {}", t.x, t.y, t.path, t.rotation), self.selected == Some(i), 0) {
                self.selected = Some(i);
            }

            gui::pop_id();
        }

        if let Some(i) = self.selected.filter(|&i| i < tiles.len()) {
            if gui::button("turn 90") {
                action = Some((i, false));
            }

            gui::same_line();
            if gui::button("remove") {
                action = Some((i, true));
            }
        }

        match action {
            Some((i, true)) => {
                Self::edit(e, doc, "remove an explicit tile", |x| x.remove(i));
                self.selected = None;
            }

            Some((i, false)) => {
                let mut t = tiles[i].clone();
                t.rotation = (t.rotation + 90) % 360;
                Self::edit(e, doc, "turn an explicit tile", |x| x.set(i, t));
            }

            None => {}
        }

        gui::separator();
        gui::checkbox("place (left button, Map panel or 3D view)", &mut self.placing);
        gui::combo("rotation##explicit", &mut self.rotation, &ROTATIONS);
        let Some(d) = e.docs.find_as::<MapDocument>(doc) else { return };
        let key = Arc::as_ptr(&d.db) as usize;
        if self.entries.as_ref().is_none_or(|(k, _)| *k != key) {
            let mut v = vec![];
            for t in &d.db.tiles {
                v.push(t.location.clone());
                v.extend(t.variations.iter().map(|x| format!("{}\\{}", t.location, x.location)));
            }

            self.entries = Some((key, v));
        }

        gui::text_wrapped(if self.choice.is_empty() { "choose a tile (a variation at random) or a variation" } else { &self.choice });
        gui::input_text("filter##explicit", Some("tile or variation"), &mut self.filter, 128, 0);
        let needle = self.filter.to_lowercase();
        let entries = &self.entries.as_ref().expect("made above").1;
        let found: Vec<&String> = entries.iter().filter(|n| needle.is_empty() || n.to_lowercase().contains(&needle)).collect();
        if gui::begin_child("explicit tiles", gui::v2(0.0, 200.0), 1, 0) {
            for (i, n) in found.iter().take(LIST_MAX).enumerate() {
                gui::push_id_int(i as i32);
                if gui::selectable(n, **n == self.choice, 0) {
                    self.choice = (*n).clone();
                }

                gui::pop_id();
            }

            if found.len() > LIST_MAX {
                gui::text_disabled(&format!("{} more: narrow the filter", found.len() - LIST_MAX));
            }
        }

        gui::end_child();
    }

    /// For scripts: the tile to place and its rotation (degrees).
    pub fn choose(&mut self, path: &str, degrees: i32) {
        self.choice = path.to_owned();
        self.rotation = (degrees / 90).rem_euclid(4);
    }

    /// For scripts: removes the `i`-th explicit tile.
    pub fn remove(e: &mut Editor, doc: DocumentId, i: usize) -> bool {
        Self::edit(e, doc, "remove an explicit tile", |x| x.remove(i))
    }
}
