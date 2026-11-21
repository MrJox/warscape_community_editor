//! The figures of a campaign's characters at its start (`wce_render::campaign_character`): for every character the map shows
//! (`wce_terrain_io::attila::characters`), its art set's row, uniform, variant mesh, skeleton and idles; what fails, counted by reason.
//!
//!   cargo run --release -p wce_render --example campaign_characters_check -- <game dir> <startpos.esf> [mod packs, comma separated]

use std::collections::BTreeMap;
use std::sync::Arc;

use wce_render::campaign_character::{Afloat, CampaignCatalog, Wanted};
use wce_vfs::files::Files;
use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let game = GameProfile::from_env().clone();
    let mut config = Config::new(&a[0], game.clone());
    config.use_mods = a.get(2).is_some();
    config.mod_packs = Some(a.get(2).map(|m| m.split(',').map(str::to_owned).collect()).unwrap_or_default());
    let vfs: Arc<Vfs> = Arc::new(Vfs::mount(&config).expect("vfs"));
    let files: Arc<dyn Files> = vfs.clone();
    let schema =
        rpfm_lib::schema::Schema::load(&wce_foundation::settings::rpfm_schema_path(&game).expect("an rpfm schema"), None).expect("schema");
    let t0 = std::time::Instant::now();
    let catalog = CampaignCatalog::load(files.as_ref(), &schema);
    println!("catalog in {:.1?}", t0.elapsed());
    let startpos = std::fs::read(&a[1]).expect("startpos");
    let mut start = wce_terrain_io::attila::characters::read(&startpos).expect("characters");
    // The map's own slot cells too (a settlement whose startpos leaves them unset).
    let map = wce_terrain_io::attila::start::named(&startpos).expect("the startpos's map").1;
    let map_data = files.read(&format!("campaign_maps/{map}/map_data.esf")).expect("map_data.esf");
    let cities = wce_terrain_io::attila::cities::read(&map_data, &startpos, &Default::default()).expect("cities");
    start.settlement_cells.extend(cities.slots.iter().map(|s| (u32::from(s.cell.0), u32::from(s.cell.1))));
    let read = |p: &str| files.read(p).ok().map(|b| b.to_vec());
    let (mut ok, mut failed) = (0, BTreeMap::<String, usize>::new());
    let mut leaders = 0;
    let t0 = std::time::Instant::now();
    for c in start.on_map() {
        let afloat = if start.force_of(c).is_some_and(|f| f.navy) { Afloat::Navy } else { Afloat::Land };
        let art = if c.art_set.is_empty() {
            catalog.art_set_for(&c.kind, &c.faction, &c.subculture, &c.culture).unwrap_or_default().to_owned()
        } else {
            c.art_set.clone()
        };
        // Its own figure, then the others it may take: at sea (an army), in winter.
        let mut also = vec![(afloat, true)];
        if afloat == Afloat::Land {
            also.push((Afloat::Sea, false));
        }
        for (other, winter) in also {
            let w = Wanted { art_set: art.clone(), afloat: other, leader: c.leader, winter, faction: c.faction.clone(), seed: 0 };
            if let Err(e) = catalog.figure(&read, &w) {
                *failed.entry(format!("{other:?} winter {winter}: {}", e.split('\'').next().unwrap_or_default())).or_default() += 1;
            }
        }
        let w = Wanted { art_set: art.clone(), afloat, leader: c.leader, winter: false, faction: c.faction.clone(), seed: u64::from(c.id) };
        leaders += usize::from(c.leader);
        match catalog.figure(&read, &w) {
            Ok(f) => {
                if ok < 3 {
                    println!(
                        "  {} {} {:?}: {} parts, {} clips, {}",
                        c.id,
                        art,
                        afloat,
                        f.figure.parts.parts.len(),
                        f.figure.segments.len(),
                        f.figure.set.skeleton
                    );
                }
                ok += 1;
            }
            Err(e) => {
                if failed.values().sum::<usize>() < 12 {
                    println!("  fail {} {} {} {:?} at {:?}: {e}", c.id, c.kind, art, afloat, c.cell);
                }
                *failed
                    .entry(
                        e.split('\'').next().unwrap_or_default().to_owned()
                            + &e.split('\'').nth(1).map(|s| format!("'{s}'")).unwrap_or_default(),
                    )
                    .or_default() += 1
            }
        }
    }
    println!("leaders {leaders}");
    println!("{ok} figures in {:.1?}; failed {}", t0.elapsed(), failed.values().sum::<usize>());
    for (e, n) in failed.iter().take(20) {
        println!("  {n} x {e}");
    }
}
