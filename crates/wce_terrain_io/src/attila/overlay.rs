//! What the campaign map's overlay pass draws at the start for a chosen faction: the borders coloured by its diplomacy and its trade
//! routes ([`super::borders`], [`super::trade_routes`], [`super::diplomacy`]; docs/formats/campaign_borders.md).
//!
//! The frame of every point: the campaign world as the game's border and trade-route shaders receive it, x along the hex grid's columns
//! (x = column · `colw`, `colw` = the world's width / (columns - 1): 678.5 / 1015 on `main_attila_map`), z along its rows (z = row ·
//! √3 · R, + half that on odd columns, R = `colw` / 1.5; row 0 at z = 0, the map's south edge), so the grid spans 678.5 x 555.6 there.
//! Units are world units.

use std::collections::HashMap;
use std::io::Cursor;

use rpfm_lib::files::Decodeable;
use rpfm_lib::files::esf::{ESF, NodeType};
use rpfm_lib::schema::Schema;

use super::border_fit::Grid;
use super::borders::{BorderChain, chains, decode_pbd};
use super::cities::{as_record, child, table_rows, u32_of, world_bounds};
use super::diplomacy::{Diplomacy, FactionInfo, default_faction, faction_infos};
use super::start;
use super::trade_routes::{TradeSegment, decode_ptd, segments, start_routes};

/// The borders and trade routes of a campaign map for a faction.
#[derive(Clone, Debug, Default)]
pub struct CampaignOverlay {
    /// The campaign whose startpos was read ([`read`]; empty from [`build`]), and the faction.
    pub campaign: String,
    pub faction: String,
    /// The hex grid (columns, rows), its column step and the hexes' radius in world units.
    pub grid: (u32, u32),
    pub colw: f32,
    pub radius: f32,
    pub borders: Vec<BorderChain>,
    pub trade_routes: Vec<TradeSegment>,
    /// What was missing or left out.
    pub notes: Vec<String>,
}

/// The paths of a map's files.
#[must_use]
pub fn pbd_path(map: &str) -> String {
    format!("campaign_maps/{map}/display/borders/borders.pbd")
}

#[must_use]
pub fn ptd_path(map: &str) -> String {
    format!("campaign_maps/{map}/trade_routes.ptd")
}

/// The hex grid of a map's `map_data.esf` (`REGIONS_DATA`'s world and `theatres/CAMPAIGN_THEATRE/HEX_MAP_DATA`'s size).
#[must_use]
pub fn grid_of(map_data: &[u8]) -> Option<Grid> {
    let [x0, _, x1, _] = world_bounds(map_data)?;
    let md = ESF::decode(&mut Cursor::new(map_data), &None).ok()?;
    let root = as_record(md.root_node())?;
    let hex = ["REGIONS_DATA", "theatres", "CAMPAIGN_THEATRE", "HEX_MAP_DATA"].iter().try_fold(root, |r, n| child(r, n))?;
    let v: Vec<u32> = hex.children().first()?.iter().filter_map(u32_of).take(2).collect();
    (v.len() == 2).then(|| Grid::new(v[0], v[1], x1 - x0))
}

/// The db's region -> province (`region_to_province_junctions`).
#[must_use]
pub fn provinces(schema: &Schema, list: &dyn Fn(&str) -> Vec<String>, read: &dyn Fn(&str) -> Option<Vec<u8>>) -> HashMap<String, String> {
    table_rows(schema, "region_to_province_junctions_tables", list, read)
        .into_iter()
        .filter_map(|r| Some((r.get("region")?.clone(), r.get("province")?.clone())))
        .collect()
}

fn decode_startpos(bytes: &[u8]) -> Result<ESF, String> {
    ESF::decode(&mut Cursor::new(bytes), &None).map_err(|e| format!("startpos.esf: {e}"))
}

fn root_of(esf: &ESF) -> Result<&rpfm_lib::files::esf::RecordNode, String> {
    match esf.root_node() {
        NodeType::Record(r) => Ok(r),
        _ => Err("startpos.esf: no root record".to_owned()),
    }
}

/// From the files themselves: `map_data.esf`, `borders.pbd`, `trade_routes.ptd` (none: no trade routes), the startpos, the provinces;
/// `faction` the chosen faction's key (none: [`default_faction`]).
///
/// # Errors
///
/// A file does not decode, the map has no hex grid, or the faction is not in the startpos.
pub fn build(
    map_data: &[u8],
    pbd: &[u8],
    ptd: Option<&[u8]>,
    startpos: &[u8],
    provinces: &HashMap<String, String>,
    faction: Option<&str>,
) -> Result<CampaignOverlay, String> {
    let grid = grid_of(map_data).ok_or("map_data.esf: no hex grid")?;
    let pbd = decode_pbd(pbd).map_err(|e| format!("borders.pbd: {e}"))?;
    let esf = decode_startpos(startpos)?;
    let root = root_of(&esf)?;
    let diplomacy = Diplomacy::read(root)?;
    let faction = match faction {
        Some(f) => f.to_owned(),
        None => default_faction(&faction_infos(root)).map(|i| i.key.clone()).ok_or("startpos.esf: no factions")?,
    };
    let player = diplomacy.faction_id(&faction).ok_or_else(|| format!("{faction} is not a faction of the startpos"))?;
    let mut notes = vec![];
    if !pbd.crc_ok {
        notes.push("borders.pbd: the CRC does not match".to_owned());
    }

    let (borders, n) = chains(&pbd, &grid, &diplomacy, provinces, player);
    notes.extend(n);
    let mut trade_routes = vec![];
    if let Some(ptd) = ptd {
        let ptd = decode_ptd(ptd).map_err(|e| format!("trade_routes.ptd: {e}"))?;
        let routes = start_routes(root);
        let (s, missing) = segments(&ptd, &routes, &diplomacy, player, grid.radius);
        notes.push(format!("{} trade-route segments of {} routes", s.len(), routes.iter().filter(|r| r.owner == player).count()));
        if !missing.is_empty() {
            notes.push(format!("trade-route legs not in the file: {}", missing.join(", ")));
        }
        trade_routes = s;
    }

    Ok(CampaignOverlay {
        campaign: String::new(),
        faction,
        grid: (grid.cols as u32, grid.rows as u32),
        colw: grid.colw as f32,
        radius: grid.radius as f32,
        borders,
        trade_routes,
        notes,
    })
}

/// The borders and trade routes of `map` for `faction` (none: the default), its startpos found as [`start::find`] does: `list` the
/// paths under a prefix, `read` a path's bytes (the packs' files), `startpos` a startpos path's bytes.
///
/// # Errors
///
/// No startpos, no `map_data.esf` or `borders.pbd`, or as [`build`].
pub fn read(
    schema: &Schema,
    map: &str,
    faction: Option<&str>,
    list: &dyn Fn(&str) -> Vec<String>,
    read: &dyn Fn(&str) -> Option<Vec<u8>>,
    startpos: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<CampaignOverlay, String> {
    let start = start::find(schema, map, list, read, startpos)?;
    let map_data = read(&format!("campaign_maps/{map}/map_data.esf")).ok_or("no map_data.esf")?;
    let pbd = read(&pbd_path(map)).ok_or("no borders.pbd")?;
    let ptd = read(&ptd_path(map));
    let mut o = build(&map_data, &pbd, ptd.as_deref(), &start.startpos, &provinces(schema, list, read), faction)?;
    o.campaign = start.campaign;
    if ptd.is_none() {
        o.notes.push("no trade_routes.ptd".to_owned());
    }
    Ok(o)
}

/// The campaign's faction list (`FACTION_INFOS`, the file's order; sort by `order` to show it as the game does) and the default pick.
///
/// # Errors
///
/// No startpos, or it does not decode.
pub fn factions(
    schema: &Schema,
    map: &str,
    list: &dyn Fn(&str) -> Vec<String>,
    read: &dyn Fn(&str) -> Option<Vec<u8>>,
    startpos: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<(Vec<FactionInfo>, Option<String>), String> {
    let start = start::find(schema, map, list, read, startpos)?;
    let esf = decode_startpos(&start.startpos)?;
    let infos = faction_infos(root_of(&esf)?);
    let default = default_faction(&infos).map(|i| i.key.clone());
    Ok((infos, default))
}
