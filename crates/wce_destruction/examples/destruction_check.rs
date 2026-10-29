//! Generates destroyed buildings with the default parameters and checks them: twice, the same bytes; every model, animation, `.meta`
//! and the logic file read back; each collapse's blocks bound to its bones; the levels have the original's LODs; the rows copy. With
//! `--out <folder>` the files are written there (and a pack, `wce_destruction_check.pack`, with their rows).
//!
//!   cargo run --release -p wce_destruction --example destruction_check -- <game folder> <schema_att.ron> <key>... [--out <folder>] [--levels n] [--seed s]

use std::path::{Path, PathBuf};
use std::time::Instant;

use wce_destruction::generate::{Options, generate};
use wce_destruction::params::Params;
use wce_destruction::rows::{category_of, rows};
use wce_destruction::source::{Source, Templates, decode_model};
use wce_formats::anim::Animation;
use wce_formats::anim_meta::MetaFile;
use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    if let Err(e) = run() {
        eprintln!("destruction_check: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let flag = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    let skip = ["--out", "--levels", "--seed"];
    let positional = args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && !(*i > 0 && skip.contains(&args[i - 1].as_str()))).map(|(_, a)| a.clone()).collect::<Vec<_>>();
    let [game, schema, keys @ ..] = positional.as_slice() else {
        return Err("usage: destruction_check <game folder> <schema_att.ron> <key>... [--out <folder>] [--levels n] [--seed s]".into());
    };

    let vfs = Vfs::mount(&Config::new(PathBuf::from(game), GameProfile::ATTILA)).map_err(|e| e.to_string())?;
    let schema = rpfm_lib::schema::Schema::load(Path::new(schema), None).map_err(|e| e.to_string())?;
    let templates = Templates::load(&vfs);
    println!("templates: damage {} rubble {} stone sounds {:?} wood sounds {:?}", templates.damage.is_some(), templates.rubble.is_some(), templates.stone, templates.wood);
    let exists = |p: &str| vfs.read(p).is_ok();
    let out_dir = flag("--out").map(PathBuf::from);
    let mut bad = 0;
    let mut pack_files = vec![];
    let mut pack_tables = vec![];
    for key in keys {
        let category = category_of(&vfs, &schema, key);
        let source = Source::read(&vfs, key, &category)?;
        let new_key = format!("wce_check_{key}");
        let mut params = Params::defaults(&new_key, &source.bounds, &category);
        if let Some(n) = flag("--levels").and_then(|n| n.parse::<usize>().ok()) {
            params.set_level_count(n, &source.bounds, &category);
        }

        if let Some(s) = flag("--seed").and_then(|n| n.parse::<u64>().ok()) {
            params.seed = s;
        }

        println!("== {key} ({category}): {} pieces, bounds {:?} .. {:?}", source.pieces.len(), source.bounds.min.as_slice(), source.bounds.max.as_slice());
        let t = Instant::now();
        let a = generate(&source, &params, &templates, &exists, Options::default())?;
        let ms = t.elapsed().as_millis();
        let b = generate(&source, &params, &templates, &exists, Options::default())?;
        let same = a.files == b.files;
        println!("  generated in {ms} ms, {} files; again the same bytes: {same}", a.files.len());
        bad += usize::from(!same);
        for c in &a.collapses {
            println!("  collapse piece{:02} into level {}: {} fragments, {:.2} s, {} effects", c.piece, c.level, c.fragments, c.duration, c.effects.len());
        }

        for n in &a.notes {
            println!("  note: {n}");
        }

        for (name, bytes) in &a.files {
            let problem = check(name, bytes, &a, &source);
            match problem {
                Ok(what) => println!("  ok {name} ({} KB) {what}", bytes.len() / 1024),
                Err(e) => {
                    bad += 1;
                    println!("  BAD {name}: {e}");
                }
            }
        }

        let (tables, notes) = rows(&vfs, &schema, key, &new_key, source.logic().map(Vec::as_slice), a.logic.as_deref(), a.levels)?;
        for n in notes {
            println!("  rows: {n}");
        }
        println!("  rows: {:?}", tables.iter().map(|(t, db)| (t.clone(), db.data().len())).collect::<Vec<_>>());

        if let Some(dir) = &out_dir {
            let folder = dir.join(format!("rigidmodels/buildings/{new_key}"));
            std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
            for (name, bytes) in &a.files {
                std::fs::write(folder.join(name), bytes).map_err(|e| e.to_string())?;
                pack_files.push((format!("rigidmodels/buildings/{new_key}/{name}"), bytes.clone()));
            }
            pack_tables.extend(tables);
        }
    }

    if let Some(dir) = &out_dir {
        let pack = dir.join("wce_destruction_check.pack");
        wce_walls::pack::write_pack(&pack, GameProfile::ATTILA.key, pack_files, pack_tables)?;
        println!("written: {}", pack.display());
    }

    if bad > 0 {
        return Err(format!("{bad} problems"));
    }

    println!("all good");
    Ok(())
}

fn check(name: &str, bytes: &[u8], g: &wce_destruction::generate::Generated, source: &Source) -> Result<String, String> {
    if name.ends_with(".rigid_model_v2") {
        let m = decode_model(bytes)?;
        let blocks = m.lods().iter().map(|l| l.mesh_blocks().len()).collect::<Vec<_>>();
        let verts = m.lods().first().map_or(0, |l| l.mesh_blocks().iter().map(|b| b.mesh().vertices().len()).sum::<usize>());
        if name.ends_with("_anim.rigid_model_v2") {
            let anim = g.files.get(&name.replace(".rigid_model_v2", ".anim")).ok_or("no .anim")?;
            let a = Animation::decode(anim, usize::MAX).ok_or("the .anim doesn't read")?;
            let top = m.lods()[0].mesh_blocks().iter().map(|b| *b.material().i_matrix_index()).max().unwrap_or(0);
            if top as usize >= a.parents.len() {
                return Err(format!("a block on bone {top}, the animation has {}", a.parents.len()));
            }
            if m.skeleton_id() != "building" || m.lods().len() != 1 {
                return Err("not a one-LOD 'building' model".into());
            }
            return Ok(format!("bones {} frames {} {:.1} s, blocks {blocks:?}, LOD 0 {verts} vertices", a.parents.len(), a.frames.len(), a.duration()));
        }

        let number = name.split("_piece").last().and_then(|r| r.get(..2)).and_then(|n| n.parse::<u32>().ok());
        if number.is_some_and(|n| source.other_pieces.contains(&n)) {
            return Ok(format!("copied, blocks {blocks:?}"));
        }
        let state = source.state_pieces.iter().find(|p| Some(p.number) == number).and_then(|p| p.states.iter().find(|(s, _)| name.contains(&format!("{s}.rigid_model_v2"))));
        let original = match state {
            Some((_, m)) => m.lods().len(),
            None => source.pieces.iter().find(|p| Some(p.number) == number).map_or(0, |p| p.model.lods().len()),
        };
        if m.lods().len() != original {
            return Err(format!("{} LODs, the original {original}", m.lods().len()));
        }
        for b in m.lods().iter().flat_map(|l| l.mesh_blocks()) {
            if b.mesh().indices().iter().any(|&i| i as usize >= b.mesh().vertices().len()) {
                return Err("an index past the vertices".into());
            }
        }
        return Ok(format!("blocks {blocks:?}, LOD 0 {verts} vertices"));
    }

    if name.ends_with(".anim") {
        return Animation::decode(bytes, 1).map(|a| format!("{} bones", a.parents.len())).ok_or_else(|| "doesn't read".into());
    }

    if name.ends_with(".meta") {
        return MetaFile::parse(bytes).map(|m| format!("{} entries", m.entries.len())).ok_or_else(|| "doesn't read".into());
    }

    if name.ends_with(".cs2.parsed") {
        if !wce_walls::logic::round_trips(bytes)? {
            return Err("doesn't write back".into());
        }
        let v = summary(bytes)?;
        return Ok(v);
    }

    Ok(String::new())
}

fn summary(bytes: &[u8]) -> Result<String, String> {
    use rpfm_lib::files::Decodeable;
    let parsed = rpfm_lib::files::cs2_parsed::Cs2Parsed::decode(&mut std::io::Cursor::new(bytes), &None).map_err(|e| e.to_string())?;
    let v = serde_json::to_value(&parsed).map_err(|e| e.to_string())?;
    let mut s = vec![];
    for p in v["pieces"].as_array().into_iter().flatten() {
        for d in p["destructs"].as_array().into_iter().flatten() {
            let n = |k: &str| d[k].as_array().map_or(0, Vec::len);
            s.push(format!("{}: {} platforms, {} lines, {} outlines, {} feet, collision {} tris", d["name"].as_str().unwrap_or_default(), n("platforms"), n("ef_lines"), n("collision_outlines"), n("orange_thingies"), d["collision_3d"]["triangles"].as_array().map_or(0, Vec::len)));
        }
    }
    Ok(s.join("; "))
}
