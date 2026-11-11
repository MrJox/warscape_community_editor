//! Undo of tile document edits gives the exact bytes back: the cases the differential dump found broken in the C++ code (and fixed in both).

use std::collections::BTreeMap;
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
    assert!(d.tile().is_polygon(p));
    assert_eq!(d.set_placement(&[p], wce_tile_editor::tile_document::Placement::Landscape), 1);
    assert!(d.tile().off_terrain(-1.0, Some(&[p])).is_empty(), "a polygon's pivot is no height: nothing to report");
    d.history_mut().undo();
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
    use wce_tile_editor::tile_document::{tilted, turned};

    let (_dir, mut d) = open_with_house();
    let mut scene = RenderScene::default();
    let layer = scene.add_layer(d.id(), "tile");
    d.publish(&mut scene, layer);
    let text = b"<BUILDING_INSTANCE key=\"house\" name=\"h\" position=\"vector(0,0,0)\" orientation=\"euler(0,0,0)\" surface_mode=\"MSM_FREEFORM\"/>".to_vec();
    let at = Float3::new(20.0, 3.0, 30.0);
    let shown = |s: &RenderScene| s.entities().iter().filter(|e| e.transient).map(|e| (e.id, e.world)).collect::<Vec<_>>();

    d.tile_mut().set_preview(&mut scene, Some(&text), at, 0.0, None);
    let (before, structure) = (shown(&scene), scene.structure_revision());
    assert!(!before.is_empty(), "the preview shows");

    d.tile_mut().set_preview(&mut scene, Some(&text), at, 37.0, None);
    let by_yaw = shown(&scene);
    assert_eq!(by_yaw.iter().map(|e| e.0).collect::<Vec<_>>(), before.iter().map(|e| e.0).collect::<Vec<_>>(), "the same entities");
    assert_eq!(scene.structure_revision(), structure, "nothing was made again");
    assert!(by_yaw[0].1 != before[0].1, "it turned");

    // What a click places, shown from its turned text.
    d.tile_mut().set_preview(&mut scene, Some(&turned(&text, 37.0)), at, 0.0, None);
    let placed = shown(&scene);
    assert_eq!(placed.len(), by_yaw.len());
    let same = |a: &[(u32, wce_foundation::math::Float4x4)], b: &[(u32, wce_foundation::math::Float4x4)]| {
        assert_eq!(a.len(), b.len());
        for (a, b) in a.iter().zip(b) {
            for r in 0..3 {
                for c in 0..4 {
                    assert!((a.1.m[r][c] - b.1.m[r][c]).abs() < 1e-3, "[{r}][{c}]: {:?} for {:?}", a.1.m, b.1.m);
                }
            }
        }
    };

    same(&by_yaw, &placed);

    // Tilted to a surface as well: the same entities again, and what a click places: turned, then tilted.
    let normal = Double3::new(0.3, 1.0, -0.2);
    d.tile_mut().set_preview(&mut scene, Some(&text), at, 0.0, None);
    let (before, structure) = (shown(&scene), scene.structure_revision());
    d.tile_mut().set_preview(&mut scene, Some(&text), at, 37.0, Some(normal));
    let by_matrix = shown(&scene);
    assert_eq!(by_matrix.iter().map(|e| e.0).collect::<Vec<_>>(), before.iter().map(|e| e.0).collect::<Vec<_>>(), "the same entities");
    assert_eq!(scene.structure_revision(), structure, "nothing was made again");
    d.tile_mut().set_preview(&mut scene, Some(&tilted(&turned(&text, 37.0), normal)), at, 0.0, None);
    same(&by_matrix, &shown(&scene));
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
        r#"<SplineRiverInstance flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(20,0,10)" name="r" surface_mode="MSM_FREEFORM" orientation="euler(0,0.4,0)" wce_river_height="game">
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
        r#"<SplineRiverInstance flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(20,0,10)" name="land" surface_mode="MSM_LANDSCAPE" orientation="euler(0,0,0)" wce_river_height="game">
                            <points>
                                <point position="0,1,0" controlIn="0,0,0" controlOut="4,1,0"/>
                                <point position="10,2,0" controlIn="6,2,1" controlOut="0,0,0"/>
                            </points>
                        </SplineRiverInstance>
                        <SplineRiverInstance flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(20,0,10)" name="free" surface_mode="MSM_FREEFORM" orientation="euler(0,0,0)" wce_river_height="game">
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
fn a_flipped_river_of_teds_goes_where_the_game_has_it_and_keeps_its_real_heights() {
    use wce_tile::river_files::{Factors, RIVER_HEIGHT_ATTR, river_files};
    use wce_tile::splines;
    use wce_tile::world_transform::world_transform;
    use wce_tile_editor::tile_document::FlipPlane;

    // No mark and no file yet: as TEd's today, at scene / 1.25 in the game.
    let text = TED.replace(
        r#"<AI_HINT_POLYGON_INSTANCE"#,
        r#"<SplineRiverInstance flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(20,10,10)" name="r" surface_mode="MSM_FREEFORM" orientation="euler(0,0,0)">
                            <points>
                                <point position="0,0,0" controlIn="0,0,0" controlOut="4,0,0"/>
                                <point position="10,5,0" controlIn="6,5,1" controlOut="0,0,0"/>
                            </points>
                        </SplineRiverInstance>
                        <AI_HINT_POLYGON_INSTANCE"#,
    );
    let (dir, _) = open();
    std::fs::write(dir.path().join("tile.ted"), text).unwrap();
    let mut d = TileDocument::new(TileProject::open(dir.path()).unwrap(), None);
    let original = self::text(&d);
    let sc = d.tile().tscene();
    let river = sc.find_all("SplineRiverInstance")[0];
    let layer = sc.find_all("LayerInstance")[0];
    assert_eq!(Factors::of(sc, river, None).height, 1.25, "TEd's");

    let world = |d: &TileDocument, e: usize| {
        let w = world_transform(d.tile().tscene(), e);
        splines::read(d.tile().tscene(), e).iter().map(|p| (w.position + w.rotation * p.position).y).collect::<Vec<_>>()
    };

    let source = world(&d, river);
    let plane = FlipPlane { x: 40.0, z: 0.0, angle: 0.0 };

    // A copy: the source stays as it was, the copy has in the scene the height the source has in game.
    let copy = d.flip(&[river], plane, Some(layer)).unwrap()[0];
    let sc = d.tile().tscene();
    assert!(sc.doc().get(river, RIVER_HEIGHT_ATTR).is_none());
    assert_eq!(sc.doc().get(copy, RIVER_HEIGHT_ATTR).as_deref(), Some("game"));
    assert_eq!(Factors::of(sc, copy, None).height, 1.0);
    for (a, b) in source.iter().zip(world(&d, copy)) {
        assert!((a / 1.25 - b).abs() < 1e-6, "{a} / 1.25 against {b}");
    }

    // Their files: both at the same height for the game.
    let files = river_files(d.tile().tscene(), None, false);
    let first_y = |t: &[u8]| {
        let t = String::from_utf8_lossy(t).into_owned();
        t.split(" y=\"").nth(1).unwrap().split('"').next().unwrap().parse::<f64>().unwrap()
    };
    assert_eq!(first_y(&files[0].1), first_y(&files[1].1), "the same height in game");

    d.history_mut().undo();
    assert_eq!(self::text(&d), original, "undo gives the file back");
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
fn a_fires_char_moves_the_instances_and_nothing_else() {
    use wce_scene::render_scene::{Payload, RenderScene};

    let (_dir, mut d) = open_with_house();
    let mut scene = RenderScene::default();
    let layer = scene.add_layer(d.id(), "tile");
    d.publish(&mut scene, layer);
    let building = d.tile().tscene().find_all("BUILDING_INSTANCE")[0];
    let burn = |scene: &RenderScene| scene.entities().iter().find_map(|e| match &e.payload {
        Payload::MeshInstance(mi) if e.source.item == item_of(building) => Some((mi.p6[0], mi.burning)),
        _ => None,
    }).unwrap();

    d.tile_mut().update_fire(&mut scene, 0.0);
    d.tile_mut().fire_start(building, 0.0);
    d.tile_mut().update_fire(&mut scene, 0.1);
    assert!(burn(&scene).1, "it burns");

    let structure = scene.structure_revision();
    let mut cursor = 0;
    let _ = scene.burn_since(&mut cursor);
    let was = burn(&scene).0;
    d.tile_mut().update_fire(&mut scene, 9.0);
    assert!(burn(&scene).0 > was, "the char moves");
    assert_eq!(scene.structure_revision(), structure, "nothing is built again");
    assert!(!scene.burn_since(&mut cursor).unwrap().is_empty(), "the renderer is told");

    // The next stage changes what plays: that is built.
    d.tile_mut().update_fire(&mut scene, 20.0);
    assert!(scene.structure_revision() != structure);
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
fn a_whole_tile_flip_mirrors_the_items_and_the_ground_under_them_and_undoes_at_once() {
    use wce_tile::world_transform::world_transform;
    use wce_tile_editor::tile_document::TileFlipAxis;

    let (_dir, mut d) = open();
    let original = text(&d);
    let unit = d.tile().unit();
    let (ex, ez) = (d.project().extent_x() as f32, d.project().extent_z() as f32);
    let house = d.tile().tscene().find_all("BUILDING_INSTANCE")[0];
    let pos = |d: &TileDocument| world_transform(d.tile().tscene(), house).position;
    let p0 = pos(&d);
    let (hx, hz) = ((p0.x / unit) as f32, (p0.z / unit) as f32);
    let ground_before = d.tile().terrain_height(hx, hz);
    let heights = |d: &TileDocument| (0..9).flat_map(|i| (0..9).map(move |j| (i as f32 * ex / 8.0, j as f32 * ez / 8.0))).map(|(x, z)| d.tile().terrain_height(x, z)).collect::<Vec<_>>();
    let before_heights = heights(&d);
    let size = d.history().size();

    for axis in [TileFlipAxis::X, TileFlipAxis::Z] {
        d.flip_tile(axis).unwrap();
        assert_eq!(d.history().size(), size + 1, "one undo entry");
        let p = pos(&d);
        let (mx, mz) = if axis == TileFlipAxis::X { (ex - hx, hz) } else { (hx, ez - hz) };
        assert!(((p.x / unit) as f32 - mx).abs() < 1e-3 && ((p.z / unit) as f32 - mz).abs() < 1e-3, "{axis:?}: the house goes across: {p:?}");
        assert!((p.y - p0.y).abs() < 1e-6, "{axis:?}: a landscape item keeps its height, the ground went with it");
        let ground = d.tile().terrain_height(mx, mz);
        assert!((ground - ground_before).abs() < 1e-3, "{axis:?}: the ground under it went with it: {ground} vs {ground_before}");
        let mirrored = heights(&d);
        for i in 0..9 {
            for j in 0..9 {
                let (a, b) = if axis == TileFlipAxis::X { (8 - i, j) } else { (i, 8 - j) };
                assert!((mirrored[i * 9 + j] - before_heights[a * 9 + b]).abs() < 1e-3, "{axis:?}: the heights mirror at ({i}, {j})");
            }
        }

        d.history_mut().undo();
        assert_eq!(text(&d), original, "{axis:?}: undo gives the file back");
        assert_eq!(heights(&d), before_heights, "{axis:?}: and the heights");
        d.history_mut().redo();
        assert!(((pos(&d).x / unit) as f32 - mx).abs() < 1e-3, "{axis:?}: redo");
        assert_eq!(heights(&d), mirrored, "{axis:?}: redo mirrors the heights again");
        d.history_mut().undo();
    }
}

#[test]
fn a_whole_tile_flip_mirrors_the_terrain_layers_and_undoes_them() {
    use wce_tile_editor::tile_document::TileFlipAxis;
    use wce_tile_editor::tile_document::layers::LayerType;

    let (_dir, mut d) = open();
    let (ex, ez) = (d.project().extent_x() as f32, d.project().extent_z() as f32);
    let heights = |d: &TileDocument| (0..9).flat_map(|i| (0..9).map(move |j| (i as f32 * ex / 8.0, j as f32 * ez / 8.0))).map(|(x, z)| d.tile().terrain_height(x, z)).collect::<Vec<_>>();
    let layer = d.add_terrain_layer(LayerType::Height, "bump").unwrap();
    d.set_working_layer(LayerType::Height, Some(layer));
    d.brush_step(BrushKind::Raise, ex * 0.25, ez * 0.25, BrushSettings::default(), 0.05);
    d.end_stroke();
    d.refresh_terrain();
    let before = heights(&d);

    d.flip_tile(TileFlipAxis::X).unwrap();
    d.refresh_terrain();
    let after = heights(&d);
    for i in 0..9 {
        for j in 0..9 {
            assert!((after[i * 9 + j] - before[(8 - i) * 9 + j]).abs() < 1e-3, "the heights mirror at ({i}, {j})");
        }
    }

    // The layer itself went across: hidden, the bump goes with it from both sides.
    d.set_terrain_layer_visible(LayerType::Height, layer, false);
    d.refresh_terrain();
    let hidden = heights(&d);
    d.set_terrain_layer_visible(LayerType::Height, layer, true);
    d.refresh_terrain();
    assert_eq!(heights(&d), after);
    assert!(hidden.iter().zip(&after).any(|(a, b)| (a - b).abs() > 1e-3), "the bump is in the layer");

    d.history_mut().undo();
    d.history_mut().undo();
    d.history_mut().undo();
    d.refresh_terrain();
    assert_eq!(heights(&d), before, "undo puts the layer back where it was");
}

/// `open`'s tile with a 32 x 32 blend map: all the first texture, but channel 5 in its west quarter.
fn open_with_blend() -> (tempfile::TempDir, TileDocument) {
    let (dir, _) = open();
    let (w, h) = (32u32, 32u32);
    let mut bytes = wce_formats::image_containers::TiffFile::ted_header(w, h);
    for _ in 0..h {
        for x in 0..w {
            let mut px = [0u16; 16];
            px[if x < w / 4 { 5 } else { 0 }] = u16::MAX;
            bytes.extend(px.iter().flat_map(|v| v.to_le_bytes()));
        }
    }

    std::fs::write(dir.path().join("blend_map.tif"), bytes).unwrap();
    let doc = TileDocument::new(TileProject::open(dir.path()).unwrap(), None);
    (dir, doc)
}

#[test]
fn painting_the_whole_tile_is_one_dab_everywhere_and_one_undo_entry() {
    let (_dir, mut d) = open_with_blend();
    d.tile_mut().load_blend().unwrap();
    let weights = |d: &TileDocument| d.project().blend.as_ref().unwrap().weights().to_vec();
    let before = weights(&d);
    let size = d.history().size();

    // Full strength: every pixel is the texture.
    let s = BrushSettings { opacity: 1.0, ..BrushSettings::default() };
    assert!(d.paint_whole_tile(BrushKind::Paint(9), &s).unwrap());
    assert_eq!(d.history().size(), size + 1);
    assert!(weights(&d).chunks(16).all(|p| p[9] == u16::MAX && p.iter().map(|&v| u32::from(v)).sum::<u32>() == 65535), "all channel 9");
    assert!(!d.paint_whole_tile(BrushKind::Paint(9), &s).unwrap(), "again: nothing changes, no entry");
    assert_eq!(d.history().size(), size + 1);
    d.history_mut().undo();
    assert_eq!(weights(&d), before);
    d.history_mut().redo();
    assert!(weights(&d).chunks(16).all(|p| p[9] == u16::MAX));
    d.history_mut().undo();

    // Half strength: half of what each pixel held moves over, the same everywhere.
    let s = BrushSettings { opacity: 0.5, ..BrushSettings::default() };
    d.paint_whole_tile(BrushKind::Paint(9), &s).unwrap();
    assert!(weights(&d).chunks(16).all(|p| (i32::from(p[9]) - 32768).abs() <= 1), "half: {:?}", &weights(&d)[..16]);
}

#[test]
fn a_whole_tile_flip_mirrors_the_blend_map() {
    use wce_tile_editor::tile_document::TileFlipAxis;

    let (_dir, mut d) = open_with_blend();
    d.flip_tile(TileFlipAxis::X).unwrap();
    let b = d.project().blend.as_ref().unwrap();
    assert!((0..32).all(|x| (b.pixel(x, 7)[5] == u16::MAX) == (x >= 24)), "the painted quarter is in the east now");
    d.history_mut().undo();
    let b = d.project().blend.as_ref().unwrap();
    assert!((0..32).all(|x| (b.pixel(x, 7)[5] == u16::MAX) == (x < 8)));
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

#[test]
fn the_pieces_of_a_building_have_their_own_properties() {
    let (_dir, mut d) = open_with_house();
    let b = d.tile().tscene().find_all("BUILDING_INSTANCE")[0];

    // One item per piece (the destruction states of a piece are not pieces).
    let count = d.tile().model_of(b).filter(|m| m.ok).map_or(0, |m| m.pieces.len());
    assert_eq!(count, 2, "two pieces");
    let pieces = (0..count).map(|i| wce_scene::render_scene::SourceRef { doc: d.id(), item: wce_tile_editor::tile_document::piece_item(b, i) }).collect::<Vec<_>>();

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
        ("SiegeAINodeInstance", &at, "", &["SIEGE_AI_POLYGON_INSTANCE", "SIEGE_AI_POLYGON_INSTANCE"]),
        ("DeploymentZonePolygonInstanceGroup", &at, "", &["DEPLOYMENT_ZONE_POLYGON_INSTANCE"]),
        ("ZONE_POLYGON_INSTANCE", &drawn, "", &[]),
        ("LiteBuildingZone", &drawn, "", &[]),
        ("AISeparatorInstance", &drawn, "", &[]),
        ("SiegeAIEdgeInstance", &drawn, "", &["SIEGE_AI_POLYGON_INSTANCE", "SIEGE_AI_POLYGON_INSTANCE", "SIEGE_AI_POLYGON_INSTANCE"]),
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

        if kind == "DeploymentZonePolygonInstanceGroup" {
            assert_eq!(sc.doc().get(made, "deployment_category").as_deref(), Some("DAC_DEFAULT"), "a new zone is Default, not TEd's Ambush");
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
fn shapes_go_only_into_their_holders() {
    let (_dir, mut d) = open();
    let original = text(&d);
    let graph = d.tile().tscene().scene_graph().unwrap();
    let at = Create::Point(Float3::new(10.0, 0.0, 10.0));
    let square = Create::Line(vec![Float3::new(0.0, 0.0, 0.0), Float3::new(8.0, 0.0, 0.0), Float3::new(8.0, 0.0, 8.0), Float3::new(0.0, 0.0, 8.0)]);
    let names = |d: &TileDocument, h: usize| {
        let sc = d.tile().tscene();
        sc.children(h).into_iter().map(|c| sc.name(c)).collect::<Vec<_>>()
    };

    // TEd's shapes, by name, in TEd's order.
    let node = d.create("SiegeAINodeInstance", graph, &at, "").unwrap();
    assert_eq!(names(&d, node), ["Bounding Box", "Boundary"]);
    let line = Create::Line(vec![Float3::new(0.0, 0.0, 0.0), Float3::new(30.0, 0.0, 0.0)]);
    let edge = d.create("SiegeAIEdgeInstance", graph, &line, "").unwrap();
    assert_eq!(names(&d, edge), ["Street", "Street boundary", "Street boundary"]);

    // A shape alone goes nowhere but into a holder.
    assert!(d.create("SIEGE_AI_POLYGON_INSTANCE", graph, &square, "").is_err());
    assert!(d.create("CaptureLocationPolygonInstance", graph, &square, "").is_err());

    // Drawn into a node: the role asked for replaces the one it has, in its place; a Bounding Box is a rectangle.
    let node = d.tile().tscene().find_all("SiegeAINodeInstance")[0];
    let boundary = BTreeMap::from([("name".to_owned(), "Boundary".to_owned())]);
    let tri = Create::Line(vec![Float3::new(0.0, 0.0, 0.0), Float3::new(9.0, 0.0, 0.0), Float3::new(4.0, 0.0, 7.0)]);
    let _ = d.take_selection_hint();
    let b = d.create_with("SIEGE_AI_POLYGON_INSTANCE", node, &tri, "", &boundary).unwrap();
    assert_eq!(d.take_selection_hint(), Some(vec![item_of(node)]), "a drawn shape leaves its node selected");
    let node = d.tile().tscene().find_all("SiegeAINodeInstance")[0];
    assert_eq!(names(&d, node), ["Bounding Box", "Boundary"]);
    assert_eq!(d.tile().tscene().children(node)[1], b);
    let bbox = BTreeMap::from([("name".to_owned(), "Bounding Box".to_owned())]);
    let bb = d.create_with("SIEGE_AI_POLYGON_INSTANCE", node, &tri, "", &bbox).unwrap();
    let node = d.tile().tscene().find_all("SiegeAINodeInstance")[0];
    assert_eq!(names(&d, node), ["Bounding Box", "Boundary"]);
    assert_eq!(d.tile().tscene().polygon_points(bb).len(), 4, "the box around the drawn points");

    // A street edge has two boundaries at most.
    let edge = d.tile().tscene().find_all("SiegeAIEdgeInstance")[0];
    let side = BTreeMap::from([("name".to_owned(), "Street boundary".to_owned())]);
    assert!(d.create_with("SIEGE_AI_POLYGON_INSTANCE", edge, &line, "", &side).unwrap_err().contains("delete one first"));

    // Holders hold nothing else, and their shapes stay in them.
    let layer = d.add_layer(Some(graph), "L").unwrap();
    let node = d.tile().tscene().find_all("SiegeAINodeInstance")[0];
    assert!(d.create("POINT_LIGHT_INSTANCE", node, &at, "").is_err());
    let shape = d.tile().tscene().children(node)[0];
    assert!(d.move_items(&[shape], layer, None).is_err());
    assert!(d.group_items(&[shape], "G").is_err());

    // A shape moves from a holder to another of its kind.
    let other = d.create("SiegeAINodeInstance", graph, &at, "").unwrap();
    let node = d.tile().tscene().find_all("SiegeAINodeInstance")[0];
    let shape = d.tile().tscene().children(node)[0];
    d.move_items(&[shape], other, None).unwrap();
    let nodes = d.tile().tscene().find_all("SiegeAINodeInstance");
    assert_eq!((names(&d, nodes[0]).len(), names(&d, nodes[1]).len()), (1, 3));

    // A deployment polygon outside a zone (scripts): a new zone around it, the polygon with the values asked for it.
    let poly = BTreeMap::from([("deployment_polygon_type".to_owned(), "DZPT_SUBSTRACTIVE".to_owned())]);
    let g = d.create_with("DEPLOYMENT_ZONE_POLYGON_INSTANCE", graph, &square, "", &poly).unwrap();
    let sc = d.tile().tscene();
    assert_eq!(sc.type_of(g), "DeploymentZonePolygonInstanceGroup");
    assert_eq!(sc.doc().get(sc.children(g)[0], "deployment_polygon_type").as_deref(), Some("DZPT_SUBSTRACTIVE"));

    while d.history_mut().undo() {}
    assert_eq!(text(&d), original);
}

#[test]
fn a_bounding_box_is_a_rectangle_and_a_boundary_fits_the_ground() {
    let (_dir, mut d) = open();
    let original = text(&d);
    let graph = d.tile().tscene().scene_graph().unwrap();
    let (ex, ez) = (d.tile().extent_x() as f32, d.tile().extent_z() as f32);
    let (cx, cz) = (ex / 2.0, ez / 2.0);
    d.create("SiegeAINodeInstance", graph, &Create::Point(Float3::new(cx, 0.0, cz)), "").unwrap();
    let find = |d: &TileDocument| d.tile().tscene().find_all("SiegeAINodeInstance")[0];
    let shape = |d: &TileDocument, name: &str| {
        let sc = d.tile().tscene();
        sc.children(find(d)).into_iter().find(|&c| sc.name(c) == name).unwrap()
    };

    // Its first side, then a point at its depth (turned 30 degrees): a true rectangle, 12 x 6.
    let (s, c) = (0.5f32, 0.866f32);
    let at = |u: f32, v: f32| Float3::new(cx + u * c - v * s, 0.0, cz + u * s + v * c);
    let bbox = BTreeMap::from([("name".to_owned(), "Bounding Box".to_owned())]);
    for pts in [vec![at(-6.0, -3.0), at(6.0, -3.0), at(5.0, 3.0)], vec![at(-6.0, -3.0), at(6.0, -3.0), at(7.0, 3.0), at(-4.0, 2.0)]] {
        d.create_with("SIEGE_AI_POLYGON_INSTANCE", find(&d), &Create::Line(pts), "", &bbox).unwrap();
        let p = d.tile().polygon_world_points(shape(&d, "Bounding Box"));
        assert_eq!(p.len(), 4);
        let len = |a: Float3, b: Float3| ((a.x - b.x).powi(2) + (a.z - b.z).powi(2)).sqrt();
        assert!((len(p[0], p[1]) - 12.0).abs() < 0.05 && (len(p[1], p[2]) - 6.0).abs() < 0.05, "{p:?}");
        let dot = (p[1].x - p[0].x) * (p[2].x - p[1].x) + (p[1].z - p[0].z) * (p[2].z - p[1].z);
        assert!(dot.abs() < 0.05, "square corners: {p:?}");
        assert!((len(p[2], p[3]) - 12.0).abs() < 0.05);
    }

    // A no-go region 4 m east of the box: the Boundary stops at its edge (not grown onto it, as onto a building's face; within the
    // raster's 2 units), and reaches further the other ways.
    let nogo = [(cx + 10.0, cz - 30.0), (cx + 40.0, cz - 30.0), (cx + 40.0, cz + 30.0), (cx + 10.0, cz + 30.0)].map(|(x, z)| Float3::new(x, 0.0, z));
    d.add_polygon(Some(graph), "NOGO_REGION_POLYGON_INSTANCE", "", &nogo).unwrap();
    let _ = d.take_selection_hint();
    let b = d.adjust_node_boundary(find(&d), None).unwrap();
    assert_eq!(d.take_selection_hint(), Some(vec![item_of(find(&d))]), "the node stays selected, not its new Boundary");
    let sc = d.tile().tscene();
    assert_eq!(sc.children(find(&d)).into_iter().map(|c| sc.name(c)).collect::<Vec<_>>(), ["Bounding Box", "Boundary"], "replaced in place");
    assert_eq!(shape(&d, "Boundary"), b);
    let p = d.tile().polygon_world_points(b);
    let u = d.tile().unit() as f32;
    let east = p.iter().map(|q| q.x).fold(f32::MIN, f32::max);
    let west = p.iter().map(|q| q.x).fold(f32::MAX, f32::min);
    assert!(east <= cx + 10.0 + 2.0 / u, "stops at the no-go region's edge: {east} (centre {cx}, {u} units a metre)");
    assert!(west < cx - 10.0, "reaches out where the ground is free: {west}");

    // Another node 40 m west, its Boundary 24 m square (cx - 52 to cx - 28): even a wide range keeps out of it and does not go round it.
    d.create("SiegeAINodeInstance", graph, &Create::Point(Float3::new(cx - 40.0, 0.0, cz)), "").unwrap();
    let b = d.adjust_node_boundary(find(&d), Some(60.0)).unwrap();
    let p = d.tile().polygon_world_points(b);
    let m = 2.0 / u;
    let inside = |q: &Float3| q.x > cx - 52.0 + m && q.x < cx - 28.0 - m && q.z > cz - 12.0 + m && q.z < cz + 12.0 - m;
    assert!(!p.iter().any(inside), "a point inside the other node's Boundary: {p:?}");
    let mid = Float3::new(cx - 40.0, 0.0, cz);
    // Where its outline crosses the line through both centres.
    let xs = (0..p.len()).filter_map(|i| {
        let (a, b) = (p[i], p[(i + 1) % p.len()]);
        ((a.z > cz) != (b.z > cz)).then(|| a.x + (cz - a.z) / (b.z - a.z) * (b.x - a.x))
    }).collect::<Vec<_>>();
    assert!(xs.iter().all(|&x| x >= cx - 28.0 - m), "level with it, it is east of it: {xs:?} ({mid:?})");
    let holds = |o: Float3| {
        let mut inside = false;
        for i in 0..p.len() {
            let (a, b) = (p[i], p[(i + 1) % p.len()]);
            if (a.z > o.z) != (b.z > o.z) && o.x < a.x + (o.z - a.z) / (b.z - a.z) * (b.x - a.x) {
                inside = !inside;
            }
        }

        inside
    };
    assert!(!holds(mid) && !holds(Float3::new(cx - 51.0, 0.0, cz)), "not round it: it holds none of it");
    assert!(xs.iter().any(|&x| x <= cx - 28.0 + 2.0 * m), "up to it (they touch): {xs:?}");

    // A range: no farther from the centre than it (or the Bounding Box's corners, 7.2 m here), and the growth onto faces.
    let b = d.adjust_node_boundary(find(&d), Some(8.0)).unwrap();
    let far = d.tile().polygon_world_points(b).iter().map(|q| (q.x - cx).hypot(q.z - cz)).fold(0.0, f32::max);
    assert!(far <= 8.0 + 5.0 / u && far > 6.0, "within the range: {far}");

    while d.history_mut().undo() {}
    assert_eq!(text(&d), original);
}

#[test]
fn a_street_fits_its_boundaries_between_its_nodes() {
    let (_dir, mut d) = open();
    let original = text(&d);
    let graph = d.tile().tscene().scene_graph().unwrap();
    let (cx, cz) = (d.tile().extent_x() as f32 / 2.0, d.tile().extent_z() as f32 / 2.0);
    let u = d.tile().unit() as f32;
    for x in [cx - 30.0, cx + 30.0] {
        d.create("SiegeAINodeInstance", graph, &Create::Point(Float3::new(x, 0.0, cz)), "").unwrap();
    }

    // No-go strips 8 m to either side of the corridor between the nodes' Boundaries (12 m from their centres).
    for z in [cz + 8.0, cz - 8.0] {
        let far = if z > cz { z + 10.0 } else { z - 10.0 };
        let strip = [(cx - 18.0, z), (cx + 18.0, z), (cx + 18.0, far), (cx - 18.0, far)].map(|(x, z)| Float3::new(x, 0.0, z));
        d.add_polygon(Some(graph), "NOGO_REGION_POLYGON_INSTANCE", "", &strip).unwrap();
    }

    let line = Create::Line(vec![Float3::new(cx - 30.0, 0.0, cz), Float3::new(cx + 30.0, 0.0, cz)]);
    d.create("SiegeAIEdgeInstance", graph, &line, "").unwrap();
    let edge = d.tile().tscene().find_all("SiegeAIEdgeInstance")[0];
    assert!(d.street_nodes(edge).is_some(), "its ends are in the two nodes' rectangles");

    let _ = d.take_selection_hint();
    d.fit_street_sides(edge).unwrap();
    assert_eq!(d.take_selection_hint(), Some(vec![item_of(edge)]), "the edge stays selected");
    let sc = d.tile().tscene();
    let names = sc.children(edge).into_iter().map(|c| sc.name(c)).collect::<Vec<_>>();
    assert_eq!(names, ["Street", "Street boundary", "Street boundary"]);
    let sides = sc.children(edge)[1..].iter().map(|&c| d.tile().polygon_world_points(c)).collect::<Vec<_>>();
    for side in &sides {
        // Between the nodes' Boundaries, at the strips' faces; the ends inside the Boundaries.
        let mid = side.iter().filter(|p| (p.x - cx).abs() < 18.0).collect::<Vec<_>>();
        assert!(!mid.is_empty(), "{side:?}");
        assert!(mid.iter().all(|p| ((p.z - cz).abs() - 8.0).abs() < 2.5 / u + 0.5), "at the faces: {mid:?}");
        let (first, last) = (side[0], side[side.len() - 1]);
        assert!((first.x - (cx - 30.0)).abs() < 12.0 && (last.x - (cx + 30.0)).abs() < 12.0, "ends in the nodes: {first:?} {last:?}");

        // Each end joins the Boundary where the side reaches it (out at the side, not turned in to the line: no wedge left between).
        assert!((first.z - cz).abs() > 5.0 && (last.z - cz).abs() > 5.0, "sealed at the side: {first:?} {last:?}");
    }

    assert!((sides[0][sides[0].len() / 2].z - cz) * (sides[1][sides[1].len() / 2].z - cz) < 0.0, "one to each side");

    // An area connection has none.
    d.set_attribute(edge, "type", "SAET_AREA_CONNECTION");
    assert!(d.fit_street_sides(edge).is_err());

    while d.history_mut().undo() {}
    assert_eq!(text(&d), original);
}

#[test]
fn a_click_picks_a_holders_shapes() {
    let (_dir, mut d) = open();
    let graph = d.tile().tscene().scene_graph().unwrap();
    let (cx, cz) = (d.tile().extent_x() as f32 / 2.0, d.tile().extent_z() as f32 / 2.0);
    d.create("SiegeAINodeInstance", graph, &Create::Point(Float3::new(cx, 0.0, cz)), "").unwrap();
    let mut scene = wce_scene::render_scene::RenderScene::default();
    let scene_layer = scene.add_layer(1, "tile");
    wce_editor_core::document::Document::republish(&mut d, &mut scene, scene_layer, &wce_editor_core::command::Dirty::default());
    let sc = d.tile().tscene();
    let node = sc.find_all("SiegeAINodeInstance")[0];
    let (bbox, boundary) = (sc.children(node)[0], sc.children(node)[1]);
    let at = |x: f32, z: f32| Float3::new(cx + x, 0.0, cz + z);

    // Near an outline: that shape (the Bounding Box is 20 m square, the Boundary 24 m); inside both, away from them: the smaller.
    assert_eq!(d.tile().pick_polygon(at(11.8, 0.0), 0.5), Some(boundary));
    assert_eq!(d.tile().pick_polygon(at(10.2, 3.0), 0.5), Some(bbox));
    assert_eq!(d.tile().pick_polygon(at(2.0, 3.0), 0.5), Some(bbox));
    assert_eq!(d.tile().pick_polygon(at(11.0, 0.0), 0.5), Some(boundary));
    assert_eq!(d.tile().pick_polygon(at(30.0, 0.0), 0.5), None);
}

#[test]
fn a_capture_boundary_takes_preset_shapes() {
    use wce_tile_editor::tile_document::{BoundaryKind, BoundaryShape};

    let (_dir, mut d) = open();
    let original = text(&d);
    let graph = d.tile().tscene().scene_graph().unwrap();
    let (cx, cz) = (d.tile().extent_x() as f32 / 2.0, d.tile().extent_z() as f32 / 2.0);
    let loc = d.create("CaptureLocationInstance", graph, &Create::Point(Float3::new(cx, 0.0, cz)), "").unwrap();
    let b = d.capture_boundary(loc).unwrap();

    // TEd's 20 m circle reads back as one.
    let s = d.boundary_shape(b).unwrap();
    assert_eq!(s.kind, BoundaryKind::Circle);
    assert!((s.size - 20.0).abs() < 0.01 && (s.centre.x - cx).abs() < 0.01, "{s:?}");

    let near = |a: f32, b: f32, tol: f32| (a - b).abs() < tol;
    let cases = [(BoundaryKind::Semicircle, 15.0, 90.0, 21, Some("PS_SEMICIRCLE")), (BoundaryKind::Square, 10.0, 30.0, 4, Some("PS_RECTANGLE")), (BoundaryKind::Circle, 12.0, 0.0, 20, Some("PS_CIRCLE"))];
    for (kind, size, turn, points, ted) in cases {
        let b = d.capture_boundary(loc).unwrap();
        let _ = d.take_selection_hint();
        let new = d.set_boundary_shape(b, BoundaryShape { kind, centre: Float3::new(cx, 0.0, cz), size, turn }, Some(loc)).unwrap();
        assert_eq!(d.take_selection_hint(), Some(vec![item_of(loc)]), "the capture location stays selected");
        assert_eq!(d.capture_boundary(loc), Some(new), "in the old one's place");
        let sc = d.tile().tscene();
        assert_eq!(sc.doc().get(new, "polygon_shape").as_deref(), ted);
        assert_eq!(sc.name(new), "Boundary", "its own attributes kept");
        assert_eq!(d.tile().polygon_world_points(new).len(), points);
        let back = d.boundary_shape(new).unwrap();
        assert_eq!(back.kind, kind);
        assert!(near(back.size, size, 0.01), "{back:?}");
        if kind.turns() {
            assert!(near(back.turn, turn, 0.05), "{back:?}");
        }
    }

    // A semicircle turned 90: its round side towards +x.
    let b = d.capture_boundary(loc).unwrap();
    let semi = d.set_boundary_shape(b, BoundaryShape { kind: BoundaryKind::Semicircle, centre: Float3::new(cx, 0.0, cz), size: 10.0, turn: 90.0 }, None).unwrap();
    let pts = d.tile().polygon_world_points(semi);
    assert!(pts.iter().all(|p| p.x >= cx - 0.01) && near(pts[10].x, cx + 10.0, 0.01), "{pts:?}");

    // A polygon keeps its points, freely edited.
    let poly = d.set_boundary_shape(semi, BoundaryShape { kind: BoundaryKind::Polygon, centre: Float3::default(), size: 0.0, turn: 0.0 }, None).unwrap();
    assert_eq!(d.tile().tscene().doc().get(poly, "polygon_shape").as_deref(), Some("PS_POLYGON"));
    assert_eq!(d.tile().polygon_world_points(poly).len(), 21);

    while d.history_mut().undo() {}
    assert_eq!(text(&d), original);
}

#[test]
fn a_new_capture_location_takes_its_boundary_preset() {
    use wce_tile_editor::tile_document::{BoundaryKind, Draft};

    let (_dir, mut d) = open();
    let graph = d.tile().tscene().scene_graph().unwrap();
    let (cx, cz) = (d.tile().extent_x() as f32 / 2.0, d.tile().extent_z() as f32 / 2.0);
    let mut draft = Draft::new("CaptureLocationInstance", "");
    for (k, v) in [("wce_boundary_shape", "Semicircle"), ("wce_boundary_size", "10"), ("wce_boundary_turn", "90")] {
        draft.attrs.insert(k.to_owned(), v.to_owned());
    }

    let text = d.draft_text(&draft).unwrap();
    assert!(!String::from_utf8_lossy(&text).contains("wce_boundary"), "the preset is the draft's, not the element's");
    let loc = d.place_text("Add capture location", &text, graph, Float3::new(cx, 0.0, cz), None, true).unwrap();
    let b = d.capture_boundary(loc).unwrap();
    let s = d.boundary_shape(b).unwrap();
    assert_eq!(s.kind, BoundaryKind::Semicircle);
    assert!((s.size - 10.0).abs() < 0.01 && (s.turn - 90.0).abs() < 0.05, "{s:?}");

    // Without a preset: TEd's 20 m circle, as before.
    let plain = d.draft_text(&Draft::new("CaptureLocationInstance", "")).unwrap();
    let loc = d.place_text("Add capture location", &plain, graph, Float3::new(cx + 50.0, 0.0, cz), None, true).unwrap();
    let s = d.boundary_shape(d.capture_boundary(loc).unwrap()).unwrap();
    assert!(s.kind == BoundaryKind::Circle && (s.size - 20.0).abs() < 0.01, "{s:?}");
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
fn a_road_pieces_guides_show_on_the_prefab_stage_only() {
    use wce_scene::render_scene::RenderScene;
    use wce_tile::road_pieces::{base_guides, group_text};

    // A base piece placed in a tile, and the same piece as the Prefab tool's stage holds it.
    let piece = String::from_utf8(group_text("road_base_straight", &base_guides("straight"))).unwrap();
    let staged = piece.replacen("<UserInstanceGroup ", "<UserInstanceGroup wce_stage_root=\"true\" ", 1);
    let inside = format!("{piece}{staged}").lines().map(|l| format!("                        {l}
")).collect::<String>();
    let ted = TED.replacen("                    </children>
                </LayerInstance>", &format!("{inside}                    </children>
                </LayerInstance>"), 1);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("tile.ted"), &ted).unwrap();
    let mut d = TileDocument::new(TileProject::open(dir.path()).unwrap(), None);
    let check = |d: &TileDocument| {
        let t = d.tile();
        let guides = t.tscene().find_all(wce_tile::new_instances::ROAD_GUIDE);
        assert_eq!(guides.len(), 14);
        let (tile, stage) = guides.split_at(7);
        assert!(tile.iter().all(|&g| !t.shown(g)) && stage.iter().all(|&g| t.shown(g)), "in a tile hidden, on the stage shown");
        assert!(t.is_polygon(guides[0]));
    };

    check(&d);
    let mut scene = RenderScene::default();
    let layer = scene.add_layer(d.id(), "tile");
    d.publish(&mut scene, layer);
    check(&d);
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
    use wce_tile_editor::tile_document::{REF_HUMAN, REF_SHIP, REF_UNIT};

    let (_dir, mut d) = open();
    let layer = d.tile().find_layer("Buildings").unwrap();
    let attrs = std::collections::BTreeMap::from([("unit".to_owned(), "att_rom_sagittarii".to_owned())]);
    let human = d.create_with(REF_HUMAN, layer, &Create::Point(Float3::new(10.0, 0.0, 10.0)), "", &attrs).unwrap();
    let unit = d.create(REF_UNIT, layer, &Create::Point(Float3::new(30.0, 0.0, 10.0)), "").unwrap();
    let ship = d.create(REF_SHIP, layer, &Create::Point(Float3::new(60.0, 0.0, -40.0)), "").unwrap();
    let again = d.create(REF_HUMAN, layer, &Create::Point(Float3::new(20.0, 0.0, 10.0)), "").unwrap();

    // As many as wanted, and each an instance like any other.
    let mut scene = wce_scene::render_scene::RenderScene::default();
    let scene_layer = scene.add_layer(1, "tile");
    wce_editor_core::document::Document::republish(&mut d, &mut scene, scene_layer, &wce_editor_core::command::Dirty::default());
    let t = d.tile();
    assert_eq!(t.tscene().find_all(REF_HUMAN).len(), 2);
    assert!(t.is_instance(human) && t.is_instance(unit) && t.is_instance(ship) && t.is_instance(again) && !t.is_polygon(unit));
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
    assert!(report.dropped.iter().any(|d| d.contains(REF_UNIT)) && report.dropped.iter().any(|d| d.contains(REF_SHIP)), "{report:?}");

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

/// A prefab ground source with one prefab: `rocks`, a 2 high block of `n` x `n` samples `spacing` apart around its origin.
fn rocks_ground(normalize: bool, n: u32, spacing: f64) -> wce_tile::tile_project::PrefabGroundSource {
    let half = f64::from(n - 1) * spacing / 2.0;
    let mut t = wce_tile::prefab_terrain::PrefabTerrain::empty([-half, -half], spacing, n, n);
    t.heights.iter_mut().for_each(|h| *h = 2.0);
    t.coverage.iter_mut().for_each(|c| *c = 1.0);
    (t.normalize, t.feather) = (normalize, 0.0);
    let t = std::sync::Arc::new(t);
    let sig = t.signature();
    wce_tile::tile_project::PrefabGroundSource(std::sync::Arc::new(move |k: &str| (k == "rocks").then(|| (sig, t.clone()))))
}

/// The height map's value at a scene point.
fn ground_at(d: &TileDocument, x: f64, z: f64) -> f64 {
    let g = wce_tile::prefab_terrain::Grid::of(d.project()).unwrap();
    let (px, py) = (((x - g.origin[0]) / g.spacing[0]).round() as u32, ((z - g.origin[1]) / g.spacing[1]).round() as u32);
    d.project().height.as_ref().unwrap().metres(px, py)
}

fn prefab_owners(d: &TileDocument) -> Vec<String> {
    d.project().layers.as_ref().and_then(|l| l.height.as_ref()).map(|s| s.layers.iter().filter(|l| l.owner.starts_with("prefab:")).map(|l| l.owner.clone()).collect()).unwrap_or_default()
}

#[test]
fn a_prefab_reference_brings_its_ground_and_takes_it_along() {
    let (_dir, mut d) = open();
    let original = text(&d);
    let graph = d.tile().tscene().scene_graph().unwrap();
    let b = d.tile().tscene().find_all("BUILDING_INSTANCE")[0];
    let g = d.group_instances(&[b], graph, "rocks").unwrap();
    assert!(d.make_prefab_reference(g, "rocks"));
    d.set_prefab_ground(Some(rocks_ground(false, 5, 2.0)));
    let at = |d: &TileDocument| d.tile().world_position(g);
    let p = at(&d);
    let (x, z) = (f64::from(p.x) * d.project().unit_scale, f64::from(p.z) * d.project().unit_scale);
    let before = (ground_at(&d, x, z), ground_at(&d, x + 40.0, z + 40.0));

    d.refresh_terrain();
    assert!((ground_at(&d, x, z) - before.0 - 2.0).abs() < 0.01, "{} over {}", ground_at(&d, x, z), before.0);
    let owners = prefab_owners(&d);
    assert_eq!(owners.len(), 1, "{owners:?}");
    assert!(d.terrain_layers(wce_tile_editor::tile_document::layers::LayerType::Height).0.is_empty(), "not one of the user's layers");

    // Looked at again, nothing changes.
    d.refresh_terrain();
    assert_eq!(prefab_owners(&d), owners);

    // Moved, it takes its ground along.
    let local = d.tile().tscene().position(g).unwrap();
    assert!(d.set_local(g, Some(local + Double3::new(40.0, 0.0, 40.0)), None, 0));
    d.refresh_terrain();
    assert!((ground_at(&d, x, z) - before.0).abs() < 0.01 && (ground_at(&d, x + 40.0, z + 40.0) - before.1 - 2.0).abs() < 0.01);
    assert_ne!(prefab_owners(&d), owners);

    // Switched off, it brings none.
    assert!(d.set_attribute(g, wce_tile_editor::tile_document::prefab_ground::TERRAIN_ATTR, "off"));
    d.refresh_terrain();
    assert!(prefab_owners(&d).is_empty() && (ground_at(&d, x + 40.0, z + 40.0) - before.1).abs() < 0.01);

    while d.history_mut().can_undo() {
        d.history_mut().undo();
    }

    d.refresh_terrain();
    assert_eq!(text(&d), original);
    assert!(prefab_owners(&d).is_empty() && (ground_at(&d, x, z) - before.0).abs() < 0.01);
}

#[test]
fn a_normalized_prefab_is_level_wherever_it_stands() {
    let (_dir, mut d) = open();
    let graph = d.tile().tscene().scene_graph().unwrap();
    let b = d.tile().tscene().find_all("BUILDING_INSTANCE")[0];
    let g = d.group_instances(&[b], graph, "rocks").unwrap();
    assert!(d.make_prefab_reference(g, "rocks"));
    d.set_prefab_ground(Some(rocks_ground(true, 7, 8.0)));
    let p = d.tile().world_position(g);
    let (x, z) = (f64::from(p.x) * d.project().unit_scale, f64::from(p.z) * d.project().unit_scale);
    let spots = [(-16.0, -16.0), (16.0, -16.0), (0.0, 0.0), (-16.0, 16.0), (16.0, 16.0)];
    let was = spots.map(|(dx, dz)| ground_at(&d, x + dx, z + dz));
    d.refresh_terrain();

    // The test tile slopes; under the prefab it is level, 2 over the plain under its origin.
    let now = spots.map(|(dx, dz)| ground_at(&d, x + dx, z + dz));
    let (lo, hi) = was.iter().fold((f64::MAX, f64::MIN), |(a, b), &v| (a.min(v), b.max(v)));
    assert!(hi - lo > 1.0, "the ground slopes: {was:?}");
    assert!(now.iter().all(|v| (v - now[2]).abs() < 0.02), "level: {now:?}");
    assert!(now[2] - 2.0 > lo && now[2] - 2.0 < hi, "2 over the plain: {now:?} over {was:?}");
}

#[test]
fn an_unpacked_prefab_keeps_its_ground_as_a_layer_of_the_users() {
    let (_dir, mut d) = open();
    let original = text(&d);
    let graph = d.tile().tscene().scene_graph().unwrap();
    let b = d.tile().tscene().find_all("BUILDING_INSTANCE")[0];
    let g = d.group_instances(&[b], graph, "rocks").unwrap();
    assert!(d.make_prefab_reference(g, "rocks"));
    d.set_prefab_ground(Some(rocks_ground(false, 5, 2.0)));
    let p = d.tile().world_position(g);
    let (x, z) = (f64::from(p.x) * d.project().unit_scale, f64::from(p.z) * d.project().unit_scale);
    let before = ground_at(&d, x, z);
    d.refresh_terrain();
    let raised = ground_at(&d, x, z);
    assert!((raised - before - 2.0).abs() < 0.01);

    assert!(d.unpack_prefab_copy(g, "my rocks"));
    d.refresh_terrain();
    let height = wce_tile_editor::tile_document::layers::LayerType::Height;
    assert!(prefab_owners(&d).is_empty());
    assert_eq!(d.terrain_layers(height).0.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["rocks ground"]);
    assert!((ground_at(&d, x, z) - raised).abs() < 0.01, "the ground stays");

    // Undone one step: unpacked, without it; then the reference with its own again.
    d.history_mut().undo();
    d.refresh_terrain();
    assert!(d.terrain_layers(height).0.is_empty() && (ground_at(&d, x, z) - before).abs() < 0.01);
    while d.history_mut().can_undo() {
        d.history_mut().undo();
        d.refresh_terrain();
        if !prefab_owners(&d).is_empty() {
            assert!((ground_at(&d, x, z) - raised).abs() < 0.01);
        }
    }

    assert_eq!(text(&d), original);
}

#[test]
fn a_references_ground_is_in_its_inspector() {
    let (_dir, mut d) = open();
    let graph = d.tile().tscene().scene_graph().unwrap();
    let b = d.tile().tscene().find_all("BUILDING_INSTANCE")[0];
    let g = d.group_instances(&[b], graph, "rocks").unwrap();
    assert!(d.make_prefab_reference(g, "rocks"));
    d.set_prefab_ground(Some(rocks_ground(true, 5, 2.0)));
    let rows = inspect(&mut d, g, None);
    for f in ["ground_on", "text:ground_mode", "ground_opacity", "ground_feather"] {
        assert!(rows.iter().any(|r| r == f), "{f} in {rows:?}");
    }

    // Switched off from the Inspector: the attribute, which is not one of the other attributes.
    inspect(&mut d, g, Some(("ground_on", Edit::Bool(false))));
    assert_eq!(d.tile().tscene().doc().get(g, "wce_terrain").as_deref(), Some("off"));
    assert!(!inspect(&mut d, g, None).iter().any(|r| r == "text:attr_wce_terrain"));
}

#[test]
fn a_prefabs_layer_goes_onto_the_stage_and_back() {
    use wce_tile::prefab_stage::{StageSettings, make};
    use wce_tile::prefab_terrain::PrefabTerrain;

    let dir = tempfile::tempdir().unwrap();
    let group = b"<UserInstanceGroup flags=\" OIF_VISIBLE\" position=\"vector(0,0,0)\" name=\"rocks\" orientation=\"euler(0,0,0)\">\n    <children>\n        <BUILDING_INSTANCE position=\"vector(1,0,0)\" key=\"rock\" name=\"rock\"/>\n    </children>\n</UserInstanceGroup>\n";
    let s = StageSettings { cells: 1, density: 32, ..Default::default() };
    let mut d = TileDocument::new(make(&dir.path().join("stage"), &s, group).unwrap(), None);

    // The stage's pixels are 8 apart: a 5 x 5 layer at that spacing, raised on its +x half, its footprint all of it.
    let mut t = PrefabTerrain::empty([-16.0, -16.0], 8.0, 5, 5);
    for z in 0..5 {
        for x in 0..5 {
            t.coverage[z * 5 + x] = 1.0;
            t.heights[z * 5 + x] = if x >= 3 { 1.5 } else { 0.0 };
        }
    }

    (t.normalize, t.feather) = (true, 6.0);
    d.begin_stage(Some(&t)).unwrap();
    assert!(!d.modified(), "a stage starts unmodified");
    let back = d.stage_terrain(true, 6.0).unwrap();
    assert_eq!((back.origin, back.spacing, back.width, back.height), (t.origin, t.spacing, t.width, t.height), "{back:?}");
    assert!(back.heights.iter().zip(&t.heights).all(|(a, b)| (a - b).abs() < 1e-3), "{:?}", back.heights);
    assert_eq!(back.coverage, t.coverage);

    // An empty stage holds none.
    let mut empty = TileDocument::new(make(&dir.path().join("stage2"), &s, group).unwrap(), None);
    empty.begin_stage(None).unwrap();
    assert!(empty.stage_layer().is_some() && empty.stage_terrain(false, 4.0).is_none());
}

#[test]
fn on_a_stage_a_nested_prefabs_ground_goes_under_the_prefabs_own() {
    use wce_tile::prefab_stage::{StageSettings, make};

    let dir = tempfile::tempdir().unwrap();
    let group = b"<UserInstanceGroup flags=\" OIF_VISIBLE\" position=\"vector(0,0,0)\" name=\"camp\" orientation=\"euler(0,0,0)\">\n    <children>\n        <UserInstanceGroup flags=\" OIF_VISIBLE\" position=\"vector(0,0,0)\" name=\"rocks\" orientation=\"euler(0,0,0)\" wce_prefab=\"rocks\">\n            <children>\n                <BUILDING_INSTANCE position=\"vector(1,0,0)\" key=\"rock\" name=\"rock\"/>\n            </children>\n        </UserInstanceGroup>\n    </children>\n</UserInstanceGroup>\n";
    let s = StageSettings { cells: 1, density: 32, ..Default::default() };
    let mut d = TileDocument::new(make(&dir.path().join("stage"), &s, group).unwrap(), None);
    d.begin_stage(None).unwrap();
    d.set_prefab_ground(Some(rocks_ground(false, 5, 8.0)));
    d.refresh_terrain();
    let names = d.project().layers.as_ref().unwrap().height.as_ref().unwrap().layers.iter().map(|l| l.name.clone()).collect::<Vec<_>>();
    assert_eq!(names, ["Prefab rocks", wce_tile_editor::tile_document::stage::STAGE_LAYER], "{names:?}");

    // Looked at again, the order holds; what the stage saves is the prefab's own layer only.
    d.refresh_terrain();
    assert_eq!(prefab_owners(&d).len(), 1);
    assert!(d.stage_terrain(false, 4.0).is_none(), "the nested prefab's ground is not the prefab's own");
}

#[test]
fn a_references_ground_follows_its_opacity_while_it_is_dragged() {
    let (_dir, mut d) = open();
    let graph = d.tile().tscene().scene_graph().unwrap();
    let b = d.tile().tscene().find_all("BUILDING_INSTANCE")[0];
    let g = d.group_instances(&[b], graph, "rocks").unwrap();
    assert!(d.make_prefab_reference(g, "rocks"));
    d.set_prefab_ground(Some(rocks_ground(false, 5, 2.0)));
    let p = d.tile().world_position(g);
    let (x, z) = (f64::from(p.x) * d.project().unit_scale, f64::from(p.z) * d.project().unit_scale);
    let before = ground_at(&d, x, z);
    d.refresh_terrain();

    // Being dragged (a draft, no edit yet): half the ground.
    let edited = text(&d);
    inspect(&mut d, g, Some(("ground_opacity", Edit::Float(0.5))));
    d.refresh_terrain();
    assert!((ground_at(&d, x, z) - before - 1.0).abs() < 0.01, "{}", ground_at(&d, x, z) - before);
    assert_eq!(text(&d), edited, "not an edit while dragged");
}

#[test]
fn on_a_stage_what_is_on_the_landscape_stands_on_the_ground_and_saves_as_it_was() {
    use wce_tile::prefab_stage::{StageSettings, make};

    let dir = tempfile::tempdir().unwrap();
    let group = b"<UserInstanceGroup flags=\" OIF_VISIBLE\" position=\"vector(0,0,0)\" name=\"path\" orientation=\"euler(0,0,0)\">\n    <children>\n        <BUILDING_INSTANCE position=\"vector(4,2.08,0)\" key=\"border\" name=\"border\" surface_mode=\"MSM_LANDSCAPE\"/>\n        <BUILDING_INSTANCE position=\"vector(-4,2,0)\" key=\"post\" name=\"post\" surface_mode=\"MSM_FREEFORM\"/>\n    </children>\n</UserInstanceGroup>\n";
    let s = StageSettings { cells: 1, density: 32, ..Default::default() };
    let mut d = TileDocument::new(make(&dir.path().join("stage"), &s, group).unwrap(), None);
    let g = d.stage_group().unwrap();
    let file_children = d.children_text(g);
    d.begin_stage(None).unwrap();
    assert!(!d.modified());

    let find = |d: &TileDocument, name: &str| d.tile().tscene().find_all("BUILDING_INSTANCE").into_iter().find(|&e| d.tile().tscene().name(e) == name).unwrap();
    let world_y = |d: &TileDocument, e: usize| wce_tile::world_transform::world_transform(d.tile().tscene(), e).position.y;
    let (border, post) = (find(&d, "border"), find(&d, "post"));
    assert!((world_y(&d, border) - s.height).abs() < 1e-3, "on the ground: {}", world_y(&d, border));
    assert!((world_y(&d, post) - s.height - 2.0).abs() < 1e-9, "freeform keeps its height");

    // Saved untouched: the file's heights.
    assert_eq!(d.stage_children_text(g), file_children);

    // Moved: where it was moved to.
    let local = d.tile().tscene().position(border).unwrap();
    assert!(d.set_local(border, Some(local + Double3::new(1.0, 0.0, 0.0)), None, 0));
    let saved = String::from_utf8(d.stage_children_text(g)).unwrap();
    assert!(!saved.contains("vector(4,2.08,0)") && saved.contains("vector(-4,2,0)"), "{saved}");
}

#[test]
fn a_procedural_roads_points_have_handles_that_turn_it_and_go_with_them() {
    use wce_tile::splines::{read, written};

    let (_dir, mut d) = open();
    let original = text(&d);
    let road = d.add_polygon(None, wce_tile::new_instances::ROAD_SPLINE, "", &[Float3::new(10.0, 0.0, 10.0), Float3::new(30.0, 0.0, 10.0), Float3::new(50.0, 0.0, 20.0)]).unwrap();
    let new = text(&d);
    assert!(d.has_handles(road) && !d.is_spline(road), "handles, not TEd's spline panel");

    // None written: the Catmull-Rom ones, a sixth of the way from the point before to the one after.
    assert!(written(d.tile().tscene(), road).iter().all(|h| h.iter().all(Option::is_none)));
    let h = d.tile().spline_world_handles(road);
    let out = h[1].1.unwrap();
    assert!((out.x - (30.0 + 40.0 / 6.0)).abs() < 1e-3 && (out.z - (10.0 + 10.0 / 6.0)).abs() < 1e-3, "{out:?}");

    // A handle dragged: written, the other mirrored.
    let mut texts = vec![new.clone()];
    d.spline_point(road, "out", 1, Float3::new(30.0, 0.0, 20.0), 1).unwrap();
    texts.push(text(&d));
    let w = written(d.tile().tscene(), road);
    assert!(w[1][0].is_some() && w[1][1].is_some() && w[0][1].is_none(), "{w:?}");
    let turned = d.tile().spline_world_handles(road)[1];
    assert!((turned.0.unwrap().z - 0.0).abs() < 1e-3, "mirrored: {turned:?}");

    // The point moved: its handles go with it; a point inserted before it gets none written.
    let before = read(d.tile().tscene(), road)[1];
    d.polygon_point(road, "move", 1, Float3::new(32.0, 0.0, 12.0), 2).unwrap();
    texts.push(text(&d));
    let after = read(d.tile().tscene(), road)[1];
    let moved = after.control_out - before.control_out;
    assert!((moved - (after.position - before.position)).x.abs() < 1e-6 && (moved - (after.position - before.position)).z.abs() < 1e-6);
    d.polygon_point(road, "insert", 0, Float3::new(20.0, 0.0, 9.0), 3).unwrap();
    let w = written(d.tile().tscene(), road);
    assert_eq!(w.len(), 4);
    assert!(w[1].iter().all(Option::is_none) && w[2].iter().all(Option::is_some), "{w:?}");

    // Each undone in turn: the road as it was before it (the handle's first writing, its own undo step), then nothing.
    let names = ["the handle", "the move", "the insert"];
    for k in (0..texts.len()).rev() {
        d.history_mut().undo();
        if k == 0 {
            d.history_mut().undo();
        }

        assert!(text(&d) == texts[k], "undoing {}: {}", names[k], String::from_utf8_lossy(&text(&d)));
    }

    d.history_mut().undo();
    assert!(text(&d) == original, "undoing the road");
}
