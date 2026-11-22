//! A campaign's characters and forces at its start, from its startpos (Attila's, TDD's and Thrones' layout;
//! docs/plans/startpos_editing.md): who stands where on the campaign map, as what (a general and his army or navy, an agent), with which
//! art set and units.
//!
//! Records (`CAMPAIGN_STARTPOS/CAMPAIGN_ENV/CAMPAIGN_MODEL/WORLD/FACTION_ARRAY/FACTION`, its key `^+2`; leaf indices count records):
//! - `CHARACTER_ARRAY/CHARACTER`: [0] `LOCOMOTABLE` ([0] the world point (x east, y north), [1] / [2] the hex column / row, [3] the
//!   facing in radians), [1] its id, [2] `CHARACTER_DETAILS` (its `CHARACTER_DETAILS_ART_SET_INFO`: faction, culture, subculture, agent
//!   type, art set id), [3] the agent type (`general`, `colonel`: a garrison's commander, `spy`, `champion`, `dignitary`), [4] its
//!   force's id (0: none). A character off the map stands at (0, 0);
//! - `ARMY_ARRAY/MILITARY_FORCE`: [0] its id, [1] its commander's id, [6] `UNIT_CONTAINER/UNITS_ARRAY/UNIT` (each `UNIT_RECORD_KEY`'s
//!   first leaf its unit), [3] `MILITARY_FORCE_LEGACY` ([5] its emblem), [7] the garrison residence it stands in (0: none), [19] a
//!   navy. A force has no place of its own: its
//!   commander's.

use std::collections::HashMap;
use std::io::Cursor;

use rpfm_lib::files::Decodeable;
use rpfm_lib::files::esf::{ESF, NodeType, RecordNode};

use super::cities::{as_record, child, startpos_slot_cells, str_of, u32_of};

/// A character of the start.
#[derive(Clone, Debug, PartialEq)]
pub struct StartCharacter {
    pub id: u32,
    pub faction: String,

    /// `general`, `colonel`, `spy`, `champion`, `dignitary`...
    pub kind: String,

    /// Its art set (`campaign_character_art_sets`), empty when the startpos names none.
    pub art_set: String,
    pub culture: String,
    pub subculture: String,

    /// Its world point (x east, y north), hex cell and facing (radians).
    pub world: (f64, f64),
    pub cell: (u32, u32),
    pub facing: f32,

    /// Its force's id, when it leads one.
    pub force: Option<u32>,

    /// Whether it leads its faction: it holds the `faction_leader` post ([8] the post, [28] 4 and [29] true: RPFM's
    /// `world::government`; 60 of Attila's main campaign's 578).
    pub leader: bool,
}

/// A force of the start (an army, a navy, a garrison).
#[derive(Clone, Debug, PartialEq)]
pub struct StartForce {
    pub id: u32,
    pub faction: String,
    pub commander: u32,
    pub navy: bool,

    /// The garrison residence it stands in (a settlement's), when it does.
    pub residence: Option<u32>,

    /// Its units' keys (`main_units`).
    pub units: Vec<String>,

    /// Its legacy's emblem (`military_force_legacy_emblems`: its banner's decorator), empty for a garrison.
    pub emblem: String,
}

/// A campaign's characters and forces at its start.
#[derive(Clone, Debug, Default)]
pub struct StartCharacters {
    pub characters: Vec<StartCharacter>,
    pub forces: HashMap<u32, StartForce>,

    /// The cells of the settlements' slots (`cities::startpos_slot_cells`).
    pub settlement_cells: std::collections::HashSet<(u32, u32)>,
}

impl StartCharacters {
    /// The characters the campaign map shows: on the map, not a garrison's commander, and leading a force or standing alone: not in
    /// a settlement (`settlement_cells`) nor with an army (a horde's family stands on its army's cell: Attila's Visigoths, Alans,
    /// Vandals). Capture T2 (TDD, Rohan): Dunland's army at its capital stands there, drawn; its four generals without a force on the
    /// same cell are not; Theodred's army and an Isengard spy in the open are.
    pub fn on_map(&self) -> impl Iterator<Item = &StartCharacter> {
        let armies: std::collections::HashSet<(u32, u32)> =
            self.characters.iter().filter(|c| c.force.is_some() && c.kind != "colonel").map(|c| c.cell).collect();
        self.characters.iter().filter(move |c| {
            let placed = c.world != (0.0, 0.0);
            let alone = !self.settlement_cells.contains(&c.cell) && !armies.contains(&c.cell);
            placed && c.kind != "colonel" && (c.force.is_some() || alone)
        })
    }

    /// The force a character leads.
    pub fn force_of(&self, c: &StartCharacter) -> Option<&StartForce> {
        c.force.and_then(|f| self.forces.get(&f))
    }
}

/// The characters and forces of a startpos.
pub fn read(startpos: &[u8]) -> Result<StartCharacters, String> {
    let sp = ESF::decode(&mut Cursor::new(startpos), &None).map_err(|e| format!("startpos.esf: {e}"))?;
    let root = as_record(sp.root_node()).ok_or("startpos.esf: no root record")?;
    let world = child(root, "CAMPAIGN_ENV")
        .and_then(|r| child(r, "CAMPAIGN_MODEL"))
        .and_then(|r| child(r, "WORLD"))
        .ok_or("startpos.esf: no CAMPAIGN_ENV/CAMPAIGN_MODEL/WORLD")?;
    let factions = child(world, "FACTION_ARRAY").ok_or("startpos.esf: no FACTION_ARRAY")?;
    let mut out = StartCharacters::default();
    let regions = child(world, "REGION_MANAGER").and_then(|m| child(m, "REGIONS_ARRAY"));
    for region in regions.into_iter().flat_map(records).filter(|r| r.name() == "REGION") {
        for settlement in records(region).filter(|r| r.name() == "SETTLEMENT") {
            out.settlement_cells.extend(startpos_slot_cells(settlement).into_values().map(|(c, r)| (u32::from(c), u32::from(r))));
        }
    }
    for faction in records(factions).filter(|r| r.name() == "FACTION") {
        let Some(fg) = faction.children().first() else { continue };
        let key = super::cities::faction_key(fg).unwrap_or_default().to_owned();
        if let Some(chars) = child(faction, "CHARACTER_ARRAY") {
            out.characters.extend(records(chars).filter(|r| r.name() == "CHARACTER").filter_map(|c| character(c, &key)));
        }
        if let Some(forces) = child(faction, "ARMY_ARRAY") {
            for f in records(forces).filter(|r| r.name() == "MILITARY_FORCE").filter_map(|f| force(f, &key)) {
                out.forces.insert(f.id, f);
            }
        }
    }
    Ok(out)
}

/// The records under a record's groups.
fn records(r: &RecordNode) -> impl Iterator<Item = &RecordNode> {
    r.children().iter().flatten().filter_map(as_record)
}

fn f32_of(n: &NodeType) -> Option<f32> {
    match n {
        NodeType::F32(v) => Some(*v.value()),
        NodeType::F64(v) => Some(*v as f32),
        _ => None,
    }
}

fn character(c: &RecordNode, faction: &str) -> Option<StartCharacter> {
    let g = c.children().first()?;
    let loco = g.first().and_then(as_record).filter(|r| r.name() == "LOCOMOTABLE")?;
    let lg = loco.children().first()?;
    let world = match lg.first()? {
        NodeType::Coord2d(p) => (f64::from(*p.x()), f64::from(*p.y())),
        _ => return None,
    };
    let cell = (lg.get(1).and_then(u32_of).unwrap_or(0), lg.get(2).and_then(u32_of).unwrap_or(0));
    let facing = lg.get(3).and_then(f32_of).unwrap_or(std::f32::consts::PI);
    let id = g.get(1).and_then(u32_of)?;
    let kind = g.get(3).and_then(str_of).unwrap_or_default().to_owned();
    let force = g.get(4).and_then(u32_of).filter(|&f| f != 0);
    // The art set: `CHARACTER_DETAILS/CHARACTER_DETAILS_ART_SET_INFO` (faction, culture, subculture, agent type, art set).
    let art: Vec<&str> = g
        .get(2)
        .and_then(as_record)
        .and_then(|d| child(d, "CHARACTER_DETAILS_ART_SET_INFO"))
        .and_then(|a| a.children().first())
        .map(|ag| ag.iter().filter_map(str_of).collect())
        .unwrap_or_default();
    let text = |i: usize| art.get(i).copied().unwrap_or_default().to_owned();
    Some(StartCharacter {
        id,
        faction: faction.to_owned(),
        kind,
        art_set: text(4),
        culture: text(1),
        subculture: text(2),
        world,
        cell,
        facing,
        force,
        leader: matches!(g.get(28).and_then(u32_of), Some(4)) && matches!(g.get(29), Some(NodeType::Bool(b)) if *b.value()),
    })
}

fn force(f: &RecordNode, faction: &str) -> Option<StartForce> {
    let g = f.children().first()?;
    let units = g
        .get(6)
        .and_then(as_record)
        .and_then(|c| child(c, "UNITS_ARRAY"))
        .map(|a| {
            records(a)
                .filter(|u| u.name() == "UNIT")
                .filter_map(|u| {
                    let ug = u.children().first()?;
                    let key = ug.get(1).and_then(as_record).filter(|r| r.name() == "UNIT_RECORD_KEY")?;
                    key.children().first()?.first().and_then(str_of).map(str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default();
    Some(StartForce {
        id: g.first().and_then(u32_of)?,
        faction: faction.to_owned(),
        commander: g.get(1).and_then(u32_of).unwrap_or(0),
        navy: matches!(g.get(19), Some(NodeType::Bool(b)) if *b.value()),
        residence: g.get(7).and_then(u32_of).filter(|&r| r != 0),
        units,
        // [3] `MILITARY_FORCE_LEGACY`: [5] the emblem.
        emblem: g
            .get(3)
            .and_then(as_record)
            .filter(|r| r.name() == "MILITARY_FORCE_LEGACY")
            .and_then(|r| r.children().first()?.get(5).and_then(str_of).map(str::to_owned))
            .unwrap_or_default(),
    })
}

/// A rebel army a campaign's script raises at its start for the human player (Thrones' `vik_starting_rebellions.lua`:
/// `faction_by_key("<player>"):is_human()` then `CreateStartingRebels("<region>", "<culture>", <column>, <row>)`, which forces a
/// rebellion in the region with an army at that hex; capture V2, Gwined's: at (400, 339), beside the faction leader's army).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptedRebels {
    pub player: String,
    pub region: String,
    pub culture: String,
    pub cell: (u32, u32),
}

/// The starting rebel armies a campaign script raises (`ScriptedRebels`), read from its text: each `CreateStartingRebels` call with
/// the player whose `is_human()` test comes before it.
#[must_use]
pub fn scripted_rebels(lua: &str) -> Vec<ScriptedRebels> {
    let quoted = |s: &str| -> Vec<String> { s.split('"').skip(1).step_by(2).map(str::to_owned).collect() };
    let mut player: Option<String> = None;
    let mut out = vec![];
    for line in lua.lines() {
        let line = line.split("--").next().unwrap_or("");
        if let Some(at) = line.find("faction_by_key(")
            && line[at..].contains("is_human()")
        {
            player = quoted(&line[at..]).into_iter().next();
        }
        let Some(at) = line.find("CreateStartingRebels(") else { continue };
        let args = &line[at + "CreateStartingRebels(".len()..];
        let Some(args) = args.split(')').next() else { continue };
        let strings = quoted(args);
        let numbers: Vec<u32> = args.split(',').filter_map(|a| a.trim().parse().ok()).collect();
        if let (Some(p), [region, culture], [c, r]) = (&player, strings.as_slice(), numbers.as_slice()) {
            out.push(ScriptedRebels { player: p.clone(), region: region.clone(), culture: culture.clone(), cell: (*c, *r) });
        }
    }
    out
}

#[cfg(test)]
mod rebels_tests {
    use super::{ScriptedRebels, scripted_rebels};

    #[test]
    fn reads_the_players_and_their_rebels() {
        let lua = r#"
            if cm:model():world():faction_by_key("vik_fact_gwined"):is_human() then
                CreateStartingRebels("vik_reg_aberffro", "welsh", 400, 339)
                cm:trigger_mission("vik_fact_gwined", "vik_starting_rebels", true);
            end
        function CreateStartingRebels(region, culture, x, y)
        "#;
        assert_eq!(
            scripted_rebels(lua),
            vec![ScriptedRebels {
                player: "vik_fact_gwined".to_owned(),
                region: "vik_reg_aberffro".to_owned(),
                culture: "welsh".to_owned(),
                cell: (400, 339)
            }]
        );
    }
}
