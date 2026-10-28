//! Which tiles can follow a tile at its links (Phase 6a of docs/plans/tile_ted_parity.md): every tile of the database, at every rotation it
//! may take and every place along the tile's edges where it touches it, kept when the links of the two agree as BOB's fill tests them
//! (`fill.rs`, `test_final_tile_position`; docs/re/battle_maps.md, "The fit") and it carries on one of the tile's joins (a link target of
//! the tile that the other's "must be" rule lands on, with the same link set).
//!
//! The pair is tested alone, on a map that holds just the two: a rule of one that lands on a cell of the other passes or fails against
//! that tile's link marks (`add_tile_to_link_map`: a tile without link targets marks its cells with its set's link set, one with targets
//! marks only its target cells, with the targets' sets); a rule that lands on neither is about another neighbour and is not tested. Cells
//! are the entries' (x along +x, y from the +z edge); the open tile is at (0, 0), unturned.

use super::fill::rotate_in_tile_space;
use super::tile_db::{LinkTest, Tile, TileDatabase};

/// The rotations BOB places a tile with (0, 90, 180, 270 degrees).
pub const ROTATIONS: [u16; 4] = [0x10, 0x20, 0x40, 0x80];

/// A tile that can follow: the database's tile, its rotation, its origin cell (the open tile's cells), and the open tile's link targets
/// (indices) it carries on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub tile: usize,
    pub rotation: u16,
    pub x: i32,
    pub y: i32,
    pub joins: Vec<usize>,
}

/// A tile placed on the little map: its tile, origin and rotation.
struct Placed<'a> {
    tile: &'a Tile,
    x: i32,
    y: i32,
    rotation: u16,
}

impl Placed<'_> {
    /// Its size on the map.
    fn size(&self) -> (i32, i32) {
        if matches!(self.rotation, 0x20 | 0x80) { (self.tile.height, self.tile.width) } else { (self.tile.width, self.tile.height) }
    }

    /// The map cell of a cell of the tile.
    fn at(&self, x: i32, y: i32) -> (i32, i32) {
        let (dx, dy) = rotate_in_tile_space(self.tile.width, self.tile.height, self.rotation, x, y);
        (self.x + dx, self.y + dy)
    }

    /// Whether it covers a map cell (a cell of its mask).
    fn covers(&self, cell: (i32, i32)) -> bool {
        (0..self.tile.height).any(|y| (0..self.tile.width).any(|x| self.tile.subtile_masked_valid(x, y) && self.at(x, y) == cell))
    }

    /// The link sets it marks a map cell with (link names).
    fn marks<'b>(&self, db: &'b TileDatabase, cell: (i32, i32)) -> Vec<&'b str> {
        if self.tile.link_targets.is_empty() {
            return if self.covers(cell) { self.tile.set.map(|s| db.sets[s].link_name()).into_iter().collect() } else { vec![] };
        }

        self.tile.link_targets.iter().filter(|t| self.at(t.x, t.y) == cell).filter_map(|t| t.set.map(|s| db.sets[s].link_name())).collect()
    }
}

/// The link name of a set index.
fn link_name(db: &TileDatabase, set: Option<usize>) -> Option<&str> {
    set.map(|s| db.sets[s].link_name())
}

/// Whether every rule of `a` that lands on `b` passes there.
fn rules_pass(db: &TileDatabase, a: &Placed, b: &Placed) -> bool {
    for l in &a.tile.links {
        let cell = a.at(l.x, l.y);
        if !b.covers(cell) {
            continue;
        }

        let Some(want) = link_name(db, l.set) else { return false };
        match l.test {
            LinkTest::Equals => {
                if !b.marks(db, cell).contains(&want) {
                    return false;
                }
            }

            LinkTest::NotEquals => {
                if link_name(db, b.tile.set) == Some(want) {
                    return false;
                }
            }
        }
    }

    true
}

/// The tiles that can follow `open` (a tile as the database reads it: the open tile's entry, with its edits) at its links, by the database's
/// tile order, rotation and place.
pub fn candidates(db: &TileDatabase, open: &Tile) -> Vec<Candidate> {
    let a = Placed { tile: open, x: 0, y: 0, rotation: 0x10 };
    let (aw, ah) = (open.width, open.height);
    let mut out = vec![];
    for (ti, t) in db.tiles.iter().enumerate() {
        // A tile that asks nothing can't carry a join on (a tile can follow itself).
        if t.links.is_empty() {
            continue;
        }

        let rotations: &[u16] = if t.random_rotatable { &ROTATIONS } else { &ROTATIONS[..1] };
        for &rotation in rotations {
            let probe = Placed { tile: t, x: 0, y: 0, rotation };
            let (bw, bh) = probe.size();

            // Every place where it touches the open tile's box along an edge: west, east, north (y < 0), south.
            let mut spots = vec![];
            for y in (1 - bh)..ah {
                spots.push((-bw, y));
                spots.push((aw, y));
            }

            for x in (1 - bw)..aw {
                spots.push((x, -bh));
                spots.push((x, ah));
            }

            for (x, y) in spots {
                let b = Placed { tile: t, x, y, rotation };
                if !rules_pass(db, &a, &b) || !rules_pass(db, &b, &a) {
                    continue;
                }

                // The open tile's joins it carries on: one of its "must be" rules lands on the target's cell, with the target's link set.
                let joins = open.link_targets.iter().enumerate().filter(|(_, tg)| {
                    let Some(set) = link_name(db, tg.set) else { return false };
                    t.links.iter().any(|l| l.test == LinkTest::Equals && b.at(l.x, l.y) == (tg.x, tg.y) && link_name(db, l.set) == Some(set))
                }).map(|(i, _)| i).collect::<Vec<_>>();

                if !joins.is_empty() {
                    out.push(Candidate { tile: ti, rotation, x, y, joins });
                }
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle_map::tile_db::{Link, LinkTarget, TileSet};

    fn set(name: &str) -> TileSet {
        TileSet { name: name.to_owned(), colour: [0.0; 3], link_as_set: String::new(), linking_tile: String::new(), shared_geometry: String::new(), also_place_tile_set: String::new(), id: 0 }
    }

    fn tile(location: &str, w: i32, h: i32, set: usize, targets: Vec<(usize, i32, i32)>, links: Vec<(usize, i32, i32, LinkTest)>) -> Tile {
        Tile {
            location: location.to_owned(), set: Some(set), width: w, height: h, colour: [0.0; 3], mask: vec![], random_rotatable: true, variations: vec![],
            links: links.into_iter().map(|(s, x, y, test)| Link { set: Some(s), x, y, test, is_entry: false }).collect(),
            link_targets: targets.into_iter().map(|(s, x, y)| LinkTarget { set: Some(s), x, y }).collect(), id: 0,
        }
    }

    #[test]
    fn a_road_carries_on_east_and_turned() {
        // 0 roads, 1 plains. The open tile: a 1 x 1 road going east (target at its cell, rule on the cell east of it).
        let mut db = TileDatabase { sets: vec![set("roads"), set("plains")], ..Default::default() };
        let open = tile("open", 1, 1, 0, vec![(0, 0, 0)], vec![(0, 1, 0, LinkTest::Equals)]);

        // A road going west to east too: it follows east (its rule on the cell west of it lands on the open tile's target). Turned, it also
        // fits west of the open tile (its rule lands on the open tile's road cell; the open tile asks nothing of the west): a target is a cell,
        // not a side, as in the game's test.
        db.tiles.push(tile("road", 1, 1, 0, vec![(0, 0, 0)], vec![(0, -1, 0, LinkTest::Equals)]));

        // Plains that must not touch a road: never.
        db.tiles.push(tile("plains", 1, 1, 1, vec![], vec![(0, -1, 0, LinkTest::NotEquals)]));
        let c = candidates(&db, &open);
        assert!(c.iter().any(|c| c.tile == 0 && c.rotation == 0x10 && (c.x, c.y) == (1, 0)));
        assert!(c.iter().all(|c| c.tile == 0 && c.joins == vec![0]));
    }
}
