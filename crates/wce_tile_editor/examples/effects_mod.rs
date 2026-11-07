//! Builds a mod pack that lights what Attila leaves dark: it drops the effects of a standing piece's `.meta`, and plays the `vfx_emitters`
//! of its logic file (docs/re/buildings.md, "What gates a piece's effects"). Each always-on effect of a `.meta` goes to the list of its
//! destruct, at the same place. The packs read are the game's; what comes out holds the patched logic files.
//!
//!   cargo run --release -p wce_tile_editor --example effects_mod -- <out pack, or - to only list> <game pack>...

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::Path;

use rpfm_lib::files::cs2_parsed::Cs2Parsed;
use rpfm_lib::files::{Decodeable, DecodeableExtraData, Encodeable};
use serde_json::{Value, json};
use wce_formats::anim_meta::{MetaEffect, meta_effects};

/// Its model is 294 bytes and the game stops with a runtime error when it is placed; nothing attaches it.
const LEFT_OUT: &[&str] = &["greek_torch_sconce"];

/// The always-on effects of a `.meta` (the `.meta` itself keeps them: the game ignores it).
fn always_on(bytes: &[u8]) -> Vec<MetaEffect> {
    let effects = meta_effects(bytes).unwrap_or_default();
    effects.into_iter().filter(|e| e.tag.is_empty() && e.volume.is_none() && !e.effect.starts_with("destruction_vfx")).collect()
}

/// A place as a logic file's matrix: the identity, the place in its last row.
fn matrix(at: [f32; 3]) -> Value {
    let mut m = serde_json::Map::new();
    for r in 0..4 {
        for c in 0..4 {
            let v = if r == 3 && c < 3 { at[c] } else if r == c { 1.0 } else { 0.0 };
            m.insert(format!("m{r}{c}"), json!(v));
        }
    }

    Value::Object(m)
}

fn decode(bytes: &[u8]) -> Result<Cs2Parsed, String> {
    Cs2Parsed::decode(&mut Cursor::new(bytes), &Some(DecodeableExtraData::default())).map_err(|e| e.to_string())
}

fn encode(mut parsed: Cs2Parsed) -> Result<Vec<u8>, String> {
    let mut out = vec![];
    parsed.encode(&mut out, &None).map_err(|e| e.to_string())?;
    Ok(out)
}

/// The logic file with the effects added, each list by destruct name.
fn patched(bytes: &[u8], by_destruct: &BTreeMap<String, Vec<MetaEffect>>) -> Result<Vec<u8>, String> {
    let parsed = decode(bytes)?;
    if encode(parsed.clone())? != bytes {
        return Err("does not survive a round trip".to_owned());
    }

    let mut v = serde_json::to_value(&parsed).map_err(|e| e.to_string())?;
    let mut added = 0;
    for piece in v["pieces"].as_array_mut().ok_or("no pieces")? {
        for d in piece["destructs"].as_array_mut().ok_or("no destructs")? {
            let name = d["name"].as_str().ok_or("a destruct without a name")?.to_lowercase();
            let Some(effects) = by_destruct.get(&name) else { continue };
            let list = d["vfx_emitters"].as_array_mut().ok_or("this version has no vfx_emitters")?;
            for (n, e) in effects.iter().enumerate() {
                list.push(json!({ "key": format!("{name}_emitter_{}_{:02}", e.effect, n + 1), "matrix_1": matrix(e.position) }));
                added += 1;
            }
        }
    }

    if added != by_destruct.values().map(Vec::len).sum::<usize>() {
        return Err(format!("{added} placed of {by_destruct:?}"));
    }

    encode(serde_json::from_value(v).map_err(|e| e.to_string())?)
}

fn main() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let (out, packs) = args.split_first().ok_or("<out pack, or -> <game pack>...")?;
    let mut files = BTreeMap::new();
    for pack in packs {
        files.extend(wce_walls::pack::read_pack_files(Path::new(pack), "attila")?);
    }

    // Building key -> destruct -> its effects.
    let mut buildings: BTreeMap<String, BTreeMap<String, Vec<MetaEffect>>> = BTreeMap::new();
    for (path, bytes) in files.iter().filter(|(p, _)| p.starts_with("rigidmodels/buildings/") && p.ends_with(".meta") && !p.ends_with("_anim.meta")) {
        let effects = always_on(bytes);
        let (Some(key), Some(file), false) = (path.split('/').nth(2), path.rsplit('/').next(), effects.is_empty()) else { continue };
        for e in &effects {
            println!("{path}: {} at {:?}, turned {:?}, scale {} (start {} end {})", e.effect, e.position, e.rotation, e.scale, e.start, e.end);
        }

        if LEFT_OUT.contains(&key) {
            println!("  left out");
            continue;
        }

        let Some(destruct) = file.find("_piece").map(|at| file[at + 1..].trim_end_matches(".meta").to_lowercase()) else { continue };
        buildings.entry(key.to_owned()).or_default().entry(destruct).or_default().extend(effects);
    }

    let mut patches = vec![];
    for (key, by_destruct) in &buildings {
        let path = format!("rigidmodels/buildings/{key}/{key}_tech.cs2.parsed");
        let bytes = files.get(&path).ok_or(format!("{path}: not in these packs"))?;
        let new = patched(bytes, by_destruct).map_err(|e| format!("{path}: {e}"))?;
        println!("{path}: {} to {} bytes", bytes.len(), new.len());
        patches.push((path, new));
    }

    if out != "-" {
        wce_walls::pack::write_pack(Path::new(out), "attila", patches, vec![])?;
    }

    Ok(())
}
