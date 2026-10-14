//! Differential dump of wce_tile, in the format of the C++ harness: TEd number formatting and vector parsing, random edit sessions on tile.ted
//! fixtures, the height field, and tiles opened through a kit.
//!
//!   cargo run --release -p wce_tile --example tile_dump -- <fixtures folder> <temp folder>

#![allow(clippy::manual_is_multiple_of, reason = "the random draws mirror the C++ harness's expressions")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use wce_formats::png_file::PngFile;
use wce_tile::height_field::{Brush, HeightField};
use wce_tile::ted_scene::{TedScene, Vec3, format_general, format_ted_number, parse_vec3};
use wce_tile::tile_project::TileProject;
use wce_tile::world_transform::{euler_from_matrix, euler_matrix, rotate_world_yaw, set_world_position, set_world_rotation, world_transform};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 32) as u32
    }

    fn d(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * f64::from(self.next() % 1_000_000) / 1_000_000.0
    }

    fn v(&mut self, lo: f64, hi: f64) -> Vec3 {
        let a = self.d(lo, hi);
        let b = self.d(lo, hi);
        let c = self.d(lo, hi);
        [a, b, c]
    }

    fn pick(&mut self, v: &[usize]) -> usize {
        v[self.next() as usize % v.len()]
    }
}

fn fnv_from(h: u64, d: &[u8]) -> u64 {
    d.iter().fold(h, |h, &c| (h ^ u64::from(c)).wrapping_mul(1099511628211))
}

fn fnv(d: &[u8]) -> u64 {
    fnv_from(14695981039346656037, d)
}

fn f64s(v: &[f64]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn hsave(s: &TedScene) -> u64 {
    fnv(&s.save())
}

fn filtered(s: &TedScene, f: impl Fn(usize) -> bool) -> Vec<usize> {
    (0..s.doc().elements().len()).filter(|&i| f(i)).collect()
}

fn opt(v: Option<usize>) -> i64 {
    v.map_or(-1, |v| v as i64)
}

fn rel(p: &Path, base: &Path) -> String {
    p.strip_prefix(base).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

fn sorted_files(dir: &Path, out: &mut Vec<PathBuf>, name: Option<&str>, recursive: bool) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            if recursive {
                sorted_files(&p, out, name, recursive);
            }
        } else if name.is_none_or(|n| p.file_name().is_some_and(|f| f == n)) {
            out.push(p);
        }
    }
}

fn scene_op(sc: &mut TedScene, r: &mut Rng, op: u32, k: usize) -> Result<String, String> {
    let ins = filtered(sc, |i| sc.position(i).is_some() && !sc.is_layer(i));
    let lay = filtered(sc, |i| sc.is_layer(i));
    let wp = filtered(sc, |i| sc.position(i).is_some());
    let polys = sc.find_all("AI_HINT_POLYGON_INSTANCE");
    let flags = ["OIF_VISIBLE", "OIF_ACTIVE", "OIF_DRAGGABLE"];
    let ptypes = ["AI_HINT_POLYGON_INSTANCE", "NOGO_REGION_POLYGON_INSTANCE", "GO_REGION_POLYGON_INSTANCE", "VegetationExclusionPolygonInstance", "SIEGE_AI_POLYGON_INSTANCE"];
    let mut res = String::new();
    let before_of = |sc: &TedScene, r: &mut Rng, l: usize| {
        let kids = sc.children(l);
        if kids.is_empty() || r.next() % 2 != 0 { None } else { Some(r.pick(&kids)) }
    };

    match op {
        0 if !wp.is_empty() => {
            let e = r.pick(&wp);
            let d = r.v(-5.0, 5.0);
            sc.translate(e, d)?;
        }

        1 if !wp.is_empty() => {
            let e = r.pick(&wp);
            let v = r.v(-3.0, 3.0);
            sc.set_euler(e, v);
        }

        2 if !wp.is_empty() => {
            let e = r.pick(&wp);
            let f = flags[(r.next() % 3) as usize];
            let on = r.next() % 2 != 0;
            sc.set_flag(e, f, on);
        }

        3 if !wp.is_empty() => {
            let e = r.pick(&wp);
            let d = r.d(-3.0, 3.0);
            rotate_world_yaw(sc, e, d);
        }

        4 if !wp.is_empty() => {
            let e = r.pick(&wp);
            let v = r.v(-500.0, 500.0);
            set_world_position(sc, e, v);
        }

        5 if !wp.is_empty() => {
            let e = r.pick(&wp);
            let v = r.v(-3.0, 3.0);
            set_world_rotation(sc, e, &euler_matrix(v));
        }

        6 if !ins.is_empty() => {
            let e = r.pick(&ins);
            res = sc.duplicate(e)?.to_string();
        }

        7 if !ins.is_empty() => {
            let e = r.pick(&ins);
            sc.remove(e)?;
        }

        8 => {
            let l = if lay.is_empty() || r.next() % 3 == 0 { sc.scene_graph() } else { Some(r.pick(&lay)) };
            res = sc.add_layer(l, &format!("New & <layer> {k}"))?.to_string();
        }

        9 if !wp.is_empty() && !lay.is_empty() => {
            let e = r.pick(&wp);
            let l = r.pick(&lay);
            let before = before_of(sc, r, l);
            res = sc.move_element(e, l, before)?.to_string();
        }

        10 if !wp.is_empty() && !lay.is_empty() => {
            let n = 1 + r.next() % 3;
            let es = (0..n).map(|_| r.pick(&wp)).collect::<Vec<_>>();
            let l = r.pick(&lay);
            let before = before_of(sc, r, l);
            for moved in sc.move_elements(es, l, before)? {
                res += &format!("{moved},");
            }
        }

        11 if !lay.is_empty() => {
            let l = r.pick(&lay);
            let t = ptypes[(r.next() % 5) as usize];
            let n = 2 + r.next() % 4;
            let pts = (0..n).map(|_| r.v(-100.0, 100.0)).collect::<Vec<_>>();
            let mut extra = BTreeMap::new();
            if r.next() % 2 != 0 {
                extra.insert("hint_type".to_owned(), "AIH_FORT_PERIMETER".to_owned());
            }

            if r.next() % 3 == 0 {
                extra.insert("zzz".to_owned(), "a\"b".to_owned());
            }

            res = sc.add_polygon(Some(l), t, &format!("poly {k}"), &pts, &extra)?.to_string();
        }

        12 if !polys.is_empty() => {
            let p = r.pick(&polys);
            let np = sc.polygon_points(p).len();
            let which = r.next() % 3;
            let i = if np != 0 { r.next() as usize % np } else { 0 };
            let v = r.v(-60.0, 60.0);
            match which {
                0 => sc.set_polygon_point(p, i, v)?,
                1 => sc.insert_polygon_point(p, i, v)?,
                _ => res = u8::from(sc.remove_polygon_point(p, i)?).to_string(),
            }

            res += &format!(" area {:016x}", sc.polygon_area(p).to_bits());
        }

        13 if !wp.is_empty() => {
            let mark = sc.begin_edit()?;
            let e = r.pick(&wp);
            let d = r.v(-5.0, 5.0);
            sc.translate(e, d)?;
            let ins2 = filtered(sc, |i| sc.position(i).is_some() && !sc.is_layer(i));
            if !ins2.is_empty() {
                let c = r.pick(&ins2);
                sc.duplicate(c)?;
            }

            let rec = sc.end_edit(mark)?;
            let h1 = hsave(sc);
            sc.unapply(&rec)?;
            let h0 = hsave(sc);
            sc.apply(&rec)?;
            res = format!("{} {h1:016x} {h0:016x}", rec.len());
        }

        14 if !wp.is_empty() => {
            let snap = sc.snapshot();
            let e = r.pick(&wp);
            let d = r.v(-5.0, 5.0);
            sc.translate(e, d)?;
            let h1 = hsave(sc);
            sc.restore(&snap);
            res = format!("{h1:016x}");
        }

        15 if !wp.is_empty() => {
            let e = r.pick(&wp);
            let w = world_transform(sc, e);
            let h = fnv_from(fnv(&f64s(&w.position)), &f64s(&w.rotation));
            let h = fnv_from(h, &f64s(&euler_from_matrix(&w.rotation)));
            res = format!("{h:016x}");
        }

        16 if !wp.is_empty() => {
            let e = r.pick(&wp);
            res = format!("{} {} {} {} {}{}", sc.layer_path(e), sc.children(e).len(), sc.type_of(e), sc.name(e), u8::from(sc.visible(e)), u8::from(sc.active(e)));
            if let Some(s) = sc.scale(e) {
                res += &format!(" {:016x}", s[0].to_bits());
            }
        }

        _ => {}
    }

    Ok(res)
}

fn show(p: &mut TileProject, label: &str, fx: &Path, tmp: &Path, n: &mut usize, r: &mut Rng) {
    let i = &p.info;
    println!(
        "project {label} dir {} loc {} var {} cells {} {} min {:016x} scale {:016x} ns {:016x} ob {:016x} td {} found {} entry {} variation {} raw {}",
        rel(&p.dir, fx), i.location, i.variation, i.cells_x, i.cells_y, i.min_height.to_bits(), i.scale.to_bits(), i.normal_strength.to_bits(), i.overlap_border_size.to_bits(),
        i.raw_data_tri_density, u8::from(i.found), u8::from(p.has_entry()), opt(p.variation), p.raw_root.as_ref().map_or("-".to_owned(), |r| rel(r, fx))
    );

    println!(
        "  warning [{}] height {} bordered {} border {:016x} mpp {:016x} {:016x} extent {:016x} {:016x}", if p.warning.is_empty() { "" } else { "set" }, u8::from(p.has_height()),
        u8::from(p.bordered()), p.border_m().to_bits(), p.mpp_x().to_bits(), p.mpp_z().to_bits(), p.extent_x().to_bits(), p.extent_z().to_bits()
    );

    let mut h = 14695981039346656037u64;
    for _ in 0..200 {
        let x = r.d(-200.0, 700.0);
        let z = r.d(-200.0, 700.0);
        h = fnv_from(h, &p.height_at(x, z).to_le_bytes());
    }

    println!("  heights {h:016x}");
    let wp = filtered(&p.scene, |i| p.scene.position(i).is_some());
    if let Some(&first) = wp.first() {
        p.scene.translate(first, [1.0, 2.0, 3.0]).unwrap();
    }

    if let Some(field) = &mut p.height {
        field.begin_edit().unwrap();
        field.raise(&Brush { cx: 10.0, cy: 10.0, radius: 6.0, ..Default::default() }, 3.0).unwrap();
        field.end_edit();
        p.height_dirty = true;
    }

    let out = tmp.join(format!("p{n}"));
    *n += 1;
    let _ = std::fs::remove_dir_all(&out);
    let (wrote, msg) = p.save(&out, false, false).unwrap();
    println!("  save {} {msg}", u8::from(wrote));
    for f in ["tile.ted", "height_map_0.png"] {
        if let Ok(b) = std::fs::read(out.join(f)) {
            println!("  {f} {:016x}", fnv(&b));
        }
    }

    let (again, msg) = p.save(&out, true, true).unwrap();
    println!("  again {} {msg}", u8::from(again));
    println!("  bak {} {}", u8::from(out.join("tile.ted.bak").exists()), u8::from(out.join("height_map_0.png.bak").exists()));
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let (fx, tmp) = (PathBuf::from(&args[0]), PathBuf::from(&args[1]));
    let mut r = Rng(88172645463325252);

    let mut h = 14695981039346656037u64;
    for i in 0..300000 {
        let kind = r.next() % 6;
        let v = match kind {
            0 => r.d(-1000.0, 1000.0),
            1 => r.d(-1.0, 1.0) * 1e-5,
            2 => f64::from((r.next() % 20000) as i32 - 10000) / 8.0,
            3 => {
                let hi = u64::from(r.next());
                let b = (hi << 32) | u64::from(r.next());
                let v = f64::from_bits(b);
                if v.is_finite() && v.abs() <= 3.0e38 { v } else { 0.5 }
            }

            4 => r.d(-1.0, 1.0) * 1e9,
            _ => f64::from((r.next() % 2_000_001) as i32 - 1_000_000) / 1_000_000.0 + 0.0000005,
        };

        h = fnv_from(h, format!("{}|{}\n", format_ted_number(v), format_general(v)).as_bytes());
        if i % 30000 == 29999 {
            println!("numbers {i} {h:016x}");
        }
    }

    let alpha = b"0123456789.,-+e()vectoruler xinan";
    let mut h = 14695981039346656037u64;
    for _ in 0..100000 {
        let pre = r.next() % 3;
        let mut s = match pre {
            0 => "vector(".to_owned(),
            1 => "euler(".to_owned(),
            _ => String::new(),
        };

        let n = r.next() % 20;
        for _ in 0..n {
            s.push(alpha[r.next() as usize % alpha.len()] as char);
        }

        if r.next() % 2 != 0 {
            s.push(')');
        }

        let p = ["vector", "euler", ""][pre as usize];
        let line = match parse_vec3(&s, p) {
            Some(v) => format!("{:016x} {:016x} {:016x}\n", v[0].to_bits(), v[1].to_bits(), v[2].to_bits()),
            None => "none\n".to_owned(),
        };

        h = fnv_from(h, line.as_bytes());
    }

    println!("vectors {h:016x}");

    let mut teds = vec![];
    sorted_files(&fx.join("teds"), &mut teds, None, false);
    teds.sort();
    for path in &teds {
        let bytes = std::fs::read(path).unwrap();
        let mut sc = TedScene::parse(bytes.clone()).unwrap();
        let pos = filtered(&sc, |i| sc.position(i).is_some());
        let (mut fmt_ok, mut fmt_total) = (0, 0);
        for &e in &pos {
            let p = sc.position(e).unwrap();
            sc.set_position(e, p);
            for a in ["position", "orientation", "scale"] {
                let Some(txt) = sc.doc().get(e, a) else { continue };
                let pre = if a == "orientation" { "euler" } else { "vector" };
                let Some(v) = parse_vec3(&txt, pre) else { continue };
                fmt_total += 1;
                if format!("{pre}({},{},{})", format_ted_number(v[0]), format_ted_number(v[1]), format_ted_number(v[2])) == txt {
                    fmt_ok += 1;
                }
            }
        }

        println!(
            "ted {} v{} graph {} elements {} same {} modified {} fmt {fmt_ok}/{fmt_total}", path.file_name().unwrap().to_string_lossy(), sc.project_version(), opt(sc.scene_graph()),
            sc.doc().elements().len(), u8::from(sc.save() == bytes), u8::from(sc.modified())
        );

        for k in 0..150 {
            let op = r.next() % 17;
            let res = scene_op(&mut sc, &mut r, op, k).unwrap_or_else(|_| "err".to_owned());
            println!("  {k} op {op} {res} {:016x} {}", hsave(&sc), u8::from(sc.modified()));
        }

        sc.revert_all();
        println!("  reverted {:016x}", hsave(&sc));
    }

    let mut maps = vec![];
    sorted_files(&fx, &mut maps, Some("height_map_0.png"), true);
    maps.sort();
    for m in &maps {
        let png = PngFile::parse(&std::fs::read(m).unwrap()).unwrap();
        let mn = r.d(-20.0, 0.0);
        let scl = r.d(5.0, 900.0);
        let mut hf = HeightField::from_png(&png, mn, scl).unwrap();
        let bad = (0..65536u32).filter(|&raw| u32::from(hf.to_raw(hf.to_metres(raw as u16))) != raw).count();
        println!("map {} {}x{} bad {bad}", rel(m, &fx), hf.width(), hf.height());
        for k in 0..80 {
            let op = r.next() % 8;
            let (w, hh) = (f64::from(hf.width()), f64::from(hf.height()));
            let cx = r.d(-5.0, w + 5.0);
            let cy = r.d(-5.0, hh + 5.0);
            let radius = r.d(0.5, 25.0);
            let strength = r.d(0.0, 1.0);
            let falloff = r.d(0.0, 1.0);
            let b = Brush { cx, cy, radius, strength, falloff };
            let res: Result<String, String> = (|| {
                Ok(match op {
                    0..=4 => {
                        hf.begin_edit()?;
                        match op {
                            0 => {
                                let mm = r.d(-10.0, 10.0);
                                hf.raise(&b, mm)?;
                            }

                            1 => hf.smooth(&b)?,
                            2 => {
                                let t = r.d(-30.0, 900.0);
                                hf.flatten(&b, t)?;
                            }

                            3 => hf.set_level_under(&b)?,
                            _ => {
                                let x1 = r.d(0.0, w);
                                let y1 = r.d(0.0, hh);
                                let wd = r.d(0.5, 12.0);
                                let m0 = r.d(-20.0, 100.0);
                                let m1 = r.d(-20.0, 100.0);
                                hf.ramp(b.cx, b.cy, x1, y1, wd, m0, m1)?;
                            }
                        }

                        hf.end_edit();
                        String::new()
                    }

                    5 => u8::from(hf.undo()).to_string(),
                    6 => u8::from(hf.redo()).to_string(),
                    _ => {
                        let x = r.d(-3.0, w + 3.0);
                        let y = r.d(-3.0, hh + 3.0);
                        format!("{:016x}", hf.sample(x, y).to_bits())
                    }
                })
            })();

            let res = res.unwrap_or_else(|e| format!("err {e}"));
            let samples = hf.samples().iter().flat_map(|s| s.to_le_bytes()).collect::<Vec<_>>();
            println!("  {k} op {op} {res} {:016x} {}", fnv(&samples), hf.edit_depth());
        }

        let out = hf.to_png(&png).write().unwrap();
        println!("  png {:016x}", fnv(&out));
    }

    let kit = fx.join("kit/raw_data/terrain/tiles/battle");
    let opens = [
        kit.join("synth_set/two/aa"), kit.join("synth_set/two/bb/tile.ted"), kit.join("synth_set/two"), kit.join("synth_set/wide/aa"), kit.join("other_set/one/medium"),
        kit.join("synth_set/two/zz"), fx.join("loose_tile"), fx.join("nothing_here"),
    ];

    let mut n = 0;
    for o in &opens {
        match TileProject::open(o) {
            Ok(mut p) => show(&mut p, &rel(o, &fx), &fx, &tmp, &mut n, &mut r),
            Err(_) => println!("project {} failed", rel(o, &fx)),
        }
    }

    for (entry, v) in [("synth_set_two.xml", -1), ("synth_set_two.xml", 1), ("synth_set_two.xml", 5), ("other_set_one.xml", 0)] {
        let variation = usize::try_from(v).ok();
        match TileProject::open_entry(&kit.join("_tile_database/TILES").join(entry), variation) {
            Ok(mut p) => show(&mut p, &format!("{entry}#{v}"), &fx, &tmp, &mut n, &mut r),
            Err(e) => println!("entry {entry} {v} failed: {e}"),
        }
    }
}
