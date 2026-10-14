//! Undo of tile document edits gives the exact bytes back: the cases the differential dump found broken in the C++ code (and fixed in both).

use std::io::Write;

use wce_editor_core::document::Document;
use wce_foundation::math::Float3;
use wce_tile::tile_project::TileProject;
use wce_tile_editor::tile_document::{BrushKind, BrushSettings, TileDocument};

const TED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project version="16">
    <scene>
        <SCENE_GRAPH name="root" position="vector(0,0,0)" orientation="euler(0,0,0)">
            <children>
                <LayerInstance flags=" OIF_VISIBLE|OIF_ACTIVE" export_type="LayerExportType_Normal" lf_offset="0" position="vector(0,0,0)" name="Buildings" surface_mode="MSM_FREEFORM" orientation="euler(0,0,0)">
                    <children>
                        <BUILDING_INSTANCE flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(10.50,2,30)" name="a" surface_mode="MSM_LANDSCAPE" orientation="euler(0.0,0,0)" key="house"/>
                        <AI_HINT_POLYGON_INSTANCE flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(50,0,50)" name="p" surface_mode="MSM_LANDSCAPE" orientation="euler(0,0,0)" area="12.5">
                            <points>
                                <point position="0,0,0"/>
                                <point position="10.0,0,0"/>
                                <point position="0,0,10"/>
                            </points>
                        </AI_HINT_POLYGON_INSTANCE>
                    </children>
                </LayerInstance>
            </children>
        </SCENE_GRAPH>
    </scene>
</project>
"#;

/// A loose tile (no kit) with a height map.
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

fn text(d: &TileDocument) -> Vec<u8> {
    d.tile().tscene().save()
}

#[test]
fn flags_are_toggled_only_where_the_file_has_them() {
    let (_dir, mut d) = open();
    let original = text(&d);

    // The scene graph has no flags attribute: nothing to toggle, no text added.
    let graph = d.tile().tscene().scene_graph().unwrap();
    assert!(!d.set_flag(graph, "OIF_VISIBLE", false));
    assert_eq!(text(&d), original);

    let layer = d.tile().tscene().find_all("LayerInstance")[0];
    assert!(d.set_flag(layer, "OIF_VISIBLE", false));
    assert!(!d.tile().shown(layer));
    d.history_mut().undo();
    assert_eq!(text(&d), original);
}

#[test]
fn moves_undo_to_the_file_text() {
    let (_dir, mut d) = open();
    let original = text(&d);
    let b = d.tile().tscene().find_all("BUILDING_INSTANCE")[0];
    assert!(d.set_local(b, Some([1.0, 2.0, 3.0]), Some([0.5, 0.0, 0.0]), 0));
    d.history_mut().undo();
    assert_eq!(text(&d), original, "\"10.50\" and \"0.0\" come back as they were written, not as TEd would write them");

    let p = d.tile().tscene().find_all("AI_HINT_POLYGON_INSTANCE")[0];
    d.polygon_point(p, "move", 1, Float3::new(30.0, 0.0, 30.0), 0).unwrap();
    d.history_mut().undo();
    assert_eq!(text(&d), original, "the point and the polygon's area come back as they were written");
}

#[test]
fn a_stroke_interrupted_by_another_edit_is_two_entries_with_their_own_heights() {
    let (_dir, mut d) = open();
    let heights = || -> fn(&TileDocument) -> Vec<u16> { |d| d.project().height.as_ref().unwrap().samples().to_vec() };
    let h0 = heights()(&d);
    let s = BrushSettings::default();
    d.brush_step(BrushKind::Raise, 40.0, 40.0, s, 0.05);
    let h1 = heights()(&d);
    assert_ne!(h1, h0);

    // The stroke stays open (no mouse up), another edit comes in, then the brush goes on.
    let b = d.tile().tscene().find_all("BUILDING_INSTANCE")[0];
    assert!(d.set_local(b, Some([1.0, 2.0, 3.0]), None, 0));
    d.brush_step(BrushKind::Raise, 90.0, 90.0, s, 0.05);
    d.end_stroke();
    assert_eq!(d.history().size(), 3, "the second stroke is an entry of its own");

    d.history_mut().undo();
    assert_eq!(heights()(&d), h1, "undoing the second stroke keeps the first one's heights");
    d.history_mut().undo();
    d.history_mut().undo();
    assert_eq!(heights()(&d), h0);
}
