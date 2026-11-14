//! Rome 2's battle locations through the Terrain tool's scene and back (`wce_terrain_io::built::scene`): for each map of the packs with a
//! `locations.blm`, the scene made from it and its deployment files, then written again: the blm and every deployment file byte for
//! byte. Exit 1 otherwise.
//!
//!   WCE_GAME_KEY=rome_2 cargo run --release -p wce_terrain_io --example built_scene_check -- <game dir> [--vanilla]

use std::collections::BTreeMap;

use wce_terrain_io::built::scene;
use wce_tile::battle_map::locations::BattleLocations;
use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut cfg = Config::new(&a[0], GameProfile::from_env().clone());
    if a.iter().any(|x| x == "--vanilla") {
        cfg.mod_packs = Some(vec![]);
    }

    let vfs = Vfs::mount(&cfg).expect("vfs");
    let mut bad = 0;
    for blm_path in vfs.list("terrain/battles/", "/locations.blm") {
        let folder = blm_path.trim_end_matches("/locations.blm");
        let t0 = std::time::Instant::now();
        let bytes = vfs.read(&blm_path).expect("blm");
        let blm = BattleLocations::parse(&bytes).expect("blm parses");
        let mut files = BTreeMap::new();
        let mut deployments = BTreeMap::new();
        for p in vfs.list(&format!("{folder}/deployment/"), ".xml") {
            let b = vfs.read(&p).expect("deployment");
            let name = p.rsplit('/').next().unwrap_or_default().to_owned();
            if let Some(n) = name.strip_prefix("deployment_").and_then(|s| s.strip_suffix(".xml")).and_then(|s| s.parse::<u32>().ok()) {
                deployments.insert(n, wce_tile::vegetation::ca_xml::parse(&b));
            }

            files.insert(format!("deployment/{name}"), b);
        }

        let p = match scene::scene(&blm, &deployments) {
            Ok(p) => p,
            Err(e) => {
                println!("{folder}: no scene: {e}");
                bad += 1;
                continue;
            }
        };

        let (blm2, deps) = scene::write(&p, &blm).expect("representable locations.blm candidate");
        let blm_same = blm2 == bytes;
        let made: BTreeMap<String, Vec<u8>> = deps.into_iter().collect();
        // The files no location names (numbers past the made ones) stay as they are: orphans.
        let orphans = files.keys().filter(|k| !made.contains_key(*k)).count();
        let files: BTreeMap<String, Vec<u8>> = files.into_iter().filter(|(k, _)| made.contains_key(k)).collect();
        let same = files.iter().filter(|(k, v)| made.get(*k) == Some(v)).count();
        let blm_at = blm2.iter().zip(&bytes).position(|(x, y)| x != y);
        let first = files.iter().find(|(k, v)| made.get(*k) != Some(v)).map(|(k, v)| {
            let m = made.get(k).cloned().unwrap_or_default();
            let at = m.iter().zip(v.iter()).position(|(x, y)| x != y).unwrap_or(m.len().min(v.len()));
            format!(
                "{k} at byte {at}: ours {:?} / shipped {:?}",
                String::from_utf8_lossy(&m[at.saturating_sub(40)..(at + 40).min(m.len())]),
                String::from_utf8_lossy(&v[at.saturating_sub(40)..(at + 40).min(v.len())])
            )
        });
        println!(
            "{folder}: {} entities; blm {}; deployment files {same} of {} the same ({} made, {orphans} orphans kept){}; {:.1} s",
            p.entities.len(),
            if blm_same { "the same".to_owned() } else { format!("DIFFERS (sizes {} / {}, first at {blm_at:?})", blm2.len(), bytes.len()) },
            files.len(),
            made.len(),
            first.map_or(String::new(), |f| format!("; first: {f}")),
            t0.elapsed().as_secs_f64()
        );
        if !blm_same || same != files.len() || made.len() != files.len() {
            bad += 1;
        }
    }

    if bad > 0 {
        std::process::exit(1);
    }
}
