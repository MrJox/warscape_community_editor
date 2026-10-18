//! Tests of the vfx definitions and weather effects on small files. `assets_dump` compares them with the C++ code over the game's files.

use std::collections::BTreeMap;

use wce_assets::vfx_constants::{VFX_CURVE_WIDTH, VFX_DISTORTION_VIEW_ID, VFX_MAIN_VIEW_ID, build_emitter_constants, vfx_bits, vfx_texture_path};
use wce_assets::vfx_def::{parse_curves, parse_vfx};
use wce_assets::weather_vfx::load_weather_vfx;
use wce_formats::xml_doc::XmlDoc;

const LIBRARY: &str = r#"<vfx camera_following="true">
  <emitters>
    <emitter id="drops" type="continuous_mesh">
      <property name="GravityAmount" quality="1" value="9"/>
      <ranged_property name="GravityAmount" min="-9.8" max="-12x"/>
      <property name="RecycleClippedBox" x="20" y="30"/>
      <property name="RecycleClipped" value="true"/>
      <property name="ParticleScaleStart" x="2" y="4"/>
      <property name="ParticleScaleUseSingleScale" value="1"/>
    </emitter>
    <emitter id="sound" type="continuous_mesh"/>
    <emitter id="burst" type="burst_rigid"/>
  </emitters>
  <behaviour_chains>
    <behaviour_chain id="render">
      <modifier fragment="ps_texture"><texture_property name="diffuse" path="Rain_Drop.dds"/></modifier>
      <modifier fragment="vs_vertex_alpha"><property name="curve_alpha">alpha_min,1;1.0,0.5;0.0,0.0|alpha_max,2;0.0,1.0</property></modifier>
      <modifier fragment="vs_cell_texture"><property name="atlas_number_of_cells_x" value="4"/><property name="atlas_cell" value="6"/></modifier>
    </behaviour_chain>
  </behaviour_chains>
  <effect_render_stacks>
    <effect_render_stack id="main">
      <emitters_selected><e id="drops"/><e id="sound"/><e id="burst"/></emitters_selected>
      <behaviour_chains_selected><c id="render"/></behaviour_chains_selected>
      <render_params scene_view_type="1" cast_shadows="true"/>
    </effect_render_stack>
  </effect_render_stacks>
</vfx>"#;

const TOP: &str = r#"<vfx/><vfx_references>
  <vfx inst_name="rain" vfx_ref="library" scale_min="1" scale_max="2" pos_y="5">
    <emitter name="drops" tint_min_r="0.5" particle_lifetime_bias_max="0.25"/>
    <emitter name="sound" enable="false"/>
  </vfx>
  <vfx inst_name="broken" vfx_ref="missing_library"/>
</vfx_references>"#;

const DEFINITIONS: &str = r#"<root><WEATHER_DEFINITION name="none"/><WEATHER_DEFINITION name="rain" type="WT_RAIN" vfx1="top" severity="2.7" wind_size_max="3"/></root>"#;

fn doc(text: &str) -> XmlDoc {
    XmlDoc::parse(text.as_bytes().to_vec()).unwrap()
}

#[test]
fn curves() {
    let curves = parse_curves("a,1;1.0,2.0;0.0,1.0;bad|b,2;0.5,4||");
    assert_eq!(curves.len(), 2);
    let a = &curves["a"];
    assert_eq!(a.keys, [(0.0, 1.0), (1.0, 2.0)]);
    assert_eq!((a.eval(-1.0), a.eval(0.5), a.eval(2.0)), (1.0, 1.5, 2.0));
    assert_eq!(curves["b"].eval(0.0), 4.0);
}

#[test]
fn library_definitions_and_constants() {
    let f = parse_vfx(&doc(LIBRARY));
    assert!(f.camera_following());
    let drops = f.emitter("drops").unwrap();

    // The base quality wins over the override listed before it; numbers read as C's atof.
    assert_eq!(drops.range("GravityAmount", 0.0, 0.0), (-9.8, -12.0));
    assert_eq!(f.stacks[0].emitters, ["drops", "sound", "burst"]);
    assert_eq!((f.stacks[0].scene_view_type, f.stacks[0].cast_shadows), (1, true));

    let k = build_emitter_constants(drops, f.chain("render"));
    assert_eq!(k.ps_sim_following_emitter_params, [3.0, 20.0, 30.0, 10.0]);
    assert_eq!(k.diffuse, "vfx/textures/rain_drop.dds");
    assert_eq!(k.behaviour_mask, [vfx_bits::PS_TEXTURE | vfx_bits::VERTEX_ALPHA | vfx_bits::CELL_TEXTURE, 0, 0, VFX_MAIN_VIEW_ID]);
    assert_eq!(k.vs_atlas_texture_params, [0.25, 1.0, 0.5, 1.0]);
    assert_eq!((k.curves[0][0][1], k.curves[1][VFX_CURVE_WIDTH - 1][0]), (2.0, 1.0));
    assert_eq!((k.curves[6][VFX_CURVE_WIDTH - 1][0], k.curves[7][0][3]), (0.5, 1.0));
    assert_eq!(vfx_texture_path("A\\B.dds"), "a/b.dds");
}

#[test]
fn weather_effects() {
    let files = BTreeMap::from([("weather/weather_definitions.xml", DEFINITIONS), ("vfx/top.xml", TOP), ("vfx/library.xml", LIBRARY)]);
    let read = |path: &str| files.get(path).map(|text| text.as_bytes().to_vec()).ok_or_else(|| format!("missing {path}"));

    let w = load_weather_vfx(&read, "rain").unwrap();
    assert_eq!((w.definition.type_index(), w.definition.severity, w.definition.wind_size_max, w.definition.wind_gust_curve), (1, 2, 3.0, 4.0));
    assert_eq!(w.emitters.len(), 1);
    let drops = &w.emitters[0];
    assert_eq!((drops.instance.as_str(), drops.tint_min[0], drops.particle_lifetime_bias, drops.scale, drops.pos), ("rain", 0.5, [0.0, 0.25], 1.5, [0.0, 5.0, 0.0]));
    assert!(drops.camera_following);
    assert_eq!(drops.k.behaviour_mask[3], VFX_DISTORTION_VIEW_ID);
    assert_eq!(w.notes, ["rain/burst: emitter type burst_rigid not supported (rigid model '')", "vfx/missing_library.xml: missing vfx/missing_library.xml"]);

    assert!(load_weather_vfx(&read, "").unwrap().emitters.is_empty());
    assert_eq!(load_weather_vfx(&read, "nope").unwrap().notes.len(), 1);
    assert!(load_weather_vfx(&|_: &str| Err("no packs".to_owned()), "rain").is_err());
}
