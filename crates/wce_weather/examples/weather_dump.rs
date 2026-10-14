//! Differential dump of the weather tool's documents, in the format of the C++ harness: `std::shuffle` on mt19937, the sample map over many
//! seeds, and random edit sessions on weather files (attribute and structural edits, undo, redo, revert).
//!
//!   cargo run --release -p wce_weather --example weather_dump -- <folder with rigidmodels/buildings> <folder with weather/>

use std::path::{Path, PathBuf};

use wce_editor_core::document::{Document, OutlinerNode};
use wce_editor_core::document_manager::DocumentManager;
use wce_foundation::math::{Float3, rotation_y, translation};
use wce_foundation::std_random::{Mt19937, shuffle};
use wce_scene::render_scene::{Payload, RenderScene, SourceRef};
use wce_vfs::files::FolderFiles;
use wce_weather::sample_map::{BUILDINGS, publish_sample_map};
use wce_weather::sample_map_document::{Placement, SampleMapDocument};
use wce_weather::xml_file_document::{Op, Source, XmlFileDocument};

fn fnv(d: &[u8]) -> u64 {
    d.iter().fold(14695981039346656037, |h, &c| (h ^ u64::from(c)).wrapping_mul(1099511628211))
}

fn floats(f: &[f32]) -> Vec<u8> {
    f.iter().flat_map(|v| v.to_le_bytes()).collect()
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 32) as u32
    }
}

fn count_nodes(n: &OutlinerNode) -> usize {
    1 + n.children.iter().map(count_nodes).sum::<usize>()
}

fn files_under(root: &Path, dir: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files_under(root, &path, out);
        } else {
            out.push(path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
        }
    }
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    for seed in [1u32, 7, 12345, 4_000_000_000] {
        for n in 1..=300 {
            let mut v = (0..n).collect::<Vec<i32>>();
            let mut rng = Mt19937::new(seed);
            shuffle(&mut v, &mut rng);
            let bytes = v.iter().flat_map(|i| i.to_le_bytes()).collect::<Vec<_>>();
            println!("shuffle {seed} {n} {:016x} {}", fnv(&bytes), rng.next_u32());
        }
    }

    let mut big = (0..100000).collect::<Vec<i32>>();
    shuffle(&mut big, &mut Mt19937::new(99));
    println!("shuffle big {:016x}", fnv(&big.iter().flat_map(|i| i.to_le_bytes()).collect::<Vec<_>>()));

    let models = FolderFiles(PathBuf::from(&args[0]));
    for seed in 1..=25u32 {
        for count in [8usize, 30] {
            let mut scene = RenderScene::default();
            let l = scene.add_layer(1, "s");
            let r = publish_sample_map(&models, &mut scene, l, 1, seed, count, BUILDINGS);
            println!("map {seed} {count} buildings {} errors {} bounds {:08x} {:08x} {:08x} {:08x} {:08x} {:08x}", r.buildings, r.errors.len(), r.bounds_min.x.to_bits(),
                r.bounds_min.y.to_bits(), r.bounds_min.z.to_bits(), r.bounds_max.x.to_bits(), r.bounds_max.y.to_bits(), r.bounds_max.z.to_bits());
            for c in &r.centres {
                println!("  c {:08x} {:08x}", c.x.to_bits(), c.z.to_bits());
            }

            for e in scene.entities() {
                let model = if let Payload::MeshInstance(mi) = &e.payload { mi.model.as_str() } else { "-" };
                println!("  e {} {model} {:016x}", e.source.item, fnv(&floats(e.world.data())));
            }
        }
    }

    // The document: placements, gizmo deltas, undo. The C++ harness makes a first document it does not use (it takes an id).
    let mut rnd = Rng(88172645463325252);
    {
        let _unused = SampleMapDocument::new(&models, 3, 8);
        let mut scene = RenderScene::default();
        let mut docs = DocumentManager::default();
        let id = docs.add(&mut scene, Box::new(SampleMapDocument::new(&models, 3, 8)), "weather");
        for k in 0..400 {
            let item = 1 + u64::from(rnd.next() % 8);
            let a = ((rnd.next() % 2000) as i32 - 1000) as f32 / 100.0;
            let c = ((rnd.next() % 2000) as i32 - 1000) as f32 / 100.0;
            let yaw = (rnd.next() % 628) as i32 as f32 / 100.0;
            let op = rnd.next() % 4;
            let g = if op <= 1 { u64::from(rnd.next() % 3) } else { 0 };
            let p = docs.find_as_mut::<SampleMapDocument>(id).unwrap();
            match op {
                0 => p.set_placement(item, Placement { offset: Float3::new(a, 0.0, c), yaw }, g),
                1 => {
                    let items = [SourceRef { doc: id, item }, SourceRef { doc: id, item: 1 + item % 8 }];
                    p.transform_items(&items, &(translation(Float3::new(a, 0.0, c)) * rotation_y(yaw)), g);
                }

                2 => {
                    p.history_mut().undo();
                }

                _ => {
                    p.history_mut().redo();
                }
            }

            docs.update(&mut scene);
            if k % 20 == 0 {
                let mut h = 14695981039346656037u64;
                for e in scene.entities() {
                    h ^= fnv(&floats(e.world.data())).wrapping_add(h << 6);
                }

                let p = docs.find_as::<SampleMapDocument>(id).unwrap();
                println!("doc {k} size {} pos {} scene {h:016x} outline {} {}", p.history().size(), p.history().position(), count_nodes(&p.outline()), p.describe(SourceRef { doc: id, item }));
            }
        }
    }

    let root = PathBuf::from(&args[1]);
    let mut envs = vec![];
    files_under(&root, &root.join("weather"), &mut envs);
    envs.sort();
    let texts = ["0.5", "1", "2.25", "true", "false", "0,0.5,1", "1,2,3,4", "abc", "-3.75e-2", "12", "0.1,0.2", "a&b<c", ""];
    let names = ["extra", "x", "fog", "name"];
    let ops = [Op::Remove, Op::Duplicate, Op::MoveUp, Op::MoveDown, Op::AddChild, Op::AddAttr, Op::RemoveAttr];
    for path in envs.iter().step_by(4) {
        let source = Source { vfs_path: path.clone(), disk: root.join(path), label: "loose".to_owned() };
        let mut d = XmlFileDocument::new(source, std::fs::read(root.join(path)).unwrap(), PathBuf::from("/nonexistent")).unwrap();
        println!("file {path} kind {} title {}", d.kind(), d.title());
        for k in 0..120 {
            let ne = d.xml().elements().len() as u32;
            if ne == 0 {
                break;
            }

            let e = (rnd.next() % ne) as usize;
            let na = d.xml().elements()[e].attrs.len() as u32;
            let op = rnd.next() % 10;
            if op < 4 && na != 0 {
                let at = (rnd.next() % na) as usize;
                let t = texts[(rnd.next() % 13) as usize];
                let g = u64::from(rnd.next() % 3);
                d.set_attr_text(e, at, t, g);
            } else if op == 4 && na != 0 {
                let at = (rnd.next() % na) as usize;
                let t = texts[(rnd.next() % 13) as usize];
                d.set_attr(e, at, t.as_bytes().to_vec(), 0);
            } else if op == 5 {
                let o = rnd.next() % 7;
                let at = if na != 0 { (rnd.next() % na) as usize } else { 0 };
                let nm = names[(rnd.next() % 4) as usize];
                let t = texts[(rnd.next() % 13) as usize];
                match d.structural(ops[o as usize], e, at, nm, t) {
                    Ok(sel) => println!("  op {o} {e} -> 1 {}", sel.map_or(-1, |s| s as i64)),
                    Err(_) => println!("  op {o} {e} -> 0 -2"),
                }
            } else if op == 6 || op == 7 {
                d.history_mut().undo();
            } else if op == 8 {
                d.history_mut().redo();
            } else if rnd.next().is_multiple_of(8) {
                d.revert_all();
            }

            if k % 10 == 9 {
                let bytes = d.xml().save();
                println!("  {k} save {:016x} size {} pos {} modified {} outline {} env {}", fnv(&bytes), d.history().size(), d.history().position(), u8::from(d.modified()),
                    count_nodes(&d.outline()), u8::from(d.environment().is_some()));
                if d.history().position() != 0 {
                    println!("  name {}", d.history().name(d.history().position() - 1));
                }
            }
        }
    }
}
