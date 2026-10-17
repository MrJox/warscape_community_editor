//! The C++ `unit_tests` and `height_check` gates, on a small tile.ted and height map made here (the gates ran on kit tiles).

use std::collections::BTreeMap;

use wce_formats::png_file::PngFile;
use wce_foundation::math::{Double3, Double3x3};
use wce_tile::height_field::{Brush, HeightField};
use wce_tile::ted_scene::{TedScene, format_ted_number};
use wce_tile::world_transform::{euler_from_matrix, euler_matrix, rotate_world_yaw, set_world_position, world_transform};

const TED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project version="16">
    <scene>
        <SCENE_GRAPH name="root" position="vector(0,0,0)" orientation="euler(0,0,0)">
            <children>
                <LayerInstance flags=" OIF_VISIBLE|OIF_ACTIVE" export_type="LayerExportType_Normal" lf_offset="0" position="vector(10,0,-4)" name="Buildings" surface_mode="MSM_FREEFORM" orientation="euler(0.3,0,0)">
                    <children>
                        <BUILDING_INSTANCE flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(1.5,2,3)" name="a" surface_mode="MSM_LANDSCAPE" orientation="euler(1,0.1,0)" key="house_a"/>
                        <BUILDING_INSTANCE flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(-7,0,12.25)" name="b" surface_mode="MSM_LANDSCAPE" orientation="euler(-2,0,0.05)" key="house_b"/>
                        <BUILDING_INSTANCE flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(30,1,-8)" name="c" surface_mode="MSM_FREEFORM" orientation="euler(0,0,0)" key="house_c"/>
                    </children>
                </LayerInstance>
                <LayerInstance flags=" OIF_VISIBLE|OIF_ACTIVE" export_type="LayerExportType_Normal" lf_offset="0" position="vector(0,0,0)" name="Trees" surface_mode="MSM_FREEFORM" orientation="euler(0,0,0)">
                    <children>
                        <VEGETATION_INSTANCE flags=" OIF_VISIBLE" lf_offset="0" position="vector(5,0,5)" name="t" surface_mode="MSM_LANDSCAPE" orientation="euler(0,0,0)" key="tree" scale="1.5"/>
                    </children>
                </LayerInstance>
            </children>
        </SCENE_GRAPH>
    </scene>
</project>
"#;

#[test]
fn euler_round_trip() {
    let mut s = 1u64;
    let mut ang = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        -3.1 + 6.2 * (s % 1_000_000) as f64 / 1_000_000.0
    };

    let mut worst = 0.0f64;
    for _ in 0..20000 {
        let e = Double3::new(ang(), ang() * 0.5, ang());
        let (m, m2) = (euler_matrix(e), euler_matrix(euler_from_matrix(&euler_matrix(e))));
        for (a, b) in m.data().iter().zip(m2.data()) {
            worst = worst.max((a - b).abs());
        }
    }

    assert!(worst < 1e-9, "euler_from_matrix does not invert euler_matrix: {worst}");
}

#[test]
fn number_formatting() {
    assert_eq!(format_ted_number(1.0), "1");
    assert_eq!(format_ted_number(-0.0), "-0");
    assert_eq!(format_ted_number(12.5), "12.5");
    assert_eq!(format_ted_number(1235.059326), "1235.059326");
    assert_eq!(format_ted_number(-28.2578125), "-28.257813", "a tie rounds away from zero");
}

#[test]
fn structural_edits_and_transforms() {
    let bytes = TED.as_bytes().to_vec();
    let mut sc = TedScene::parse(bytes.clone()).unwrap();
    let buildings = sc.find_all("BUILDING_INSTANCE");
    let b = buildings[buildings.len() / 2];

    // Duplicate, then restore the snapshot: bytes identical.
    let snap = sc.snapshot();
    let copy = sc.duplicate(b).unwrap();
    assert_eq!(sc.type_of(copy), "BUILDING_INSTANCE", "duplicate gives a building");
    assert_ne!(sc.save(), bytes, "duplicate changes the file");
    sc.restore(&snap);
    assert_eq!(sc.save(), bytes, "restore after duplicate restores the bytes");

    // Duplicate then remove the copy: bytes identical.
    let copy = sc.duplicate(b).unwrap();
    sc.remove(copy).unwrap();
    assert_eq!(sc.save(), bytes, "duplicate + remove restores the bytes");

    // Yaw rotation: rotate by +0.3 then -0.3 returns to the same world rotation.
    let w0 = world_transform(&sc, b);
    rotate_world_yaw(&mut sc, b, 0.3);
    let w1 = world_transform(&sc, b);
    let close = |a: &Double3x3, b: &Double3x3| a.data().iter().zip(b.data()).all(|(a, b)| (a - b).abs() < 2e-5);
    assert!(close(&(Double3x3::rotation_y(0.3) * w0.rotation), &w1.rotation), "rotate_world_yaw applies Ry(delta) in world space");
    rotate_world_yaw(&mut sc, b, -0.3);
    let w2 = world_transform(&sc, b);
    assert!(close(&w0.rotation, &w2.rotation), "rotating back restores the world rotation");

    // set_world_position keeps the world position.
    set_world_position(&mut sc, b, Double3::new(w0.position.x + 6.0, w0.position.y, w0.position.z - 6.0));
    let w3 = world_transform(&sc, b);
    assert!((w3.position.x - (w0.position.x + 6.0)).abs() < 1e-3 && (w3.position.z - (w0.position.z - 6.0)).abs() < 1e-3, "set_world_position");
    sc.revert_all();
    assert_eq!(sc.save(), bytes);

    // Layers: add, move an instance into it.
    let mut s2 = TedScene::parse(bytes.clone()).unwrap();
    let root = s2.scene_graph();
    assert!(!s2.children(root.unwrap()).is_empty(), "root has layers");
    let before_count = s2.find_all("BUILDING_INSTANCE").len();
    let layer = s2.add_layer(root, "Test layer").unwrap();
    assert!(s2.name(layer) == "Test layer" && s2.type_of(layer) == "LayerInstance", "add_layer creates the layer");
    assert!(s2.children(layer).is_empty(), "new layer is empty");
    let bs = s2.find_all("BUILDING_INSTANCE");
    let victim = bs[bs.len() / 3];
    let key = s2.doc().get(victim, "key");
    let old_path = s2.layer_path(victim);
    let moved = s2.move_to_layer(victim, layer).unwrap();
    assert!(s2.type_of(moved) == "BUILDING_INSTANCE" && s2.doc().get(moved, "key") == key, "moved instance keeps its key");
    assert_eq!(s2.find_all("BUILDING_INSTANCE").len(), before_count, "move keeps the instance count");
    assert!(s2.layer_path(moved).contains("Test layer") && s2.layer_path(moved) != old_path, "moved instance is in the new layer");
    let s3 = TedScene::parse(s2.save()).unwrap();
    assert_eq!(s3.find_all("BUILDING_INSTANCE").len(), before_count, "saved text re-parses with the same count");
}

#[test]
fn polygons() {
    let mut sc = TedScene::parse(TED.as_bytes().to_vec()).unwrap();
    let orig = sc.save();
    let layer = sc.add_layer(sc.scene_graph(), "Poly layer").unwrap();
    let extra = [("hint_type", "AIH_FORT_PERIMETER"), ("polygon_closed", "1")].into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect::<BTreeMap<_, _>>();
    let pts = [[0.0, 1.0, 0.0], [20.0, 1.0, 0.0], [20.0, 1.0, 10.0], [0.0, 1.0, 10.0]].map(Double3::from);
    let poly = sc.add_polygon(Some(layer), "AI_HINT_POLYGON_INSTANCE", "Test hint", &pts, &extra).unwrap();
    assert_eq!(sc.polygon_points(poly).len(), 4, "polygon has 4 points");
    assert!((sc.polygon_area(poly) - 200.0).abs() < 1e-6, "rectangle area 200");
    assert_eq!(sc.doc().get(poly, "area").as_deref(), Some("200"), "area attribute written");
    assert_eq!(sc.doc().get(poly, "hint_type").as_deref(), Some("AIH_FORT_PERIMETER"), "hint type override");
    let p0 = sc.point_position(sc.polygon_points(poly)[0]).unwrap();
    assert!((p0.x + 10.0).abs() < 1e-6, "points are local to the centroid");
    sc.set_polygon_point(poly, 0, Double3::new(-20.0, 0.0, -5.0)).unwrap();
    assert!(sc.polygon_area(poly) > 200.0, "area follows point edits");
    sc.insert_polygon_point(poly, 1, Double3::new(0.0, 0.0, -8.0)).unwrap();
    assert_eq!(sc.polygon_points(poly).len(), 5, "insert adds a point");
    assert!(sc.remove_polygon_point(poly, 1).unwrap(), "remove a point");
    assert_eq!(sc.polygon_points(poly).len(), 4, "remove drops a point");
    let re = TedScene::parse(sc.save()).unwrap();
    assert_eq!(re.find_all("AI_HINT_POLYGON_INSTANCE").len(), sc.find_all("AI_HINT_POLYGON_INSTANCE").len(), "polygon survives save and parse");
    assert_ne!(sc.save(), orig, "polygon edits change the file");
}

/// A 16-bit grey map through the kit encoder.
fn height_png() -> (PngFile, Vec<u8>) {
    let (w, h) = (96u32, 80u32);
    let pixels = (0..h).flat_map(|y| (0..w).flat_map(move |x| (((x * 977 + y * 613) % 65536) as u16).to_be_bytes())).collect();
    let png = PngFile { width: w, height: h, bit_depth: 16, color_type: 0, pixels, pixels_edited: true, ..Default::default() };
    let bytes = png.write().unwrap();
    (PngFile::parse(&bytes).unwrap(), bytes)
}

#[test]
fn height_field() {
    let (png, bytes) = height_png();
    let mut hf = HeightField::from_png(&png, -8.370342, 17.933426).unwrap();

    // 1. Conversions.
    assert!((0..65536u32).all(|r| u32::from(hf.to_raw(hf.to_metres(r as u16))) == r), "metres <-> raw round trips");

    // 2. No-op save.
    assert_eq!(hf.to_png(&png).write().unwrap(), bytes, "a no-op save gives the same bytes");

    // 3. Strokes change samples only inside the brush.
    let before = hf.samples().to_vec();
    let b = Brush { cx: f64::from(hf.width()) * 0.4, cy: f64::from(hf.height()) * 0.6, radius: 20.0, strength: 0.8, falloff: 0.7 };
    hf.begin_edit().unwrap();
    hf.raise(&b, 2.5).unwrap();
    hf.end_edit();
    hf.begin_edit().unwrap();
    hf.smooth(&b).unwrap();
    hf.end_edit();
    hf.begin_edit().unwrap();
    hf.flatten(&b, hf.metres(b.cx as u32, b.cy as u32)).unwrap();
    hf.end_edit();
    let after = hf.samples().to_vec();
    let mut changed = 0;
    for y in 0..hf.height() {
        for x in 0..hf.width() {
            let i = (y * hf.width() + x) as usize;
            if before[i] != after[i] {
                changed += 1;
                assert!((f64::from(x) - b.cx).hypot(f64::from(y) - b.cy) <= b.radius + 1.5, "a sample changed outside the brush");
            }
        }
    }

    assert!(changed > 0, "the strokes changed something");

    // 4. Undo and redo.
    while hf.can_undo() {
        hf.undo();
    }

    assert_eq!(hf.samples(), before.as_slice(), "undo restores the samples");
    assert_eq!(hf.to_png(&png).write().unwrap(), bytes, "undo restores the PNG bytes");
    while hf.redo() {}
    assert_eq!(hf.samples(), after.as_slice(), "redo reproduces the edit");
}
