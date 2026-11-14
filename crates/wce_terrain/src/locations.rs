//! `locations.blm` and its masks as BOB_Terrain's `Tilemap` writes them (`save_meta_data_map`, BOB_Terrain fn 0x10011f60, the masks from
//! ToolDataBuilderDll's `process_terrain_internal`; `WARSCAPE::BATTLE_LOCATIONS_MAP::save`; docs/re/terry.md, "locations.blm"):
//!
//! - per battle type a record per BCA in entity order: the cell rectangle of its `Catchment Region` (each edge's metres from the map's
//!   centre over 256, truncated toward zero, plus `w / 2`, or `h / 2` minus for y), its position's cell the same way, its id
//!   (`0xffff0000 + n` for a BCA with deployment zones, `n` its deployment file's number, else -1), 0, 0;
//! - the types and the climate table in the order of the engine's hash map (`CA_STD::UNORDERED_MAP`, [`hash_order`]): the types
//!   inserted as their first BCA comes, the climates in the tile database's order;
//! - the climate table: per climate of the tile database the environments `environment_map.xml` lists for it (fn 0x1002d110): land 2,
//!   sea 4, day 0x20000, night 0x10000, dry 0x400000, rain 0x200000, fog 0x40000, snow 0x100000, storm 0x80000 (the attributes'
//!   text searched for the words, as `CA::String::find`);
//! - `mask0` and `mask1`, a u16 per cell of the tile map's tile layer ([`masks`]).

use wce_tile::battle_map::locations::{BattleLocations, Location};
use wce_tile::battle_map::tile_db::TileDatabase;
use wce_tile::battle_map::tile_map::TileMap;

use crate::terry::{EntityKind, TerryProject};

/// The engine's string hash (fn 0x1003eb50): from the last character back, each (signed) byte shifted by 0, 8, 16, 24 in turn, xored.
pub fn ca_hash(s: &str) -> u32 {
    let mut h = 0xf73a_51c9u32;
    let mut shift = 0;
    for &c in s.as_bytes().iter().rev() {
        h ^= ((c as i8 as i32) << shift) as u32;
        shift = (shift + 8) % 32;
    }

    h
}

/// The order `CA_STD::UNORDERED_MAP` iterates keys inserted in this order: 4 buckets, each key appended to its bucket (hash modulo the
/// count), the buckets grown to `2n + 1` (their keys moved bucket by bucket) when the keys outnumber them; iterated bucket by bucket.
pub fn hash_order<'a>(keys: &[&'a str]) -> Vec<&'a str> {
    let mut buckets: Vec<Vec<&str>> = vec![vec![]; 4];
    let mut size = 0usize;
    for &k in keys {
        if buckets[(ca_hash(k) as usize) % buckets.len()].contains(&k) {
            continue;
        }

        size += 1;
        if size > buckets.len() {
            let n = buckets.len() * 2 + 1;
            let mut next: Vec<Vec<&str>> = vec![vec![]; n];
            for e in buckets.iter().flatten() {
                next[(ca_hash(e) as usize) % n].push(e);
            }

            buckets = next;
        }

        let n = buckets.len();
        buckets[(ca_hash(k) as usize) % n].push(k);
    }

    buckets.into_iter().flatten().collect()
}

/// A coordinate's cell: metres from the centre over 256, truncated toward zero, from the centre cell.
fn cell(m: f64, centre: i32) -> i32 {
    centre + (m / 256.0).trunc() as i32
}

/// The battle types' records of a project's BCAs on a `w` x `h` map, in the hash map's order.
pub fn records(p: &TerryProject, w: u32, h: u32) -> Vec<(String, Vec<Location>)> {
    let (cx, cy) = ((w / 2) as i32, (h / 2) as i32);
    let mut types: Vec<(String, Vec<Location>)> = vec![];
    let mut deployment = 0u32;
    for e in &p.entities {
        let EntityKind::Bca(kind) = p.kind(e) else { continue };
        let kids: Vec<_> = e.children.iter().filter_map(|&c| p.entity(c)).collect();
        let id = if kids.iter().any(|z| matches!(p.kind(z), EntityKind::DeploymentZone)) {
            deployment += 1;
            (0xffff_0000u32 + deployment - 1) as i32
        } else {
            -1
        };

        let pos = p.world_position(e.id);
        let region = kids.iter().find(|c| matches!(p.kind(c), EntityKind::CatchmentRegion)).copied().unwrap_or(e);
        let size = p.rectangle(region).unwrap_or([0.0, 0.0]);
        let rp = p.world_position(region.id);
        let rect = [
            cell(rp[0] - size[0] / 2.0, cx),
            cell(-(rp[2] + size[1] / 2.0), cy),
            cell(rp[0] + size[0] / 2.0, cx),
            cell(-(rp[2] - size[1] / 2.0), cy),
        ];
        let loc = Location { rect, centre: [cell(pos[0], cx), cell(-pos[2], cy)], id, extra: [0, 0] };
        match types.iter_mut().find(|t| t.0 == kind) {
            Some(t) => t.1.push(loc),
            None => types.push((kind, vec![loc])),
        }
    }

    let names: Vec<&str> = types.iter().map(|t| t.0.as_str()).collect();
    let order: Vec<String> = hash_order(&names).into_iter().map(str::to_owned).collect();
    order.into_iter().filter_map(|n| types.iter().position(|t| t.0 == n).map(|i| types[i].clone())).collect()
}

/// The environments each climate of the database has (fn 0x1002d110), from `environment_map.xml`.
pub fn climate_environments(db: &TileDatabase, environment_map: &[u8]) -> Vec<(String, u32)> {
    let root = wce_tile::vegetation::ca_xml::parse(environment_map);
    let mut files = vec![];
    fn walk<'a>(e: &'a wce_tile::vegetation::ca_xml::Element, out: &mut Vec<&'a wce_tile::vegetation::ca_xml::Element>) {
        if e.name == "environment_file" {
            out.push(e);
        }

        for c in &e.children {
            walk(c, out);
        }
    }

    walk(&root, &mut files);
    let words: [(&str, &[(&str, u32)]); 3] = [
        ("battle_types", &[("land", 2), ("sea", 4)]),
        ("time_of_day", &[("day", 0x20000), ("night", 0x10000)]),
        ("precipitation", &[("dry", 0x40_0000), ("rain", 0x20_0000), ("fog", 0x4_0000), ("snow", 0x10_0000), ("storm", 0x8_0000)]),
    ];

    db.climates
        .iter()
        .map(|c| {
            let mut v = 0u32;
            for f in files.iter().filter(|f| f.attribute("climate").is_some_and(|a| a.contains(c.name.as_str()))) {
                for (attr, list) in &words {
                    let text = f.attribute(attr).unwrap_or_default();
                    for (w, bit) in list.iter() {
                        if text.contains(w) {
                            v |= bit;
                        }
                    }
                }
            }

            (c.name.clone(), v)
        })
        .collect()
}

/// What follows the records: the cell count and the climate table (in the hash map's order).
pub fn tail(w: u32, h: u32, climates: &[(String, u32)]) -> Vec<u8> {
    let mut o = (w * h).to_le_bytes().to_vec();
    o.extend_from_slice(&(climates.len() as u32).to_le_bytes());
    let names: Vec<&str> = climates.iter().map(|c| c.0.as_str()).collect();
    for n in hash_order(&names) {
        let v = climates.iter().find(|c| c.0 == n).map_or(0, |c| c.1);
        o.extend_from_slice(&(n.len() as u16).to_le_bytes());
        o.extend_from_slice(n.as_bytes());
        o.extend_from_slice(&v.to_le_bytes());
    }

    o
}

/// The whole file.
pub fn build(p: &TerryProject, w: u32, h: u32, climates: &[(String, u32)]) -> BattleLocations {
    BattleLocations { version: 8, width: w, height: h, types: records(p, w, h), tail: tail(w, h, climates) }
}

/// An RGBA image (8 bits a channel, row by row) for the masks.
pub struct Image<'a> {
    pub width: u32,
    pub height: u32,
    pub pixels: &'a [[u8; 4]],
}

impl Image<'_> {
    fn at(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        (x < self.width && y < self.height).then(|| self.pixels[(y * self.width + x) as usize])
    }
}

/// The inputs of the masks besides the tile map.
pub struct MaskInputs<'a> {
    /// `snow_map.png`: a white cell (alpha too) sets 0x200.
    pub snow: Option<Image<'a>>,

    /// `culture_map.png` and the cultures' colours (`tile_upgrades.xml`'s `TILE_UPGRADES` children with `id`, `red`, `green`, `blue`,
    /// alpha 255): the first five set 0x400, 0x800, 0x1000, 0x4000, 0x8000; read only when the image covers the map.
    pub culture: Option<Image<'a>>,
    pub culture_colours: Vec<[u8; 4]>,

    /// `nogo.dds` (RGBA): (0, 139, 185) sets 4, (0, 174, 0) sets 2 (BOB_Terrain's own pass; it compares the file's B, G, R bytes).
    pub nogo: Option<Image<'a>>,

    /// The snow attrition maps `snow_map_sp`, `_su`, `_au`, `_wi.png` (fn 0x1002dbc0): with all four of one size (the map's width or
    /// height: `main_attila_map`'s are 4 rows taller, read from the top), a white cell sets
    /// mask1 0x100, 0x200, 0x400, 0x800 by season and mask0 0x200; without them a white `snow_map.png` cell sets mask0 0x200.
    pub seasons: [Option<Image<'a>>; 4],
}

/// `locations.blm.mask0` and `.mask1` (u16 a cell, row by row). mask0: 1 an encampable tile, 0x100 no tile, 8 a tile with a barbarian
/// variation `minor`, 0x200 snow, the culture bits, bits 4 .. 7 the tile's climate, then nogo's 2 and 4; mask1: the seasons' snow
/// (0x100 .. 0x800), 0x1000 a barbarian variation `small`, 0x2000 `medium`. A cell's tile is its tile layer's (its origin's for a covered cell).
pub fn masks(db: &TileDatabase, map: &TileMap, inputs: &MaskInputs) -> (Vec<u16>, Vec<u16>) {
    let (w, h) = (map.width, map.height);
    let n = (w * h) as usize;
    let (mut m0, mut m1) = (vec![0u16; n], vec![0u16; n]);
    let by_name: std::collections::HashMap<String, usize> = db
        .tiles
        .iter()
        .enumerate()
        .flat_map(|(t, tile)| (0..tile.variations.len()).map(move |v| (t, v)))
        .map(|(t, v)| (db.tile_location_of(t, v).replace('/', "\\").to_lowercase(), t))
        .collect();
    let layer = &map.layers[0];
    let culture_ok = inputs.culture.as_ref().is_some_and(|c| c.width >= w && c.height >= h);
    // The four of one size, and not both sides other than the map's (BOB's test: `width != w && height != h` fails it).
    let size = inputs.seasons[0].as_ref().map(|s| (s.width, s.height));
    let seasonal = size.is_some_and(|(sw, sh)| sw != 0 && !(sw != w && sh != h))
        && inputs.seasons.iter().all(|s| s.as_ref().map(|s| (s.width, s.height)) == size);
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            let c = layer[i];
            let origin = if c.index < 0 { layer.get((-c.index - 1) as usize).copied() } else { Some(c) };
            let tile = origin
                .filter(|o| o.index > 0)
                .and_then(|o| map.names.get(o.index as usize - 1))
                .and_then(|name| by_name.get(&name.replace('/', "\\").to_lowercase()))
                .map(|&t| &db.tiles[t]);
            match tile {
                None => m0[i] |= 0x100,
                Some(t) => {
                    if t.encampable {
                        m0[i] |= 1;
                    }

                    for v in t.variations.iter().filter(|v| v.barbarian) {
                        match v.location.as_str() {
                            "minor" => m0[i] |= 8,
                            "small" => m1[i] |= 0x1000,
                            "medium" => m1[i] |= 0x2000,
                            _ => {}
                        }
                    }
                }
            }

            if seasonal {
                for (k, img) in inputs.seasons.iter().enumerate() {
                    if img.as_ref().and_then(|s| s.at(x, y)) == Some([255; 4]) {
                        m1[i] |= 0x100 << k;
                        m0[i] |= 0x200;
                    }
                }
            } else if inputs.snow.as_ref().and_then(|s| s.at(x, y)) == Some([255; 4]) {
                m0[i] |= 0x200;
            }

            // process_terrain_internal's own test of snow_map.png (behind the culture map's width, as it is).
            if inputs.culture.as_ref().is_some_and(|c| c.width != 0)
                && let Some(s) = inputs.snow.as_ref().and_then(|s| s.at(x, y))
                && s == [255; 4]
            {
                m0[i] |= 0x200;
            }

            if culture_ok && let Some(px) = inputs.culture.as_ref().and_then(|c| c.at(x, y)) {
                for (k, bit) in [0x400u16, 0x800, 0x1000, 0x4000, 0x8000].iter().enumerate() {
                    if inputs.culture_colours.get(k) == Some(&px) {
                        m0[i] |= bit;
                    }
                }
            }

            if let Some(o) = origin.filter(|o| o.index != 0) {
                m0[i] |= (u16::from(o.climate) << 4) & 0xf0;
            }

            if let Some(px) = inputs.nogo.as_ref().and_then(|g| g.at(x, y)) {
                if px[..3] == [0, 139, 185] {
                    m0[i] |= 4;
                } else if px[..3] == [0, 174, 0] {
                    m0[i] |= 2;
                }
            }
        }
    }

    (m0, m1)
}

/// A mask's bytes.
pub fn mask_bytes(m: &[u16]) -> Vec<u8> {
    m.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// The cultures' colours from `tile_upgrades.xml` (fn 0x1002d820): the `TILE_UPGRADES` children with `id`, `red`, `green` and `blue`.
pub fn culture_colours(tile_upgrades: &[u8]) -> Vec<[u8; 4]> {
    let root = wce_tile::vegetation::ca_xml::parse(tile_upgrades);
    let find = |e: &wce_tile::vegetation::ca_xml::Element| -> Option<wce_tile::vegetation::ca_xml::Element> {
        if e.name == "TILE_UPGRADES" { Some(e.clone()) } else { e.children.iter().find(|c| c.name == "TILE_UPGRADES").cloned() }
    };
    let Some(up) = find(&root) else { return vec![] };
    up.children
        .iter()
        .filter_map(|c| {
            c.attribute("id")?;
            let n = |a: &str| c.attribute(a).and_then(|v| v.trim().parse::<f64>().ok()).map(|v| v as u8);
            Some([n("red")?, n("green")?, n("blue")?, 255])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_engines_orders() {
        let types = [
            "land_normal",
            "naval_normal",
            "land_ambush",
            "coastal_battle",
            "settlement_standard",
            "port_assault",
            "settlement_unfortified",
            "unfortified_port",
            "land_bridge",
        ];
        assert_eq!(
            hash_order(&types),
            [
                "land_normal",
                "settlement_unfortified",
                "land_ambush",
                "naval_normal",
                "port_assault",
                "coastal_battle",
                "settlement_standard",
                "unfortified_port",
                "land_bridge"
            ]
        );
        let climates = [
            "default",
            "ROME_Continental",
            "ROME_Med",
            "ROME_Atlantic",
            "ROME_Desert",
            "ROME_EasternDesert",
            "ROME_Destruct",
            "ROME_Destructdes",
        ];
        assert_eq!(
            hash_order(&climates),
            [
                "ROME_EasternDesert",
                "ROME_Destructdes",
                "ROME_Destruct",
                "default",
                "ROME_Continental",
                "ROME_Desert",
                "ROME_Med",
                "ROME_Atlantic"
            ]
        );
    }
}
