//! A settlement's slot cells in the startpos (`SETTLEMENT_EXPANSION_MAP_DATA`: each slot's footprint, zone and wall arrays) against a
//! RenderDoc capture's rigid instances (`scripts/re/rd_campaign_frame.py`'s folder): which cells the game drew a model on. The zones it
//! filled tell which slots it linked at the start, the wall cells it drew where its wall ring runs (docs/re/terry.md, "Settlements at
//! the start").
//!
//!   cargo run --release -p wce_terrain_io --example settlement_cells_check -- <frame folder> <region key | *> [campaign] [map]
//!
//! `*`: every settlement whose main slot the capture drew, the number of secondary slots whose zone the game filled against the
//! number `wce_terrain_io::attila::cities` links (`expansion_order`: which ones is a draw, in the game too).
//!
//! `WCE_ATTILA_DIR` the game (default this machine's Steam folder).

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;
use std::path::PathBuf;

use rpfm_lib::files::Decodeable;
use rpfm_lib::files::esf::{ESF, NodeType, RecordNode};

fn rec(n: &NodeType) -> Option<&RecordNode> {
    match n {
        NodeType::Record(r) => Some(r),
        _ => None,
    }
}

fn child<'a>(r: &'a RecordNode, name: &str) -> Option<&'a RecordNode> {
    r.children().iter().flatten().filter_map(rec).find(|c| c.name() == name)
}

fn num(n: &NodeType) -> Option<u32> {
    match n {
        NodeType::U8(v) => Some(u32::from(*v)),
        NodeType::U16(v) => Some(u32::from(*v)),
        NodeType::U32(v) => Some(*v.value()),
        NodeType::I32(v) => u32::try_from(*v.value()).ok(),
        _ => None,
    }
}

fn text(n: &NodeType) -> Option<&str> {
    match n {
        NodeType::Ascii(v) | NodeType::Utf16(v) => Some(v),
        _ => None,
    }
}

/// The cells of a record of cells (each group two u16).
fn cells(r: &RecordNode) -> Vec<(u32, u32)> {
    r.children()
        .iter()
        .filter_map(|g| {
            let mut v = g.iter().filter_map(num);
            Some((v.next()?, v.next()?))
        })
        .collect()
}

struct Slot {
    index: u32,
    cell: (u32, u32),
    tail: Vec<u32>,
    footprint: Vec<(u32, u32)>,
    zone: Vec<(u32, u32)>,
    wall: Vec<(u32, u32)>,
}

fn slot(r: &RecordNode) -> Option<Slot> {
    let g = r.children().first()?;
    let nums: Vec<u32> = g.iter().filter_map(num).collect();
    let arr = |name: &str| g.iter().filter_map(rec).find(|c| c.name() == name).map(cells).unwrap_or_default();
    Some(Slot {
        index: *nums.first()?,
        cell: (*nums.get(1)?, *nums.get(2)?),
        tail: nums[3..].to_vec(),
        footprint: arr("SLOT_FOOTPRINT_ARRAY"),
        zone: arr("SLOT_ZOE_ARRAY"),
        wall: arr("SLOT_WALL_ARRAY"),
    })
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (dir, region) = (PathBuf::from(&a[0]), a[1].clone());
    let campaign = a.get(2).map_or("main_attila", String::as_str);
    let game_dir = std::env::var_os("WCE_ATTILA_DIR")
        .map_or_else(|| PathBuf::from("D:/Juegos/Steam/steamapps/common/Total War Attila"), PathBuf::from);
    let startpos = std::fs::read(game_dir.join(format!("data/campaigns/{campaign}/startpos.esf"))).expect("startpos.esf");
    let esf = ESF::decode(&mut Cursor::new(startpos), &None).expect("esf");
    let root = rec(esf.root_node()).expect("root");
    let world = child(root, "CAMPAIGN_ENV").and_then(|r| child(r, "CAMPAIGN_MODEL")).and_then(|r| child(r, "WORLD")).expect("WORLD");
    let regions = child(world, "REGION_MANAGER").and_then(|m| child(m, "REGIONS_ARRAY")).expect("REGIONS_ARRAY");
    let mut by_region: Vec<(String, Vec<Slot>)> = vec![];
    let mut flags: std::collections::HashMap<String, Vec<(bool, bool)>> = std::collections::HashMap::new();
    for r in regions.children().iter().flatten().filter_map(rec).filter(|r| r.name() == "REGION") {
        let Some(g) = r.children().first() else { continue };
        let Some(key) = g.iter().filter_map(text).find(|t| t.contains("_reg_")) else { continue };
        if region != "*" && key != region {
            continue;
        }
        let mut slots = vec![];
        // The region's slots (`REGION_SLOT_MANAGER/REGION_SLOT_ARRAY`): secondary ones' building and the flag after it.
        if let Some(manager) = g.iter().filter_map(rec).find(|c| c.name() == "REGION_SLOT_MANAGER") {
            let list: Vec<(bool, bool)> = child(manager, "REGION_SLOT_ARRAY")
                .into_iter()
                .flat_map(|a| a.children().iter().flatten().filter_map(rec))
                .filter(|r| r.name() == "REGION_SLOT")
                .filter_map(|slot| {
                    let sg = slot.children().first()?;
                    if sg.iter().filter_map(text).any(|t| t == "primary" || t == "port") {
                        return None;
                    }
                    let m = sg.iter().filter_map(rec).find(|c| c.name() == "REGION_BUILDING_MANAGER")?;
                    let mg = m.children().first()?;
                    let built = mg.iter().filter_map(rec).any(|c| c.name() == "BUILDING_BASE");
                    let bools: Vec<bool> = mg
                        .iter()
                        .filter_map(|n| match n {
                            NodeType::Bool(b) => Some(*b.value()),
                            _ => None,
                        })
                        .collect();
                    // Bools: has a building, then (with one) true, then the flag; (without) false, then the flag.
                    Some((built, bools.get(2).copied().unwrap_or(false)))
                })
                .collect();
            flags.insert(key.to_owned(), list);
        }
        let Some(data) = g
            .iter()
            .filter_map(rec)
            .find(|c| c.name() == "SETTLEMENT")
            .and_then(|s| child(s, "SETTLEMENT_EXPANSION_MANAGER"))
            .and_then(|m| child(m, "SETTLEMENT_EXPANSION_MAP_DATA"))
        else {
            continue;
        };
        for c in data.children().iter().flatten().filter_map(rec) {
            match c.name().as_str() {
                "SLOT_MAP_DATA" => slots.extend(slot(c)),
                "SECONDARY_SLOT_ARRAY" => {
                    slots.extend(c.children().iter().flatten().filter_map(rec).filter(|s| s.name() == "SLOT_MAP_DATA").filter_map(slot))
                }
                _ => {}
            }
        }
        slots.retain(|s| s.cell.0 != 65535);
        by_region.push((key.to_owned(), slots));
    }

    // A cell's centre in the world (RPFM's MapScale: main_attila_map's hex grid 1016 x 720 over 678.5 x 556.144).
    let (cols, rows, w, h) = (1016.0, 720.0, 678.5, 556.1439);
    let sy = h / (rows * 8.0 + 4.0);
    let centre = |(c, r): (u32, u32)| {
        let (c, r) = (f64::from(c), f64::from(r));
        (c * w / (cols - 1.0), r * 8.0 * sy + if c as u32 % 2 == 1 { 4.0 * sy } else { 0.0 })
    };

    // The capture's rigid instances (x, z, event), every draw, and each draw's index count.
    let inst3: Vec<(f64, f64, u32)> = std::fs::read_to_string(dir.join("instances.csv"))
        .expect("instances.csv")
        .lines()
        .skip(1)
        .filter_map(|l| {
            let mut p = l.split(',');
            let e: u32 = p.next()?.parse().ok()?;
            let f: Vec<f64> = p.nth(1)?.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            (f.len() >= 12).then(|| (f[3], f[11], e))
        })
        .collect();
    let inst: Vec<(f64, f64)> = inst3.iter().map(|i| (i.0, i.1)).collect();
    let indices: std::collections::HashMap<u32, String> = std::fs::read_to_string(dir.join("events.csv"))
        .expect("events.csv")
        .lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split(',').collect();
            Some((f.first()?.parse().ok()?, (*f.get(3)?).to_owned()))
        })
        .collect();
    let at_cell = |cell: (u32, u32)| -> Vec<String> {
        let (x, z) = centre(cell);
        let mut v: Vec<String> =
            inst3.iter().filter(|i| (i.0 - x).hypot(i.1 - z) < 0.05).filter_map(|i| indices.get(&i.2).cloned()).collect();
        v.sort();
        v.dedup();
        v
    };
    let drawn = |cell: (u32, u32)| {
        let (x, z) = centre(cell);
        inst.iter().any(|&(ix, iz)| (ix - x).hypot(iz - z) < 0.05)
    };

    if region == "*" {
        let map = a.get(3).map_or("main_attila_map", String::as_str);
        let mut cfg = wce_vfs::Config::new(&game_dir, wce_vfs::GameProfile::from_env().clone());
        cfg.mod_packs = Some(vec![]);
        let vfs = wce_vfs::Vfs::mount(&cfg).expect("vfs");
        let map_data = vfs.read(&format!("campaign_maps/{map}/map_data.esf")).expect("map_data.esf").to_vec();
        let startpos = std::fs::read(game_dir.join(format!("data/campaigns/{campaign}/startpos.esf"))).expect("startpos.esf");
        let cities = wce_terrain_io::attila::cities::read(&map_data, &startpos, &Default::default()).expect("cities");
        let (mut same, mut count, mut all) = (0, 0, 0);
        for (key, slots) in &by_region {
            let Some(main) = slots.iter().find(|s| s.index == 0) else { continue };
            if !drawn(main.cell) {
                continue;
            }
            // A secondary slot is linked when the game filled its zone (any cell of it but its footprint drawn).
            let filled = |s: &Slot, cells: &dyn Fn((u32, u32)) -> bool| s.zone.iter().any(|&c| !s.footprint.contains(&c) && cells(c));
            let game: Vec<u32> = slots.iter().filter(|s| s.index >= 2 && drawn(s.cell)).map(|s| s.index).collect();
            let zoned: Vec<u32> = slots.iter().filter(|s| s.index >= 2 && filled(s, &drawn)).map(|s| s.index).collect();
            let ours: std::collections::HashSet<(u32, u32)> =
                cities.sprawl.iter().filter(|p| &p.region == key).map(|p| (u32::from(p.cell.0), u32::from(p.cell.1))).collect();
            let editor: Vec<u32> = slots.iter().filter(|s| s.index >= 2 && filled(s, &|c| ours.contains(&c))).map(|s| s.index).collect();
            // Walls: the cells of every slot's wall walk where the game drew a Roman wall or gate (by its first mesh's index count:
            // `model_bounds`), against the editor's wall and gate pieces.
            // Also the walls' and gates' other meshes (168, 360): a model's meshes are drawn apart, each at its pivot, and the first's
            // can stand off the cell's centre.
            const ROMAN_WALLS: [&str; 9] =
                ["462", "492", "738+168+360+360", "690", "654", "834", "918+360+360+168+360+360+168", "168", "360"];
            let candidates: BTreeSet<(u32, u32)> = slots.iter().flat_map(|s| s.wall.iter().copied()).collect();
            let game_walls: BTreeSet<(u32, u32)> = candidates
                .iter()
                .copied()
                .filter(|&c| at_cell(c).iter().any(|i| ROMAN_WALLS.iter().any(|w| w.split('+').next() == Some(i.as_str()))))
                .collect();
            let editor_walls: BTreeSet<(u32, u32)> = cities
                .sprawl
                .iter()
                .filter(|p| &p.region == key && (p.piece.starts_with("wall_") || p.piece.contains("gatehouse_")))
                .map(|p| (u32::from(p.cell.0), u32::from(p.cell.1)))
                .collect();
            if std::env::var("WCE_SHOW").is_ok_and(|r| key.contains(&r)) {
                let zones: std::collections::HashMap<(u32, u32), u32> =
                    slots.iter().flat_map(|s| s.zone.iter().map(move |&c| (c, s.index))).collect();
                for &c in editor_walls.difference(&game_walls) {
                    let piece = cities
                        .sprawl
                        .iter()
                        .find(|p| &p.region == key && (u32::from(p.cell.0), u32::from(p.cell.1)) == c)
                        .map(|p| p.piece.clone());
                    println!("      editor only {c:?} zone {:?} {piece:?}: the game drew {:?}", zones.get(&c), at_cell(c));
                }
                for &c in game_walls.difference(&editor_walls) {
                    println!("      game only {c:?} zone {:?}: {:?}", zones.get(&c), at_cell(c));
                }
            }
            let main_building = cities.slots.iter().find(|s| &s.region == key && s.index == 0).and_then(|s| s.building.clone());
            println!(
                "    walls of {key} (same slots: {}): the game {} cells, the editor {}, both {}; main {main_building:?}",
                game == editor,
                game_walls.len(),
                editor_walls.len(),
                game_walls.intersection(&editor_walls).count()
            );
            all += 1;
            same += usize::from(game == editor);
            count += usize::from(game.len() == editor.len());
            let f = flags.get(key).cloned().unwrap_or_default();
            println!(
                "{key}: the game's anchors {game:?} (zones touched {zoned:?}), the editor {editor:?}; secondary slots (building, flag) {:?}",
                f.iter().map(|&(b, x)| format!("{}{}", if b { "B" } else { "-" }, if x { "F" } else { "" })).collect::<Vec<_>>()
            );
        }
        println!("{count} of {all} settlements link as many secondary slots, {same} the same ones");
        return;
    }
    let slots = by_region.into_iter().next().map(|r| r.1).unwrap_or_default();
    if slots.is_empty() {
        eprintln!("no slots for {region}");
        return;
    }

    let mark = |v: &[(u32, u32)]| -> String {
        let d = v.iter().filter(|&&c| drawn(c)).count();
        format!("{d}/{}", v.len())
    };
    let walls: BTreeSet<(u32, u32)> = slots.iter().flat_map(|s| s.wall.iter().copied()).collect();
    for s in &slots {
        println!(
            "slot {} cell {:?} {} tail {:?}: footprint {} drawn, zone {} drawn, walls {} drawn",
            s.index,
            s.cell,
            if drawn(s.cell) { "drawn" } else { "empty" },
            s.tail,
            mark(&s.footprint),
            mark(&s.zone),
            mark(&s.wall)
        );
    }
    // The draws on each zone cell (their index counts: `model_bounds` names the model by them).
    for s in &slots {
        for &c in s.zone.iter().filter(|c| !s.footprint.contains(c)) {
            let d = at_cell(c);
            if !d.is_empty() {
                println!("  slot {} cell {c:?}: {}", s.index, d.join(" "));
            }
        }
    }
    // A map of the cells: zone cells by their slot's index, walls `W` / `w` (drawn or not), `#` drawn otherwise.
    let mut grid: BTreeMap<(u32, u32), String> = BTreeMap::new();
    for s in &slots {
        for &c in &s.zone {
            grid.insert(c, format!("{}{}", s.index, if drawn(c) { "#" } else { "." }));
        }
    }
    for &c in &walls {
        grid.insert(c, if drawn(c) { "W#".to_owned() } else { "w.".to_owned() });
    }
    for s in &slots {
        grid.insert(s.cell, format!("S{}", s.index));
    }
    let (c0, c1) = (grid.keys().map(|k| k.0).min().unwrap_or(0), grid.keys().map(|k| k.0).max().unwrap_or(0));
    let (r0, r1) = (grid.keys().map(|k| k.1).min().unwrap_or(0), grid.keys().map(|k| k.1).max().unwrap_or(0));
    println!("columns {c0}..{c1}, rows {r1} down to {r0} (north up)");
    for r in (r0..=r1).rev() {
        let line: String = (c0..=c1).map(|c| format!("{:>3}", grid.get(&(c, r)).map_or(" ", String::as_str))).collect();
        println!("{r:4} {line}");
    }
}
