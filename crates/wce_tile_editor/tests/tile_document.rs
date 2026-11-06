//! Undo of tile document edits gives the exact bytes back: the cases the differential dump found broken in the C++ code (and fixed in both).

use std::io::Write;

use wce_editor_core::document::Document;
use wce_editor_core::inspector::{InspectTarget, Inspector};
use wce_scene::render_scene::SourceRef;
use wce_foundation::math::{Double3, Float3};
use wce_tile::tile_project::TileProject;
use wce_tile_editor::tile_document::{BrushKind, BrushSettings, Create, TileDocument, item_of};

const TED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project version="16">
    <scene>
        <SCENE_GRAPH name="root" position="vector(0,0,0)" orientation="euler(0,0,0)">
            <children>
                <LayerInstance flags=" OIF_VISIBLE|OIF_ACTIVE" export_type="LayerExportType_Normal" lf_offset="0" position="vector(0,0,0)" name="Buildings" surface_mode="MSM_FREEFORM" orientation="euler(0,0,0)">
                    <children>
                        <BUILDING_INSTANCE flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(10.50,2,30)" name="a" surface_mode="MSM_LANDSCAPE" orientation="euler(0.0,0,0)" key="house" opacity="1" burn="false" glow_amount="1" bounds_colour="argb(1.0,0.0,0.0,1.0)"/>
                        <AI_HINT_POLYGON_INSTANCE flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(50,0,50)" name="p" surface_mode="MSM_LANDSCAPE" orientation="euler(0,0,0)" area="12.5" generated="false" is_generated="false" hint_type="AIH_COVER_LINE" polygon_shape="PS_POLYGON">
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
    assert!(d.set_local(b, Some(Double3::new(1.0, 2.0, 3.0)), Some(Double3::new(0.5, 0.0, 0.0)), 0));
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
    d.brush_step(BrushKind::Raise, 40.0, 40.0, s.clone(), 0.05);
    let h1 = heights()(&d);
    assert_ne!(h1, h0);

    // The stroke stays open (no mouse up), another edit comes in, then the brush goes on.
    let b = d.tile().tscene().find_all("BUILDING_INSTANCE")[0];
    assert!(d.set_local(b, Some(Double3::new(1.0, 2.0, 3.0)), None, 0));
    d.brush_step(BrushKind::Raise, 90.0, 90.0, s, 0.05);
    d.end_stroke();
    assert_eq!(d.history().size(), 3, "the second stroke is an entry of its own");

    d.history_mut().undo();
    assert_eq!(heights()(&d), h1, "undoing the second stroke keeps the first one's heights");
    d.history_mut().undo();
    d.history_mut().undo();
    assert_eq!(heights()(&d), h0);
}


/// An inspector that records the rows and applies one scripted edit.
#[derive(Default)]
struct Rows {
    target: InspectTarget,
    fields: Vec<String>,
    edit: Option<(String, Edit)>,
}

#[derive(Clone)]
enum Edit {
    Float(f32),
    Bool(bool),
    Int(i64),
    Choice(i32),
    Text(String),
    Colour([f32; 4]),
}

impl Rows {
    fn on(&mut self, field: &str) -> Option<Edit> {
        self.fields.push(field.to_owned());
        self.edit.as_ref().filter(|(f, _)| f == field).map(|(_, e)| e.clone())
    }
}

impl Inspector for Rows {
    fn target(&self) -> InspectTarget {
        self.target
    }

    fn set_target(&mut self, target: InspectTarget) {
        self.target = target;
    }

    fn section(&mut self, _: &str) {}

    fn text(&mut self, field: &str, _: &str, _: &str) {
        self.fields.push(format!("text:{field}"));
    }

    fn edit_float(&mut self, field: &str, _: &str, v: &mut f32, _: f32, _: f32, _: f32) -> bool {
        if let Some(Edit::Float(x)) = self.on(field) {
            *v = x;
            return true;
        }

        false
    }

    fn edit_float3(&mut self, field: &str, _: &str, _: &mut [f32; 3], _: f32) -> bool {
        self.on(field);
        false
    }

    fn edit_float2(&mut self, field: &str, _: &str, _: &mut [f32; 2], _: f32) -> bool {
        self.on(field);
        false
    }

    fn edit_int(&mut self, field: &str, _: &str, v: &mut i64, _: f32) -> bool {
        if let Some(Edit::Int(x)) = self.on(field) {
            *v = x;
            return true;
        }

        false
    }

    fn edit_color3(&mut self, field: &str, _: &str, _: &mut [f32; 3]) -> bool {
        self.on(field);
        false
    }

    fn edit_color4(&mut self, field: &str, _: &str, v: &mut [f32; 4]) -> bool {
        if let Some(Edit::Colour(x)) = self.on(field) {
            *v = x;
            return true;
        }

        false
    }

    fn edit_bool(&mut self, field: &str, _: &str, v: &mut bool) -> bool {
        if let Some(Edit::Bool(x)) = self.on(field) {
            *v = x;
            return true;
        }

        false
    }

    fn edit_string(&mut self, field: &str, _: &str, v: &mut String) -> bool {
        if let Some(Edit::Text(x)) = self.on(field) {
            *v = x;
            return true;
        }

        false
    }

    fn edit_choice(&mut self, field: &str, _: &str, index: &mut i32, _: &[String]) -> bool {
        if let Some(Edit::Choice(x)) = self.on(field) {
            *index = x;
            return true;
        }

        false
    }

    fn gesture_ended(&mut self) -> bool {
        // Typing ends at once in the script.
        matches!(self.edit, Some((_, Edit::Text(_))))
    }
}

fn inspect(d: &mut TileDocument, elem: usize, edit: Option<(&str, Edit)>) -> Vec<String> {
    let mut rows = Rows { edit: edit.map(|(f, e)| (f.to_owned(), e)), ..Default::default() };
    let item = SourceRef { doc: d.id(), item: item_of(elem) };
    d.inspect(item, &mut rows);
    rows.fields
}

#[test]
fn the_grid_of_a_building_is_teds_and_every_row_edits_the_file() {
    let (_dir, mut d) = open();
    let original = text(&d);
    let b = d.tile().tscene().find_all("BUILDING_INSTANCE")[0];
    let rows = inspect(&mut d, b, None);
    for f in [
        "flags", "surface_mode", "position", "euler", "key", "opacity", "bounds_colour", "burn", "burn_amount", "destruction_level",
        "indestructible", "toggleable", "capture_location",
    ] {
        assert!(rows.iter().any(|r| r == f || r == &format!("text:{f}") || r.starts_with(&format!("{f}."))), "{f} is in the grid: {rows:?}");
    }

    // TEd shows these, wce does not: nothing uses them (docs/re/buildings.md); the file keeps them as they are.
    for f in ["dirt_level", "glow_amount", "building_tech_level"] {
        assert!(!rows.iter().any(|r| r == f || r == &format!("text:{f}") || r.starts_with(&format!("{f}."))), "{f} is hidden: {rows:?}");
    }

    // TEd shows these for a fort wall only.
    for f in ["weak_wall", "ai_breachable", "dockable"] {
        assert!(!rows.iter().any(|r| r == f), "{f} is not for this building");
    }

    // A ranged float: the text is TEd's, and the undo gives the file's bytes back.
    inspect(&mut d, b, Some(("opacity", Edit::Float(0.25))));
    assert_eq!(d.tile().tscene().doc().get(b, "opacity").as_deref(), Some("0.25"));
    d.history_mut().undo();
    assert_eq!(text(&d), original);

    // A bool, a colour (r, g, b, a with one decimal at least), an enum the element lacks (added), the text of a key.
    inspect(&mut d, b, Some(("burn", Edit::Bool(true))));
    assert_eq!(d.tile().tscene().doc().get(b, "burn").as_deref(), Some("true"));
    inspect(&mut d, b, Some(("bounds_colour", Edit::Colour([0.5, 1.0, 0.0, 1.0]))));
    assert_eq!(d.tile().tscene().doc().get(b, "bounds_colour").as_deref(), Some("argb(0.5,1.0,0.0,1.0)"));
    inspect(&mut d, b, Some(("key", Edit::Text("tower".to_owned()))));
    assert_eq!(d.tile().tscene().doc().get(b, "key").as_deref(), Some("tower"));

    // The level is clamped to the building's levels (one, without the models).
    inspect(&mut d, b, Some(("destruction_level", Edit::Int(5))));
    assert_ne!(d.tile().tscene().doc().get(b, "destruction_level").as_deref(), Some("5"));

    // Everything comes back in reverse.
    while d.history().position() > 0 {
        d.history_mut().undo();
    }

    assert_eq!(text(&d), original);
}

#[test]
fn a_buildings_scale_works_only_as_a_prop() {
    use wce_foundation::math::Float4x4;

    let (_dir, mut d) = open();
    let b = d.tile().tscene().find_all("BUILDING_INSTANCE")[0];
    let item = SourceRef { doc: d.id(), item: item_of(b) };
    let scale = |d: &TileDocument| d.tile().tscene().doc().get(b, "scale");
    let drawn = |d: &TileDocument| {
        let m = d.tile().world_matrix(b).m;
        (m[0][0] * m[0][0] + m[1][0] * m[1][0] + m[2][0] * m[2][0]).sqrt()
    };
    let one = drawn(&d);
    let row = |rows: &[String], f: &str| rows.iter().any(|r| r == f || r == &format!("text:{f}") || r.starts_with(&format!("{f}.")));

    // A building: no row, and the gizmo does nothing.
    assert!(!row(&inspect(&mut d, b, None), "scale"));
    d.scale_items(&[item], [2.0; 3], &Float4x4::IDENTITY, 1);
    assert_eq!(scale(&d), None);

    // A prop: the gizmo's factor is since the drag began, so its steps do not add up; the next drag goes on from there.
    inspect(&mut d, b, Some(("wce_export_as_prop", Edit::Bool(true))));
    assert!(row(&inspect(&mut d, b, None), "scale"));
    d.scale_items(&[item], [2.0; 3], &Float4x4::IDENTITY, 2);
    d.scale_items(&[item], [3.0, 2.0, 2.0], &Float4x4::IDENTITY, 2);
    assert_eq!(scale(&d).as_deref(), Some("vector(3,2,2)"));
    d.scale_items(&[item], [2.0, 1.0, 1.0], &Float4x4::IDENTITY, 3);
    assert_eq!(scale(&d).as_deref(), Some("vector(6,2,2)"));
    assert!((drawn(&d) / one - 6.0).abs() < 1e-4, "the viewport follows");

    // Off again: the value stays, greyed out, and the building is drawn as the game draws it.
    inspect(&mut d, b, Some(("wce_export_as_prop", Edit::Bool(false))));
    assert!(row(&inspect(&mut d, b, None), "scale"));
    assert_eq!(scale(&d).as_deref(), Some("vector(6,2,2)"));
    assert!((drawn(&d) - one).abs() < 1e-5, "{} {one}", drawn(&d));
    d.scale_items(&[item], [2.0; 3], &Float4x4::IDENTITY, 4);
    assert_eq!(scale(&d).as_deref(), Some("vector(6,2,2)"));

    // A capture location keeps it a building, marked or not.
    inspect(&mut d, b, Some(("wce_export_as_prop", Edit::Bool(true))));
    assert!((drawn(&d) / one - 6.0).abs() < 1e-4);
    inspect(&mut d, b, Some(("capture_location", Edit::Text("cl".to_owned()))));
    assert!((drawn(&d) - one).abs() < 1e-5);
}

#[test]
fn the_scale_gizmo_works_on_everything_with_a_scale() {
    use wce_foundation::math::Float4x4;

    let (_dir, mut d) = open();
    let layer = d.tile().tscene().find_all("LayerInstance")[0];
    let add = |d: &mut TileDocument, tag: &str, scale: &str| {
        let attrs = [("key", "k"), ("scale", scale), ("surface_mode", "MSM_FREEFORM"), ("flags", " OIF_VISIBLE|OIF_ACTIVE")].map(|(k, v)| (k.to_owned(), v.to_owned()));
        let mut scene = wce_tile::ted_scene::TedScene::parse_any(d.tile().tscene().save()).unwrap();
        scene.add_instance(Some(layer), tag, &attrs).unwrap();
        scene.save()
    };

    // A fresh document of the tile with the four types in it.
    let text = add(&mut d, "SIMPLE_MESH_INSTANCE", "vector(1,2,3)");
    std::fs::write(_dir.path().join("tile.ted"), text).unwrap();
    let mut d = TileDocument::new(TileProject::open(_dir.path()).unwrap(), None);
    for (tag, scale) in [("ProjectedDecalInstance", "vector(12,20,-3)"), ("VFXInstance", "0.15"), ("VEGETATION_INSTANCE", "1")] {
        let text = add(&mut d, tag, scale);
        std::fs::write(_dir.path().join("tile.ted"), text).unwrap();
        d = TileDocument::new(TileProject::open(_dir.path()).unwrap(), None);
    }

    let find = |d: &TileDocument, tag: &str| d.tile().tscene().find_all(tag)[0];
    let (mesh, decal, vfx, tree) = (find(&d, "SIMPLE_MESH_INSTANCE"), find(&d, "ProjectedDecalInstance"), find(&d, "VFXInstance"), find(&d, "VEGETATION_INSTANCE"));
    let items = [mesh, decal, vfx, tree].map(|e| SourceRef { doc: d.id(), item: item_of(e) });
    let scale = |d: &TileDocument, e: usize| d.tile().tscene().doc().get(e, "scale").unwrap();
    let before = d.history().size();

    // All at once, one undo entry; steps of a drag do not add up; the axis that moved most drives the single numbers.
    d.scale_items(&items, [3.0, 1.0, 1.0], &Float4x4::IDENTITY, 7);
    d.scale_items(&items, [1.5, 2.0, 1.0], &Float4x4::IDENTITY, 7);
    assert_eq!(scale(&d, mesh), "vector(1.5,4,3)");
    assert_eq!(scale(&d, decal), "vector(18,40,-3)", "a decal's z stays negative");
    assert_eq!(scale(&d, vfx), "0.3");
    assert_eq!(scale(&d, tree), "2");
    assert_eq!(d.history().size(), before + 1);

    // A tree stops where its byte would wrap.
    d.scale_items(&items, [5.0; 3], &Float4x4::IDENTITY, 8);
    assert_eq!(scale(&d, tree), "2");
    assert_eq!(scale(&d, vfx), "1.5");

    d.history_mut().undo();
    d.history_mut().undo();
    assert_eq!((scale(&d, mesh), scale(&d, vfx), scale(&d, tree)), ("vector(1,2,3)".to_owned(), "0.15".to_owned(), "1".to_owned()));
}

#[test]
fn a_fill_is_the_general_value_times_its_types() {
    use wce_tile_editor::tile_document::PolygonLook;

    let mut look = PolygonLook::default();
    assert_eq!(look.fill_of("NOGO_REGION_POLYGON_INSTANCE"), 0.3);
    look.fill = 0.8;
    look.set_type_factor("NOGO_REGION_POLYGON_INSTANCE", 0.5);
    look.set_type_factor("ZONE_POLYGON_INSTANCE", 0.0);
    assert_eq!(look.fill_of("NOGO_REGION_POLYGON_INSTANCE"), 0.4);
    assert_eq!(look.fill_of("ZONE_POLYGON_INSTANCE"), 0.0);
    assert_eq!(look.fill_of("AI_HINT_POLYGON_INSTANCE"), 0.8, "a type untouched follows the general value");

    // Back at 1 it is not kept, so a reset look compares equal.
    look.set_type_factor("NOGO_REGION_POLYGON_INSTANCE", 1.0);
    look.set_type_factor("ZONE_POLYGON_INSTANCE", 1.0);
    assert!(look.type_fill.is_empty());
}

#[test]
fn a_preview_turns_without_being_made_again_and_as_the_item_will_be() {
    use wce_scene::render_scene::RenderScene;
    use wce_tile_editor::tile_document::turned;

    let (_dir, mut d) = open_with_house();
    let mut scene = RenderScene::default();
    let layer = scene.add_layer(d.id(), "tile");
    d.publish(&mut scene, layer);
    let text = b"<BUILDING_INSTANCE key=\"house\" name=\"h\" position=\"vector(0,0,0)\" orientation=\"euler(0,0,0)\" surface_mode=\"MSM_FREEFORM\"/>".to_vec();
    let at = Float3::new(20.0, 3.0, 30.0);
    let shown = |s: &RenderScene| s.entities().iter().filter(|e| e.transient).map(|e| (e.id, e.world)).collect::<Vec<_>>();

    d.tile_mut().set_preview(&mut scene, Some(&text), at, 0.0);
    let (before, structure) = (shown(&scene), scene.structure_revision());
    assert!(!before.is_empty(), "the preview shows");

    d.tile_mut().set_preview(&mut scene, Some(&text), at, 37.0);
    let by_yaw = shown(&scene);
    assert_eq!(by_yaw.iter().map(|e| e.0).collect::<Vec<_>>(), before.iter().map(|e| e.0).collect::<Vec<_>>(), "the same entities");
    assert_eq!(scene.structure_revision(), structure, "nothing was made again");
    assert!(by_yaw[0].1 != before[0].1, "it turned");

    // What a click places, shown from its turned text.
    d.tile_mut().set_preview(&mut scene, Some(&turned(&text, 37.0)), at, 0.0);
    let placed = shown(&scene);
    assert_eq!(placed.len(), by_yaw.len());
    for (a, b) in by_yaw.iter().zip(&placed) {
        for r in 0..3 {
            for c in 0..4 {
                assert!((a.1.m[r][c] - b.1.m[r][c]).abs() < 1e-3, "[{r}][{c}]: {:?} for {:?}", a.1.m, b.1.m);
            }
        }
    }
}

/// What a scene shows of the items: (item, kind, its matrix's bits), sorted.
fn items_shown(scene: &wce_scene::render_scene::RenderScene) -> Vec<(u64, String, Vec<u32>)> {
    let mut out = scene.entities().iter().filter(|e| e.source.item != 0).map(|e| (e.source.item, format!("{:?}", e.kind()), e.world.m.iter().flatten().map(|v| v.to_bits()).collect())).collect::<Vec<_>>();
    out.sort();
    out
}

fn two_layers() -> String {
    TED.replace(
        "</LayerInstance>",
        r#"</LayerInstance>
                <LayerInstance flags=" OIF_VISIBLE|OIF_ACTIVE" export_type="LayerExportType_Normal" lf_offset="0" position="vector(0,0,0)" name="Later" orientation="euler(0,0,0)">
                    <children>
                        <BUILDING_INSTANCE flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(80,0,30)" name="b" surface_mode="MSM_FREEFORM" orientation="euler(0,0,0)" key="house"/>
                        <AI_HINT_POLYGON_INSTANCE flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(90,0,50)" name="q" surface_mode="MSM_FREEFORM" orientation="euler(0,0,0)">
                            <points>
                                <point position="0,0,0"/>
                                <point position="10.0,0,0"/>
                                <point position="0,0,10"/>
                            </points>
                        </AI_HINT_POLYGON_INSTANCE>
                        <AI_HINT_LOCATOR_INSTANCE flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(95,0,55)" name="m" surface_mode="MSM_FREEFORM" orientation="euler(0,0,0)"/>
                    </children>
                </LayerInstance>"#,
    )
}

#[test]
fn edits_of_a_part_of_the_tile_leave_the_scene_as_a_new_publish_would() {
    use wce_scene::render_scene::RenderScene;

    let (dir, _) = open();
    let tile = dir.path().join("tile.ted");
    let open_text = |text: &[u8]| {
        std::fs::write(&tile, text).unwrap();
        TileDocument::new(TileProject::open(dir.path()).unwrap(), Some(std::sync::Arc::new(HouseFiles)))
    };

    let mut d = open_text(two_layers().as_bytes());
    let mut scene = RenderScene::default();
    let layer = scene.add_layer(d.id(), "tile");
    let eye = Float3::new(0.0, 100.0, 0.0);
    d.publish(&mut scene, layer);
    d.tile_mut().update_view(&mut scene, eye);
    let fresh = |text: &[u8]| {
        let mut d = open_text(text);
        let mut scene = RenderScene::default();
        let layer = scene.add_layer(d.id(), "tile");
        d.publish(&mut scene, layer);
        d.tile_mut().update_view(&mut scene, eye);
        items_shown(&scene)
    };

    let same = |d: &mut TileDocument, scene: &mut RenderScene, what: &str| {
        let dirty = d.history_mut().take_dirty();
        assert!(!dirty.all, "not everything: {what}");
        d.republish(scene, layer, &dirty);
        d.tile_mut().update_view(scene, eye);
        let (mine, new) = (items_shown(scene), fresh(&text(d)));
        assert_eq!(mine.len(), new.len(), "{what}");
        for (a, b) in mine.iter().zip(&new) {
            assert_eq!((a.0, &a.1), (b.0, &b.1), "{what}");
            assert!(a.2.iter().zip(&b.2).all(|(x, y)| (f32::from_bits(*x) - f32::from_bits(*y)).abs() < 1e-3), "{what}: {a:?} {b:?}");
        }
    };

    let sc = d.tile().tscene();
    let (first, later) = (sc.find_all("LayerInstance")[0], sc.find_all("LayerInstance")[1]);
    let a = sc.find_all("BUILDING_INSTANCE")[0];
    let polygon = sc.find_all("AI_HINT_POLYGON_INSTANCE")[1];

    assert!(d.rename(a, "another"));
    same(&mut d, &mut scene, "a rename");
    assert_eq!(d.tile().tscene().name(a), "another");

    assert!(d.set_attribute(a, "opacity", "0.5"));
    same(&mut d, &mut scene, "a property the file did not have");
    d.history_mut().undo();
    same(&mut d, &mut scene, "its undo");

    assert!(d.set_attribute(a, "key", "elsewhere"));
    same(&mut d, &mut scene, "another key");
    d.history_mut().undo();
    same(&mut d, &mut scene, "its undo");

    let at = Double3::new(7.0, 0.0, 9.0);
    assert!(d.set_local(later, Some(at), None, 1));
    same(&mut d, &mut scene, "a layer moved");
    d.history_mut().undo();
    same(&mut d, &mut scene, "its undo");

    assert!(d.set_flag(later, "OIF_VISIBLE", false));
    same(&mut d, &mut scene, "a layer hidden");
    d.history_mut().undo();
    same(&mut d, &mut scene, "shown again");
    assert!(d.set_flag(first, "OIF_ACTIVE", false));
    same(&mut d, &mut scene, "a layer locked");
    assert!(scene.entities().iter().any(|e| e.source.item == item_of(a) && e.locked), "its buildings cannot be picked");
    d.history_mut().undo();
    same(&mut d, &mut scene, "unlocked");
    assert!(!scene.entities().iter().any(|e| e.source.item == item_of(a) && e.locked));

    let new = d.add_layer(d.tile().tscene().scene_graph(), "New").unwrap();
    same(&mut d, &mut scene, "a new layer");
    d.history_mut().undo();
    same(&mut d, &mut scene, "its undo");
    assert!(new > first);

    d.polygon_point(polygon, "insert", 0, Float3::new(95.0, 0.0, 55.0), 0).unwrap();
    same(&mut d, &mut scene, "a point added");
    d.polygon_point(polygon, "remove", 3, Float3::default(), 0).unwrap();
    same(&mut d, &mut scene, "a point removed");
    d.history_mut().undo();
    same(&mut d, &mut scene, "its undo");
    d.history_mut().undo();
    same(&mut d, &mut scene, "the first one's undo");

    let copy = d.add_polygon(Some(first), "AI_HINT_POLYGON_INSTANCE", "", &[Float3::new(1.0, 0.0, 1.0), Float3::new(5.0, 0.0, 1.0), Float3::new(1.0, 0.0, 5.0)]).unwrap();
    same(&mut d, &mut scene, "a new polygon");
    d.history_mut().undo();
    same(&mut d, &mut scene, "its undo");
    assert!(copy > a);
}

#[test]
fn an_item_added_or_removed_leaves_the_scene_as_a_new_publish_would_without_making_the_rest_again() {
    use wce_scene::render_scene::RenderScene;
    use wce_tile_editor::tile_document::FlipPlane;

    let two_layers = two_layers();
    let (dir, _) = open();
    let tile = dir.path().join("tile.ted");
    let open_text = |text: &[u8]| {
        std::fs::write(&tile, text).unwrap();
        TileDocument::new(TileProject::open(dir.path()).unwrap(), Some(std::sync::Arc::new(HouseFiles)))
    };

    let mut d = open_text(two_layers.as_bytes());
    let mut scene = RenderScene::default();
    let layer = scene.add_layer(d.id(), "tile");
    let eye = Float3::new(0.0, 100.0, 0.0);
    d.publish(&mut scene, layer);
    d.tile_mut().update_view(&mut scene, eye);
    let fresh = |text: &[u8]| {
        let mut d = open_text(text);
        let mut scene = RenderScene::default();
        let layer = scene.add_layer(d.id(), "tile");
        d.publish(&mut scene, layer);
        d.tile_mut().update_view(&mut scene, eye);
        items_shown(&scene)
    };

    let follow = |d: &mut TileDocument, scene: &mut RenderScene| {
        let dirty = d.history_mut().take_dirty();
        assert!(!dirty.all, "not everything: {}", d.history().name(d.history().position().saturating_sub(1)));
        d.republish(scene, layer, &dirty);
        d.tile_mut().update_view(scene, eye);
    };

    let sc = d.tile().tscene();
    let (first_layer, a) = (sc.find_all("LayerInstance")[0], sc.find_all("BUILDING_INSTANCE")[0]);
    let later = sc.find_all("BUILDING_INSTANCE")[1];
    let terrain_and_later = scene.entities().iter().filter(|e| e.source.item == item_of(later)).map(|e| e.id).collect::<Vec<_>>();
    assert!(!terrain_and_later.is_empty(), "the building has its pieces");
    let original = items_shown(&scene);

    // Pasted into the first layer: all that is in the second one takes other numbers.
    let items = d.copy_items(&[a]);
    let new = d.paste(&items, first_layer, Float3::new(5.0, 0.0, 5.0)).unwrap();
    follow(&mut d, &mut scene);
    assert_eq!(items_shown(&scene), fresh(&text(&d)), "after an insert");
    let moved = scene.entities().iter().filter(|e| e.source.item == item_of(later + 1)).map(|e| e.id).collect::<Vec<_>>();
    assert_eq!(moved, terrain_and_later, "the entities stay, with the new number");

    // A flip touches what it moves alone.
    d.flip(&new, FlipPlane { x: 40.0, z: 0.0, angle: 30.0 }, None).unwrap();
    follow(&mut d, &mut scene);
    assert_eq!(items_shown(&scene), fresh(&text(&d)), "after a flip");
    d.history_mut().undo();
    follow(&mut d, &mut scene);
    assert_eq!(items_shown(&scene), fresh(&text(&d)), "after its undo");

    // A copy across, its undo, the redo.
    let polygon = d.tile().tscene().find_all("AI_HINT_POLYGON_INSTANCE")[0];
    d.flip(&[polygon], FlipPlane { x: 40.0, z: 0.0, angle: 0.0 }, Some(first_layer)).unwrap();
    follow(&mut d, &mut scene);
    assert_eq!(items_shown(&scene), fresh(&text(&d)), "after a copy across");
    d.history_mut().undo();
    follow(&mut d, &mut scene);
    assert_eq!(items_shown(&scene), fresh(&text(&d)), "after its undo");
    d.history_mut().redo();
    follow(&mut d, &mut scene);
    assert_eq!(items_shown(&scene), fresh(&text(&d)), "after its redo");
    d.history_mut().undo();
    follow(&mut d, &mut scene);

    // Removed: what it held goes, the numbers come back; undone: it comes back.
    assert!(d.remove(new[0]));
    follow(&mut d, &mut scene);
    assert_eq!(items_shown(&scene), original, "after a removal");
    d.history_mut().undo();
    follow(&mut d, &mut scene);
    assert_eq!(items_shown(&scene), fresh(&text(&d)), "after its undo");
    d.history_mut().undo();
    follow(&mut d, &mut scene);
    assert_eq!(items_shown(&scene), original, "the paste undone");
}

#[test]
fn a_flip_leaves_a_splines_unused_handles_turns_a_walls_side_and_mirrors_a_shelters_target() {
    use wce_tile::splines;
    use wce_tile::world_transform::world_transform;
    use wce_tile_editor::tile_document::FlipPlane;

    let text = TED.replace(
        r#"<AI_HINT_POLYGON_INSTANCE"#,
        r#"<SplineRiverInstance flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(20,0,10)" name="r" surface_mode="MSM_FREEFORM" orientation="euler(0,0.4,0)">
                            <points>
                                <point position="0,0,0" controlIn="0,0,0" controlOut="4,0,0"/>
                                <point position="10,0,0" controlIn="6,0,1" controlOut="14,0,-1"/>
                                <point position="20,0,5" controlIn="16,0,5" controlOut="0,0,0"/>
                            </points>
                        </SplineRiverInstance>
                        <WceWallSpline flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(0,0,0)" name="w" surface_mode="MSM_FREEFORM" orientation="euler(0,0,0)" polygon_closed="0" wall_base="barbarian_fort_straight">
                            <points>
                                <point position="60,0,60"/>
                                <point position="80,0,60"/>
                            </points>
                        </WceWallSpline>
                        <CivilianShelter flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(30,0,30)" name="s" surface_mode="MSM_FREEFORM" orientation="euler(0,0,0)" target="vector(35,0,32)"/>
                        <AI_HINT_POLYGON_INSTANCE"#,
    );
    let (dir, _) = open();
    std::fs::write(dir.path().join("tile.ted"), text).unwrap();
    let mut d = TileDocument::new(TileProject::open(dir.path()).unwrap(), None);
    let original = self::text(&d);
    let unit = d.tile().unit();
    let sc = d.tile().tscene();
    let (river, wall, shelter) = (sc.find_all("SplineRiverInstance")[0], sc.find_all("WceWallSpline")[0], sc.find_all("CivilianShelter")[0]);
    let world = |d: &TileDocument, p: Double3| {
        let w = world_transform(d.tile().tscene(), river);
        w.position + w.rotation * p
    };

    let before = splines::read(d.tile().tscene(), river);
    let (mid, handle) = (world(&d, before[1].position), world(&d, before[1].control_out));
    let plane = FlipPlane { x: 50.0, z: 0.0, angle: 0.0 };
    let mirrored = |p: Double3| Double3::new(2.0 * 50.0 * unit - p.x, p.y, p.z);
    let near = |a: Double3, b: Double3| (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3 && (a.z - b.z).abs() < 1e-3;

    d.flip(&[river, wall, shelter], plane, None).unwrap();

    let after = splines::read(d.tile().tscene(), river);
    assert!(near(world(&d, after[1].position), mirrored(mid)), "{:?}", world(&d, after[1].position));
    assert!(near(world(&d, after[1].control_out), mirrored(handle)), "the handles go with the points");
    assert_eq!(after[0].control_in, Double3::ZERO, "an unused handle stays unused");
    assert_eq!(after[2].control_out, Double3::ZERO, "an unused handle stays unused");

    assert_eq!(d.tile().tscene().doc().get(wall, "wall_flip").as_deref(), Some("true"), "its outer face stays the outer one");
    let target = d.tile().tscene().doc().get(shelter, "target").unwrap();
    let t = wce_tile::ted_scene::parse_vec3(&target, "vector").unwrap();
    assert!(near(t, Double3::new(100.0 * unit - 35.0, 0.0, 32.0)), "{target}");

    d.history_mut().undo();
    assert_eq!(self::text(&d), original, "undo gives the file back");
}

#[test]
fn a_flip_keeps_what_follows_the_landscape_as_high_above_the_ground() {
    use wce_tile::splines;
    use wce_tile::world_transform::world_transform;
    use wce_tile_editor::tile_document::FlipPlane;

    // The height map of `open` rises along x, so the other side of the plane is higher.
    let text = TED.replace(
        r#"<AI_HINT_POLYGON_INSTANCE"#,
        r#"<SplineRiverInstance flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(20,0,10)" name="land" surface_mode="MSM_LANDSCAPE" orientation="euler(0,0,0)">
                            <points>
                                <point position="0,1,0" controlIn="0,0,0" controlOut="4,1,0"/>
                                <point position="10,2,0" controlIn="6,2,1" controlOut="0,0,0"/>
                            </points>
                        </SplineRiverInstance>
                        <SplineRiverInstance flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(20,0,10)" name="free" surface_mode="MSM_FREEFORM" orientation="euler(0,0,0)">
                            <points>
                                <point position="0,1,0" controlIn="0,0,0" controlOut="4,1,0"/>
                                <point position="10,2,0" controlIn="6,2,1" controlOut="0,0,0"/>
                            </points>
                        </SplineRiverInstance>
                        <AI_HINT_POLYGON_INSTANCE"#,
    );
    let (dir, _) = open();
    std::fs::write(dir.path().join("tile.ted"), text).unwrap();
    let mut d = TileDocument::new(TileProject::open(dir.path()).unwrap(), None);
    let sc = d.tile().tscene();
    let (land, free) = (sc.find_all("SplineRiverInstance")[0], sc.find_all("SplineRiverInstance")[1]);
    let unit = d.tile().unit();
    let world = |d: &TileDocument, e: usize, p: Double3| {
        let w = world_transform(d.tile().tscene(), e);
        w.position + w.rotation * p
    };

    let ground = |d: &TileDocument, p: Double3| f64::from(d.tile().terrain_height((p.x / unit) as f32, (p.z / unit) as f32)) * unit;
    let points = |d: &TileDocument, e: usize| splines::read(d.tile().tscene(), e);
    let (before, before_free) = (points(&d, land), points(&d, free));
    let above = before.iter().map(|p| world(&d, land, p.position)).map(|p| p.y - ground(&d, p)).collect::<Vec<_>>();
    let first_y = world(&d, land, before[0].position).y;
    let handle_above = world(&d, land, before[0].control_out).y - world(&d, land, before[0].position).y;
    let free_y = before_free.iter().map(|p| world(&d, free, p.position).y).collect::<Vec<_>>();

    d.flip(&[land, free], FlipPlane { x: 40.0, z: 0.0, angle: 0.0 }, None).unwrap();

    let after = points(&d, land);
    for (p, was) in after.iter().zip(&above) {
        let q = world(&d, land, p.position);
        assert!((q.y - ground(&d, q) - was).abs() < 1e-2, "{} above the ground, was {was}", q.y - ground(&d, q));
    }

    let moved = world(&d, land, after[0].position).y - first_y;
    assert!(moved.abs() > 0.1, "the ground is another height there: {moved}");
    assert!((world(&d, land, after[0].control_out).y - world(&d, land, after[0].position).y - handle_above).abs() < 1e-2, "a handle goes with its point");
    assert_eq!(after[1].control_out, Double3::ZERO);

    for (p, y) in points(&d, free).iter().zip(&free_y) {
        assert!((world(&d, free, p.position).y - y).abs() < 1e-6, "a freeform one keeps its height");
    }
}

#[test]
fn a_flip_of_a_prefab_reference_stays_a_reference_with_its_items_as_overrides_and_its_copy_is_refused_inside_one() {
    use wce_tile::project_files::PREFAB_ATTR;
    use wce_tile::world_transform::world_transform;
    use wce_tile_editor::tile_document::FlipPlane;

    let text = TED.replace(
        r#"<AI_HINT_POLYGON_INSTANCE"#,
        r#"<UserInstanceGroup flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(100,0,100)" name="g" surface_mode="MSM_FREEFORM" orientation="euler(0,0.5,0)" wce_prefab="set">
                            <children>
                                <BUILDING_INSTANCE flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(20,0,0)" name="in1" surface_mode="MSM_FREEFORM" orientation="euler(0,0.3,0)" key="house"/>
                            </children>
                        </UserInstanceGroup>
                        <AI_HINT_POLYGON_INSTANCE"#,
    );
    let (dir, _) = open();
    std::fs::write(dir.path().join("tile.ted"), text).unwrap();
    let mut d = TileDocument::new(TileProject::open(dir.path()).unwrap(), None);
    let original = self::text(&d);
    let unit = d.tile().unit();
    let sc = d.tile().tscene();
    let (group, item) = (sc.find_all("UserInstanceGroup")[0], sc.find_all("BUILDING_INSTANCE")[1]);
    let p = world_transform(sc, item).position;
    let plane = FlipPlane { x: 50.0, z: 0.0, angle: 0.0 };

    d.flip(&[group], plane, None).unwrap();
    let q = world_transform(d.tile().tscene(), item).position;
    assert!((q.x - (100.0 * unit - p.x)).abs() < 1e-3 && (q.z - p.z).abs() < 1e-3, "{q:?}");
    assert_eq!(d.tile().tscene().doc().get(group, PREFAB_ATTR).as_deref(), Some("set"), "still a reference");
    d.history_mut().undo();
    assert_eq!(self::text(&d), original);

    // A copy cannot go into a reference: its items are values only.
    assert!(d.flip(&[item], plane, Some(group)).is_err());
    assert_eq!(self::text(&d), original);
}

#[test]
fn a_flip_mirrors_places_and_headings_undoes_at_once_and_can_copy() {
    use wce_tile::world_transform::world_transform;
    use wce_tile_editor::tile_document::FlipPlane;

    let text = TED.replace(
        r#"<AI_HINT_POLYGON_INSTANCE"#,
        r#"<UserInstanceGroup flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(100,0,100)" name="g" surface_mode="MSM_FREEFORM" orientation="euler(0,0.5,0)">
                            <children>
                                <BUILDING_INSTANCE flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(20,0,0)" name="in1" surface_mode="MSM_FREEFORM" orientation="euler(0,0.3,0)" key="house"/>
                                <BUILDING_INSTANCE flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(0,0,40)" name="in2" surface_mode="MSM_FREEFORM" orientation="euler(0,0,0)" key="house"/>
                            </children>
                        </UserInstanceGroup>
                        <AI_HINT_POLYGON_INSTANCE"#,
    );
    let (dir, _) = open();
    std::fs::write(dir.path().join("tile.ted"), text).unwrap();
    let mut d = TileDocument::new(TileProject::open(dir.path()).unwrap(), None);
    let original = self::text(&d);
    let unit = d.tile().unit();
    let sc = d.tile().tscene();
    let (group, in1, in2) = (sc.find_all("UserInstanceGroup")[0], sc.find_all("BUILDING_INSTANCE")[1], sc.find_all("BUILDING_INSTANCE")[2]);
    let polygon = sc.find_all("AI_HINT_POLYGON_INSTANCE")[0];
    let pos = |d: &TileDocument, e: usize| world_transform(d.tile().tscene(), e).position;
    let (p1, p2) = (pos(&d, in1), pos(&d, in2));
    let forward = |d: &TileDocument, e: usize| world_transform(d.tile().tscene(), e).rotation.col(2);
    let f1 = forward(&d, in1);
    let world_point = |d: &TileDocument, i: usize| {
        let sc = d.tile().tscene();
        let w = world_transform(sc, polygon);
        w.position + w.rotation * sc.point_position(sc.polygon_points(polygon)[i]).unwrap()
    };

    let q1 = world_point(&d, 1);

    // Across the plane at x = 50 m: every item at its own mirror, so the group's layout mirrors and is not turned as one.
    let plane = FlipPlane { x: 50.0, z: 0.0, angle: 0.0 };
    let before = d.history().size();
    d.flip(&[group, polygon], plane, None).unwrap();
    assert_eq!(d.history().size(), before + 1, "one undo entry");
    let mirrored = |p: Double3| Double3::new(2.0 * 50.0 * unit - p.x, p.y, p.z);
    let near = |a: Double3, b: Double3| (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3 && (a.z - b.z).abs() < 1e-3;
    assert!(near(pos(&d, in1), mirrored(p1)) && near(pos(&d, in2), mirrored(p2)), "{:?}", pos(&d, in1));
    assert!(near(forward(&d, in1), Double3::new(-f1.x, f1.y, f1.z)), "the heading is reflected: {:?} from {f1:?}", forward(&d, in1));
    let r = world_transform(d.tile().tscene(), in1).rotation;
    let det = r.col(0).x * (r.col(1).y * r.col(2).z - r.col(1).z * r.col(2).y) - r.col(0).y * (r.col(1).x * r.col(2).z - r.col(1).z * r.col(2).x) + r.col(0).z * (r.col(1).x * r.col(2).y - r.col(1).y * r.col(2).x);
    assert!((det - 1.0).abs() < 1e-6, "still a rotation: {det}");

    // On the landscape: its height is the ground's over there (a_flip_keeps_what_follows_the_landscape_as_high_above_the_ground).
    let p = world_point(&d, 1);
    assert!(near(Double3::new(p.x, q1.y, p.z), mirrored(q1)), "a polygon's points mirror: {p:?}");

    d.history_mut().undo();
    assert_eq!(self::text(&d), original, "undo gives the file back");

    // A copy leaves the items where they are and adds the mirrored ones.
    let layer = d.tile().tscene().find_all("LayerInstance")[0];
    let n = d.tile().tscene().find_all("BUILDING_INSTANCE").len();
    let went = d.flip(&[in2], plane, Some(layer)).unwrap();
    assert_eq!(went.len(), 1);
    assert_eq!(d.tile().tscene().find_all("BUILDING_INSTANCE").len(), n + 1);
    assert!(near(pos(&d, went[0]), mirrored(p2)), "{:?}", pos(&d, went[0]));
    d.history_mut().undo();
    assert_eq!(self::text(&d), original);
}

#[test]
fn the_steps_of_a_drag_are_one_entry() {
    let (_dir, mut d) = open();
    let original = text(&d);
    let b = d.tile().tscene().find_all("BUILDING_INSTANCE")[0];
    for v in [0.9, 0.7, 0.4] {
        inspect(&mut d, b, Some(("opacity", Edit::Float(v))));
    }

    assert_eq!(d.history().size(), 1);
    assert_eq!(d.tile().tscene().doc().get(b, "opacity").as_deref(), Some("0.4"));
    d.history_mut().undo();
    assert_eq!(text(&d), original);
    d.history_mut().redo();
    assert_eq!(d.tile().tscene().doc().get(b, "opacity").as_deref(), Some("0.4"));
}

#[test]
fn a_polygon_keeps_its_twins_and_its_rules() {
    let (_dir, mut d) = open();
    let original = text(&d);
    let p = d.tile().tscene().find_all("AI_HINT_POLYGON_INSTANCE")[0];
    let rows = inspect(&mut d, p, None);

    // The area is shown, not edited; the radius of a polygon (not a circle) is not in the grid.
    assert!(rows.iter().any(|r| r == "text:area"));
    assert!(!rows.iter().any(|r| r == "radius" || r == "text:radius"));
    inspect(&mut d, p, Some(("generated", Edit::Bool(true))));
    assert_eq!(d.tile().tscene().doc().get(p, "generated").as_deref(), Some("true"));
    assert_eq!(d.tile().tscene().doc().get(p, "is_generated").as_deref(), Some("true"), "the file keeps both");
    inspect(&mut d, p, Some(("hint_type", Edit::Choice(1))));
    assert_eq!(d.tile().tscene().doc().get(p, "hint_type").as_deref(), Some("AIH_DEFENSIVE_HILL"));
    d.history_mut().undo();
    d.history_mut().undo();
    assert_eq!(text(&d), original);
}

#[test]
fn layers_show_only_what_teds_grid_shows() {
    let (_dir, mut d) = open();
    let layer = d.tile().tscene().find_all("LayerInstance")[0];
    let rows = inspect(&mut d, layer, None);
    assert!(rows.iter().any(|r| r.starts_with("flags")) && rows.iter().any(|r| r == "export_type") && rows.iter().any(|r| r == "position"));
    assert!(!rows.iter().any(|r| r == "surface_mode" || r == "euler"), "a layer has neither: {rows:?}");
    inspect(&mut d, layer, Some(("export_type", Edit::Choice(3))));
    assert_eq!(d.tile().tscene().doc().get(layer, "export_type").as_deref(), Some("LayerExportType_Escalation1"));
}

#[test]
fn a_layer_is_moved_by_its_position_and_everything_below_it_follows() {
    let (_dir, mut d) = open();
    let original = text(&d);
    let layer = d.tile().tscene().find_all("LayerInstance")[0];
    let b = d.tile().tscene().find_all("BUILDING_INSTANCE")[0];
    let before = d.tile().world_position(b);
    assert!(d.set_local(layer, Some(Double3::new(20.0, 0.0, 0.0)), None, 0));
    assert!((d.tile().world_position(b).x - before.x - 10.0).abs() < 1e-3, "the building follows its layer (20 scene units are 10 m)");
    d.history_mut().undo();
    assert_eq!(text(&d), original);
    assert!((d.tile().world_position(b).x - before.x).abs() < 1e-4);
}


/// Two pieces (and a second destruction level of the first) for the building key "house".
struct HouseFiles;

impl wce_vfs::files::Files for HouseFiles {
    fn read(&self, _: &str) -> Result<Vec<u8>, String> {
        Err("no such file".to_owned())
    }

    fn exists(&self, _: &str) -> bool {
        false
    }

    fn list(&self, prefix: &str, extension: &str) -> Vec<String> {
        if prefix != "rigidmodels/buildings/" || extension != ".rigid_model_v2" {
            return vec![];
        }

        ["house_piece01_destruct01", "house_piece01_destruct02", "house_piece02_destruct01"].iter().map(|f| format!("rigidmodels/buildings/house/{f}.rigid_model_v2")).collect()
    }
}

fn open_with_house() -> (tempfile::TempDir, TileDocument) {
    let (dir, d) = open();
    let project = TileProject::open(dir.path()).unwrap();
    drop(d);
    let d = TileDocument::new(project, Some(std::sync::Arc::new(HouseFiles)));
    (dir, d)
}

fn find_nodes(n: &wce_editor_core::document::OutlinerNode, label: &str, out: &mut Vec<wce_scene::render_scene::SourceRef>) {
    if n.label == label {
        out.push(n.source);
    }

    for c in &n.children {
        find_nodes(c, label, out);
    }
}

#[test]
fn the_pieces_of_a_building_are_its_children_with_their_own_properties() {
    let (_dir, mut d) = open_with_house();
    let b = d.tile().tscene().find_all("BUILDING_INSTANCE")[0];

    // One node per piece (the destruction states of a piece are not nodes), named like TEd names them.
    let mut pieces = vec![];
    find_nodes(&d.outline(), "building piece", &mut pieces);
    assert_eq!(pieces.len(), 2, "two pieces");
    let has_children = |item: u64| {
        fn walk(n: &wce_editor_core::document::OutlinerNode, item: u64) -> Option<usize> {
            if n.source.item == item && n.label.starts_with("a  [") {
                return Some(n.children.len());
            }

            n.children.iter().find_map(|c| walk(c, item))
        }

        walk(&d.outline(), item)
    };
    assert_eq!(has_children(item_of(b)), Some(2));

    // The grid of a piece is TEd's: flags, surface_mode, position, orientation, scale, opacity, burn ..., seasons, destruction_level.
    let rows = {
        let mut r = Rows::default();
        d.inspect(pieces[0], &mut r);
        r.fields
    };
    for f in ["flags.visible", "surface_mode", "position", "orientation", "scale", "opacity", "burn", "burn_amount", "bounds_colour", "show_collision", "seasons.Winter", "destruction_level"] {
        assert!(rows.iter().any(|r| r == f), "{f} is in the grid of a piece: {rows:?}");
    }

    // An edit of a piece lives in the session: no undo entry, the tile is not modified, the file is not touched.
    let original = text(&d);
    let mut r = Rows { edit: Some(("opacity".to_owned(), Edit::Float(0.5))), ..Default::default() };
    d.inspect(pieces[1], &mut r);
    assert_eq!(d.history().size(), 0);
    assert_eq!(text(&d), original);
    assert_eq!(d.tile().piece_props(b, 1).map(|p| p.opacity), Some(0.5));
    assert!(d.tile().piece_props(b, 0).is_none(), "the other piece keeps its defaults");

    // The building's destruction level goes up to its number of levels - 1 (the highest destructNN of its piece files).
    inspect(&mut d, b, Some(("destruction_level", Edit::Int(5))));
    assert_eq!(d.tile().tscene().doc().get(b, "destruction_level").as_deref(), Some("1"));

    // The properties of a building are handed on to its pieces (TEd loops over them), also by undo and redo.
    inspect(&mut d, b, Some(("opacity", Edit::Float(0.25))));
    assert_eq!(d.tile().piece_props(b, 0).map(|p| p.opacity), Some(0.25));
    assert_eq!(d.tile().piece_props(b, 1).map(|p| p.opacity), Some(0.25));
    d.history_mut().undo();
    assert_eq!(d.tile().piece_props(b, 0).map(|p| p.opacity), Some(1.0));
    d.history_mut().redo();
    assert_eq!(d.tile().piece_props(b, 1).map(|p| p.opacity), Some(0.25));
}

#[test]
fn a_group_keeps_its_items_in_the_world_and_ungroups_into_a_layer() {
    let (_dir, mut d) = open();
    let original = text(&d);
    let graph = d.tile().tscene().scene_graph().unwrap();
    let layer = d.tile().tscene().find_all("LayerInstance")[0];
    let (b, p) = (d.tile().tscene().find_all("BUILDING_INSTANCE")[0], d.tile().tscene().find_all("AI_HINT_POLYGON_INSTANCE")[0]);
    let before = [d.tile().world_position(b), d.tile().world_position(p)];

    // Grouped into the scene graph (no working layer): a UserInstanceGroup at their centre, holding both, which stay where they were.
    let g = d.group_instances(&[b, p], graph, "").unwrap();
    let sc = d.tile().tscene();
    assert_eq!(sc.type_of(g), "UserInstanceGroup");
    assert_eq!(d.tile().tscene().children(graph).last().copied(), Some(g));
    let kids = sc.children(g);
    assert_eq!(kids.iter().map(|&k| sc.type_of(k)).collect::<Vec<_>>(), ["BUILDING_INSTANCE", "AI_HINT_POLYGON_INSTANCE"]);
    let near = |a: Float3, b: Float3| (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3 && (a.z - b.z).abs() < 1e-3;
    assert!(near(d.tile().world_position(kids[0]), before[0]) && near(d.tile().world_position(kids[1]), before[1]));
    let gp = d.tile().world_position(g);
    assert!(near(gp, (before[0] + before[1]) * 0.5), "the group sits at the centre of its items: {gp:?}");

    // Ungrouped into the layer: the items are its last children, in the same places; the group is gone.
    let layer_now = d.tile().tscene().find_all("LayerInstance")[0];
    assert_eq!(layer_now, layer);
    let freed = d.ungroup(&[g], layer).unwrap();
    let sc = d.tile().tscene();
    assert!(sc.find_all("UserInstanceGroup").is_empty());
    assert_eq!(freed.len(), 2);
    assert!(freed.iter().all(|&f| sc.doc().elements()[f].parent.and_then(|w| sc.doc().elements()[w].parent) == Some(layer)));
    assert!(near(d.tile().world_position(freed[0]), before[0]) && near(d.tile().world_position(freed[1]), before[1]));

    // Each is one undo entry.
    d.history_mut().undo();
    d.history_mut().undo();
    assert_eq!(text(&d), original);
}

#[test]
fn several_groups_ungroup_at_once() {
    let (_dir, mut d) = open();
    let graph = d.tile().tscene().scene_graph().unwrap();
    let b = d.tile().tscene().find_all("BUILDING_INSTANCE")[0];
    let g1 = d.group_instances(&[b], graph, "one").unwrap();
    let p = d.tile().tscene().find_all("AI_HINT_POLYGON_INSTANCE")[0];
    let g2 = d.group_instances(&[p], graph, "two").unwrap();
    let g1 = d.tile().tscene().find_all("UserInstanceGroup").into_iter().find(|&g| d.tile().tscene().name(g) == "one").unwrap_or(g1);
    let freed = d.ungroup(&[g1, g2], graph).unwrap();
    let sc = d.tile().tscene();
    assert!(sc.find_all("UserInstanceGroup").is_empty());
    let mut types = freed.iter().map(|&f| sc.type_of(f).to_owned()).collect::<Vec<_>>();
    types.sort();
    assert_eq!(types, ["AI_HINT_POLYGON_INSTANCE", "BUILDING_INSTANCE"], "the freed indices point at the moved items");
}

#[test]
fn prefab_copies_are_placed_and_updated_together() {
    let (_dir, mut d) = open();
    let original = text(&d);
    let graph = d.tile().tscene().scene_graph().unwrap();
    let b = d.tile().tscene().find_all("BUILDING_INSTANCE")[0];
    let g = d.group_instances(&[b], graph, "rocks").unwrap();

    // Only references are copies: a group just named after the prefab is not one.
    assert!(d.prefab_copies("rocks").is_empty());
    assert!(d.make_prefab_reference(g, "rocks"));
    let group_text = d.element_text(g);
    let children = d.children_text(g);
    assert!(group_text.starts_with(b"<UserInstanceGroup ") && children.starts_with(b"<BUILDING_INSTANCE "));

    // A second copy, placed elsewhere: same contents, its own place.
    let c = d.place_prefab("rocks", &group_text, graph, Float3::new(20.0, 0.0, 20.0)).unwrap();
    assert_eq!(d.prefab_copies("ROCKS").len(), 2);
    assert_eq!(d.children_text(c), children);
    let p = d.tile().world_position(c);
    assert!((p.x - 20.0).abs() < 1e-3 && (p.z - 20.0).abs() < 1e-3);

    // New contents for every copy, one entry; copies that already have them do not change.
    let new_children = String::from_utf8(children.clone()).unwrap().replace("key=\"house\"", "key=\"tower\"").into_bytes();
    let copies = d.prefab_copies("rocks");
    assert_eq!(d.set_groups_children(&copies, &new_children, "Update rocks").unwrap(), 2);
    assert!(d.prefab_copies("rocks").iter().all(|&g| d.children_text(g) == new_children));
    let copies = d.prefab_copies("rocks");
    assert_eq!(d.set_groups_children(&copies, &new_children, "Update rocks").unwrap(), 0);

    for _ in 0..5 {
        d.history_mut().undo();
    }

    assert_eq!(text(&d), original);
}

#[test]
fn every_kind_is_created_with_its_parts() {
    let (_dir, mut d) = open();
    let original = text(&d);
    let graph = d.tile().tscene().scene_graph().unwrap();
    let at = Create::Point(Float3::new(10.0, 0.0, 10.0));
    let drawn = Create::Line(vec![Float3::new(0.0, 0.0, 0.0), Float3::new(20.0, 0.0, 0.0), Float3::new(20.0, 0.0, 20.0)]);
    let cases: [(&str, &Create, &str, &[&str]); 12] = [
        ("POINT_LIGHT_INSTANCE", &at, "", &[]),
        ("ProjectedDecalInstance", &at, "rigidmodels/decals/x.rigid_model_v2", &[]),
        ("VFXInstance", &at, "enviro_vfx_smoke", &[]),
        ("CivilianShelter", &at, "", &[]),
        ("CaptureLocationInstance", &at, "", &["CaptureLocationPolygonInstance"]),
        ("SiegeAINodeInstance", &at, "", &["SIEGE_AI_POLYGON_INSTANCE"]),
        ("DeploymentZonePolygonInstanceGroup", &at, "", &["DEPLOYMENT_ZONE_POLYGON_INSTANCE"]),
        ("ZONE_POLYGON_INSTANCE", &drawn, "", &[]),
        ("LiteBuildingZone", &drawn, "", &[]),
        ("AISeparatorInstance", &drawn, "", &[]),
        ("SiegeAIEdgeInstance", &drawn, "", &["SIEGE_AI_POLYGON_INSTANCE", "SIEGE_AI_POLYGON_INSTANCE"]),
        ("DEPLOYMENT_ZONE_POLYGON_INSTANCE", &drawn, "", &[]),
    ];

    for (kind, shape, key, parts) in cases {
        let e = d.create(kind, graph, shape, key).unwrap_or_else(|err| panic!("{kind}: {err}"));
        let sc = d.tile().tscene();
        let made = if kind == "DEPLOYMENT_ZONE_POLYGON_INSTANCE" { sc.children(e)[0] } else { e };
        assert_eq!(sc.type_of(made), kind, "a drawn deployment polygon outside a group comes in a new group");
        assert_eq!(sc.children(made).iter().map(|&c| sc.type_of(c)).collect::<Vec<_>>(), parts, "{kind}");
        if !key.is_empty() {
            let attr = if kind == "VFXInstance" { "vfx" } else { "key" };
            assert_eq!(sc.doc().get(made, attr).as_deref(), Some(key));
        }

        if let Create::Point(p) = shape {
            let w = d.tile().world_position(made);
            assert!((w.x - p.x).abs() < 1e-3 && (w.z - p.z).abs() < 1e-3, "{kind} at {w:?}");
        }
    }

    // The file reads back, and every addition undoes.
    assert!(wce_tile::ted_scene::TedScene::parse(text(&d)).is_ok());
    for _ in 0..12 {
        d.history_mut().undo();
    }

    assert_eq!(text(&d), original);
}

#[test]
fn a_spline_is_drawn_and_edited_with_its_handles() {
    let (_dir, mut d) = open();
    let original = text(&d);
    let graph = d.tile().tscene().scene_graph().unwrap();
    let drawn = Create::Line(vec![Float3::new(0.0, 0.0, 0.0), Float3::new(0.0, 0.0, 30.0), Float3::new(30.0, 0.0, 30.0)]);
    let r = d.create("SplineRiverInstance", graph, &drawn, "").unwrap();
    let pts = |d: &TileDocument| wce_tile::splines::read(d.tile().tscene(), r);
    let first = pts(&d);
    assert_eq!(first.len(), 3);
    assert_eq!(first[0].position, Double3::ZERO, "TEd's layout: the instance at the first point");
    assert_eq!((first[0].control_in, first[2].control_out), (Double3::ZERO, Double3::ZERO), "unused handles are 0,0,0");

    // A moved point takes its handles along; a moved handle mirrors the other.
    d.spline_point(r, "move", 1, Float3::new(5.0, 0.0, 30.0), 1).unwrap();
    d.spline_point(r, "move", 1, Float3::new(10.0, 0.0, 30.0), 1).unwrap();
    let moved = pts(&d);
    let shift = moved[1].position - first[1].position;
    assert!((moved[1].control_in - (first[1].control_in + shift)).max_abs_diff(Double3::ZERO) < 1e-3, "{moved:?}");
    d.spline_point(r, "out", 1, Float3::new(20.0, 0.0, 40.0), 0).unwrap();
    let h = pts(&d)[1];
    assert!((wce_tile::splines::mirror(h.position, h.control_out) - h.control_in).max_abs_diff(Double3::ZERO) < 1e-3, "{h:?}");
    assert!(d.spline_point(r, "in", 0, Float3::new(1.0, 0.0, 1.0), 0).is_err(), "the first point's controlIn is not used");

    // Growing at both ends and removing the ends keep the unused handles at 0,0,0.
    d.spline_point(r, "insert", 2, Float3::new(60.0, 0.0, 30.0), 0).unwrap();
    d.spline_point(r, "prepend", 0, Float3::new(0.0, 0.0, -30.0), 0).unwrap();
    let grown = pts(&d);
    assert_eq!(grown.len(), 5);
    assert_eq!((grown[0].control_in, grown[4].control_out), (Double3::ZERO, Double3::ZERO));
    assert!(grown[3].control_out != Double3::ZERO && grown[1].control_in != Double3::ZERO, "the old ends have both handles now");
    d.spline_point(r, "remove", 4, Float3::default(), 0).unwrap();
    d.spline_point(r, "remove", 0, Float3::default(), 0).unwrap();
    let cut = pts(&d);
    assert_eq!((cut.len(), cut[0].control_in, cut[2].control_out), (3, Double3::ZERO, Double3::ZERO));

    // Values of points, then a road; everything undoes.
    assert!(d.set_spline_value(r, &[0, 1, 2], "width", "40", 0));
    assert_eq!(d.tile().tscene().doc().get(d.tile().tscene().polygon_points(r)[2], "width").as_deref(), Some("40"));
    d.create("SEGMENTED_SPLINE_INSTANCE", graph, &drawn, "").unwrap();
    assert!(wce_tile::ted_scene::TedScene::parse(text(&d)).is_ok());
    while d.history_mut().can_undo() {
        d.history_mut().undo();
    }

    assert_eq!(text(&d), original);
}

#[test]
fn an_item_tilted_to_its_surface_keeps_its_heading() {
    let (_dir, mut d) = open();
    let original = text(&d);
    let b = d.tile().tscene().find_all("BUILDING_INSTANCE")[0];
    d.rotate(b, 30.0);
    let heading = |d: &TileDocument| {
        let f = wce_tile::world_transform::world_transform(d.tile().tscene(), b).rotation * Double3::new(0.0, 0.0, 1.0);
        f.x.atan2(f.z)
    };

    let before = heading(&d);
    let n = Double3::new(0.3, 1.0, -0.2);
    assert!(d.tilt_to_surface(b, n, 0));
    let w = wce_tile::world_transform::world_transform(d.tile().tscene(), b).rotation;
    let up = w * Double3::new(0.0, 1.0, 0.0);
    let len = (n.x * n.x + n.y * n.y + n.z * n.z).sqrt();
    assert!((up - n * (1.0 / len)).max_abs_diff(Double3::ZERO) < 1e-6, "up {up:?}");
    assert!((heading(&d) - before).abs() < 0.05, "the heading is kept: {} vs {before}", heading(&d));
    while d.history_mut().can_undo() {
        d.history_mut().undo();
    }

    assert_eq!(text(&d), original);
}

#[test]
fn import_from_the_kit_puts_the_walls_back_in_place_of_their_pieces() {
    use wce_tile_editor::tile_document::WALL_SPLINE;

    // The project: a wall in the Buildings layer.
    let (_dir, mut project) = open();
    let layer = project.tile().find_layer("Buildings");
    let pts = [Float3::new(10.0, 0.0, 10.0), Float3::new(40.0, 0.0, 10.0), Float3::new(60.0, 0.0, 30.0)];
    project.add_polygon(layer, WALL_SPLINE, "barbarian_fort_straight", &pts).unwrap();
    let walls = project.wall_texts();
    assert_eq!(walls.len(), 1);
    assert_eq!(walls[0].0, "Buildings");

    // The kit's copy, as Save for Assembly Kit wrote it: no wall, its pieces in their own layer.
    let pieces = r#"                <LayerInstance flags=" OIF_VISIBLE|OIF_ACTIVE" export_type="LayerExportType_Normal" lf_offset="0" position="vector(0,0,0)" name="WCE procedural walls" surface_mode="MSM_FREEFORM" orientation="euler(0,0,0)">
                    <children>
                        <BUILDING_INSTANCE flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(10,0,10)" name="Wall: wce_x_0_00" surface_mode="MSM_LANDSCAPE" orientation="euler(0,0,0)" key="wce_x_0_00"/>
                    </children>
                </LayerInstance>
            </children>
        </SCENE_GRAPH>"#;
    let kit_text = TED.replacen("            </children>\n        </SCENE_GRAPH>", pieces, 1);
    let kit = tempfile::tempdir().unwrap();
    std::fs::write(kit.path().join("tile.ted"), &kit_text).unwrap();
    let mut d = TileDocument::new(TileProject::open(kit.path()).unwrap(), None);
    let before = text(&d);

    assert_eq!(d.restore_walls(&walls).unwrap(), 1);
    let after = String::from_utf8(text(&d)).unwrap();
    assert!(!after.contains("WCE procedural walls") && !after.contains("wce_x_0_00"), "{after}");
    let sc = d.tile().tscene();
    let restored = sc.find_all(WALL_SPLINE);
    assert_eq!(restored.len(), 1);
    assert_eq!(sc.layer_path(restored[0]), "Buildings");
    assert_eq!(sc.polygon_points(restored[0]).len(), 3);

    // One undo entry.
    d.history_mut().undo();
    assert_eq!(text(&d), before);
}

#[test]
fn a_wall_is_drawn_as_the_create_panel_draws_it() {
    use wce_tile_editor::tile_document::WALL_SPLINE;

    let (_dir, mut d) = open();
    let layer = d.tile().find_layer("Buildings").unwrap();
    let pts = vec![Float3::new(10.0, 0.0, 10.0), Float3::new(40.0, 0.0, 10.0), Float3::new(60.0, 0.0, 30.0)];
    let attrs = std::collections::BTreeMap::from([("wall_base".to_owned(), "roman_fort_straight".to_owned())]);
    let wall = d.create_with(WALL_SPLINE, layer, &Create::Line(pts), "", &attrs).unwrap();

    let t = d.tile();
    assert!(t.is_wall(wall) && t.is_polygon(wall) && t.is_instance(wall));
    assert_eq!(t.tscene().polygon_points(wall).len(), 3);
    assert_eq!(t.tscene().layer_path(wall), "Buildings");
    let get = |a: &str| t.tscene().doc().get(wall, a).unwrap_or_default();
    assert_eq!((get("wall_base").as_str(), get("polygon_closed").as_str(), get("wall_tolerance").as_str()), ("roman_fort_straight", "0", "0.5"));
    assert!(t.wall_layout(wall).is_some(), "the wall has a layout");
}

#[test]
fn a_reference_is_an_element_the_tile_keeps_and_no_export_sees() {
    use wce_tile_editor::tile_document::{REF_HUMAN, REF_UNIT};

    let (_dir, mut d) = open();
    let layer = d.tile().find_layer("Buildings").unwrap();
    let attrs = std::collections::BTreeMap::from([("unit".to_owned(), "att_rom_sagittarii".to_owned())]);
    let human = d.create_with(REF_HUMAN, layer, &Create::Point(Float3::new(10.0, 0.0, 10.0)), "", &attrs).unwrap();
    let unit = d.create(REF_UNIT, layer, &Create::Point(Float3::new(30.0, 0.0, 10.0)), "").unwrap();
    let again = d.create(REF_HUMAN, layer, &Create::Point(Float3::new(20.0, 0.0, 10.0)), "").unwrap();

    // As many as wanted, and each an instance like any other.
    let mut scene = wce_scene::render_scene::RenderScene::default();
    let scene_layer = scene.add_layer(1, "tile");
    wce_editor_core::document::Document::republish(&mut d, &mut scene, scene_layer, &wce_editor_core::command::Dirty::default());
    let t = d.tile();
    assert_eq!(t.tscene().find_all(REF_HUMAN).len(), 2);
    assert!(t.is_instance(human) && t.is_instance(unit) && t.is_instance(again) && !t.is_polygon(unit));
    assert_eq!(t.tscene().doc().get(human, "unit").as_deref(), Some("att_rom_sagittarii"));
    assert_eq!(t.tscene().doc().get(unit, "surface_mode").as_deref(), Some("MSM_LANDSCAPE"));

    // A click finds a soldier of it, well away from the element.
    let (origin, down) = (Float3::new(35.0, 8.0, 5.0), wce_foundation::math::normalize(Float3::new(5.0, -7.0, 5.0)));
    assert_eq!(t.pick_instance(origin, down, false), None);
    t.set_ref_spots(vec![(unit, Float3::new(40.0, 1.0, 10.0), 0.8)]);
    assert_eq!(t.pick_instance(origin, down, false), Some(unit));

    // Saved in the tile; TEd's text and what is built from it have none.
    let wce = t.tscene().save();
    assert!(String::from_utf8_lossy(&wce).contains(REF_UNIT));
    let (ted, report) = wce_tile::project_files::to_ted(wce).unwrap();
    assert!(!String::from_utf8_lossy(&ted).contains("WceRef"));
    assert!(report.dropped.iter().any(|d| d.contains(REF_UNIT)), "{report:?}");

    d.history_mut().undo();
    assert_eq!(d.tile().tscene().find_all(REF_HUMAN).len(), 1);
}

#[test]
fn a_building_copied_from_an_earlier_layer_goes_into_a_later_one() {
    use wce_tile_editor::tile_document::Placement;

    let (_dir, mut d) = open();
    let graph = d.tile().tscene().scene_graph();
    let later = d.add_layer(graph, "Later").unwrap();
    let template = d.tile().tscene().find_all("BUILDING_INSTANCE")[0];

    // The copy goes in after its template, before the later layer: the layer's index moves under it.
    let added = d.add_building_in("tower", Some(template), Some(Float3::new(20.0, 0.0, 20.0)), Placement::Landscape, Some(later)).unwrap();
    let sc = d.tile().tscene();
    assert_eq!(sc.layer_path(added), "Later");
    assert_eq!(sc.doc().get(added, "key").as_deref(), Some("tower"));
    assert_eq!(sc.layer_path(template), "Buildings");
}
