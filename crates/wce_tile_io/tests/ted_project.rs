//! The Attila TEd project through the model and back, on tiles made here in the shapes of the kit's (versions 1 to 16, every map kind, a
//! river, an entry, Photoshop-style images, a broken file, files carried along). The kit's own tiles: `kit_samples.rs` and the corpus gate
//! `tile_roundtrip_check`.

use std::collections::BTreeMap;
use std::sync::Arc;

use wce_tile_io::attila::TedProject;
use wce_tile_io::attila::images::{read_png, write_png, write_tiff};
use wce_tile_io::{PartRead, PartWritten, TileFiles, TileFormat, format, formats};
use wce_tile_model::{Blob, MapKind, Node, Raster, Samples, Sym, Tile, Value};

const V16: &str = concat!(
    "<project version=\"16\">\r\n",
    "    <modules>\r\n",
    "        <Vegetation decal_generation_seed=\"54016492\" generation_seed=\"188233471\" vfx_generation_seed=\"54016492\"/>\r\n",
    "        <Grass generation_seed=\"6815828\"/>\r\n",
    "    </modules>\r\n",
    "    <scene>\r\n",
    "        <SCENE_GRAPH flags=\" OIF_VISIBLE|OIF_ACTIVE\" export_type=\"LayerExportType_Normal\" lf_offset=\"0\" position=\"vector(0,0,0)\" name=\"root\" surface_mode=\"MSM_FREEFORM\" orientation=\"euler(0,0,0)\">\r\n",
    "            <children>\r\n",
    "                <LayerInstance flags=\" OIF_VISIBLE|OIF_ACTIVE\" export_type=\"LayerExportType_Normal\" lf_offset=\"0\" position=\"vector(0,0,0)\" name=\"Buildings &amp; walls\" surface_mode=\"MSM_FREEFORM\" orientation=\"euler(0,0,0)\">\r\n",
    "                    <children>\r\n",
    "                        <BUILDING_INSTANCE flags=\" OIF_VISIBLE|OIF_ACTIVE|OIF_DRAGGABLE\" opacity=\"1\" building_tech_level=\"TECH_LEVEL_1\" bounds_colour=\"argb(1.0,0.74902,0.545098,1.0)\" indestructible=\"false\" lf_offset=\"-1.7191e-05\" glow_amount=\"0.85\" position=\"vector(6451.527832,133.062256,4916.137207)\" key=\"roman_villa_01\" dirt_level=\"1\" destruction_level=\"2\" name=\"roman_villa_01\" toggleable=\"false\" burn=\"false\" burn_amount=\"1\" surface_mode=\"MSM_LANDSCAPE\" orientation=\"euler(1.353718,3.141593,-3.141593)\"/>\r\n",
    "                        <UserInstanceGroup flags=\" OIF_VISIBLE|OIF_ACTIVE\" low_frequency_change_effect=\"LFCE_NONE\" lf_offset=\"nan\" position=\"vector(10,0,-4)\" name=\"Instance Group\" surface_mode=\"MSM_FREEFORM\" orientation=\"euler(0,0,0)\">\r\n",
    "                            <children>\r\n",
    "                                <VEGETATION_INSTANCE flags=\" \" lf_offset=\"1.27781\" position=\"vector(5,0,5)\" key=\"tree_oak\" name=\"tree\" scale=\"1.5\" surface_mode=\"MSM_LANDSCAPE\" orientation=\"euler(0,0,0)\"/>\r\n",
    "                                <ProjectedDecalInstance flags=\" OIF_VISIBLE\" lf_offset=\"0\" position=\"vector(1,2,3)\" key=\"decal\" name=\"d\" scale=\"vector(24.000002,24.000002,-24)\" parallax_scale=\"0.1\" tiled=\"true\" surface_mode=\"MSM_LANDSCAPE\" orientation=\"euler(0,0,0)\"/>\r\n",
    "                                <VFXInstance flags=\" OIF_VISIBLE|OIF_ACTIVE\" vfx=\"fire_small\" autoplay=\"true\" use_in_outfield=\"false\" scale=\"20\" seasons=\" SEASON_BF_WINTER|SEASON_BF_SPRING|SEASON_BF_SUMMER|SEASON_BF_AUTUMN\" lf_offset=\"0\" position=\"vector(0,0,0)\" name=\"fx\" surface_mode=\"MSM_FREEFORM\" orientation=\"euler(0,0,0)\"/>\r\n",
    "                                <POINT_LIGHT_INSTANCE flags=\" OIF_VISIBLE|OIF_ACTIVE\" colour=\"argb(1.0,1.0,0.8,1.0)\" colour_scale=\"0.8\" radius=\"2.044122\" animation_type=\"LA_NONE\" animation_speed_scale=\"vector(1,2.5)\" colour_min=\"0\" random_offset=\"0\" falloff_type=\"LF_LINEAR\" lf_offset=\"0\" position=\"vector(0,1,0)\" name=\"light\" surface_mode=\"MSM_FREEFORM\" orientation=\"euler(0,0,0)\"/>\r\n",
    "                            </children>\r\n",
    "                        </UserInstanceGroup>\r\n",
    "                    </children>\r\n",
    "                </LayerInstance>\r\n",
    "                <LayerInstance flags=\" OIF_ACTIVE\" export_type=\"LayerExportType_Escalation1\" lf_offset=\"0\" position=\"vector(0,0,0)\" name=\"Logic\" surface_mode=\"MSM_FREEFORM\" orientation=\"euler(0,0,0)\">\r\n",
    "                    <children>\r\n",
    "                        <AI_HINT_POLYGON_INSTANCE flags=\" OIF_VISIBLE|OIF_ACTIVE|OIF_DRAGGABLE\" recenter_polygon=\"0\" lf_offset=\"0\" line_colour=\"argb(1.0,1.0,1.0,1.0)\" polygon_closed=\"1\" position=\"vector(3107.091309,240.921814,2449.395752)\" selected_line_colour=\"argb(1.0,0.0,0.0,1.0)\" area=\"1786.786499\" hint_type=\"AIHT_DEFENSIVE\" generated=\"false\" is_generated=\"false\" subdivision_length=\"5\" max_subdivisions=\"100\" polygon_shape=\"PS_POLYGON\" name=\"hint\" surface_mode=\"MSM_LANDSCAPE\" orientation=\"euler(0,0,0)\">\r\n",
    "                            <points>\r\n",
    "                                <point position=\"421.93,-83.0674,-339.355\"/>\r\n",
    "                                <point position=\"-6.86646e-05,0,288.074\"/>\r\n",
    "                                <point position=\"288.074,19.1262,0.00390625\"/>\r\n",
    "                            </points>\r\n",
    "                        </AI_HINT_POLYGON_INSTANCE>\r\n",
    "                        <SplineRiverInstance flags=\" OIF_VISIBLE|OIF_ACTIVE\" step_size=\"10\" select_all_points=\"0\" closed=\"0\" lf_offset=\"-7.63961\" position=\"vector(0,0,0)\" name=\"river\" surface_mode=\"MSM_FREEFORM\" orientation=\"euler(0,0,0)\">\r\n",
    "                            <points>\r\n",
    "                                <point width=\"90\" foam_amount=\"0.1\" alpha_fade=\"1\" flow_speed=\"0.1\" terrain_offset=\"4.114203\" controlIn=\"-69.1123,0.0300741,10.3463\" controlOut=\"-135.557,0.0300741,33.9842\" position=\"1,2,3\"/>\r\n",
    "                            </points>\r\n",
    "                        </SplineRiverInstance>\r\n",
    "                    </children>\r\n",
    "                </LayerInstance>\r\n",
    "            </children>\r\n",
    "        </SCENE_GRAPH>\r\n",
    "    </scene>\r\n",
    "    <camera vertical_fov=\"1.047198\" orthographic=\"false\" orthographic_height=\"1499.866455\" far_distance=\"12000\" smoothed=\"true\" target=\"vector(5453.069824,597.57959,1107.249634)\" ground_height=\"1.4\" DOF_blur_size=\"0\" DOF_focus=\"variance(0,40)\" position=\"vector(5470.440918,610.820984,1059.838379)\"/>\r\n",
    "</project>\r\n",
);

/// The shape of the oldest files: no layers, `exportable`, a self-closed SCENE_GRAPH, LF line ends.
const V8: &str = "<project version=\"8\">\n    <modules>\n        <Vegetation generation_seed=\"185417209\"/>\n    </modules>\n    <scene>\n        <SCENE_GRAPH lf_offset=\"0\" exportable=\"true\" name=\"root\"/>\n    </scene>\n    <camera vertical_fov=\"1.047198\" orthographic=\"false\" target=\"vector(290.51236,572.297974,-192.261292)\"/>\n</project>\n";

/// The one v1 file's root layer group (flags without TEd's leading space: kept as text).
const V1: &str = "<project version=\"1\">\r\n    <scene>\r\n        <LAYER_INSTANCE_GROUP flags=\"OIF_VISIBLE|OIF_ACTIVE\" name=\"Base layer\" position=\"vector(0,0,0)\" season_mask=\"SEASON_BF_WINTER|SEASON_BF_SPRING\" surface_mode=\"MSM_FREEFORM\" orientation=\"euler(0,0,0)\"/>\r\n    </scene>\r\n</project>\r\n";

const RIVER: &str = "<SEGMENTED_SPLINE>\r\n    <SPLINE>\r\n        <POINT0 width=\"45\" x=\"261.169\" y=\"-6.11169\" foam_amount=\"0.1\" z=\"63.9035\" alpha_fade=\"1\" flow_speed=\"0.1\"/>\r\n        <POINT1 width=\"57.5\" x=\"243.539\" y=\"-7\" foam_amount=\"1\" z=\"64.0212\" alpha_fade=\"0.99\" flow_speed=\"0\"/>\r\n    </SPLINE>\r\n</SEGMENTED_SPLINE>\r\n";

const ENTRY: &str = "\u{feff}<TILE serialise_version='3' location='terrain\\tiles\\battle\\test\\2x1' tile_set='test' mask='' width='2' height='1' red='0.000000' green='0.500000' blue='1.000000' requires_infield_lodding='true' random_rotatable='true' custom_alpha_blend_texture='' scalable='false' encampable='false' custom_blend_tile=''>\r\n\t<TEXTURE_GROUP red='grass' green='forest' blue='mud' alpha='sand'/>\r\n\t<VARIATIONS>\r\n\t\t<VARIATION serialise_version='2' location='aa' min_height='-5.000000' scale='24.550000' normal_strength='2.000000' overlap_border_size='1.000000' raw_data_tri_density='128' blend_common='' index_common='' normal_common='' red='0.000000' green='0.000000' blue='0.000000' barbarian='false'/>\r\n\t</VARIATIONS>\r\n\t<TILE_LINK_TARGETS>\r\n\t\t<TILE_LINK_TARGET serialise_version='1' tile_set='river' x='0' y='1'/>\r\n\t</TILE_LINK_TARGETS>\r\n\t<TILE_LINKS/>\r\n</TILE>\r\n";

fn height(w: u32, h: u32) -> Raster {
    Raster { width: w, height: h, channels: 1, samples: Samples::U16((0..w * h).map(|i| (i.wrapping_mul(2654435761) >> 16) as u16).collect()) }
}

fn png(r: &Raster) -> Vec<u8> {
    write_png(r, None).unwrap().0
}

/// A PNG with an ancillary chunk after IHDR, as Photoshop writes them.
fn photoshop_png(r: &Raster) -> Vec<u8> {
    let plain = png(r);
    let ihdr_end = 8 + 12 + 13;
    let mut chunk = vec![0, 0, 0, 9];
    chunk.extend_from_slice(b"pHYs");
    chunk.extend_from_slice(&[0, 0, 0x0b, 0x13, 0, 0, 0x0b, 0x13, 1]);
    chunk.extend_from_slice(&[0x00, 0x9a, 0x9c, 0x18]);
    [&plain[..ihdr_end], &chunk, &plain[ihdr_end..]].concat()
}

fn files(ted: &str) -> TileFiles {
    let blend = Raster { width: 4, height: 2, channels: 16, samples: Samples::U16((0..128).map(|i| i * 511).collect()) };
    let ground = Raster { width: 4, height: 2, channels: 4, samples: Samples::U8((0..32).map(|i| i * 7).collect()) };
    let mut folder = BTreeMap::new();
    let mut put = |name: &str, bytes: Vec<u8>| {
        folder.insert(name.to_owned(), Blob::Bytes(bytes.into()));
    };

    put("tile.ted", ted.as_bytes().to_vec());
    put("height_map_0.png", png(&height(12, 8)));
    put("blend_map.tif", write_tiff(&blend, None).unwrap());
    put("ground_type_map.png", photoshop_png(&ground));
    put("nogo.png", png(&Raster { width: 4, height: 2, channels: 1, samples: Samples::U16(vec![0; 8]) }));
    put("lf_influence_map.png", b"\x89PNG\r\n\x1a\r\n broken by a CRLF conversion".to_vec());
    put("river_spline0.xml", RIVER.as_bytes().to_vec());
    put("final_heights.dds", b"DDS |not read".to_vec());
    put("raw/sculpt.png", b"artist's file".to_vec());
    TileFiles { folder, entry: Some(("test_2x1.xml".to_owned(), Blob::Bytes(ENTRY.as_bytes().into()))), variation: Some("aa".to_owned()) }
}

fn bytes(b: &Blob) -> Arc<[u8]> {
    b.bytes().unwrap()
}

/// Read, write, compare every file; read the written files again: the same model.
fn round_trip(input: &TileFiles) -> (Tile, TileFiles) {
    let (tile, report) = TedProject.read(input).unwrap();
    let (out, _) = TedProject.write(&tile).unwrap();
    assert_eq!(out.folder.keys().collect::<Vec<_>>(), input.folder.keys().collect::<Vec<_>>());
    for (name, blob) in &input.folder {
        assert_eq!(bytes(&out.folder[name]), bytes(blob), "{name}");
    }

    assert_eq!(out.entry.as_ref().map(|e| bytes(&e.1)), input.entry.as_ref().map(|e| bytes(&e.1)));
    let (again, _) = TedProject.read(&out).unwrap();
    assert!(again.semantic() == tile.semantic());
    assert!(report.xml.unknown.is_empty(), "{:?}", report.xml.unknown);
    (tile, out)
}

#[test]
fn every_version_shape_round_trips() {
    for ted in [V16, V8, V1] {
        round_trip(&files(ted));
    }
}

#[test]
fn what_the_model_holds() {
    let (tile, report) = TedProject.read(&files(V16)).unwrap();
    assert_eq!(tile.source_format, Some("attila.ted_project"));
    assert_eq!(tile.variation.as_deref(), Some("aa"));
    assert_eq!(tile.maps.iter().map(|m| m.kind).collect::<Vec<_>>(), [MapKind::Height, MapKind::Blend, MapKind::GroundType, MapKind::Nogo]);
    assert_eq!(tile.rivers.len(), 1);
    assert_eq!(tile.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["final_heights.dds", "lf_influence_map.png", "raw/sculpt.png"]);
    assert!(matches!(report.parts.iter().find(|p| p.0 == "lf_influence_map.png").unwrap().1, PartRead::Failed(_)));

    let scene = tile.scene.as_ref().unwrap();
    let graph = &scene.child("scene").unwrap().children[0];
    let layer = &graph.wrapped("children")[0];
    assert_eq!(layer.get("name"), Some(&Value::Str("Buildings & walls".into())));
    let building = &layer.wrapped("children")[0];
    assert_eq!(building.class, "BUILDING_INSTANCE");
    assert_eq!(building.get("position").and_then(Value::as_vec3).map(|v| v.map(|x| x as f32)), Some([6451.528, 133.06226, 4916.137]));
    assert!(building.get("flags").unwrap().has_flag("OIF_DRAGGABLE"));
    assert_eq!(building.get("destruction_level"), Some(&Value::Int(2)));
    assert_eq!(building.get("burn"), Some(&Value::Bool(false)));
    assert!(matches!(building.get("bounds_colour"), Some(Value::Colour(_))));
    let group = &layer.wrapped("children")[1];
    assert!(group.get("lf_offset").and_then(Value::as_f64).unwrap().is_nan());
    assert_eq!(group.wrapped("children")[0].get("scale"), Some(&Value::Float(1.5)));
    assert!(matches!(group.wrapped("children")[1].get("scale"), Some(Value::Vec3(_))));
    let hint = &graph.wrapped("children")[1].wrapped("children")[0];
    assert_eq!(hint.wrapped("points").len(), 3);
    assert!(matches!(hint.wrapped("points")[0].get("position"), Some(Value::Vec3(_))));

    let entry = tile.entry.as_ref().unwrap();
    assert_eq!(entry.root.get("width"), Some(&Value::Int(2)));
    assert_eq!(entry.root.get("green"), Some(&Value::Float(0.5)));
    assert_eq!(entry.root.child("TEXTURE_GROUP").unwrap().get("red"), Some(&Value::Str("grass".into())));
    assert_eq!(tile.rivers[0].root.children[0].children[1].get("width"), Some(&Value::Float(57.5)));
}

#[test]
fn values_teds_writer_would_not_give_back_keep_their_text() {
    let (tile, report) = TedProject.read(&files(V1)).unwrap();
    assert_eq!(report.xml.verbatim.values().sum::<usize>(), 1);
    let group = &tile.scene.as_ref().unwrap().child("scene").unwrap().children[0];
    assert_eq!(group.verbatim(Sym::new("flags")), Some("OIF_VISIBLE|OIF_ACTIVE"));
    assert!(group.get("flags").unwrap().has_flag("OIF_ACTIVE"));
}

/// The node of the building in the V16 scene.
fn building(tile: &mut Tile) -> &mut Node {
    let scene = tile.scene.as_mut().unwrap();
    let graph = &mut scene.child_mut("scene").unwrap().children[0];
    &mut graph.wrapped_mut("children")[0].wrapped_mut("children")[0]
}

#[test]
fn an_edit_changes_only_its_value() {
    let input = files(V16);
    let (mut tile, _) = TedProject.read(&input).unwrap();
    building(&mut tile).set("position", Value::Vec3([100.0, 2.5, -3.25]));
    let (out, report) = TedProject.write(&tile).unwrap();
    let (old, new) = (bytes(&input.folder["tile.ted"]), bytes(&out.folder["tile.ted"]));
    let old = String::from_utf8_lossy(&old);
    let new = String::from_utf8_lossy(&new);
    assert_eq!(new, old.replace("vector(6451.527832,133.062256,4916.137207)", "vector(100,2.5,-3.25)"));
    assert_eq!(report.parts.iter().find(|p| p.0 == "ground_type_map.png").unwrap().1, PartWritten::Kept);
    let (back, _) = TedProject.read(&out).unwrap();
    let mut back = back;
    assert_eq!(building(&mut back).get("position"), Some(&Value::Vec3([100.0, 2.5, -3.25])));
}

#[test]
fn a_new_node_is_written_in_teds_style() {
    let (mut tile, _) = TedProject.read(&files(V16)).unwrap();
    let scene = tile.scene.as_mut().unwrap();
    let graph = &mut scene.child_mut("scene").unwrap().children[0];
    let mut b = Node::new("BUILDING_INSTANCE");
    b.set("flags", Value::Flags(vec![Sym::new("OIF_VISIBLE"), Sym::new("OIF_ACTIVE")]));
    b.set("key", Value::Str("hut".into()));
    b.set("position", Value::Vec3([1.0, 0.0, -2.5]));
    b.set("orientation", Value::Euler([0.0, 0.0, std::f64::consts::PI]));
    graph.wrapped_mut("children")[1].wrapped_mut("children").push(b);
    let (out, _) = TedProject.write(&tile).unwrap();
    let text = String::from_utf8(bytes(&out.folder["tile.ted"]).to_vec()).unwrap();
    let line = "                        <BUILDING_INSTANCE flags=\" OIF_VISIBLE|OIF_ACTIVE\" key=\"hut\" position=\"vector(1,0,-2.5)\" orientation=\"euler(0,0,3.141593)\"/>\r\n                    </children>";
    assert!(text.contains(line), "{text}");
}

#[test]
fn edited_pixels_are_encoded_the_kits_way() {
    let input = files(V16);
    let (mut tile, _) = TedProject.read(&input).unwrap();
    for kind in [MapKind::Height, MapKind::GroundType] {
        match &mut tile.map_mut(kind, 0).unwrap().raster.samples {
            Samples::U16(v) => v[5] = 12345,
            Samples::U8(v) => v[5] = 200,
            Samples::F32(_) => unreachable!(),
        }
    }

    let (out, report) = TedProject.write(&tile).unwrap();
    assert_eq!(report.parts.iter().find(|p| p.0 == "ground_type_map.png").unwrap().1, PartWritten::Encoded);
    let (raster, trivia) = read_png(&bytes(&out.folder["ground_type_map.png"])).unwrap();
    assert_eq!(&raster, &tile.map(MapKind::GroundType, 0).unwrap().raster);
    assert!(matches!(trivia, wce_tile_io::attila::images::ImageTrivia::Png { kept: None, .. }));
    let (back, _) = TedProject.read(&out).unwrap();
    assert_eq!(back.map(MapKind::Height, 0), tile.map(MapKind::Height, 0));
}

#[test]
fn a_tile_from_nowhere_is_written_in_teds_style() {
    let (read, _) = TedProject.read(&files(V16)).unwrap();
    let tile = Tile { scene: read.scene.clone(), maps: read.maps.clone(), entry: read.entry.clone(), ..Default::default() };
    let (out, _) = TedProject.write(&tile).unwrap();
    assert_eq!(bytes(&out.folder["tile.ted"]), Arc::from(V16.as_bytes()));
    assert_eq!(bytes(&out.folder["height_map_0.png"]), bytes(&files(V16).folder["height_map_0.png"]));
    let entry = String::from_utf8(bytes(&out.entry.unwrap().1).to_vec()).unwrap();
    assert_eq!(entry, ENTRY.trim_start_matches('\u{feff}'));
}

#[test]
fn registry() {
    assert!(formats().iter().any(|f| f.id() == "attila.ted_project"));
    let f = format("attila.ted_project").unwrap();
    assert!(f.claims(&files(V8)));
    assert!(!f.claims(&TileFiles::default()));
}
