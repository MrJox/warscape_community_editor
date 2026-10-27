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
