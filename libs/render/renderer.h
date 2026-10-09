#pragma once
// Battle renderer built from the game's own shaders. Pass order and shader choices are documented
// in docs/re/renderer.md; anything not yet confirmed against the game is marked there.
#include "foundation/math.h"
#include "render/draw_set.h"
#include "render/gpu_timer.h"
#include "render/model_cache.h"
#include "render/particles.h"
#include "render/render_target.h"
#include "render/shader_lib.h"
#include "render/wind.h"
#include "scene/camera.h"
#include "scene/environment.h"
#include "scene/render_scene.h"

#include <d3d11.h>
#include <wrl/client.h>

#include <map>
#include <unordered_map>
#include <memory>
#include <string>
#include <vector>

namespace wce {

class IVfs;

// What one frame needs besides the scene: the camera and the time (from the Clock).
struct ViewParams {
    Camera camera;
    float time_s = 0;
};

class Renderer {
public:
    enum class View { Final, HdrRaw, Albedo, Normal, Specular, Depth, SkyCube, Bloom, Quarter, ToneMapped, AO };
    float debug_points[4] = {}; int debug_maps = 0;
    size_t draw_count() const { return draw_entities_ + terrain_entities_; }      // mesh and terrain entities that are ready to draw
    size_t mesh_count() const { return draw_meshes_ + terrain_meshes_; }         // their LOD 0 meshes (terrain: patches)
    // What the last frame submitted (all views: camera, shadow cascades, environment cube).
    struct DrawStats {
        size_t items = 0, batches = 0, models = 0, waiting_models = 0;
        uint64_t views = 0, instances = 0, draw_calls = 0, triangles = 0, camera_instances = 0, camera_triangles = 0, shadow_instances = 0, shadow_triangles = 0, terrain_patches = 0;
    };
    const DrawStats& draw_stats() const { return draw_stats_; }
    float lod_scale = 1.0f;                // multiplies every LOD distance (the editor can trade detail for speed)
    float min_pixels = 0.75f;              // LOD items smaller than this many pixels (radius) are not drawn
    float shadow_min_texels = 2.0f;        // LOD items smaller than this many shadow-map texels (radius) cast no shadow
    // Fixed-step runs (screenshots, regression, recording): GPU readbacks wait for their data instead of using it a frame later when it
    // happens to be ready, so frame N is the same every run. Set together with a fixed-step Clock.
    bool deterministic = false;
    bool fog = true;
    bool shadows = true;
    bool clouds = true;
    bool ssao = true;
    bool lens_flare = true;
    bool vignette = true;        // gfx_vignette: the lens-flare vignette pass
    bool depth_of_field = true;  // gfx_depth_of_field: ENABLE_DEPTH_OF_FIELD hdr_to_screen when use_DOF
    bool distortion = true;      // gfx_distortion: distortion composite
    int shadow_quality = 3;      // gfx_shadow_quality: cascade split row (ShadowMapView +0x84)
    void apply_graphics_options(const GraphicsOptions& o) {
        vignette = o.vignette; ssao = o.ssao; distortion = o.distortion; depth_of_field = o.depth_of_field > 0;
        shadow_quality = o.shadow_quality;
    }
    float sun_visibility() const { return sun_visibility_; }

    // ---- editor overlays and picking (editor_passes.cpp)
    // Overlay entities of the scene (lines, triangles) are drawn after the final image, depth tested against the scene unless the
    // overlay says otherwise. On top of them the renderer draws its own helpers:
    bool show_grid = false;                    // a line grid on y = 0
    float grid_step = 10.0f, grid_half_extent = 200.0f;
    std::vector<EntityId> selection;           // entities whose bounds are outlined (yellow, on top)
    // The entity under pixel (x, y) of `target` as `view` sees the scene, 0 for none. Mesh and terrain entities are found with an
    // id pass of their own (the game's G-buffer id target is fed by an engine-private lookup table); overlay entities are picked in
    // screen space (within 6 px of a line, inside a triangle) and win over meshes. Layers marked locked are skipped.
    EntityId pick(const RenderScene& scene, const ViewParams& view, const RenderTarget& target, int x, int y);
    // World point to pixel of a w x h view; false when the point is behind the camera.
    static bool project(const Camera& cam, int w, int h, const float3& p, float& px, float& py);
    static float4x4 view_projection(const Camera& cam, int w, int h);
    uint64_t vfx_revision = 0; // changes when an open vfx/ or weather/ XML is edited: particles reload
    uint32_t cloud_seed = 1;
    // Battle state the engine feeds the weather shaders (not part of the environment files).
    uint32_t wind_dir_seed = 1;  // wind_dir_random: the battle's random direction comes from this seed
    int season = 0;              // weather_shared.g_season_type (snow dump: 3 in winter)
    bool snow_cover = false;     // weather_shared.g_snow_mask (battle map has snow)
    const WindSim& wind() const { return wind_; }
    float wind_direction_degrees(const EnvironmentParams& env) const;
    size_t cloud_sprites() const { size_t n = 0; for (auto& p : cloud_gpu_) n += p.count; return n; }
    float luma_black() const { return luma_black_; }
    std::string engine_ambient_faces() const {
        std::string s;
        for (auto& f : amb_faces_) s += "(" + std::to_string(f[0]) + " " + std::to_string(f[1]) + " " + std::to_string(f[2]) + ") ";
        return s;
    }
    float luma_white() const { return luma_white_; }
    // Forgets what earlier frames left behind (auto exposure, tone mapper limits, frame counter, clock), so the next frames depend on the scene only.
    void restart_temporal() { frame_ = 0; last_time_ = -1; luma_black_ = 0.001f; luma_white_ = 9.0f; env_valid_ = false; }

    Renderer(ID3D11Device* dev, ID3D11DeviceContext* ctx, IVfs& vfs);
    ~Renderer();

    // Draws `scene` (lit by scene.environment.resolved()) as seen by `view.camera` into `target`, at the target's size.
    void render(const RenderScene& scene, const ViewParams& view, RenderTarget& target);
    View view = View::Final;
    TextureCache& textures() { return textures_; }
    ModelCache& models() { return models_; }
    GpuTimer& gpu_timer() { return *gpu_timer_; }

    ConstantBuffers& cbuffers() { return cbs_; }
    ParticleSystem* particles() { return particles_.get(); }
    const std::vector<std::string>& errors() const { return errors_; }

private:
    struct Target {
        Microsoft::WRL::ComPtr<ID3D11Texture2D> tex;
        Microsoft::WRL::ComPtr<ID3D11RenderTargetView> rtv;
        Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> srv;
    };
    void resize(int w, int h);
    Target make_target(int w, int h, DXGI_FORMAT fmt);
    void make_states();
    void set_camera(const float4x4& view, const float4x4& proj, const float3& eye, int w, int h, float time_s);
    void pass_sky_cube(const EnvironmentParams& env, float time_s);
    void pass_gbuffer();
    void pass_sky_forward();
    void pass_directional_light();
    void pass_fog();
    void pass_ssao(const Camera& cam);
    void pass_clouds(const EnvironmentParams& env);
    void pass_sun(const EnvironmentParams& env, const float4x4& view, const float4x4& proj);
    void analyse_depth(const Camera& cam);
    Microsoft::WRL::ComPtr<ID3D11Texture2D> depth_staging_[2];
    int depth_staging_w_ = 0, depth_staging_h_ = 0;
    uint64_t depth_frame_ = 0;
    bool depth_range_valid_ = false;
    float depth_min_ = 1, depth_max_ = 781;
    void pass_shadows(const Camera& cam, float aspect, const float3& light_dir, float time_s);
    void pass_display(RenderTarget& target);
    void pass_overlay(const RenderScene& scene, RenderTarget& target);
    void create_editor_resources();
    void pass_tonemap(const EnvironmentParams& env, float dt);
    void pass_to_screen(const EnvironmentParams& env);
    void pass_lens_flare(const EnvironmentParams& env, const float3& eye, float time_s);
    void fullscreen_to(ID3D11RenderTargetView* rtv, int w, int h, const GameShader& ps);
    void fullscreen(const GameShader& ps);
    const GameShader* shader(const std::string& name);
    void error(const std::string& e);

    ID3D11Device* dev_;
    ID3D11DeviceContext* ctx_;
    IVfs& vfs_;
    ConstantBuffers cbs_;
    float4x4 view_proj_;                      // of the frame being drawn (overlay pass)
    // overlay pass and id pass (editor_passes.cpp), created on first use
    Microsoft::WRL::ComPtr<ID3D11VertexShader> ovl_vs_, pick_vs_;
    Microsoft::WRL::ComPtr<ID3D11PixelShader> ovl_ps_, pick_ps_;
    Microsoft::WRL::ComPtr<ID3D11InputLayout> ovl_il_, pick_il_;
    Microsoft::WRL::ComPtr<ID3D11Buffer> ovl_cb_, ovl_vb_, pick_cb_;
    UINT ovl_vb_capacity_ = 0;
    // The scene's overlays transformed to world space, per batch (triangles / lines with the depth test, then without): rebuilt only when the scene changed.
    std::vector<OverlayVertex> ovl_cache_[4];
    const RenderScene* ovl_cache_scene_ = nullptr;
    uint64_t ovl_cache_rev_ = 0;
    Microsoft::WRL::ComPtr<ID3D11BlendState> ovl_blend_;
    Microsoft::WRL::ComPtr<ID3D11DepthStencilState> ovl_ds_test_, ovl_ds_off_, pick_ds_;
    Microsoft::WRL::ComPtr<ID3D11RasterizerState> ovl_rs_;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> pick_id_tex_, pick_depth_tex_, pick_staging_;
    Microsoft::WRL::ComPtr<ID3D11RenderTargetView> pick_id_rtv_;
    Microsoft::WRL::ComPtr<ID3D11DepthStencilView> pick_dsv_;
    int pick_w_ = 0, pick_h_ = 0;
    Microsoft::WRL::ComPtr<ID3D11Buffer> pick_inst_vb_, pick_id_vb_;
    size_t pick_inst_capacity_ = 0;
    TextureCache textures_;
    ModelCache models_;
    std::unique_ptr<GpuTimer> gpu_timer_;

    // The mesh and terrain entities as culling items (render/draw_set.h); rebuilt when the scene changes or a model finishes loading.
    DrawSet draws_;                             // model instances
    DrawSet terrain_draws_;                     // terrain patches: a brush stroke rebuilds only these (a few hundred items)
    struct EntityBounds {
        float4x4 world;
        float3 bmin, bmax;
    };
    std::unordered_map<EntityId, EntityBounds> terrain_bounds_;
    size_t draw_entities_ = 0, draw_meshes_ = 0, draws_waiting_ = 0, terrain_entities_ = 0, terrain_meshes_ = 0;
    uint64_t draws_terrain_rev_ = 0;
    void build_terrain_draws(const RenderScene& scene);
    const RenderScene* draws_scene_ = nullptr;
    uint64_t draws_revision_ = 0, draws_model_revision_ = 0;   // the scene's structure revision and the model cache's revision the draw set was built from
    uint64_t moved_cursor_ = 0;
    std::vector<uint32_t> item_of_entity_;     // entity id -> item (single-item entities), kNoItem otherwise
    std::vector<EntityId> moved_scratch_;
    static constexpr uint32_t kNoItem = 0xffffffffu;
    void build_draws(const RenderScene& scene, const float3& eye);
    DrawStats draw_stats_;
    DrawSet::Visible vis_, vis_terrain_;        // the view being drawn (camera, cascade, environment face), per set
    float4x4 cur_vp_;                           // set by set_camera: what pass_gbuffer culls with
    float3 cur_eye_;
    float cur_px_ratio_ = 0.001f;               // angular size of one pixel (tangent)
    // Per-view instance data: one dynamic vertex buffer used as a ring, one Map per view.
    Microsoft::WRL::ComPtr<ID3D11Buffer> instance_ring_;
    size_t ring_capacity_ = 0, ring_cursor_ = 0;
    bool ring_fresh_ = true;
    // Writes the instances of both sets into the ring with one Map (growing or wrapping the ring must not lose what the first set wrote); firsts[k] is the
    // first instance of set k.
    void upload_instances(DrawSet* const sets[2], DrawSet::Visible* const viss[2], UINT firsts[2]);
    DrawSet::View camera_view(float lod_scale_factor) const;
    struct PsSlots { int slot[kTexSlots]; };
    std::unordered_map<const GameShader*, PsSlots> ps_slots_;
    const PsSlots& slots_of(const GameShader& ps);
    std::unique_ptr<ShaderLibrary> lib_;
    std::unique_ptr<ParticleSystem> particles_;
    std::vector<std::string> errors_;

    int w_ = 0, h_ = 0;
    Target gb_[5];   // gbuffer channels 1..4 + per-instance id
    Target hdr_, final_, ao_, ao_tmp_, ao_blur_; // ao_: full-res resolve; ao_tmp_/ao_blur_: half-res blur
    Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> jitter_srv_;
    ID3D11ShaderResourceView* ao_result_ = nullptr;
    Microsoft::WRL::ComPtr<ID3D11SamplerState> s_wrap_point_; // final_: hdr_to_screen output (its alpha is the refraction mask)
    Microsoft::WRL::ComPtr<ID3D11Texture2D> depth_tex_;
    Microsoft::WRL::ComPtr<ID3D11DepthStencilView> dsv_;
    Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> depth_srv_;

    static constexpr int kSkyCube = 256;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> sky_cube_;
    Microsoft::WRL::ComPtr<ID3D11RenderTargetView> sky_face_rtv_[6];
    Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> sky_cube_srv_;
    // Engine ambient (use_custom_ambient_cube_colours = false): per-face averages of the environment cube
    // (1x1 mip, read back through two alternating staging textures). See apply_engine_ambient().
    Microsoft::WRL::ComPtr<ID3D11Texture2D> amb_staging_[2];
    float amb_faces_[6][3] = {};
    bool amb_valid_ = false;
    uint64_t amb_frame_ = 0;
    void readback_ambient();
    // Environment cube ("EnvMapGen", fn 0x118e00e0): the whole scene rendered from 50 m above the origin into a cube;
    // s_environment for reflections and, through its face averages, the ambient cube of non-custom lighting.
    Microsoft::WRL::ComPtr<ID3D11Texture2D> env_cube_;
    Microsoft::WRL::ComPtr<ID3D11RenderTargetView> env_face_rtv_[6];
    Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> env_cube_srv_;
    Target env_gb_[5], env_hdr_;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> env_depth_tex_;
    Microsoft::WRL::ComPtr<ID3D11DepthStencilView> env_dsv_;
    bool env_pass_ = false, env_valid_ = false;
    void render_env_cube(const EnvironmentParams& env, float time_s);
    void apply_engine_ambient(const EnvironmentParams& env);

    Microsoft::WRL::ComPtr<ID3D11Buffer> cube_vb_, cube_ib_, instance_vb_;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> instance_ids_tex_;
    Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> instance_ids_srv_, white_srv_, black_srv_, shadow_srv_;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> shadow_tex_;
    // Cascaded shadow atlas: 2048^2, four 1024^2 quadrants (layout from the game's g_split_uv_texture_bounds).
    static constexpr int kShadowSize = 2048;
    Target shadow_atlas_;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> shadow_depth_tex_;
    Microsoft::WRL::ComPtr<ID3D11DepthStencilView> shadow_dsv_;
    Microsoft::WRL::ComPtr<ID3D11RasterizerState> rs_shadow_;
    Microsoft::WRL::ComPtr<ID3D11VertexShader> fs_vs_, fs_vs2_;
    Microsoft::WRL::ComPtr<ID3D11PixelShader> display_ps_;
    Microsoft::WRL::ComPtr<ID3D11Buffer> display_cb_;

    // Tone mapping chain (hdr_tone_mapper.hlsl), see docs/re/renderer.md
    struct SizedTarget { Target t; int w = 0, h = 0; };
    SizedTarget quarter_, bloom_a_, bloom_b_, ldr_;
    std::vector<SizedTarget> stats_;          // luma stats pyramid, last is 1x1
    SizedTarget points_[2];                  // black/white points ping-pong
    static constexpr int kReadbackRing = 4;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> points_staging_[kReadbackRing];
    int points_cur_ = 0;
    uint64_t frame_ = 0;
    float last_time_ = -1, frame_dt_ = 0.016f;
    float luma_black_ = 0.001f, luma_white_ = 9.0f;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> scurve_tex_;
    Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> scurve_srv_;
    uint16_t scurve_last_[256 * 4] = {};

    Microsoft::WRL::ComPtr<ID3D11SamplerState> s_wrap_aniso_, s_wrap_linear_, s_clamp_linear_, s_clamp_point_, s_shadow_cmp_;
    Microsoft::WRL::ComPtr<ID3D11DepthStencilState> ds_gbuffer_, ds_sky_, ds_light_, ds_off_;
    Microsoft::WRL::ComPtr<ID3D11RasterizerState> rs_back_, rs_none_;
    Microsoft::WRL::ComPtr<ID3D11BlendState> bs_opaque_, bs_alpha_, bs_additive_alpha_;
    Microsoft::WRL::ComPtr<ID3D11Buffer> sun_vb_, cloud_quad_vb_;
    struct CloudGpu {
        std::string signature;
        CloudField field;
        Microsoft::WRL::ComPtr<ID3D11Buffer> instances;
        UINT capacity = 0, count = 0;
    };
    float cloud_dt_ = 0;
    WindSim wind_;
    void update_weather(const EnvironmentParams& env, float time_s);
    std::vector<CloudGpu> cloud_gpu_;
    std::map<std::string, Microsoft::WRL::ComPtr<ID3D11ShaderResourceView>> sky_textures_;
    ID3D11ShaderResourceView* sky_texture(const std::string& path, bool srgb = true);
    std::string sun_texture_path_;
    // Sun visibility for the lens flare (lense_flare_VS.sun_occlusion): occlusion queries of the sun disc with and
    // without the depth test, read back a few frames later.
    Microsoft::WRL::ComPtr<ID3D11Query> sun_query_[4][2];
    int sun_query_frame_ = 0;
    float sun_visibility_ = 0;
    Microsoft::WRL::ComPtr<ID3D11Buffer> flare_vb_;
    Microsoft::WRL::ComPtr<ID3D11BlendState> bs_add_one_, bs_no_write_, bs_sun_;
    Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> sun_srv_;
    bool fog_enabled_ = true;
};

} // namespace wce
