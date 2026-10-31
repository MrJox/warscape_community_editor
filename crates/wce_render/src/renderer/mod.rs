//! The battle renderer, built from the game's own shaders. Pass order and shader choices follow the game; anything not yet confirmed
//! against it is marked ASSUMPTION.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::sync::Arc;

use wce_d3d11::*;
use wce_foundation::math::{Float3, Float4x4, inverse, look_at_lh, translation};
use wce_foundation::wce_prof;
use wce_scene::camera::Camera;
use wce_scene::environment::{EnvironmentParams, GraphicsOptions};
use wce_scene::render_scene::{EntityId, RenderScene};
use windows_core::Interface;

use crate::RenderError;
use crate::cbuffers::{ConstantBuffers, Var};
use crate::environment_apply::apply_environment;
use crate::files::{Files, shared_reader};
use crate::gpu_timer::GpuTimer;
use crate::model_cache::{MeshKind, ModelCache, RigidInstance, TEX_SLOTS, TextureCache};
use crate::particles::{Frame, ParticleSystem};
use crate::render_target::RenderTarget;
use crate::shader_lib::{FileReader, GameShader, ShaderLibrary, bind_shader};
use crate::wind::WindSim;

mod decals;
mod lights;
mod grass;
mod draws;
mod editor;
pub mod game;
pub use self::editor::{SurfaceGrid, SurfaceHit};
mod passes;
mod post;
mod shadow_fit;
mod shared;
mod sky;
mod states;
mod targets;
mod blend_mesh;
mod terrain;
mod terrain_custom;
mod terrain_rome2;
mod water;
mod ocean;
mod nis;
mod nis_coef;

pub(crate) use draws::SceneDraws;
pub(crate) use post::{DepthRange, ToneMap};
pub(crate) use shared::SharedResources;
pub(crate) use sky::{CloudLayer, EnvCube, SkyCube, SunGpu};
pub(crate) use states::States;
pub(crate) use targets::{GBuffer, PostTargets, ShadowMap, Target, depth_target, make_target};

// Shader choices: the highest-quality dx11 variants.
/// The global snow normals every shader with snow reads (s_snow_normals): the weather node loads them (fn 0x118ce490).
pub(crate) const SNOW_NORMALS: &str = "commontextures/snow_normal.dds";
pub(crate) const SKY_VS: &str = "dx11_sm_3_0_skydome.hlsl_vs_vs30_main";
pub(crate) const SKY_GEN_PS: &str = "dx11_sm_3_0_skydome.hlsl_ps_ps30_skygen";
pub(crate) const SKY_PS: &str = "dx11_sm_3_0_skydome.hlsl_ps_ps30_main";
pub(crate) const RIGID_VS: &str = "dx11_sm_4_0_rigidcommon.hlsl_vs_vs30_main";
pub(crate) const RIGID_GBUFFER_PS: &str = "dx11_sm_3_0_rigidstandard.hlsl_ps_ps30_gbuffer";
pub(crate) const RIGID_GBUFFER_ALPHA_PS: &str = "dx11_sm_3_0_rigidstandard.hlsl_ps_ps30_gbuffer_alpha_test";
pub(crate) const TILED_DIRT_GBUFFER_PS: &str = "dx11_sm_3_0_rigidtileddirtmapmesh.hlsl_ps_ps30_gbuffer";
pub(crate) const TILED_DIRT_GBUFFER_ALPHA_PS: &str = "dx11_sm_3_0_rigidtileddirtmapmesh.hlsl_ps_ps30_gbuffer_alpha_test";
pub(crate) const DIRT_GBUFFER_PS: &str = "dx11_sm_3_0_rigidwithdirtmesh.hlsl_ps_ps30_gbuffer";
pub(crate) const DIRT_GBUFFER_ALPHA_PS: &str = "dx11_sm_3_0_rigidwithdirtmesh.hlsl_ps_ps30_gbuffer_alpha_test";
pub(crate) const AMBIENT_MAP_GBUFFER_PS: &str = "dx11_sm_3_0_rigidambientmapmesh.hlsl_ps_ps30_gbuffer";
pub(crate) const AMBIENT_MAP_GBUFFER_ALPHA_PS: &str = "dx11_sm_3_0_rigidambientmapmesh.hlsl_ps_ps30_gbuffer_alpha_test";
pub(crate) const TREE_BRANCH_VS: &str = "dx11_sm_3_0_rigidbranchcommon_v3.hlsl_vs_vs30_main";
pub(crate) const TREE_BRANCH_PS: &str = "dx11_sm_3_0_rigidbranch_v3.hlsl_ps_ps30_gbuffer";
pub(crate) const TREE_LEAF_VS: &str = "dx11_sm_3_0_rigidleafcommon_v3.hlsl_vs_vs30_main";
pub(crate) const TREE_LEAF_PS: &str = "dx11_sm_3_0_rigidleaf_v3.hlsl_ps_ps30_gbuffer";
pub(crate) const TREE_BILLBOARD_VS: &str = "dx11_sm_3_0_rigidtreebillboard.hlsl_vs_vs30_main";
pub(crate) const TREE_BILLBOARD_PS: &str = "dx11_sm_3_0_rigidtreebillboard.hlsl_ps_ps30_gbuffer";
pub(crate) const TERRAIN_BLEND_VS: &str = "dx11_sm_3_0_rigidterrainblend.hlsl_vs_vs30_main";

/// The TerrainBlend meshes with their own textures. The game draws them with ps30_null_gbuffer instead, from the terrain's virtual texture
/// (the ground's colour under the building), which the editor doesn't build.
pub(crate) const TERRAIN_BLEND_GBUFFER_PS: &str = "dx11_sm_3_0_rigidterrainblend.hlsl_ps_ps30_gbuffer";

pub(crate) const FIRE_GBUFFER_PS: &str = "dx11_sm_3_0_rigidstandardfire.hlsl_ps_ps30_gbuffer";
pub(crate) const FIRE_GBUFFER_ALPHA_PS: &str = "dx11_sm_3_0_rigidstandardfire.hlsl_ps_ps30_gbuffer_alpha_test";
pub(crate) const TILED_DIRT_FIRE_GBUFFER_PS: &str = "dx11_sm_3_0_rigidtileddirtmapfiremesh.hlsl_ps_ps30_gbuffer";
pub(crate) const TILED_DIRT_FIRE_GBUFFER_ALPHA_PS: &str = "dx11_sm_3_0_rigidtileddirtmapfiremesh.hlsl_ps_ps30_gbuffer_alpha_test";
pub(crate) const DIRT_BURN_GBUFFER_PS: &str = "dx11_sm_3_0_rigidwithdirtburnmesh.hlsl_ps_ps30_gbuffer";
pub(crate) const DIRT_BURN_GBUFFER_ALPHA_PS: &str = "dx11_sm_3_0_rigidwithdirtburnmesh.hlsl_ps_ps30_gbuffer_alpha_test";
pub(crate) const AMBIENT_MAP_FIRE_GBUFFER_PS: &str = "dx11_sm_3_0_rigidambientmapfiremesh.hlsl_ps_ps30_gbuffer";
pub(crate) const AMBIENT_MAP_FIRE_GBUFFER_ALPHA_PS: &str = "dx11_sm_3_0_rigidambientmapfiremesh.hlsl_ps_ps30_gbuffer_alpha_test";

/// The texture the engine binds as `s_fire_map` of the rigid materials (empire.retail.dll sets it up with the blood map); sampled
/// linear (its samplers have no sRGB).
pub(crate) const FIRE_MAP: &str = "commontextures/vfx_fire_diffuse.dds";

/// The G-buffer sub-passes, in drawing order: the material they draw, alpha-tested or not, burning or not, vertex and pixel shader, GPU
/// timer label. rigidtileddirtmapmesh and rigidambientmapmesh have no vertex shader of their own: rigidcommon's. A burning instance (the
/// engine's render flag 0x200: a building on fire, or TEd's `burn`) draws with its material's fire variant, which chars the colour by
/// POSITION6.x (the burn amount) with embers from `s_fire_map`.
pub(crate) const GBUFFER_PASSES: [(MeshKind, bool, bool, &str, &str, &str); 20] = [
    (MeshKind::Standard, false, false, RIGID_VS, RIGID_GBUFFER_PS, "gbuffer opaque"),
    (MeshKind::Standard, true, false, RIGID_VS, RIGID_GBUFFER_ALPHA_PS, "gbuffer alpha-tested"),
    (MeshKind::TiledDirtmap, false, false, RIGID_VS, TILED_DIRT_GBUFFER_PS, "gbuffer tiled dirt"),
    (MeshKind::TiledDirtmap, true, false, RIGID_VS, TILED_DIRT_GBUFFER_ALPHA_PS, "gbuffer tiled dirt alpha-tested"),
    (MeshKind::Dirtmap, false, false, RIGID_VS, DIRT_GBUFFER_PS, "gbuffer dirt"),
    (MeshKind::Dirtmap, true, false, RIGID_VS, DIRT_GBUFFER_ALPHA_PS, "gbuffer dirt alpha-tested"),
    (MeshKind::AmbientMap, false, false, RIGID_VS, AMBIENT_MAP_GBUFFER_PS, "gbuffer ambient map"),
    (MeshKind::AmbientMap, true, false, RIGID_VS, AMBIENT_MAP_GBUFFER_ALPHA_PS, "gbuffer ambient map alpha-tested"),
    (MeshKind::Standard, false, true, RIGID_VS, FIRE_GBUFFER_PS, "gbuffer burning"),
    (MeshKind::Standard, true, true, RIGID_VS, FIRE_GBUFFER_ALPHA_PS, "gbuffer burning alpha-tested"),
    (MeshKind::TiledDirtmap, false, true, RIGID_VS, TILED_DIRT_FIRE_GBUFFER_PS, "gbuffer burning tiled dirt"),
    (MeshKind::TiledDirtmap, true, true, RIGID_VS, TILED_DIRT_FIRE_GBUFFER_ALPHA_PS, "gbuffer burning tiled dirt alpha-tested"),
    (MeshKind::Dirtmap, false, true, RIGID_VS, DIRT_BURN_GBUFFER_PS, "gbuffer burning dirt"),
    (MeshKind::Dirtmap, true, true, RIGID_VS, DIRT_BURN_GBUFFER_ALPHA_PS, "gbuffer burning dirt alpha-tested"),
    (MeshKind::AmbientMap, false, true, RIGID_VS, AMBIENT_MAP_FIRE_GBUFFER_PS, "gbuffer burning ambient map"),
    (MeshKind::AmbientMap, true, true, RIGID_VS, AMBIENT_MAP_FIRE_GBUFFER_ALPHA_PS, "gbuffer burning ambient map alpha-tested"),
    (MeshKind::TreeBranch, false, false, TREE_BRANCH_VS, TREE_BRANCH_PS, "gbuffer tree branches"),
    (MeshKind::TreeLeaf, false, false, TREE_LEAF_VS, TREE_LEAF_PS, "gbuffer tree leaves"),
    (MeshKind::TreeBillboard, false, false, TREE_BILLBOARD_VS, TREE_BILLBOARD_PS, "gbuffer tree billboards"),
    (MeshKind::TerrainBlend, false, false, TERRAIN_BLEND_VS, TERRAIN_BLEND_GBUFFER_PS, "gbuffer terrain blend"),
];

/// The blended forward shaders of the faded instances (opacity below 1: the engine's render flag 0x2000, docs/re/buildings.md), per
/// material: lit and fogged in one pass, the diffuse alpha times the opacity (POSITION4.w) as the alpha.
pub(crate) const FADED_PASSES: [(MeshKind, &str); 4] = [
    (MeshKind::Standard, "dx11_enable_shadows_shadows_hardware_gather_5_0_sm_5_0_rigidstandard.hlsl_ps_ps30_alpha_blend_40"),
    (MeshKind::TiledDirtmap, "dx11_enable_shadows_shadows_hardware_gather_5_0_sm_5_0_rigidtileddirtmapmesh.hlsl_ps_ps30_alpha_blend_40"),
    (MeshKind::Dirtmap, "dx11_enable_shadows_shadows_hardware_gather_5_0_sm_5_0_rigidwithdirtmesh.hlsl_ps_ps30_alpha_blend_40"),
    (MeshKind::AmbientMap, "dx11_enable_shadows_shadows_hardware_gather_5_0_sm_5_0_rigidambientmapmesh.hlsl_ps_ps30_alpha_blend_40"),
];

pub(crate) const DIR_LIGHT_PS: &str = "dx11_enable_shadows_shadows_hardware_gather_5_0_sm_5_0_directionallight.hlsl_ps_ps30_slm_highdef";

/// The environment specular pass (DEFERRED_ENVIRONMENT_SPECULAR_SHADER, full resolution; the game runs their MSAA variants) and its blurs.
pub(crate) const ENV_SPECULAR_PS: &str = "dx11_enable_shadows_shadows_hardware_gather_5_0_sm_5_0_environment_specular_light.hlsl_ps_ps40_standard_lighting_model_full_res";
pub(crate) const REFLECTION_BLUR_X_PS: &str = "dx11_sm_4_0_reflections_blur.hlsl_ps_ps40_main_blur_x";
pub(crate) const REFLECTION_BLUR_Y_PS: &str = "dx11_sm_4_0_reflections_blur.hlsl_ps_ps40_main_blur_y_fullres";

/// gfx_screen_space_reflections 1: the half resolution entries (the same code as the full resolution ones, at half the size).
pub(crate) const ENV_SPECULAR_HALF_PS: &str = "dx11_enable_shadows_shadows_hardware_gather_5_0_sm_5_0_environment_specular_light.hlsl_ps_ps40_standard_lighting_model";
pub(crate) const REFLECTION_BLUR_Y_HALF_PS: &str = "dx11_sm_4_0_reflections_blur.hlsl_ps_ps40_main_blur_y";
pub(crate) const TM_DOWN_LUMA: &str = "dx11_sm_3_0_hdr_tone_mapper.hlsl_ps_ps_downsample_image_store_luma";
pub(crate) const TM_LUMA_STATS: &str = "dx11_sm_3_0_hdr_tone_mapper.hlsl_ps_ps_hdr_luma_to_quarter_size_luma_stats";
pub(crate) const TM_STATS_DOWN: &str = "dx11_sm_3_0_hdr_tone_mapper.hlsl_ps_ps_downsample_quarter_size_luma_stats";
pub(crate) const TM_POINTS: &str = "dx11_sm_3_0_hdr_tone_mapper.hlsl_ps_ps_determine_black_and_white_points";
pub(crate) const TM_HIGH_PASS: &str = "dx11_sm_3_0_hdr_tone_mapper.hlsl_ps_ps_high_pass_luminance_cutoff";
pub(crate) const TM_BLUR_X: &str = "dx11_sm_3_0_hdr_tone_mapper.hlsl_ps_ps_blur_x";
pub(crate) const TM_BLUR_Y: &str = "dx11_sm_3_0_hdr_tone_mapper.hlsl_ps_ps_blur_y";

/// The game's live path (dump: constant_tone_mapping_buffer = 0); linear out.
/// Rome 2's luminance statistics: in log Y (CIE log Y xy), from a quarter-size image.
pub(crate) const TM_R2_DOWN: &str = "dx11_sm_3_0_hdr_tone_mapper.hlsl_ps_ps_downsample_image";
pub(crate) const TM_R2_LOG_Y: &str = "dx11_sm_3_0_hdr_tone_mapper.hlsl_ps_ps_hdr_rgb_to_hdr_cie_log_y_xy";
pub(crate) const TM_R2_STATS: &str = "dx11_sm_3_0_hdr_tone_mapper.hlsl_ps_ps_hdr_cie_log_y_xy_to_quarter_size_log_y_stats";
pub(crate) const TM_R2_STATS_DOWN: &str = "dx11_sm_3_0_hdr_tone_mapper.hlsl_ps_ps_downsample_quarter_size_log_y_stats";
pub(crate) const TM_TONE_MAP: &str = "dx11_sm_3_0_hdr_tone_mapper.hlsl_ps_ps_basic_tone_mapper";
/// Thrones of Britannia's cloud shadows (fn 0x117c3680): the texture, cloud_shade_battle_scale and cloud_shade_battle_speed.
pub(crate) const CLOUD_SHADE_TEXTURE: &str = "terrain/textures/common/cloud_shade.dds";
pub(crate) const CLOUD_SHADE_BATTLE_SCALE: f32 = 0.0005;
pub(crate) const CLOUD_SHADE_BATTLE_SPEED: f32 = 50.0;

/// Thrones of Britannia's film grain texture (fn 0x117d7da0).
pub(crate) const GRAIN_TEXTURE: &str = "commontextures/grain.dds";

/// The film grain's state: the generator, the time since its last move, its offset (`hdr_to_screen_PS.grain_tex_offset`).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Grain {
    pub seed: u32,
    pub since: f32,
    pub offset: [f32; 2],
}

pub(crate) const TO_SCREEN_PS: &str = "dx11_sm_3_0_hdr_to_screen.hlsl_ps_ps30_main_high";

/// The same shader compiled with ENABLE_DISTORTION: uv += (distortion_sampler.xy - 0.498) * 0.02 (heat haze, lens drops).
pub(crate) const TO_SCREEN_DISTORT_PS: &str = "dx11_enable_distortion_sm_3_0_hdr_to_screen.hlsl_ps_ps30_main_high";

/// ENABLE_DEPTH_OF_FIELD variants: a 25-tap blur (sample_data_5x5) scaled by 1 - exp(-(z - min(focus, 500))^2 / focal^2), result =
/// lerp(frame, blurred, blur_kernel_scale).
pub(crate) const TO_SCREEN_DOF_PS: &str = "dx11_enable_depth_of_field_sm_3_0_hdr_to_screen.hlsl_ps_ps30_main_high";
pub(crate) const TO_SCREEN_DOF_DISTORT_PS: &str = "dx11_enable_distortion_enable_depth_of_field_sm_3_0_hdr_to_screen.hlsl_ps_ps30_main_high";
pub(crate) const FOG_PS: &str = "dx11_sm_3_0_battlefog.hlsl_ps_ps30_main";

/// Stencil bit 7: the pixels projected decals may paint. The game's G-buffer writes 129 (bit 7 | 1) for the ground and the TerrainBlend
/// meshes (its rocks) and 1 for every other model (RenderDoc, coast.rdc: events 307, 692, 808 against 552 .. 776); a decal's box paints
/// the ground in it, not the tents, walls or tables standing there. The other stencil tests leave the bit out (`STENCIL_LOW`).
pub(crate) const DECAL_RECEIVER: u8 = 0x80;

/// The stencil bits below the decal bit: what the lights, the fog and the terrain's resolve compare.
pub(crate) const STENCIL_LOW: u8 = 0x7F;

/// The terrain surface's and the TerrainBlend meshes' stencil: 2 (the terrain pixels the blend boxes and the resolve take), and decals.
pub(crate) const TERRAIN_STENCIL: u32 = 2 | DECAL_RECEIVER as u32;
pub(crate) const SHADOW_VS: &str = "dx11_sm_3_0_rigidcommon.hlsl_vs_vs30_shadow";
pub(crate) const SUN_VS: &str = "dx11_sm_3_0_sun.hlsl_vs_vs30_main";
pub(crate) const SUN_PS: &str = "dx11_sm_3_0_sun.hlsl_ps_ps30_main";
pub(crate) const SSAO_PS: &str = "dx11_sm_3_0_ssao.hlsl_ps_pixel_resolve";
pub(crate) const SSAO_BLUR_X_PS: &str = "dx11_sm_3_0_ssao.hlsl_ps_pixel_blur_x";
pub(crate) const SSAO_BLUR_Y_PS: &str = "dx11_sm_3_0_ssao.hlsl_ps_pixel_blur_y";
pub(crate) const CLOUD_VS: &str = "dx11_sm_3_0_cloud_plane.hlsl_vs_vs30_main";
pub(crate) const CLOUD_PS: &str = "dx11_sm_3_0_cloud_plane.hlsl_ps_ps30_main";
pub(crate) const SHADOW_PS: &str = "dx11_sm_3_0_rigidcommon.hlsl_ps_ps30_shadow";

/// The tree casters: their vertex shaders (wind, LOD blend) and alpha-tested pixel shaders (the leaves' also dissolve).
pub(crate) const TREE_SHADOW_PASSES: [(MeshKind, &str, &str); 3] = [
    (MeshKind::TreeBranch, "dx11_sm_3_0_rigidbranchcommon_v3.hlsl_vs_vs30_shadow", "dx11_sm_3_0_rigidbranchcommon_v3.hlsl_ps_ps30_shadow"),
    (MeshKind::TreeLeaf, "dx11_sm_3_0_rigidleafcommon_v3.hlsl_vs_vs30_shadow", "dx11_sm_3_0_rigidleafcommon_v3.hlsl_ps_ps30_shadow"),
    (MeshKind::TreeBillboard, "dx11_sm_3_0_rigidtreebillboard.hlsl_vs_vs30_shadow", "dx11_sm_3_0_rigidtreebillboard.hlsl_ps_ps30_shadow"),
];

/// Memory dump: sun_VS.g_world_view translation = view rotation * to_sun * 7600.
pub(crate) const SUN_DISTANCE: f32 = 7600.0;

/// The sun mesh's half-size (sun object +0x80 = 2000 in the dry dump; sun_VS: corner = v.xy * g_sun_scale). Cross-checked with the occlusion
/// total of fn 0x1927460: int(k / w * 960) * int(k / w * 540 * aspect) with k = 0.1 * 2000 and view depth 431.6 gives the dumped 153664.
pub(crate) const SUN_MESH_SIZE: f32 = 2000.0;

/// The sky and environment cubes.
pub(crate) const SKY_CUBE: u32 = 256;

/// The cascaded shadow atlas: 2048^2, four 1024^2 quadrants (the layout of the game's g_split_uv_texture_bounds).
pub(crate) const SHADOW_SIZE: u32 = 2048;
pub(crate) const READBACK_RING: usize = 4;

/// Compiles a helper shader.
pub(crate) fn compile(source: &str, name: &str, entry: &str, target: &str) -> Result<Vec<u8>, RenderError> {
    wce_d3d11::safe::compile(source, name, entry, target).map_err(|message| RenderError(format!("{name} shader: {message}")))
}

/// An input element.
pub(crate) fn element(semantic: &'static std::ffi::CStr, index: u32, format: DXGI_FORMAT, slot: u32, offset: u32, instanced: bool) -> InputElement {
    InputElement::new(semantic, index, format, slot, offset, instanced)
}

/// skydome.hlsl vs30_main declares POSITION0 (xyz) and an unused TEXCOORD0 (xy); it is aliased onto the position.
pub(crate) fn sky_layout() -> Vec<InputElement> {
    vec![element(c"POSITION", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 0, false), element(c"TEXCOORD", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 0, false)]
}

/// STANDARD_RIGID_MESH_VERTEX (32 bytes) + 7 float4 per instance. Byte vectors are BGRA ("X/Z swapped").
pub(crate) fn rigid_layout() -> Vec<InputElement> {
    let mut el = vec![
        element(c"POSITION", 0, DXGI_FORMAT_R16G16B16A16_FLOAT, 0, 0, false),
        element(c"TEXCOORD", 0, DXGI_FORMAT_R16G16B16A16_FLOAT, 0, 8, false),
        element(c"NORMAL", 0, DXGI_FORMAT_B8G8R8A8_UNORM, 0, 16, false),
        element(c"NORMAL", 1, DXGI_FORMAT_B8G8R8A8_UNORM, 0, 20, false),
        element(c"NORMAL", 2, DXGI_FORMAT_B8G8R8A8_UNORM, 0, 24, false),
        element(c"COLOR", 0, DXGI_FORMAT_B8G8R8A8_UNORM, 0, 28, false),
    ];

    for i in 1..=7 {
        el.push(element(c"POSITION", i, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, (i - 1) * 16, true));
    }

    el
}

/// The inputs of the tree vertex shaders (rigidbranchcommon_v3, rigidleafcommon_v3), from their code: the Tree vertex
/// ([`crate::model_cache::TREE_STRIDE`] bytes: POSITION0 position, 1 LOD position, 2..4 normal / tangent / bitangent, 5 uv, 6 the two LOD
/// weights, 7 colour and wind weight; bytes 48..52 unused) and the instance (TEXCOORD0..2 the world rows for the position, tilted to the
/// terrain; TEXCOORD3..5 the untilted rows for POSITION1, from the rigid instance's POSITION5..7 (the draw set's gather puts them there);
/// TEXCOORD6 (tint and wind seed, 1 - dissolve, burn) = the rigid instance's POSITION4).
/// Rome 2's shaders read the same values in another order: TEXCOORD3 (.xy: tint and wind seed, 1 - dissolve; no burn), TEXCOORD4..6 the
/// LOD position's rows.
pub(crate) fn tree_layout(game: game::RenderGame) -> Vec<InputElement> {
    let h4 = DXGI_FORMAT_R16G16B16A16_FLOAT;
    let mut el = vec![
        element(c"POSITION", 0, h4, 0, 0, false),
        element(c"POSITION", 1, h4, 0, 8, false),
        element(c"POSITION", 2, h4, 0, 16, false),
        element(c"POSITION", 3, h4, 0, 24, false),
        element(c"POSITION", 4, h4, 0, 32, false),
        element(c"POSITION", 5, DXGI_FORMAT_R16G16_FLOAT, 0, 40, false),
        element(c"POSITION", 6, DXGI_FORMAT_R16G16_FLOAT, 0, 44, false),
        element(c"POSITION", 7, h4, 0, 52, false),
    ];

    // The semantic indices in order: the position's rows, then (Attila) the LOD position's rows and the values, (Rome 2) the values and the
    // LOD position's rows.
    let rome2 = game == game::RenderGame::Rome2;
    for i in 0..7 {
        let offset = match (rome2, i) {
            (_, 0..=2) => i * 16,
            (false, 3..=5) | (true, 4..=6) => 64 + (i - if rome2 { 4 } else { 3 }) * 16,
            _ => 48,
        };
        el.push(element(c"TEXCOORD", i, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, offset, true));
    }

    el
}

/// The inputs of rigidtreebillboard's vertex shaders: the Billboard vertex ([`crate::model_cache::BILLBOARD_STRIDE`] bytes: POSITION0
/// the corner, TEXCOORD0 the uv) and the instance's POSITION5 .. 7 (filled by the draw set's gather: TEXCOORD1 position and 1 - dissolve,
/// TEXCOORD2 facing, tint seed and size, TEXCOORD3 burn).
pub(crate) fn billboard_layout() -> Vec<InputElement> {
    vec![
        element(c"POSITION", 0, DXGI_FORMAT_R16G16B16A16_FLOAT, 0, 0, false),
        element(c"TEXCOORD", 0, DXGI_FORMAT_R16G16_FLOAT, 0, 8, false),
        element(c"TEXCOORD", 1, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 64, true),
        element(c"TEXCOORD", 2, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 80, true),
        element(c"TEXCOORD", 3, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 96, true),
    ]
}

/// The view of a cube face.
pub(crate) fn face_view(face: usize) -> Float4x4 {
    let dirs = [Float3::new(1.0, 0.0, 0.0), Float3::new(-1.0, 0.0, 0.0), Float3::new(0.0, 1.0, 0.0), Float3::new(0.0, -1.0, 0.0), Float3::new(0.0, 0.0, 1.0), Float3::new(0.0, 0.0, -1.0)];
    let ups = [Float3::new(0.0, 1.0, 0.0), Float3::new(0.0, 1.0, 0.0), Float3::new(0.0, 0.0, -1.0), Float3::new(0.0, 0.0, 1.0), Float3::new(0.0, 1.0, 0.0), Float3::new(0.0, 1.0, 0.0)];
    look_at_lh(Float3::default(), dirs[face], ups[face])
}

/// A viewport.
pub(crate) fn viewport(x: f32, y: f32, w: f32, h: f32) -> D3D11_VIEWPORT {
    D3D11_VIEWPORT { TopLeftX: x, TopLeftY: y, Width: w, Height: h, MinDepth: 0.0, MaxDepth: 1.0 }
}

/// What one frame needs besides the scene: the camera and the time (from the clock).
#[derive(Clone, Copy, Debug, Default)]
pub struct ViewParams {
    pub camera: Camera,
    pub time_s: f32,
}

/// What the renderer shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum View {
    #[default]
    Final,
    HdrRaw,
    Albedo,
    Normal,
    Specular,
    Depth,
    SkyCube,
    Bloom,
    Quarter,
    ToneMapped,
    Ao,
}

/// What the last frame submitted (all views: camera, shadow cascades, environment cube).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrawStats {
    pub items: usize,
    pub batches: usize,
    pub models: usize,
    pub waiting_models: usize,
    pub views: u64,
    pub instances: u64,
    pub draw_calls: u64,
    pub triangles: u64,
    pub camera_instances: u64,
    pub camera_triangles: u64,
    pub shadow_instances: u64,
    pub shadow_triangles: u64,
    pub terrain_patches: u64,
}

/// What the renderer draws and how: the editor's graphics options, overlays and the battle state the weather shaders read.
#[derive(Clone, Debug)]
pub struct RenderSettings {
    /// Multiplies every LOD distance (the editor can trade detail for speed).
    pub lod_scale: f32,

    /// LOD items smaller than this many pixels (radius) are not drawn.
    pub min_pixels: f32,

    /// LOD items smaller than this many shadow-map texels (radius) cast no shadow.
    pub shadow_min_texels: f32,

    /// Fixed-step runs (screenshots, regression, recording): GPU readbacks wait for their data instead of using it a frame later when it
    /// happens to be ready, so frame N is the same every run. Set together with a fixed-step clock.
    pub deterministic: bool,
    pub fog: bool,
    pub shadows: bool,
    pub clouds: bool,
    pub ssao: bool,
    pub lens_flare: bool,

    /// gfx_vignette: the lens-flare vignette pass. (Like the C++ editor, the vignette draws whatever this says: the pass tested its shader.)
    pub vignette: bool,

    /// gfx_depth_of_field: ENABLE_DEPTH_OF_FIELD hdr_to_screen when use_DOF.
    pub depth_of_field: bool,

    /// gfx_distortion: the distortion composite.
    pub distortion: bool,

    /// gfx_shadow_quality: the cascade split row (ShadowMapView +0x84).
    pub shadow_quality: i32,

    /// gfx_water_quality: the water planes' and the sea's pixel shaders (the engine's `water_low` .. `water_ultra`; Rome 2's extreme is
    /// ultra).
    pub water_quality: i32,

    /// gfx_building_quality, gfx_unit_quality, gfx_tree_quality: the first LODs of some models are switched off at the low levels
    /// (`draw_set::LodQuality`).
    pub building_quality: i32,
    pub unit_quality: i32,
    pub tree_quality: i32,

    /// gfx_sky_quality: the sky cube's size (`sky::SKY_CUBE_LARGE`, 256 at 0).
    pub sky_quality: i32,

    /// gfx_screen_space_reflections: the environment specular pass off at 0 (the directional light adds the specular then), its
    /// buffers at half size at 1, full size at 2.
    pub reflections: i32,

    /// gfx_grain: the film grain (Thrones of Britannia's hdr_to_screen ENABLE_GRAIN variants).
    pub grain: bool,

    /// gfx_cloud_shadows: the clouds' shade over the sun (Thrones of Britannia's directional light).
    pub cloud_shadows: bool,

    /// gfx_effects_quality: the effects' quality variant (0 best .. 2) and the particles' capacity (`particles`).
    pub effects_quality: i32,

    /// gfx_texture_filtering: the materials' and the terrain's "quality" samplers, trilinear or anisotropic 2^n (fn 0x1177f410).
    pub texture_filtering: i32,

    /// Editor only: the scene is drawn at this fraction of the target's size (0.5 .. 1) and enlarged with NVIDIA Image Scaling (`nis.rs`)
    /// with this sharpness (0 .. 1); 1 draws it at the target's size, as before.
    pub render_scale: f32,
    pub sharpness: f32,

    /// The game view: the scene as the game shows it, without the editor's overlays (markers, polygons, the grid), the selection outline
    /// and the tint; overlays are not picked either.
    pub game_view: bool,

    // Editor overlays: a line grid on y = 0, and the entities whose bounds are outlined (yellow, on top).
    pub show_grid: bool,
    pub grid_step: f32,
    pub grid_half_extent: f32,
    pub selection: Vec<EntityId>,

    /// Mesh entities laid over with `tint_colour` (RGBA, 0..1; alpha = how strong): an editor view filter (the tile tool's freeform
    /// instances), drawn over the game's shading.
    pub tinted: Vec<EntityId>,
    pub tint_colour: [f32; 4],

    /// The selection outline's colour (RGBA, 0..1) and width in pixels.
    pub selection_colour: [f32; 4],
    pub selection_outline_px: f32,

    /// Changes when an open vfx/ or weather/ XML is edited: the particles reload.
    pub vfx_revision: u64,
    pub cloud_seed: u32,

    /// The battle's wind (a key of `wind_levels_tables`, `calm_westerly` .. `storm_westerly`): it picks the sea's surfaces and whitecaps.
    pub sea_state: String,

    // Battle state the engine feeds the weather shaders (not part of the environment files).

    /// wind_dir_random: the battle's random direction comes from this seed.
    pub wind_dir_seed: u32,

    /// weather_shared.g_season_type (snow dump: 3 in winter; a summer capture: 1). Unread by every shader; a scene with a vegetation season
    /// sets it.
    pub season: i32,

    /// weather_shared.g_snow_mask (the battle map has snow).
    pub snow_cover: bool,
    pub view: View,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            lod_scale: 1.0,
            min_pixels: 0.75,
            shadow_min_texels: 2.0,
            deterministic: false,
            fog: true,
            shadows: true,
            clouds: true,
            ssao: true,
            lens_flare: true,
            vignette: true,
            depth_of_field: true,
            distortion: true,
            shadow_quality: 3,
            water_quality: 3,
            building_quality: 3,
            unit_quality: 3,
            tree_quality: 3,
            sky_quality: 3,
            reflections: 2,
            effects_quality: 3,
            grain: false,
            cloud_shadows: false,
            texture_filtering: 3,
            render_scale: 1.0,
            sharpness: 0.5,
            game_view: false,
            show_grid: false,
            grid_step: 10.0,
            grid_half_extent: 200.0,
            selection: vec![],
            tinted: vec![],
            tint_colour: [0.0; 4],
            selection_colour: [1.0, 0.82, 0.24, 1.0],
            selection_outline_px: 2.0,
            vfx_revision: 0,
            cloud_seed: 1,
            sea_state: "calm_westerly".to_owned(),
            wind_dir_seed: 1,
            season: 0,
            snow_cover: false,
            view: View::Final,
        }
    }
}

impl RenderSettings {
    pub fn apply_graphics_options(&mut self, o: &GraphicsOptions) {
        self.vignette = o.vignette;
        self.ssao = o.ssao;
        self.distortion = o.distortion;
        self.depth_of_field = o.depth_of_field > 0;
        self.shadow_quality = o.shadow_quality;
        self.water_quality = o.water_quality;
        self.building_quality = o.building_quality;
        self.unit_quality = o.unit_quality;
        self.tree_quality = o.tree_quality;
        self.sky_quality = o.sky_quality;
        self.reflections = o.reflections;
        self.effects_quality = o.effects_quality;
        self.grain = o.grain;
        self.cloud_shadows = o.cloud_shadows;
        self.texture_filtering = o.texture_filtering;
    }
}

/// The view being drawn (camera, cascade or environment face): what culling and the overlay use.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CameraState {
    /// Of the main camera (the overlay pass).
    pub main_view_proj: Float4x4,

    /// Set by set_camera: what pass_gbuffer culls with.
    pub view_proj: Float4x4,
    pub eye: Float3,

    /// The angular size of one pixel (tangent).
    pub px_ratio: f32,

    /// Game units per scene unit (`RenderScene::game_units`). Culling, LODs and the overlay work in scene units; what the game's shaders
    /// see (the camera constants, instance matrices, terrain, decals, lights, particles) is scaled by this ([`game_view`] and the others).
    pub units: f32,

    /// What `camera_VS_PS` was last given (game units): the view, the view-projection and the eye (the terrain moves them with its LF
    /// frame for its draws, then puts them back).
    pub cb_view: Float4x4,
    pub cb_view_proj: Float4x4,
    pub cb_eye: Float3,

    /// The render origin (scene units, y 0): what the shaders get (the camera, instances, terrain, decals, lights, the sea, the effects) is
    /// relative to it, the scene and culling stay absolute. Far from the scene's own origin (a map context, a whole campaign map away from
    /// the open tile) float32 positions of 10^5 lost the depth's and the ground textures' precision: the terrain swam as the camera moved.
    /// It stays within ORIGIN_REACH of the camera, on that grid.
    pub origin: Float3,
}

/// The near plane follows the camera's clearance over the ground: a quarter of it, less what the tallest buildings take, between the
/// camera's own near plane and NEAR_MAX (scene units). A 24-bit depth's precision goes with near / distance^2: at 1 unit, the terrain 8 km
/// away was placed to several units, and the terrain blend pass, which takes its positions back from the depth, and the sea, which compares
/// depths at the shore, flickered as the camera moved.
const NEAR_FRACTION: f32 = 0.25;
const NEAR_MARGIN: f32 = 60.0;
const NEAR_MAX: f32 = 100.0;

/// How far the camera goes from the render origin before it moves, and the grid it moves on (scene units). Within about this much of
/// the origin, as a game battle is of its own, the terrain blend pass takes its positions back from the depth precisely enough: 4 km
/// away (the 16 km grid this was) the ground slid a few pixels against the buildings' stone floors as the camera moved. A multiple of the
/// LF's samples and normal texels (32, 64), the alpha maps' texels (4), and with the terrain layers' tilings rounded to whole divisions
/// of it (tiling: the climates' 9, 40.1, 620 and 1200.1 m become 9.007, 40.53, 608 and 1216, at most 1.9 % off) the ground's patterns do
/// not move when the origin does.
pub const ORIGIN_REACH: f32 = 1216.0;

/// A terrain layer's tiling (`blend_pixel_scale`, game units) rounded to a whole division of the origin grid (`grid`, game units).
pub(crate) fn tiling(scale: f32, grid: f32) -> f32 {
    if scale <= 0.0 { scale } else { grid / (grid / scale).round().max(1.0) }
}

impl Default for CameraState {
    fn default() -> Self {
        Self {
            main_view_proj: Float4x4::IDENTITY, view_proj: Float4x4::IDENTITY, eye: Float3::default(), px_ratio: 0.001, units: 1.0, cb_view: Float4x4::IDENTITY,
            cb_view_proj: Float4x4::IDENTITY, cb_eye: Float3::default(), origin: Float3::default(),
        }
    }
}

/// A world -> view matrix of the scene as the shaders get it (world and view space in game units): the translation times `s`.
pub(crate) fn game_view(v: &Float4x4, s: f32) -> Float4x4 {
    let mut g = *v;
    for row in g.m.iter_mut().take(3) {
        row[3] *= s;
    }

    g
}

/// A perspective projection for a view space in game units: near and far times `s` (the fourth column), the same depth and NDC as `p` gives
/// the scene's view space, w the game units' view depth.
pub(crate) fn game_proj(p: &Float4x4, s: f32) -> Float4x4 {
    let mut g = *p;
    for row in &mut g.m {
        row[3] *= s;
    }

    g
}

/// A world -> clip (or shadow texture) matrix of the scene for world positions in game units: `m * scaling(1 / s)`.
pub(crate) fn game_clip(m: &Float4x4, s: f32) -> Float4x4 {
    let mut g = *m;
    for row in &mut g.m {
        for v in row.iter_mut().take(3) {
            *v /= s;
        }
    }

    g
}

/// A scene world matrix for the shaders: relative to the render origin `o`, in game units.
pub(crate) fn rel_world(m: &Float4x4, o: Float3, s: f32) -> Float4x4 {
    let mut w = *m;
    (w.m[0][3], w.m[1][3], w.m[2][3]) = (w.m[0][3] - o.x, w.m[1][3] - o.y, w.m[2][3] - o.z);
    game_world(&w, s)
}

/// A scene world matrix (an instance's, an effect's) in game units: the whole matrix times `s` (models drawn `s` times larger, positions `s`
/// times farther).
pub(crate) fn game_world(m: &Float4x4, s: f32) -> Float4x4 {
    let mut g = *m;
    for row in g.m.iter_mut().take(3) {
        for v in row.iter_mut() {
            *v *= s;
        }
    }

    g
}

/// The frame counter and the frame time, from the times the frames were drawn at.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameClock {
    pub frame: u64,

    /// The time of the last frame; negative before the first.
    pub last_time: f32,
    pub dt: f32,
}

impl Default for FrameClock {
    fn default() -> Self {
        Self { frame: 0, last_time: -1.0, dt: 0.016 }
    }
}

/// The renderer: the game's passes over a set of GPU subsystems.
/// The ids of the scene's entities of the kinds a pass looks for every frame (water, point lights, terrain), rebuilt when the scene's structure
/// changes: a scene with a map context has over a million entities, and walking them all once per pass cost milliseconds each.
#[derive(Default)]
pub(crate) struct KindIndex {
    scene: usize,
    revision: u64,
    pub water: Vec<wce_scene::render_scene::EntityId>,
    pub lights: Vec<wce_scene::render_scene::EntityId>,
    pub terrain: Vec<wce_scene::render_scene::EntityId>,
}

impl KindIndex {
    fn refresh(&mut self, scene: &RenderScene) {
        let key = (std::ptr::from_ref(scene) as usize, scene.structure_revision());
        if (self.scene, self.revision) == key {
            return;
        }

        (self.scene, self.revision) = key;
        self.water.clear();
        self.lights.clear();
        self.terrain.clear();
        for e in scene.entities() {
            match &e.payload {
                wce_scene::render_scene::Payload::Water(_) => self.water.push(e.id),
                wce_scene::render_scene::Payload::PointLight(_) => self.lights.push(e.id),
                wce_scene::render_scene::Payload::Terrain(_) => self.terrain.push(e.id),
                _ => {}
            }
        }
    }

    /// The entities of a list that still exist, in scene order.
    pub fn entities<'s>(ids: &[wce_scene::render_scene::EntityId], scene: &'s RenderScene) -> Vec<&'s wce_scene::render_scene::Entity> {
        ids.iter().filter_map(|&id| scene.find(id)).collect()
    }
}

pub struct Renderer {
    /// The game whose renderer this is.
    pub(crate) game: game::RenderGame,

    pub(crate) kinds: KindIndex,
    pub(crate) dev: ID3D11Device,
    pub(crate) ctx: ID3D11DeviceContext,
    pub(crate) files: Arc<dyn Files>,
    pub(crate) cbs: ConstantBuffers,
    pub(crate) lib: ShaderLibrary,
    pub(crate) shader_failures: Vec<String>,
    pub(crate) errors: Vec<String>,

    /// The material texture slots of each pixel shader (by address).
    pub(crate) ps_slots: HashMap<usize, [Option<u32>; TEX_SLOTS]>,

    pub(crate) textures: Arc<TextureCache>,
    pub(crate) models: ModelCache,
    pub(crate) gpu_timer: GpuTimer,
    pub(crate) states: States,

    /// Bytes of the GPU's own video memory (0 unknown): the game's presets choose the texture quality by it.
    pub video_memory: u64,

    /// The render scale's upscaler, the scene's image at its size, and whether NIS failed (the scale is then off).
    nis: nis::Nis,
    scaled: Option<RenderTarget>,

    /// The film grain (gfx_grain).
    grain: Grain,

    /// The cloud shade's offset (g_cloud_shade_params.xy): moved with the wind by seconds * cloud_shade_battle_speed / 4096, kept in 0..1.
    cloud_shade: [f32; 2],

    /// The effects quality the definitions were read with.
    effects_applied: Option<i32>,
    nis_failed: bool,
    pub(crate) res: SharedResources,

    /// The main view's targets (the environment cube's while its faces render), and the post-processing ones.
    pub(crate) gbuffer: GBuffer,
    pub(crate) post: PostTargets,
    pub(crate) shadow_map: ShadowMap,
    pub(crate) draw_lists: SceneDraws,
    pub(crate) camera: CameraState,
    pub(crate) clock: FrameClock,
    pub(crate) draw_stats: DrawStats,
    pub(crate) sky: SkyCube,
    pub(crate) env: EnvCube,
    pub(crate) sun: SunGpu,
    pub(crate) cloud_layer: CloudLayer,
    pub(crate) wind: WindSim,
    pub(crate) tonemap: ToneMap,
    pub(crate) depth_range: DepthRange,

    /// This frame's snowing intensities: the weather's, and with snow regions the snowy cells' (at least the weather's); the one the
    /// shaders have now (weather_shared.g_snowing_intensity).
    pub(crate) snow_weather: f32,
    pub(crate) snow_region: Option<f32>,
    pub(crate) snow_bound: f32,

    /// The scene's turn of the battle's frame (`RenderScene::world_yaw`, degrees), for the wind.
    pub(crate) world_yaw: f32,
    pub(crate) particles: ParticleSystem,

    /// Rome 2's effects (its own particle system and per-effect shaders).
    pub(crate) vfx_rome2: crate::vfx_rome2::Rome2Vfx,

    /// The scene (address, revision) the particles' placed effects were gathered from.
    pub(crate) vfx_scene: (usize, u64, u32, u32, u32, u32),
    pub(crate) editor: editor::EditorGpu,
    pub(crate) water: water::WaterGpu,
    pub(crate) terrain: terrain::TerrainGpuState,
    pub(crate) decals: decals::DecalGpu,
    pub(crate) lights: lights::LightGpu,
    pub(crate) grass: grass::GrassGpu,
    pub settings: RenderSettings,
}

/// An immutable buffer.
pub(crate) fn immutable_buffer(dev: &ID3D11Device, data: &[u8], bind: D3D11_BIND_FLAG) -> Option<ID3D11Buffer> {
    let desc = D3D11_BUFFER_DESC { ByteWidth: data.len() as u32, Usage: D3D11_USAGE_IMMUTABLE, BindFlags: bind as u32, CPUAccessFlags: 0, MiscFlags: 0, StructureByteStride: 0 };
    dev.create_buffer(&desc, Some(data)).ok()
}

/// A dynamic (CPU-written) buffer.
pub(crate) fn dynamic_buffer(dev: &ID3D11Device, bytes: usize, bind: D3D11_BIND_FLAG) -> Option<ID3D11Buffer> {
    let desc = D3D11_BUFFER_DESC { ByteWidth: bytes as u32, Usage: D3D11_USAGE_DYNAMIC, BindFlags: bind as u32, CPUAccessFlags: D3D11_CPU_ACCESS_WRITE as u32, MiscFlags: 0, StructureByteStride: 0 };
    dev.create_buffer(&desc, None).ok()
}

/// The bytes of floats.
pub(crate) fn f32_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// An immutable texture of one subresource, and its view.
pub(crate) fn immutable_srv(dev: &ID3D11Device, w: u32, h: u32, fmt: DXGI_FORMAT, data: &[u8]) -> Option<ID3D11ShaderResourceView> {
    let td = D3D11_TEXTURE2D_DESC {
        Width: w, Height: h, MipLevels: 1, ArraySize: 1, Format: fmt, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 }, Usage: D3D11_USAGE_IMMUTABLE,
        BindFlags: D3D11_BIND_SHADER_RESOURCE as u32, CPUAccessFlags: 0, MiscFlags: 0,
    };

    let pitch = (data.len() / h.max(1) as usize) as u32;
    dev.create_texture_2d(&td, Some(&[InitData::new(data, pitch)])).and_then(|tex| dev.create_shader_resource_view(&tex, None)).ok()
}

/// A 1x1 texture of one colour.
pub(crate) fn solid_srv(dev: &ID3D11Device, fmt: DXGI_FORMAT, texel: &[u8]) -> Option<ID3D11ShaderResourceView> {
    immutable_srv(dev, 1, 1, fmt, texel)
}

/// Binds one sampler to every sampler slot of a pixel shader.
pub(crate) fn ps_samplers(ctx: &ID3D11DeviceContext, s: &GameShader, smp: &Option<ID3D11SamplerState>) {
    for &slot in s.samplers.values() {
        ctx.ps_set_samplers(slot, std::slice::from_ref(smp));
    }
}

/// Unbinds the first `n` pixel shader resources.
pub(crate) fn null_ps_srvs(ctx: &ID3D11DeviceContext, n: usize) {
    ctx.ps_set_shader_resources(0, &vec![None; n]);
}

impl Renderer {

    /// A renderer on a device, reading the game's files from `files`.
    ///
    /// # Errors
    ///
    /// Fails if the helper shaders don't compile.
    pub fn new(dev: &ID3D11Device, ctx: &ID3D11DeviceContext, files: Arc<dyn Files>) -> Result<Self, RenderError> {
        // Which GPU renders matters for reproducing pixels (two adapters: results differ between them).
        let mut video_memory = 0;
        if let Ok(dxgi) = dev.cast::<IDXGIDevice>() {
            // SAFETY: plain queries of the device's adapter.
            unsafe {
                if let Some(info) = dxgi.GetAdapter().and_then(|a| a.cast::<IDXGIAdapter1>()).ok().and_then(|a| adapter_info(&a)) {
                    wce_foundation::log::info(format!("GPU: {}", info.name));
                    video_memory = info.video_memory;
                }
            }
        }

        let reader = shared_reader(&files);
        let textures = Arc::new(TextureCache::new(dev, reader.clone()));
        let models = ModelCache::new(dev, reader, textures.clone());
        let particle_files = files.clone();
        let supported = game::RenderGame::of(files.as_ref()) != game::RenderGame::Rome2;
        let particles = ParticleSystem::new_for(dev, ctx, Box::new(move |path: &str| particle_files.read(path)), supported);
        let vfx_files = files.clone();
        let vfx_rome2 = crate::vfx_rome2::Rome2Vfx::new(dev, ctx, Box::new(move |path: &str| vfx_files.read(path)));

        let mut cbs = ConstantBuffers::default();

        // Faction colours: white = no tint on the test map.
        cbs.declare("rigid_faction_colours", 64, &BTreeMap::from([("faction_colours".to_owned(), Var { offset: 0, size: 60 })]));
        cbs.set_f("rigid_faction_colours", "faction_colours", &[1.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0]);
        Ok(Self {
            game: game::RenderGame::of(files.as_ref()),
            dev: dev.clone(),
            ctx: ctx.clone(),
            files,
            kinds: KindIndex::default(),
            cbs,
            lib: ShaderLibrary::new(dev),
            shader_failures: vec![],
            errors: vec![],
            ps_slots: HashMap::new(),
            textures,
            models,
            gpu_timer: GpuTimer::new(dev, ctx),
            states: States::new(dev),
            video_memory,
            nis: nis::Nis::default(),
            effects_applied: None,
            grain: Grain::default(),
            cloud_shade: [0.0; 2],
            scaled: None,
            nis_failed: false,
            res: SharedResources::new(dev)?,
            gbuffer: GBuffer::default(),
            post: PostTargets::default(),
            shadow_map: ShadowMap::new(dev),
            draw_lists: SceneDraws::default(),
            camera: CameraState::default(),
            clock: FrameClock::default(),
            draw_stats: DrawStats::default(),
            sky: SkyCube::new(dev),
            env: EnvCube::new(dev),
            sun: SunGpu::new(dev),
            cloud_layer: CloudLayer::default(),
            wind: WindSim::default(),
            tonemap: ToneMap::new(dev),
            depth_range: DepthRange::default(),
            snow_weather: 0.0,
            snow_region: None,
            snow_bound: 0.0,
            world_yaw: 0.0,
            particles,
            vfx_rome2,
            vfx_scene: (0, u64::MAX, 0, 0, 0, 0),
            editor: editor::EditorGpu::default(),
            water: water::WaterGpu::default(),
            terrain: terrain::TerrainGpuState::default(),
            decals: decals::DecalGpu::default(),
            lights: lights::LightGpu::default(),
            grass: grass::GrassGpu::default(),
            settings: RenderSettings::default(),
        })
    }

    // ---- Public accessors.

    /// Mesh and terrain entities that are ready to draw.
    pub fn draw_count(&self) -> usize {
        self.draw_lists.entities + self.draw_lists.terrain_entities
    }

    /// Their LOD 0 meshes (terrain: patches).
    pub fn mesh_count(&self) -> usize {
        self.draw_lists.meshes + self.draw_lists.terrain_meshes
    }

    pub fn draw_stats(&self) -> &DrawStats {
        &self.draw_stats
    }

    /// The game shaders that couldn't be loaded (name: reason), newest first: the editor shows them where a pass would otherwise silently draw nothing.
    pub fn shader_failures(&self) -> &[String] {
        &self.shader_failures
    }

    pub fn sun_visibility(&self) -> f32 {
        self.sun.visibility
    }

    pub fn wind(&self) -> &WindSim {
        &self.wind
    }

    pub fn cloud_sprites(&self) -> usize {
        self.cloud_layer.planes.iter().map(|p| p.count as usize).sum()
    }

    /// The tone mapper's black / white points as last read back, and how many reads there were.
    pub fn points_readback(&self) -> ([f32; 4], i32) {
        (self.tonemap.debug_points, self.tonemap.debug_maps)
    }

    pub fn luma_black(&self) -> f32 {
        self.tonemap.luma_black
    }

    pub fn luma_white(&self) -> f32 {
        self.tonemap.luma_white
    }

    pub fn engine_ambient_faces(&self) -> String {
        self.env.ambient_faces.iter().map(|f| format!("({:.6} {:.6} {:.6}) ", f[0], f[1], f[2])).collect()
    }

    /// Forgets what earlier frames left behind (auto exposure, tone mapper limits, frame counter, clock), so the next frames depend on the scene only.
    pub fn restart_temporal(&mut self) {
        self.clock.frame = 0;
        self.clock.last_time = -1.0;
        self.tonemap.luma_black = ToneMap::BLACK;
        self.tonemap.luma_white = ToneMap::WHITE;
        self.env.valid = false;
    }

    pub fn textures(&self) -> &Arc<TextureCache> {
        &self.textures
    }

    pub fn models(&mut self) -> &mut ModelCache {
        &mut self.models
    }

    pub fn gpu_timer(&mut self) -> &mut GpuTimer {
        &mut self.gpu_timer
    }

    pub fn cbuffers(&mut self) -> &mut ConstantBuffers {
        &mut self.cbs
    }

    /// The global snow normals (black until loaded: the shaders read them only while it snows).
    pub(crate) fn snow_normals_srv(&mut self) -> Option<ID3D11ShaderResourceView> {
        self.textures.get(SNOW_NORMALS, false).and_then(|t| t.read().ok().and_then(|t| t.srv.clone())).or_else(|| self.res.black_srv.clone())
    }

    pub fn particles(&mut self) -> &mut ParticleSystem {
        &mut self.particles
    }

    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    // ---- Shared helpers.

    pub(crate) fn error(&mut self, e: String) {
        if !self.errors.contains(&e) {
            wce_foundation::log::error(e.clone());
            self.errors.push(e);
        }
    }

    /// A game shader, or `None` (reported) when it can't be loaded.
    pub(crate) fn shader(&mut self, name: &str) -> Option<Rc<GameShader>> {
        let files = self.files.clone();
        let read: &FileReader = &|path: &str| files.read(path);
        match self.lib.get(read, &mut self.cbs, name) {
            Ok(shader) => Some(shader),
            Err(error) => {
                self.error(error.0.clone());
                let why = format!("{name}: {}", error.0);
                if !self.shader_failures.contains(&why) {
                    self.shader_failures.insert(0, why);
                }

                None
            }
        }
    }

    /// An input layout of a shader; `None` (reported) when the device refuses it.
    pub(crate) fn layout(&mut self, vs: &GameShader, id: &str, elements: &[InputElement]) -> Option<ID3D11InputLayout> {
        match self.lib.layout(vs, id, elements) {
            Ok(layout) => Some(layout),
            Err(error) => {
                self.error(error.0);
                None
            }
        }
    }

    pub(crate) fn upload(&mut self) {
        let _ = self.cbs.upload(&self.dev, &self.ctx);
    }

    pub(crate) fn bind(&self, s: &GameShader) {
        bind_shader(&self.ctx, s, &self.cbs);
    }

    fn resize(&mut self, w: u32, h: u32) {
        if w == self.gbuffer.w && h == self.gbuffer.h {
            return;
        }

        self.gbuffer = GBuffer::new(&self.dev, w, h);
        self.post.resize(&self.dev, w, h);
    }

    /// The near plane for a camera (see NEAR_FRACTION): from its clearance over the scene's terrains (their highest ground under the eye).
    fn near_plane(&self, scene: &RenderScene, cam: &Camera) -> f32 {
        let ground = KindIndex::entities(&self.kinds.terrain, scene).into_iter()
            .filter(|e| scene.drawable(e))
            .filter_map(|e| match &e.payload {
                wce_scene::render_scene::Payload::Terrain(t) => t.ground_at(&e.world, cam.eye.x, cam.eye.z),
                _ => None,
            })
            .fold(None, |m: Option<f32>, h| Some(m.map_or(h, |m| m.max(h))));
        let Some(ground) = ground else { return cam.znear };
        ((cam.eye.y - ground - NEAR_MARGIN) * NEAR_FRACTION).clamp(cam.znear, NEAR_MAX.max(cam.znear))
    }

    /// `view_m` is the view of positions relative to the render origin (scene units), `eye` the absolute one: culling gets the absolute view,
    /// the shaders the relative one in game units ([`CameraState::units`]).
    pub(crate) fn set_camera(&mut self, view_m: &Float4x4, proj: &Float4x4, eye: Float3, w: u32, h: u32, time_s: f32) {
        let o = self.camera.origin;
        self.camera.view_proj = *proj * *view_m * translation(-o);
        self.camera.eye = eye;
        self.camera.px_ratio = 1.0 / (0.5 * h as f32 * proj.m[1][1]);
        let s = self.camera.units;
        let (view_m, proj, eye) = (&game_view(view_m, s), &game_proj(proj, s), (eye - o) * s);
        let vp = *proj * *view_m;
        (self.camera.cb_view, self.camera.cb_view_proj, self.camera.cb_eye) = (*view_m, vp, eye);
        let cbs = &mut self.cbs;
        cbs.set_f("camera_VS_PS", "camera_position", &[eye.x, eye.y, eye.z]);
        cbs.set_m("camera_VS_PS", "view", view_m.data());
        cbs.set_m("camera_VS_PS", "projection", proj.data());
        cbs.set_m("camera_VS_PS", "view_projection", vp.data());
        cbs.set_m("camera_VS_PS", "inv_view", inverse(view_m).data());
        cbs.set_m("camera_VS_PS", "inv_projection", inverse(proj).data());
        cbs.set_m("camera_VS_PS", "inv_view_projection", inverse(&vp).data());
        cbs.set_f("camera_VS_PS", "time_in_sec", &[time_s]);
        cbs.set_f("camera_VS_PS", "g_inverse_focal_length", &[1.0 / proj.m[0][0], 1.0 / proj.m[1][1]]);
        let (fw, fh) = (w as f32, h as f32);
        cbs.set_f("camera_VS_PS", "g_screen_size", &[fw, fh, 1.0 / fw, 1.0 / fh]);

        // SV_Position is already at pixel centres in D3D11.
        cbs.set_f("camera_VS_PS", "g_vpos_texel_offset", &[0.0]);
        cbs.set_f("camera_VS_PS", "g_viewport_dimensions", &[fw, fh, 1.0 / fw, 1.0 / fh]);
        cbs.set_f("camera_VS_PS", "g_render_target_dimensions", &[fw, fh, 1.0 / fw, 1.0 / fh]);
        cbs.set_i("camera_VS_PS", "g_num_of_samples", 1);

        // The forward shaders discard outside this rectangle (in normalized device coordinates); the game's is the whole view.
        cbs.set_f("camera_VS_PS", "g_clip_rect", &[-1.0, -1.0, 1.0, 1.0]);

        // Rome 2's leaves fade near the camera: their alpha is scaled by saturate(distance^2 * g_hide_foliage) (0 hides every leaf). The
        // engine's value is not known: 1, the leaves within a unit of the camera faded.
        cbs.set_f("camera_VS_PS", "g_hide_foliage", &[1.0]);
    }

    /// The full-screen triangle with a pixel shader.
    pub(crate) fn fullscreen(&self, ps: &GameShader) {
        self.ctx.set_input_layout(None);
        self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        self.ctx.vs_set_shader(self.res.fs_vs.as_ref());
        self.bind(ps);
        self.ctx.draw(3, 0);
    }

    pub(crate) fn fullscreen_to(&mut self, rtv: &Option<ID3D11RenderTargetView>, w: u32, h: u32, ps: &GameShader) {
        self.ctx.set_render_targets(std::slice::from_ref(rtv), None);
        self.ctx.set_viewports(&[viewport(0.0, 0.0, w as f32, h as f32)]);

        self.cbs.set_f("render_target_dims_buffer", "g_render_target_recip_size", &[1.0 / w as f32, 1.0 / h as f32]);
        self.upload();
        self.fullscreen(ps);
        null_ps_srvs(&self.ctx, 8);
    }

    pub fn wind_direction_degrees(&self, env: &EnvironmentParams) -> f32 {
        if !env.wind_dir_random {
            return env.wind_dir_degrees;
        }

        let mut seed = self.settings.wind_dir_seed;
        WindSim::random_direction(&mut seed)
    }

    /// The WEATHER node's per-frame update (fn 0x18ee0c0): the wind simulation, weather_shared, wind_VS, wind_PS.
    /// The shaders' snowing intensity (weather_shared), uploaded when it changes.
    pub(crate) fn set_snowing(&mut self, intensity: f32) {
        if intensity != self.snow_bound {
            self.snow_bound = intensity;
            self.cbs.set_f("weather_shared", "g_snowing_intensity", &[intensity]);
            self.upload();
        }
    }

    fn update_weather(&mut self, env: &EnvironmentParams, time_s: f32) {
        // Rome 2's weather particles are not drawn yet (TODO(rome2) in the particle pass); its wind still moves the effects.
        if self.game != game::RenderGame::Rome2 {
            self.particles.set_weather(&env.weather_definition, self.settings.vfx_revision);
        }

        let def = self.particles.effect().definition.clone();
        // The wind's angle runs from +x towards +z: the scene's turn takes it the other way round.
        let degrees = self.wind_direction_degrees(env) - self.world_yaw;
        self.wind.update(Some(&def), degrees, time_s, self.clock.dt);
        let (w, cbs) = (&self.wind, &mut self.cbs);
        cbs.set_f("weather_shared", "g_wind", &[w.smoothed.x, w.smoothed.y]);
        cbs.set_f("weather_shared", "g_wind_offset", &[w.offset.x, w.offset.y]);
        cbs.set_i("weather_shared", "g_weather_type", def.type_index());
        cbs.set_i("weather_shared", "g_season_type", self.settings.season);
        cbs.set_f("weather_shared", "g_snow_mask", &[if self.settings.snow_cover { 1.0 } else { 0.0 }]);
        cbs.set_f("wind_VS", "wind_direction", &[w.direction.x, w.direction.y]);
        cbs.set_f("wind_VS", "gust_start_time", &[w.gust_end]);
        cbs.set_f("wind_VS", "wind_vegetation_multiplier", &[def.wind_vegetation_multiplier]);
        cbs.set_f("wind_VS", "wind_vegetation_hstrength", &[WindSim::VEGETATION_H_STRENGTH]);
        cbs.set_f("wind_VS", "wind_vegetation_vstrength", &[WindSim::VEGETATION_V_STRENGTH]);

        // Only rivers (rigidriver.hlsl) declare it.
        if cbs.has("wind_PS", "wind_direction_PS") {
            cbs.set_f("wind_PS", "wind_direction_PS", &[w.direction.x, w.direction.y]);
        }
    }

    /// Draws a scene (lit by its environment slot) as a view sees it into a target, at the target's size.
    pub fn render(&mut self, scene: &RenderScene, view_params: &ViewParams, target: &mut RenderTarget) {
        self.grass.season.clone_from(&scene.vegetation_season);

        // The battle's season as the game sends it (summer 1 in a capture, winter 3 in a dump; spring 0 and autumn 2 by their order). No
        // shader reads it (every vanilla and TDD shader declares it unused, 2026-10-31): kept for fidelity only.
        if let Some(k) = ["sp", "su", "au", "wi"].iter().position(|s| scene.vegetation_season.eq_ignore_ascii_case(s)) {
            self.settings.season = k as i32;
        }
        self.camera.units = if scene.game_units > 0.0 { scene.game_units } else { 1.0 };
        wce_prof!("render.total");
        let mut cam = view_params.camera;
        let time_s = view_params.time_s;
        let (out_w, out_h) = (target.width(), target.height());
        if out_w < 8 || out_h < 8 {
            return;
        }

        // The render scale (editor only): the scene at a fraction of the target's size, enlarged by NIS at the end.
        let scale = self.settings.render_scale;
        let (width, height) = if scale.is_finite() && scale < 0.999 && !self.nis_failed {
            let s = scale.max(0.5);
            (((out_w as f32 * s).round() as u32).max(8), ((out_h as f32 * s).round() as u32).max(8))
        } else {
            (out_w, out_h)
        };

        let env = scene.environment.resolved().turned(scene.world_yaw);
        self.world_yaw = scene.world_yaw;
        self.kinds.refresh(scene);
        cam.znear = self.near_plane(scene, &cam);

        // Rome 2's filtering levels are not mapped yet (docs/re/graphics_options.md): anisotropic 8x, as before.
        let filtering = if self.game == game::RenderGame::Rome2 { 3 } else { self.settings.texture_filtering.clamp(0, 4) };
        if self.states.filtering != filtering {
            self.states.set_filtering(&self.dev, filtering);
        }

        // gfx_effects_quality: the effects' quality variant (not Rome 2's, whose levels are not mapped); a change reads them again.
        let effects = if self.game == game::RenderGame::Rome2 { 3 } else { self.settings.effects_quality };
        if self.effects_applied != Some(effects) {
            wce_assets::vfx_def::set_quality(effects);
            self.particles.set_limit(effects);
            if self.effects_applied.is_some() {
                self.particles.reload_definitions();
                self.vfx_scene = (0, u64::MAX, 0, 0, 0, 0);
            }

            self.effects_applied = Some(effects);
        }

        // Fixed-step runs load every texture whole: frame N must be the same every run.
        self.textures.set_streaming(!self.settings.deterministic);
        self.textures.pump();
        self.build_draws(scene, cam.eye);
        self.place_effect_rigids();
        let waiting = self.draw_lists.waiting;
        self.draw_stats = DrawStats { items: self.draw_stats.items, batches: self.draw_stats.batches, models: self.draw_stats.models, waiting_models: waiting, ..Default::default() };
        self.resize(width, height);
        self.gpu_timer.begin_frame();
        self.gpu_timer.mark("setup");

        // Every cbuffer this frame needs is declared before values are written.
        let luma_stats = match self.game {
            game::RenderGame::Attila => [TM_DOWN_LUMA, TM_LUMA_STATS, TM_STATS_DOWN, TM_POINTS],
            game::RenderGame::Rome2 => [TM_R2_DOWN, TM_R2_LOG_Y, TM_R2_STATS, TM_R2_STATS_DOWN],
        };
        for s in [SKY_VS, SKY_GEN_PS, SKY_PS, RIGID_VS, RIGID_GBUFFER_PS, RIGID_GBUFFER_ALPHA_PS, DIR_LIGHT_PS, TM_POINTS, TM_HIGH_PASS, TM_BLUR_X, TM_BLUR_Y,
            TM_TONE_MAP, TO_SCREEN_PS, FOG_PS, SHADOW_VS, SHADOW_PS, SUN_VS, SUN_PS, CLOUD_VS, CLOUD_PS, SSAO_PS, SSAO_BLUR_X_PS, SSAO_BLUR_Y_PS]
            .into_iter()
            .chain(luma_stats)
        {
            self.shader(s);
        }

        apply_environment(&env, &mut self.cbs);

        // gfx_screen_space_reflections 0 (Attila, Thrones of Britannia; fn 0x11904130): no environment specular pass, the directional
        // light adds the environment's specular itself.
        if self.reflections_off() {
            self.cbs.set_i("lighting_VS_PS", "g_apply_environment_specular", 1);
        }

        // Fog off: the fog pass is skipped, and the shaders that fog themselves (the rivers) get none either.
        if !self.settings.fog {
            self.cbs.set_f("fog_VS_PS", "g_fog_distance_start", &[1.0e9]);
            self.cbs.set_f("fog_VS_PS", "g_fog_distance_strength", &[0.0]);
            self.cbs.set_f("fog_VS_PS", "g_fog_height_strength", &[0.0]);
        }
        self.clock.dt = if self.clock.last_time < 0.0 { 0.016 } else { (time_s - self.clock.last_time).max(0.0) };

        // The cloud shade moves with the wind (fn 0x117c3680: the change of seconds * speed / 4096 along the wind's direction).
        if self.settings.cloud_shadows {
            let step = self.clock.dt * CLOUD_SHADE_BATTLE_SPEED / 4096.0;
            let d = self.wind.direction;
            self.cloud_shade = [(self.cloud_shade[0] + d.x * step).rem_euclid(1.0), (self.cloud_shade[1] + d.y * step).rem_euclid(1.0)];
        }

        // The clock restarted.
        if self.clock.last_time >= 0.0 && time_s < self.clock.last_time {
            self.wind.reset(WindSim::DEFAULT_SEED);
        }

        self.clock.last_time = time_s;
        self.cloud_layer.dt = self.clock.dt;
        self.update_weather(&env, time_s);

        // Attila's water leaves its reflection to the environment specular pass (the rivers' shader skips its cube reflection with this 1,
        // as in the game's captures); Rome 2 has no such pass.
        if self.game != game::RenderGame::Rome2 {
            self.cbs.set_i("lighting_VS_PS", "g_ssr_enabled", 1);
        }

        // Snow regions: what lies in them (flagged instances, decals, snowy trees and ground) snows at their amount, the rest as the weather.
        self.snow_weather = env.snowing_intensity;
        self.snow_region = scene.snow().map(|s| s.amount.max(env.snowing_intensity));
        self.set_snowing(self.snow_region.unwrap_or(self.snow_weather));

        self.gpu_timer.mark("sky cube");
        self.pass_sky_cube(&env, time_s);
        self.gpu_timer.mark("env cube");

        // The render origin follows the camera, on a grid; its height is the sea's (the shaders' heights are relative to
        // the sea, as the game's: its terrain lifts land and lowers sea bed by their sign, so that the shore and the water do not fight).
        let o = self.camera.origin;
        if (cam.eye.x - o.x).abs() > ORIGIN_REACH || (cam.eye.z - o.z).abs() > ORIGIN_REACH {
            let snap = |v: f32| (v / ORIGIN_REACH).round() * ORIGIN_REACH;
            self.camera.origin = Float3::new(snap(cam.eye.x), 0.0, snap(cam.eye.z));
        }

        self.camera.origin.y = scene.sea_level;

        // The XML ambient while it renders.
        if self.clock.frame.is_multiple_of(4) || !self.env.valid {
            wce_prof!("render.env_cube");
            self.render_env_cube(&env, time_s, scene);
        }

        self.apply_engine_ambient(&env);

        // The camera relative to the render origin (built from the relative eye: the absolute view's translation alone lost the precision).
        let aspect = width as f32 / height as f32;
        let o = self.camera.origin;
        let view_m = cam.view_from(o);
        let proj = cam.projection(aspect);

        // Before the shadows: the material terrain casts its own. The shadow pass sets only the cascades' view_projection; the rest of the
        // camera's constants (its position and view, which the trees' shaders take their LOD and billboards' facing from) must be the main
        // camera's: every fourth frame the environment cube left its last face's, and the trees' and buildings' shadows changed shape.
        self.terrain.active = self.prepare_terrain_materials(scene);
        self.terrain.lod_eye = cam.eye;
        self.set_camera(&view_m, &proj, cam.eye, width, height, time_s);
        self.gpu_timer.mark("shadows");
        {
            wce_prof!("render.shadows");
            self.pass_shadows(scene, &cam, aspect, -env.to_sun(), time_s);
        }

        self.set_camera(&view_m, &proj, cam.eye, width, height, time_s);
        self.camera.main_view_proj = self.camera.view_proj;
        self.upload();
        let main_vp = viewport(0.0, 0.0, width as f32, height as f32);
        self.ctx.set_viewports(&[main_vp]);
        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());

        self.gpu_timer.mark("gbuffer");
        {
            wce_prof!("render.gbuffer");
            self.pass_gbuffer();
        }

        if self.terrain.active {
            self.gpu_timer.mark("terrain");
            self.pass_terrain_material(scene);
        }

        // The grass is G-buffer content like the rest; the decals are the first step of the game's "DEFERRED LIGHTING" stage (fn 0x118c2750:
        // DEFERRED DECAL, then POINTLIGHT, REFRACTION, FOG, the water views), so they paint over the grass inside their boxes too.
        self.gpu_timer.mark("grass");
        self.pass_grass(scene);
        self.gpu_timer.mark("decals");
        self.pass_decals(scene);

        self.gpu_timer.mark("depth + ssao");
        self.analyse_depth(&cam);
        self.pass_ssao(&cam);
        self.gpu_timer.mark("sky/clouds/light/fog");
        self.ctx.set_viewports(&[main_vp]);
        if let Some(rtv) = &self.gbuffer.hdr.rtv {
            self.ctx.clear_render_target_view(rtv, [0.0; 4]);
        }

        self.pass_sky_forward();
        self.pass_sun(&env, &view_m, &proj);
        self.pass_clouds(&env);
        self.pass_directional_light();
        self.pass_point_lights(scene);
        self.gpu_timer.mark("sea");
        self.pass_water(scene, false);

        // Attila, as the game: the rivers with the other water, before the environment specular pass that adds their reflection (they are
        // stencil 2: the fog pass leaves them, their shader fogs itself). Rome 2's are forward and come after the fog.
        let rome2 = self.game == game::RenderGame::Rome2;
        if !rome2 {
            self.gpu_timer.mark("rivers");
            self.pass_water(scene, true);
        }

        self.pass_environment_specular();
        self.pass_fog();
        if rome2 {
            self.gpu_timer.mark("rivers");
            self.pass_water(scene, true);
        }

        // The faded instances, lit and fogged by their own shaders, over the finished light buffer.
        self.gpu_timer.mark("faded");
        self.pass_faded();

        self.gpu_timer.mark("particles");

        // The weather precipitation (vfx1 of the WEATHER definition).
        // The particles live in game units (the effects' sizes, speeds and the weather's volumes are the game's).
        let frame = Frame {
            eye: (cam.eye - o) * self.camera.units,
            view: view_m,
            time: time_s,
            dt: self.clock.dt,
            wind_velocity: self.wind.vfx_velocity,
            wind_variance: self.wind.vfx_variance,
            target: self.gbuffer.hdr.rtv.clone(),
            depth: self.gbuffer.dsv.clone(),
            depth_zw: self.gbuffer.channels[3].srv.clone(),
            sky: self.sky.cube.srv.clone(),
            width,
            height,
        };

        // The placed effects (tile VFX instances), gathered again when the scene changes.
        let units = self.camera.units;
        let scene_key = (std::ptr::from_ref(scene) as usize, scene.revision(), units.to_bits(), o.x.to_bits(), o.y.to_bits(), o.z.to_bits());
        if self.vfx_scene != scene_key {
            // A new origin moves the effects (the particles alive keep where they were).
            self.vfx_scene = scene_key;
            let effects = scene.entities().iter().filter(|e| scene.drawable(e)).filter_map(|e| match &e.payload {
                wce_scene::render_scene::Payload::VfxEmitter(v) => {
                    let mut world = e.world;
                    for (row, o) in world.m.iter_mut().zip([o.x, o.y, o.z]) {
                        row[3] = (row[3] - o) * units;
                    }

                    Some(crate::particles::WorldEffect { key: u64::from(e.id), effect: v.effect.clone(), world, scale: v.scale * units, mesh: v.mesh.clone() })
                }

                _ => None,
            }).collect::<Vec<_>>();
            if self.game == game::RenderGame::Rome2 {
                self.vfx_rome2.set_world_effects(&effects);
            } else {
                self.particles.set_world_effects(&effects);
            }
        }

        let files = self.files.clone();
        let read: &FileReader = &|path: &str| files.read(path);
        // Rome 2's effects: CPU particles drawn by the shaders the game generated per effect (vfx_rome2).
        // TODO(rome2): its weather particles (the WEATHER definition's vfx) are not drawn yet.
        if self.game == game::RenderGame::Rome2 {
            wce_prof!("render.particles");
            self.vfx_rome2.render(&frame, &mut self.lib, read, &mut self.cbs, self.wind.vfx_velocity);
        } else {
            wce_prof!("render.particles");
            self.particles.render(&frame, &mut self.lib, read, &mut self.cbs);
        }
        self.ctx.set_viewports(&[main_vp]);

        null_ps_srvs(&self.ctx, 16);
        let dt = self.clock.dt;
        self.gpu_timer.mark("tonemap + post");
        self.pass_tonemap(&env, dt);
        if self.settings.grain {
            self.advance_grain(dt);
        }
        if self.settings.view == View::Final {
            self.pass_to_screen(&env);
            if self.settings.lens_flare {
                self.pass_lens_flare(&env, (cam.eye - o) * self.camera.units, time_s);
            }
        }

        self.gpu_timer.mark("display + overlay");

        // Final: copies final_ with alpha = 1 for the viewport; scaled, into the scene's size first (the overlays test the scene's depth),
        // enlarged into the target after them.
        let mut scaled = ((width, height) != (out_w, out_h)).then(|| {
            let mut t = self.scaled.take().unwrap_or_default();
            if t.width() != width || t.height() != height {
                t.create(&self.dev, width, height);
            }

            t
        });
        let display = scaled.as_ref().filter(|t| t.valid()).unwrap_or(target);
        self.pass_display(display);
        if self.settings.view == View::Final && !self.settings.game_view {
            {
                wce_prof!("render.tint");
                self.pass_tint(scene, display);
            }

            {
                wce_prof!("render.selection_outline");
                self.pass_selection_outline(scene, display);
            }

            wce_prof!("render.overlay");
            self.pass_overlay(scene, display);
        }

        if let Some(t) = scaled.take() {
            wce_prof!("render.nis");
            self.ctx.set_render_targets(&[], None);
            let done = match t.srv() {
                Some(srv) => self.nis.upscale(&self.dev, &self.ctx, srv, (width, height), target, self.settings.sharpness),
                None => Ok(false),
            };

            match done {
                Ok(true) => {}
                Ok(false) => self.nis_failed = true,
                Err(e) => {
                    self.nis_failed = true;
                    self.error(format!("the render scale is off: {}", e.0));
                }
            }

            self.scaled = Some(t);
        }

        self.gpu_timer.end_frame();
        null_ps_srvs(&self.ctx, 16);
        self.ctx.set_render_targets(&[], None);
    }
}

/// Maps a staging texture's subresource for reading; `None` when it isn't ready (with `wait` false) or fails.
pub(crate) fn map_read<'a>(ctx: &'a ID3D11DeviceContext, tex: &ID3D11Texture2D, sub: u32, wait: bool) -> Option<Mapped<'a>> {
    ctx.map(tex, sub, D3D11_MAP_READ, if wait { 0 } else { D3D11_MAP_FLAG_DO_NOT_WAIT as u32 }).ok()
}

/// A tuple of options that is only taken when a condition holds.
pub(crate) trait IntoIterIf: Sized {
    fn into_iter_if(self, condition: bool) -> Self;
}

impl<A, B> IntoIterIf for (Option<A>, Option<B>) {
    fn into_iter_if(self, condition: bool) -> Self {
        if condition { self } else { (None, None) }
    }
}

impl<A, B, C> IntoIterIf for (Option<A>, Option<B>, Option<C>) {
    fn into_iter_if(self, condition: bool) -> Self {
        if condition { self } else { (None, None, None) }
    }
}
