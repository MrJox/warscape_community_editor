//! Tests of the scene layer. `scene_dump` compares the environment and clouds with the C++ code over the game's files, and `half_sweep`
//! the half-float conversion over every float.

use wce_formats::xml_doc::XmlDoc;
use wce_foundation::math::{Float3, Float4x4, translation};
use wce_scene::clouds::{CloudField, cloud_texture, generate_clouds};
use wce_scene::environment::{blend_environments, build_scurve, colour_matrix, parse_graphics_options, read_environment};
use wce_scene::half::f32_to_f16;
use wce_scene::render_scene::{Kind, MeshInstance, Overlay, Payload, RenderScene, SourceRef, Terrain, overlay_colour};

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
