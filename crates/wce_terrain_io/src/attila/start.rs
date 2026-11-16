//! Which campaign, and so which startpos, a campaign map starts from.
//!
//! The engine loads a campaign's startpos from `campaigns/<campaign key>/startpos.esf` (its path format `data/campaigns/%S/startpos.esf`,
//! the key of the `campaigns` table's row), not from the map's name: a map can have several campaigns (Attila's `main_attila_map`:
//! `main_attila` and `pro_attila`) and a campaign's key needs not be its map's (TDD's `rom_third_age_map` is the map of its campaign
//! `main_attila_map`, whose startpos `tdd_pack1` ships). The startpos names both: `CAMPAIGN_ENV/CAMPAIGN_SETUP`'s first text is the key,
//! `CAMPAIGN_ENV/CAMPAIGN_MODEL/CAMPAIGN_MAP_DATA`'s third the map.
//!
//! The campaigns of a map: the `campaigns` rows whose `map_name` is the map, the grand ones first (`is_grand`), then by `campaign_order`
//! and key; the first whose startpos is found and names the map. That check settles rows the merged table has twice: TDD's
//! `rom_campaigns` gives `main_attila` its map too, while `main_attila`'s startpos (a loose file of the game's `data`) stays the game's,
//! on `main_attila_map`.

use std::io::Cursor;

use rpfm_lib::files::Decodeable;
use rpfm_lib::files::esf::ESF;
use rpfm_lib::schema::Schema;

use super::cities::{as_record, child, leaf, str_of, table_rows};

/// A campaign's start: its key (the db's `campaign`, as `campaign_map_roads` names it) and its startpos.
#[derive(Clone, Debug)]
pub struct CampaignStart {
    pub campaign: String,
    pub startpos: Vec<u8>,
}

/// The campaigns of `map` (by the db: `list` the paths under a prefix, `read` a path's bytes), in the order they are tried.
pub fn campaigns_of(schema: &Schema, map: &str, list: &dyn Fn(&str) -> Vec<String>, read: &dyn Fn(&str) -> Option<Vec<u8>>) -> Vec<String> {
    let mut rows: Vec<(bool, i64, String)> = table_rows(schema, "campaigns_tables", list, read)
        .into_iter()
        .filter(|r| r.get("map_name").is_some_and(|m| m.eq_ignore_ascii_case(map)))
        .filter_map(|r| {
            let grand = r.get("is_grand").is_some_and(|g| g == "true" || g == "1");
            let order = r.get("campaign_order").and_then(|o| o.parse().ok()).unwrap_or(0);
            Some((grand, order, r.get("campaign_name")?.clone()))
        })
        .collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    rows.dedup_by(|a, b| a.2.eq_ignore_ascii_case(&b.2));
    rows.into_iter().map(|r| r.2).collect()
}

/// The campaign key and map name a startpos names (`CAMPAIGN_SETUP`, `CAMPAIGN_MAP_DATA`).
pub fn named(startpos: &[u8]) -> Option<(String, String)> {
    let esf = ESF::decode(&mut Cursor::new(startpos), &None).ok()?;
    let root = as_record(esf.root_node())?;
    let env = child(root, "CAMPAIGN_ENV")?;
    let first = |r: &rpfm_lib::files::esf::RecordNode, n: usize| -> Option<String> {
        r.children().iter().find_map(|g| leaf(g, None, n).and_then(str_of)).map(str::to_owned)
    };
    let campaign = first(child(env, "CAMPAIGN_SETUP")?, 1)?;
    let map = first(child(child(env, "CAMPAIGN_MODEL")?, "CAMPAIGN_MAP_DATA")?, 3)?;
    Some((campaign, map))
}

/// The start of `map`: the first of its campaigns (`campaigns_of`) whose startpos `startpos` finds (by its path,
/// `campaigns/<key>/startpos.esf`) and names the map. The error lists what was tried.
pub fn find(
    schema: &Schema,
    map: &str,
    list: &dyn Fn(&str) -> Vec<String>,
    read: &dyn Fn(&str) -> Option<Vec<u8>>,
    startpos: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<CampaignStart, String> {
    let campaigns = campaigns_of(schema, map, list, read);
    let mut tried = vec![];
    for campaign in &campaigns {
        let path = format!("campaigns/{campaign}/startpos.esf");
        let Some(bytes) = startpos(&path) else {
            tried.push(format!("{path}: not found"));
            continue;
        };

        match named(&bytes) {
            Some((_, on)) if on.eq_ignore_ascii_case(map) => return Ok(CampaignStart { campaign: campaign.clone(), startpos: bytes }),
            Some((_, on)) => tried.push(format!("{path}: on {on}")),
            None => tried.push(format!("{path}: no map named")),
        }
    }

    Err(if campaigns.is_empty() {
        format!("no campaign of {map} in the db")
    } else {
        format!("no startpos of {map}: {}", tried.join("; "))
    })
}
