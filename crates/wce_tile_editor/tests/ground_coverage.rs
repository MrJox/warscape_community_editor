//! The ground coverage goes into its layer, a group per kind, as one undo entry; a run replaces what its kind's group held but the brushed
//! strokes, and keeps its settings on the group. The first version's layer is moved into the cliffs' group.

use std::io::Write;

use wce_editor_core::document::Document;
use wce_tile::ground_coverage::{Differential, Kind, Params, Placed, State};
use wce_tile::tile_project::TileProject;
use wce_tile_editor::tile_document::TileDocument;
use wce_tile_editor::tile_document::ground_coverage::{BRUSHED_GROUP, COVERAGE_LAYER, CoverageSettings};

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

/// The first version's layer: its settings on it, a `Slope` group, and a rock put in by hand.
const OLD_LAYER: &str = r#"<LayerInstance flags=" OIF_VISIBLE|OIF_ACTIVE" export_type="LayerExportType_Normal" lf_offset="0" position="vector(0,0,0)" name="(AUTOGEN) Ground Coverage" surface_mode="MSM_FREEFORM" orientation="euler(0,0,0)" wce_autogen_params="min_rise=4" wce_autogen_seed="5" wce_autogen_rocks="old rocks" wce_autogen_plants="">
                    <children>
                        <UserInstanceGroup flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(0,0,0)" name="Slope 4" surface_mode="MSM_FREEFORM" orientation="euler(0,0,0)">
                            <children>
                                <BUILDING_INSTANCE flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(5,1,5)" name="r" surface_mode="MSM_FREEFORM" orientation="euler(0.0,0,0)" key="gaul_rock_01"/>
                            </children>
                        </UserInstanceGroup>
                        <BUILDING_INSTANCE flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(9,1,9)" name="mine" surface_mode="MSM_FREEFORM" orientation="euler(0.0,0,0)" key="gaul_rock_02"/>
                    </children>
                </LayerInstance>
"#;

fn open() -> (tempfile::TempDir, TileDocument) {
    open_with(TED)
}

fn open_with(ted: &str) -> (tempfile::TempDir, TileDocument) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::File::create(dir.path().join("tile.ted")).unwrap().write_all(ted.as_bytes()).unwrap();
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

fn settings(seed: u64) -> CoverageSettings {
    CoverageSettings { params: Params { min_rise: 5.0, ..Default::default() }, seed, rocks: "rocks".to_owned(), plants: "bushes".to_owned(), preset: "steep".to_owned() }
}

/// The names of an element's children.
fn names(d: &TileDocument, elem: usize) -> Vec<String> {
    let sc = d.tile().tscene();
    sc.children(elem).iter().map(|&c| sc.name(c)).collect()
}

#[test]
fn the_coverage_fills_its_layer_replaces_it_and_undoes() {
    let (_dir, mut d) = open();
    let before = d.tile().tscene().save();
    let settings = settings(99);
    let dropped = Differential { number: 3, state: Some(State::TooLow(1.0)), ..Default::default() };
    let counts = d.apply_coverage(Kind::Cliffs, &[slope(1, 20.0), dropped, slope(2, 40.0)], &settings).unwrap();
    assert_eq!((counts.slopes, counts.rocks, counts.plants, counts.exclusions), (2, 4, 2, 2));

    let sc = d.tile().tscene();
    let layer = d.coverage_layer().expect("the layer");
    assert_eq!(sc.name(layer), COVERAGE_LAYER);
    assert_eq!(names(&d, layer), ["Cliffs"]);
    assert_eq!(d.coverage_settings(Kind::Cliffs), Some(settings.clone()), "the settings are kept on the kind's group");
    assert_eq!(d.coverage_settings(Kind::Coast), None);
    let cliffs = d.coverage_group(Kind::Cliffs).unwrap();
    let groups = sc.children(cliffs);
    assert_eq!(names(&d, cliffs), ["Slope 1", "Slope 2"]);
    assert_eq!(sc.children(groups[0]).iter().map(|&e| sc.type_of(e).to_owned()).collect::<Vec<_>>(),
        ["BUILDING_INSTANCE", "BUILDING_INSTANCE", "VEGETATION_INSTANCE", "VegetationExclusionPolygonInstance"]);
    assert_eq!(d.coverage_items(Kind::Cliffs), 8);

    // A rock is freeform at its height, scaled when it is not at 1; the world positions are the generator's.
    let rock = sc.children(groups[0])[0];
    assert_eq!(sc.doc().get(rock, "surface_mode").as_deref(), Some("MSM_FREEFORM"));
    assert_eq!(sc.doc().get(rock, "wce_scale").as_deref(), Some("1.3"));
    assert_eq!(sc.doc().get(sc.children(groups[0])[1], "wce_scale"), None);
    let p = d.tile().world_position(rock);
    assert!((p.x - 20.0).abs() < 1e-3 && (p.y - 1.0).abs() < 1e-3 && (p.z - 10.0).abs() < 1e-3, "{p:?}");

    // Again: the layer and the group are the same ones, with only the new run in them.
    let counts = d.apply_coverage(Kind::Cliffs, &[slope(7, 30.0)], &settings).unwrap();
    assert_eq!(counts.slopes, 1);
    let sc = d.tile().tscene();
    assert_eq!(sc.find_all("LayerInstance").into_iter().filter(|&l| sc.name(l) == COVERAGE_LAYER).count(), 1);
    assert_eq!(names(&d, d.coverage_layer().unwrap()), ["Cliffs"]);
    assert_eq!(names(&d, d.coverage_group(Kind::Cliffs).unwrap()), ["Slope 7"]);

    // Each run is one undo entry; undoing both gives the file back.
    d.history_mut().undo();
    assert_eq!(d.coverage_items(Kind::Cliffs), 8);
    d.history_mut().undo();
    assert_eq!(d.tile().tscene().save(), before);
}

#[test]
fn each_kind_has_its_group_and_a_run_replaces_only_its_own() {
    let (_dir, mut d) = open();
    d.apply_coverage(Kind::Cliffs, &[slope(1, 20.0)], &settings(1)).unwrap();
    d.apply_coverage(Kind::Coast, &[slope(2, 40.0), slope(3, 50.0)], &settings(2)).unwrap();
    d.apply_coverage(Kind::Riverbank, &[slope(4, 60.0)], &settings(3)).unwrap();
    let layer = d.coverage_layer().unwrap();
    assert_eq!(names(&d, layer), ["Cliffs", "Coast", "Riverbanks"]);
    assert_eq!(names(&d, d.coverage_group(Kind::Coast).unwrap()), ["Shore 2", "Shore 3"]);
    assert_eq!(names(&d, d.coverage_group(Kind::Riverbank).unwrap()), ["Bank 4"]);

    // The coast again: the cliffs and the banks stay as they were.
    d.apply_coverage(Kind::Coast, &[slope(9, 30.0)], &settings(4)).unwrap();
    assert_eq!(names(&d, d.coverage_group(Kind::Cliffs).unwrap()), ["Slope 1"]);
    assert_eq!(names(&d, d.coverage_group(Kind::Coast).unwrap()), ["Shore 9"]);
    assert_eq!(names(&d, d.coverage_group(Kind::Riverbank).unwrap()), ["Bank 4"]);
    assert_eq!(d.coverage_settings(Kind::Coast).map(|s| s.seed), Some(4));
    assert_eq!(d.coverage_settings(Kind::Cliffs).map(|s| s.seed), Some(1));
    assert_eq!((d.coverage_items(Kind::Cliffs), d.coverage_items(Kind::Coast)), (4, 4));
}

/// A layer whose cliffs have a run (`Slope 1`) and a stroke of the brush (`Brushed` > `Stroke 1` > a rock).
const BRUSHED_LAYER: &str = r#"<LayerInstance flags=" OIF_VISIBLE|OIF_ACTIVE" export_type="LayerExportType_Normal" lf_offset="0" position="vector(0,0,0)" name="(AUTOGEN) Ground Coverage" surface_mode="MSM_FREEFORM" orientation="euler(0,0,0)">
                    <children>
                        <UserInstanceGroup flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(0,0,0)" name="Cliffs" surface_mode="MSM_FREEFORM" orientation="euler(0,0,0)" wce_autogen_params="" wce_autogen_seed="1" wce_autogen_rocks="rocks" wce_autogen_plants="">
                            <children>
                                <UserInstanceGroup flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(0,0,0)" name="Slope 1" surface_mode="MSM_FREEFORM" orientation="euler(0,0,0)">
                                    <children>
                                        <BUILDING_INSTANCE flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(5,1,5)" name="r" surface_mode="MSM_FREEFORM" orientation="euler(0.0,0,0)" key="gaul_rock_01"/>
                                    </children>
                                </UserInstanceGroup>
                                <UserInstanceGroup flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(0,0,0)" name="Brushed" surface_mode="MSM_FREEFORM" orientation="euler(0,0,0)">
                                    <children>
                                        <UserInstanceGroup flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(0,0,0)" name="Stroke 1" surface_mode="MSM_FREEFORM" orientation="euler(0,0,0)">
                                            <children>
                                                <BUILDING_INSTANCE flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(7,1,7)" name="b" surface_mode="MSM_FREEFORM" orientation="euler(0.0,0,0)" key="gaul_rock_02"/>
                                            </children>
                                        </UserInstanceGroup>
                                    </children>
                                </UserInstanceGroup>
                            </children>
                        </UserInstanceGroup>
                    </children>
                </LayerInstance>
"#;

/// The tile with `layer` at the end of its scene graph.
fn with_layer(layer: &str) -> String {
    let ted = TED.replace("            </children>\n        </SCENE_GRAPH>", &format!("                {layer}            </children>\n        </SCENE_GRAPH>"));
    assert_ne!(ted, TED);
    ted
}

#[test]
fn a_run_keeps_the_brushed_strokes() {
    let (_dir, mut d) = open_with(&with_layer(BRUSHED_LAYER));
    let cliffs = d.coverage_group(Kind::Cliffs).unwrap();
    assert_eq!(names(&d, cliffs), ["Slope 1", BRUSHED_GROUP]);
    assert_eq!(d.coverage_items(Kind::Cliffs), 1, "the brushed rock is not what a run replaces");

    d.apply_coverage(Kind::Cliffs, &[slope(2, 40.0)], &settings(2)).unwrap();
    let cliffs = d.coverage_group(Kind::Cliffs).unwrap();
    assert_eq!(names(&d, cliffs), [BRUSHED_GROUP, "Slope 2"]);
    let brushed = d.tile().tscene().children(cliffs)[0];
    assert_eq!(names(&d, brushed), ["Stroke 1"]);
    assert_eq!(d.tile().instances_below(&[brushed]).len(), 1);
}

#[test]
fn the_first_versions_layer_becomes_the_cliffs() {
    let (_dir, mut d) = open_with(&with_layer(OLD_LAYER));
    assert_eq!(d.coverage_settings(Kind::Cliffs).map(|s| (s.params.min_rise, s.seed)), Some((4.0, 5)), "read from the layer");
    assert_eq!(d.coverage_items(Kind::Cliffs), 1, "its Slope group, not the rock put in by hand");

    // A run of another kind moves the old groups into the cliffs' (and leaves the hand's rock alone).
    d.apply_coverage(Kind::Coast, &[slope(2, 40.0)], &settings(2)).unwrap();
    let layer = d.coverage_layer().unwrap();
    assert_eq!(names(&d, layer), ["mine", "Cliffs", "Coast"]);
    assert_eq!(names(&d, d.coverage_group(Kind::Cliffs).unwrap()), ["Slope 4"]);
    assert_eq!(d.coverage_settings(Kind::Cliffs).map(|s| s.seed), Some(5), "still the layer's");

    // A run of the cliffs replaces them, and its settings are its group's from then on.
    d.apply_coverage(Kind::Cliffs, &[slope(1, 20.0)], &settings(7)).unwrap();
    assert_eq!(names(&d, d.coverage_group(Kind::Cliffs).unwrap()), ["Slope 1"]);
    assert_eq!(d.coverage_settings(Kind::Cliffs).map(|s| s.seed), Some(7));
    assert_eq!(names(&d, d.coverage_layer().unwrap()), ["mine", "Cliffs", "Coast"]);
}
