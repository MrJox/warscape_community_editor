//! BOB's fill of a battle map's tile map (WARSCAPE::EDITOR_TILE_MAP, Warscape.AssemblyKit.dll; docs/re/battle_maps.md): which tile, variation and
//! rotation goes on every cell, from the map's `tile_map.png` (a tile set, tile or variation colour per cell) and `climate_map.png`, then the
//! battle field the Terrain processor writes as `terrain.tile_list`.
//!
//! Not ported yet: resource tiles, custom geometry, `also_place_tile_set` and shared geometry tiles (no vanilla tile set has one).

use super::tile_db::{LinkTest, SET_BIT, TileDatabase, VARIATION_BIT};

/// The rotations (BATTLE_TILE_ROTATION): 0, 90, 180 and 270 degrees.
pub const ROTATIONS: [u16; 4] = [0x10, 0x20, 0x40, 0x80];

/// `terrain_large_tile`: a tile is large from this width or height on.
const LARGE_TILE: i32 = 8;

/// `SCAN_TILE_TYPE`, in `build_tile_map`'s order (0x10280720).
const SCANS: [u8; 10] = [1, 2, 9, 8, 5, 7, 6, 3, 4, 0];

/// `TILE_MAP::rotate_in_tile_space` (0x102e2f40): a cell of a `w` x `h` tile under a rotation, in map cells from the tile's origin.
pub fn rotate_in_tile_space(w: i32, h: i32, rotation: u16, x: i32, y: i32) -> (i32, i32) {
    match rotation {
        0x20 => (h - 1 - y, x),
        0x40 => (w - 1 - x, h - 1 - y),
        0x80 => (y, w - 1 - x),
        _ => (x, y),
    }
}

/// A cell of one of the tile map's two layers (TILE_MAP, 20 bytes).
#[derive(Clone, Copy, Debug, Default)]
pub struct Cell {
    /// 0: empty; > 0: 1-based index of the placed variation (the tile's origin cell); < 0: -(origin cell + 1).
    pub index: i32,
    pub rotation: u16,

    /// Bits 8..10 (0x700: the layers a cell draws).
    pub layer_bits: u16,
    pub origin_x: u16,
    pub origin_y: u16,
    pub climate: u8,
    pub flipped: bool,
    pub lf_min: f32,
    pub lf_max: f32,
}

/// The game's random generator, as BOB's map uses it (fn 0x102806a0, seed 0x12344332).
#[derive(Clone, Debug)]
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u16 {
        self.0 = self.0.wrapping_mul(0x343fd).wrapping_add(0x269ec3);
        (self.0 >> 16) as u16
    }

    /// A value below `n`, rerolled while at or below `0xffff % n`.
    fn below(&mut self, n: u32) -> u32 {
        let n = n.max(1);
        let mut v = u32::from(self.next());
        while v <= 0xffff % n {
            v = u32::from(self.next());
        }

        v % n
    }
}

/// A placed tile of the battle field (BATTLE_TILE_MAP::TILE_INSTANCE).
#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    pub tile: usize,
    pub variation: usize,
    pub x: i16,
    pub y: i16,
    pub flags: u16,
    pub climate: u8,
    pub lf: [f32; 2],
}

/// The inputs of a map.
pub struct MapImages<'a> {
    pub width: u32,
    pub height: u32,

    /// RGB per cell, row by row.
    pub tile_map: &'a [[u8; 3]],
    pub climate_map: Option<&'a [[u8; 3]]>,

    /// `x, y, tile path, rotation degrees` lines (explicit_tiles.txt).
    pub explicit_tiles: Option<&'a str>,

    /// The low-frequency height field (0..1) and its size.
    pub lf: Option<(&'a [f32], u32, u32)>,
}

pub struct TileMapFill<'a> {
    db: &'a TileDatabase,
    pub w: u32,
    pub h: u32,
    ids: Vec<u32>,
    climates: Vec<u32>,
    areas: Vec<[i32; 4]>,
    rng: Rng,
    pub layers: [Vec<Cell>; 2],

    /// (tile, variation) per placed index - 1 (`find_tile`).
    pub placed: Vec<(usize, usize)>,
    links: Vec<Vec<bool>>,

    /// Lines BOB would print ("Failed to find tile for point x x y").
    pub messages: Vec<String>,
}

fn colour_eq(c: [f32; 3], p: [u8; 3]) -> bool {
    c[0] == f32::from(p[0]) && c[1] == f32::from(p[1]) && c[2] == f32::from(p[2])
}

impl<'a> TileMapFill<'a> {

    /// `load_map` (0x1027e4f0): the cell ids and climates, then `scan_tile_areas`.
    pub fn new(db: &'a TileDatabase, m: &MapImages) -> Self {
        let (w, h) = (m.width, m.height);
        let n = (w * h) as usize;
        let mut f = Self {
            db, w, h, ids: vec![0; n], climates: vec![0; n], areas: vec![[i32::MAX, i32::MAX, i32::MIN + 1, i32::MIN + 1]; db.sets.len()],
            rng: Rng(0x1234_4332), layers: [vec![Cell::default(); n], vec![Cell::default(); n]], placed: vec![],
            links: vec![vec![false; db.sets.len() + 1]; n], messages: vec![],
        };

        // get_tile_id (0x10281b40): a tile set's colour, else a tile's, else a variation's; black is nothing. A pixel equal to the one before
        // keeps its id.
        let mut last: Option<([u8; 3], u32)> = None;
        for i in 0..n {
            let p = m.tile_map.get(i).copied().unwrap_or([0; 3]);
            f.ids[i] = match last {
                Some((lp, id)) if lp == p => id,
                _ => {
                    let id = f.tile_id(p);
                    last = Some((p, id));
                    id
                }
            };

            if let Some(c) = m.climate_map {
                let p = c.get(i).copied().unwrap_or([0; 3]);
                f.climates[i] = db.climates.iter().position(|k| colour_eq(k.colour, p)).map_or(0, |k| k as u32);
            }
        }

        f.scan_tile_areas();
        f
    }

    fn tile_id(&self, p: [u8; 3]) -> u32 {
        if p == [0; 3] {
            return 0;
        }

        if let Some(s) = self.db.sets.iter().find(|s| colour_eq(s.colour, p)) {
            return s.id;
        }

        for t in &self.db.tiles {
            if colour_eq(t.colour, p) {
                return t.id;
            }

            if let Some(v) = t.variations.iter().position(|v| colour_eq(v.colour, p)) {
                return t.variation_id(v);
            }
        }

        0
    }

    /// The set a cell id counts for in the areas, and the pad around it (FUN_10398350): a tile's (id 0 is the first tile) with its size, a
    /// set's with 128.
    fn scan_tile_areas(&mut self) {
        for y in 0..self.h as i32 {
            for x in 0..self.w as i32 {
                let id = self.ids[(y as u32 * self.w + x as u32) as usize];
                let (set_id, pw, ph) = if id & SET_BIT == 0 {
                    match self.db.tile_from_id(id).and_then(|t| self.db.tiles[t].set.map(|s| (s, t))) {
                        Some((s, t)) => (self.db.sets[s].id, self.db.tiles[t].width, self.db.tiles[t].height),
                        None => continue,
                    }
                } else {
                    (id, 0x80, 0x80)
                };

                let Some(s) = self.db.set_from_id(set_id) else { continue };
                let a = &mut self.areas[s];
                a[0] = a[0].min(x - pw);
                a[1] = a[1].min(y - ph);
                a[2] = a[2].max(x - pw);
                a[3] = a[3].max(y - ph);
                a[0] = a[0].min(x + pw);
                a[1] = a[1].min(y + ph);
                a[2] = a[2].max(x + pw);
                a[3] = a[3].max(y + ph);
            }
        }
    }

    fn cell(&self, x: u32, y: u32) -> usize {
        (y * self.w + x) as usize
    }

    /// `TILE_MAP::valid_tile_at_point`: something placed (outside the map counts as taken).
    fn taken(&self, x: u32, y: u32, geometry: bool) -> bool {
        if x >= self.w || y >= self.h {
            return true;
        }

        self.layers[usize::from(geometry)][self.cell(x, y)].index != 0
    }

    /// `build_tile_map` (0x10280720).
    pub fn build(&mut self, explicit: Option<&str>) {
        if let Some(text) = explicit {
            self.add_explicit_tiles(text);
        }

        for scan in SCANS {
            self.scan_for_tiles(scan);
        }

        self.calculate_flow();
    }

    /// The origin cell of a tile-layer cell.
    fn origin_of(&self, c: usize) -> usize {
        let i = self.layers[0][c].index;
        if i < 0 { (-1 - i) as usize } else { c }
    }

    /// `TILE_MAP::calculate_flow` (0x102e6180): which way the rivers flow. From every river mouth, a breadth-first walk over the links into
    /// `river` / `river_crossing` sets (a cell can be queued again until it is walked); then every tile reached gets the `is_entry` of its link
    /// (into `river` or `river_transition_end`) that lands on the tile it was reached from.
    fn calculate_flow(&mut self) {
        let db = self.db;
        let mut walked: Vec<usize> = vec![];
        for c in 0..self.layers[0].len() {
            let cell = self.layers[0][c];
            if cell.index <= 0 {
                continue;
            }

            let (t, _) = self.placed[(cell.index - 1) as usize];
            if db.tiles[t].set.is_none_or(|s| db.sets[s].name != "river_mouth") || walked.contains(&c) {
                continue;
            }

            // (cell, offset to the parent in the list); the second list keeps each entry's own index.
            let mut reached: Vec<(usize, isize)> = vec![(c, 0)];
            let mut queue: Vec<(usize, usize)> = vec![(c, 0)];
            let mut i = 0;
            while i < queue.len() {
                let (qc, own) = queue[i];
                let o = self.origin_of(qc);
                let qcell = self.layers[0][qc];
                let (qt, _) = self.placed[(self.layers[0][o].index - 1) as usize];
                let tile = &db.tiles[qt];
                for link in &tile.links {
                    let (dx, dy) = rotate_in_tile_space(tile.width, tile.height, qcell.rotation & 0xf0, link.x, link.y);
                    let (px, py) = (i32::from(qcell.origin_x) + dx, i32::from(qcell.origin_y) + dy);
                    if px < 0 || py < 0 || px as u32 >= self.w || py as u32 >= self.h {
                        continue;
                    }

                    let c2 = self.origin_of(py as usize * self.w as usize + px as usize);
                    if self.layers[0][c2].index == 0 {
                        continue;
                    }

                    let Some(ls) = link.set else { continue };
                    if db.sets[ls].name != "river" && db.sets[ls].name != "river_crossing" {
                        continue;
                    }

                    if !walked.contains(&c2) {
                        reached.push((c2, own as isize - reached.len() as isize));
                        queue.push((c2, queue.len()));
                    }
                }

                walked.push(qc);
                i += 1;
            }

            for k in 1..reached.len() {
                let (rc, off) = reached[k];
                let parent = self.layers[0][reached[(k as isize + off) as usize].0];
                let (pt, _) = self.placed[(parent.index - 1) as usize];
                let ptile = &db.tiles[pt];
                let turned = matches!(parent.rotation & 0xf0, 0x20 | 0x80);
                let (pw, ph) = if turned { (ptile.height, ptile.width) } else { (ptile.width, ptile.height) };
                let cell = self.layers[0][rc];
                let (ct, _) = self.placed[(self.layers[0][self.origin_of(rc)].index - 1) as usize];
                let ctile = &db.tiles[ct];
                for link in &ctile.links {
                    let Some(ls) = link.set else { continue };
                    if db.sets[ls].name != "river" && db.sets[ls].name != "river_transition_end" {
                        continue;
                    }

                    let (dx, dy) = rotate_in_tile_space(ctile.width, ctile.height, cell.rotation & 0xf0, link.x, link.y);
                    let (px, py) = (i32::from(cell.origin_x) + dx, i32::from(cell.origin_y) + dy);
                    let (ox, oy) = (i32::from(parent.origin_x), i32::from(parent.origin_y));
                    if ox <= px && px < ox + pw && oy <= py && py < oy + ph {
                        self.layers[0][rc].flipped = link.is_entry;
                        break;
                    }
                }
            }
        }
    }

    /// `add_explicit_tiles` (0x1027fb90): `x,y,location,rotation` (0 / 90 / 180 / 270); a tile's location gets a random variation.
    fn add_explicit_tiles(&mut self, text: &str) {
        for line in text.lines() {
            let parts: Vec<&str> = line.split(',').map(str::trim).collect();
            if parts.len() < 4 {
                continue;
            }

            let (Ok(x), Ok(y)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) else { continue };
            let rotation = match parts[3] {
                "0" => 0x10,
                "90" => 0x20,
                "180" => 0x40,
                "270" => 0x80,
                _ => continue,
            };

            if x >= self.w || y >= self.h {
                continue;
            }

            let climate = self.climates[self.cell(x, y)];
            let loc = parts[2];
            if let Some(t) = self.db.tiles.iter().position(|t| t.location.eq_ignore_ascii_case(loc)) {
                let v = self.rng.below(self.db.tiles[t].variations.len() as u32) as usize;
                self.place_tile(x, y, t, v, rotation, climate, false);
            } else if let Some((t, v)) = self.db.tiles.iter().enumerate().find_map(|(t, tile)| {
                tile.variations.iter().position(|v| format!("{}\\{}", tile.location, v.location).eq_ignore_ascii_case(loc)).map(|v| (t, v))
            }) {
                self.place_tile(x, y, t, v, rotation, climate, false);
            }
        }
    }

    /// The candidates of a scan (the first loop of `scan_for_tiles`, 0x10281ea0), in the database's order.
    fn candidates(&self, scan: u8) -> Vec<usize> {
        let db = self.db;
        let mut out = vec![];
        for (i, t) in db.tiles.iter().enumerate() {
            if scan == 8 && !(t.width == 2 && t.height == 2 && !t.masked() && t.links.is_empty()) {
                continue;
            }

            if scan == 7 {
                let other = t.link_targets.iter().any(|g| g.set != t.set);
                if !other && t.link_targets.len() <= 2 {
                    continue;
                }
            } else {
                let masked_scan = scan == 4 || scan == 6;
                if t.masked() != masked_scan {
                    continue;
                }
            }

            if matches!(scan, 6 | 3 | 1) {
                if t.width < LARGE_TILE && t.height < LARGE_TILE {
                    continue;
                }
            } else if t.width >= LARGE_TILE && t.height >= LARGE_TILE {
                continue;
            }

            if scan == 5 {
                let first = t.link_targets.first().map(|g| g.set);
                if !t.link_targets.iter().any(|g| Some(g.set) != first) {
                    continue;
                }
            }

            if matches!(scan, 1 | 2) && t.set.is_none_or(|s| db.sets[s].shared_geometry != db.sets[s].name) {
                continue;
            }

            out.push(i);
        }

        out
    }

    fn scan_for_tiles(&mut self, scan: u8) {
        let mut candidates = self.candidates(scan);
        if candidates.is_empty() {
            return;
        }

        // The junction pass orders its tiles by link target count (std::sort, FUN_1037d260: not stable, ties as MSVC's introsort leaves them).
        if scan == 7 {
            let db = self.db;
            crate::vegetation::msvc_sort::sort(&mut candidates, &|a: &usize, b: &usize| db.tiles[*a].link_targets.len() < db.tiles[*b].link_targets.len());
        }

        let shared = matches!(scan, 1 | 2);
        for y in 0..self.h {
            for x in 0..self.w {
                if !(matches!(scan, 7 | 4 | 6) || !self.taken(x, y, false)) {
                    continue;
                }

                // Resource tiles: not ported (no settlement map has a resource map).
                if scan == 8 {
                    continue;
                }

                let mut climate = self.climates[self.cell(x, y)];
                for &t in &candidates {
                    if !shared {
                        let Some(s) = self.db.tiles[t].set else { continue };
                        let a = self.areas[s];
                        let (xi, yi) = (x as i32, y as i32);
                        if xi < a[0] || a[2] < xi || yi < a[1] || a[3] < yi {
                            continue;
                        }
                    }

                    if let Some((variation, rotation)) = self.test_final_tile_position(t, x, y, shared, scan == 8, &mut climate, scan == 9) {
                        self.place_tile(x, y, t, variation, rotation, climate, shared);
                        if !shared {
                            self.add_tile_to_link_map(x, y, t, rotation);
                        }

                        if scan != 4 && scan != 6 {
                            break;
                        }
                    }
                }

                if scan == 0 && !self.taken(x, y, false) && self.ids[self.cell(x, y)] != 0 {
                    self.messages.push(format!("Failed to find tile for point {x}x{y}"));
                }
            }
        }
    }

    /// `test_final_tile_position` (0x10280b30): whether the tile fits with its origin at (x, y), in which rotation and variation.
    #[allow(clippy::too_many_arguments)]
    fn test_final_tile_position(&mut self, t: usize, x: u32, y: u32, shared: bool, resource: bool, climate: &mut u32, exact: bool) -> Option<(usize, u16)> {
        let db = self.db;
        let tile = &db.tiles[t];
        let (tw, th) = (tile.width, tile.height);
        let (mw, mh) = (self.w as i32, self.h as i32);
        let mut ok = [true; 4];
        let mut origin = [(-1i32, -1i32); 4];
        let mut random_variation = true;
        let mut variation = 0usize;
        for r in 0..4 {
            if resource && r != 0 {
                ok[r] = false;
            }

            let rot = ROTATIONS[r];
            'rows: for sy in 0..th {
                for sx in 0..tw {
                    let (dx, dy) = rotate_in_tile_space(tw, th, rot, sx, sy);
                    let (px, py) = (x as i32 + dx, y as i32 + dy);
                    if px >= mw || py >= mh {
                        ok[r] = false;
                        break;
                    }

                    if tile.masked() && !tile.subtile_masked_valid(sx, sy) {
                        continue;
                    }

                    if origin[r] == (-1, -1) {
                        origin[r] = (px, py);
                    }

                    if self.taken(px as u32, py as u32, shared) {
                        ok[r] = false;
                        break;
                    }

                    let cid = self.ids[self.cell(px as u32, py as u32)];
                    if cid == 0 {
                        ok[r] = false;
                        break;
                    }

                    if exact {
                        let mut found = tile.id == cid;
                        if let Some(v) = (0..tile.variations.len()).find(|&v| tile.variation_id(v) == cid) {
                            found = true;
                            variation = v;
                        }

                        if !found {
                            ok[r] = false;
                            break;
                        }
                    } else if shared {
                        // No vanilla tile set shares geometry.
                        ok[r] = false;
                        break;
                    } else {
                        if cid & SET_BIT == 0 && !db.tiles_match(cid, tile.id) {
                            ok[r] = false;
                            break;
                        }

                        if cid & SET_BIT != 0 && tile.set.map(|s| db.sets[s].id) != Some(cid) {
                            ok[r] = false;
                            break;
                        }

                        if cid & VARIATION_BIT != 0 {
                            variation = ((cid & !VARIATION_BIT) >> 16) as usize;
                            random_variation = false;
                        }
                    }
                }

                if !ok[r] {
                    break 'rows;
                }
            }

            if ok[r] && !tile.links.is_empty() {
                for link in &tile.links {
                    let (dx, dy) = rotate_in_tile_space(tw, th, rot, link.x, link.y);
                    let (px, py) = (x as i32 + dx, y as i32 + dy);
                    if px < 0 || py < 0 || px >= mw || py >= mh {
                        continue;
                    }

                    let cid = self.ids[self.cell(px as u32, py as u32)];
                    let cell_set = if cid & SET_BIT != 0 {
                        db.set_from_id(cid)
                    } else {
                        match db.tile_from_id(cid) {
                            Some(c) => db.tiles[c].set,
                            None => None,
                        }
                    };

                    let Some(cell_set) = cell_set else {
                        ok[r] = false;
                        break;
                    };

                    let cell_link = db.set_from_name(db.sets[cell_set].link_name());
                    let Some(link_set) = link.set.and_then(|s| db.set_from_name(db.sets[s].link_name())) else {
                        ok[r] = false;
                        break;
                    };

                    let matched = cell_link.is_some_and(|c| db.tile_sets_match(db.sets[c].id, db.sets[link_set].id));
                    let li = (db.sets[link_set].id & 0x7fff_ffff) as usize;
                    let v = &self.links[self.cell(px as u32, py as u32)];
                    match link.test {
                        LinkTest::Equals => {
                            if matched && v.iter().all(|b| !b) {
                                continue;
                            }

                            if !v.get(li).copied().unwrap_or(false) {
                                ok[r] = false;
                                break;
                            }
                        }

                        LinkTest::NotEquals => {
                            if matched {
                                ok[r] = false;
                                break;
                            }
                        }
                    }
                }
            }
        }

        if !tile.random_rotatable {
            return ok[0].then_some((0, 0x10));
        }

        if random_variation {
            variation = self.rng.below(tile.variations.len() as u32) as usize;
        }

        let start = if self.w == 64 { 0 } else { self.rng.below(4) as usize };
        for r in (start..4).chain(0..start) {
            if ok[r] {
                if tile.masked() {
                    let (ox, oy) = origin[r];
                    *climate = self.climates[(oy as u32 * self.w + ox as u32) as usize];
                }

                return Some((variation, ROTATIONS[r]));
            }
        }

        None
    }

    /// `TILE_MAP::place_tile` (0x102e3140).
    #[allow(clippy::too_many_arguments)]
    fn place_tile(&mut self, x: u32, y: u32, t: usize, variation: usize, rotation: u16, climate: u32, geometry: bool) {
        let index = match self.placed.iter().position(|&p| p == (t, variation)) {
            Some(i) => i as i32 + 1,
            None => {
                self.placed.push((t, variation));
                self.placed.len() as i32
            }
        };

        let tile = &self.db.tiles[t];
        let (tw, th) = (tile.width, tile.height);
        let layer = usize::from(geometry);
        let mut first: Option<usize> = None;
        for sy in 0..th {
            for sx in 0..tw {
                let (dx, dy) = rotate_in_tile_space(tw, th, rotation, sx, sy);
                if tile.masked() && !tile.subtile_masked_valid(sx, sy) {
                    continue;
                }

                let (px, py) = (x as i32 + dx, y as i32 + dy);
                if py < 0 || py as u32 >= self.h || px < 0 {
                    continue;
                }

                let c = (py as u32 * self.w + px as u32) as usize;
                let bits = if geometry { 0x100 } else if self.layers[1].get(c).is_some_and(|g| g.index != 0) { 0x600 } else { 0x700 };
                match first {
                    None => {
                        if px as u32 >= self.w {
                            continue;
                        }

                        first = Some(c);
                        self.layers[layer][c] = Cell {
                            index, rotation, layer_bits: bits, origin_x: x as u16, origin_y: y as u16, climate: climate as u8, flipped: false, lf_min: 0.0, lf_max: 0.0,
                        };
                    }

                    Some(f) => {
                        if c == f || px as u32 >= self.w {
                            continue;
                        }

                        self.layers[layer][c] = Cell {
                            index: -(f as i32 + 1), rotation: 0x10, layer_bits: bits, origin_x: x as u16, origin_y: y as u16, climate: climate as u8,
                            flipped: false, lf_min: 0.0, lf_max: 0.0,
                        };
                    }
                }
            }
        }
    }

    /// `add_tile_to_link_map` (0x10282740).
    fn add_tile_to_link_map(&mut self, x: u32, y: u32, t: usize, rotation: u16) {
        let db = self.db;
        let tile = &db.tiles[t];
        let (tw, th) = (tile.width, tile.height);
        let mark = |s: &mut Self, px: i32, py: i32, index: usize| {
            if px < 0 || py < 0 || px as u32 >= s.w || py as u32 >= s.h {
                return;
            }

            let c = (py as u32 * s.w + px as u32) as usize;
            if let Some(b) = s.links[c].get_mut(index) {
                *b = true;
            }
        };

        if tile.link_targets.is_empty() {
            let Some(set) = tile.set.and_then(|s| db.set_from_name(db.sets[s].link_name())) else { return };
            let index = (db.sets[set].id & 0x7fff_ffff) as usize;
            for sy in 0..th {
                for sx in 0..tw {
                    let (dx, dy) = rotate_in_tile_space(tw, th, rotation, sx, sy);
                    if tile.subtile_masked_valid(sx, sy) {
                        mark(self, x as i32 + dx, y as i32 + dy, index);
                    }
                }
            }
        } else {
            // The cells themselves get the last slot (the decompiled write indexes the vector with the set count).
            let last = db.sets.len();
            for sy in 0..th {
                for sx in 0..tw {
                    if tile.subtile_masked_valid(sx, sy) {
                        let (dx, dy) = rotate_in_tile_space(tw, th, rotation, sx, sy);
                        mark(self, x as i32 + dx, y as i32 + dy, last);
                    }
                }
            }

            for g in &tile.link_targets {
                let (dx, dy) = rotate_in_tile_space(tw, th, rotation, g.x, g.y);
                let Some(set) = g.set.and_then(|s| db.set_from_name(db.sets[s].link_name())) else { continue };
                mark(self, x as i32 + dx, y as i32 + dy, (db.sets[set].id & 0x7fff_ffff) as usize);
            }
        }
    }

    /// `TILE_MAP::calculate_lf_min_maxs` (0x102e6ec0): the low-frequency height range under every placed tile, two cells around it.
    pub fn calculate_lf_min_maxs(&mut self, field: &[f32], fw: u32, fh: u32) {
        let (mw, mh) = (f64::from(self.w), f64::from(self.h));
        for layer in 0..2 {
            for c in 0..self.layers[layer].len() {
                let cell = self.layers[layer][c];
                if cell.index <= 0 {
                    continue;
                }

                let (t, _) = self.placed[(cell.index - 1) as usize];
                let tile = &self.db.tiles[t];
                let size = f64::from(tile.width.max(tile.height));
                let clamp = |v: f32| v.clamp(0.0, 1.0);
                let x0 = clamp((f64::from(i32::from(cell.origin_x) - 2) / mw) as f32);
                let y0 = clamp((f64::from(i32::from(cell.origin_y) - 2) / mh) as f32);
                let x1 = clamp(((f64::from(cell.origin_x) + size + 2.0) / mw) as f32);
                let y1 = clamp(((f64::from(cell.origin_y) + size + 2.0) / mh) as f32);
                let (sx, sy) = (x0 * fw as f32, y0 * fh as f32);
                let (ex, ey) = (x1 * fw as f32, y1 * fh as f32);
                let (mut lo, mut hi) = (f32::MAX, f32::MIN);
                let mut py = sy.round_ties_even() as i64;
                while (py as f32) < ey {
                    let mut px = sx.round_ties_even() as i64;
                    while (px as f32) < ex {
                        let v = field.get(py as usize * fw as usize + px as usize).copied().unwrap_or(0.0);
                        lo = lo.min(v);
                        hi = hi.max(v);
                        px += 1;
                    }

                    py += 1;
                }

                let cell = &mut self.layers[layer][c];
                cell.lf_min = lo;
                cell.lf_max = hi;
            }
        }
    }

    /// `TILE_MAP::build_battle_field` (FUN_1035f640) over the whole map: every origin cell, the tile layer then the geometry layer.
    pub fn battle_field(&self) -> Vec<Placement> {
        let mut out = vec![];
        for c in 0..(self.w * self.h) as usize {
            for layer in 0..2 {
                let cell = self.layers[layer][c];
                if cell.index <= 0 {
                    continue;
                }

                let (tile, variation) = self.placed[(cell.index - 1) as usize];
                out.push(Placement {
                    tile, variation, x: cell.origin_x as i16, y: cell.origin_y as i16,
                    flags: (cell.rotation & 0xf0) | if cell.flipped { 4 } else { 0 } | (cell.layer_bits & 0x700),
                    climate: cell.climate, lf: [cell.lf_min, cell.lf_max],
                });
            }
        }

        out
    }
}
