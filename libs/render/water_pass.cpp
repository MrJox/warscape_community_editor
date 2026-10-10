// The water pass (Phase 4 of docs/plans/tile_ted_parity.md, docs/re/water.md): river ribbons and sea / lake planes of the scene's Water entities, shaded into the HDR
// target after the fog, before the post passes. Not the game's shaders (rigidriver ps30_main_battle_river, waterplane): the same ingredients in a small own
// shader - the river's scrolling normal map in three scales, a Schlick fresnel between the sky cube and a body colour from the environment's deep / shallow
// water colours, the sun's glint, opacity from the water's thickness over the ground behind it (the G-buffer depth) and a soft bank.
#include "foundation/profile.h"
#include "render/renderer.h"
#include "scene/environment.h"

#include <d3dcompiler.h>

#include <algorithm>
#include <cmath>
#include <cstring>
#include <stdexcept>

using Microsoft::WRL::ComPtr;

namespace wce {

namespace {

const char* kWaterHlsl = R"(
cbuffer W : register(b0) {
    row_major float4x4 vp;
    float3 eye; float time;
    float3 sun_dir; float near_p;     // sun_dir: towards the sun
    float3 sun_rad; float far_p;      // the sun's radiance in HDR units
    float3 deep; float is_sea;
    float3 shallow; float foam;
    float2 inv_screen; float2 pad2;
};
Texture2D normal_tex : register(t0);
TextureCube sky : register(t1);
Texture2D depth_zw : register(t2);
SamplerState wrap_s : register(s0);
SamplerState clamp_s : register(s1);

struct VIn { float3 pos : POSITION; float2 uv : TEXCOORD0; float flow : TEXCOORD1; float fade : TEXCOORD2; };
struct VOut { float4 sv : SV_Position; float3 wp : TEXCOORD0; float2 uv : TEXCOORD1; float2 ff : TEXCOORD2; float vz : TEXCOORD3; };

VOut water_vs(VIn i) {
    VOut o;
    o.sv = mul(vp, float4(i.pos, 1));
    o.wp = i.pos;
    o.uv = i.uv;
    o.ff = float2(i.flow, i.fade);
    o.vz = o.sv.w;
    return o;
}

float3 decode_n(float4 t) {
    float2 xy = (t.wy + 0.001961) * 2 - 1;
    return float3(xy.x, sqrt(max(1 - dot(xy, xy), 0)), xy.y);
}

float4 water_ps(VOut i) : SV_Target {
    float flow = i.ff.x, fade = i.ff.y;
    float2 tc;
    float scroll = time * (0.25 + flow * 0.6);
    if (is_sea > 0.5) tc = i.uv * 0.045 + float2(time * 0.012, time * 0.007);
    else tc = float2(i.uv.x * 0.059, i.uv.y * 0.177 - scroll * 0.6);
    float2 tc2 = tc * 1.7 + float2(0.4, -scroll * 0.35);
    float2 tc3 = tc * 4.1 + float2(0.1, -scroll * 0.9);
    float3 n1 = decode_n(normal_tex.Sample(wrap_s, tc));
    float3 n2 = decode_n(normal_tex.Sample(wrap_s, tc2));
    float3 n3 = decode_n(normal_tex.Sample(wrap_s, tc3));
    float3 N = normalize(n1 + n2 + n3 * 0.3 + float3(0, 1, 0));
    float3 V = normalize(eye - i.wp);
    float ndv = saturate(dot(N, V));
    float fres = 0.04 + 0.96 * pow(1 - ndv, 5);
    float3 R = reflect(-V, N);
    R.y = abs(R.y);
    float3 refl = sky.SampleLevel(clamp_s, R, 0).rgb;
    // thickness of the water over what is behind it (view-space depth of the ground minus the water's)
    float zw = depth_zw.Load(int3(i.sv.xy, 0)).r;
    float ground = zw <= 0 ? 1e5 : near_p * far_p / (far_p - zw * (far_p - near_p));
    float thick = max(ground - i.vz, 0);
    float d01 = saturate(thick / (is_sea > 0.5 ? 14.0 : 5.0));
    float3 amb = sky.SampleLevel(clamp_s, float3(N.x, 1, N.z), 7).rgb;
    float3 light = amb * 2.2 + sun_rad * saturate(dot(N, sun_dir)) * 0.5;
    float3 body = lerp(shallow, deep, d01) * light;
    float spec = pow(saturate(dot(R, sun_dir)), 220) * 6;
    float3 col = lerp(body, refl, fres) + sun_rad * spec;
    float alpha = saturate(0.30 + 0.70 * d01);
    alpha *= saturate(thick * 1.2 + 0.02) * fade;
    if (foam > 0 && is_sea < 0.5) {
        float shore = saturate(1 - thick * 0.9);
        col += float3(1, 1, 1) * sun_rad.x * 0.12 * foam * shore * alpha * (0.5 + 0.5 * n3.x);
    }
    return float4(col, alpha);
}
)";

struct WaterCb {
    float vp[16];
    float eye[3], time;
    float sun_dir[3], near_p;
    float sun_rad[3], far_p;
    float deep[3], is_sea;
    float shallow[3], foam;
    float inv_screen[2], pad2[2];
};

ComPtr<ID3DBlob> compile_water(const char* entry, const char* target) {
    ComPtr<ID3DBlob> code, err;
    if (FAILED(D3DCompile(kWaterHlsl, std::strlen(kWaterHlsl), "water", nullptr, nullptr, entry, target, 0, 0, &code, &err)))
        throw std::runtime_error(std::string("water shader: ") + (err ? static_cast<const char*>(err->GetBufferPointer()) : entry));
    return code;
}

} // namespace

void Renderer::create_water_resources() {
    if (water_vs_) return;
    auto vs = compile_water("water_vs", "vs_5_0");
    dev_->CreateVertexShader(vs->GetBufferPointer(), vs->GetBufferSize(), nullptr, &water_vs_);
    const D3D11_INPUT_ELEMENT_DESC el[] = {{"POSITION", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 0, D3D11_INPUT_PER_VERTEX_DATA, 0},
                                           {"TEXCOORD", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 12, D3D11_INPUT_PER_VERTEX_DATA, 0},
                                           {"TEXCOORD", 1, DXGI_FORMAT_R32_FLOAT, 0, 20, D3D11_INPUT_PER_VERTEX_DATA, 0},
                                           {"TEXCOORD", 2, DXGI_FORMAT_R32_FLOAT, 0, 24, D3D11_INPUT_PER_VERTEX_DATA, 0}};
    dev_->CreateInputLayout(el, 4, vs->GetBufferPointer(), vs->GetBufferSize(), &water_il_);
    auto ps = compile_water("water_ps", "ps_5_0");
    dev_->CreatePixelShader(ps->GetBufferPointer(), ps->GetBufferSize(), nullptr, &water_ps_);
    D3D11_BUFFER_DESC cb{sizeof(WaterCb), D3D11_USAGE_DYNAMIC, D3D11_BIND_CONSTANT_BUFFER, D3D11_CPU_ACCESS_WRITE};
    dev_->CreateBuffer(&cb, nullptr, &water_cb_);
    D3D11_BLEND_DESC bd{};
    bd.RenderTarget[0].BlendEnable = TRUE;
    bd.RenderTarget[0].SrcBlend = D3D11_BLEND_SRC_ALPHA;
    bd.RenderTarget[0].DestBlend = D3D11_BLEND_INV_SRC_ALPHA;
    bd.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
    bd.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ZERO;
    bd.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_ONE;
    bd.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
    bd.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL;
    dev_->CreateBlendState(&bd, &water_blend_);
    D3D11_DEPTH_STENCIL_DESC dd{};
    dd.DepthEnable = TRUE;
    dd.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ZERO;
    dd.DepthFunc = D3D11_COMPARISON_LESS_EQUAL;
    dev_->CreateDepthStencilState(&dd, &water_ds_);
}

void Renderer::pass_water(const RenderScene& scene, const EnvironmentParams& env, const Camera& cam, float time_s) {
    bool any = false;
    for (const Entity& e : scene.entities())
        if (e.get<Water>() && scene.drawable(e)) { any = true; break; }
    if (!any) {
        water_gpu_.clear();
        return;
    }
    WCE_PROF("render.water");
    try {
        create_water_resources();
    } catch (const std::exception& ex) {
        error(ex.what());
        return;
    }
    const Texture* river_n = textures_.get("battleterrain/river/river_normal.dds", false);
    const Texture* sea_n = textures_.get("sea/water_detail_normal.dds", false);
    // GPU copies of the meshes: rebuilt when an entity's version moves on, dropped with the entity
    std::map<EntityId, WaterGpu> keep;
    for (const Entity& e : scene.entities()) {
        const Water* w = e.get<Water>();
        if (!w || w->verts.empty() || w->indices.empty()) continue;
        WaterGpu g;
        auto it = water_gpu_.find(e.id);
        if (it != water_gpu_.end() && it->second.version == w->version && it->second.vb) g = it->second;
        else {
            D3D11_BUFFER_DESC vb{UINT(w->verts.size() * sizeof(WaterVertex)), D3D11_USAGE_IMMUTABLE, D3D11_BIND_VERTEX_BUFFER};
            D3D11_SUBRESOURCE_DATA vsd{w->verts.data()};
            D3D11_BUFFER_DESC ib{UINT(w->indices.size() * 4), D3D11_USAGE_IMMUTABLE, D3D11_BIND_INDEX_BUFFER};
            D3D11_SUBRESOURCE_DATA isd{w->indices.data()};
            if (FAILED(dev_->CreateBuffer(&vb, &vsd, &g.vb)) || FAILED(dev_->CreateBuffer(&ib, &isd, &g.ib))) continue;
            g.index_count = UINT(w->indices.size());
            g.version = w->version;
        }
        keep[e.id] = g;
    }
    water_gpu_ = std::move(keep);

    const float3 to_sun = env.to_sun();
    const float3 sun_rad = colour_to_linear(env.sun_colour) * (env.sun_colour_scale * kSunHdrScale * 0.004f);   // the shaders multiply by 0.004 in HDR
    ctx_->OMSetRenderTargets(1, hdr_.rtv.GetAddressOf(), dsv_.Get());
    ctx_->OMSetDepthStencilState(water_ds_.Get(), 0);
    ctx_->OMSetBlendState(water_blend_.Get(), nullptr, 0xffffffff);
    ctx_->RSSetState(rs_none_.Get());
    ctx_->IASetInputLayout(water_il_.Get());
    ctx_->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    ctx_->VSSetShader(water_vs_.Get(), nullptr, 0);
    ctx_->PSSetShader(water_ps_.Get(), nullptr, 0);
    ID3D11SamplerState* samplers[2] = {s_wrap_aniso_.Get(), s_clamp_linear_.Get()};
    ctx_->PSSetSamplers(0, 2, samplers);
    ID3D11ShaderResourceView* common[2] = {sky_cube_srv_.Get(), gb_[3].srv.Get()};
    ctx_->PSSetShaderResources(1, 2, common);
    const float3 deep = colour_to_linear(env.deep_water), shallow = colour_to_linear(env.shallow_water);
    for (const Entity& e : scene.entities()) {
        const Water* w = e.get<Water>();
        auto it = w ? water_gpu_.find(e.id) : water_gpu_.end();
        if (it == water_gpu_.end() || !scene.drawable(e)) continue;
        WaterCb cb{};
        std::memcpy(cb.vp, view_proj_.data(), sizeof cb.vp);
        cb.eye[0] = cam.eye.x; cb.eye[1] = cam.eye.y; cb.eye[2] = cam.eye.z;
        cb.time = time_s;
        cb.sun_dir[0] = to_sun.x; cb.sun_dir[1] = to_sun.y; cb.sun_dir[2] = to_sun.z;
        cb.near_p = cam.znear;
        cb.sun_rad[0] = sun_rad.x; cb.sun_rad[1] = sun_rad.y; cb.sun_rad[2] = sun_rad.z;
        cb.far_p = cam.zfar;
        cb.deep[0] = deep.x; cb.deep[1] = deep.y; cb.deep[2] = deep.z;
        cb.is_sea = w->river ? 0.0f : 1.0f;
        cb.shallow[0] = shallow.x; cb.shallow[1] = shallow.y; cb.shallow[2] = shallow.z;
        cb.foam = w->foam;
        D3D11_MAPPED_SUBRESOURCE ms;
        if (FAILED(ctx_->Map(water_cb_.Get(), 0, D3D11_MAP_WRITE_DISCARD, 0, &ms))) continue;
        std::memcpy(ms.pData, &cb, sizeof cb);
        ctx_->Unmap(water_cb_.Get(), 0);
        ctx_->VSSetConstantBuffers(0, 1, water_cb_.GetAddressOf());
        ctx_->PSSetConstantBuffers(0, 1, water_cb_.GetAddressOf());
        const Texture* nt = w->river ? river_n : sea_n;
        ID3D11ShaderResourceView* nsrv = nt ? nt->srv.Get() : black_srv_.Get();
        ctx_->PSSetShaderResources(0, 1, &nsrv);
        const UINT stride = sizeof(WaterVertex), off = 0;
        ID3D11Buffer* vbp = it->second.vb.Get();
        ctx_->IASetVertexBuffers(0, 1, &vbp, &stride, &off);
        ctx_->IASetIndexBuffer(it->second.ib.Get(), DXGI_FORMAT_R32_UINT, 0);
        ctx_->DrawIndexed(it->second.index_count, 0, 0);
        ++draw_stats_.draw_calls;
    }
    ctx_->OMSetBlendState(bs_opaque_.Get(), nullptr, 0xffffffff);
    ID3D11ShaderResourceView* nulls[3] = {};
    ctx_->PSSetShaderResources(0, 3, nulls);
}

} // namespace wce
