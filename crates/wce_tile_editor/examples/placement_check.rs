//! Phase 0 oracle of the Tile tool parity plan (docs/plans/tile_ted_parity.md): compares the world position / orientation the tool computes
//! for every BUILDING_INSTANCE (the tile document's world transforms, positions and the height map) with TEd's own stage A export,
//! `building_list.xml` (working_data/.../building_list.xml: key, position_x/y/z, position_type, quaternion, scale). Read-only: it reports, it
//! does not judge. Port of `cli/placement_check.cpp`.
//!
//! Columns: e_abs = list y - world y (the export's y is absolute: 0 for everything but 15 crates); "above" = world y - the tool's terrain
//! height (0 for a landscape building sitting on the height map).
//!
//!   cargo run --release -p wce_tile_editor --example placement_check -- <kit root> [--filter <substring of the tile path>] [--limit N]
//!       [--worst N] [--csv <file>]
//!
//! <kit root> is an assembly_kit_*_tilemap folder (raw_data/... holds tile.ted, working_data/... holds the export). Heights are reported under
//! the hypotheses the plan lists: the export's y is absolute (list_y = world_y) or relative to the tool's terrain (list_y = world_y - terrain).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use wce_formats::c_num::{printf_g, strtod_prefix};
use wce_formats::xml_doc::XmlDoc;
use wce_foundation::math::Double3x3;
use wce_tile::tile_project::TileProject;
use wce_tile::world_transform::world_transform;
use wce_tile_editor::tile_document::TileDocument;

#[derive(Clone, Debug, Default)]
struct ListEntry {
    key: String,
    x: f64,
    y: f64,
    z: f64,

    /// i j k w
    q: [f64; 4],
    used: bool,
}

/// C's atof of an attribute, `def` when it is missing.
fn num(d: &XmlDoc, e: usize, a: &str, def: f64) -> f64 {
    d.get(e, a).map_or(def, |v| strtod_prefix(&v).map_or(0.0, |(x, _)| x))
}

fn read_list(p: &Path) -> Result<Vec<ListEntry>, String> {
    let doc = XmlDoc::parse(std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let mut out = vec![];
    for &root in doc.roots() {
        for &c in &doc.elements()[root].children {
            if doc.elements()[c].name != "BATTLEFIELD_BUILDING" {
                continue;
            }

            out.push(ListEntry {
                key: doc.get(c, "key").unwrap_or_default(),
                x: num(&doc, c, "position_x", 0.0),
                y: num(&doc, c, "position_y", 0.0),
                z: num(&doc, c, "position_z", 0.0),
                q: [num(&doc, c, "orientation_i", 0.0), num(&doc, c, "orientation_j", 0.0), num(&doc, c, "orientation_k", 0.0), num(&doc, c, "orientation_w", 1.0)],
                used: false,
            });
        }
    }

    Ok(out)
}

fn quat_matrix(q: &[f64; 4]) -> Double3x3 {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let n = (x * x + y * y + z * z + w * w).sqrt();
    let (x, y, z, w) = (x / n, y / n, z / n, w / n);
    Double3x3 {
        m: [
            [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)],
            [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)],
            [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)],
        ],
    }
}

/// F * M * F with F = diag(1, 1, -1): the same rotation seen from the frame with z mirrored.
fn mirror_z(m: &Double3x3) -> Double3x3 {
    let m = &m.m;
    Double3x3 { m: [[m[0][0], m[0][1], -m[0][2]], [m[1][0], m[1][1], -m[1][2]], [-m[2][0], -m[2][1], m[2][2]]] }
}

#[allow(clippy::manual_clamp)]
fn angle_between(a: &Double3x3, b: &Double3x3) -> f64 {
    let r = (a.transpose() * *b).m;
    let c = ((r[0][0] + r[1][1] + r[2][2] - 1.0) * 0.5).max(-1.0).min(1.0);
    c.acos() * 57.295_779_513_082_32
}

#[derive(Default)]
struct Stat {
    v: Vec<f64>,
}

impl Stat {
    fn add(&mut self, x: f64) {
        self.v.push(x);
    }

    fn mean(&self) -> f64 {
        if self.v.is_empty() { 0.0 } else { self.v.iter().sum::<f64>() / self.v.len() as f64 }
    }

    /// Sorts the values (as the C++ one did).
    fn median(&mut self) -> f64 {
        if self.v.is_empty() {
            return 0.0;
        }

        self.v.sort_by(f64::total_cmp);
        self.v[self.v.len() / 2]
    }

    fn max_abs(&self) -> f64 {
        self.v.iter().fold(0.0, |m, x| f64::max(m, x.abs()))
    }

    fn mean_abs(&self) -> f64 {
        if self.v.is_empty() { 0.0 } else { self.v.iter().map(|x| x.abs()).sum::<f64>() / self.v.len() as f64 }
    }

    fn median_abs(&self) -> f64 {
        let mut a = self.v.iter().map(|x| x.abs()).collect::<Vec<_>>();
        if a.is_empty() {
            return 0.0;
        }

        a.sort_by(f64::total_cmp);
        a[a.len() / 2]
    }

    fn signed_median(&self) -> f64 {
        let mut a = self.v.clone();
        if a.is_empty() {
            return 0.0;
        }

        a.sort_by(f64::total_cmp);
        a[a.len() / 2]
    }
}

struct Fit {
    tile: String,
    n: usize,
    min: f64,
    scale: f64,
    entry_min: f64,
    entry_scale: f64,
    rms: f64,
}

#[derive(Clone, Debug, Default)]
struct Row {
    tile: String,
    key: String,
    mode: String,
    parent_mode: String,
    depth: i32,
    world_y: f64,

    /// The y the file's transform chain gives (before snapping a landscape instance to the terrain).
    file_y: f64,
    list_y: f64,
    terrain: f64,
    terrain2: f64,
    dx: f64,
    dz: f64,
    rot: f64,
    rot_m: f64,
    lf_off: f64,
}

/// The tile.ted folders under a path, depth first (as std::filesystem::recursive_directory_iterator), sorted afterwards by the caller.
fn find_tiles(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            find_tiles(&p, out);
        } else if p.file_name().is_some_and(|f| f == "tile.ted")
            && let Some(parent) = p.parent()
        {
            out.push(parent.to_path_buf());
        }
    }
}

fn relative(p: &Path, base: &Path) -> String {
    p.strip_prefix(base).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_default()
}

/// A mode without its "MSM_" prefix.
fn short(mode: &str) -> &str {
    mode.get(4..).unwrap_or("")
}

fn bucket(rows: &[Row], name: &str, pred: impl Fn(&Row) -> bool) {
    let (mut dxz, mut e_abs, mut e_rel, mut e_rel2, mut rot0, mut rotm) = (Stat::default(), Stat::default(), Stat::default(), Stat::default(), Stat::default(), Stat::default());
    for r in rows.iter().filter(|r| pred(r)) {
        dxz.add(r.dx.hypot(r.dz));
        e_abs.add(r.list_y - r.world_y);
        e_rel.add(r.world_y - r.terrain);
        e_rel2.add(r.world_y - r.terrain2);
        rot0.add(r.rot);
        rotm.add(r.rot_m);
    }

    if dxz.v.is_empty() {
        return;
    }

    println!(
        "{name:<34} n {:7} | xz mean {:.4} max {:.4} | y absolute: mean|e| {:.3} med|e| {:.3} max {:.3} | world y above the tool's terrain (min_height + scale * raw): mean|d| {:.3} med|d| {:.3} max {:.3}, signed median {:.3} | above raw * 3000 / 65535 - 20: med|d| {:.3} | rot as is: mean {:.3} max {:.2}, z mirrored: mean {:.3} max {:.2}",
        dxz.v.len(), dxz.mean_abs(), dxz.max_abs(), e_abs.mean_abs(), e_abs.median_abs(), e_abs.max_abs(), e_rel.mean_abs(), e_rel.median_abs(), e_rel.max_abs(),
        e_rel.signed_median(), e_rel2.median_abs(), rot0.mean(), rot0.max_abs(), rotm.mean(), rotm.max_abs(),
    );
}

fn worst(rows: &[Row], n: usize, title: &str, score: impl Fn(&Row) -> f64) {
    let mut order = rows.iter().enumerate().map(|(i, r)| (score(r), i)).collect::<Vec<_>>();
    order.sort_by(|a, b| b.0.total_cmp(&a.0));
    println!("\nworst {n} by {title}");
    for &(s, i) in order.iter().take(n) {
        let r = &rows[i];
        println!(
            "  {s:9.3}  {:<54} {:<34} {:<14} depth {}  y world {:.3} list {:.3} terrain {:.3}  dxz {:.4} {:.4} rot {:.2} / {:.2}", r.tile, r.key, short(&r.mode),
            r.depth, r.world_y, r.list_y, r.terrain, r.dx, r.dz, r.rot, r.rot_m,
        );
    }
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let Some(kit) = args.first().map(PathBuf::from) else {
        eprintln!("usage: placement_check <kit root> [--filter s] [--limit N] [--worst N] [--csv file]");
        std::process::exit(2);
    };

    let (mut filter, mut csv, mut limit, mut worst_n) = (String::new(), String::new(), usize::MAX, 20usize);
    let atoi = |s: &str| wce_formats::c_num::strtol(s).unwrap_or(0).max(0) as usize;
    let mut i = 1;
    while i < args.len() {
        let more = i + 1 < args.len();
        match args[i].as_str() {
            "--filter" if more => {
                i += 1;
                filter = args[i].clone();
            }

            "--limit" if more => {
                i += 1;
                limit = atoi(&args[i]);
            }

            "--worst" if more => {
                i += 1;
                worst_n = atoi(&args[i]);
            }

            "--csv" if more => {
                i += 1;
                csv = args[i].clone();
            }

            _ => {}
        }

        i += 1;
    }

    let raw_root = kit.join("raw_data");
    let work_root = kit.join("working_data");
    let battle = raw_root.join("terrain").join("tiles").join("battle");
    let mut found = vec![];
    find_tiles(&raw_root.join("terrain").join("tiles"), &mut found);
    let mut tiles = found
        .into_iter()
        .filter(|dir| {
            let list = work_root.join(dir.strip_prefix(&raw_root).unwrap_or(dir)).join("building_list.xml");
            std::fs::metadata(&list).is_ok_and(|m| m.len() >= 1024) && (filter.is_empty() || dir.to_string_lossy().replace('\\', "/").contains(&filter))
        })
        .collect::<Vec<_>>();
    tiles.sort();
    tiles.truncate(limit);
    println!("placement_check: {} tiles with a TEd building_list.xml", tiles.len());

    let mut rows: Vec<Row> = vec![];
    let mut fits = vec![];
    let (mut unmatched_tile, mut unmatched_list, mut matched, mut ordered, mut fallback_tiles, mut no_entry) = (0, 0, 0, 0, 0, 0);
    println!(
        "\n{:<62} {:>7} {:>7} {:>5}  {:<7} {:>9} {:>9}  {:>9} {:>9}  {:>8} {:>8}", "tile", "list", "match", "order", "landsc.", "med e_abs", "med above", "freef. abs",
        "med above", "max xz", "max rot",
    );
    for dir in &tiles {
        let list_path = work_root.join(dir.strip_prefix(&raw_root).unwrap_or(dir)).join("building_list.xml");
        let loaded = read_list(&list_path).and_then(|list| TileProject::open(dir).map(|p| (list, p)).map_err(|e| e.to_string()));
        let (mut list, project) = match loaded {
            Ok(x) => x,
            Err(ex) => {
                println!("{}: {ex}", dir.display());
                continue;
            }
        };

        if !project.info.found {
            no_entry += 1;
        }

        let doc = TileDocument::new(project, None);
        let t = doc.tile();
        let sc = t.tscene();
        let mut inst = t.instances().iter().copied().filter(|&e| sc.type_of(e) == "BUILDING_INSTANCE").collect::<Vec<_>>();
        inst.sort_unstable();
        let extent_z = t.extent_z();
        let key_of = |e: usize| sc.doc().get(e, "key").unwrap_or_default();

        // Pairing: the same order when the keys line up, else key + position.
        let mut pair: Vec<Option<usize>> = vec![None; inst.len()];
        let same_order = inst.len() == list.len() && inst.iter().zip(&list).all(|(&e, le)| key_of(e) == le.key);
        if same_order {
            for (i, p) in pair.iter_mut().enumerate() {
                *p = Some(i);
            }

            ordered += 1;
        } else {
            fallback_tiles += 1;
            let mut by_key: HashMap<String, Vec<usize>> = HashMap::new();
            for (i, le) in list.iter().enumerate() {
                by_key.entry(le.key.clone()).or_default().push(i);
            }

            for (i, &e) in inst.iter().enumerate() {
                let p = t.world_position(e);
                let Some(candidates) = by_key.get(&key_of(e)) else { continue };
                let mut best = 0.05;
                let mut bi = None;
                for &li in candidates {
                    if list[li].used {
                        continue;
                    }

                    let d = (list[li].x - f64::from(p.x)).hypot(list[li].z - (extent_z - f64::from(p.z)));
                    if d < best {
                        best = d;
                        bi = Some(li);
                    }
                }

                if let Some(bi) = bi {
                    pair[i] = Some(bi);
                    list[bi].used = true;
                }
            }
        }

        let (mut land_abs, mut land_rel, mut free_abs, mut free_rel, mut xz, mut rot) = (Stat::default(), Stat::default(), Stat::default(), Stat::default(), Stat::default(), Stat::default());
        let mut n_match = 0;

        // y = a + b * raw over the landscape buildings (raw: the tool's height sample as the 16-bit value).
        let (mut sx, mut sy, mut sxx, mut sxy, mut syy) = (0.0, 0.0, 0.0, 0.0, 0.0);
        let mut nfit = 0usize;
        let info = &doc.project().info;
        for (i, &e) in inst.iter().enumerate() {
            let Some(pi) = pair[i] else {
                unmatched_tile += 1;
                continue;
            };

            let le = &list[pi];
            let w = world_transform(sc, e);
            let p = t.world_position(e);
            let mut r = Row {
                tile: relative(dir, &battle), key: le.key.clone(), mode: sc.doc().get(e, "surface_mode").unwrap_or_default(),
                lf_off: num(sc.doc(), e, "lf_offset", 0.0), ..Default::default()
            };

            let mut a = sc.doc().elements()[e].parent;
            while let Some(anc) = a {
                let ty = &sc.doc().elements()[anc].name;
                if ty == "UserInstanceGroup" || ty == "INSTANCE_GROUP" {
                    if r.depth == 0 {
                        r.parent_mode = sc.doc().get(anc, "surface_mode").unwrap_or_default();
                    }

                    r.depth += 1;
                }

                a = sc.doc().elements()[anc].parent;
            }

            // Scene units, as the export's: the tool's world is scene space divided by the unit scale.
            let unit = doc.project().unit_scale;
            r.world_y = f64::from(p.y) * unit;
            r.file_y = w.position.y;
            r.list_y = le.y;
            r.terrain = f64::from(t.terrain_height(p.x, p.z)) * unit;

            // The other candidate formula: the 16-bit sample read as the campaign LF height, raw * lf_height / 65535 - vertical_offset
            // (_settings.xml: 3000 and 20).
            let raw = if info.scale != 0.0 { (r.terrain - info.min_height) / info.scale * 65535.0 } else { 0.0 };
            r.terrain2 = raw * 3000.0 / 65535.0 - 20.0;
            r.dx = le.x - f64::from(p.x);
            r.dz = le.z - (extent_z - f64::from(p.z));
            let rl = quat_matrix(&le.q);

            // The tool's matrix against the export's quaternion as is, and against the z mirrored frame.
            r.rot = angle_between(&w.rotation, &rl);
            r.rot_m = angle_between(&mirror_z(&w.rotation), &rl);
            let (e_abs, e_rel) = (le.y - r.world_y, r.world_y - r.terrain);
            let landscape = r.mode == "MSM_LANDSCAPE";
            if landscape && info.scale != 0.0 {
                let raw = (r.terrain - info.min_height) / info.scale * 65535.0;
                sx += raw;
                sy += r.world_y;
                sxx += raw * raw;
                sxy += raw * r.world_y;
                syy += r.world_y * r.world_y;
                nfit += 1;
            }

            if landscape { &mut land_abs } else { &mut free_abs }.add(e_abs);
            if landscape { &mut land_rel } else { &mut free_rel }.add(e_rel);
            xz.add(r.dx.hypot(r.dz));
            rot.add(r.rot.min(r.rot_m));
            rows.push(r);
            n_match += 1;
            matched += 1;
        }

        if !same_order {
            unmatched_list += list.iter().filter(|le| !le.used).count();
        }

        if nfit >= 8 {
            let n = nfit as f64;
            let den = n * sxx - sx * sx;
            if den.abs() > 1e-6 {
                let slope = (n * sxy - sx * sy) / den;
                let scale = slope * 65535.0;
                fits.push(Fit {
                    tile: relative(dir, &battle), n: nfit, scale, min: (sy - slope * sx) / n, entry_min: info.min_height, entry_scale: info.scale,
                    rms: (syy / n - (sy / n) * (sy / n) - (scale / 65535.0) * (scale / 65535.0) * (sxx / n - (sx / n) * (sx / n))).max(0.0).sqrt(),
                });
            }
        }

        println!(
            "{:<62} {:7} {:7} {:>5}  {:<7} {:9.3} {:9.3}  {:9.3} {:9.3}  {:8.4} {:8.3}", rows.last().map_or("?", |r| r.tile.as_str()), list.len(), n_match,
            if same_order { "yes" } else { "no" }, land_abs.v.len(), land_abs.median(), land_rel.median(), free_abs.median(), free_rel.median(), xz.max_abs(),
            rot.max_abs(),
        );
    }

    println!("\ntiles: {ordered} in the same order as the export, {fallback_tiles} paired by key and position, {no_entry} without a tile database entry");
    println!("buildings: {matched} paired, {unmatched_tile} of the tiles unpaired, {unmatched_list} of the exports unpaired");

    // ---- which height formula fits: y of the landscape buildings against the tool's raw sample, per tile
    println!(
        "\nlandscape buildings: y = min + scale * raw / 65535 fitted per tile against the tile database entry (tiles whose fit differs from the entry by more than 5 cm at the low end or 0.5 % of the scale)"
    );
    let mut shown = 0;
    for f in &fits {
        let (dmin, dsc) = (f.min - f.entry_min, f.scale - f.entry_scale);
        if dmin.abs() < 0.05 && dsc.abs() < 0.005 * f.entry_scale.abs() {
            continue;
        }

        println!(
            "  {:<60} n {:6}  fit min {:9.4} scale {:9.4} (rms {:.3} m) | entry min {:9.4} scale {:9.4} | d min {dmin:8.4} d scale {dsc:8.4}", f.tile, f.n, f.min, f.scale,
            f.rms, f.entry_min, f.entry_scale,
        );
        shown += 1;
    }

    println!("  {shown} of {} fitted tiles differ; the other {} agree with the entry's min_height and scale", fits.len(), fits.len() - shown);

    // ---- report by surface mode, depth in groups
    println!("\nby surface mode and depth in groups (units: metres, degrees)");
    bucket(&rows, "all", |_| true);
    for mode in ["MSM_LANDSCAPE", "MSM_FREEFORM"] {
        bucket(&rows, mode, |r| r.mode == mode);
        for d in 0..4 {
            let name = format!("{mode}, depth {d}{}", if d == 3 { "+" } else { "" });
            bucket(&rows, &name, |r| r.mode == mode && if d == 3 { r.depth >= 3 } else { r.depth == d });
        }
    }

    println!("\nby the mode of the nearest group (depth >= 1) against the building's own");
    for own in ["MSM_LANDSCAPE", "MSM_FREEFORM"] {
        for par in ["MSM_LANDSCAPE", "MSM_FREEFORM", ""] {
            let name = format!("own {} / group {}", short(own), if par.is_empty() { "(none)" } else { short(par) });
            bucket(&rows, &name, |r| r.mode == own && r.parent_mode == par && if par.is_empty() { r.depth == 0 } else { r.depth >= 1 });
        }
    }

    println!("\nby lf_offset");
    bucket(&rows, "lf_offset = 0", |r| r.lf_off == 0.0);
    bucket(&rows, "lf_offset != 0", |r| r.lf_off != 0.0);

    // ---- the worst ones (by the better of the two y hypotheses for their mode, then by xz, then by rotation)
    worst(&rows, worst_n, "|y error| under the better hypothesis", |r| (r.list_y - r.world_y).abs().min((r.list_y - (r.world_y - r.terrain)).abs()));
    worst(&rows, worst_n, "xz error", |r| r.dx.hypot(r.dz));
    worst(&rows, worst_n, "rotation error (degrees, better of as is / mirrored)", |r| r.rot.min(r.rot_m));

    if !csv.is_empty() {
        // The C++ stream wrote doubles with 6 significant digits.
        let mut out = "tile\tkey\tmode\tgroup_mode\tdepth\tlf_offset\tworld_y\tfile_y\tlist_y\tterrain\tdx\tdz\trot\trot_mirrored\n".to_owned();
        for r in &rows {
            out += &format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n", r.tile, r.key, r.mode, r.parent_mode, r.depth, printf_g(r.lf_off), printf_g(r.world_y), printf_g(r.file_y),
                printf_g(r.list_y), printf_g(r.terrain), printf_g(r.dx), printf_g(r.dz), printf_g(r.rot), printf_g(r.rot_m),
            );
        }

        let _ = std::fs::write(&csv, out);
    }
}
