//! Kit tiles through the model and back: one folder per tile.ted version (battle and campaign), and the cases the corpus showed (the
//! largest file, a river, NaNs, Photoshop images, every TIFF layout, legacy marker exports, a sub folder, CRLF-broken PNGs).
//!
//! Needs `WCE_ATTILA_KIT`: the Assembly Kit folder (`<Attila>/assembly_kit`), or a path list of kits (the Windows install has
//! `assembly_kit_battle_tilemap` and `assembly_kit_campaign_tilemap`); without it the test says so and passes. The whole kit: the
//! `tile_roundtrip_check` example.

use std::path::PathBuf;

use wce_tile_io::attila::TedProject;
use wce_tile_io::{PartRead, TileFiles, TileFormat};

const SAMPLES: &[&str] = &[
    "battle/settlement_eastern_cities/eastern_city_a/medium",
    "battle/plains/1x1/1x1_aa",
    "battle/plains/3x1/3x1_aa",
    "battle/river_ford/2x2_lb_bl_ford/aa",
    "battle/terrain_plains_med/2x1/aa",
    "battle/atlantic_mountains/med_point_45_l/aa",
    "battle/farm_nordic/8x8/aa",
    "battle/dunes/8x8/8x8_aa",
    "battle/atlantic_mountains/custom_mont_blanc/aa",
    "battle/roads/2x2_river_bl_lt_road/aa",
    "battle/gaul_impass_forest/small_oval_45_l/aa",
    "battle/sea/2x2/2x2_aa",
    "battle/dunes/1x1/1x1_aa",
    "battle/atlantic_mountains/med_point_30_l/aa",
    "battle/sea/10x10/10x10_aa",
    "campaign/roads/2x2_junction_br_tl_tr/bb",
    "campaign/sea_coast/1x1_straight/aa",
    "campaign/roads/6x2_straight_l0_r1/bb",
    "campaign/roads/6x2_straight_b0_r0/bb",
    "campaign/river/1x1_straight/aa",
    "campaign/alpine_mountains/large_point_45_l/aa",
    "campaign/alpine_mountains/custom_monte_rosa/aa",
    "campaign/sea/16x16/aa",
    "campaign/atlantic_mountains/large_oval_45_r/aa",
    "campaign/mud_01/1x1/aa",
];

#[test]
fn kit_samples_round_trip() {
    let Some(kits) = std::env::var_os("WCE_ATTILA_KIT") else {
        eprintln!("WCE_ATTILA_KIT is not set: kit samples skipped");
        return;
    };

    let roots = std::env::split_paths(&kits).map(|k| k.join("raw_data").join("terrain").join("tiles")).collect::<Vec<PathBuf>>();
    let mut failures = vec![];
    for rel in SAMPLES {
        let Some(dir) = roots.iter().map(|r| r.join(rel)).find(|d| d.join("tile.ted").is_file()) else {
            failures.push(format!("{rel}: not in {roots:?}"));
            continue;
        };

        let files = TileFiles::from_kit_dir(&dir).unwrap_or_else(|e| panic!("{rel}: {e}"));
        let (tile, report) = TedProject.read(&files).unwrap_or_else(|e| panic!("{rel}: {e}"));
        let (out, _) = TedProject.write(&tile).unwrap_or_else(|e| panic!("{rel}: {e}"));
        for (part, how) in &report.parts {
            let (orig, new) = match &files.entry {
                Some((name, blob)) if name == part => (blob, &out.entry.as_ref().expect("entry written").1),
                _ => (&files.folder[part], &out.folder[part]),
            };

            if orig.bytes().unwrap() != new.bytes().unwrap() {
                failures.push(format!("{rel}/{part} differs ({how:?})"));
            }
        }

        // Only the CRLF-broken PNGs of plains/3x1/3x1_aa do not read.
        let failed = report.parts.iter().filter(|p| matches!(p.1, PartRead::Failed(_))).count();
        if failed != if rel.ends_with("3x1_aa") { 3 } else { 0 } {
            failures.push(format!("{rel}: {failed} parts did not read"));
        }

        let (back, _) = TedProject.read(&out).unwrap();
        if back.semantic() != tile.semantic() {
            failures.push(format!("{rel}: another model read back"));
        }

        if report.xml.unknown.values().sum::<usize>() != 0 {
            failures.push(format!("{rel}: unknown attributes {:?}", report.xml.unknown));
        }
    }

    assert!(failures.is_empty(), "{failures:#?}");
}
