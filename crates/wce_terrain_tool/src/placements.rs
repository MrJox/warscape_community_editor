//! A built map's placements (a game without kit maps: Rome 2's pack maps; docs/plans/terrain_editor.md, Phase 9): its battle field
//! (`terrain.tile_list`) edited as it is, there being no tile map to fill from. Select the placement under a click (its tile, variation and
//! turn shown and changed), turn it, remove it, move it (a click: its new origin cell), or place a tile of the database (its variation,
//! a quarter turn). A placement must lie inside the map and on no other. Every edit is one step on the map's history; the placed map is
//! made again from the list (`refill`), the battle field's bounds recomputed (Rome 2's: each placement's origin -/+ its tile's larger side).
//!
//! Rome 2's lists (versions 7 to 9) name a placement's tile alone, its variation the placement's byte 12; a new placement copies the
//! list's most common flags (their high byte and render bits, unknown) and unknown bytes, with its own turn.

use std::sync::Arc;

use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_scene::render_scene::DocumentId;
use wce_terrain_editor::MapDocument;
use wce_terrain_editor::cells::TileListCommand;
use wce_tile::battle_map::tile_db::TileDatabase;
use wce_tile::battle_map::tile_list::{TileList, TileListPlacement};

const ROTATIONS: [u16; 4] = [0x10, 0x20, 0x40, 0x80];
const LIST_MAX: usize = 400;

/// What a left click does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Off,
    Select,
    Place,
    Move,
}

#[derive(Default)]
pub struct PlacementsPanel {
    pub mode: Mode,

    /// The tile to place (its location) and its variation and turn (0 .. 3).
    choice: String,
    variation: i32,
    rotation: i32,
    filter: String,
    pub selected: Option<usize>,

    /// Under the cursor while placing or moving: the cells and whether they fit.
    pub preview: Option<([i64; 4], bool)>,
    entries: Option<(usize, Vec<String>)>,
}

/// A tile's index in the database by its location.
fn tile_of(db: &TileDatabase, location: &str) -> Option<usize> {
    let l = location.replace('/', "\\");
    db.tiles.iter().position(|t| t.location.eq_ignore_ascii_case(&l))
}

/// A placement's cells (x0, y0, x1, y1; end exclusive): its tile's size turned.
fn rect(list: &TileList, db: &TileDatabase, p: &TileListPlacement) -> [i64; 4] {
    let (w, h) = list.names.get(p.name).and_then(|n| tile_of(db, n)).map_or((1, 1), |t| (db.tiles[t].width, db.tiles[t].height));
    let (w, h) = if matches!(p.flags & 0xf0, 0x20 | 0x80) { (h, w) } else { (w, h) };
    let (x, y) = (i64::from(p.x), i64::from(p.y));
    [x, y, x + i64::from(w), y + i64::from(h)]
}

fn overlap(a: [i64; 4], b: [i64; 4]) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

/// Whether cells `r` lie inside the map and on no placement but `except`.
fn fits(list: &TileList, db: &TileDatabase, r: [i64; 4], except: Option<usize>) -> bool {
    let inside = r[0] >= 0 && r[1] >= 0 && r[2] <= i64::from(list.battle_size[0]) && r[3] <= i64::from(list.battle_size[1]);
    inside && list.placements.iter().enumerate().all(|(i, p)| Some(i) == except || !overlap(rect(list, db, p), r))
}

/// The battle field's bounds again: each placement's origin -/+ its tile's larger side.
fn rebound(list: &mut TileList, db: &TileDatabase) {
    let mut b = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
    for p in &list.placements {
        let size = list.names.get(p.name).and_then(|n| tile_of(db, n)).map_or(1, |t| db.tiles[t].width.max(db.tiles[t].height));
        let (x, y) = (i32::from(p.x), i32::from(p.y));
        b = [b[0].min(x - size), b[1].min(y - size), b[2].max(x + size), b[3].max(y + size)];
    }

    if !list.placements.is_empty() {
        list.bounds = b;
    }
}

impl PlacementsPanel {
    pub fn claims(&self) -> bool {
        self.mode != Mode::Off
    }

    fn list(d: &MapDocument) -> Option<&TileList> {
        d.model().filter(|m| m.built).and_then(|m| m.tile_list.as_ref()).map(|t| t.get())
    }

    /// The cells a click at `at` would fill (placing: the chosen tile; moving: the selected one) and whether they fit.
    pub fn hover(&mut self, d: &MapDocument, at: (i64, i64)) {
        self.preview = Self::list(d).and_then(|l| {
            let (r, except) = match self.mode {
                Mode::Place => {
                    let t = tile_of(&d.db, &self.choice)?;
                    let (w, h) = (i64::from(d.db.tiles[t].width), i64::from(d.db.tiles[t].height));
                    let (w, h) = if self.rotation % 2 == 1 { (h, w) } else { (w, h) };
                    ([at.0, at.1, at.0 + w, at.1 + h], None)
                }

                Mode::Move => {
                    let i = self.selected.filter(|&i| i < l.placements.len())?;
                    let r = rect(l, &d.db, &l.placements[i]);
                    ([at.0, at.1, at.0 + r[2] - r[0], at.1 + r[3] - r[1]], Some(i))
                }

                _ => return None,
            };

            Some((r, fits(l, &d.db, r, except)))
        });
    }

    /// The Map panel's outlines: the selected placement, the preview.
    pub fn marks(&self, d: &MapDocument) -> Vec<([i64; 4], [u8; 4])> {
        let mut out = vec![];
        if let Some(l) = Self::list(d)
            && let Some(p) = self.selected.and_then(|i| l.placements.get(i))
        {
            out.push((rect(l, &d.db, p), [255, 255, 255, 255]));
        }

        if let Some((r, ok)) = self.preview.filter(|_| matches!(self.mode, Mode::Place | Mode::Move)) {
            out.push((r, if ok { [60, 230, 90, 255] } else { [240, 60, 50, 255] }));
        }

        out
    }

    /// An edit of the document's battle field onto its history: `f` changes a copy (an error leaves it), the bounds made again.
    fn edit(e: &mut Editor, doc: DocumentId, name: &str, changes_count: bool, f: impl FnOnce(&mut TileList, &TileDatabase) -> Result<String, String>) -> Result<String, String> {
        let d = e.docs.find_as_mut::<MapDocument>(doc).ok_or("no map shown")?;
        Self::edit_document(d, name, changes_count, f)
    }

    fn edit_document(
        d: &mut MapDocument,
        name: &str,
        changes_count: bool,
        f: impl FnOnce(&mut TileList, &TileDatabase) -> Result<String, String>,
    ) -> Result<String, String> {
        let before = Self::list(d)
            .ok_or("a built map with a terrain.tile_list (Rome 2's pack maps) has placements to edit")?
            .clone();
        if before.version >= 11 {
            return Err("placements are edited on Rome 2's tile lists (versions 7 to 9)".to_owned());
        }

        before.check_placement_edit(name, changes_count)?;

        let db = d.db.clone();
        let mut after = before.clone();
        let said = f(&mut after, &db)?;
        after.validate_placement_edit(&before)?;
        rebound(&mut after, &db);
        let watch = d.cells.clone();
        d.push_tile_list(TileListCommand {
            name: name.to_owned(),
            before,
            after,
            watch,
        });
        Ok(said)
    }

    /// A left click on cell `at`. What happened.
    pub fn click(&mut self, e: &mut Editor, doc: DocumentId, at: (i64, i64)) -> Result<String, String> {
        match self.mode {
            Mode::Off => Err(String::new()),
            Mode::Select => {
                let d = e.docs.find_as::<MapDocument>(doc).ok_or("no map shown")?;
                let l = Self::list(d).ok_or("no placements to edit")?;
                let cell = [at.0, at.1, at.0 + 1, at.1 + 1];
                self.selected = l.placements.iter().rposition(|p| overlap(rect(l, &d.db, p), cell));
                Ok(match self.selected {
                    Some(i) => format!("placement {i}: {}", describe(l, &d.db, i)),
                    None => format!("no placement at {}, {}", at.0, at.1),
                })
            }

            Mode::Move => {
                let i = self.selected.ok_or("select a placement first")?;
                Self::edit(e, doc, "move a placement", false, |l, db| {
                    let p = l.placements.get(i).cloned().ok_or("no such placement")?;
                    let r = rect(l, db, &p);
                    let to = [at.0, at.1, at.0 + r[2] - r[0], at.1 + r[3] - r[1]];
                    if !fits(l, db, to, Some(i)) {
                        return Err(format!("it does not fit at {}, {} (the map's edge or another placement)", at.0, at.1));
                    }

                    (l.placements[i].x, l.placements[i].y) = (at.0 as i16, at.1 as i16);
                    Ok(format!("placement {i} moved to {}, {}", at.0, at.1))
                })
            }

            Mode::Place => {
                let (choice, variation, rotation) = (self.choice.clone(), self.variation, self.rotation);
                let r = Self::edit(e, doc, "place a tile", true, |l, db| place(l, db, &choice, variation, rotation, at));
                if r.is_ok() {
                    self.selected = e.docs.find_as::<MapDocument>(doc).and_then(Self::list).map(|l| l.placements.len() - 1);
                }

                r
            }
        }
    }

    /// For scripts: the tile to place, its variation and turn (degrees).
    pub fn choose(&mut self, location: &str, variation: i32, degrees: i32) {
        (self.choice, self.variation, self.rotation) = (location.to_owned(), variation, (degrees / 90).rem_euclid(4));
    }

    /// The selected placement turned a quarter, removed, or given another variation.
    pub fn turn(e: &mut Editor, doc: DocumentId, i: usize) -> Result<String, String> {
        Self::edit(e, doc, "turn a placement", false, |l, db| {
            let p = l.placements.get(i).cloned().ok_or("no such placement")?;
            let k = ROTATIONS.iter().position(|&r| r == p.flags & 0xf0).unwrap_or(0);
            let mut q = p.clone();
            q.flags = (p.flags & !0xf0) | ROTATIONS[(k + 1) % 4];
            if !fits(l, db, rect(l, db, &q), Some(i)) {
                return Err("turned, it does not fit (the map's edge or another placement)".to_owned());
            }

            l.placements[i] = q;
            Ok(format!("placement {i} turned to {} degrees", (k + 1) % 4 * 90))
        })
    }

    pub fn remove(e: &mut Editor, doc: DocumentId, i: usize) -> Result<String, String> {
        Self::edit(e, doc, "remove a placement", true, |l, _| {
            if i >= l.placements.len() {
                return Err("no such placement".to_owned());
            }

            l.placements.remove(i);
            Ok(format!("placement {i} removed"))
        })
    }

    pub fn set_variation(e: &mut Editor, doc: DocumentId, i: usize, v: u8) -> Result<String, String> {
        Self::edit(e, doc, "change a placement's variation", false, |l, db| {
            let p = l.placements.get(i).ok_or("no such placement")?;
            let t = l.names.get(p.name).and_then(|n| tile_of(db, n)).ok_or("its tile is not in the database")?;
            let name = db.tiles[t].variations.get(usize::from(v)).map(|x| x.location.clone()).ok_or("no such variation")?;
            l.placements[i].climate = v;
            Ok(format!("placement {i}: variation {name}"))
        })
    }

    pub fn draw_section(&mut self, e: &mut Editor, doc: DocumentId) {
        let Some(d) = e.docs.find_as::<MapDocument>(doc) else { return };
        let Some(l) = Self::list(d) else {
            self.mode = Mode::Off;
            return;
        };

        if !gui::collapsing_header("Placements", 0) {
            return;
        }

        gui::text_disabled(&format!("{} placements, {} tiles; the map's battle field edited as it is (no fill)", l.placements.len(), l.names.len()));
        let mut m = self.mode as i32;
        if gui::combo("left button##placements", &mut m, &["off", "select the placement under it", "place the chosen tile", "move the selected placement there"]) {
            self.mode = [Mode::Off, Mode::Select, Mode::Place, Mode::Move][m.clamp(0, 3) as usize];
        }

        let selected = self.selected.filter(|&i| i < l.placements.len());
        let mut action: Option<Result<String, String>> = None;
        if let Some(i) = selected {
            gui::text_wrapped(&format!("selected {i}: {}", describe(l, &d.db, i)));
            let p = &l.placements[i];
            if let Some(t) = l.names.get(p.name).and_then(|n| tile_of(&d.db, n)) {
                let names: Vec<&str> = d.db.tiles[t].variations.iter().map(|v| v.location.as_str()).collect();
                let mut v = i32::from(p.climate);
                if names.len() > 1 && gui::combo("variation##placements", &mut v, &names) {
                    action = Some(Self::set_variation(e, doc, i, v.clamp(0, 255) as u8));
                }
            }

            if action.is_none() {
                if gui::button("turn 90##placements") {
                    action = Some(Self::turn(e, doc, i));
                }

                gui::same_line();
                if gui::button("remove##placements") {
                    action = Some(Self::remove(e, doc, i));
                    if action.as_ref().is_some_and(Result::is_ok) {
                        self.selected = None;
                    }
                }
            }
        }

        if let Some(r) = action {
            match r {
                Ok(m) => e.status(&m, false),
                Err(err) => e.status(&err, true),
            }
        }

        gui::separator();
        let Some(d) = e.docs.find_as::<MapDocument>(doc) else { return };
        let key = Arc::as_ptr(&d.db) as usize;
        if self.entries.as_ref().is_none_or(|(k, _)| *k != key) {
            self.entries = Some((key, d.db.tiles.iter().map(|t| t.location.clone()).collect()));
        }

        gui::text_wrapped(if self.choice.is_empty() { "choose a tile to place" } else { &self.choice });
        if let Some(t) = tile_of(&d.db, &self.choice) {
            let names: Vec<&str> = d.db.tiles[t].variations.iter().map(|v| v.location.as_str()).collect();
            gui::combo("variation to place##placements", &mut self.variation, &names);
        }

        gui::combo("turn##placements", &mut self.rotation, &["0", "90", "180", "270"]);
        gui::input_text("filter##placements", Some("tile"), &mut self.filter, 128, 0);
        let needle = self.filter.to_lowercase();
        let entries = &self.entries.as_ref().expect("made above").1;
        let found: Vec<&String> = entries.iter().filter(|n| needle.is_empty() || n.to_lowercase().contains(&needle)).collect();
        if gui::begin_child("placement tiles", gui::v2(0.0, 180.0), 1, 0) {
            for (i, n) in found.iter().take(LIST_MAX).enumerate() {
                gui::push_id_int(i as i32);
                if gui::selectable(n, **n == self.choice, 0) {
                    self.choice = (*n).clone();
                    self.variation = 0;
                }

                gui::pop_id();
            }

            if found.len() > LIST_MAX {
                gui::text_disabled(&format!("{} more: narrow the filter", found.len() - LIST_MAX));
            }
        }

        gui::end_child();
    }
}

#[cfg(test)]
#[path = "placement_integrity_test.rs"]
mod integrity_tests;

/// A placement in words: its tile, variation, origin and turn.
fn describe(l: &TileList, db: &TileDatabase, i: usize) -> String {
    let p = &l.placements[i];
    let name = l.names.get(p.name).map_or("?", String::as_str);
    let variation = tile_of(db, name).and_then(|t| db.tiles[t].variations.get(usize::from(p.climate))).map_or_else(|| p.climate.to_string(), |v| v.location.clone());
    let turn = ROTATIONS.iter().position(|&r| r == p.flags & 0xf0).unwrap_or(0) * 90;
    format!("{name} ({variation}) at {}, {} turned {turn}", p.x, p.y)
}

/// A new placement of tile `location` (variation `v`, `turn` quarters) at `at`: it must fit; its name added to the list when new; flags
/// and unknown bytes the list's most common.
fn place(l: &mut TileList, db: &TileDatabase, location: &str, v: i32, turn: i32, at: (i64, i64)) -> Result<String, String> {
    let t = tile_of(db, location).ok_or("choose a tile of the database")?;
    let tile = &db.tiles[t];
    let v = usize::try_from(v).ok().filter(|&v| v < tile.variations.len()).unwrap_or(0);
    let (w, h) = (i64::from(tile.width), i64::from(tile.height));
    let (w, h) = if turn % 2 == 1 { (h, w) } else { (w, h) };
    let r = [at.0, at.1, at.0 + w, at.1 + h];
    if !fits(l, db, r, None) {
        return Err(format!("{location} does not fit at {}, {} (the map's edge or another placement)", at.0, at.1));
    }

    // The list's most common flags (without the turn) and unknown bytes.
    let mut counts: std::collections::HashMap<(u16, [u8; 3]), usize> = std::collections::HashMap::new();
    for p in &l.placements {
        *counts.entry((p.flags & !0xf0, p.old)).or_default() += 1;
    }

    let ((flags, old), _) = counts.into_iter().max_by_key(|(k, n)| (*n, std::cmp::Reverse(k.0))).unwrap_or(((0x0700, [0; 3]), 0));
    let name = match l.names.iter().position(|n| n.eq_ignore_ascii_case(&tile.location)) {
        Some(k) => k,
        None => {
            l.names.push(tile.location.clone());
            l.names.len() - 1
        }
    };

    let rotation = ROTATIONS[turn.rem_euclid(4) as usize];
    l.placements.push(TileListPlacement { name, x: at.0 as i16, y: at.1 as i16, flags: flags | rotation, climate: v as u8, lf: [0.0; 2], old });
    Ok(format!("{} ({}) placed at {}, {}", tile.location, tile.variations.get(v).map_or("", |x| x.location.as_str()), at.0, at.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_as_rome2s() {
        // river_test's: 8 x 8 tiles from 0 to 56 give -8 .. 64 (no database: a size of 1 for unknown tiles, so check the formula alone).
        let mut l = TileList { version: 8, battle_size: [64, 64], names: vec!["x".to_owned()], ..Default::default() };
        for x in [0i16, 56] {
            l.placements.push(TileListPlacement { name: 0, x, y: x, flags: 0x0710, ..Default::default() });
        }

        rebound(&mut l, &TileDatabase::default());
        assert_eq!(l.bounds, [-1, -1, 57, 57]);
    }
}
