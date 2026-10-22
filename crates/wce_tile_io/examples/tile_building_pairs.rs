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
    // The list's position from the kit's world position: in f64 then rounded once, against TEd's f32 world transform (BOB's).
    let (mut exact64, mut exact32) = ([0usize; 3], [0usize; 3]);
    for (e, i, w) in &pairs {
        let b = &buildings[*i];
        let d = [(w.position.x / 2.0) as f32, w.position.y as f32, (extent_z - w.position.z / 2.0) as f32];
        let m = wce_tile::world_transform::ted_world_matrix_f32(sc, *e);
        let f = [m[3] * 0.5, m[7], extent_z as f32 - m[11] * 0.5];
        for k in 0..3 {
            exact64[k] += usize::from(d[k] == b.position[k]);
            exact32[k] += usize::from(f[k] == b.position[k]);
        }
    }

    println!("positions exact (x, y, z): f64 world {exact64:?}, f32 world {exact32:?} of {}", pairs.len());

    // Other ways TEd may compose the chain: each element's products summed in extended precision (x87) then rounded once; and the
    // parent on the right (row vectors).
    let chain = |e: usize| {
        let mut c = vec![];
        let mut x = Some(e);
        while let Some(y) = x {
            if sc.doc().get(y, "position").is_some() || sc.doc().get(y, "orientation").is_some() {
                c.push(y);
            }
            x = sc.doc().elements()[y].parent;
        }
        c.reverse();
        c
    };
    let local = |e: usize| {
        let v = |a: &str, p: &str| -> [f32; 3] {
            sc.doc().get(e, a).and_then(|t| {
                let i = t.trim().strip_prefix(p)?.strip_prefix('(')?.strip_suffix(')')?.to_owned();
                let mut it = i.split(',').map(|x| x.trim().parse::<f32>().ok());
                Some([it.next()??, it.next()??, it.next()??])
            }).unwrap_or([0.0; 3])
        };
        let mut m = wce_tile::world_transform::ted_rotation_f32(v("orientation", "euler"));
        let p = v("position", "vector");
        (m[3], m[7], m[11]) = (p[0], p[1], p[2]);
        m
    };
    let mul_x87 = |w: &[f32; 16], l: &[f32; 16]| -> [f32; 16] {
        std::array::from_fn(|k| {
            let (i, j) = (k / 4, k % 4);
            (0..4).map(|t| f64::from(w[i * 4 + t]) * f64::from(l[t * 4 + j])).sum::<f64>() as f32
        })
    };
    let mut ex = [[0usize; 3]; 2];
    for (e, i, _) in &pairs {
        let b = &buildings[*i];
        let c = chain(*e);
        let mut wa: Option<[f32; 16]> = None;
        let mut wb: Option<[f32; 16]> = None;
        for &x in &c {
            let l = local(x);
            wa = Some(wa.map_or(l, |w| mul_x87(&w, &l)));
            // Row vectors: the translation in the last row, local * parent.
            let lt: [f32; 16] = std::array::from_fn(|k| l[(k % 4) * 4 + k / 4]);
            wb = Some(wb.map_or(lt, |w| wce_tile::world_transform::ted_mul_f32(&lt, &w)));
        }

        let (wa, wb) = (wa.unwrap(), wb.unwrap());
        let half = 0.5f32;
        let fa = [wa[3] * half, wa[7], extent_z as f32 - wa[11] * half];
        let fb = [wb[12] * half, wb[13], extent_z as f32 - wb[14] * half];
        for k in 0..3 {
            ex[0][k] += usize::from(fa[k] == b.position[k]);
            ex[1][k] += usize::from(fb[k] == b.position[k]);
        }
    }

    println!("positions exact: x87 sums {:?}, row vectors {:?}", ex[0], ex[1]);

    // How far off the f32 world is, in units in the last place of the list's value (per axis: 0, 1, 2-4, 5-100, more).
    let mut hist = [[0usize; 5]; 3];
    for (e, i, _) in &pairs {
        let b = &buildings[*i];
        let m = wce_tile::world_transform::ted_world_matrix_f32(sc, *e);
        let f = [m[3] * 0.5, m[7], extent_z as f32 - m[11] * 0.5];
        for k in 0..3 {
            let ulps = (i64::from(f[k].to_bits() as i32) - i64::from(b.position[k].to_bits() as i32)).unsigned_abs();
            hist[k][match ulps { 0 => 0, 1 => 1, 2..=4 => 2, 5..=100 => 3, _ => 4 }] += 1;
        }
    }

    // Quaternion composition: q = qz(c) qy(a) qx(-b) from f32 half-angle sin / cos, world q = parent q * local q, world position =
    // parent position + parent q rotating the local position (through q's matrix, or v + 2 w (u x v) + 2 u x (u x v)).
    let qmul = |a: [f32; 4], b: [f32; 4]| -> [f32; 4] {
        let (x1, y1, z1, w1) = (a[0], a[1], a[2], a[3]);
        let (x2, y2, z2, w2) = (b[0], b[1], b[2], b[3]);
        [w1 * x2 + x1 * w2 + y1 * z2 - z1 * y2, w1 * y2 - x1 * z2 + y1 * w2 + z1 * x2, w1 * z2 + x1 * y2 - y1 * x2 + z1 * w2, w1 * w2 - x1 * x2 - y1 * y2 - z1 * z2]
    };
    let qeuler = |e: [f32; 3]| -> [f32; 4] {
        let h = |v: f32| { let t = f64::from(v) * 0.5; (t.sin() as f32, t.cos() as f32) };
        let (sa, ca) = h(e[0]);
        let (sb, cb) = h(-e[1]);
        let (sc_, cc) = h(e[2]);
        qmul(qmul([0.0, 0.0, sc_, cc], [0.0, sa, 0.0, ca]), [sb, 0.0, 0.0, cb])
    };
    let rot_m = |q: [f32; 4], v: [f32; 3]| -> [f32; 3] {
        let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
        let m = [
            [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)],
            [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)],
            [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)],
        ];
        std::array::from_fn(|i| m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2])
    };
    let rot_v = |q: [f32; 4], v: [f32; 3]| -> [f32; 3] {
        let u = [q[0], q[1], q[2]];
        let cross = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
        let t = cross(u, v).map(|c| 2.0 * c);
        let ut = cross(u, t);
        std::array::from_fn(|k| v[k] + q[3] * t[k] + ut[k])
    };
    let attr3 = |e: usize, a: &str, p: &str| -> [f32; 3] {
        sc.doc().get(e, a).and_then(|t| {
            let i = t.trim().strip_prefix(p)?.strip_prefix('(')?.strip_suffix(')')?.to_owned();
            let mut it = i.split(',').map(|x| x.trim().parse::<f32>().ok());
            Some([it.next()??, it.next()??, it.next()??])
        }).unwrap_or([0.0; 3])
    };
    let mut exq = [[0usize; 3]; 2];
    for (e, i, _) in &pairs {
        let b = &buildings[*i];
        for (k, rot) in [(0usize, &rot_m as &dyn Fn([f32; 4], [f32; 3]) -> [f32; 3]), (1, &rot_v)] {
            let (mut q, mut p) = ([0.0f32, 0.0, 0.0, 1.0], [0.0f32; 3]);
            for &x in &chain(*e) {
                let lp = attr3(x, "position", "vector");
                let r = rot(q, lp);
                p = [p[0] + r[0], p[1] + r[1], p[2] + r[2]];
                q = qmul(q, qeuler(attr3(x, "orientation", "euler")));
            }

            let f = [p[0] * 0.5, p[1], extent_z as f32 - p[2] * 0.5];
            for a in 0..3 {
                exq[k][a] += usize::from(f[a] == b.position[a]);
            }
        }
    }

    println!("positions exact: quaternion chain, matrix rotate {:?}, vector rotate {:?}", exq[0], exq[1]);

    // The f32 world's exactness (x) by chain depth and whether an ancestor turns.
    let mut by: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for (e, i, _) in &pairs {
        let b = &buildings[*i];
        let c = chain(*e);
        let turned = c[..c.len() - 1].iter().any(|&x| attr3(x, "orientation", "euler") != [0.0; 3]);
        let own = attr3(*e, "orientation", "euler") != [0.0; 3];
        let m = wce_tile::world_transform::ted_world_matrix_f32(sc, *e);
        let k = format!("depth {} ancestor turned {turned} own turn {own}", c.len());
        let v = by.entry(k).or_default();
        v.0 += usize::from(m[3] * 0.5 == b.position[0]);
        v.1 += 1;
    }

    // The chains without a turn: the translations and the list's x, for working out the sum.
    let mut shown = 0;
    for (e, i, _) in &pairs {
        let c = chain(*e);
        if c.iter().any(|&x| attr3(x, "orientation", "euler") != [0.0; 3]) || shown >= 15 {
            continue;
        }

        shown += 1;
        let xs = c.iter().map(|&x| attr3(x, "position", "vector")[0]).collect::<Vec<_>>();
        println!("  untwisted chain x {:?} -> list x {:?} (z {:?} -> {:?})", xs, buildings[*i].position[0], c.iter().map(|&x| attr3(x, "position", "vector")[2]).collect::<Vec<_>>(), buildings[*i].position[2]);
    }

    for (k, (a, n)) in &by {
        println!("  {k}: x exact {a} / {n}");
    }

    println!("f32 world off by ulps (0, 1, 2-4, 5-100, more): x {:?}, y {:?}, z {:?}", hist[0], hist[1], hist[2]);
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
