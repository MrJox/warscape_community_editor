//! The binary building lists below a folder read as `attila::building_list` (every list), and against their XML twins where a pack ships
//! both (every building and attribute; numbers as the XML writes them, `%.6f`).
//!
//!   cargo run --release -p wce_tile_io --example tile_building_list_check -- <folder>

use std::io::Cursor;
use std::path::{Path, PathBuf};

use rpfm_lib::files::Decodeable;
use rpfm_lib::files::esf::ESF;
use wce_formats::c_num::printf_f_msvc;
use wce_formats::xml_doc::XmlDoc;
use wce_tile_io::attila::building_list::{BattlefieldBuilding, read};

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            files(&p, out);
        } else if p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("building_list") || n == "far_building_list")
            && p.extension().is_none()
        {
            out.push(p);
        }
    }
}

fn attrs(b: &BattlefieldBuilding) -> Vec<(&'static str, String)> {
    let f = |v: f32| printf_f_msvc(f64::from(v), 6);
    let t = |v: bool| if v { "true" } else { "false" }.to_owned();
    let ptype = match b.position_type {
        0 => "relative".to_owned(),
        1 => "absolute".to_owned(),
        2 => "lf_relative".to_owned(),
        n => n.to_string(),
    };

    vec![
        ("key", b.key.clone()), ("position_x", f(b.position[0])), ("position_y", f(b.position[1])), ("position_z", f(b.position[2])),
        ("position_type", ptype), ("orientation_i", f(b.orientation[0])), ("orientation_j", f(b.orientation[1])), ("orientation_k", f(b.orientation[2])),
        ("orientation_w", f(b.orientation[3])), ("scale_x", f(b.scale[0])), ("scale_y", f(b.scale[1])), ("scale_z", f(b.scale[2])),
        ("starting_damage", f(b.starting_damage)), ("on_fire", t(b.on_fire)), ("start_disabled", t(b.start_disabled)), ("weak_point", t(b.weak_point)),
        ("ai_breachable", t(b.ai_breachable)), ("indestructible", t(b.indestructible)), ("dockable", t(b.dockable)), ("toggleable", t(b.toggleable.unwrap_or(false))),
        ("lite", t(b.lite.unwrap_or(false))),
    ]
}

fn main() {
    let root = std::env::args().nth(1).expect("usage: tile_building_list_check <folder>");
    let mut all = vec![];
    files(Path::new(&root), &mut all);
    let (mut lists, mut buildings, mut failed, mut twins, mut compared, mut differ) = (0, 0, 0, 0, 0, 0);
    for p in &all {
        let bytes = std::fs::read(p).expect("read");
        let parsed = ESF::decode(&mut Cursor::new(&bytes), &None).map_err(|e| e.to_string()).and_then(|esf| read(&esf));
        let list = match parsed {
            Ok(l) => l,
            Err(e) => {
                failed += 1;
                println!("{}: {e}", p.display());
                continue;
            }
        };

        lists += 1;
        buildings += list.len();
        let xml = p.with_file_name(format!("{}.xml", p.file_name().unwrap().to_string_lossy()));
        let Ok(text) = std::fs::read(&xml) else { continue };
        twins += 1;
        let doc = XmlDoc::parse(text).expect("xml");
        let elems = (0..doc.elements().len()).filter(|&i| doc.elements()[i].name == "BATTLEFIELD_BUILDING").collect::<Vec<_>>();
        if elems.len() != list.len() {
            differ += 1;
            println!("{}: {} buildings, the XML has {}", p.display(), list.len(), elems.len());
            continue;
        }

        for (b, &e) in list.iter().zip(&elems) {
            compared += 1;
            for (name, value) in attrs(b) {
                if doc.get(e, name).as_deref() != Some(value.as_str()) {
                    differ += 1;
                    println!("{}: {} {name} = {value}, XML {:?}", p.display(), b.key, doc.get(e, name));
                }
            }
        }
    }

    println!("{lists} lists ({failed} did not read), {buildings} buildings; {twins} with an XML twin, {compared} buildings compared, {differ} differences");
    std::process::exit(i32::from(failed + differ != 0));
}
