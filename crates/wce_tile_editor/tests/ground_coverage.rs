//! The ground coverage goes into its layer as one undo entry, replaces what the layer held when run again, and keeps its settings there.

use std::io::Write;

use wce_editor_core::document::Document;
use wce_tile::ground_coverage::{Differential, Params, Placed, State};
use wce_tile::tile_project::TileProject;
use wce_tile_editor::tile_document::TileDocument;
use wce_tile_editor::tile_document::ground_coverage::{COVERAGE_LAYER, CoverageSettings};

const TED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project version="16">
    <scene>
        <SCENE_GRAPH name="root" position="vector(0,0,0)" orientation="euler(0,0,0)">
            <children>
                <LayerInstance flags=" OIF_VISIBLE|OIF_ACTIVE" export_type="LayerExportType_Normal" lf_offset="0" position="vector(0,0,0)" name="Buildings" surface_mode="MSM_FREEFORM" orientation="euler(0,0,0)">
                    <children>
                        <BUILDING_INSTANCE flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(10.50,2,30)" name="a" surface_mode="MSM_LANDSCAPE" orientation="euler(0.0,0,0)" key="house"/>
                    </children>
                </LayerInstance>
            </children>
        </SCENE_GRAPH>
    </scene>
</project>
"#;

fn open() -> (tempfile::TempDir, TileDocument) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::File::create(dir.path().join("tile.ted")).unwrap().write_all(TED.as_bytes()).unwrap();
    let (w, h) = (64u32, 64u32);
    let pixels = (0..h).flat_map(|y| (0..w).flat_map(move |x| ((x * 300 + y * 200) as u16).to_be_bytes())).collect();
    let png = wce_formats::png_file::PngFile { width: w, height: h, bit_depth: 16, color_type: 0, pixels, pixels_edited: true, ..Default::default() };
    std::fs::write(dir.path().join("height_map_0.png"), png.write().unwrap()).unwrap();
    let doc = TileDocument::new(TileProject::open(dir.path()).unwrap(), None);
    (dir, doc)
}

fn placed(key: &str, x: f32, z: f32, scale: f32) -> Placed {
    Placed { key: key.to_owned(), x, y: 1.0, z, scale, yaw: 30.0 }
}

fn slope(number: usize, x: f32) -> Differential {
    Differential {
        number,
        state: Some(State::Kept),
        rocks: vec![placed("gaul_rock_01", x, 10.0, 1.3), placed("gaul_rock_02", x + 2.0, 12.0, 1.0)],
        plants: vec![placed("shrubs.shrub_rosec", x + 1.0, 8.0, 0.9)],
        exclusions: vec![vec![(x - 2.0, 6.0), (x + 4.0, 6.0), (x + 4.0, 10.0), (x - 2.0, 10.0)]],
        ..Default::default()
    }
}

#[test]
fn the_coverage_fills_its_layer_replaces_it_and_undoes() {
    let (_dir, mut d) = open();
    let before = d.tile().tscene().save();
    let settings = CoverageSettings { params: Params { min_rise: 5.0, ..Default::default() }, seed: 99, rocks: "rocks".to_owned(), plants: "bushes".to_owned() };
    let dropped = Differential { number: 3, state: Some(State::TooLow(1.0)), ..Default::default() };
    let counts = d.apply_coverage(&[slope(1, 20.0), dropped, slope(2, 40.0)], &settings).unwrap();
    assert_eq!((counts.slopes, counts.rocks, counts.plants, counts.exclusions), (2, 4, 2, 2));

    let sc = d.tile().tscene();
    let layer = d.coverage_layer().expect("the layer");
    assert_eq!(sc.name(layer), COVERAGE_LAYER);
    assert_eq!(d.coverage_settings(), Some(settings.clone()), "the settings are kept on the layer");
    let groups = sc.children(layer);
    assert_eq!(groups.iter().map(|&g| sc.name(g)).collect::<Vec<_>>(), ["Slope 1", "Slope 2"]);
    assert_eq!(sc.children(groups[0]).iter().map(|&e| sc.type_of(e).to_owned()).collect::<Vec<_>>(),
        ["BUILDING_INSTANCE", "BUILDING_INSTANCE", "VEGETATION_INSTANCE", "VegetationExclusionPolygonInstance"]);
    assert_eq!(d.coverage_items(), 8);

    // A rock is freeform at its height, scaled when it is not at 1; the world positions are the generator's.
    let rock = sc.children(groups[0])[0];
    assert_eq!(sc.doc().get(rock, "surface_mode").as_deref(), Some("MSM_FREEFORM"));
    assert_eq!(sc.doc().get(rock, "wce_scale").as_deref(), Some("1.3"));
    assert_eq!(sc.doc().get(sc.children(groups[0])[1], "wce_scale"), None);
    let p = d.tile().world_position(rock);
    assert!((p.x - 20.0).abs() < 1e-3 && (p.y - 1.0).abs() < 1e-3 && (p.z - 10.0).abs() < 1e-3, "{p:?}");

    // Again: the layer is the same one, with only the new run in it.
    let counts = d.apply_coverage(&[slope(7, 30.0)], &settings).unwrap();
    assert_eq!(counts.slopes, 1);
    let sc = d.tile().tscene();
    assert_eq!(sc.find_all("LayerInstance").into_iter().filter(|&l| sc.name(l) == COVERAGE_LAYER).count(), 1);
    let layer = d.coverage_layer().unwrap();
    assert_eq!(sc.children(layer).iter().map(|&g| sc.name(g)).collect::<Vec<_>>(), ["Slope 7"]);

    // Each run is one undo entry; undoing both gives the file back.
    d.history_mut().undo();
    assert_eq!(d.coverage_items(), 8);
    d.history_mut().undo();
    assert_eq!(d.tile().tscene().save(), before);
}
