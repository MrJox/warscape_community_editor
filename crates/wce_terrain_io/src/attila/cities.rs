//! A campaign's settlements as the game shows them at its start: each settlement slot's building, from the startpos, as the model the
//! settlement display tables give it, at the slot's cell of the campaign map. Attila's and Thrones' startposes (the same records) and
//! Rome 2's (its slots' `BUILDING_MANAGER/BUILDING`, which names the building's faction; its display tables link a building level to
//! a model by the id row's `key` and `display_building_key`): `cities_check` on `main_attila` 517 models, `vik_attila` 353,
//! Rome 2's `main_rome` (map `main_rome_map`) 316.
//!
//! The sources, as RPFM's startpos code reads them (`rpfm_extensions::startpos`: `settlements`, `extract::regions`, `coords`):
//! - the campaign map's `map_data.esf`: the world's size (`REGIONS_DATA`'s second `Coord2d`; 678.5 x 556.14 on `main_attila_map`), the hex
//!   grid (`HEX_MAP_DATA`: 1016 x 720), and per region with a settlement (`REGION_DATA/SETTLEMENT_INFO`) its slots' cells: the main
//!   slot and the secondary ones (`SLOT_ARRAY_BLOCK`'s groups, each its first `SLOT_POSITIONS_BLOCK` cell), the port
//!   (`PORT_POSITIONS_BLOCK`);
//! - the startpos (`data/campaigns/<campaign>/startpos.esf`, a loose file): per region (`WORLD/REGION_MANAGER/REGIONS_ARRAY/REGION`) its
//!   key (`^+2`), its owner (`SETTLEMENT+9`, a faction's runtime id: `FACTION` `^+1`, its key `^+2`) and its slots
//!   (`REGION_SLOT_MANAGER/REGION_SLOT_ARRAY/REGION_SLOT`: the type `REGION_BUILDING_MANAGER+3`, the building
//!   `REGION_BUILDING_MANAGER/BUILDING_BASE` `^+2`), the main slot first, then the port, then the secondary ones;
//! - the db: `factions` (subculture), `cultures_subcultures` (culture), `campaign_settlement_display_building_ids` (a building level's
//!   model id, by faction, subculture or culture, else for all), `campaign_settlement_display_buildings` (a model id's file, its
//!   normal state: not slum, sieged or blockaded).
//!
//! A slot's cell: the map data's by the slot's kind (`primary` the main cell, `port` the port's, `secondary` the next secondary one: a
//! region without a port has no port slot, its secondary slots start at index 1), else the startpos's own
//! (`SETTLEMENT/SETTLEMENT_EXPANSION_MANAGER/SETTLEMENT_EXPANSION_MAP_DATA`'s `SLOT_MAP_DATA`: index 0 the main slot, 1 the port, then
//! the secondary ones; 65535 unset). Both agree on all 745 slots of `main_attila` that have both.
//!
//! Aqueducts: `campaign_settlement_display_aqueducts` (a model, a world position, a clockwise rotation, by region), shown for the regions
//! with a building of `has_aqueduct` (`main_attila`: Ravenna's one of 62 rows).
//!
//! A leaf's name is RPFM's: `NAME+n` the n-th leaf after the record `NAME` in its group (records skipped), `^+n` the n-th before any.
//!
//! Positions: a cell's world position is its top-left corner (RPFM's `MapScale::world_position`; odd columns half a cell down), x east and
//! y north over the world's size, the same frame as the forests' `trees.campaign_tree_list` (`terrain_tool::prop_models`). ASSUMPTION:
//! a model stands at its cell's centre, unturned; the game's own placement (orientation, the settlement's attachment points, its sprawl
//! and walls) is not read yet.

use std::collections::HashMap;
use std::io::Cursor;

use rpfm_lib::files::db::DB;
use rpfm_lib::files::esf::{ESF, NodeType, RecordNode};
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use rpfm_lib::schema::Schema;

/// Raw map units a cell, across and down; raw units past the world's origin.
pub(crate) const CELL_RAW_WIDTH: f64 = 6.0;
pub(crate) const CELL_RAW_HEIGHT: f64 = 8.0;

/// A slot of a settlement.
#[derive(Clone, Debug, PartialEq)]
pub struct CitySlot {
    pub region: String,

    /// Its place in the region's slots (0 the main settlement, 1 the port, then the secondary ones).
    pub index: usize,

    /// The slot's type (`REGION_BUILDING_MANAGER+3`: `primary`, `port`, `secondary`...).
    pub kind: String,

    /// Its hex cell (column, row).
    pub cell: (u16, u16),

    /// Its cell's centre in world units (x east, y north).
    pub world: (f64, f64),

    /// The building in it, if any.
    pub building: Option<String>,

    /// The model the display tables give the building for the region's owner (a packs path, lower case, `/`).
    pub model: Option<String>,
}

/// A campaign's settlements.
#[derive(Clone, Debug, Default)]
pub struct Cities {
    /// The world's size (x, y).
    pub world: (f64, f64),

    /// The hex grid (columns, rows).
    pub grid: (u32, u32),

    /// World units a settlement model unit (the models' footprint decal against the map's hexes: 0.405 on `main_attila_map`).
    pub model_scale: f64,

    pub slots: Vec<CitySlot>,

    /// The aqueducts of the regions with an aqueduct building.
    pub aqueducts: Vec<Aqueduct>,

    /// The settlements' sprawl at the start (ASSUMPTION: every sprawl cell in the zone of a slot with a building, at a level from its tier).
    pub sprawl: Vec<SprawlPiece>,

    /// What was not found (counts by reason).
    pub notes: Vec<String>,
}

/// A sprawl piece: a sprawl cell's model, turned by `turn` sixths of a turn clockwise seen from above (the game's `FUN_10b29d90`: the
/// hex's rotation byte times 60 degrees).
#[derive(Clone, Debug, PartialEq)]
pub struct SprawlPiece {
    pub region: String,
    pub cell: (u16, u16),
    pub world: (f64, f64),
    /// `sprawl_edge_<shape digits>`, `sea_sprawl_edge_<digits>`, `sea_sprawl_default`.
    pub piece: String,
    pub level: u32,
    pub turn: u32,
    pub model: Option<String>,
}

/// A building level's tier: the number its key ends with (`att_bld_roman_city_major_3`: 3; 1 without one).
fn tier_of(building: &str) -> u32 {
    building.rsplit('_').next().and_then(|t| t.parse().ok()).unwrap_or(1)
}

/// A piece's name for an edge shape (`FUN_10af6ae0`): its edges' numbers, 1 to 6 (`sprawl_edge_0` without one).
fn shape_digits(shape: u32) -> String {
    let d: String = (0..6).filter(|k| shape & (1 << k) != 0).map(|k| char::from(b'1' + k as u8)).collect();
    if d.is_empty() { "0".to_owned() } else { d }
}

/// An aqueduct of `campaign_settlement_display_aqueducts`: shown while its region has a building with `has_aqueduct`
/// (`building_levels`: `att_bld_roman_water_aqueducts_2` to `_4`).
#[derive(Clone, Debug, PartialEq)]
pub struct Aqueduct {
    pub region: String,

    /// A packs path, lower case, `/`.
    pub model: String,

    /// World units (x east, y north), as the slots'.
    pub world: (f64, f64),

    /// `position_z_height` (not used: the model stands on the ground).
    pub height: f64,

    /// Clockwise seen from above, radians.
    pub rotation: f64,
}

pub(crate) fn as_record(n: &NodeType) -> Option<&RecordNode> {
    match n {
        NodeType::Record(r) => Some(r),
        _ => None,
    }
}

pub(crate) fn child<'a>(r: &'a RecordNode, name: &str) -> Option<&'a RecordNode> {
    r.children().iter().flatten().filter_map(as_record).find(|c| c.name() == name)
}

pub(crate) fn u32_of(n: &NodeType) -> Option<u32> {
    match n {
        NodeType::U8(v) => Some(u32::from(*v)),
        NodeType::U16(v) => Some(u32::from(*v)),
        NodeType::U32(v) => Some(*v.value()),
        NodeType::I32(v) => u32::try_from(*v.value()).ok(),
        _ => None,
    }
}

pub(crate) fn str_of(n: &NodeType) -> Option<&str> {
    match n {
        NodeType::Ascii(v) | NodeType::Utf16(v) => Some(v),
        _ => None,
    }
}

/// The `n`-th leaf (1-based) after the record `after` in a group (`None`: before any record), records skipped (RPFM's `NAME+n`, `^+n`).
pub(crate) fn leaf<'a>(group: &'a [NodeType], after: Option<&str>, n: usize) -> Option<&'a NodeType> {
    let start = match after {
        None => 0,
        Some(name) => group.iter().position(|c| as_record(c).is_some_and(|r| r.name() == name))? + 1,
    };

    group[start..].iter().take_while(|c| after.is_some() || as_record(c).is_none()).filter(|c| as_record(c).is_none()).nth(n - 1)
}

/// The first cell of a record of one cell a group (two `U16` leaves each).
fn first_cell(r: &RecordNode) -> Option<(u16, u16)> {
    r.children().iter().find_map(|g| {
        let mut v = g.iter().filter_map(|n| match n {
            NodeType::U16(x) => Some(*x),
            _ => None,
        });
        Some((v.next()?, v.next()?))
    })
}

/// A startpos slot's type (`primary`, `port`, `secondary`...), its building and, when the slot names it, the building's faction:
/// Attila's and Thrones' `REGION_BUILDING_MANAGER+3` and `REGION_BUILDING_MANAGER/BUILDING_BASE ^+2`; Rome 2's `BUILDING_MANAGER+3` and
/// `BUILDING_MANAGER/BUILDING` `^+4` (its faction `^+5`).
pub(crate) fn slot_of(slot: &RecordNode) -> (String, Option<String>, Option<String>) {
    let Some(sg) = slot.children().first() else { return (String::new(), None, None) };
    let text = |n: Option<&NodeType>| n.and_then(str_of).filter(|s| !s.is_empty()).map(str::to_owned);
    if let Some(m) = child(slot, "REGION_BUILDING_MANAGER") {
        let building = child(m, "BUILDING_BASE").and_then(|b| b.children().first().and_then(|bg| text(leaf(bg, None, 2))));
        return (text(leaf(sg, Some("REGION_BUILDING_MANAGER"), 3)).unwrap_or_default(), building, None);
    }

    if let Some(m) = child(slot, "BUILDING_MANAGER") {
        let b = child(m, "BUILDING").and_then(|b| b.children().first());
        return (
            text(leaf(sg, Some("BUILDING_MANAGER"), 3)).unwrap_or_default(),
            b.and_then(|bg| text(leaf(bg, None, 4))),
            b.and_then(|bg| text(leaf(bg, None, 5))),
        );
    }

    (String::new(), None, None)
}

/// The neighbour of a hex cell in a direction (RPFM's `coords.rs`: 0 the next row, then clockwise; odd columns half a row up).
fn hex_neighbour((c, r): (u16, u16), d: u32) -> Option<(u16, u16)> {
    const ODD: [(i32, i32); 6] = [(0, 1), (1, 1), (1, 0), (0, -1), (-1, 0), (-1, 1)];
    const EVEN: [(i32, i32); 6] = [(0, 1), (1, 0), (1, -1), (0, -1), (-1, -1), (-1, 0)];
    let (dx, dy) = if c % 2 == 1 { ODD } else { EVEN }.get(d as usize).copied()?;
    Some((u16::try_from(i32::from(c) + dx).ok()?, u16::try_from(i32::from(r) + dy).ok()?))
}

/// The direction of a neighbouring cell.
fn hex_direction(a: (u16, u16), b: (u16, u16)) -> Option<u32> {
    (0..6).find(|&d| hex_neighbour(a, d) == Some(b))
}

/// An edge mask as a piece shape and a rotation (RPFM's `EdgePiece::from_mask`: the smallest rotation with its first two edges set, else
/// the smallest; the rotation the smallest turning it into the mask).
fn edge_piece(mask: u32) -> (u32, u32) {
    let rot = |m: u32, k: u32| ((m << k) | (m >> ((6 - k) % 6))) & 63;
    let shape =
        (0..6).map(|k| rot(mask, k)).filter(|m| m & 3 == 3).min().unwrap_or_else(|| (0..6).map(|k| rot(mask, k)).min().unwrap_or(0));
    (shape, (0..6).find(|&k| rot(shape, k) == mask).unwrap_or(0))
}

/// Every slot's wall walk in the startpos (`SETTLEMENT_EXPANSION_MAP_DATA`'s `SLOT_MAP_DATA` and its `SECONDARY_SLOT_ARRAY`'s, their
/// `SLOT_WALL_ARRAY`), each in order; none of fewer than three cells.
fn wall_walks(data: &RecordNode) -> Vec<Vec<(u16, u16)>> {
    let walk = |slot: &RecordNode| -> Option<Vec<(u16, u16)>> {
        let walls = child(slot, "SLOT_WALL_ARRAY")?;
        let cells: Vec<(u16, u16)> = walls
            .children()
            .iter()
            .filter_map(|g| {
                let mut v = g.iter().filter_map(|n| match n {
                    NodeType::U16(x) => Some(*x),
                    _ => None,
                });
                Some((v.next()?, v.next()?))
            })
            .collect();
        (cells.len() > 2).then_some(cells)
    };

    let records = data.children().iter().flatten().filter_map(as_record);
    let primary = records.clone().filter(|r| r.name() == "SLOT_MAP_DATA");
    let secondary = records
        .filter(|r| r.name() == "SECONDARY_SLOT_ARRAY")
        .flat_map(|a| a.children().iter().flatten().filter_map(as_record))
        .filter(|r| r.name() == "SLOT_MAP_DATA");
    primary.chain(secondary).filter_map(walk).collect()
}

/// A settlement's slot cells in the startpos (`SETTLEMENT/SETTLEMENT_EXPANSION_MANAGER/SETTLEMENT_EXPANSION_MAP_DATA`: its
/// `SLOT_MAP_DATA`, the main slot and the port, and its `SECONDARY_SLOT_ARRAY`'s): the slot's index (the leading u32) -> its cell.
fn startpos_slot_cells(settlement: &RecordNode) -> HashMap<usize, (u16, u16)> {
    let mut out = HashMap::new();
    let Some(data) = child(settlement, "SETTLEMENT_EXPANSION_MANAGER").and_then(|m| child(m, "SETTLEMENT_EXPANSION_MAP_DATA")) else {
        return out;
    };
    let mut take = |r: &RecordNode| {
        for g in r.children() {
            let mut v = g.iter().filter(|n| as_record(n).is_none());
            let (Some(i), Some(NodeType::U16(c)), Some(NodeType::U16(row))) = (v.next().and_then(u32_of), v.next(), v.next()) else {
                continue;
            };
            out.insert(i as usize, (*c, *row));
        }
    };

    for r in data.children().iter().flatten().filter_map(as_record) {
        match r.name().as_str() {
            "SLOT_MAP_DATA" => take(r),
            "SECONDARY_SLOT_ARRAY" => {
                r.children().iter().flatten().filter_map(as_record).filter(|s| s.name() == "SLOT_MAP_DATA").for_each(&mut take)
            }
            _ => {}
        }
    }

    out
}

/// A db table's rows by column name (every file of `db/<table>/`; `read` gives a path's bytes, `list` the paths under a prefix).
pub fn table_rows(
    schema: &Schema,
    table: &str,
    list: &dyn Fn(&str) -> Vec<String>,
    read: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Vec<HashMap<String, String>> {
    let mut out = vec![];
    for path in list(&format!("db/{table}/")) {
        let Some(bytes) = read(&path) else { continue };
        let mut extra = DecodeableExtraData::default();
        extra.set_schema(Some(schema));
        extra.set_table_name(Some(table));
        let Ok(db) = DB::decode(&mut Cursor::new(&bytes), &Some(extra)) else { continue };
        let names: Vec<String> = db.definition().fields_processed().iter().map(|f| f.name().to_owned()).collect();
        for row in db.data().iter() {
            out.push(names.iter().cloned().zip(row.iter().map(|c| c.data_to_string().into_owned())).collect());
        }
    }

    out
}

/// The tables the models are chosen with.
#[derive(Clone, Debug, Default)]
pub struct DisplayTables {
    /// faction -> subculture; subculture -> culture.
    pub subculture: HashMap<String, String>,
    pub culture: HashMap<String, String>,

    /// building level -> (faction, subculture, culture, model id) rows.
    pub ids: HashMap<String, Vec<[String; 4]>>,

    /// (sprawl piece type, level) -> (faction, subculture, culture, model id) rows (the same table's `sprawl_piece_type` rows).
    pub sprawl: HashMap<(String, u32), Vec<[String; 4]>>,

    /// model id -> model path (its normal state).
    pub models: HashMap<String, String>,

    /// The aqueducts by region (`campaign_settlement_display_aqueducts`), and the building levels with `has_aqueduct`.
    pub aqueducts: Vec<Aqueduct>,
    pub with_aqueduct: std::collections::HashSet<String>,
}

impl DisplayTables {
    /// The tables of the game's packs.
    pub fn read(schema: &Schema, list: &dyn Fn(&str) -> Vec<String>, read: &dyn Fn(&str) -> Option<Vec<u8>>) -> Self {
        let rows = |t: &str| table_rows(schema, t, list, read);
        let get = |r: &HashMap<String, String>, k: &str| r.get(k).cloned().unwrap_or_default();
        let mut t = Self::default();
        for r in rows("factions_tables") {
            t.subculture.insert(get(&r, "key"), get(&r, "subculture"));
        }

        for r in rows("cultures_subcultures_tables") {
            t.culture.insert(get(&r, "subculture"), get(&r, "culture"));
        }

        for r in rows("campaign_settlement_display_building_ids_tables") {
            let level = get(&r, "building_level_key");
            // Attila's row names its model id; Rome 2's row key is its model id (`display_building_key` of the next table).
            let model = Some(get(&r, "building_model_id")).filter(|m| !m.is_empty()).unwrap_or_else(|| get(&r, "key"));
            let row = [get(&r, "faction"), get(&r, "sub_culture"), get(&r, "culture"), model];
            let piece = get(&r, "sprawl_piece_type");
            if !piece.is_empty() {
                t.sprawl.entry((piece, get(&r, "sprawl_piece_level").parse().unwrap_or(0))).or_default().push(row.clone());
            }

            if !level.is_empty() {
                t.ids.entry(level).or_default().push(row);
            }
        }

        for r in rows("campaign_settlement_display_buildings_tables") {
            let flagged = ["is_slum", "is_sieged", "is_blockaded"].iter().any(|k| get(&r, k) == "true");
            let path = get(&r, "building_path").replace('\\', "/").to_lowercase();
            let id = Some(get(&r, "model_id")).filter(|m| !m.is_empty()).unwrap_or_else(|| get(&r, "display_building_key"));
            if !flagged && !path.is_empty() {
                t.models.entry(id).or_insert(path);
            }
        }

        let num = |r: &HashMap<String, String>, k: &str| r.get(k).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
        for r in rows("campaign_settlement_display_aqueducts_tables") {
            t.aqueducts.push(Aqueduct {
                region: get(&r, "region_key"),
                model: get(&r, "building_path").replace('\\', "/").to_lowercase(),
                world: (num(&r, "position_x_map"), num(&r, "position_y_map")),
                height: num(&r, "position_z_height"),
                rotation: num(&r, "rotation_cw_radians"),
            });
        }

        t.with_aqueduct = rows("building_levels_tables")
            .into_iter()
            .filter(|r| get(r, "has_aqueduct") == "true")
            .map(|r| get(&r, "level_name"))
            .collect();
        t
    }

    /// The model of a building level for a faction: the row of its faction, else its subculture, else its culture, else for all.
    pub fn sprawl_model(&self, piece: &str, level: u32, faction: &str) -> Option<&str> {
        self.pick(self.sprawl.get(&(piece.to_owned(), level))?, faction)
    }

    pub fn model(&self, building: &str, faction: &str) -> Option<&str> {
        self.pick(self.ids.get(building)?, faction)
    }

    /// The model of rows for a faction: the row of its faction, else its subculture, else its culture, else for all.
    fn pick(&self, rows: &[[String; 4]], faction: &str) -> Option<&str> {
        let sub = self.subculture.get(faction).map_or("", String::as_str);
        let cult = self.culture.get(sub).map_or("", String::as_str);
        let pick = |f: &dyn Fn(&[String; 4]) -> bool| rows.iter().find(|r| f(r));
        let row = pick(&|r| !r[0].is_empty() && r[0] == faction)
            .or_else(|| pick(&|r| r[0].is_empty() && !r[1].is_empty() && r[1] == sub))
            .or_else(|| pick(&|r| r[0].is_empty() && r[1].is_empty() && !r[2].is_empty() && r[2] == cult))
            .or_else(|| pick(&|r| r[0].is_empty() && r[1].is_empty() && r[2].is_empty()))?;
        self.models.get(&row[3]).map(String::as_str)
    }
}

/// The settlements of a campaign from its map's `map_data.esf`, its startpos and the display tables.
///
/// # Errors
///
/// A file is not an ESF, or the map data has no world.
pub fn read(map_data: &[u8], startpos: &[u8], tables: &DisplayTables) -> Result<Cities, String> {
    let md = ESF::decode(&mut Cursor::new(map_data), &None).map_err(|e| format!("map_data.esf: {e}"))?;
    let sp = ESF::decode(&mut Cursor::new(startpos), &None).map_err(|e| format!("startpos.esf: {e}"))?;
    let md_root = as_record(md.root_node()).ok_or("map_data.esf: no root record")?;
    let regions_data = child(md_root, "REGIONS_DATA").ok_or("map_data.esf: no REGIONS_DATA")?;
    let world = regions_data
        .children()
        .iter()
        .find_map(|g| {
            g.iter()
                .filter_map(|n| match n {
                    NodeType::Coord2d(c) => Some((f64::from(*c.x()), f64::from(*c.y()))),
                    _ => None,
                })
                .nth(1)
        })
        .ok_or("map_data.esf: no world size")?;
    let grid = child(regions_data, "theatres")
        .and_then(|t| child(t, "CAMPAIGN_THEATRE"))
        .and_then(|t| child(t, "HEX_MAP_DATA"))
        .and_then(|h| h.children().first().map(|g| g.iter().filter_map(u32_of).take(2).collect::<Vec<_>>()))
        .filter(|v| v.len() == 2)
        .map(|v| (v[0], v[1]))
        .ok_or("map_data.esf: no hex grid")?;

    // RPFM's MapScale: world units a raw unit, across and down.
    let raw_w = f64::from(grid.0.saturating_sub(1)) * CELL_RAW_WIDTH;
    let raw_h = f64::from(grid.1) * CELL_RAW_HEIGHT + CELL_RAW_HEIGHT / 2.0;
    let (sx, sy) = (world.0 / raw_w, world.1 / raw_h);
    // A cell's centre: RPFM's `world_position` (raw = world / scale + 4, a hex 8 raw units across: its centre is the raw origin of its
    // column and row, 4 in, so the world position of column * 6 and row * 8 raw units, odd columns half a row up).
    let centre = |(c, r): (u16, u16)| {
        let x = f64::from(c) * CELL_RAW_WIDTH * sx;
        let y = f64::from(r) * CELL_RAW_HEIGHT * sy + if c % 2 == 1 { CELL_RAW_HEIGHT / 2.0 * sy } else { 0.0 };
        (x, y)
    };

    // The map's slots by region: main, port (or none), the secondary ones.
    let mut map_slots: HashMap<String, Vec<Option<(u16, u16)>>> = HashMap::new();
    if let Some(block) = child(regions_data, "REGIONS_BLOCK") {
        for region in block.children().iter().flatten().filter_map(as_record).filter(|r| r.name() == "REGION_DATA") {
            let Some(key) = region.children().first().and_then(|g| g.iter().find_map(str_of)) else { continue };
            let Some(info) = child(region, "SETTLEMENT_INFO") else { continue };
            // A group of the slot array a slot (RPFM's `map_slot`): its first position.
            let slots: Vec<(u16, u16)> = child(info, "SLOT_ARRAY_BLOCK")
                .into_iter()
                .flat_map(|b| b.children().iter())
                .filter_map(|g| g.iter().filter_map(as_record).find(|r| r.name() == "SLOT_POSITIONS_BLOCK").and_then(first_cell))
                .collect();
            let port = child(info, "PORT_POSITIONS_BLOCK").and_then(first_cell);
            let mut v = vec![slots.first().copied(), port];
            v.extend(slots.iter().skip(1).map(|&c| Some(c)));
            map_slots.insert(key.to_owned(), v);
        }
    }

    // The startpos's factions and regions.
    let sp_root = as_record(sp.root_node()).ok_or("startpos.esf: no root record")?;
    let world_rec =
        ["CAMPAIGN_ENV", "CAMPAIGN_MODEL", "WORLD"].iter().try_fold(sp_root, |r, n| child(r, n)).ok_or("startpos.esf: no WORLD")?;
    let mut factions: HashMap<u32, String> = HashMap::new();
    if let Some(array) = child(world_rec, "FACTION_ARRAY") {
        for f in array.children().iter().flatten().filter_map(as_record).filter(|r| r.name() == "FACTION") {
            for g in f.children() {
                if let (Some(id), Some(key)) = (leaf(g, None, 1).and_then(u32_of), leaf(g, None, 2).and_then(str_of)) {
                    factions.insert(id, key.to_owned());
                }
            }
        }
    }

    // The settlement models' scale: their ground decal (`ProjectedDecalV2`, 5.51 x 5.72 model units on every settlement and port model
    // measured) is the slot's footprint, the anchor cell and its six neighbours, of hexes 1.1 model units a side; the map's hexes are
    // regular in world units (6 * sx = 1.5 sides, 8 * sy = sqrt(3) sides: `main_attila_map` 0.4457 a side), so a model unit is
    // 0.4457 / 1.1 = 0.405 world units.
    let model_scale = CELL_RAW_WIDTH * sx / 1.5 / 1.1;
    let mut cities = Cities { world, grid, model_scale, ..Default::default() };
    let (mut no_cell, mut no_model, mut no_building) = (0, 0, 0);
    let (mut from_startpos, mut agree, mut differ) = (0, 0, 0);
    let mut unmatched: HashMap<String, usize> = HashMap::new();
    let mut aqueduct_regions = std::collections::HashSet::new();
    let regions = child(world_rec, "REGION_MANAGER").and_then(|m| child(m, "REGIONS_ARRAY")).ok_or("startpos.esf: no REGIONS_ARRAY")?;
    for region in regions.children().iter().flatten().filter_map(as_record).filter(|r| r.name() == "REGION") {
        for g in region.children() {
            let Some(key) = leaf(g, None, 2).and_then(str_of) else { continue };
            let owner = leaf(g, Some("SETTLEMENT"), 9).and_then(u32_of).and_then(|id| factions.get(&id)).cloned().unwrap_or_default();
            let Some(manager) = g.iter().filter_map(as_record).find(|r| r.name() == "REGION_SLOT_MANAGER") else { continue };
            let cells = map_slots.get(key);
            let own_cells = g.iter().filter_map(as_record).find(|r| r.name() == "SETTLEMENT").map(startpos_slot_cells).unwrap_or_default();
            let slots = child(manager, "REGION_SLOT_ARRAY")
                .into_iter()
                .flat_map(|a| a.children().iter().flatten().filter_map(as_record))
                .filter(|r| r.name() == "REGION_SLOT");
            let mut secondary = 0;
            // The tier of each slot's building, by the startpos's slot index (the sprawl's `zone_slots` bits).
            let mut tiers: HashMap<usize, u32> = HashMap::new();
            for (index, slot) in slots.enumerate() {
                let (kind, building, faction) = slot_of(slot);
                if building.as_ref().is_some_and(|b| tables.with_aqueduct.contains(b)) {
                    aqueduct_regions.insert(key.to_owned());
                }

                // The map's cell of the slot by its kind (the main slot, the port, the secondary ones in order: a region without a port has
                // no port slot, its secondary ones start at index 1), else the startpos's own (its settlement's expansion data; 65535 unset).
                let map_index = match kind.as_str() {
                    "primary" => Some(0),
                    "port" => Some(1),
                    _ => {
                        secondary += 1;
                        Some(1 + secondary)
                    }
                };
                let map_cell = map_index.and_then(|i| cells.and_then(|c| c.get(i).copied().flatten()));
                let own = map_index.and_then(|i| own_cells.get(&i)).copied().filter(|&(c, r)| c != u16::MAX && r != u16::MAX);
                match (map_cell, own) {
                    (Some(a), Some(b)) if a == b => agree += 1,
                    (Some(_), Some(_)) => differ += 1,
                    (None, Some(_)) => from_startpos += 1,
                    _ => {}
                }

                let Some(cell) = map_cell.or(own) else {
                    no_cell += usize::from(building.is_some());
                    continue;
                };

                // The owner: the region's settlement's (Attila, Thrones), else the building's own (Rome 2's slots name it).
                let model = building
                    .as_deref()
                    .and_then(|b| tables.model(b, faction.as_deref().filter(|_| owner.is_empty()).unwrap_or(&owner)))
                    .map(str::to_owned);
                match (&building, &model) {
                    (None, _) => no_building += 1,
                    (Some(b), None) => {
                        no_model += 1;
                        *unmatched.entry(b.clone()).or_default() += 1;
                    }
                    _ => {}
                }

                if let (Some(i), Some(b)) = (map_index, &building) {
                    tiers.insert(i, tier_of(b));
                }

                cities.slots.push(CitySlot { region: key.to_owned(), index, kind, cell, world: centre(cell), building, model });
            }

            // The sprawl: the cells of the zones of slots with a building (their bit in `zone_slots`: the startpos's slot index), the
            // piece the cell's road or coast shape names, turned by its rotation; level from the slot's building tier (ASSUMPTION: the
            // runtime sprawl's level is grown by chance, not stored).
            let faction = owner.clone();
            let expansion = g
                .iter()
                .filter_map(as_record)
                .find(|r| r.name() == "SETTLEMENT")
                .and_then(|s| child(s, "SETTLEMENT_EXPANSION_MANAGER"))
                .and_then(|m| child(m, "SETTLEMENT_EXPANSION_MAP_DATA"));
            // Per sprawl cell: the zones' slots, the map's road and river edges.
            let mut cell_info: HashMap<(u16, u16), (i64, i64, i64)> = HashMap::new();
            if let Some(blocks) = expansion.and_then(|d| child(d, "SPRAWL_BLOCK")) {
                for sg in blocks.children() {
                    let Some(hex) = sg.iter().filter_map(as_record).find(|r| r.name() == "SPRAWL_HEX_MAP_DATA") else { continue };
                    let Some(hg) = hex.children().first() else { continue };
                    let v: Vec<i64> = hg
                        .iter()
                        .filter_map(|n| match n {
                            NodeType::Bool(b) => Some(i64::from(*b.value())),
                            NodeType::I32(x) => Some(i64::from(*x.value())),
                            other => u32_of(other).map(i64::from),
                        })
                        .collect();
                    // RPFM's writer: column, row, in a footprint, in a zone, the map's two edge masks, water, the road mask, its shape and
                    // rotation, the zones' slots, the coast mask, its shape and rotation.
                    let [
                        c,
                        r,
                        in_footprint,
                        in_zone,
                        road_edges,
                        river_edges,
                        water,
                        _,
                        road_shape,
                        road_turn,
                        zone_slots,
                        _,
                        coast_shape,
                        coast_turn,
                    ] = v[..]
                    else {
                        continue;
                    };
                    cell_info.insert((c as u16, r as u16), (zone_slots, road_edges, river_edges));
                    if in_zone == 0 || in_footprint != 0 {
                        continue;
                    }

                    let Some(tier) = (0..32).filter(|k| zone_slots & (1 << k) != 0).filter_map(|k| tiers.get(&(k as usize)).copied()).max()
                    else {
                        continue;
                    };
                    let level = tier.div_ceil(2).clamp(1, 3);
                    let (piece, turn) = if water != 0 {
                        let shape = coast_shape as u32;
                        let piece = if [3, 7, 15, 31].contains(&shape) {
                            format!("sea_sprawl_edge_{}", shape_digits(shape))
                        } else {
                            "sea_sprawl_default".to_owned()
                        };
                        (piece, coast_turn as u32)
                    } else {
                        (format!("sprawl_edge_{}", shape_digits(road_shape as u32)), road_turn as u32)
                    };

                    let cell = (c as u16, r as u16);
                    let model = tables.sprawl_model(&piece, level, &faction).map(str::to_owned);
                    cities.sprawl.push(SprawlPiece { region: key.to_owned(), cell, world: centre(cell), piece, level, turn, model });
                }
            }

            // The walls of a major settlement (RPFM: Attila walls its major settlements): every slot's wall walk (`SLOT_WALL_ARRAY`, in
            // order), the cells on the outer edge of the zones' union only (ASSUMPTION: the game drops the walls between touching zones),
            // each cell a `wall_<shape>` of the directions to the walk's previous and next cells, `gatehouse_` where a road crosses
            // (`river_gatehouse_` a river), `_inverse` when the edge before its turn leads out of the zones (a straight 14 is turned half a
            // turn instead), as the game's `FUN_10aecb70` / `FUN_10af7850` choose them; a connecting tower between two cells of a walk.
            let main = tiers.get(&0).copied();
            let walled = cities
                .slots
                .iter()
                .rev()
                .take_while(|s| s.region == key)
                .any(|s| s.index == 0 && s.building.as_deref().is_some_and(|b| b.contains("_major_")));
            let walks = expansion.map(wall_walks).unwrap_or_default();
            let inside = |c: (u16, u16)| cell_info.get(&c).is_some_and(|i| i.0 != 0);
            let outer = |c: (u16, u16)| (0..6).any(|d| !hex_neighbour(c, d).is_some_and(inside));
            let on_wall: std::collections::HashSet<(u16, u16)> = walks.iter().flatten().copied().filter(|&c| outer(c)).collect();
            if let (true, Some(tier), false) = (walled, main, on_wall.is_empty()) {
                // The wall's cells hold no sprawl.
                cities.sprawl.retain(|p| p.region != key || !on_wall.contains(&p.cell));
                let level = tier.div_ceil(2).clamp(1, 3);
                for walk in &walks {
                    let n = walk.len();
                    for (i, &cell) in walk.iter().enumerate().filter(|(_, c)| on_wall.contains(c)) {
                        let (prev, next) = (walk[(i + n - 1) % n], walk[(i + 1) % n]);
                        let mask = [prev, next].iter().filter_map(|&o| hex_direction(cell, o)).fold(0u32, |m, d| m | (1 << d));
                        if mask.count_ones() != 2 {
                            continue;
                        }

                        let (shape, rot) = edge_piece(mask);
                        let zone_mask = (0..6).filter(|&d| hex_neighbour(cell, d).is_some_and(inside)).fold(0u32, |m, d| m | (1 << d));
                        let edge = 1 << ((rot + 5) % 6);
                        let outward = zone_mask & edge != edge;
                        let (roads, rivers) = cell_info.get(&cell).map_or((0, 0), |i| (i.1 as u32, i.2 as u32));
                        let kind = if roads & !mask != 0 {
                            "gatehouse_"
                        } else if rivers & !mask != 0 {
                            "river_gatehouse_"
                        } else {
                            "wall_"
                        };
                        let base = format!("{kind}{}", shape_digits(shape));
                        let (piece, turn) = match (outward, shape) {
                            (true, 9) => (base, (rot + 3) % 6),
                            (true, _) => (format!("{base}_inverse"), rot),
                            _ => (base, rot),
                        };

                        let model = tables
                            .sprawl_model(&piece, level, &faction)
                            .or_else(|| tables.sprawl_model(&piece.replacen(kind, "wall_", 1), level, &faction))
                            .map(str::to_owned);
                        cities.sprawl.push(SprawlPiece { region: key.to_owned(), cell, world: centre(cell), piece, level, turn, model });
                    }

                    // A connecting tower between each two cells of the walk (`FUN_10aecb70`: halfway, turned by the step's direction, half a
                    // turn more when the edge before it leads out of the zone; `FUN_10af7610` names it `tower`).
                    for (i, &cell) in walk.iter().enumerate().filter(|(_, c)| on_wall.contains(c)) {
                        let next = walk[(i + 1) % n];
                        let Some(d) = hex_direction(cell, next).filter(|_| on_wall.contains(&next)) else { continue };
                        let zone_mask = (0..6).filter(|&e| hex_neighbour(cell, e).is_some_and(inside)).fold(0u32, |m, e| m | (1 << e));
                        let turn = if zone_mask & (1 << ((d + 5) % 6)) == 0 { (d + 3) % 6 } else { d };
                        let (a, b) = (centre(cell), centre(next));
                        let model = tables.sprawl_model("tower", level, &faction).map(str::to_owned);
                        cities.sprawl.push(SprawlPiece {
                            region: key.to_owned(),
                            cell,
                            world: ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0),
                            piece: "tower".to_owned(),
                            level,
                            turn,
                            model,
                        });
                    }
                }
            }
        }
    }

    cities.aqueducts = tables.aqueducts.iter().filter(|a| aqueduct_regions.contains(&a.region)).cloned().collect();
    cities.notes.push(format!(
        "{no_cell} buildings in slots without a cell, {no_building} slots without a building, {no_model} buildings without a model"
    ));
    cities.notes.push(format!(
        "sprawl: {} pieces, {} with a model",
        cities.sprawl.len(),
        cities.sprawl.iter().filter(|p| p.model.is_some()).count()
    ));
    cities.notes.push(format!("slot cells: map data and startpos agree on {agree}, differ on {differ}; {from_startpos} from the startpos alone; {} aqueducts of {} in {} regions {:?}",
        cities.aqueducts.len(), tables.aqueducts.len(), aqueduct_regions.len(), aqueduct_regions));
    let mut top: Vec<(String, usize)> = unmatched.into_iter().collect();
    top.sort_by_key(|t| std::cmp::Reverse(t.1));
    if !top.is_empty() {
        cities
            .notes
            .push(format!("without a model: {}", top.iter().take(8).map(|(b, n)| format!("{b} x{n}")).collect::<Vec<_>>().join(", ")));
    }

    Ok(cities)
}
