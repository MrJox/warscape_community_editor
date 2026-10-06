#include "renderer.h"

#include "core/vfs.h"
#include "render/scene.h"

#include <d3dcompiler.h>

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

// Editor-side helpers (not game shaders): full-screen triangle and the debug/temporary display pass.
const char* kHelperHlsl = R"(
struct VSOut { float4 pos : SV_Position; float2 uv : TEXCOORD0; };
VSOut fs_vs(uint id : SV_VertexID) {
    VSOut o;
    o.uv = float2((id << 1) & 2, id & 2);
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
    make_states();

    auto vs = compile("fs_vs", "vs_5_0");
    dev_->CreateVertexShader(vs->GetBufferPointer(), vs->GetBufferSize(), nullptr, &fs_vs_);
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
    gb_[4] = make_target(w, h, DXGI_FORMAT_R32_FLOAT);      // per-instance value
    hdr_ = make_target(w, h, DXGI_FORMAT_R16G16B16A16_FLOAT);
    out_ = make_target(w, h, DXGI_FORMAT_R8G8B8A8_UNORM);
    out_srv_ = out_.srv;
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
    // No shadow cascades yet: zero splits (shader then skips the shadow lookup).
    cbs_.set_f("shadowmap_PS", "g_iSplitCount", {0.0f});
    float zero_shroud = 0;
    cbs_.set("shroud_PS", "g_shroud_enabled", &zero_shroud, 4);
    float4x4 ident;
    cbs_.set_m("directional_light_PS", "g_overlay_view_projection", ident.data());
    cbs_.upload(dev_, ctx_);
    for (int i = 0; i < 4; ++i) bind_srv(ctx_, *ps, "gbuffer_channel_" + std::to_string(i + 1) + "_sampler", gb_[i].srv.Get());
    bind_srv(ctx_, *ps, "s_ssao", white_srv_.Get());
    bind_srv(ctx_, *ps, "s_environment", sky_cube_srv_.Get());
    bind_srv(ctx_, *ps, "s_overlay", black_srv_.Get());
    bind_srv(ctx_, *ps, "s_shroud", white_srv_.Get());
    bind_srv(ctx_, *ps, "g_tShadowBuffer_SM50", shadow_srv_.Get());
    for (auto& [name, slot] : ps->samplers) ctx_->PSSetSamplers(slot, 1, s_clamp_point_.GetAddressOf());
    bind_sampler(ctx_, *ps, "s_environment", s_clamp_linear_.Get());
    bind_sampler(ctx_, *ps, "s_ssao", s_clamp_linear_.Get());
    bind_sampler(ctx_, *ps, "sShadowBuffer_SM50", s_shadow_cmp_.Get());
    fullscreen(*ps);
}

void Renderer::pass_display() {
    ctx_->OMSetRenderTargets(1, out_.rtv.GetAddressOf(), nullptr);
    ctx_->OMSetDepthStencilState(ds_off_.Get(), 0);
    struct { int mode; float exposure; float pad[2]; } dc{0, 1.0f, {}};
    ID3D11ShaderResourceView* src = hdr_.srv.Get();
    switch (view) {
    case View::Final: dc.mode = 0; break;
    case View::HdrRaw: dc.mode = 1; break;
    case View::Albedo: dc.mode = 2; src = gb_[1].srv.Get(); break;
    case View::Normal: dc.mode = 3; src = gb_[0].srv.Get(); break;
    case View::Specular: dc.mode = 3; src = gb_[2].srv.Get(); break;
    case View::Depth: dc.mode = 4; src = gb_[3].srv.Get(); break;
    case View::SkyCube: dc.mode = 5; break;
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
    for (const char* s : {kSkyVS, kSkyGenPS, kSkyPS, kRigidVS, kRigidGbufferPS, kRigidGbufferAlphaPS, kDirLightPS}) shader(s);
    apply_environment(env, cbs_);

    pass_sky_cube(env, time_s);

    float aspect = float(width) / float(height);
    float4x4 view_m = look_at_lh(cam.eye, cam.target, {0, 1, 0});
    float4x4 proj = perspective_lh(cam.fov_y, aspect, cam.znear, cam.zfar);
    set_camera(view_m, proj, cam.eye, width, height, time_s);
    cbs_.upload(dev_, ctx_);
    D3D11_VIEWPORT vp{0, 0, float(width), float(height), 0, 1};
    ctx_->RSSetViewports(1, &vp);
    ctx_->OMSetBlendState(bs_opaque_.Get(), nullptr, 0xffffffff);

    pass_gbuffer(scene);
    const float clear[4] = {0, 0, 0, 0};
    ctx_->ClearRenderTargetView(hdr_.rtv.Get(), clear);
    pass_sky_forward();
    pass_directional_light();

    ID3D11ShaderResourceView* nulls[16] = {};
    ctx_->PSSetShaderResources(0, 16, nulls);
    pass_display();
    ctx_->PSSetShaderResources(0, 16, nulls);
    ctx_->OMSetRenderTargets(0, nullptr, nullptr);
}

} // namespace we
