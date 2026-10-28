//! The tile links' validator (Phase 6a of docs/plans/tile_ted_parity.md): what in a database entry's link targets and rules the game
//! would not use, or the kit never does. Measured on the battle kit's 626 entries (2026-10-28): a target lies on an edge cell of the tile
//! (615 of 621) with a "must be" rule of its set on the cell just outside it (608); a rule names a cell just past an edge (1354 of
//! 1383). The game matches a rule against the link set of the neighbour's cell (docs/re/battle_maps.md, "The fit"), so a set that is no
//! tile set never matches. TEd checks none of this (its integrity errors are about scene instances).

use wce_formats::tile_entry::TileEntry;

/// The tile set names of a kit's battle tile database (`<raw_root>/terrain/tiles/battle/_tile_database/_settings.xml`), sorted; empty
/// without the file.
pub fn kit_tile_sets(raw_root: &std::path::Path) -> Vec<String> {
    let path = raw_root.join("terrain/tiles/battle/_tile_database/_settings.xml");
    let Ok(doc) = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| wce_formats::xml_doc::XmlDoc::parse(b).map_err(|e| e.to_string())) else { return vec![] };
    let mut names = doc.elements().iter().enumerate().filter(|(_, el)| el.name == "TILE_SET").filter_map(|(i, _)| doc.get(i, "name")).collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

/// A problem of the links: what it is about (a target or a rule, its index in the entry) and what is wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkProblem {
    pub target: bool,
    pub index: usize,
    pub message: String,
}

/// The entry's link problems; `sets` are the kit's tile set names (empty: not checked).
pub fn link_problems(entry: &TileEntry, sets: &[String]) -> Vec<LinkProblem> {
    let (w, h) = (entry.width() as i32, entry.height() as i32);
    let (targets, links) = (entry.link_targets(), entry.links());
    let inside = |x: i32, y: i32| (0..w).contains(&x) && (0..h).contains(&y);
    let just_past = |x: i32, y: i32| ((x == -1 || x == w) && (0..h).contains(&y)) || ((y == -1 || y == h) && (0..w).contains(&x));
    let known = |s: &str| sets.is_empty() || sets.iter().any(|k| k == s);
    let mut out = vec![];
    let mut add = |target: bool, index: usize, message: String| out.push(LinkProblem { target, index, message });

    for (i, t) in targets.iter().enumerate() {
        let what = format!("target {} at ({}, {})", t.tile_set, t.x, t.y);
        if !inside(t.x, t.y) {
            add(true, i, format!("{what} is outside the tile ({w} x {h} cells): a target is a cell of the tile's own edge"));
            continue;
        }

        if !(t.x == 0 || t.y == 0 || t.x == w - 1 || t.y == h - 1) {
            add(true, i, format!("{what} is not on an edge cell: no neighbour touches it"));
        }

        if !known(&t.tile_set) {
            add(true, i, format!("{what}: '{}' is no tile set of the kit", t.tile_set));
        }

        let next = [(t.x - 1, t.y), (t.x + 1, t.y), (t.x, t.y - 1), (t.x, t.y + 1)];
        let asked = links.iter().any(|l| l.link_set == t.tile_set && l.test != "TLT_NOT_EQUALS" && !inside(l.x, l.y) && next.contains(&(l.x, l.y)));
        if !asked {
            add(true, i, format!("{what}: no rule asks for {} on the cell just outside it (the kit pairs every target with one)", t.tile_set));
        }

        if targets[..i].iter().any(|o| o.tile_set == t.tile_set && (o.x, o.y) == (t.x, t.y)) {
            add(true, i, format!("{what} is there twice"));
        }
    }

    for (i, l) in links.iter().enumerate() {
        let test = if l.test == "TLT_NOT_EQUALS" { "must not be" } else { "must be" };
        let what = format!("rule ({}, {}) {test} {}", l.x, l.y, l.link_set);
        if !matches!(l.test.as_str(), "TLT_EQUALS" | "TLT_NOT_EQUALS") {
            add(false, i, format!("{what}: test '{}' is neither TLT_EQUALS nor TLT_NOT_EQUALS", l.test));
        }

        if inside(l.x, l.y) {
            add(false, i, format!("{what} names a cell of the tile itself: a rule is about a neighbour's cell"));
        } else if !just_past(l.x, l.y) {
            add(false, i, format!("{what} names a cell that is not just past an edge (a corner or further): the kit almost never does"));
        }

        if !known(&l.link_set) {
            add(false, i, format!("{what}: '{}' is no tile set of the kit, it never matches", l.link_set));
        }

        if links[..i].iter().any(|o| o.link_set == l.link_set && (o.x, o.y) == (l.x, l.y) && o.test == l.test) {
            add(false, i, format!("{what} is there twice"));
        }

        if links[..i].iter().any(|o| o.link_set == l.link_set && (o.x, o.y) == (l.x, l.y) && o.test != l.test) {
            add(false, i, format!("{what}: the same cell must be and must not be {}: nothing can be put there", l.link_set));
        }
    }

    out
}
