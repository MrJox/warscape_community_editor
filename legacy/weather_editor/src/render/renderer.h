#pragma once
// Battle renderer built from the game's own shaders. Pass order and shader choices are documented
// in docs/re/renderer.md; anything not yet confirmed against the game is marked there.
#include "render/environment.h"
#include "render/math.h"
#include "render/shader_lib.h"
#include "render/wind.h"
#include "vfx/particles.h"

#include <d3d11.h>
#include <wrl/client.h>

#include <map>
#include <memory>
#include <string>
#include <vector>

namespace we {

class Vfs;
class TestScene;

struct Camera {
    float3 eye{0, 30, -80}, target{0, 5, 0};
    // The game's battle camera (memory dump): near 1, far 13000, vertical FOV 1.13446 rad (65 degrees).
    float fov_y = 1.13446f, znear = 1.0f, zfar = 13000.0f;
};

class Renderer {
public:
    enum class View { Final, HdrRaw, Albedo, Normal, Specular, Depth, SkyCube, Bloom, Quarter, ToneMapped, AO };
    float debug_points[4] = {}; int debug_maps = 0;
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

    Renderer(ID3D11Device* dev, ID3D11DeviceContext* ctx, Vfs& vfs);
    ~Renderer();

    void render(const TestScene* scene, const Camera& cam, const EnvironmentParams& env, int width, int height, float time_s);
    ID3D11ShaderResourceView* output() const { return out_srv_.Get(); }
    View view = View::Final;

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
    void pass_gbuffer(const TestScene* scene);
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
    void pass_shadows(const TestScene* scene, const Camera& cam, float aspect, const float3& light_dir, float time_s);
    void pass_display();
    void pass_tonemap(const EnvironmentParams& env, float dt);
    void pass_to_screen(const EnvironmentParams& env);
    void pass_lens_flare(const EnvironmentParams& env, const float3& eye, float time_s);
    void fullscreen_to(ID3D11RenderTargetView* rtv, int w, int h, const GameShader& ps);
    void fullscreen(const GameShader& ps);
    const GameShader* shader(const std::string& name);
    void error(const std::string& e);

    ID3D11Device* dev_;
    ID3D11DeviceContext* ctx_;
    Vfs& vfs_;
    ConstantBuffers cbs_;
    std::unique_ptr<ShaderLibrary> lib_;
    std::unique_ptr<ParticleSystem> particles_;
    std::vector<std::string> errors_;

    int w_ = 0, h_ = 0;
    Target gb_[5];   // gbuffer channels 1..4 + per-instance id
    Target hdr_, out_, final_, ao_, ao_tmp_, ao_blur_; // ao_: full-res resolve; ao_tmp_/ao_blur_: half-res blur
    Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> jitter_srv_;
    ID3D11ShaderResourceView* ao_result_ = nullptr;
    Microsoft::WRL::ComPtr<ID3D11SamplerState> s_wrap_point_; // final_: hdr_to_screen output (its alpha is the refraction mask)
    Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> out_srv_;
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
    void render_env_cube(const TestScene* scene, const EnvironmentParams& env, float time_s);
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

} // namespace we
