//! Tests of the scene layer. `scene_dump` compares the environment and clouds with the C++ code over the game's files, and `half_sweep`
//! the half-float conversion over every float.

use wce_formats::xml_doc::XmlDoc;
use wce_foundation::math::{Float3, Float4x4, translation};
use wce_scene::clouds::{CloudField, cloud_texture, generate_clouds};
use wce_scene::environment::{blend_environments, build_scurve, colour_matrix, parse_graphics_options, read_environment};
use wce_scene::half::f32_to_f16;
use wce_scene::render_scene::{Kind, LfMap, MeshInstance, Overlay, Payload, RenderScene, SourceRef, StructureChange, Terrain, overlay_colour};

const ENVIRONMENT: &str = "<ENVIRONMENT>\
<LIGHTING sun_direction_azimuth='90' sun_direction_elevation='0' sun_colour='rgb(1,0.5,0.25)' use_custom_ambient_cube_colours='false'/>\
<SKYGEN sky_colour_scale='2.5'/>\
<FOG volume_fog_colour='argb(0.1,0.2,0.3,0.4)'/>\
<HDR low_tones_bias_red='0.2' high_tones_bias_blue='0.9'/>\
<DEPTH_OF_FIELD DOF_focus='variance(1,50)'/>\
<SKY_DOME><CLOUD_PLANE_MESH name='Far Clouds' grid_size='4' cloud_density='1' cloud_velocity='100' cloud_rotation='0.5'>\
<CLOUD probability='1'><CLOUD_ELEMENT enabled='true' scale='2' offset_x='0.5'/><CLOUD_ELEMENT enabled='false'/></CLOUD>\
</CLOUD_PLANE_MESH></SKY_DOME>\
<WEATHER definition_name='rain_heavy' wind_dir_random='true'/></ENVIRONMENT>";

#[test]
fn half_floats() {
    assert_eq!(f32_to_f16(1.0), 0x3C00);
    assert_eq!(f32_to_f16(-2.0), 0xC000);
    assert_eq!(f32_to_f16(65504.0), 0x7BFF);
    assert_eq!(f32_to_f16(65520.0), 0x7C00);
    assert_eq!(f32_to_f16(5.960_464_5e-8), 0x0001);
    assert_eq!(f32_to_f16(2.980_232_2e-8), 0x0000);
    assert_eq!(f32_to_f16(f32::NAN) & 0x7E00, 0x7E00);
    assert_eq!(f32_to_f16(1.0 + 1.0 / 2048.0), 0x3C00);
}

#[test]
fn environment_reading_and_mapping() {
    let e = read_environment(&XmlDoc::parse(ENVIRONMENT.as_bytes().to_vec()).unwrap());
    assert_eq!((e.sun_azimuth, e.sun_colour, e.use_custom_ambient), (90.0, Float3::new(1.0, 0.5, 0.25), false));
    assert!(e.has_sky && e.wind_dir_random);
    assert_eq!((e.sky_colour_scale, e.volume_fog_colour, e.dof_focus), (2.5, [0.1, 0.2, 0.3, 0.4], [1.0, 50.0]));
    assert_eq!((e.low_tones_bias[0], e.high_tones_bias[2], e.weather_definition.as_str()), (0.2, 0.9, "rain_heavy"));
    assert!((e.to_sun().x - 1.0).abs() < 1e-6);

    assert_eq!(colour_matrix(&Default::default(), 1.0), Float4x4::IDENTITY);
    let curve = build_scurve(&e);
    assert_eq!((curve[0], curve[255 * 4], curve[3]), (0, 0x3C00, 0));

    let mut b = e.clone();
    b.sun_azimuth = 350.0;
    b.fog_clear_distance = 0.0;
    let mid = blend_environments(&e, &b, 0.5);
    assert_eq!((mid.sun_azimuth, mid.fog_clear_distance), (40.0, 5000.0));
    assert_eq!(mid.weather_definition, b.weather_definition);

    let options = parse_graphics_options("gfx_gamma_setting 1.8;\r\ngfx_vignette false;\ngfx_shadow_quality 9;\nbroken\n");
    assert_eq!((options.gamma, options.vignette, options.shadow_quality, options.ssao), (1.8, false, 3, true));
}

#[test]
fn clouds() {
    let e = read_environment(&XmlDoc::parse(ENVIRONMENT.as_bytes().to_vec()).unwrap());
    let plane = &e.cloud_planes[0];
    assert!(plane.far_plane);
    assert_eq!(cloud_texture(plane, 1), "rigidmodels/skybox/cloudsets/cloudset_1/far2.dds");

    // Density 1: every cell of the (4 + 2)^2 grid gets a cloud of one enabled element.
    let clouds = generate_clouds(plane, 5);
    assert_eq!(clouds.len(), 36);
    assert_eq!(clouds, generate_clouds(plane, 5));
    assert_ne!(clouds, generate_clouds(plane, 6));

    // cloud_velocity 100 drifts a cloud 10 rad/s (cloud_velocity * 0.1, +-2.5%), not 100.
    let mut field = CloudField::default();
    field.reset(plane, 5);
    let before = field.instances()[0].t2[0];
    field.update(plane, 0.001);
    let moved = field.instances()[0].t2[0] - before;
    assert!((0.00975..=0.01025).contains(&moved), "{moved}");

    let mut field = CloudField::default();
    field.reset(plane, 5);
    for _ in 0..100 {
        field.update(plane, 1.0);
    }

    assert!(!field.instances().is_empty());
}

#[test]
fn render_scene() {
    let mut scene = RenderScene::default();
    let layer = scene.add_layer(7, "doc");
    let source = SourceRef { doc: 7, item: 3 };
    let mesh = scene.add(layer, Float4x4::IDENTITY, Payload::MeshInstance(MeshInstance::default()), source);
    let terrain = scene.add(layer, Float4x4::IDENTITY, Payload::Terrain(Terrain::flat(10.0, "grass.dds", 4.0)), SourceRef::default());
    let overlay = scene.add(layer, Float4x4::IDENTITY, Payload::Overlay(Overlay::default()), source);
    assert_eq!(scene.entities_of(source), [mesh, overlay]);
    assert_eq!(scene.find(terrain).map(|e| e.kind()), Some(Kind::Terrain));

    let (revision, structure, terrain_rev) = (scene.revision(), scene.structure_revision(), scene.terrain_revision());
    let mut cursor = 0;
    assert_eq!(scene.moved_since(&mut cursor), Some(vec![]));
    scene.update_world(mesh, translation(Float3::new(1.0, 0.0, 0.0)));
    scene.update_payload(terrain, Payload::Terrain(Terrain::default()));
    scene.update_payload(overlay, Payload::Overlay(Overlay::default()));
    assert_eq!((scene.revision(), scene.structure_revision(), scene.terrain_revision()), (revision + 3, structure, terrain_rev + 1));
    assert_eq!(scene.moved_since(&mut cursor), Some(vec![mesh]));
    assert_eq!(scene.moved_since(&mut cursor), Some(vec![]));
    assert_eq!(scene.moved_since(&mut 99), None);

    scene.clear_layer_except(layer, terrain);
    assert_eq!(scene.entities().len(), 1);
    assert!(scene.find(mesh).is_none() && scene.find(terrain).is_some());

    let entity = scene.find(terrain).unwrap().clone();
    assert!(scene.drawable(&entity) && scene.pickable(&entity));
    scene.layer_mut(layer).unwrap().locked = true;
    assert!(scene.drawable(&entity) && !scene.pickable(&entity));
    scene.set_layer_visible(layer, false);
    assert!(!scene.drawable(&entity));

    scene.remove_layer(layer);
    assert!(scene.entities().is_empty() && scene.layers().is_empty());
    assert_eq!(overlay_colour(0.25, 0.5, 1.2, 1.0), 0xFFFF_8040);

    let mut lines = Overlay::default();
    lines.box_wire(Float3::default(), Float3::new(1.0, 1.0, 1.0), 0);
    lines.polygon_outline(&[Float3::default(), Float3::new(1.0, 0.0, 0.0), Float3::new(0.0, 0.0, 1.0)], true, 0);
    assert_eq!(lines.lines.len(), 24 + 6);
}

#[test]
fn structure_log() {
    let mut scene = RenderScene::default();
    let layer = scene.add_layer(1, "context");
    let mut cursor = 0;
    let _ = scene.structure_since(&mut cursor);
    let ids: Vec<_> = (0..6).map(|_| scene.add(layer, Float4x4::IDENTITY, Payload::MeshInstance(MeshInstance::default()), SourceRef::default())).collect();

    // Additions are not logged (their ids follow the last one seen); removals by ranges are, one pass for all.
    assert_eq!(scene.structure_since(&mut cursor), Some(vec![]));
    scene.remove_ranges(&[ids[4]..ids[5] + 1, ids[0]..ids[2]]);
    assert_eq!(scene.entities().iter().map(|e| e.id).collect::<Vec<_>>(), [ids[2], ids[3]]);
    assert!(scene.find(ids[3]).is_some() && scene.find(ids[0]).is_none());
    assert_eq!(scene.structure_since(&mut cursor), Some(vec![StructureChange::Removed(ids[0]..ids[2]), StructureChange::Removed(ids[4]..ids[5] + 1)]));
    scene.remove_range(ids[3]..ids[4]);
    assert_eq!(scene.structure_since(&mut cursor), Some(vec![StructureChange::Removed(ids[3]..ids[4])]));

    // A payload of another kind, and anything else.
    scene.update_payload(ids[2], Payload::Overlay(Overlay::default()));
    scene.set_layer_visible(layer, false);
    assert_eq!(scene.structure_since(&mut cursor), Some(vec![StructureChange::Payload(ids[2]), StructureChange::Other]));
    assert_eq!(scene.structure_since(&mut 999), None);
}

#[test]
fn lf_map_and_ground() {
    // 3 x 2 samples 10 apart from (100, 200): values 0, 65535 ... scaled by 30, less 5.
    let lf = LfMap { x0: 100.0, z0: 200.0, step: 10.0, nx: 3, nz: 2, values: vec![0, 65535, 0, 65535, 65535, 65535], scale: 30.0, offset: 5.0, ..Default::default() };
    assert!((lf.height_at(100.0, 200.0) + 5.0).abs() < 1e-4);
    assert!((lf.height_at(105.0, 200.0) - 10.0).abs() < 1e-3);
    assert!((lf.height_at(105.0, 205.0) - 17.5).abs() < 1e-3);
    assert!((lf.height_at(-1e6, 200.0) + 5.0).abs() < 1e-4, "clamped");
    assert_eq!(lf.range(100.0, 200.0, 101.0, 201.0), (-5.0, 25.0));

    // A 2 x 2 grid (heights 0, 1 / 2, 3) turned a quarter about y and moved: world (x, z) = (z + 50, -x + 60).
    let t = Terrain { nx: 2, nz: 2, origin_x: 0.0, origin_z: 0.0, step: 10.0, heights: std::sync::Arc::new(vec![0.0, 1.0, 2.0, 3.0]), ..Default::default() };
    let mut w = Float4x4::IDENTITY;
    w.m[0] = [0.0, 0.0, 1.0, 50.0];
    w.m[2] = [-1.0, 0.0, 0.0, 60.0];
    // Local (10, 0) is world (50, 50): height 1; local (0, 10) is world (60, 60): height 2.
    assert!((t.ground_at(&w, 50.0, 50.0).unwrap() - 1.0).abs() < 1e-5);
    assert!((t.ground_at(&w, 60.0, 60.0).unwrap() - 2.0).abs() < 1e-5);
    assert!(t.ground_at(&w, 0.0, 0.0).is_none());
    let with_lf = Terrain { lf: Some(std::sync::Arc::new(lf)), ..t };
    assert!((with_lf.ground_at(&w, 50.0, 50.0).unwrap() - (1.0 - 5.0)).abs() < 1e-4, "the LF clamped to its first sample");
}
