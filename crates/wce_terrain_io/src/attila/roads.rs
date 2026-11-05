//! A campaign's roads as the game shows them at its start: the kit's map paints every road `roads_climate` (level 0: `tile_map.png`'s
//! roads are all that set's colour on `main_attila_map`), and the game upgrades a road's tiles by the road level of the region it runs
//! through (docs/re/terry.md, "Roads").
//!
//! - A region's road value: the sum of the road effect (`att_effect_building_road`: the effects ending `_building_road`, scope
//!   `region_to_region_own`, no other source in the db: no technology or effect bundle gives it) of its buildings in the startpos
//!   (`REGION_SLOT`s' `BUILDING_BASE`, as [`super::cities`] reads them), `building_effects_junction`'s `value`.
//! - Its level: the last of the campaign's `campaign_map_roads` rows (by `threshold`) whose threshold the value reaches: Attila's main
//!   campaign `att_road_lv_1` 0, `att_road_lv_2` 10, `att_road_lv_3` 40 (`turns_required_to_upgrade_to` 1: on the first turn already).
//! - Its tiles: level n > 0 the `road0_to_road<n>` group of `terrain/tiles/campaign/tile_upgrades.xml` (`roads_climate` to `roads`, then
//!   to `roads_stone`; the linking tile `1x1_straight` has no upgrade and stays), the game's `FUN_1186de70` (empire.retail.dll).
//! - Where a road runs: `map_data.esf`'s `HEX_MAP_DATA` (1016 x 720 on `main_attila_map`), 7 bytes a hex cell, row 0 the south edge:
//!   byte 0 the region (an index into `REGIONS_BLOCK`; 171 there `att_reg_terra_incognita`, the sea and what is off the map), byte 5
//!   the cell's road edges (a 6-bit mask: 9, 18, 36 straight through, 5, 10, 20, 40, 17, 34 bends), byte 6 its river's.
//!
//! ASSUMPTION: a tile takes the highest level of the regions of the road cells under it (the game picks a tile's group list in
//! `FUN_11806d80` / `FUN_1181e380`, not decompiled to that point); a tile without a road cell under it the level at its centre.
//!
//! The farmland, the same way: the map's `dynamic_resources.esf` (`CAMPAIGN_DYNAMIC_RESOURCES`) lists per region (its key 0xfe00 + the
//! region's index) areas of hex cells (row 0 the south edge) with a building superchain, a level and an upgrade group
//! (`att_sch_agriculture`, 1 to 3, `farmland_mediterranean` or `farmland_northern` on `main_attila_map`). An area is farmland when
//! the region has a building of that superchain (`building_chains`' `building_superchain`) whose chain level (`building_levels`'
//! `level`: 0 for `att_bld_all_agriculture_generic_1`, 1 for the `_2`s) reaches the area's: its grass, mud and snow tiles take the
//! group's farmland tiles. ASSUMPTIONS: that comparison (the game's is not decompiled), and a tile taking the area of its centre cell.

use std::collections::HashMap;
use std::io::Cursor;

use rpfm_lib::files::Decodeable;
use rpfm_lib::files::esf::{ESF, NodeType};
use rpfm_lib::schema::Schema;
use wce_tile::battle_map::context::PlacedMap;

use super::cities::{CELL_RAW_HEIGHT, CELL_RAW_WIDTH, as_record, child, leaf, str_of, table_rows, u32_of};

/// The road set the kit's maps paint (level 0).
const BASE_SET: &str = "terrain\\tiles\\campaign\\roads_climate\\";

/// Bytes a hex cell in `HEX_MAP_DATA`.
const CELL_BYTES: usize = 7;

fn norm(s: &str) -> String {
    s.replace('/', "\\").trim_end_matches('\\').to_lowercase()
}

/// The db's road rules of a campaign.
#[derive(Clone, Debug, Default)]
pub struct RoadTables {
    /// The levels' thresholds, ascending (level 0 the first).
    pub thresholds: Vec<f64>,

    /// A building's road value.
    pub values: HashMap<String, f64>,

    /// A building's superchain and its level in its chain (`building_levels`, `building_chains`).
    pub chain_levels: HashMap<String, (String, u32)>,
}

impl RoadTables {
    /// From the db (`list` the paths under a prefix, `read` a path's bytes): `campaign`'s rows of `campaign_map_roads` and the road
    /// effect's rows of `building_effects_junction`.
    pub fn read(schema: &Schema, campaign: &str, list: &dyn Fn(&str) -> Vec<String>, read: &dyn Fn(&str) -> Option<Vec<u8>>) -> Self {
        let mut thresholds: Vec<f64> = table_rows(schema, "campaign_map_roads_tables", list, read).iter()
            .filter(|r| r.get("campaign").is_some_and(|c| c == campaign))
            .filter_map(|r| r.get("threshold")?.parse().ok())
            .collect();
        thresholds.sort_by(f64::total_cmp);
        let values = table_rows(schema, "building_effects_junction_tables", list, read).iter()
            // Attila's `att_effect_building_road`; Thrones' `vik_road_mod` (in the db, given by nothing in vanilla: its roads stay level 0).
            .filter(|r| r.get("effect").is_some_and(|e| e.ends_with("_building_road") || e.ends_with("_road_mod")))
            .filter_map(|r| Some((r.get("building")?.clone(), r.get("value")?.parse::<f64>().ok()?)))
            .fold(HashMap::new(), |mut m: HashMap<String, f64>, (b, v)| {
                *m.entry(b).or_default() += v;
                m
            });
        let superchains: HashMap<String, String> = table_rows(schema, "building_chains_tables", list, read).into_iter()
            .filter_map(|r| Some((r.get("key")?.clone(), r.get("building_superchain")?.clone())))
            .collect();
        let chain_levels = table_rows(schema, "building_levels_tables", list, read).into_iter()
            .filter_map(|r| Some((r.get("level_name")?.clone(), (superchains.get(r.get("chain")?)?.clone(), r.get("level")?.parse::<u32>().ok()?))))
            .collect();
        Self { thresholds, values, chain_levels }
    }

    /// The level a road value reaches (0 when none).
    pub fn level(&self, value: f64) -> usize {
        self.thresholds.iter().rposition(|&t| value >= t).unwrap_or(0)
    }
}

/// `tile_upgrades.xml`'s groups: per group, a tile location's upgrade (locations normalised: lower case, backslashes).
#[derive(Clone, Debug, Default)]
pub struct Upgrades {
    pub groups: HashMap<String, HashMap<String, String>>,
}

impl Upgrades {
    pub fn parse(xml: &[u8]) -> Self {
        let root = wce_tile::vegetation::ca_xml::parse(xml);
        let top = if root.name == "TILE_UPGRADES" { Some(&root) } else { root.child("TILE_UPGRADES") };
        let groups = top.into_iter().flat_map(|t| t.children.iter()).filter(|g| g.name == "UPGRADE_GROUP")
            .filter_map(|g| {
                let ups = g.children.iter().filter(|u| u.name == "UPGRADE")
                    .filter_map(|u| Some((norm(u.attribute("src")?), u.attribute("dst")?.replace('/', "\\"))))
                    .collect();
                Some((g.attribute("name")?.to_owned(), ups))
            })
            .collect();
        Self { groups }
    }
}

/// A campaign map's hex cells: their regions and roads.
#[derive(Clone, Debug, Default)]
pub struct HexMap {
    pub w: u32,
    pub h: u32,

    /// The world's size (x east, y north).
    pub world: (f64, f64),

    /// The regions' keys by index.
    pub regions: Vec<String>,
    cells: Vec<u8>,
}

impl HexMap {
    /// From `map_data.esf`.
    ///
    /// # Errors
    ///
    /// Not an ESF, or without the world, the regions or the hex cells.
    pub fn read(map_data: &[u8]) -> Result<Self, String> {
        let md = ESF::decode(&mut Cursor::new(map_data), &None).map_err(|e| format!("map_data.esf: {e}"))?;
        let root = as_record(md.root_node()).ok_or("map_data.esf: no root record")?;
        let rd = child(root, "REGIONS_DATA").ok_or("map_data.esf: no REGIONS_DATA")?;
        let world = rd.children().iter().find_map(|g| {
            g.iter().filter_map(|n| match n {
                NodeType::Coord2d(c) => Some((f64::from(*c.x()), f64::from(*c.y()))),
                _ => None,
            }).nth(1)
        }).ok_or("map_data.esf: no world size")?;
        let regions = child(rd, "REGIONS_BLOCK").into_iter().flat_map(|b| b.children().iter().flatten().filter_map(as_record))
            .filter(|r| r.name() == "REGION_DATA")
            .map(|r| r.children().first().and_then(|g| g.iter().find_map(str_of)).unwrap_or_default().to_owned())
            .collect();
        let hex = child(rd, "theatres").and_then(|t| child(t, "CAMPAIGN_THEATRE")).and_then(|t| child(t, "HEX_MAP_DATA")).ok_or("map_data.esf: no HEX_MAP_DATA")?;
        let g = hex.children().first().ok_or("map_data.esf: an empty HEX_MAP_DATA")?;
        let dims: Vec<u32> = g.iter().filter_map(u32_of).take(2).collect();
        let cells = g.iter().find_map(|n| match n {
            NodeType::U8Array(v) => Some(v.clone()),
            _ => None,
        }).ok_or("map_data.esf: HEX_MAP_DATA without its cells")?;
        let [w, h] = dims[..] else { return Err("map_data.esf: HEX_MAP_DATA without its size".to_owned()) };
        if cells.len() != w as usize * h as usize * CELL_BYTES {
            return Err(format!("map_data.esf: {} bytes of hex cells for {w} x {h}", cells.len()));
        }

        Ok(Self { w, h, world, regions, cells })
    }

    fn byte(&self, (c, r): (u32, u32), k: usize) -> u8 {
        self.cells[(r as usize * self.w as usize + c as usize) * CELL_BYTES + k]
    }

    /// A cell's region (its index).
    pub fn region(&self, cell: (u32, u32)) -> u8 {
        self.byte(cell, 0)
    }

    /// A cell's road edges (0: no road).
    pub fn road(&self, cell: (u32, u32)) -> u8 {
        self.byte(cell, 5)
    }

    /// The cell at a world position (x east, y north), as RPFM's `MapScale` lays them (odd columns half a cell up).
    pub fn cell_at(&self, x: f64, y: f64) -> Option<(u32, u32)> {
        let sx = self.world.0 / (f64::from(self.w.saturating_sub(1)) * CELL_RAW_WIDTH);
        let sy = self.world.1 / (f64::from(self.h) * CELL_RAW_HEIGHT + CELL_RAW_HEIGHT / 2.0);
        let c = (x / (CELL_RAW_WIDTH * sx)).floor();
        if !(0.0..f64::from(self.w)).contains(&c) {
            return None;
        }

        let up = if c as u32 % 2 == 1 { CELL_RAW_HEIGHT / 2.0 * sy } else { 0.0 };
        let r = ((y - up) / (CELL_RAW_HEIGHT * sy)).floor();
        (0.0..f64::from(self.h)).contains(&r).then_some((c as u32, r as u32))
    }
}

/// Each region's buildings in a startpos (every slot's, a slot with a cell on the map or not).
///
/// # Errors
///
/// Not an ESF, or without the world's regions.
pub fn region_buildings(startpos: &[u8]) -> Result<HashMap<String, Vec<String>>, String> {
    let sp = ESF::decode(&mut Cursor::new(startpos), &None).map_err(|e| format!("startpos.esf: {e}"))?;
    let root = as_record(sp.root_node()).ok_or("startpos.esf: no root record")?;
    let world = ["CAMPAIGN_ENV", "CAMPAIGN_MODEL", "WORLD"].iter().try_fold(root, |r, n| child(r, n)).ok_or("startpos.esf: no WORLD")?;
    let regions = child(world, "REGION_MANAGER").and_then(|m| child(m, "REGIONS_ARRAY")).ok_or("startpos.esf: no REGIONS_ARRAY")?;
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for region in regions.children().iter().flatten().filter_map(as_record).filter(|r| r.name() == "REGION") {
        for g in region.children() {
            let Some(key) = leaf(g, None, 2).and_then(str_of) else { continue };
            let Some(manager) = g.iter().filter_map(as_record).find(|r| r.name() == "REGION_SLOT_MANAGER") else { continue };
            let list = out.entry(key.to_owned()).or_default();
            let slots = child(manager, "REGION_SLOT_ARRAY").into_iter().flat_map(|a| a.children().iter().flatten().filter_map(as_record)).filter(|r| r.name() == "REGION_SLOT");
            for slot in slots {
                list.extend(super::cities::slot_of(slot).1);
            }
        }
    }

    Ok(out)
}

/// An area of `dynamic_resources.esf`: hex cells of a region that take an upgrade group once the region's buildings of a superchain
/// reach a level.
#[derive(Clone, Debug, PartialEq)]
pub struct ResourceArea {
    pub region: usize,
    pub superchain: String,
    pub level: u32,
    pub group: String,
    pub cells: Vec<(u16, u16)>,
}

/// The areas of a campaign map's `dynamic_resources.esf`.
///
/// # Errors
///
/// Not an ESF, or not `CAMPAIGN_DYNAMIC_RESOURCES`.
pub fn dynamic_resources(esf: &[u8]) -> Result<Vec<ResourceArea>, String> {
    let e = ESF::decode(&mut Cursor::new(esf), &None).map_err(|e| format!("dynamic_resources.esf: {e}"))?;
    let root = as_record(e.root_node()).filter(|r| r.name() == "CAMPAIGN_DYNAMIC_RESOURCES").ok_or("dynamic_resources.esf: not CAMPAIGN_DYNAMIC_RESOURCES")?;
    let mut out = vec![];
    for info in root.children().iter().flatten().filter_map(as_record).filter(|r| r.name() == "REGION_RESOURCE_INFO_BLOCK") {
        for g in info.children() {
            let Some(id) = g.iter().find_map(u32_of) else { continue };
            let region = id.wrapping_sub(0xfe00) as usize;
            for areas in g.iter().filter_map(as_record).filter(|r| r.name() == "REGION_RESOURCE_INFO_AREAS_BLOCK") {
                for ag in areas.children() {
                    let mut texts = ag.iter().filter_map(str_of);
                    let (Some(superchain), Some(group)) = (texts.next(), texts.next()) else { continue };
                    let level = ag.iter().find_map(u32_of).unwrap_or(0);
                    let cells = ag.iter().filter_map(as_record).filter(|r| r.name() == "REGION_RESOURCE_AREAS_BLOCK")
                        .flat_map(|r| r.children().iter().flatten())
                        .filter_map(|n| match n {
                            NodeType::U16Array(v) => Some(v.as_chunks::<2>().0.iter().map(|c| (c[0], c[1])).collect::<Vec<_>>()),
                            _ => None,
                        })
                        .flatten()
                        .collect();
                    out.push(ResourceArea { region, superchain: superchain.to_owned(), level, group: group.to_owned(), cells });
                }
            }
        }
    }

    Ok(out)
}

/// The roads and farmland of a campaign at its start: each region's road level, the farmland's cells, and the swaps that draw them.
#[derive(Clone, Debug, Default)]
pub struct CampaignRoads {
    pub hex: HexMap,

    /// Per region index its value and level.
    pub values: Vec<f64>,
    pub levels: Vec<usize>,
    pub upgrades: Upgrades,

    /// The farmland's hex cells and their upgrade groups.
    pub farms: HashMap<(u32, u32), String>,

    /// The resource areas reached and not.
    pub areas_on: usize,
    pub areas_off: usize,
}

impl CampaignRoads {
    pub fn new(hex: HexMap, buildings: &HashMap<String, Vec<String>>, tables: &RoadTables, upgrades: Upgrades, areas: &[ResourceArea]) -> Self {
        let values: Vec<f64> = hex.regions.iter()
            .map(|r| buildings.get(r).map_or(0.0, |b| b.iter().filter_map(|b| tables.values.get(b)).sum()))
            .collect();
        let levels = values.iter().map(|&v| tables.level(v)).collect();
        let mut farms = HashMap::new();
        let (mut areas_on, mut areas_off) = (0, 0);
        for a in areas {
            // The region's highest level in the superchain.
            let reached = hex.regions.get(a.region).and_then(|r| buildings.get(r)).into_iter().flatten()
                .filter_map(|b| tables.chain_levels.get(b).filter(|(sc, _)| *sc == a.superchain).map(|(_, l)| *l))
                .max();
            if reached.is_some_and(|l| l >= a.level) {
                areas_on += 1;
                farms.extend(a.cells.iter().map(|&(c, r)| ((u32::from(c), u32::from(r)), a.group.clone())));
            } else {
                areas_off += 1;
            }
        }

        Self { hex, values, levels, upgrades, farms, areas_on, areas_off }
    }

    /// How many regions are at each level.
    pub fn level_counts(&self) -> Vec<usize> {
        let mut n = vec![0; self.levels.iter().max().map_or(1, |m| m + 1)];
        for &l in &self.levels {
            n[l] += 1;
        }

        n
    }

    /// A region's level by key.
    pub fn level_of(&self, region: &str) -> Option<usize> {
        self.hex.regions.iter().position(|r| r == region).map(|i| self.levels[i])
    }

    /// The level a tile placed over the map's cells `[x0, x1) x [y0, y1)` is drawn at (`w` x `h` the map's cells, row 0 the north edge).
    pub fn tile_level(&self, rect: [i32; 4], (w, h): (u32, u32)) -> usize {
        let world = |x: f64, y: f64| (x / f64::from(w) * self.hex.world.0, (1.0 - y / f64::from(h)) * self.hex.world.1);
        let level = |cell: (u32, u32)| self.levels.get(usize::from(self.hex.region(cell))).copied().unwrap_or(0);
        let mut best = None;
        for y in rect[1]..rect[3] {
            for x in rect[0]..rect[2] {
                let (wx, wy) = world(f64::from(x) + 0.5, f64::from(y) + 0.5);
                if let Some(cell) = self.hex.cell_at(wx, wy).filter(|&c| self.hex.road(c) != 0) {
                    best = Some(best.unwrap_or(0).max(level(cell)));
                }
            }
        }

        best.unwrap_or_else(|| {
            let (wx, wy) = world(f64::from(rect[0] + rect[2]) / 2.0, f64::from(rect[1] + rect[3]) / 2.0);
            self.hex.cell_at(wx, wy).map_or(0, level)
        })
    }

    /// The farmland group of the hex cell at a tile's centre (`w` x `h` the map's cells, row 0 the north edge).
    pub fn tile_farm(&self, rect: [i32; 4], (w, h): (u32, u32)) -> Option<&str> {
        let (x, y) = (f64::from(rect[0] + rect[2]) / 2.0, f64::from(rect[1] + rect[3]) / 2.0);
        let cell = self.hex.cell_at(x / f64::from(w) * self.hex.world.0, (1.0 - y / f64::from(h)) * self.hex.world.1)?;
        self.farms.get(&cell).map(String::as_str)
    }

    /// The map with its road tiles swapped for their level's and the tiles on farmland for its group's (the others as they are); the
    /// numbers of road and of farmland tiles swapped.
    pub fn apply(&self, m: &PlacedMap) -> (PlacedMap, usize, usize) {
        let mut out = m.clone();
        let (mut n, mut farmed) = (0, 0);
        for p in &mut out.placements {
            let loc = norm(&p.location);
            if !loc.starts_with(BASE_SET) {
                if !self.farms.is_empty()
                    && let Some(dst) = self.tile_farm(p.rect(), (m.width, m.height)).and_then(|g| self.upgrades.groups.get(g)).and_then(|g| g.get(&loc))
                {
                    p.location.clone_from(dst);
                    farmed += 1;
                }

                continue;
            }

            let level = self.tile_level(p.rect(), (m.width, m.height));
            if level == 0 {
                continue;
            }

            if let Some(dst) = self.upgrades.groups.get(&format!("road0_to_road{level}")).and_then(|g| g.get(&loc)) {
                p.location.clone_from(dst);
                n += 1;
            }
        }

        (out, n, farmed)
    }
}

/// A campaign's roads and farmland from its files: `map_data.esf`, the startpos, `tile_upgrades.xml`, the db, and the map's
/// `dynamic_resources.esf` when it has one.
///
/// # Errors
///
/// A file can't be read as it should.
pub fn read(map_data: &[u8], startpos: &[u8], tile_upgrades: &[u8], tables: &RoadTables, dynamic: Option<&[u8]>) -> Result<CampaignRoads, String> {
    let areas = dynamic.map(dynamic_resources).transpose()?.unwrap_or_default();
    Ok(CampaignRoads::new(HexMap::read(map_data)?, &region_buildings(startpos)?, tables, Upgrades::parse(tile_upgrades), &areas))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_by_threshold() {
        let t = RoadTables { thresholds: vec![0.0, 10.0, 40.0], ..Default::default() };
        assert_eq!([t.level(0.0), t.level(9.9), t.level(10.0), t.level(39.0), t.level(40.0), t.level(95.0)], [0, 0, 1, 1, 2, 2]);
    }

    #[test]
    fn upgrades_parse_by_group() {
        let xml = b"<TILE_UPGRADES>\n<UPGRADE_GROUP name='road0_to_road2'>\n<UPGRADE src='terrain\\tiles\\campaign\\roads_climate\\2x2_endpiece_l' dst='terrain\\tiles\\campaign\\roads_stone\\2x2_endpiece_l' />\n</UPGRADE_GROUP>\n</TILE_UPGRADES>";
        let u = Upgrades::parse(xml);
        assert_eq!(u.groups["road0_to_road2"]["terrain\\tiles\\campaign\\roads_climate\\2x2_endpiece_l"], "terrain\\tiles\\campaign\\roads_stone\\2x2_endpiece_l");
    }

    #[test]
    fn cells_by_world_position() {
        let hex = HexMap { w: 4, h: 3, world: (18.0, 28.0), regions: vec![], cells: vec![0; 4 * 3 * CELL_BYTES] };
        // 6 x 8 raw units a cell, the world 18 x 28 raw (3 columns' width past the first, 3.5 rows): a raw unit a world unit.
        assert_eq!(hex.cell_at(1.0, 1.0), Some((0, 0)));
        // Column 1 (odd) starts half a cell up: y 3 is below its first row, 5 in it.
        assert_eq!(hex.cell_at(7.0, 3.0), None);
        assert_eq!(hex.cell_at(7.0, 5.0), Some((1, 0)));
        assert_eq!(hex.cell_at(13.0, 17.0), Some((2, 2)));
        assert_eq!(hex.cell_at(-1.0, 1.0), None);
    }
}
