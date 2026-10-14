//! The battle renderer, built from the game's own shaders. Pass order and shader choices follow the game; anything not yet confirmed
//! against it is marked ASSUMPTION.

use std::collections::{BTreeMap, HashMap};
use std::ffi::CString;
use std::rc::Rc;
use std::sync::Arc;

use wce_d3d11::*;
use wce_foundation::math::{Float3, Float4x4, inverse, length, look_at_lh, perspective_lh};
use wce_foundation::wce_prof;
use wce_scene::camera::Camera;
use wce_scene::clouds::CloudField;
use wce_scene::environment::{EnvironmentParams, GraphicsOptions};
use wce_scene::render_scene::{EntityId, Payload, RenderScene};
use windows_core::{Interface, PCSTR};

use crate::RenderError;
use crate::cbuffers::{ConstantBuffers, Var};
use crate::draw_set::{DrawSet, Frustum, View as DrawView, Visible};
use crate::environment_apply::apply_environment;
use crate::files::{Files, shared_reader};
use crate::gpu_timer::GpuTimer;
use crate::model_cache::{ModelCache, RigidInstance, State, TEX_SLOTS, TextureCache, gpu_texture_name};
use crate::particles::{Frame, ParticleSystem};
use crate::render_target::RenderTarget;
use crate::shader_lib::{FileReader, GameShader, ShaderLibrary, bind_shader};
use crate::wind::WindSim;

mod editor;
mod passes;
mod terrain;
mod water;

// Shader choices: the highest-quality dx11 variants.
pub(crate) const SKY_VS: &str = "dx11_sm_3_0_skydome.hlsl_vs_vs30_main";
pub(crate) const SKY_GEN_PS: &str = "dx11_sm_3_0_skydome.hlsl_ps_ps30_skygen";
pub(crate) const SKY_PS: &str = "dx11_sm_3_0_skydome.hlsl_ps_ps30_main";
pub(crate) const RIGID_VS: &str = "dx11_sm_4_0_rigidcommon.hlsl_vs_vs30_main";
pub(crate) const RIGID_GBUFFER_PS: &str = "dx11_sm_3_0_rigidstandard.hlsl_ps_ps30_gbuffer";
pub(crate) const RIGID_GBUFFER_ALPHA_PS: &str = "dx11_sm_3_0_rigidstandard.hlsl_ps_ps30_gbuffer_alpha_test";
pub(crate) const DIR_LIGHT_PS: &str = "dx11_enable_shadows_shadows_hardware_gather_5_0_sm_5_0_directionallight.hlsl_ps_ps30_slm_highdef";
pub(crate) const TM_DOWN_LUMA: &str = "dx11_sm_3_0_hdr_tone_mapper.hlsl_ps_ps_downsample_image_store_luma";
pub(crate) const TM_LUMA_STATS: &str = "dx11_sm_3_0_hdr_tone_mapper.hlsl_ps_ps_hdr_luma_to_quarter_size_luma_stats";
pub(crate) const TM_STATS_DOWN: &str = "dx11_sm_3_0_hdr_tone_mapper.hlsl_ps_ps_downsample_quarter_size_luma_stats";
pub(crate) const TM_POINTS: &str = "dx11_sm_3_0_hdr_tone_mapper.hlsl_ps_ps_determine_black_and_white_points";
pub(crate) const TM_HIGH_PASS: &str = "dx11_sm_3_0_hdr_tone_mapper.hlsl_ps_ps_high_pass_luminance_cutoff";
pub(crate) const TM_BLUR_X: &str = "dx11_sm_3_0_hdr_tone_mapper.hlsl_ps_ps_blur_x";
pub(crate) const TM_BLUR_Y: &str = "dx11_sm_3_0_hdr_tone_mapper.hlsl_ps_ps_blur_y";

/// The game's live path (dump: constant_tone_mapping_buffer = 0); linear out.
pub(crate) const TM_TONE_MAP: &str = "dx11_sm_3_0_hdr_tone_mapper.hlsl_ps_ps_basic_tone_mapper";
pub(crate) const TO_SCREEN_PS: &str = "dx11_sm_3_0_hdr_to_screen.hlsl_ps_ps30_main_high";

/// The same shader compiled with ENABLE_DISTORTION: uv += (distortion_sampler.xy - 0.498) * 0.02 (heat haze, lens drops).
pub(crate) const TO_SCREEN_DISTORT_PS: &str = "dx11_enable_distortion_sm_3_0_hdr_to_screen.hlsl_ps_ps30_main_high";

/// ENABLE_DEPTH_OF_FIELD variants: a 25-tap blur (sample_data_5x5) scaled by 1 - exp(-(z - min(focus, 500))^2 / focal^2), result =
/// lerp(frame, blurred, blur_kernel_scale).
pub(crate) const TO_SCREEN_DOF_PS: &str = "dx11_enable_depth_of_field_sm_3_0_hdr_to_screen.hlsl_ps_ps30_main_high";
pub(crate) const TO_SCREEN_DOF_DISTORT_PS: &str = "dx11_enable_distortion_enable_depth_of_field_sm_3_0_hdr_to_screen.hlsl_ps_ps30_main_high";
pub(crate) const FOG_PS: &str = "dx11_sm_3_0_battlefog.hlsl_ps_ps30_main";
pub(crate) const SHADOW_VS: &str = "dx11_sm_3_0_rigidcommon.hlsl_vs_vs30_shadow";
pub(crate) const SUN_VS: &str = "dx11_sm_3_0_sun.hlsl_vs_vs30_main";
pub(crate) const SUN_PS: &str = "dx11_sm_3_0_sun.hlsl_ps_ps30_main";
pub(crate) const SSAO_PS: &str = "dx11_sm_3_0_ssao.hlsl_ps_pixel_resolve";
pub(crate) const SSAO_BLUR_X_PS: &str = "dx11_sm_3_0_ssao.hlsl_ps_pixel_blur_x";
pub(crate) const SSAO_BLUR_Y_PS: &str = "dx11_sm_3_0_ssao.hlsl_ps_pixel_blur_y";
pub(crate) const CLOUD_VS: &str = "dx11_sm_3_0_cloud_plane.hlsl_vs_vs30_main";
pub(crate) const CLOUD_PS: &str = "dx11_sm_3_0_cloud_plane.hlsl_ps_ps30_main";
pub(crate) const SHADOW_PS: &str = "dx11_sm_3_0_rigidcommon.hlsl_ps_ps30_shadow";

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

/// Editor-side helpers (not game shaders): the full-screen triangle and the debug display pass.
const HELPER_HLSL: &str = r#"
struct VSOut { float4 pos : SV_Position; float2 uv : TEXCOORD0; };
VSOut fs_vs(uint id : SV_VertexID) {
    VSOut o;
    o.uv = float2((id << 1) & 2, id & 2);
    o.pos = float4(o.uv * float2(2, -2) + float2(-1, 1), 0, 1);
    return o;
}
// Same, with a second texcoord packed into the same register (ssao.hlsl pixel_resolve expects TEXCOORD1.zw).
struct VSOut2 { float4 pos : SV_Position; float2 uv : TEXCOORD0; float2 uv1 : TEXCOORD1; };
VSOut2 fs_vs2(uint id : SV_VertexID) {
    VSOut2 o;
    o.uv = float2((id << 1) & 2, id & 2);
    o.uv1 = o.uv;
    o.pos = float4(o.uv * float2(2, -2) + float2(-1, 1), 0, 1);
    return o;
}
cbuffer display : register(b0) { int mode; float exposure; float2 pad; };
Texture2D src : register(t0);
TextureCube cube : register(t1);
SamplerState smp : register(s0);
float4 display_ps(VSOut i) : SV_Target {
    float4 c = src.SampleLevel(smp, i.uv, 0);
    if (mode == 0) { // TEMPORARY until the game's tone mapper chain is wired: simple exposure + gamma
        float3 x = c.rgb * exposure;
        x = x / (1 + x);
        return float4(pow(saturate(x), 1 / 2.2), 1);
    }
    if (mode == 1) return float4(pow(saturate(c.rgb), 1 / 2.2), 1);
    if (mode == 2) return float4(c.rgb * c.rgb, 1);          // albedo is stored as sqrt
    if (mode == 3) return float4(c.rgb, 1);
    if (mode == 4) return float4(frac(c.rrr * 50), 1);
    float2 a = i.uv * float2(6.2831853, 3.14159265);
    float3 d = float3(sin(a.y) * sin(a.x), cos(a.y), sin(a.y) * cos(a.x));
    return float4(pow(saturate(cube.SampleLevel(smp, d, 0).rgb), 1 / 2.2), 1);
}
"#;

/// Compiles a helper shader.
pub(crate) fn compile(source: &str, name: &str, entry: &str, target: &str) -> Result<Vec<u8>, RenderError> {
    let (entry_c, target_c, name_c) = (CString::new(entry).unwrap_or_default(), CString::new(target).unwrap_or_default(), CString::new(name).unwrap_or_default());
    let (mut code, mut errors) = (None, None);
    // SAFETY: the source, names and outputs are alive during the call.
    let result = unsafe {
        D3DCompile(source.as_ptr().cast(), source.len(), PCSTR(name_c.as_ptr().cast()), None, None, PCSTR(entry_c.as_ptr().cast()),
            PCSTR(target_c.as_ptr().cast()), 0, 0, &raw mut code, &raw mut errors)
    };

    let bytes = |blob: &ID3D10Blob| {
        // SAFETY: the blob's buffer is valid for its size while the blob lives.
        unsafe { std::slice::from_raw_parts(blob.GetBufferPointer().cast::<u8>(), blob.GetBufferSize()).to_vec() }
    };

    match (result.is_ok(), code) {
        (true, Some(code)) => Ok(bytes(&code)),
        _ => {
            let message = errors.as_ref().map(|e| String::from_utf8_lossy(&bytes(e)).trim_end_matches('\0').to_owned()).unwrap_or_else(|| entry.to_owned());
            Err(RenderError(format!("{name} shader: {message}")))
        }
    }
}

/// An input element.
pub(crate) fn element(semantic: &'static std::ffi::CStr, index: u32, format: DXGI_FORMAT, slot: u32, offset: u32, instanced: bool) -> D3D11_INPUT_ELEMENT_DESC {
    D3D11_INPUT_ELEMENT_DESC {
        SemanticName: PCSTR(semantic.as_ptr().cast()),
        SemanticIndex: index,
        Format: format,
        InputSlot: slot,
        AlignedByteOffset: offset,
        InputSlotClass: if instanced { D3D11_INPUT_PER_INSTANCE_DATA } else { D3D11_INPUT_PER_VERTEX_DATA },
        InstanceDataStepRate: u32::from(instanced),
    }
}

/// skydome.hlsl vs30_main declares POSITION0 (xyz) and an unused TEXCOORD0 (xy); it is aliased onto the position.
pub(crate) fn sky_layout() -> Vec<D3D11_INPUT_ELEMENT_DESC> {
    vec![element(c"POSITION", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 0, false), element(c"TEXCOORD", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 0, false)]
}

/// STANDARD_RIGID_MESH_VERTEX (32 bytes) + 7 float4 per instance. Byte vectors are BGRA ("X/Z swapped").
pub(crate) fn rigid_layout() -> Vec<D3D11_INPUT_ELEMENT_DESC> {
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

/// A texture with its render target and shader resource views.
#[derive(Clone, Debug, Default)]
pub(crate) struct Target {
    pub tex: Option<ID3D11Texture2D>,
    pub rtv: Option<ID3D11RenderTargetView>,
    pub srv: Option<ID3D11ShaderResourceView>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SizedTarget {
    pub t: Target,
    pub w: u32,
    pub h: u32,
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

/// The cloud sprites of a plane on the GPU.
#[derive(Default)]
pub(crate) struct CloudGpu {
    pub signature: String,
    pub field: CloudField,
    pub instances: Option<ID3D11Buffer>,
    pub capacity: u32,
    pub count: u32,
}

/// The world matrix and bounds of a terrain entity (the selection outline).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct EntityBounds {
    pub world: Float4x4,
    pub bmin: Float3,
    pub bmax: Float3,
}

/// An item index of an entity without a single item.
pub(crate) const NO_ITEM: u32 = u32::MAX;

/// The renderer.
pub struct Renderer {
    pub(crate) dev: ID3D11Device,
    pub(crate) ctx: ID3D11DeviceContext,
    pub(crate) files: Arc<dyn Files>,
    pub(crate) cbs: ConstantBuffers,
    pub(crate) lib: ShaderLibrary,

    /// The view-projection of the frame being drawn (the overlay pass).
    pub(crate) view_proj: Float4x4,
    pub(crate) editor: editor::EditorGpu,
    pub(crate) water: water::WaterGpu,
    pub(crate) terrain: terrain::TerrainGpuState,
    pub(crate) shader_failures: Vec<String>,

    pub(crate) textures: Arc<TextureCache>,
    pub(crate) models: ModelCache,
    pub(crate) gpu_timer: GpuTimer,

    /// The mesh entities as culling items, rebuilt when the scene changes or a model finishes loading; and the terrain patches, in a set of
    /// their own (a brush stroke rebuilds only these, a few hundred items).
    pub(crate) draws: DrawSet,
    pub(crate) terrain_draws: DrawSet,
    pub(crate) terrain_bounds: HashMap<EntityId, EntityBounds>,
    pub(crate) draw_entities: usize,
    pub(crate) draw_meshes: usize,
    pub(crate) draws_waiting: usize,
    pub(crate) terrain_entities: usize,
    pub(crate) terrain_meshes: usize,
    pub(crate) draws_terrain_rev: u64,
    pub(crate) draws_scene: usize,
    pub(crate) draws_revision: u64,
    pub(crate) draws_model_revision: u64,
    pub(crate) moved_cursor: u64,

    /// Entity id -> item (single-item entities), NO_ITEM otherwise.
    pub(crate) item_of_entity: Vec<u32>,
    pub(crate) draw_stats: DrawStats,

    /// The view being drawn (camera, cascade, environment face), per set.
    pub(crate) vis: Visible,
    pub(crate) vis_terrain: Visible,

    /// Set by set_camera: what pass_gbuffer culls with.
    pub(crate) cur_vp: Float4x4,
    pub(crate) cur_eye: Float3,

    /// The angular size of one pixel (tangent).
    pub(crate) cur_px_ratio: f32,

    /// The per-view instance data: one dynamic vertex buffer used as a ring, one Map per view.
    pub(crate) instance_ring: Option<ID3D11Buffer>,
    pub(crate) ring_capacity: usize,
    pub(crate) ring_cursor: usize,
    pub(crate) ring_fresh: bool,
    pub(crate) ps_slots: HashMap<usize, [Option<u32>; TEX_SLOTS]>,
    pub(crate) particles: ParticleSystem,
    pub(crate) errors: Vec<String>,

    pub(crate) w: u32,
    pub(crate) h: u32,

    /// G-buffer channels 1..4 + the per-instance id.
    pub(crate) gb: [Target; 5],
    pub(crate) hdr: Target,

    /// hdr_to_screen's output (its alpha is the refraction mask).
    pub(crate) final_: Target,

    /// ao: the full-resolution resolve; ao_tmp / ao_blur: the half-resolution blur.
    pub(crate) ao: Target,
    pub(crate) ao_tmp: Target,
    pub(crate) ao_blur: Target,
    pub(crate) jitter_srv: Option<ID3D11ShaderResourceView>,
    pub(crate) ao_result: Option<ID3D11ShaderResourceView>,
    pub(crate) s_wrap_point: Option<ID3D11SamplerState>,
    pub(crate) depth_tex: Option<ID3D11Texture2D>,
    pub(crate) dsv: Option<ID3D11DepthStencilView>,

    pub(crate) sky_cube: Option<ID3D11Texture2D>,
    pub(crate) sky_face_rtv: [Option<ID3D11RenderTargetView>; 6],
    pub(crate) sky_cube_srv: Option<ID3D11ShaderResourceView>,

    /// The engine ambient (use_custom_ambient_cube_colours = false): the per-face averages of the environment cube (1x1 mip, read back through
    /// two alternating staging textures).
    pub(crate) amb_staging: [Option<ID3D11Texture2D>; 2],
    pub(crate) amb_faces: [[f32; 3]; 6],
    pub(crate) amb_valid: bool,
    pub(crate) amb_frame: u64,

    /// The environment cube ("EnvMapGen", fn 0x118e00e0): the whole scene rendered from 50 m above the origin into a cube; s_environment for
    /// reflections and, through its face averages, the ambient cube of non-custom lighting.
    pub(crate) env_cube: Option<ID3D11Texture2D>,
    pub(crate) env_face_rtv: [Option<ID3D11RenderTargetView>; 6],
    pub(crate) env_cube_srv: Option<ID3D11ShaderResourceView>,
    pub(crate) env_gb: [Target; 5],
    pub(crate) env_hdr: Target,
    pub(crate) env_dsv: Option<ID3D11DepthStencilView>,
    pub(crate) env_pass: bool,
    pub(crate) env_valid: bool,

    pub(crate) cube_vb: Option<ID3D11Buffer>,
    pub(crate) cube_ib: Option<ID3D11Buffer>,
    pub(crate) instance_ids_srv: Option<ID3D11ShaderResourceView>,
    pub(crate) white_srv: Option<ID3D11ShaderResourceView>,
    pub(crate) black_srv: Option<ID3D11ShaderResourceView>,
    pub(crate) overlay_none_srv: Option<ID3D11ShaderResourceView>,
    pub(crate) shadow_atlas: Target,
    pub(crate) shadow_dsv: Option<ID3D11DepthStencilView>,
    pub(crate) rs_shadow: Option<ID3D11RasterizerState>,
    pub(crate) fs_vs: Option<ID3D11VertexShader>,
    pub(crate) fs_vs2: Option<ID3D11VertexShader>,
    pub(crate) display_ps: Option<ID3D11PixelShader>,
    pub(crate) display_cb: Option<ID3D11Buffer>,

    // The tone mapping chain (hdr_tone_mapper.hlsl).
    pub(crate) quarter: SizedTarget,
    pub(crate) bloom_a: SizedTarget,
    pub(crate) bloom_b: SizedTarget,
    pub(crate) ldr: SizedTarget,

    /// The luma stats pyramid; the last is 1x1.
    pub(crate) stats: Vec<SizedTarget>,

    /// The black / white points, ping-pong.
    pub(crate) points: [SizedTarget; 2],
    pub(crate) points_staging: [Option<ID3D11Texture2D>; READBACK_RING],
    pub(crate) points_cur: usize,
    pub(crate) frame: u64,
    pub(crate) last_time: f32,
    pub(crate) frame_dt: f32,
    pub(crate) luma_black: f32,
    pub(crate) luma_white: f32,
    pub(crate) scurve_tex: Option<ID3D11Texture2D>,
    pub(crate) scurve_srv: Option<ID3D11ShaderResourceView>,
    pub(crate) scurve_last: [u16; 256 * 4],

    pub(crate) depth_staging: [Option<ID3D11Texture2D>; 2],
    pub(crate) depth_staging_w: u32,
    pub(crate) depth_staging_h: u32,
    pub(crate) depth_frame: u64,
    pub(crate) depth_range_valid: bool,
    pub(crate) depth_min: f32,
    pub(crate) depth_max: f32,

    pub(crate) s_wrap_aniso: Option<ID3D11SamplerState>,
    pub(crate) s_wrap_linear: Option<ID3D11SamplerState>,
    pub(crate) s_clamp_linear: Option<ID3D11SamplerState>,
    pub(crate) s_clamp_point: Option<ID3D11SamplerState>,
    pub(crate) s_shadow_cmp: Option<ID3D11SamplerState>,
    pub(crate) ds_gbuffer: Option<ID3D11DepthStencilState>,
    pub(crate) ds_sky: Option<ID3D11DepthStencilState>,
    pub(crate) ds_light: Option<ID3D11DepthStencilState>,
    pub(crate) ds_off: Option<ID3D11DepthStencilState>,
    pub(crate) rs_back: Option<ID3D11RasterizerState>,
    pub(crate) rs_none: Option<ID3D11RasterizerState>,
    pub(crate) bs_opaque: Option<ID3D11BlendState>,
    pub(crate) bs_alpha: Option<ID3D11BlendState>,
    pub(crate) bs_additive_alpha: Option<ID3D11BlendState>,
    pub(crate) sun_vb: Option<ID3D11Buffer>,
    pub(crate) cloud_quad_vb: Option<ID3D11Buffer>,
    pub(crate) cloud_dt: f32,
    pub(crate) wind: WindSim,
    pub(crate) cloud_gpu: Vec<CloudGpu>,
    pub(crate) sky_textures: BTreeMap<String, Option<ID3D11ShaderResourceView>>,
    pub(crate) sun_texture_path: String,

    /// Sun visibility for the lens flare (lense_flare_VS.sun_occlusion): occlusion queries of the sun disc, read back a few frames later.
    pub(crate) sun_query: [Option<ID3D11Query>; 4],
    pub(crate) sun_query_frame: u64,
    pub(crate) sun_visibility: f32,
    pub(crate) flare_vb: Option<ID3D11Buffer>,
    pub(crate) bs_add_one: Option<ID3D11BlendState>,
    pub(crate) bs_sun: Option<ID3D11BlendState>,
    pub(crate) sun_srv: Option<ID3D11ShaderResourceView>,

    pub debug_points: [f32; 4],
    pub debug_maps: i32,

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

    // Editor overlays: a line grid on y = 0, and the entities whose bounds are outlined (yellow, on top).
    pub show_grid: bool,
    pub grid_step: f32,
    pub grid_half_extent: f32,
    pub selection: Vec<EntityId>,

    /// Changes when an open vfx/ or weather/ XML is edited: the particles reload.
    pub vfx_revision: u64,
    pub cloud_seed: u32,

    // Battle state the engine feeds the weather shaders (not part of the environment files).

    /// wind_dir_random: the battle's random direction comes from this seed.
    pub wind_dir_seed: u32,

    /// weather_shared.g_season_type (snow dump: 3 in winter).
    pub season: i32,

    /// weather_shared.g_snow_mask (the battle map has snow).
    pub snow_cover: bool,
    pub view: View,
}

/// A target of a size and format.
pub(crate) fn make_target(dev: &ID3D11Device, w: u32, h: u32, fmt: DXGI_FORMAT) -> Target {
    let td = D3D11_TEXTURE2D_DESC {
        Width: w, Height: h, MipLevels: 1, ArraySize: 1, Format: fmt, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 }, Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE) as u32, CPUAccessFlags: 0, MiscFlags: 0,
    };

    let mut t = Target::default();
    // SAFETY: creation calls with a valid description.
    unsafe {
        let _ = dev.CreateTexture2D(&td, None, Some(&mut t.tex));
        if let Some(tex) = &t.tex {
            let _ = dev.CreateRenderTargetView(tex, None, Some(&mut t.rtv));
            let _ = dev.CreateShaderResourceView(tex, None, Some(&mut t.srv));
        }
    }

    t
}

/// An immutable buffer.
pub(crate) fn immutable_buffer(dev: &ID3D11Device, data: &[u8], bind: D3D11_BIND_FLAG) -> Option<ID3D11Buffer> {
    let desc = D3D11_BUFFER_DESC { ByteWidth: data.len() as u32, Usage: D3D11_USAGE_IMMUTABLE, BindFlags: bind as u32, CPUAccessFlags: 0, MiscFlags: 0, StructureByteStride: 0 };
    let init = D3D11_SUBRESOURCE_DATA { pSysMem: data.as_ptr().cast(), SysMemPitch: 0, SysMemSlicePitch: 0 };
    let mut buffer = None;
    // SAFETY: the data covers the buffer and lives during the call.
    let _ = unsafe { dev.CreateBuffer(&desc, Some(&init), Some(&mut buffer)) };
    buffer
}

/// A dynamic (CPU-written) buffer.
pub(crate) fn dynamic_buffer(dev: &ID3D11Device, bytes: usize, bind: D3D11_BIND_FLAG) -> Option<ID3D11Buffer> {
    let desc = D3D11_BUFFER_DESC { ByteWidth: bytes as u32, Usage: D3D11_USAGE_DYNAMIC, BindFlags: bind as u32, CPUAccessFlags: D3D11_CPU_ACCESS_WRITE as u32, MiscFlags: 0, StructureByteStride: 0 };
    let mut buffer = None;
    // SAFETY: a plain creation call.
    let _ = unsafe { dev.CreateBuffer(&desc, None, Some(&mut buffer)) };
    buffer
}

/// The bytes of floats.
pub(crate) fn f32_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// A 1x1 texture of one colour.
pub(crate) fn solid_srv(dev: &ID3D11Device, fmt: DXGI_FORMAT, texel: &[u8]) -> Option<ID3D11ShaderResourceView> {
    let td = D3D11_TEXTURE2D_DESC {
        Width: 1, Height: 1, MipLevels: 1, ArraySize: 1, Format: fmt, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 }, Usage: D3D11_USAGE_IMMUTABLE,
        BindFlags: D3D11_BIND_SHADER_RESOURCE as u32, CPUAccessFlags: 0, MiscFlags: 0,
    };

    let sd = D3D11_SUBRESOURCE_DATA { pSysMem: texel.as_ptr().cast(), SysMemPitch: texel.len() as u32, SysMemSlicePitch: 0 };
    let (mut tex, mut srv) = (None, None);
    // SAFETY: the texel lives during the calls.
    unsafe {
        let _ = dev.CreateTexture2D(&td, Some(&sd), Some(&mut tex));
        if let Some(tex) = tex {
            let _ = dev.CreateShaderResourceView(&tex, None, Some(&mut srv));
        }
    }

    srv
}

/// Binds one sampler to every sampler slot of a pixel shader.
pub(crate) fn ps_samplers(ctx: &ID3D11DeviceContext, s: &GameShader, smp: &Option<ID3D11SamplerState>) {
    for &slot in s.samplers.values() {
        // SAFETY: a binding call.
        unsafe { ctx.PSSetSamplers(slot, Some(std::slice::from_ref(smp))) };
    }
}

/// Unbinds the first `n` pixel shader resources.
pub(crate) fn null_ps_srvs(ctx: &ID3D11DeviceContext, n: usize) {
    let none = vec![None; n];
    // SAFETY: an unbinding call.
    unsafe { ctx.PSSetShaderResources(0, Some(&none)) };
}

impl Renderer {

    /// A renderer on a device, reading the game's files from `files`.
    ///
    /// # Errors
    ///
    /// Fails if the helper shaders don't compile.
    pub fn new(dev: &ID3D11Device, ctx: &ID3D11DeviceContext, files: Arc<dyn Files>) -> Result<Self, RenderError> {
        // Which GPU renders matters for reproducing pixels (two adapters: results differ between them).
        if let Ok(dxgi) = dev.cast::<IDXGIDevice>() {
            // SAFETY: plain queries of the device's adapter.
            unsafe {
                if let Some(info) = dxgi.GetAdapter().and_then(|a| a.cast::<IDXGIAdapter1>()).ok().and_then(|a| adapter_info(&a)) {
                    wce_foundation::log::info(format!("GPU: {}", info.name));
                }
            }
        }

        let reader = shared_reader(&files);
        let textures = Arc::new(TextureCache::new(dev, reader.clone()));
        let models = ModelCache::new(dev, reader, textures.clone());
        let particle_files = files.clone();
        let particles = ParticleSystem::new(dev, ctx, Box::new(move |path: &str| particle_files.read(path)));

        let mut r = Self {
            dev: dev.clone(),
            ctx: ctx.clone(),
            files,
            cbs: ConstantBuffers::default(),
            lib: ShaderLibrary::new(dev),
            view_proj: Float4x4::IDENTITY,
            editor: editor::EditorGpu::default(),
            water: water::WaterGpu::default(),
            terrain: terrain::TerrainGpuState::default(),
            shader_failures: vec![],
            textures,
            models,
            gpu_timer: GpuTimer::new(dev, ctx),
            draws: DrawSet::default(),
            terrain_draws: DrawSet::default(),
            terrain_bounds: HashMap::new(),
            draw_entities: 0,
            draw_meshes: 0,
            draws_waiting: 0,
            terrain_entities: 0,
            terrain_meshes: 0,
            draws_terrain_rev: 0,
            draws_scene: 0,
            draws_revision: 0,
            draws_model_revision: 0,
            moved_cursor: 0,
            item_of_entity: vec![],
            draw_stats: DrawStats::default(),
            vis: Visible::default(),
            vis_terrain: Visible::default(),
            cur_vp: Float4x4::IDENTITY,
            cur_eye: Float3::default(),
            cur_px_ratio: 0.001,
            instance_ring: None,
            ring_capacity: 0,
            ring_cursor: 0,
            ring_fresh: true,
            ps_slots: HashMap::new(),
            particles,
            errors: vec![],
            w: 0,
            h: 0,
            gb: Default::default(),
            hdr: Target::default(),
            final_: Target::default(),
            ao: Target::default(),
            ao_tmp: Target::default(),
            ao_blur: Target::default(),
            jitter_srv: None,
            ao_result: None,
            s_wrap_point: None,
            depth_tex: None,
            dsv: None,
            sky_cube: None,
            sky_face_rtv: Default::default(),
            sky_cube_srv: None,
            amb_staging: Default::default(),
            amb_faces: [[0.0; 3]; 6],
            amb_valid: false,
            amb_frame: 0,
            env_cube: None,
            env_face_rtv: Default::default(),
            env_cube_srv: None,
            env_gb: Default::default(),
            env_hdr: Target::default(),
            env_dsv: None,
            env_pass: false,
            env_valid: false,
            cube_vb: None,
            cube_ib: None,
            instance_ids_srv: None,
            white_srv: None,
            black_srv: None,
            overlay_none_srv: None,
            shadow_atlas: Target::default(),
            shadow_dsv: None,
            rs_shadow: None,
            fs_vs: None,
            fs_vs2: None,
            display_ps: None,
            display_cb: None,
            quarter: SizedTarget::default(),
            bloom_a: SizedTarget::default(),
            bloom_b: SizedTarget::default(),
            ldr: SizedTarget::default(),
            stats: vec![],
            points: Default::default(),
            points_staging: Default::default(),
            points_cur: 0,
            frame: 0,
            last_time: -1.0,
            frame_dt: 0.016,
            luma_black: 0.001,
            luma_white: 9.0,
            scurve_tex: None,
            scurve_srv: None,
            scurve_last: [0; 256 * 4],
            depth_staging: Default::default(),
            depth_staging_w: 0,
            depth_staging_h: 0,
            depth_frame: 0,
            depth_range_valid: false,
            depth_min: 1.0,
            depth_max: 781.0,
            s_wrap_aniso: None,
            s_wrap_linear: None,
            s_clamp_linear: None,
            s_clamp_point: None,
            s_shadow_cmp: None,
            ds_gbuffer: None,
            ds_sky: None,
            ds_light: None,
            ds_off: None,
            rs_back: None,
            rs_none: None,
            bs_opaque: None,
            bs_alpha: None,
            bs_additive_alpha: None,
            sun_vb: None,
            cloud_quad_vb: None,
            cloud_dt: 0.0,
            wind: WindSim::default(),
            cloud_gpu: vec![],
            sky_textures: BTreeMap::new(),
            sun_texture_path: String::new(),
            sun_query: Default::default(),
            sun_query_frame: 0,
            sun_visibility: 0.0,
            flare_vb: None,
            bs_add_one: None,
            bs_sun: None,
            sun_srv: None,
            debug_points: [0.0; 4],
            debug_maps: 0,
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
            show_grid: false,
            grid_step: 10.0,
            grid_half_extent: 200.0,
            selection: vec![],
            vfx_revision: 0,
            cloud_seed: 1,
            wind_dir_seed: 1,
            season: 0,
            snow_cover: false,
            view: View::Final,
        };

        r.make_states();
        r.create_resources()?;
        Ok(r)
    }

    fn create_resources(&mut self) -> Result<(), RenderError> {
        let dev = self.dev.clone();
        let vs = compile(HELPER_HLSL, "helpers", "fs_vs", "vs_5_0")?;
        let vs2 = compile(HELPER_HLSL, "helpers", "fs_vs2", "vs_5_0")?;
        let ps = compile(HELPER_HLSL, "helpers", "display_ps", "ps_5_0")?;

        // SAFETY: creation calls with valid descriptions and data that lives during them.
        unsafe {
            let _ = dev.CreateVertexShader(&vs, None, Some(&mut self.fs_vs));
            let _ = dev.CreateVertexShader(&vs2, None, Some(&mut self.fs_vs2));
            let _ = dev.CreatePixelShader(&ps, None, Some(&mut self.display_ps));
            let cb = D3D11_BUFFER_DESC { ByteWidth: 16, Usage: D3D11_USAGE_DEFAULT, BindFlags: D3D11_BIND_CONSTANT_BUFFER as u32, CPUAccessFlags: 0, MiscFlags: 0, StructureByteStride: 0 };
            let _ = dev.CreateBuffer(&cb, None, Some(&mut self.display_cb));
        }

        // The sky dome's geometry: a cube around the camera (the sky shaders only use the direction).
        let s = 10.0f32;
        let verts = [[-s, -s, -s], [s, -s, -s], [s, s, -s], [-s, s, -s], [-s, -s, s], [s, -s, s], [s, s, s], [-s, s, s]];
        let idx: [u16; 36] = [0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 3, 6, 2, 3, 7, 6, 0, 4, 7, 0, 7, 3, 1, 2, 6, 1, 6, 5];
        self.cube_vb = immutable_buffer(&dev, &f32_bytes(verts.as_flattened()), D3D11_BIND_VERTEX_BUFFER);
        self.cube_ib = immutable_buffer(&dev, &idx.iter().flat_map(|i| i.to_le_bytes()).collect::<Vec<_>>(), D3D11_BIND_INDEX_BUFFER);

        // The per-instance uint lookup read by rigidcommon vs30_main (t0, offset rigid_config_PS[4].w). Zeros for now.
        // SAFETY: creation calls with data that lives during them.
        unsafe {
            let zeros = vec![0u32; 1024];
            let td = D3D11_TEXTURE2D_DESC {
                Width: 1024, Height: 1, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R32_UINT, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_IMMUTABLE, BindFlags: D3D11_BIND_SHADER_RESOURCE as u32, CPUAccessFlags: 0, MiscFlags: 0,
            };

            let sd = D3D11_SUBRESOURCE_DATA { pSysMem: zeros.as_ptr().cast(), SysMemPitch: 4096, SysMemSlicePitch: 0 };
            let mut tex = None;
            let _ = dev.CreateTexture2D(&td, Some(&sd), Some(&mut tex));
            if let Some(tex) = tex {
                let _ = dev.CreateShaderResourceView(&tex, None, Some(&mut self.instance_ids_srv));
            }
        }

        self.white_srv = solid_srv(&dev, DXGI_FORMAT_R8G8B8A8_UNORM, &0xFFFF_FFFFu32.to_le_bytes());
        self.black_srv = solid_srv(&dev, DXGI_FORMAT_R8G8B8A8_UNORM, &0u32.to_le_bytes());

        // No overlay: colour 0, alpha 1 (the lighting of terrain pixels, material id bit 0, is overlay.rgb + lit * overlay.a).
        self.overlay_none_srv = solid_srv(&dev, DXGI_FORMAT_R8G8B8A8_UNORM, &0xFF00_0000u32.to_le_bytes());

        self.shadow_atlas = make_target(&dev, SHADOW_SIZE, SHADOW_SIZE, DXGI_FORMAT_R32_FLOAT);
        // SAFETY: creation calls with valid descriptions.
        unsafe {
            let td = D3D11_TEXTURE2D_DESC {
                Width: SHADOW_SIZE, Height: SHADOW_SIZE, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_D32_FLOAT, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT, BindFlags: D3D11_BIND_DEPTH_STENCIL as u32, CPUAccessFlags: 0, MiscFlags: 0,
            };

            let mut tex = None;
            let _ = dev.CreateTexture2D(&td, None, Some(&mut tex));
            if let Some(tex) = tex {
                let _ = dev.CreateDepthStencilView(&tex, None, Some(&mut self.shadow_dsv));
            }

            // Verified (rigid shader fn 0x116e5130, param_6 = 1 for rigidstandard: rasterizer +0x1a60 with cull field = 1): CULL_NONE, solid,
            // depth clip on, no depth bias (the engine's state objects hold D3D11 enums).
            let rd = D3D11_RASTERIZER_DESC { FillMode: D3D11_FILL_SOLID, CullMode: D3D11_CULL_NONE, DepthClipEnable: true.into(), ..Default::default() };
            let _ = dev.CreateRasterizerState(&rd, Some(&mut self.rs_shadow));
        }

        // The sun billboard: two triangles; POSITION.xy in view units (x sun_scale), TEXCOORD.xy.
        let d = SUN_MESH_SIZE;
        let sun = [[-d, -d, 0.0, 1.0], [-d, d, 0.0, 0.0], [d, d, 1.0, 0.0], [-d, -d, 0.0, 1.0], [d, d, 1.0, 0.0], [d, -d, 1.0, 1.0]];
        self.sun_vb = immutable_buffer(&dev, &f32_bytes(sun.as_flattened()), D3D11_BIND_VERTEX_BUFFER);

        // The lens flare / vignette quad (the engine's full-screen quad): POSITION xy in [-1, 1], COLOR, TEXCOORD0 uv.
        let q: [[f32; 10]; 6] = [
            [-1.0, -1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0, 1.0], [-1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0], [1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0],
            [-1.0, -1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0, 1.0], [1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0], [1.0, -1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        ];

        self.flare_vb = immutable_buffer(&dev, &f32_bytes(q.as_flattened()), D3D11_BIND_VERTEX_BUFFER);
        let qd = D3D11_QUERY_DESC { Query: D3D11_QUERY_OCCLUSION, MiscFlags: 0 };
        for query in &mut self.sun_query {
            // SAFETY: a plain creation call.
            let _ = unsafe { dev.CreateQuery(&qd, Some(query)) };
        }

        // The cloud sprite quad: POSITION xy in [-1, 1] (scaled by the sprite's angular size), TEXCOORD0 uv.
        let cq = [[-1.0f32, -1.0, 0.0, 1.0], [-1.0, 1.0, 0.0, 0.0], [1.0, 1.0, 1.0, 0.0], [-1.0, -1.0, 0.0, 1.0], [1.0, 1.0, 1.0, 0.0], [1.0, -1.0, 1.0, 1.0]];
        self.cloud_quad_vb = immutable_buffer(&dev, &f32_bytes(cq.as_flattened()), D3D11_BIND_VERTEX_BUFFER);

        // The HBAO jitter texture, 256^2 (g_JitterResolution in the dump). UNORM, expanded with *2-1 by the shader: x, y = direction rotation
        // (cos, sin of 2*pi*r / num_dir), z = step jitter. The engine's generator was not found; the texture only decorrelates the sample
        // directions and the blur passes remove the pattern, so the NVIDIA HBAO sample's content (which this shader mirrors) is used.
        let mut px = vec![0u8; 256 * 256 * 4];
        let mut seed = 12345u32;
        let mut rnd = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 24) as f32
        };

        for texel in px.as_chunks_mut::<4>().0 {
            let a = std::f32::consts::TAU * rnd() / 5.0;
            let j = rnd();
            for (c, v) in [a.cos(), a.sin(), j].into_iter().enumerate() {
                texel[c] = ((v * 0.5 + 0.5) * 255.0).round() as u8;
            }

            texel[3] = 255;
        }

        // SAFETY: creation calls with data that lives during them.
        unsafe {
            let td = D3D11_TEXTURE2D_DESC {
                Width: 256, Height: 256, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R8G8B8A8_UNORM, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_IMMUTABLE, BindFlags: D3D11_BIND_SHADER_RESOURCE as u32, CPUAccessFlags: 0, MiscFlags: 0,
            };

            let sd = D3D11_SUBRESOURCE_DATA { pSysMem: px.as_ptr().cast(), SysMemPitch: 256 * 4, SysMemSlicePitch: 0 };
            let mut tex = None;
            let _ = dev.CreateTexture2D(&td, Some(&sd), Some(&mut tex));
            if let Some(tex) = tex {
                let _ = dev.CreateShaderResourceView(&tex, None, Some(&mut self.jitter_srv));
            }

            let sm = D3D11_SAMPLER_DESC {
                Filter: D3D11_FILTER_MIN_MAG_MIP_POINT, AddressU: D3D11_TEXTURE_ADDRESS_WRAP, AddressV: D3D11_TEXTURE_ADDRESS_WRAP, AddressW: D3D11_TEXTURE_ADDRESS_WRAP,
                MaxLOD: D3D11_FLOAT32_MAX, ..Default::default()
            };

            let _ = dev.CreateSamplerState(&sm, Some(&mut self.s_wrap_point));

            // The sky cube map (the output of ps30_skygen), and the environment cube (EnvMapGen) with its 256^2 G-buffer / HDR / depth targets.
            let cd = D3D11_TEXTURE2D_DESC {
                Width: SKY_CUBE, Height: SKY_CUBE, MipLevels: 0, ArraySize: 6, Format: DXGI_FORMAT_R16G16B16A16_FLOAT, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT, BindFlags: (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE) as u32, CPUAccessFlags: 0,
                MiscFlags: (D3D11_RESOURCE_MISC_TEXTURECUBE | D3D11_RESOURCE_MISC_GENERATE_MIPS) as u32,
            };

            for (cube, rtvs, srv) in [(&mut self.sky_cube, &mut self.sky_face_rtv, &mut self.sky_cube_srv), (&mut self.env_cube, &mut self.env_face_rtv, &mut self.env_cube_srv)] {
                let _ = dev.CreateTexture2D(&cd, None, Some(cube));
                let Some(tex) = cube.as_ref() else { continue };
                for (f, rtv) in rtvs.iter_mut().enumerate() {
                    let mut rd = D3D11_RENDER_TARGET_VIEW_DESC { Format: cd.Format, ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2DARRAY, ..Default::default() };
                    rd.Anonymous.Texture2DArray.FirstArraySlice = f as u32;
                    rd.Anonymous.Texture2DArray.ArraySize = 1;
                    let _ = dev.CreateRenderTargetView(tex, Some(&rd), Some(rtv));
                }

                let _ = dev.CreateShaderResourceView(tex, None, Some(srv));
            }

            let n = SKY_CUBE;
            self.env_gb = [
                make_target(&dev, n, n, DXGI_FORMAT_R8G8B8A8_UNORM),
                make_target(&dev, n, n, DXGI_FORMAT_R8G8B8A8_UNORM),
                make_target(&dev, n, n, DXGI_FORMAT_R8G8B8A8_UNORM),
                make_target(&dev, n, n, DXGI_FORMAT_R32_FLOAT),
                make_target(&dev, n, n, DXGI_FORMAT_R32_UINT),
            ];

            self.env_hdr = make_target(&dev, n, n, DXGI_FORMAT_R16G16B16A16_FLOAT);
            let td = D3D11_TEXTURE2D_DESC {
                Width: n, Height: n, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_D24_UNORM_S8_UINT, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT, BindFlags: D3D11_BIND_DEPTH_STENCIL as u32, CPUAccessFlags: 0, MiscFlags: 0,
            };

            let mut tex = None;
            let _ = dev.CreateTexture2D(&td, None, Some(&mut tex));
            if let Some(tex) = tex {
                let _ = dev.CreateDepthStencilView(&tex, None, Some(&mut self.env_dsv));
            }

            let staging = |w: u32, array: u32, fmt: DXGI_FORMAT| D3D11_TEXTURE2D_DESC {
                Width: w, Height: 1, MipLevels: 1, ArraySize: array, Format: fmt, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 }, Usage: D3D11_USAGE_STAGING,
                BindFlags: 0, CPUAccessFlags: D3D11_CPU_ACCESS_READ as u32, MiscFlags: 0,
            };

            for s in &mut self.amb_staging {
                let _ = dev.CreateTexture2D(&staging(1, 6, DXGI_FORMAT_R16G16B16A16_FLOAT), None, Some(s));
            }

            // The black / white points: two 1x1 targets (previous / current) plus the CPU readback for the constant tone mapper.
            for p in &mut self.points {
                *p = SizedTarget { t: make_target(&dev, 1, 1, DXGI_FORMAT_R32G32B32A32_FLOAT), w: 1, h: 1 };
            }

            for s in &mut self.points_staging {
                let _ = dev.CreateTexture2D(&staging(1, 1, DXGI_FORMAT_R32G32B32A32_FLOAT), None, Some(s));
            }

            let sd = D3D11_TEXTURE2D_DESC {
                Width: 256, Height: 1, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R16G16B16A16_FLOAT, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT, BindFlags: D3D11_BIND_SHADER_RESOURCE as u32, CPUAccessFlags: 0, MiscFlags: 0,
            };

            let _ = dev.CreateTexture2D(&sd, None, Some(&mut self.scurve_tex));
            if let Some(tex) = &self.scurve_tex {
                let _ = dev.CreateShaderResourceView(tex, None, Some(&mut self.scurve_srv));
            }
        }

        // Faction colours: white = no tint on the test map.
        self.cbs.declare("rigid_faction_colours", 64, &BTreeMap::from([("faction_colours".to_owned(), Var { offset: 0, size: 60 })]));
        self.cbs.set_f("rigid_faction_colours", "faction_colours", &[1.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0]);
        Ok(())
    }

    fn make_states(&mut self) {
        let dev = self.dev.clone();
        // SAFETY: plain creation calls.
        unsafe {
            let mut s = D3D11_SAMPLER_DESC {
                Filter: D3D11_FILTER_ANISOTROPIC, AddressU: D3D11_TEXTURE_ADDRESS_WRAP, AddressV: D3D11_TEXTURE_ADDRESS_WRAP, AddressW: D3D11_TEXTURE_ADDRESS_WRAP,
                MaxAnisotropy: 8, MaxLOD: D3D11_FLOAT32_MAX, ..Default::default()
            };

            let _ = dev.CreateSamplerState(&s, Some(&mut self.s_wrap_aniso));

            // The engine's sampler objects store D3D11 enums (filter 21 = MIN_MAG_MIP_LINEAR, address 1 = WRAP, 3 = CLAMP).
            s.Filter = D3D11_FILTER_MIN_MAG_MIP_LINEAR;
            let _ = dev.CreateSamplerState(&s, Some(&mut self.s_wrap_linear));
            s.AddressU = D3D11_TEXTURE_ADDRESS_CLAMP;
            s.AddressV = D3D11_TEXTURE_ADDRESS_CLAMP;
            s.AddressW = D3D11_TEXTURE_ADDRESS_CLAMP;
            let _ = dev.CreateSamplerState(&s, Some(&mut self.s_clamp_linear));
            s.Filter = D3D11_FILTER_MIN_MAG_MIP_POINT;
            let _ = dev.CreateSamplerState(&s, Some(&mut self.s_clamp_point));
            s.Filter = D3D11_FILTER_COMPARISON_MIN_MAG_LINEAR_MIP_POINT;
            s.ComparisonFunc = D3D11_COMPARISON_LESS_EQUAL;
            let _ = dev.CreateSamplerState(&s, Some(&mut self.s_shadow_cmp));

            let replace = D3D11_DEPTH_STENCILOP_DESC { StencilFailOp: D3D11_STENCIL_OP_KEEP, StencilDepthFailOp: D3D11_STENCIL_OP_KEEP, StencilPassOp: D3D11_STENCIL_OP_REPLACE, StencilFunc: D3D11_COMPARISON_ALWAYS };
            let mut d = D3D11_DEPTH_STENCIL_DESC {
                DepthEnable: true.into(), DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ALL, DepthFunc: D3D11_COMPARISON_LESS_EQUAL, StencilEnable: true.into(),
                StencilReadMask: 0xFF, StencilWriteMask: 0xFF, FrontFace: replace, BackFace: replace,
            };

            // Geometry marks stencil = 1.
            let _ = dev.CreateDepthStencilState(&d, Some(&mut self.ds_gbuffer));

            // The sky at z = w only where nothing was drawn.
            d.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ZERO;
            d.StencilEnable = false.into();
            let _ = dev.CreateDepthStencilState(&d, Some(&mut self.ds_sky));

            // Lights only on geometry.
            d.DepthEnable = false.into();
            d.StencilEnable = true.into();
            let equal = D3D11_DEPTH_STENCILOP_DESC { StencilPassOp: D3D11_STENCIL_OP_KEEP, StencilFunc: D3D11_COMPARISON_EQUAL, ..replace };
            d.FrontFace = equal;
            d.BackFace = equal;
            let _ = dev.CreateDepthStencilState(&d, Some(&mut self.ds_light));
            d.StencilEnable = false.into();
            let _ = dev.CreateDepthStencilState(&d, Some(&mut self.ds_off));

            let mut r = D3D11_RASTERIZER_DESC { FillMode: D3D11_FILL_SOLID, CullMode: D3D11_CULL_BACK, DepthClipEnable: true.into(), ..Default::default() };
            let _ = dev.CreateRasterizerState(&r, Some(&mut self.rs_back));
            r.CullMode = D3D11_CULL_NONE;
            let _ = dev.CreateRasterizerState(&r, Some(&mut self.rs_none));
        }

        let all = D3D11_COLOR_WRITE_ENABLE_ALL as u8;
        self.bs_opaque = crate::particles::blend_state(&dev, D3D11_RENDER_TARGET_BLEND_DESC { RenderTargetWriteMask: all, ..Default::default() });

        // Verified (BATTLE_FOG_SHADER fn 0x116c96c0 blend object +0x1cc; vignette +0x71f): SRC_ALPHA / INV_SRC_ALPHA, add, on colour and alpha.
        let alpha = D3D11_RENDER_TARGET_BLEND_DESC {
            BlendEnable: true.into(), SrcBlend: D3D11_BLEND_SRC_ALPHA, DestBlend: D3D11_BLEND_INV_SRC_ALPHA, BlendOp: D3D11_BLEND_OP_ADD,
            SrcBlendAlpha: D3D11_BLEND_SRC_ALPHA, DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA, BlendOpAlpha: D3D11_BLEND_OP_ADD, RenderTargetWriteMask: all,
        };

        self.bs_alpha = crate::particles::blend_state(&dev, alpha);
        self.bs_additive_alpha = crate::particles::blend_state(&dev, D3D11_RENDER_TARGET_BLEND_DESC {
            SrcBlendAlpha: D3D11_BLEND_ZERO, DestBlendAlpha: D3D11_BLEND_ONE, DestBlend: D3D11_BLEND_ONE, ..alpha
        });

        self.bs_sun = crate::particles::blend_state(&dev, alpha);

        // The lens flare sprites (fn 0x116de600, blend object +0x6da): ONE / ONE add on colour and alpha.
        self.bs_add_one = crate::particles::blend_state(&dev, D3D11_RENDER_TARGET_BLEND_DESC {
            SrcBlend: D3D11_BLEND_ONE, DestBlend: D3D11_BLEND_ONE, SrcBlendAlpha: D3D11_BLEND_ONE, DestBlendAlpha: D3D11_BLEND_ONE, ..alpha
        });
    }

    // ---- Public accessors.

    /// Mesh and terrain entities that are ready to draw.
    pub fn draw_count(&self) -> usize {
        self.draw_entities + self.terrain_entities
    }

    /// Their LOD 0 meshes (terrain: patches).
    pub fn mesh_count(&self) -> usize {
        self.draw_meshes + self.terrain_meshes
    }

    pub fn draw_stats(&self) -> &DrawStats {
        &self.draw_stats
    }

    /// The game shaders that couldn't be loaded (name: reason), newest first: the editor shows them where a pass would otherwise silently draw nothing.
    pub fn shader_failures(&self) -> &[String] {
        &self.shader_failures
    }

    pub fn sun_visibility(&self) -> f32 {
        self.sun_visibility
    }

    pub fn apply_graphics_options(&mut self, o: &GraphicsOptions) {
        self.vignette = o.vignette;
        self.ssao = o.ssao;
        self.distortion = o.distortion;
        self.depth_of_field = o.depth_of_field > 0;
        self.shadow_quality = o.shadow_quality;
    }

    pub fn wind(&self) -> &WindSim {
        &self.wind
    }

    pub fn cloud_sprites(&self) -> usize {
        self.cloud_gpu.iter().map(|p| p.count as usize).sum()
    }

    pub fn luma_black(&self) -> f32 {
        self.luma_black
    }

    pub fn luma_white(&self) -> f32 {
        self.luma_white
    }

    pub fn engine_ambient_faces(&self) -> String {
        self.amb_faces.iter().map(|f| format!("({:.6} {:.6} {:.6}) ", f[0], f[1], f[2])).collect()
    }

    /// Forgets what earlier frames left behind (auto exposure, tone mapper limits, frame counter, clock), so the next frames depend on the scene only.
    pub fn restart_temporal(&mut self) {
        self.frame = 0;
        self.last_time = -1.0;
        self.luma_black = 0.001;
        self.luma_white = 9.0;
        self.env_valid = false;
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
    pub(crate) fn layout(&mut self, vs: &GameShader, id: &str, elements: &[D3D11_INPUT_ELEMENT_DESC]) -> Option<ID3D11InputLayout> {
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
        if w == self.w && h == self.h {
            return;
        }

        self.w = w;
        self.h = h;
        let dev = self.dev.clone();

        // Verified formats (the engine's TTF_* enum = the order of its name strings): G-buffer channels 1-3 TTF_A8R8G8B8 (fn 0x118f4b90), the
        // depth channel TTF_R32F (R16F on low settings, fn 0x118f62b0), HDR light accumulation and the dynamic tone-map view TTF_A16B16G16R16F
        // (fns 0x118f4a80, 0x117659b0). gb[4] is editor-only (instance ids).
        self.gb = [
            make_target(&dev, w, h, DXGI_FORMAT_R8G8B8A8_UNORM),  // normal (cube-normalised, *0.5+0.5), gloss
            make_target(&dev, w, h, DXGI_FORMAT_R8G8B8A8_UNORM),  // sqrt(albedo), .w mask
            make_target(&dev, w, h, DXGI_FORMAT_R8G8B8A8_UNORM),  // specular colour, .w material id / 255
            make_target(&dev, w, h, DXGI_FORMAT_R32_FLOAT),       // z / w
            make_target(&dev, w, h, DXGI_FORMAT_R32_UINT),        // per-instance value (uint output, debug layer)
        ];

        self.hdr = make_target(&dev, w, h, DXGI_FORMAT_R16G16B16A16_FLOAT);
        self.final_ = make_target(&dev, w, h, DXGI_FORMAT_R8G8B8A8_UNORM);

        // SSAOX_RESOLVE / BLUR_X / BLUR_Y RTs: TTF_R16F (fn 0x11766de0); the blur at half resolution (dump).
        self.ao = make_target(&dev, w, h, DXGI_FORMAT_R16_FLOAT);
        self.ao_tmp = make_target(&dev, (w / 2).max(1), (h / 2).max(1), DXGI_FORMAT_R16_FLOAT);
        self.ao_blur = make_target(&dev, (w / 2).max(1), (h / 2).max(1), DXGI_FORMAT_R16_FLOAT);

        let sized = |tw: u32, th: u32, f: DXGI_FORMAT| {
            let (w, h) = (tw.max(1), th.max(1));
            SizedTarget { t: make_target(&dev, w, h, f), w, h }
        };

        let (qw, qh) = (w.div_ceil(4), h.div_ceil(4));
        self.quarter = sized(qw, qh, DXGI_FORMAT_R16G16B16A16_FLOAT);  // rgb + luma in .w
        self.bloom_a = sized(qw, qh, DXGI_FORMAT_R16G16B16A16_FLOAT);
        self.bloom_b = sized(qw, qh, DXGI_FORMAT_R16G16B16A16_FLOAT);
        self.ldr = sized(w, h, DXGI_FORMAT_R16G16B16A16_FLOAT);        // tone-mapped, linear
        self.stats.clear();
        let (mut sw, mut sh) = (qw, qh);
        loop {
            sw = sw.div_ceil(4);
            sh = sh.div_ceil(4);
            self.stats.push(sized(sw, sh, DXGI_FORMAT_R32G32B32A32_FLOAT));  // min, max, avg, max
            if sw <= 1 && sh <= 1 {
                break;
            }
        }

        let td = D3D11_TEXTURE2D_DESC {
            Width: w, Height: h, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_D24_UNORM_S8_UINT, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT, BindFlags: D3D11_BIND_DEPTH_STENCIL as u32, CPUAccessFlags: 0, MiscFlags: 0,
        };

        self.depth_tex = None;
        self.dsv = None;
        // SAFETY: creation calls with a valid description.
        unsafe {
            let _ = dev.CreateTexture2D(&td, None, Some(&mut self.depth_tex));
            if let Some(tex) = &self.depth_tex {
                let _ = dev.CreateDepthStencilView(tex, None, Some(&mut self.dsv));
            }
        }
    }

    pub(crate) fn set_camera(&mut self, view_m: &Float4x4, proj: &Float4x4, eye: Float3, w: u32, h: u32, time_s: f32) {
        let vp = *proj * *view_m;
        self.cur_vp = vp;
        self.cur_eye = eye;
        self.cur_px_ratio = 1.0 / (0.5 * h as f32 * proj.m[1][1]);
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
    }

    /// The full-screen triangle with a pixel shader.
    pub(crate) fn fullscreen(&self, ps: &GameShader) {
        // SAFETY: drawing calls with objects of this device.
        unsafe {
            self.ctx.IASetInputLayout(None);
            self.ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST));
            self.ctx.VSSetShader(self.fs_vs.as_ref(), None);
        }

        self.bind(ps);
        // SAFETY: a drawing call.
        unsafe { self.ctx.Draw(3, 0) };
    }

    pub(crate) fn fullscreen_to(&mut self, rtv: &Option<ID3D11RenderTargetView>, w: u32, h: u32, ps: &GameShader) {
        // SAFETY: state calls with objects of this device.
        unsafe {
            self.ctx.OMSetRenderTargets(Some(std::slice::from_ref(rtv)), None);
            self.ctx.RSSetViewports(Some(&[viewport(0.0, 0.0, w as f32, h as f32)]));
        }

        self.cbs.set_f("render_target_dims_buffer", "g_render_target_recip_size", &[1.0 / w as f32, 1.0 / h as f32]);
        self.upload();
        self.fullscreen(ps);
        null_ps_srvs(&self.ctx, 8);
    }

    /// The texture slots of a pixel shader's material textures.
    pub(crate) fn slots_of(&mut self, ps: &Rc<GameShader>) -> [Option<u32>; TEX_SLOTS] {
        *self.ps_slots.entry(Rc::as_ptr(ps) as usize).or_insert_with(|| std::array::from_fn(|i| ps.textures.get(gpu_texture_name(i)).copied()))
    }

    /// Writes the instances of both sets into the ring with one Map (growing or wrapping the ring must not lose what the first set wrote);
    /// returns the first instance of each set.
    pub(crate) fn upload_instances(&mut self, sets: [&DrawSet; 2], viss: [&Visible; 2]) -> [u32; 2] {
        wce_prof!("render.upload_instances");
        let (n0, n1) = (viss[0].instances(), viss[1].instances());
        let n = n0 + n1;
        if n == 0 {
            return [0, 0];
        }

        if n > self.ring_capacity {
            self.ring_capacity = (n * 2).max(1 << 16);
            self.instance_ring = dynamic_buffer(&self.dev, self.ring_capacity * size_of::<RigidInstance>(), D3D11_BIND_VERTEX_BUFFER);
            if self.instance_ring.is_none() {
                self.error("cannot create the instance buffer".to_owned());
                self.ring_capacity = 0;
                return [0, 0];
            }

            self.ring_cursor = 0;
            self.ring_fresh = true;
        }

        let mut discard = self.ring_fresh;
        if self.ring_cursor + n > self.ring_capacity {
            self.ring_cursor = 0;
            discard = true;
        }

        let Some(ring) = self.instance_ring.clone() else { return [0, 0] };
        let mut ms = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: the ring is written within its capacity while mapped.
        unsafe {
            if self.ctx.Map(&ring, 0, if discard { D3D11_MAP_WRITE_DISCARD } else { D3D11_MAP_WRITE_NO_OVERWRITE }, 0, Some(&mut ms)).is_err() {
                return [0, 0];
            }

            let dst = std::slice::from_raw_parts_mut(ms.pData.cast::<RigidInstance>().add(self.ring_cursor), n);
            let (first, second) = dst.split_at_mut(n0);
            sets[0].gather(viss[0], first);
            sets[1].gather(viss[1], second);
            self.ctx.Unmap(&ring, 0);
        }

        self.ring_fresh = false;
        let firsts = [self.ring_cursor as u32, (self.ring_cursor + n0) as u32];
        self.ring_cursor += n;
        firsts
    }

    pub(crate) fn camera_view(&self, lod_scale_factor: f32) -> DrawView {
        DrawView {
            frustum: Frustum::from_view_projection(&self.cur_vp),
            eye: self.cur_eye,
            min_ratio: self.min_pixels * self.cur_px_ratio,
            lod_scale: self.lod_scale * lod_scale_factor,
            ..Default::default()
        }
    }

    pub fn wind_direction_degrees(&self, env: &EnvironmentParams) -> f32 {
        if !env.wind_dir_random {
            return env.wind_dir_degrees;
        }

        let mut seed = self.wind_dir_seed;
        WindSim::random_direction(&mut seed)
    }

    /// The WEATHER node's per-frame update (fn 0x18ee0c0): the wind simulation, weather_shared, wind_VS, wind_PS.
    fn update_weather(&mut self, env: &EnvironmentParams, time_s: f32) {
        self.particles.set_weather(&env.weather_definition, self.vfx_revision);
        let def = self.particles.effect().definition.clone();
        let degrees = self.wind_direction_degrees(env);
        self.wind.update(Some(&def), degrees, time_s, self.frame_dt);
        let (w, cbs) = (&self.wind, &mut self.cbs);
        cbs.set_f("weather_shared", "g_wind", &[w.smoothed.x, w.smoothed.y]);
        cbs.set_f("weather_shared", "g_wind_offset", &[w.offset.x, w.offset.y]);
        cbs.set_f("weather_shared", "g_snowing_intensity", &[env.snowing_intensity]);
        cbs.set_i("weather_shared", "g_weather_type", def.type_index());
        cbs.set_i("weather_shared", "g_season_type", self.season);
        cbs.set_f("weather_shared", "g_snow_mask", &[if self.snow_cover { 1.0 } else { 0.0 }]);
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

    /// The terrain patches of visible terrain entities (their own draw set: a brush stroke only rebuilds this one).
    fn build_terrain_draws(&mut self, scene: &RenderScene) {
        wce_prof!("render.build_terrain_draws");
        self.draws_terrain_rev = scene.terrain_revision();
        self.terrain_draws.clear();
        self.terrain_bounds.clear();
        self.terrain_entities = 0;
        self.terrain_meshes = 0;
        self.models.prune_terrain(scene);
        for e in scene.entities() {
            let Payload::Terrain(t) = &e.payload else { continue };
            if !scene.drawable(e) {
                continue;
            }

            let Some(tg) = self.models.terrain(e.id, t) else { continue };
            let mut eb = EntityBounds { world: e.world, bmin: Float3::new(1e30, 1e30, 1e30), bmax: Float3::new(-1e30, -1e30, -1e30) };
            for p in tg.patches.iter().filter(|p| !p.meshes.is_empty()) {
                self.terrain_draws.add(p, &e.world, None, t.casts_shadow, t.lod, e.id);
                self.terrain_meshes += 1;
                eb.bmin = Float3::new(eb.bmin.x.min(p.bmin.x), eb.bmin.y.min(p.bmin.y), eb.bmin.z.min(p.bmin.z));
                eb.bmax = Float3::new(eb.bmax.x.max(p.bmax.x), eb.bmax.y.max(p.bmax.y), eb.bmax.z.max(p.bmax.z));
            }

            self.terrain_bounds.insert(e.id, eb);
            self.terrain_entities += 1;
        }
    }

    /// The mesh entities of visible layers, as culling items. Models still loading (live runs) are requested nearest first and picked up
    /// when they finish; fixed-step runs wait for every model so that frame N is the same every run.
    pub(crate) fn build_draws(&mut self, scene: &RenderScene, eye: Float3) {
        let scene_id = scene as *const RenderScene as usize;
        let same = self.draws_scene == scene_id && self.draws_revision == scene.structure_revision();
        if same && (self.draws_waiting == 0 || self.models.revision() == self.draws_model_revision) {
            // Nothing structural changed: patch the entities that only moved (a dragged building) instead of rebuilding 100 000 items.
            let mut rebuild_terrain = self.draws_terrain_rev != scene.terrain_revision();
            if let Some(moved) = scene.moved_since(&mut self.moved_cursor) {
                wce_prof!("render.patch_moved");
                for id in moved {
                    let Some(e) = scene.find(id) else { continue };
                    if matches!(e.payload, Payload::Terrain(_)) {
                        rebuild_terrain = true;
                        continue;
                    }

                    if let Some(&item) = self.item_of_entity.get(id as usize).filter(|&&item| item != NO_ITEM) {
                        self.draws.set_world(item, &e.world);
                    }
                }
            }

            if rebuild_terrain {
                self.build_terrain_draws(scene);
            }

            return;
        }

        wce_prof!("render.build_draws");
        self.draws_scene = scene_id;
        self.draws_revision = scene.structure_revision();
        self.draws_model_revision = self.models.revision();

        // Everything is rebuilt from the current state: forget what was logged.
        let _ = scene.moved_since(&mut self.moved_cursor);
        self.draws.clear();
        self.item_of_entity = vec![NO_ITEM; scene.entities().last().map_or(0, |e| e.id as usize + 1)];
        self.draw_entities = 0;
        self.draw_meshes = 0;
        self.draws_waiting = 0;
        self.build_terrain_draws(scene);

        // Request every model first: the loads run in parallel, then wait once.
        if self.deterministic {
            for e in scene.entities() {
                if let Payload::MeshInstance(mi) = &e.payload
                    && scene.drawable(e)
                {
                    self.models.try_get(&mi.model, 0.0);
                }
            }

            self.models.wait_all();
        }

        self.draws.reserve(scene.entities().len());

        // One cache lookup (and one lock) per distinct model per build.
        let mut resolved = HashMap::<&str, Option<Arc<crate::model_cache::GpuModel>>>::new();
        for e in scene.entities() {
            let Payload::MeshInstance(mi) = &e.payload else { continue };
            if !scene.drawable(e) {
                continue;
            }

            let m = resolved.entry(&mi.model).or_insert_with(|| {
                let d = Float3::new(e.world.m[0][3] - eye.x, e.world.m[1][3] - eye.y, e.world.m[2][3] - eye.z);
                self.models.try_get(&mi.model, length(d))
            }).clone();

            let Some(m) = m else {
                if self.models.state(&mi.model) != State::Failed {
                    self.draws_waiting += 1;
                }

                continue;
            };

            let params: [f32; 16] = std::array::from_fn(|i| [mi.p4, mi.p5, mi.p6, mi.p7][i / 4][i % 4]);
            let item = self.draws.add(&m, &e.world, Some(&params), mi.casts_shadow, mi.lod, e.id);
            if let Some(slot) = self.item_of_entity.get_mut(e.id as usize) {
                *slot = item;
            }

            self.draw_entities += 1;
            self.draw_meshes += m.meshes.len();
        }

        self.draw_stats.items = self.draws.items() + self.terrain_draws.items();
        self.draw_stats.batches = self.draws.batches() + self.terrain_draws.batches();
        self.draw_stats.models = self.draws.models() + self.terrain_draws.models();
        self.draw_stats.waiting_models = self.draws_waiting;
    }

    /// Draws a scene (lit by its environment slot) as a view sees it into a target, at the target's size.
    pub fn render(&mut self, scene: &RenderScene, view_params: &ViewParams, target: &mut RenderTarget) {
        wce_prof!("render.total");
        let cam = view_params.camera;
        let time_s = view_params.time_s;
        let (width, height) = (target.width(), target.height());
        if width < 8 || height < 8 {
            return;
        }

        let env = scene.environment.resolved();

        // Fixed-step runs load every texture whole: frame N must be the same every run.
        self.textures.set_streaming(!self.deterministic);
        self.textures.pump();
        self.build_draws(scene, cam.eye);
        let waiting = self.draws_waiting;
        self.draw_stats = DrawStats { items: self.draw_stats.items, batches: self.draw_stats.batches, models: self.draw_stats.models, waiting_models: waiting, ..Default::default() };
        self.resize(width, height);
        self.gpu_timer.begin_frame();
        self.gpu_timer.mark("setup");

        // Every cbuffer this frame needs is declared before values are written.
        for s in [SKY_VS, SKY_GEN_PS, SKY_PS, RIGID_VS, RIGID_GBUFFER_PS, RIGID_GBUFFER_ALPHA_PS, DIR_LIGHT_PS, TM_DOWN_LUMA, TM_LUMA_STATS, TM_STATS_DOWN, TM_POINTS,
            TM_HIGH_PASS, TM_BLUR_X, TM_BLUR_Y, TM_TONE_MAP, TO_SCREEN_PS, FOG_PS, SHADOW_VS, SHADOW_PS, SUN_VS, SUN_PS, CLOUD_VS, CLOUD_PS, SSAO_PS, SSAO_BLUR_X_PS, SSAO_BLUR_Y_PS]
        {
            self.shader(s);
        }

        apply_environment(&env, &mut self.cbs);
        self.frame_dt = if self.last_time < 0.0 { 0.016 } else { (time_s - self.last_time).max(0.0) };

        // The clock restarted.
        if self.last_time >= 0.0 && time_s < self.last_time {
            self.wind.reset(WindSim::DEFAULT_SEED);
        }

        self.last_time = time_s;
        self.cloud_dt = self.frame_dt;
        self.update_weather(&env, time_s);

        self.gpu_timer.mark("sky cube");
        self.pass_sky_cube(&env, time_s);
        self.gpu_timer.mark("env cube");

        // The XML ambient while it renders.
        if self.frame.is_multiple_of(4) || !self.env_valid {
            wce_prof!("render.env_cube");
            self.render_env_cube(&env, time_s);
        }

        self.apply_engine_ambient(&env);

        let aspect = width as f32 / height as f32;
        self.gpu_timer.mark("shadows");
        {
            wce_prof!("render.shadows");
            self.pass_shadows(&cam, aspect, -env.to_sun(), time_s);
        }

        let view_m = look_at_lh(cam.eye, cam.target, Float3::new(0.0, 1.0, 0.0));
        let proj = perspective_lh(cam.fov_y, aspect, cam.znear, cam.zfar);
        self.set_camera(&view_m, &proj, cam.eye, width, height, time_s);
        self.view_proj = proj * view_m;
        self.upload();
        let main_vp = viewport(0.0, 0.0, width as f32, height as f32);
        // SAFETY: state calls with objects of this device.
        unsafe {
            self.ctx.RSSetViewports(Some(&[main_vp]));
            self.ctx.OMSetBlendState(self.bs_opaque.as_ref(), None, 0xFFFF_FFFF);
        }

        self.terrain.active = self.prepare_terrain_materials(scene);
        self.gpu_timer.mark("gbuffer");
        {
            wce_prof!("render.gbuffer");
            self.pass_gbuffer();
        }

        if self.terrain.active {
            self.gpu_timer.mark("terrain");
            self.pass_terrain_material(scene);
        }

        self.gpu_timer.mark("depth + ssao");
        self.analyse_depth(&cam);
        self.pass_ssao(&cam);
        self.gpu_timer.mark("sky/clouds/light/fog");
        // SAFETY: state calls with objects of this device.
        unsafe {
            self.ctx.RSSetViewports(Some(&[main_vp]));
            if let Some(rtv) = &self.hdr.rtv {
                self.ctx.ClearRenderTargetView(rtv, &[0.0; 4]);
            }
        }

        self.pass_sky_forward();
        self.pass_sun(&env, &view_m, &proj);
        self.pass_clouds(&env);
        self.pass_directional_light();
        self.gpu_timer.mark("sea");
        self.pass_water(scene, false);
        self.pass_fog();
        self.gpu_timer.mark("rivers");
        self.pass_water(scene, true);

        // The weather precipitation (vfx1 of the WEATHER definition).
        let frame = Frame {
            eye: cam.eye,
            view: view_m,
            time: time_s,
            dt: self.frame_dt,
            wind_velocity: self.wind.vfx_velocity,
            wind_variance: self.wind.vfx_variance,
            target: self.hdr.rtv.clone(),
            depth: self.dsv.clone(),
            depth_zw: self.gb[3].srv.clone(),
            sky: self.sky_cube_srv.clone(),
            width,
            height,
        };

        let files = self.files.clone();
        let read: &FileReader = &|path: &str| files.read(path);
        self.particles.render(&frame, &mut self.lib, read, &mut self.cbs);
        // SAFETY: state calls.
        unsafe { self.ctx.RSSetViewports(Some(&[main_vp])) };

        null_ps_srvs(&self.ctx, 16);
        let dt = self.frame_dt;
        self.gpu_timer.mark("tonemap + post");
        self.pass_tonemap(&env, dt);
        if self.view == View::Final {
            self.pass_to_screen(&env);
            if self.lens_flare {
                self.pass_lens_flare(&env, cam.eye, time_s);
            }
        }

        self.gpu_timer.mark("display + overlay");

        // Final: copies final_ with alpha = 1 for the viewport.
        self.pass_display(target);
        if self.view == View::Final {
            wce_prof!("render.overlay");
            self.pass_overlay(scene, target);
        }

        self.gpu_timer.end_frame();
        null_ps_srvs(&self.ctx, 16);
        // SAFETY: an unbinding call.
        unsafe { self.ctx.OMSetRenderTargets(None, None) };
    }
}

/// Maps a staging texture's subresource for reading; `None` when it isn't ready (with `wait` false) or fails.
///
/// # Safety
///
/// The texture must belong to the context's device; the caller unmaps it.
pub(crate) unsafe fn map_read(ctx: &ID3D11DeviceContext, tex: &ID3D11Texture2D, sub: u32, wait: bool) -> Option<D3D11_MAPPED_SUBRESOURCE> {
    let mut ms = D3D11_MAPPED_SUBRESOURCE::default();
    // SAFETY: as the caller ensures.
    let hr = unsafe { ctx.Map(tex, sub, D3D11_MAP_READ, if wait { 0 } else { D3D11_MAP_FLAG_DO_NOT_WAIT as u32 }, Some(&mut ms)) };
    hr.is_ok().then_some(ms)
}
