#include "renderer.h"

#include "core/vfs.h"
#include "render/scene.h"

#include <d3dcompiler.h>

#include <algorithm>
#include <cstdlib>
#include <cstring>
#include <stdexcept>

using Microsoft::WRL::ComPtr;

namespace we {

namespace {

// Shader choices (highest-quality dx11 variants). See docs/re/renderer.md.
const char* kSkyVS = "dx11_sm_3_0_skydome.hlsl_vs_vs30_main";
const char* kSkyGenPS = "dx11_sm_3_0_skydome.hlsl_ps_ps30_skygen";
const char* kSkyPS = "dx11_sm_3_0_skydome.hlsl_ps_ps30_main";
const char* kRigidVS = "dx11_sm_4_0_rigidcommon.hlsl_vs_vs30_main";
const char* kRigidGbufferPS = "dx11_sm_3_0_rigidstandard.hlsl_ps_ps30_gbuffer";
const char* kRigidGbufferAlphaPS = "dx11_sm_3_0_rigidstandard.hlsl_ps_ps30_gbuffer_alpha_test";
const char* kDirLightPS = "dx11_enable_shadows_shadows_hardware_gather_5_0_sm_5_0_directionallight.hlsl_ps_ps30_slm_highdef";
#define TM "dx11_sm_3_0_hdr_tone_mapper.hlsl_ps_ps_"
const char* kTmDownLuma = TM "downsample_image_store_luma";
const char* kTmLumaStats = TM "hdr_luma_to_quarter_size_luma_stats";
const char* kTmStatsDown = TM "downsample_quarter_size_luma_stats";
const char* kTmPoints = TM "determine_black_and_white_points";
const char* kTmHighPass = TM "high_pass_luminance_cutoff";
const char* kTmBlurX = TM "blur_x";
const char* kTmBlurY = TM "blur_y";
const char* kTmToneMap = TM "basic_tone_mapper"; // game's live path (dump: constant_tone_mapping_buffer = 0); linear out
#undef TM
const char* kToScreenPS = "dx11_sm_3_0_hdr_to_screen.hlsl_ps_ps30_main_high";
// Same shader compiled with ENABLE_DISTORTION: uv += (distortion_sampler.xy - 0.498) * 0.02 (heat haze, lens drops).
const char* kToScreenDistortPS = "dx11_enable_distortion_sm_3_0_hdr_to_screen.hlsl_ps_ps30_main_high";
const char* kFogPS = "dx11_sm_3_0_battlefog.hlsl_ps_ps30_main";
const char* kShadowVS = "dx11_sm_3_0_rigidcommon.hlsl_vs_vs30_shadow";
const char* kSunVS = "dx11_sm_3_0_sun.hlsl_vs_vs30_main";
const char* kSunPS = "dx11_sm_3_0_sun.hlsl_ps_ps30_main";
const char* kSsaoPS = "dx11_sm_3_0_ssao.hlsl_ps_pixel_resolve";
const char* kSsaoBlurXPS = "dx11_sm_3_0_ssao.hlsl_ps_pixel_blur_x";
const char* kSsaoBlurYPS = "dx11_sm_3_0_ssao.hlsl_ps_pixel_blur_y";
const char* kCloudVS = "dx11_sm_3_0_cloud_plane.hlsl_vs_vs30_main";
const char* kCloudPS = "dx11_sm_3_0_cloud_plane.hlsl_ps_ps30_main";
constexpr float kSunDistance = 7600.0f; // memory dump: sun_VS.g_world_view translation = view rotation * to_sun * 7600
const char* kShadowPS = "dx11_sm_3_0_rigidcommon.hlsl_ps_ps30_shadow";

// Editor-side helpers (not game shaders): full-screen triangle and the debug/temporary display pass.
const char* kHelperHlsl = R"(
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
)";

ComPtr<ID3DBlob> compile(const char* entry, const char* target) {
    ComPtr<ID3DBlob> code, err;
    if (FAILED(D3DCompile(kHelperHlsl, std::strlen(kHelperHlsl), "helpers", nullptr, nullptr, entry, target, 0, 0, &code, &err)))
        throw std::runtime_error(std::string("helper shader: ") + (err ? static_cast<const char*>(err->GetBufferPointer()) : entry));
    return code;
}

// skydome.hlsl vs30_main declares POSITION0 (xyz) and an unused TEXCOORD0 (xy); alias it onto the position.
std::vector<D3D11_INPUT_ELEMENT_DESC> sky_layout() {
    return {{"POSITION", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 0, D3D11_INPUT_PER_VERTEX_DATA, 0},
            {"TEXCOORD", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 0, D3D11_INPUT_PER_VERTEX_DATA, 0}};
}

float4x4 face_view(int face) {
    static const float3 dirs[6] = {{1, 0, 0}, {-1, 0, 0}, {0, 1, 0}, {0, -1, 0}, {0, 0, 1}, {0, 0, -1}};
    static const float3 ups[6] = {{0, 1, 0}, {0, 1, 0}, {0, 0, -1}, {0, 0, 1}, {0, 1, 0}, {0, 1, 0}};
    return look_at_lh({0, 0, 0}, dirs[face], ups[face]);
}

} // namespace

Renderer::Renderer(ID3D11Device* dev, ID3D11DeviceContext* ctx, Vfs& vfs) : dev_(dev), ctx_(ctx), vfs_(vfs) {
    lib_ = std::make_unique<ShaderLibrary>(dev, vfs, cbs_);
    particles_ = std::make_unique<ParticleSystem>(dev, ctx, vfs, *lib_, cbs_);
    make_states();

    auto vs = compile("fs_vs", "vs_5_0");
    dev_->CreateVertexShader(vs->GetBufferPointer(), vs->GetBufferSize(), nullptr, &fs_vs_);
    auto vs2 = compile("fs_vs2", "vs_5_0");
    dev_->CreateVertexShader(vs2->GetBufferPointer(), vs2->GetBufferSize(), nullptr, &fs_vs2_);
    auto ps = compile("display_ps", "ps_5_0");
    dev_->CreatePixelShader(ps->GetBufferPointer(), ps->GetBufferSize(), nullptr, &display_ps_);
    D3D11_BUFFER_DESC bd{16, D3D11_USAGE_DEFAULT, D3D11_BIND_CONSTANT_BUFFER};
    dev_->CreateBuffer(&bd, nullptr, &display_cb_);

    // Sky dome geometry: a cube around the camera (the sky shaders only use the direction).
    const float s = 10.0f;
    float verts[8][3] = {{-s, -s, -s}, {s, -s, -s}, {s, s, -s}, {-s, s, -s}, {-s, -s, s}, {s, -s, s}, {s, s, s}, {-s, s, s}};
    uint16_t idx[36] = {0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 3, 6, 2, 3, 7, 6, 0, 4, 7, 0, 7, 3, 1, 2, 6, 1, 6, 5};
    D3D11_BUFFER_DESC vbd{sizeof verts, D3D11_USAGE_IMMUTABLE, D3D11_BIND_VERTEX_BUFFER};
    D3D11_SUBRESOURCE_DATA vsd{verts};
    dev_->CreateBuffer(&vbd, &vsd, &cube_vb_);
    D3D11_BUFFER_DESC ibd{sizeof idx, D3D11_USAGE_IMMUTABLE, D3D11_BIND_INDEX_BUFFER};
    D3D11_SUBRESOURCE_DATA isd{idx};
    dev_->CreateBuffer(&ibd, &isd, &cube_ib_);

    D3D11_BUFFER_DESC inst{sizeof(RigidInstance), D3D11_USAGE_DYNAMIC, D3D11_BIND_VERTEX_BUFFER, D3D11_CPU_ACCESS_WRITE};
    dev_->CreateBuffer(&inst, nullptr, &instance_vb_);

    // Per-instance uint lookup read by rigidcommon vs30_main (t0, offset rigid_config_PS[4].w). Zeros for now.
    {
        std::vector<uint32_t> zeros(1024, 0);
        D3D11_TEXTURE2D_DESC td{1024, 1, 1, 1, DXGI_FORMAT_R32_UINT, {1, 0}, D3D11_USAGE_IMMUTABLE, D3D11_BIND_SHADER_RESOURCE};
        D3D11_SUBRESOURCE_DATA sd{zeros.data(), 4096};
        dev_->CreateTexture2D(&td, &sd, &instance_ids_tex_);
        dev_->CreateShaderResourceView(instance_ids_tex_.Get(), nullptr, &instance_ids_srv_);
    }
    auto solid = [&](uint32_t rgba, ComPtr<ID3D11ShaderResourceView>& out) {
        D3D11_TEXTURE2D_DESC td{1, 1, 1, 1, DXGI_FORMAT_R8G8B8A8_UNORM, {1, 0}, D3D11_USAGE_IMMUTABLE, D3D11_BIND_SHADER_RESOURCE};
        D3D11_SUBRESOURCE_DATA sd{&rgba, 4};
        ComPtr<ID3D11Texture2D> t;
        dev_->CreateTexture2D(&td, &sd, &t);
        dev_->CreateShaderResourceView(t.Get(), nullptr, &out);
    };
    solid(0xffffffff, white_srv_);
    solid(0x00000000, black_srv_);
    {   // Placeholder shadow map: depth 1 everywhere (fully lit) until the cascade pass exists.
        float one = 1.0f;
        D3D11_TEXTURE2D_DESC td{1, 1, 1, 1, DXGI_FORMAT_R32_FLOAT, {1, 0}, D3D11_USAGE_IMMUTABLE, D3D11_BIND_SHADER_RESOURCE};
        D3D11_SUBRESOURCE_DATA sd{&one, 4};
        dev_->CreateTexture2D(&td, &sd, &shadow_tex_);
        dev_->CreateShaderResourceView(shadow_tex_.Get(), nullptr, &shadow_srv_);
    }

    shadow_atlas_ = make_target(kShadowSize, kShadowSize, DXGI_FORMAT_R32_FLOAT);
    {
        D3D11_TEXTURE2D_DESC td{kShadowSize, kShadowSize, 1, 1, DXGI_FORMAT_D32_FLOAT, {1, 0}, D3D11_USAGE_DEFAULT, D3D11_BIND_DEPTH_STENCIL};
        dev_->CreateTexture2D(&td, nullptr, &shadow_depth_tex_);
        dev_->CreateDepthStencilView(shadow_depth_tex_.Get(), nullptr, &shadow_dsv_);
        D3D11_RASTERIZER_DESC r{};
        r.FillMode = D3D11_FILL_SOLID;
        r.CullMode = D3D11_CULL_FRONT; // ASSUMPTION: back faces into the shadow map (the game's sample bias is 0)
        r.DepthClipEnable = FALSE;     // keep casters behind the light's near plane
        dev_->CreateRasterizerState(&r, &rs_shadow_);
    }

    {   // Sun billboard: two triangles; POSITION.xy in view units (x sun_scale), TEXCOORD.xy.
        // ASSUMPTION: corners at +-kSunDistance, i.e. half-size = sun_scale * distance.
        const float d = kSunDistance;
        float v[6][4] = {{-d, -d, 0, 1}, {-d, d, 0, 0}, {d, d, 1, 0}, {-d, -d, 0, 1}, {d, d, 1, 0}, {d, -d, 1, 1}};
        D3D11_BUFFER_DESC sbd{sizeof v, D3D11_USAGE_IMMUTABLE, D3D11_BIND_VERTEX_BUFFER};
        D3D11_SUBRESOURCE_DATA ssd{v};
        dev_->CreateBuffer(&sbd, &ssd, &sun_vb_);
    }

    {   // Cloud sprite quad: POSITION xy in [-1, 1] (scaled by the sprite's angular size), TEXCOORD0 uv.
        float q[6][4] = {{-1, -1, 0, 1}, {-1, 1, 0, 0}, {1, 1, 1, 0}, {-1, -1, 0, 1}, {1, 1, 1, 0}, {1, -1, 1, 1}};
        D3D11_BUFFER_DESC qbd{sizeof q, D3D11_USAGE_IMMUTABLE, D3D11_BIND_VERTEX_BUFFER};
        D3D11_SUBRESOURCE_DATA qsd{q};
        dev_->CreateBuffer(&qbd, &qsd, &cloud_quad_vb_);
    }

    {   // HBAO jitter texture, 256^2 (g_JitterResolution in the dump). UNORM, expanded with *2-1 by the shader:
        // x, y = direction rotation (cos, sin of 2*pi*r / num_dir), z = step jitter.
        // ASSUMPTION: content follows NVIDIA's HBAO sample, which this shader mirrors (same constant names).
        std::vector<uint8_t> px(256 * 256 * 4);
        uint32_t jseed = 12345;
        auto rnd = [&jseed] { jseed = jseed * 1664525u + 1013904223u; return float(jseed >> 8) / float(1 << 24); };
        for (int i = 0; i < 256 * 256; ++i) {
            float a = 6.2831853f * rnd() / 5.0f, j = rnd();
            float v[3] = {std::cos(a), std::sin(a), j};
            for (int c = 0; c < 3; ++c) px[i * 4 + c] = uint8_t(std::lround((v[c] * 0.5f + 0.5f) * 255.0f));
            px[i * 4 + 3] = 255;
        }
        D3D11_TEXTURE2D_DESC td{256, 256, 1, 1, DXGI_FORMAT_R8G8B8A8_UNORM, {1, 0}, D3D11_USAGE_IMMUTABLE, D3D11_BIND_SHADER_RESOURCE};
        D3D11_SUBRESOURCE_DATA sd{px.data(), 256 * 4};
        Microsoft::WRL::ComPtr<ID3D11Texture2D> t;
        dev_->CreateTexture2D(&td, &sd, &t);
        dev_->CreateShaderResourceView(t.Get(), nullptr, &jitter_srv_);
        D3D11_SAMPLER_DESC sm{};
        sm.Filter = D3D11_FILTER_MIN_MAG_MIP_POINT;
        sm.AddressU = sm.AddressV = sm.AddressW = D3D11_TEXTURE_ADDRESS_WRAP;
        sm.MaxLOD = D3D11_FLOAT32_MAX;
        dev_->CreateSamplerState(&sm, &s_wrap_point_);
    }

    // Sky cubemap (output of ps30_skygen).
    D3D11_TEXTURE2D_DESC cd{kSkyCube, kSkyCube, 0, 6, DXGI_FORMAT_R16G16B16A16_FLOAT, {1, 0}, D3D11_USAGE_DEFAULT,
                            D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE, 0,
                            D3D11_RESOURCE_MISC_TEXTURECUBE | D3D11_RESOURCE_MISC_GENERATE_MIPS};
    dev_->CreateTexture2D(&cd, nullptr, &sky_cube_);
    for (int f = 0; f < 6; ++f) {
        D3D11_RENDER_TARGET_VIEW_DESC rd{};
        rd.Format = cd.Format;
        rd.ViewDimension = D3D11_RTV_DIMENSION_TEXTURE2DARRAY;
        rd.Texture2DArray.FirstArraySlice = f;
        rd.Texture2DArray.ArraySize = 1;
        dev_->CreateRenderTargetView(sky_cube_.Get(), &rd, &sky_face_rtv_[f]);
    }
    dev_->CreateShaderResourceView(sky_cube_.Get(), nullptr, &sky_cube_srv_);

    // Black/white points: two 1x1 targets (previous/current) plus CPU readback for the constant tone mapper.
    for (int i = 0; i < 2; ++i) {
        points_[i].w = points_[i].h = 1;
        points_[i].t = make_target(1, 1, DXGI_FORMAT_R32G32B32A32_FLOAT);
    }
    for (auto& st : points_staging_) {
        D3D11_TEXTURE2D_DESC sd{1, 1, 1, 1, DXGI_FORMAT_R32G32B32A32_FLOAT, {1, 0}, D3D11_USAGE_STAGING, 0, D3D11_CPU_ACCESS_READ};
        dev_->CreateTexture2D(&sd, nullptr, &st);
    }
    {
        D3D11_TEXTURE2D_DESC sd{256, 1, 1, 1, DXGI_FORMAT_R8G8B8A8_UNORM, {1, 0}, D3D11_USAGE_DEFAULT, D3D11_BIND_SHADER_RESOURCE};
        dev_->CreateTexture2D(&sd, nullptr, &scurve_tex_);
        dev_->CreateShaderResourceView(scurve_tex_.Get(), nullptr, &scurve_srv_);
    }

    // Faction colours: white = no tint on the test map.
    for (const char* v : {"faction_colours"}) {
        float fc[11] = {1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1};
        (void)v;
        cbs_.declare("rigid_faction_colours", 64, {{"faction_colours", {0, 60}}});
        cbs_.set("rigid_faction_colours", "faction_colours", fc, sizeof fc);
    }
}

Renderer::~Renderer() = default;

void Renderer::error(const std::string& e) {
    for (auto& x : errors_)
        if (x == e) return;
    errors_.push_back(e);
}

const GameShader* Renderer::shader(const std::string& name) {
    try {
        return &lib_->get(name);
    } catch (const std::exception& e) {
        error(e.what());
        return nullptr;
    }
}

void Renderer::make_states() {
    D3D11_SAMPLER_DESC s{};
    s.Filter = D3D11_FILTER_ANISOTROPIC;
    s.AddressU = s.AddressV = s.AddressW = D3D11_TEXTURE_ADDRESS_WRAP;
    s.MaxAnisotropy = 8;
    s.MaxLOD = D3D11_FLOAT32_MAX;
    dev_->CreateSamplerState(&s, &s_wrap_aniso_);
    s.Filter = D3D11_FILTER_MIN_MAG_MIP_LINEAR;
    s.AddressU = s.AddressV = s.AddressW = D3D11_TEXTURE_ADDRESS_CLAMP;
    dev_->CreateSamplerState(&s, &s_clamp_linear_);
    s.Filter = D3D11_FILTER_MIN_MAG_MIP_POINT;
    dev_->CreateSamplerState(&s, &s_clamp_point_);
    s.Filter = D3D11_FILTER_COMPARISON_MIN_MAG_LINEAR_MIP_POINT;
    s.ComparisonFunc = D3D11_COMPARISON_LESS_EQUAL;
    dev_->CreateSamplerState(&s, &s_shadow_cmp_);

    D3D11_DEPTH_STENCIL_DESC d{};
    d.DepthEnable = TRUE;
    d.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ALL;
    d.DepthFunc = D3D11_COMPARISON_LESS_EQUAL;
    d.StencilEnable = TRUE;
    d.StencilReadMask = d.StencilWriteMask = 0xff;
    d.FrontFace = {D3D11_STENCIL_OP_KEEP, D3D11_STENCIL_OP_KEEP, D3D11_STENCIL_OP_REPLACE, D3D11_COMPARISON_ALWAYS};
    d.BackFace = d.FrontFace;
    dev_->CreateDepthStencilState(&d, &ds_gbuffer_); // geometry marks stencil = 1
    d.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ZERO;
    d.StencilEnable = FALSE;
    dev_->CreateDepthStencilState(&d, &ds_sky_);      // sky at z = w only where nothing was drawn
    d.DepthEnable = FALSE;
    d.StencilEnable = TRUE;
    d.FrontFace = {D3D11_STENCIL_OP_KEEP, D3D11_STENCIL_OP_KEEP, D3D11_STENCIL_OP_KEEP, D3D11_COMPARISON_EQUAL};
    d.BackFace = d.FrontFace;
    dev_->CreateDepthStencilState(&d, &ds_light_);    // lights only on geometry
    d.StencilEnable = FALSE;
    dev_->CreateDepthStencilState(&d, &ds_off_);

    D3D11_RASTERIZER_DESC r{};
    r.FillMode = D3D11_FILL_SOLID;
    r.CullMode = D3D11_CULL_BACK;
    r.DepthClipEnable = TRUE;
    dev_->CreateRasterizerState(&r, &rs_back_);
    r.CullMode = D3D11_CULL_NONE;
    dev_->CreateRasterizerState(&r, &rs_none_);

    D3D11_BLEND_DESC b{};
    b.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL;
    dev_->CreateBlendState(&b, &bs_opaque_);
    // ASSUMPTION: fog composites with standard alpha blending (battlefog writes the fog amount to .w).
    b.RenderTarget[0].BlendEnable = TRUE;
    b.RenderTarget[0].SrcBlend = D3D11_BLEND_SRC_ALPHA;
    b.RenderTarget[0].DestBlend = D3D11_BLEND_INV_SRC_ALPHA;
    b.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
    b.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ZERO;
    b.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_ONE;
    b.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
    b.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_RED | D3D11_COLOR_WRITE_ENABLE_GREEN | D3D11_COLOR_WRITE_ENABLE_BLUE;
    dev_->CreateBlendState(&b, &bs_alpha_);
    // ASSUMPTION: the sun disc adds its texture (weighted by alpha) on top of the sky.
    b.RenderTarget[0].DestBlend = D3D11_BLEND_ONE;
    dev_->CreateBlendState(&b, &bs_additive_alpha_);
}

Renderer::Target Renderer::make_target(int w, int h, DXGI_FORMAT fmt) {
    Target t;
    D3D11_TEXTURE2D_DESC td{UINT(w), UINT(h), 1, 1, fmt, {1, 0}, D3D11_USAGE_DEFAULT, D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE};
    dev_->CreateTexture2D(&td, nullptr, &t.tex);
    dev_->CreateRenderTargetView(t.tex.Get(), nullptr, &t.rtv);
    dev_->CreateShaderResourceView(t.tex.Get(), nullptr, &t.srv);
    return t;
}

void Renderer::resize(int w, int h) {
    if (w == w_ && h == h_) return;
    w_ = w;
    h_ = h;
    // ASSUMPTION (pending capture): gbuffer formats. Channel contents are from the shaders.
    gb_[0] = make_target(w, h, DXGI_FORMAT_R8G8B8A8_UNORM); // normal (cube-normalised, *0.5+0.5), gloss
    gb_[1] = make_target(w, h, DXGI_FORMAT_R8G8B8A8_UNORM); // sqrt(albedo), .w mask
    gb_[2] = make_target(w, h, DXGI_FORMAT_R8G8B8A8_UNORM); // specular colour, .w material id / 255
    gb_[3] = make_target(w, h, DXGI_FORMAT_R32_FLOAT);      // z / w
    gb_[4] = make_target(w, h, DXGI_FORMAT_R32_UINT);       // per-instance value (uint output, debug layer)
    hdr_ = make_target(w, h, DXGI_FORMAT_R16G16B16A16_FLOAT);
    out_ = make_target(w, h, DXGI_FORMAT_R8G8B8A8_UNORM);
    final_ = make_target(w, h, DXGI_FORMAT_R8G8B8A8_UNORM);
    ao_ = make_target(w, h, DXGI_FORMAT_R8G8B8A8_UNORM);
    ao_tmp_ = make_target(std::max(1, w / 2), std::max(1, h / 2), DXGI_FORMAT_R8G8B8A8_UNORM); // blur at half resolution (dump)
    ao_blur_ = make_target(std::max(1, w / 2), std::max(1, h / 2), DXGI_FORMAT_R8G8B8A8_UNORM);
    out_srv_ = out_.srv;

    auto sized = [&](int tw, int th, DXGI_FORMAT f) {
        SizedTarget s;
        s.w = std::max(1, tw);
        s.h = std::max(1, th);
        s.t = make_target(s.w, s.h, f);
        return s;
    };
    int qw = (w + 3) / 4, qh = (h + 3) / 4;
    quarter_ = sized(qw, qh, DXGI_FORMAT_R16G16B16A16_FLOAT); // rgb + luma in .w
    bloom_a_ = sized(qw, qh, DXGI_FORMAT_R16G16B16A16_FLOAT);
    bloom_b_ = sized(qw, qh, DXGI_FORMAT_R16G16B16A16_FLOAT);
    ldr_ = sized(w, h, DXGI_FORMAT_R16G16B16A16_FLOAT);     // tone-mapped, linear
    stats_.clear();
    int sw = qw, sh = qh;
    do {
        sw = (sw + 3) / 4;
        sh = (sh + 3) / 4;
        stats_.push_back(sized(sw, sh, DXGI_FORMAT_R32G32B32A32_FLOAT)); // min, max, avg, max
    } while (sw > 1 || sh > 1);
    D3D11_TEXTURE2D_DESC td{UINT(w), UINT(h), 1, 1, DXGI_FORMAT_D24_UNORM_S8_UINT, {1, 0}, D3D11_USAGE_DEFAULT, D3D11_BIND_DEPTH_STENCIL};
    depth_tex_.Reset();
    dsv_.Reset();
    dev_->CreateTexture2D(&td, nullptr, &depth_tex_);
    dev_->CreateDepthStencilView(depth_tex_.Get(), nullptr, &dsv_);
}

void Renderer::set_camera(const float4x4& view_m, const float4x4& proj, const float3& eye, int w, int h, float time_s) {
    float4x4 vp = proj * view_m;
    cbs_.set_f("camera_VS_PS", "camera_position", {eye.x, eye.y, eye.z});
    cbs_.set_m("camera_VS_PS", "view", view_m.data());
    cbs_.set_m("camera_VS_PS", "projection", proj.data());
    cbs_.set_m("camera_VS_PS", "view_projection", vp.data());
    cbs_.set_m("camera_VS_PS", "inv_view", inverse(view_m).data());
    cbs_.set_m("camera_VS_PS", "inv_projection", inverse(proj).data());
    cbs_.set_m("camera_VS_PS", "inv_view_projection", inverse(vp).data());
    cbs_.set_f("camera_VS_PS", "time_in_sec", {time_s});
    cbs_.set_f("camera_VS_PS", "g_inverse_focal_length", {1.0f / proj.m[0][0], 1.0f / proj.m[1][1]});
    float fw = float(w), fh = float(h);
    cbs_.set_f("camera_VS_PS", "g_screen_size", {fw, fh, 1 / fw, 1 / fh});
    cbs_.set_f("camera_VS_PS", "g_vpos_texel_offset", {0.0f}); // SV_Position is already at pixel centres in D3D11
    cbs_.set_f("camera_VS_PS", "g_viewport_dimensions", {fw, fh, 1 / fw, 1 / fh});
    cbs_.set_f("camera_VS_PS", "g_render_target_dimensions", {fw, fh, 1 / fw, 1 / fh});
    cbs_.set_i("camera_VS_PS", "g_num_of_samples", 1);
}

void Renderer::fullscreen(const GameShader& ps) {
    ctx_->IASetInputLayout(nullptr);
    ctx_->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    ctx_->VSSetShader(fs_vs_.Get(), nullptr, 0);
    bind_shader(ctx_, ps, cbs_);
    ctx_->Draw(3, 0);
}

void Renderer::pass_sky_cube(const EnvironmentParams& env, float time_s) {
    const GameShader* vs = shader(kSkyVS);
    const GameShader* ps = shader(kSkyGenPS);
    if (!vs || !ps) return;
    float4x4 proj = perspective_lh(3.14159265f / 2, 1, 0.1f, 100);
    D3D11_VIEWPORT vp{0, 0, float(kSkyCube), float(kSkyCube), 0, 1};
    ctx_->RSSetViewports(1, &vp);
    ctx_->RSSetState(rs_none_.Get());
    ctx_->OMSetDepthStencilState(ds_off_.Get(), 0);
    ctx_->OMSetBlendState(bs_opaque_.Get(), nullptr, 0xffffffff);
    ctx_->IASetInputLayout(lib_->layout(*vs, "pos3", sky_layout()));
    UINT stride = 12, off = 0;
    ctx_->IASetVertexBuffers(0, 1, cube_vb_.GetAddressOf(), &stride, &off);
    ctx_->IASetIndexBuffer(cube_ib_.Get(), DXGI_FORMAT_R16_UINT, 0);
    ctx_->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    (void)env;
    for (int f = 0; f < 6; ++f) {
        set_camera(face_view(f), proj, {0, 0, 0}, kSkyCube, kSkyCube, time_s);
        cbs_.upload(dev_, ctx_);
        ctx_->OMSetRenderTargets(1, sky_face_rtv_[f].GetAddressOf(), nullptr);
        bind_shader(ctx_, *vs, cbs_);
        bind_shader(ctx_, *ps, cbs_);
        ctx_->DrawIndexed(36, 0, 0);
    }
    ctx_->OMSetRenderTargets(0, nullptr, nullptr);
    ctx_->GenerateMips(sky_cube_srv_.Get());
}

void Renderer::pass_gbuffer(const TestScene* scene) {
    ID3D11RenderTargetView* rtvs[5] = {gb_[0].rtv.Get(), gb_[1].rtv.Get(), gb_[2].rtv.Get(), gb_[3].rtv.Get(), gb_[4].rtv.Get()};
    const float zero[4] = {0, 0, 0, 0};
    for (auto* r : rtvs) ctx_->ClearRenderTargetView(r, zero);
    ctx_->ClearDepthStencilView(dsv_.Get(), D3D11_CLEAR_DEPTH | D3D11_CLEAR_STENCIL, 1.0f, 0);
    ctx_->OMSetRenderTargets(5, rtvs, dsv_.Get());
    ctx_->OMSetDepthStencilState(ds_gbuffer_.Get(), 1);
    ctx_->RSSetState(rs_back_.Get());
    if (!scene || scene->objects().empty()) return;

    const GameShader* vs = shader(kRigidVS);
    const GameShader* ps_opaque = shader(kRigidGbufferPS);
    const GameShader* ps_alpha = shader(kRigidGbufferAlphaPS);
    if (!vs || !ps_opaque) return;
    // STANDARD_RIGID_MESH_VERTEX (32 bytes) + 7 float4 per instance. Byte vectors are BGRA ("X/Z swapped").
    std::vector<D3D11_INPUT_ELEMENT_DESC> el = {
        {"POSITION", 0, DXGI_FORMAT_R16G16B16A16_FLOAT, 0, 0, D3D11_INPUT_PER_VERTEX_DATA, 0},
        {"TEXCOORD", 0, DXGI_FORMAT_R16G16B16A16_FLOAT, 0, 8, D3D11_INPUT_PER_VERTEX_DATA, 0},
        {"NORMAL", 0, DXGI_FORMAT_B8G8R8A8_UNORM, 0, 16, D3D11_INPUT_PER_VERTEX_DATA, 0},
        {"NORMAL", 1, DXGI_FORMAT_B8G8R8A8_UNORM, 0, 20, D3D11_INPUT_PER_VERTEX_DATA, 0},
        {"NORMAL", 2, DXGI_FORMAT_B8G8R8A8_UNORM, 0, 24, D3D11_INPUT_PER_VERTEX_DATA, 0},
        {"COLOR", 0, DXGI_FORMAT_B8G8R8A8_UNORM, 0, 28, D3D11_INPUT_PER_VERTEX_DATA, 0},
    };
    for (UINT i = 1; i <= 7; ++i)
        el.push_back({"POSITION", i, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, (i - 1) * 16, D3D11_INPUT_PER_INSTANCE_DATA, 1});
    ctx_->IASetInputLayout(lib_->layout(*vs, "rigid_std", el));
    ctx_->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    bind_shader(ctx_, *vs, cbs_);
    for (auto& [name, slot] : vs->textures) ctx_->VSSetShaderResources(slot, 1, instance_ids_srv_.GetAddressOf());

    for (auto& o : scene->objects()) {
        RigidInstance inst{};
        for (int r = 0; r < 3; ++r)
            for (int c = 0; c < 4; ++c) inst.world[r][c] = o.world.m[r][c];
        D3D11_MAPPED_SUBRESOURCE ms;
        if (FAILED(ctx_->Map(instance_vb_.Get(), 0, D3D11_MAP_WRITE_DISCARD, 0, &ms))) continue;
        std::memcpy(ms.pData, &inst, sizeof inst);
        ctx_->Unmap(instance_vb_.Get(), 0);
        for (auto& m : o.model->meshes) {
            const GameShader* ps = (m.alpha_mode == 1 && ps_alpha) ? ps_alpha : ps_opaque;
            bind_shader(ctx_, *ps, cbs_);
            for (auto& [tname, tex] : m.textures) bind_srv(ctx_, *ps, tname, tex ? tex->srv.Get() : nullptr);
            bind_srv(ctx_, *ps, "s_fire_map", black_srv_.Get());
            bind_srv(ctx_, *ps, "s_snow_normals", black_srv_.Get());
            for (auto& [sname, slot] : ps->samplers) ctx_->PSSetSamplers(slot, 1, s_wrap_aniso_.GetAddressOf());
            ID3D11Buffer* vbs[2] = {m.vb.Get(), instance_vb_.Get()};
            UINT strides[2] = {32, sizeof(RigidInstance)}, offs[2] = {0, 0};
            ctx_->IASetVertexBuffers(0, 2, vbs, strides, offs);
            ctx_->IASetIndexBuffer(m.ib.Get(), DXGI_FORMAT_R16_UINT, 0);
            ctx_->DrawIndexedInstanced(m.index_count, 1, 0, 0, 0);
        }
    }
}

void Renderer::pass_sky_forward() {
    const GameShader* vs = shader(kSkyVS);
    const GameShader* ps = shader(kSkyPS);
    if (!vs || !ps) return;
    ctx_->OMSetRenderTargets(1, hdr_.rtv.GetAddressOf(), dsv_.Get());
    ctx_->OMSetDepthStencilState(ds_sky_.Get(), 0);
    ctx_->RSSetState(rs_none_.Get());
    ctx_->IASetInputLayout(lib_->layout(*vs, "pos3", sky_layout()));
    UINT stride = 12, off = 0;
    ctx_->IASetVertexBuffers(0, 1, cube_vb_.GetAddressOf(), &stride, &off);
    ctx_->IASetIndexBuffer(cube_ib_.Get(), DXGI_FORMAT_R16_UINT, 0);
    ctx_->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    bind_shader(ctx_, *vs, cbs_);
    bind_shader(ctx_, *ps, cbs_);
    bind_srv(ctx_, *ps, "s_sky", sky_cube_srv_.Get());
    bind_sampler(ctx_, *ps, "s_sky", s_clamp_linear_.Get());
    ctx_->DrawIndexed(36, 0, 0);
}

void Renderer::pass_directional_light() {
    const GameShader* ps = shader(kDirLightPS);
    if (!ps) return;
    ctx_->OMSetRenderTargets(1, hdr_.rtv.GetAddressOf(), dsv_.Get());
    ctx_->OMSetDepthStencilState(ds_light_.Get(), 1);
    ctx_->RSSetState(rs_none_.Get());
    float zero_shroud = 0;
    cbs_.set("shroud_PS", "g_shroud_enabled", &zero_shroud, 4);
    float4x4 ident;
    cbs_.set_m("directional_light_PS", "g_overlay_view_projection", ident.data());
    cbs_.upload(dev_, ctx_);
    for (int i = 0; i < 4; ++i) bind_srv(ctx_, *ps, "gbuffer_channel_" + std::to_string(i + 1) + "_sampler", gb_[i].srv.Get());
    bind_srv(ctx_, *ps, "s_ssao", ao_result_ ? ao_result_ : white_srv_.Get());
    bind_srv(ctx_, *ps, "s_environment", sky_cube_srv_.Get());
    bind_srv(ctx_, *ps, "s_overlay", black_srv_.Get());
    bind_srv(ctx_, *ps, "s_shroud", white_srv_.Get());
    bind_srv(ctx_, *ps, "g_tShadowBuffer_SM50", shadow_atlas_.srv.Get());
    for (auto& [name, slot] : ps->samplers) ctx_->PSSetSamplers(slot, 1, s_clamp_point_.GetAddressOf());
    bind_sampler(ctx_, *ps, "s_environment", s_clamp_linear_.Get());
    bind_sampler(ctx_, *ps, "s_ssao", s_clamp_linear_.Get());
    bind_sampler(ctx_, *ps, "sShadowBuffer_SM50", s_shadow_cmp_.Get());
    fullscreen(*ps);
}

void Renderer::fullscreen_to(ID3D11RenderTargetView* rtv, int w, int h, const GameShader& ps) {
    ctx_->OMSetRenderTargets(1, &rtv, nullptr);
    D3D11_VIEWPORT vp{0, 0, float(w), float(h), 0, 1};
    ctx_->RSSetViewports(1, &vp);
    cbs_.set_f("render_target_dims_buffer", "g_render_target_recip_size", {1.0f / w, 1.0f / h});
    cbs_.upload(dev_, ctx_);
    fullscreen(ps);
    ID3D11ShaderResourceView* nulls[8] = {};
    ctx_->PSSetShaderResources(0, 8, nulls);
}

void Renderer::pass_tonemap(const EnvironmentParams& env, float dt) {
    const GameShader* down = shader(kTmDownLuma);
    const GameShader* stats = shader(kTmLumaStats);
    const GameShader* stats_down = shader(kTmStatsDown);
    const GameShader* points = shader(kTmPoints);
    const GameShader* high = shader(kTmHighPass);
    const GameShader* bx = shader(kTmBlurX);
    const GameShader* by = shader(kTmBlurY);
    const GameShader* tm = shader(kTmToneMap);
    if (!down || !stats || !stats_down || !points || !high || !bx || !by || !tm) return;
    ctx_->OMSetDepthStencilState(ds_off_.Get(), 0);
    ctx_->RSSetState(rs_none_.Get());
    // Inputs are bound after unbinding the outputs: D3D11 drops an SRV whose resource is a bound RTV.
    auto bind = [&](const GameShader& s, ID3D11SamplerState* st) {
        ctx_->OMSetRenderTargets(0, nullptr, nullptr);
        bind_shader(ctx_, s, cbs_);
        for (auto& [n, slot] : s.samplers) ctx_->PSSetSamplers(slot, 1, &st);
    };

    // 1. quarter-size copy with luma in .w
    bind(*down, s_clamp_linear_.Get());
    bind_srv(ctx_, *down, "g_hdr_rgb_texture_sampler", hdr_.srv.Get());
    fullscreen_to(quarter_.t.rtv.Get(), quarter_.w, quarter_.h, *down);

    // 2. luma statistics pyramid down to 1x1 (min, max, average), clamped by the absolute points
    ID3D11ShaderResourceView* src = quarter_.t.srv.Get();
    for (size_t i = 0; i < stats_.size(); ++i) {
        const GameShader& s = i == 0 ? *stats : *stats_down;
        bind(s, s_clamp_point_.Get());
        bind_srv(ctx_, s, i == 0 ? "g_luma_values_texture_sampler" : "g_luma_stats_texture_sampler", src);
        fullscreen_to(stats_[i].t.rtv.Get(), stats_[i].w, stats_[i].h, s);
        src = stats_[i].t.srv.Get();
    }

    // 3. black/white points with temporal adaptation (prev -> cur); the first frame snaps.
    int prev = points_cur_, cur = 1 - points_cur_;
    cbs_.set_f("auto_exposure_buffer", "g_auto_exposure_dt", {frame_ == 0 ? 1000.0f : dt});
    bind(*points, s_clamp_point_.Get());
    bind_srv(ctx_, *points, "g_prev_black_and_white_points_sampler", points_[prev].t.srv.Get());
    bind_srv(ctx_, *points, "g_luma_stats_texture_sampler", stats_.back().t.srv.Get());
    fullscreen_to(points_[cur].t.rtv.Get(), 1, 1, *points);
    points_cur_ = cur;
    // The constant tone mapper takes the points from a cbuffer: read back the previous frame's result.
    // Ring of staging copies; the oldest one (kReadbackRing - 1 frames back) is finished on the GPU.
    ctx_->CopyResource(points_staging_[frame_ % kReadbackRing].Get(), points_[cur].t.tex.Get());
    if (frame_ + 1 >= kReadbackRing) {
        D3D11_MAPPED_SUBRESOURCE ms;
        auto* st = points_staging_[(frame_ + 1) % kReadbackRing].Get();
        if (SUCCEEDED(ctx_->Map(st, 0, D3D11_MAP_READ, 0, &ms))) {
            const float* p = static_cast<const float*>(ms.pData);
            for (int k = 0; k < 4; ++k) debug_points[k] = p[k];
            ++debug_maps;
            if (p[1] > p[0]) {
                luma_black_ = p[0];
                luma_white_ = p[1];
            }
            ctx_->Unmap(st, 0);
        }
    }
    ++frame_;

    // 4. bloom: high pass on the quarter image, separable 11-tap blur
    bind(*high, s_clamp_point_.Get());
    bind_srv(ctx_, *high, "g_black_and_white_points_sampler", points_[cur].t.srv.Get());
    bind_srv(ctx_, *high, "g_hdr_rgb_texture_sampler", quarter_.t.srv.Get());
    fullscreen_to(bloom_a_.t.rtv.Get(), bloom_a_.w, bloom_a_.h, *high);
    bind(*bx, s_clamp_linear_.Get());
    bind_srv(ctx_, *bx, "g_hdr_rgb_texture_sampler", bloom_a_.t.srv.Get());
    fullscreen_to(bloom_b_.t.rtv.Get(), bloom_b_.w, bloom_b_.h, *bx);
    bind(*by, s_clamp_linear_.Get());
    bind_srv(ctx_, *by, "g_hdr_rgb_texture_sampler", bloom_b_.t.srv.Get());
    fullscreen_to(bloom_a_.t.rtv.Get(), bloom_a_.w, bloom_a_.h, *by);

    // 5. tone map with bloom and the S-curve
    uint8_t curve[256 * 4];
    build_scurve(env, curve);
    if (std::memcmp(curve, scurve_last_, sizeof curve) != 0) {
        ctx_->UpdateSubresource(scurve_tex_.Get(), 0, nullptr, curve, 256 * 4, 0);
        std::memcpy(scurve_last_, curve, sizeof curve);
    }
    bind(*tm, s_clamp_linear_.Get());
    bind_sampler(ctx_, *tm, "g_hdr_rgb_texture_sampler", s_clamp_point_.Get());
    bind_sampler(ctx_, *tm, "g_black_and_white_points_sampler", s_clamp_point_.Get());
    bind_srv(ctx_, *tm, "g_black_and_white_points_sampler", points_[cur].t.srv.Get());
    bind_srv(ctx_, *tm, "g_hdr_rgb_texture_sampler", hdr_.srv.Get());
    bind_srv(ctx_, *tm, "g_hdr_rgb_bloom_texture_sampler", bloom_a_.t.srv.Get());
    bind_srv(ctx_, *tm, "g_scurve_texture_sampler", scurve_srv_.Get());
    fullscreen_to(ldr_.t.rtv.Get(), ldr_.w, ldr_.h, *tm);
}

void Renderer::pass_to_screen() {
    ID3D11ShaderResourceView* distortion = particles_ ? particles_->distortion() : nullptr;
    const GameShader* ps = distortion ? shader(kToScreenDistortPS) : nullptr;
    if (!ps) ps = shader(kToScreenPS), distortion = nullptr;
    if (!ps) return;
    float zero2[2] = {0, 0};
    cbs_.set("shared_fog_of_war_PS", "g_fog_of_war_blend", zero2, 8); // battle: no fog of war
    ctx_->OMSetRenderTargets(0, nullptr, nullptr);
    bind_shader(ctx_, *ps, cbs_);
    for (auto& [n, slot] : ps->samplers) ctx_->PSSetSamplers(slot, 1, s_clamp_point_.GetAddressOf());
    bind_srv(ctx_, *ps, "frame_sampler", ldr_.t.srv.Get());
    bind_srv(ctx_, *ps, "gbuffer_channel_4_sampler", gb_[3].srv.Get());
    bind_srv(ctx_, *ps, "s_fog_of_war_mask", black_srv_.Get());
    if (distortion) {
        bind_srv(ctx_, *ps, "distortion_sampler", distortion);
        bind_sampler(ctx_, *ps, "distortion_sampler", s_clamp_linear_.Get());
        bind_sampler(ctx_, *ps, "frame_sampler", s_clamp_linear_.Get());
    }
    fullscreen_to(final_.rtv.Get(), w_, h_, *ps);
}

void Renderer::pass_shadows(const TestScene* scene, const Camera& cam, float aspect, const float3& light_dir, float time_s) {
    const float far_split = std::min(cam.zfar, 1500.0f);
    // Split distances (view depth). ASSUMPTION: practical split scheme (log/uniform blend 0.75); the game's
    // values depend on its camera (dump: 120, 147, 228, 416).
    float splits[5];
    const float n = std::max(cam.znear, 1.0f);
    for (int i = 0; i <= 4; ++i) {
        float t = i / 4.0f;
        float lg = n * std::pow(far_split / n, t), un = n + (far_split - n) * t;
        splits[i] = 0.75f * lg + 0.25f * un;
    }
    const float4x4 cam_view = look_at_lh(cam.eye, cam.target, {0, 1, 0});
    const float4x4 inv_view = inverse(cam_view);
    const float tan_y = std::tan(cam.fov_y * 0.5f), tan_x = tan_y * aspect;
    float3 up = std::fabs(light_dir.y) > 0.99f ? float3{0, 0, 1} : float3{0, 1, 0};

    float matrices[4][16];
    float bounds[4][4] = {{0, 0, 0.5f, 0.5f}, {0.5f, 0, 1, 0.5f}, {0, 0.5f, 0.5f, 1}, {0.5f, 0.5f, 1, 1}};
    float4x4 light_vp[4];
    for (int c = 0; c < 4; ++c) {
        // Bounding sphere of the frustum slice (stable size while the camera rotates).
        float3 pts[8];
        for (int k = 0; k < 8; ++k) {
            float d = (k & 4) ? splits[c + 1] : splits[c];
            float3 v{((k & 1) ? 1.0f : -1.0f) * tan_x * d, ((k & 2) ? 1.0f : -1.0f) * tan_y * d, d};
            pts[k] = inv_view.transform_point(v);
        }
        float3 centre{0, 0, 0};
        for (auto& p : pts) centre = centre + p * 0.125f;
        float radius = 0;
        for (auto& p : pts) radius = std::max(radius, length(p - centre));
        radius = std::ceil(radius);
        // Snap the centre to shadow texels in light space to avoid shimmering.
        float4x4 lview = look_at_lh(centre - light_dir * 2000.0f, centre, up);
        float texel = 2 * radius / (kShadowSize / 2);
        float3 lc = lview.transform_point(centre);
        float sx = std::floor(lc.x / texel) * texel - lc.x, sy = std::floor(lc.y / texel) * texel - lc.y;
        float4x4 snap = translation({sx, sy, 0});
        light_vp[c] = ortho_lh(2 * radius, 2 * radius, 0.0f, 4000.0f) * snap * lview;
        // World -> atlas uv (y down) + depth: matches the lighting shader's lookup.
        float4x4 to_uv;
        float bx = bounds[c][0], by = bounds[c][1];
        to_uv.m[0][0] = 0.25f; to_uv.m[0][3] = bx + 0.25f;
        to_uv.m[1][1] = -0.25f; to_uv.m[1][3] = by + 0.25f;
        float4x4 m = to_uv * light_vp[c];
        std::memcpy(matrices[c], m.data(), 64);
    }

    // Render casters into the four quadrants.
    const GameShader* vs = shader(kShadowVS);
    const GameShader* ps = shader(kShadowPS);
    const float one[4] = {1, 1, 1, 1};
    ctx_->OMSetRenderTargets(0, nullptr, nullptr);
    ctx_->ClearRenderTargetView(shadow_atlas_.rtv.Get(), one);
    ctx_->ClearDepthStencilView(shadow_dsv_.Get(), D3D11_CLEAR_DEPTH, 1.0f, 0);
    if (vs && ps && scene && shadows) {
        std::vector<D3D11_INPUT_ELEMENT_DESC> el = {
            {"POSITION", 0, DXGI_FORMAT_R16G16B16A16_FLOAT, 0, 0, D3D11_INPUT_PER_VERTEX_DATA, 0},
            {"TEXCOORD", 0, DXGI_FORMAT_R16G16B16A16_FLOAT, 0, 8, D3D11_INPUT_PER_VERTEX_DATA, 0},
            {"NORMAL", 0, DXGI_FORMAT_B8G8R8A8_UNORM, 0, 16, D3D11_INPUT_PER_VERTEX_DATA, 0},
            {"NORMAL", 1, DXGI_FORMAT_B8G8R8A8_UNORM, 0, 20, D3D11_INPUT_PER_VERTEX_DATA, 0},
            {"NORMAL", 2, DXGI_FORMAT_B8G8R8A8_UNORM, 0, 24, D3D11_INPUT_PER_VERTEX_DATA, 0},
            {"COLOR", 0, DXGI_FORMAT_B8G8R8A8_UNORM, 0, 28, D3D11_INPUT_PER_VERTEX_DATA, 0},
        };
        for (UINT i = 1; i <= 7; ++i)
            el.push_back({"POSITION", i, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, (i - 1) * 16, D3D11_INPUT_PER_INSTANCE_DATA, 1});
        ctx_->IASetInputLayout(lib_->layout(*vs, "rigid_std", el));
        ctx_->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        ctx_->OMSetRenderTargets(1, shadow_atlas_.rtv.GetAddressOf(), shadow_dsv_.Get());
        ctx_->OMSetDepthStencilState(ds_sky_.Get(), 0);
        ctx_->OMSetDepthStencilState(ds_gbuffer_.Get(), 0);
        ctx_->RSSetState(rs_shadow_.Get());
        for (int c = 0; c < 4; ++c) {
            D3D11_VIEWPORT vp{bounds[c][0] * kShadowSize, bounds[c][1] * kShadowSize, kShadowSize / 2.0f, kShadowSize / 2.0f, 0, 1};
            ctx_->RSSetViewports(1, &vp);
            cbs_.set_m("camera_VS_PS", "view_projection", light_vp[c].data());
            cbs_.upload(dev_, ctx_);
            bind_shader(ctx_, *vs, cbs_);
            bind_shader(ctx_, *ps, cbs_);
            for (auto& [name, slot] : vs->textures) ctx_->VSSetShaderResources(slot, 1, instance_ids_srv_.GetAddressOf());
            for (auto& o : scene->objects()) {
                if (o.model->path == "(ground)") continue; // receiver only
                RigidInstance inst{};
                for (int r = 0; r < 3; ++r)
                    for (int k = 0; k < 4; ++k) inst.world[r][k] = o.world.m[r][k];
                D3D11_MAPPED_SUBRESOURCE ms;
                if (FAILED(ctx_->Map(instance_vb_.Get(), 0, D3D11_MAP_WRITE_DISCARD, 0, &ms))) continue;
                std::memcpy(ms.pData, &inst, sizeof inst);
                ctx_->Unmap(instance_vb_.Get(), 0);
                for (auto& m : o.model->meshes) {
                    ID3D11Buffer* vbs[2] = {m.vb.Get(), instance_vb_.Get()};
                    UINT strides[2] = {32, sizeof(RigidInstance)}, offs[2] = {0, 0};
                    ctx_->IASetVertexBuffers(0, 2, vbs, strides, offs);
                    ctx_->IASetIndexBuffer(m.ib.Get(), DXGI_FORMAT_R16_UINT, 0);
                    ctx_->DrawIndexedInstanced(m.index_count, 1, 0, 0, 0);
                }
            }
        }
    }
    ctx_->OMSetRenderTargets(0, nullptr, nullptr);
    (void)time_s;

    // shadowmap_PS as the game fills it (layout and constants from the memory dump).
    float buf_size[4] = {float(kShadowSize), float(kShadowSize), 1.0f / kShadowSize, 1.0f / kShadowSize};
    cbs_.set("shadowmap_PS", "g_vHardShadowBufferSize", buf_size, 16);
    cbs_.set("shadowmap_PS", "g_amHardSplit", matrices, sizeof matrices);
    float dist[17] = {};
    for (int i = 0; i <= 4; ++i) dist[i * 4] = splits[i]; // float[5], 16-byte stride
    cbs_.set("shadowmap_PS", "g_split_distances", dist, sizeof dist);
    cbs_.set("shadowmap_PS", "g_split_uv_texture_bounds", bounds, sizeof bounds);
    cbs_.set_f("shadowmap_PS", "g_fFadeRange", {0.1f, 0.296399f});
    cbs_.set_f("shadowmap_PS", "g_sample_bias", {0.0f});
    cbs_.set_f("shadowmap_PS", "g_iSplitCount", {shadows ? 4.0f : 0.0f});
    cbs_.set_f("shadowmap_PS", "g_shadow_light_direction", {light_dir.x, light_dir.y, light_dir.z});
}

void Renderer::pass_sun(const EnvironmentParams& env, const float4x4& view_m, const float4x4& proj) {
    const GameShader* vs = shader(kSunVS);
    const GameShader* ps = shader(kSunPS);
    if (!vs || !ps || !env.has_sky || env.sun_scale <= 0) return;
    std::string tex = "rigidmodels/skybox/" + env.sun_texture;
    if (tex != sun_texture_path_) {
        sun_texture_path_ = tex;
        sun_srv_.Reset();
        try {
            Texture t = load_dds(dev_, vfs_.read(tex), true);
            sun_srv_ = t.srv;
        } catch (const std::exception& e) {
            error(std::string("sun texture: ") + e.what());
        }
    }
    if (!sun_srv_) return;
    // g_world_view: camera rotation, translation = rotation * (to_sun * distance) (camera-relative).
    float4x4 wv = view_m;
    float3 s = env.to_sun() * kSunDistance;
    for (int r = 0; r < 3; ++r) wv.m[r][3] = view_m.m[r][0] * s.x + view_m.m[r][1] * s.y + view_m.m[r][2] * s.z;
    cbs_.set_m("sun_VS", "g_world_view", wv.data());
    cbs_.set_m("sun_VS", "g_projection", proj.data());
    cbs_.set_f("sun_VS", "g_sun_scale", {env.sun_scale});
    cbs_.upload(dev_, ctx_);
    std::vector<D3D11_INPUT_ELEMENT_DESC> el = {{"POSITION", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 0, D3D11_INPUT_PER_VERTEX_DATA, 0},
                                                {"TEXCOORD", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 8, D3D11_INPUT_PER_VERTEX_DATA, 0}};
    ctx_->IASetInputLayout(lib_->layout(*vs, "sun", el));
    UINT stride = 16, off = 0;
    ctx_->IASetVertexBuffers(0, 1, sun_vb_.GetAddressOf(), &stride, &off);
    ctx_->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    ctx_->OMSetRenderTargets(1, hdr_.rtv.GetAddressOf(), dsv_.Get());
    ctx_->OMSetDepthStencilState(ds_sky_.Get(), 0);
    ctx_->OMSetBlendState(bs_additive_alpha_.Get(), nullptr, 0xffffffff);
    ctx_->RSSetState(rs_none_.Get());
    bind_shader(ctx_, *vs, cbs_);
    bind_shader(ctx_, *ps, cbs_);
    bind_srv(ctx_, *ps, "s_diffuse", sun_srv_.Get());
    for (auto& [n, slot] : ps->samplers) ctx_->PSSetSamplers(slot, 1, s_clamp_linear_.GetAddressOf());
    ctx_->Draw(6, 0);
    ctx_->OMSetBlendState(bs_opaque_.Get(), nullptr, 0xffffffff);
    // Back to the main camera for the following passes (the sun pass only touched sun_VS).
}

ID3D11ShaderResourceView* Renderer::sky_texture(const std::string& path) {
    auto it = sky_textures_.find(path);
    if (it != sky_textures_.end()) return it->second.Get();
    Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> srv;
    try {
        srv = load_dds(dev_, vfs_.read(path), true).srv; // ASSUMPTION: cloud atlases sampled as sRGB like other colour maps
    } catch (const std::exception& e) {
        error(path + ": " + e.what());
    }
    sky_textures_[path] = srv;
    return srv.Get();
}

void Renderer::pass_clouds(const EnvironmentParams& env) {
    const GameShader* vs = shader(kCloudVS);
    const GameShader* ps = shader(kCloudPS);
    if (!vs || !ps || !clouds) return;
    cloud_gpu_.resize(env.cloud_planes.size());
    std::vector<int> order;
    for (int i = 0; i < int(env.cloud_planes.size()); ++i)
        if (env.cloud_planes[i].far_plane) order.insert(order.begin(), i); // far plane first
        else order.push_back(i);
    std::vector<D3D11_INPUT_ELEMENT_DESC> el = {
        {"POSITION", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 0, D3D11_INPUT_PER_VERTEX_DATA, 0},
        {"TEXCOORD", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 8, D3D11_INPUT_PER_VERTEX_DATA, 0},
        {"TEXCOORD", 1, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 0, D3D11_INPUT_PER_INSTANCE_DATA, 1},
        {"TEXCOORD", 2, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 16, D3D11_INPUT_PER_INSTANCE_DATA, 1},
        {"TEXCOORD", 3, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 32, D3D11_INPUT_PER_INSTANCE_DATA, 1},
        {"TEXCOORD", 4, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 48, D3D11_INPUT_PER_INSTANCE_DATA, 1},
        {"TEXCOORD", 5, DXGI_FORMAT_R32G32_FLOAT, 1, 64, D3D11_INPUT_PER_INSTANCE_DATA, 1},
    };
    ctx_->IASetInputLayout(lib_->layout(*vs, "clouds", el));
    ctx_->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    ctx_->OMSetRenderTargets(1, hdr_.rtv.GetAddressOf(), dsv_.Get());
    ctx_->OMSetDepthStencilState(ds_sky_.Get(), 0);
    ctx_->OMSetBlendState(bs_alpha_.Get(), nullptr, 0xffffffff);
    ctx_->RSSetState(rs_none_.Get());
    const float3 to_sun = env.to_sun();
    for (int i : order) {
        const CloudPlane& p = env.cloud_planes[i];
        CloudGpu& g = cloud_gpu_[i];
        // Regenerate when anything that affects spawning changes.
        std::string sig = std::to_string(cloud_seed) + "|" + std::to_string(p.grid_size) + "|" + std::to_string(p.density) + "|" +
                          std::to_string(p.height_variation) + "|" + std::to_string(p.rotation) + "|" + std::to_string(p.far_plane);
        for (auto& c : p.clouds) {
            sig += "|" + std::to_string(c.this_cloud_only) + std::to_string(c.probability);
            for (auto& e : c.elements)
                sig += ";" + std::to_string(e.enabled) + std::to_string(e.scale) + std::to_string(e.offset[0]) + std::to_string(e.offset[1]) +
                       std::to_string(e.offset[2]) + std::to_string(e.texture_index) + std::to_string(e.texture_row) + std::to_string(e.texture_column);
        }
        bool dirty = false;
        if (sig != g.signature) {
            g.signature = sig;
            g.field.reset(p, cloud_seed * 7919u + uint32_t(i));
            dirty = true;
        }
        if (p.velocity * 0.1f > 0.001f && cloud_dt_ > 0) {
            g.field.update(p, cloud_dt_);
            dirty = true;
        }
        if (dirty) {
            const auto& inst = g.field.instances();
            g.count = UINT(inst.size());
            if (g.count > g.capacity || !g.instances) {
                g.capacity = std::max<UINT>(g.count + 256, 1024);
                D3D11_BUFFER_DESC bd{UINT(g.capacity * sizeof(CloudInstance)), D3D11_USAGE_DYNAMIC, D3D11_BIND_VERTEX_BUFFER, D3D11_CPU_ACCESS_WRITE};
                g.instances.Reset();
                dev_->CreateBuffer(&bd, nullptr, &g.instances);
            }
            D3D11_MAPPED_SUBRESOURCE ms;
            if (g.count && SUCCEEDED(ctx_->Map(g.instances.Get(), 0, D3D11_MAP_WRITE_DISCARD, 0, &ms))) {
                std::memcpy(ms.pData, inst.data(), inst.size() * sizeof(CloudInstance));
                ctx_->Unmap(g.instances.Get(), 0);
            }
        }
        if (!g.count || !g.instances) continue;

        // cloud_plane_VS / cloud_plane_PS as the game fills them (memory dump): colours raw except the shadow
        // colour (pow 2.2); horizon_alpha_start remapped (0.7 -> 0.97; ASSUMPTION 1 - (1 - x) / 10).
        cbs_.set_f("cloud_plane_VS", "g_wind_alignment", {0.0f}); // ASSUMPTION: wind direction angle (dump: -1.076)
        cbs_.set_f("cloud_plane_VS", "g_cloud_shadow_gradient", {p.shadow_gradient});
        cbs_.set_f("cloud_plane_VS", "g_height_scale", {p.height_scale});
        cbs_.set_f("cloud_plane_PS", "g_sun_direction", {to_sun.x, to_sun.y, to_sun.z});
        cbs_.set_f("cloud_plane_PS", "g_sun_transmitted_colour", {p.transmitted[0], p.transmitted[1], p.transmitted[2]});
        cbs_.set_f("cloud_plane_PS", "g_sun_reflected_colour", {p.reflected[0], p.reflected[1], p.reflected[2]});
        cbs_.set_f("cloud_plane_PS", "g_transmit_falloff", {p.transmit_falloff});
        cbs_.set_f("cloud_plane_PS", "g_flatten_factor", {p.flatten});
        float3 sh = colour_to_linear({p.shadow[0], p.shadow[1], p.shadow[2]});
        cbs_.set_f("cloud_plane_PS", "g_cloud_shadow_colour", {sh.x, sh.y, sh.z});
        cbs_.set_f("cloud_plane_PS", "g_cloud_shadow_falloff", {p.shadow_falloff});
        cbs_.set_f("cloud_plane_PS", "g_horizon_alpha_start", {1.0f - (1.0f - p.horizon_alpha_start) * 0.1f});
        cbs_.set_f("cloud_plane_PS", "g_horizon_alpha_end", {p.horizon_alpha_end});
        cbs_.upload(dev_, ctx_);
        bind_shader(ctx_, *vs, cbs_);
        bind_shader(ctx_, *ps, cbs_);
        bind_srv(ctx_, *ps, "s_diffuse_cloud_maps_0", sky_texture(cloud_texture(p, 0)));
        bind_srv(ctx_, *ps, "s_diffuse_cloud_maps_1", sky_texture(cloud_texture(p, 1)));
        for (auto& [n, slot] : ps->samplers) ctx_->PSSetSamplers(slot, 1, s_clamp_linear_.GetAddressOf());
        ID3D11Buffer* vbs[2] = {cloud_quad_vb_.Get(), g.instances.Get()};
        UINT strides[2] = {16, sizeof(CloudInstance)}, offs[2] = {0, 0};
        ctx_->IASetVertexBuffers(0, 2, vbs, strides, offs);
        ctx_->DrawInstanced(6, g.count, 0, 0);
    }
    ctx_->OMSetBlendState(bs_opaque_.Get(), nullptr, 0xffffffff);
}

void Renderer::pass_ssao(const Camera& cam) {
    const GameShader* res = shader(kSsaoPS);
    const GameShader* bx = shader(kSsaoBlurXPS);
    const GameShader* by = shader(kSsaoBlurYPS);
    const float white[4] = {1, 1, 1, 1};
    if (!res || !bx || !by || !ssao) {
        ctx_->ClearRenderTargetView(ao_.rtv.Get(), white);
        ao_result_ = ao_.srv.Get();
        return;
    }
    // ssao_resolve_PS / ssao_blur_PS as in the memory dump (HBAO: 5 directions x 5 steps).
    cbs_.set_f("ssao_resolve_PS", "g_R", {1.0f});
    cbs_.set_f("ssao_resolve_PS", "g_ExternalAngleBias", {0.0f});
    cbs_.set_f("ssao_resolve_PS", "g_Attenuation", {0.78f});
    cbs_.set_f("ssao_resolve_PS", "g_ExternalContrast", {1.0f});
    cbs_.set_f("ssao_resolve_PS", "g_AOResolution", {float(w_), float(h_)});
    cbs_.set_f("ssao_resolve_PS", "g_VerticalFieldOfView", {cam.fov_y});
    cbs_.set_f("ssao_resolve_PS", "g_Strength", {3.0f});
    cbs_.set_f("ssao_resolve_PS", "g_MinStrength", {0.17f});
    cbs_.set_f("ssao_resolve_PS", "g_JitterResolution", {256.0f});
    cbs_.set_f("ssao_resolve_PS", "g_NumDir", {5.0f});
    cbs_.set_f("ssao_resolve_PS", "g_NumSteps", {5.0f});
    cbs_.set_f("ssao_blur_PS", "g_BlurRadius", {4.0f});
    cbs_.set_f("ssao_blur_PS", "g_BlurSharpness", {3.0f});
    cbs_.set_f("ssao_blur_PS", "g_BlurResolution", {w_ * 0.5f, h_ * 0.5f}); // dump: half the screen size
    ctx_->OMSetDepthStencilState(ds_off_.Get(), 0);
    ctx_->RSSetState(rs_none_.Get());
    auto run = [&](const GameShader& ps, ID3D11ShaderResourceView* src, Target& dst, int tw, int th) {
        ctx_->OMSetRenderTargets(0, nullptr, nullptr);
        bind_shader(ctx_, ps, cbs_);
        for (auto& [n, slot] : ps.samplers) ctx_->PSSetSamplers(slot, 1, s_clamp_point_.GetAddressOf());
        bind_sampler(ctx_, ps, "jitter_texture_sampler", s_wrap_point_.Get());
        bind_sampler(ctx_, ps, "resolve_target_sampler", s_clamp_linear_.Get());
        bind_sampler(ctx_, ps, "blur_x_target_sampler", s_clamp_linear_.Get()); // blur_y's name for its input
        bind_srv(ctx_, ps, "gbuffer_channel_4_sampler", gb_[3].srv.Get());
        bind_srv(ctx_, ps, "jitter_texture_sampler", jitter_srv_.Get());
        if (src) {
            bind_srv(ctx_, ps, "resolve_target_sampler", src);
            bind_srv(ctx_, ps, "blur_x_target_sampler", src);
        }
        ctx_->OMSetRenderTargets(1, dst.rtv.GetAddressOf(), nullptr);
        D3D11_VIEWPORT vp{0, 0, float(tw), float(th), 0, 1};
        ctx_->RSSetViewports(1, &vp);
        cbs_.upload(dev_, ctx_);
        ctx_->IASetInputLayout(nullptr);
        ctx_->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        ctx_->VSSetShader(fs_vs2_.Get(), nullptr, 0);
        bind_shader(ctx_, ps, cbs_);
        ctx_->Draw(3, 0);
        ID3D11ShaderResourceView* nulls[4] = {};
        ctx_->PSSetShaderResources(0, 4, nulls);
    };
    run(*res, nullptr, ao_, w_, h_);
    run(*bx, ao_.srv.Get(), ao_tmp_, w_ / 2, h_ / 2);
    run(*by, ao_tmp_.srv.Get(), ao_blur_, w_ / 2, h_ / 2);
    ao_result_ = ao_blur_.srv.Get();
}

void Renderer::pass_fog() {
    const GameShader* ps = shader(kFogPS);
    if (!ps || !fog) return;
    ctx_->OMSetRenderTargets(0, nullptr, nullptr);
    bind_shader(ctx_, *ps, cbs_);
    bind_srv(ctx_, *ps, "gbuffer_channel_4_sampler", gb_[3].srv.Get());
    bind_srv(ctx_, *ps, "s_sky", sky_cube_srv_.Get());
    for (auto& [n, slot] : ps->samplers) ctx_->PSSetSamplers(slot, 1, s_clamp_point_.GetAddressOf());
    bind_sampler(ctx_, *ps, "s_sky", s_clamp_linear_.Get());
    // Fog over geometry only (stencil 1): sky pixels keep the sky.
    ctx_->OMSetRenderTargets(1, hdr_.rtv.GetAddressOf(), dsv_.Get());
    ctx_->OMSetDepthStencilState(ds_light_.Get(), 1);
    ctx_->OMSetBlendState(bs_alpha_.Get(), nullptr, 0xffffffff);
    cbs_.upload(dev_, ctx_);
    fullscreen(*ps);
    ctx_->OMSetBlendState(bs_opaque_.Get(), nullptr, 0xffffffff);
    ID3D11ShaderResourceView* nulls[8] = {};
    ctx_->PSSetShaderResources(0, 8, nulls);
}

void Renderer::pass_display() {
    ctx_->OMSetRenderTargets(1, out_.rtv.GetAddressOf(), nullptr);
    ctx_->OMSetDepthStencilState(ds_off_.Get(), 0);
    struct { int mode; float exposure; float pad[2]; } dc{0, 1.0f, {}};
    ID3D11ShaderResourceView* src = hdr_.srv.Get();
    switch (view) {
    case View::Final: dc.mode = 3; src = final_.srv.Get(); break;
    case View::HdrRaw: dc.mode = 1; break;
    case View::Albedo: dc.mode = 2; src = gb_[1].srv.Get(); break;
    case View::Normal: dc.mode = 3; src = gb_[0].srv.Get(); break;
    case View::Specular: dc.mode = 3; src = gb_[2].srv.Get(); break;
    case View::Depth: dc.mode = 4; src = gb_[3].srv.Get(); break;
    case View::SkyCube: dc.mode = 5; break;
    case View::Bloom: dc.mode = 1; src = bloom_a_.t.srv.Get(); break;
    case View::Quarter: dc.mode = 1; src = quarter_.t.srv.Get(); break;
    case View::ToneMapped: dc.mode = 1; src = ldr_.t.srv.Get(); break;
    case View::AO: dc.mode = 3; src = ao_result_ ? ao_result_ : ao_.srv.Get(); break;
    }
    ctx_->UpdateSubresource(display_cb_.Get(), 0, nullptr, &dc, 0, 0);
    ctx_->IASetInputLayout(nullptr);
    ctx_->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    ctx_->VSSetShader(fs_vs_.Get(), nullptr, 0);
    ctx_->PSSetShader(display_ps_.Get(), nullptr, 0);
    ctx_->PSSetConstantBuffers(0, 1, display_cb_.GetAddressOf());
    ID3D11ShaderResourceView* srvs[2] = {src, sky_cube_srv_.Get()};
    ctx_->PSSetShaderResources(0, 2, srvs);
    ctx_->PSSetSamplers(0, 1, s_clamp_linear_.GetAddressOf());
    ctx_->Draw(3, 0);
}

void Renderer::render(const TestScene* scene, const Camera& cam, const EnvironmentParams& env, int width, int height, float time_s) {
    if (width < 8 || height < 8) return;
    resize(width, height);
    // Make sure every cbuffer this frame needs is declared before values are written.
    for (const char* s : {kSkyVS, kSkyGenPS, kSkyPS, kRigidVS, kRigidGbufferPS, kRigidGbufferAlphaPS, kDirLightPS, kTmDownLuma,
                          kTmLumaStats, kTmStatsDown, kTmPoints, kTmHighPass, kTmBlurX, kTmBlurY, kTmToneMap, kToScreenPS, kFogPS, kShadowVS, kShadowPS, kSunVS, kSunPS, kCloudVS, kCloudPS, kSsaoPS, kSsaoBlurXPS, kSsaoBlurYPS})
        shader(s);
    apply_environment(env, cbs_);

    pass_sky_cube(env, time_s);

    float aspect = float(width) / float(height);
    frame_dt_ = last_time_ < 0 ? 0.016f : std::max(0.0f, time_s - last_time_);
    last_time_ = time_s;
    cloud_dt_ = frame_dt_;
    pass_shadows(scene, cam, aspect, -env.to_sun(), time_s);
    float4x4 view_m = look_at_lh(cam.eye, cam.target, {0, 1, 0});
    float4x4 proj = perspective_lh(cam.fov_y, aspect, cam.znear, cam.zfar);
    set_camera(view_m, proj, cam.eye, width, height, time_s);
    cbs_.upload(dev_, ctx_);
    D3D11_VIEWPORT vp{0, 0, float(width), float(height), 0, 1};
    ctx_->RSSetViewports(1, &vp);
    ctx_->OMSetBlendState(bs_opaque_.Get(), nullptr, 0xffffffff);

    pass_gbuffer(scene);
    pass_ssao(cam);
    D3D11_VIEWPORT main_vp{0, 0, float(width), float(height), 0, 1};
    ctx_->RSSetViewports(1, &main_vp);
    const float clear[4] = {0, 0, 0, 0};
    ctx_->ClearRenderTargetView(hdr_.rtv.Get(), clear);
    pass_sky_forward();
    pass_sun(env, view_m, proj);
    pass_clouds(env);
    pass_directional_light();
    pass_fog();
    if (particles_) { // weather precipitation (vfx1 of the WEATHER definition), see src/vfx/particles.h
        particles_->set_weather(env.weather_definition, vfx_revision);
        ParticleSystem::Frame pf;
        pf.eye = cam.eye;
        pf.view = view_m;
        pf.time = time_s;
        pf.dt = frame_dt_;
        pf.wind_velocity = particles_->wind_velocity();
        pf.target = hdr_.rtv.Get();
        pf.depth = dsv_.Get();
        pf.depth_zw = gb_[3].srv.Get();
        pf.sky = sky_cube_srv_.Get();
        pf.width = width;
        pf.height = height;
        particles_->render(pf);
        ctx_->RSSetViewports(1, &main_vp);
    }

    ID3D11ShaderResourceView* nulls[16] = {};
    ctx_->PSSetShaderResources(0, 16, nulls);
    float dt = frame_dt_;
    pass_tonemap(env, dt);
    if (view == View::Final) pass_to_screen();
    pass_display(); // Final: copies final_ with alpha = 1 for the viewport
    ctx_->PSSetShaderResources(0, 16, nulls);
    ctx_->OMSetRenderTargets(0, nullptr, nullptr);
}

} // namespace we
