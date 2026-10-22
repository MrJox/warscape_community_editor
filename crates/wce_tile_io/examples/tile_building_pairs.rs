//! The buildings of a pack tile's binary `building_list` paired with the kit tile.ted's `BUILDING_INSTANCE`s of the same tile (same key,
//! world position within 5 cm), and how their attributes relate: the evidence for the decompiler's mapping (docs/formats/tile_decompile.md).
//!
//!   cargo run --release -p wce_tile_io --example tile_building_pairs -- <building_list> <kit tile folder>

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::Path;

use rpfm_lib::files::Decodeable;
use rpfm_lib::files::esf::ESF;
use wce_tile::tile_project::TileProject;
use wce_tile::world_transform::{euler_from_matrix, world_transform};
use wce_tile_io::attila::building_list::read;

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let (Some(list), Some(kit)) = (args.first(), args.get(1)) else {
        eprintln!("usage: tile_building_pairs <building_list> <kit tile folder>");
        std::process::exit(2);
    };

    let esf = ESF::decode(&mut Cursor::new(std::fs::read(list).expect("read")), &None).expect("ESF");
    let buildings = read(&esf).expect("building list");
    let project = TileProject::open(Path::new(kit)).expect("tile");
    let sc = &project.scene;
    let extent_z = project.extent_z();
    let inst = (0..sc.doc().elements().len()).filter(|&e| sc.type_of(e) == "BUILDING_INSTANCE").collect::<Vec<_>>();
    println!("{} buildings in the list, {} in tile.ted, extent z {extent_z}", buildings.len(), inst.len());
    let mut used = vec![false; buildings.len()];
    let mut pairs = vec![];
    for &e in &inst {
        let w = world_transform(sc, e);
        let key = sc.doc().get(e, "key").unwrap_or_default();
        // The list's frame: x = scene x / 2, z = extent - scene z / 2, y = scene y.
        let (x, y, z) = (w.position.x / 2.0, w.position.y, extent_z - w.position.z / 2.0);
        let best = buildings
            .iter()
            .enumerate()
            .filter(|(i, b)| !used[*i] && b.key == key)
            .map(|(i, b)| (i, (f64::from(b.position[0]) - x).hypot(f64::from(b.position[2]) - z) + (f64::from(b.position[1]) - y).abs()))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, d)) = best
            && d < 0.05
        {
            used[i] = true;
            pairs.push((e, i, w));
        }
    }

    println!("paired {} (within 5 cm)", pairs.len());
    // The kit's lite building zones (world x, z polygons), against the list's `lite`.
    let zones = (0..sc.doc().elements().len())
        .filter(|&e| sc.type_of(e) == "LiteBuildingZone")
        .map(|e| {
            let w = world_transform(sc, e);
            sc.polygon_points(e).iter().filter_map(|&p| sc.point_position(p)).map(|q| { let v = w.position + w.rotation * q; (v.x, v.z) }).collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let inside = |x: f64, z: f64| {
        zones.iter().any(|poly| {
            let mut c = false;
            for k in 0..poly.len() {
                let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
                if (a.1 > z) != (b.1 > z) && x < a.0 + (z - a.1) / (b.1 - a.1) * (b.0 - a.0) {
                    c = !c;
                }
            }
            c
        })
    };
    let mut lite_table: BTreeMap<String, usize> = BTreeMap::new();
    for (_, i, w) in &pairs {
        *lite_table.entry(format!("lite {:?}, inside a kit zone {}", buildings[*i].lite, inside(w.position.x, w.position.z))).or_default() += 1;
    }
    println!("{} lite building zones: {lite_table:?}", zones.len());
    let mut table: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut rot = vec![];
    for (e, i, w) in &pairs {
        let b = &buildings[*i];
        let mut add = |k: &str, v: String| *table.entry(k.to_owned()).or_default().entry(v).or_default() += 1;
        let attr = |a: &str| sc.doc().get(*e, a).unwrap_or_else(|| "-".to_owned());
        add("on_fire / burn", format!("{} / {}", b.on_fire, attr("burn")));
        add("starting_damage / destruction_level", format!("{} / {}", b.starting_damage, attr("destruction_level")));
        // WCE_KEYS=key,key: the same per building key.
        if std::env::var("WCE_KEYS").is_ok_and(|k| k.split(',').any(|k| k == b.key)) {
            add(&format!("{}: starting_damage / destruction_level", b.key), format!("{} / {}", b.starting_damage, attr("destruction_level")));
        }
        add("indestructible / indestructible", format!("{} / {}", b.indestructible, attr("indestructible")));
        add("toggleable / toggleable", format!("{:?} / {}", b.toggleable, attr("toggleable")));
        add("start_disabled", format!("{}", b.start_disabled));
        add("weak_point / weak_wall", format!("{} / {}", b.weak_point, attr("weak_wall")));
        add("ai_breachable / ai_breachable", format!("{} / {}", b.ai_breachable, attr("ai_breachable")));
        add("dockable / dockable", format!("{} / {}", b.dockable, attr("dockable")));
        add("lite", format!("{:?}", b.lite));
        add("scale", format!("{:?}", b.scale));
        add("tile.ted opacity / dirt / glow / burn_amount / tech", format!("{} / {} / {} / {} / {}", attr("opacity"), attr("dirt_level"), attr("glow_amount"), attr("burn_amount"), attr("building_tech_level")));
        // The quaternion as the euler of the world rotation.
        let q = b.orientation.map(f64::from);
        let (x, y, z, wq) = (q[0], q[1], q[2], q[3]);
        let m = wce_foundation::math::Double3x3 {
            m: [
                [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * wq), 2.0 * (x * z + y * wq)],
                [2.0 * (x * y + z * wq), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * wq)],
                [2.0 * (x * z - y * wq), 2.0 * (y * z + x * wq), 1.0 - 2.0 * (x * x + y * y)],
            ],
        };

        let (a, b2) = (euler_from_matrix(&m), euler_from_matrix(&w.rotation));
        rot.push(((a.x - b2.x).abs() + (a.y - b2.y).abs() + (a.z - b2.z).abs()).min(10.0));
    }

    for (k, v) in &table {
        let mut v = v.iter().collect::<Vec<_>>();
        v.sort_by(|a, b| b.1.cmp(a.1));
        println!("{k}: {}", v.iter().take(6).map(|(x, n)| format!("{x} ({n})")).collect::<Vec<_>>().join(", "));
    }

    rot.sort_by(f64::total_cmp);
    if !rot.is_empty() {
        println!("euler difference (sum of the three angles): median {:.5}, p99 {:.5}", rot[rot.len() / 2], rot[rot.len() * 99 / 100]);
    }
}
