// The water passes (Phase 4 of docs/plans/tile_ted_parity.md, docs/re/water.md): river ribbons and sea / lake planes of the scene's Water entities, drawn with
// the game's own shaders - rigidriver (vs30_main_battle_river, ps30_main_battle_river), waterplane (vs30_water_battle, ps30_battle_ultra_water_quality) and
// waterrefraction (ps30_main_3_channel) from the packs' fxc/ folder, bound by their reflected names like every other pass.
//
// Frame order (the game's, as far as it can be read from the shaders): the lit scene is copied half-size by waterrefraction into s_terrain_refraction, colour in rgb and
// the refraction weight (from the ground's world height) in alpha; the sea plane is drawn over the lit scene and then covered by the fog pass (its shader has no fog); the
// river is drawn after the fog (its shader fogs itself). Both pixel shaders write four targets: the G-buffer channels 1 .. 3 (normal, albedo, specular) and the final
// lit colour into the HDR target, opaque, with the scene's depth test.
#include "foundation/profile.h"
#include "render/renderer.h"
#include "render/shader_lib.h"
#include "scene/environment.h"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstring>

using Microsoft::WRL::ComPtr;

namespace wce {

namespace {

const char* kRiverVS = "dx11_sm_3_0_rigidriver.hlsl_vs_vs30_main_battle_river";
const char* kRiverPS = "dx11_sm_3_0_rigidriver.hlsl_ps_ps30_main_battle_river";
const char* kSeaVS = "dx11_sm_3_0_waterplane.hlsl_vs_vs30_water_battle";
const char* kSeaPS = "dx11_sm_3_0_waterplane.hlsl_ps_ps30_battle_ultra_water_quality";   // the "ultra" quality level
const char* kRefractionPS = "dx11_sm_3_0_waterrefraction.hlsl_ps_ps30_main_3_channel";

// water_refraction_PS.g_refraction_depth (and directional_light_PS's, which writes the same weight into the light buffer's alpha): the weight of the refraction is
// saturate(-ground_y * this) below the water level. 0.75 is the value the engine's constructors of both cbuffers store (fn 0x116d45c0 and 0x117037e0: 0x3f400000); a runtime
// override has not been looked for.
constexpr float kRefractionDepth = 0.75f;

struct Row { float v[4]; };

} // namespace

void Renderer::water_refraction() {
    const int tw = std::max(1, w_ / 2), th = std::max(1, h_ / 2);
    if (!refr_.tex || refr_w_ != tw || refr_h_ != th) {
        refr_ = make_target(tw, th, DXGI_FORMAT_R16G16B16A16_FLOAT);
        refr_w_ = tw;
        refr_h_ = th;
    }
    const GameShader* ps = shader(kRefractionPS);
    if (!ps) return;
    cbs_.set_f("water_refraction_PS", "g_refraction_depth", {kRefractionDepth});
    ctx_->PSSetShaderResources(0, 16, std::array<ID3D11ShaderResourceView*, 16>{}.data());
    ctx_->OMSetRenderTargets(1, refr_.rtv.GetAddressOf(), nullptr);
    D3D11_VIEWPORT vp{0, 0, float(tw), float(th), 0, 1};
    ctx_->RSSetViewports(1, &vp);
    ctx_->OMSetBlendState(bs_opaque_.Get(), nullptr, 0xffffffff);
    ctx_->OMSetDepthStencilState(ds_off_.Get(), 0);
    ctx_->RSSetState(rs_none_.Get());
    for (auto& [n, slot] : ps->samplers) ctx_->PSSetSamplers(slot, 1, s_clamp_linear_.GetAddressOf());
    bind_sampler(ctx_, *ps, "gbuffer_channel_4_sampler", s_clamp_point_.Get());
    bind_srv(ctx_, *ps, "gbuffer_channel_4_sampler", gb_[3].srv.Get());
    bind_srv(ctx_, *ps, "s_light_accumulation_buffer", hdr_.srv.Get());
    cbs_.upload(dev_, ctx_);
    fullscreen(*ps);
    ctx_->PSSetShaderResources(0, 16, std::array<ID3D11ShaderResourceView*, 16>{}.data());
    D3D11_VIEWPORT main_vp{0, 0, float(w_), float(h_), 0, 1};
    ctx_->RSSetViewports(1, &main_vp);
}

void Renderer::pass_water(const RenderScene& scene, const EnvironmentParams& env, const Camera& cam, float time_s, WaterPhase phase) {
    (void)env; (void)cam; (void)time_s;
    const bool rivers = phase == WaterPhase::River;
    // GPU copies of the meshes: rebuilt when an entity's version moves on, dropped with the entity
    std::map<EntityId, WaterGpu> keep;
    bool any = false;
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
        if (w->river == rivers && scene.drawable(e)) any = true;
    }
    water_gpu_ = std::move(keep);
    if (!any) return;
    WCE_PROF("render.water");

    const GameShader* vs = shader(rivers ? kRiverVS : kSeaVS);
    const GameShader* ps = shader(rivers ? kRiverPS : kSeaPS);
    if (!vs || !ps) return;

    if (!water_ds_) {
        D3D11_DEPTH_STENCIL_DESC dd{};
        dd.DepthEnable = TRUE;
        dd.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ALL;
        dd.DepthFunc = D3D11_COMPARISON_LESS_EQUAL;
        dd.StencilEnable = TRUE;   // geometry is stencil 1: the fog pass covers the sea plane
        dd.StencilReadMask = dd.StencilWriteMask = 0xff;
        dd.FrontFace = {D3D11_STENCIL_OP_KEEP, D3D11_STENCIL_OP_KEEP, D3D11_STENCIL_OP_REPLACE, D3D11_COMPARISON_ALWAYS};
        dd.BackFace = dd.FrontFace;
        dev_->CreateDepthStencilState(&dd, &water_ds_);
    }
    if (!river_inst_vb_) {   // the instance data of the vertex shaders (one instance each): the rows of the 3x4 world matrix, identity (the vertices are in world space)
        const Row river[4] = {{{1, 0, 0, 0}}, {{0, 1, 0, 0}}, {{0, 0, 1, 0}}, {{0, 0, 0, 0}}};   // P2 .. P4 rows, P5 (x >= 1 flips the uv)
        const Row sea[6] = {{{1, 0, 0, 0}}, {{0, 1, 0, 0}}, {{0, 0, 1, 0}}, {{0, 0, 0, 0}}, {{0, 0, 0, 0}}, {{0, 0, 0, 0}}};   // P1 .. P3 rows, P4 .. P6 unused
        D3D11_BUFFER_DESC bd{sizeof river, D3D11_USAGE_IMMUTABLE, D3D11_BIND_VERTEX_BUFFER};
        D3D11_SUBRESOURCE_DATA sd{river};
        dev_->CreateBuffer(&bd, &sd, &river_inst_vb_);
        bd.ByteWidth = sizeof sea;
        sd.pSysMem = sea;
        dev_->CreateBuffer(&bd, &sd, &sea_inst_vb_);
    }

    water_refraction();

    const Texture* nt = textures_.get(rivers ? "battleterrain/river/river_normal.dds" : "sea/water_detail_normal.dds", false);
    ID3D11InputLayout* il = nullptr;
    try {
        const DXGI_FORMAT f4 = DXGI_FORMAT_R32G32B32A32_FLOAT;
        if (rivers) {
            std::vector<D3D11_INPUT_ELEMENT_DESC> el = {{"POSITION", 0, f4, 0, 0, D3D11_INPUT_PER_VERTEX_DATA, 0}, {"POSITION", 1, f4, 0, 16, D3D11_INPUT_PER_VERTEX_DATA, 0}};
            for (UINT i = 0; i < 4; ++i) el.push_back({"POSITION", 2 + i, f4, 1, i * 16, D3D11_INPUT_PER_INSTANCE_DATA, 1});
            il = lib_->layout(*vs, "river", el);
        } else {
            std::vector<D3D11_INPUT_ELEMENT_DESC> el = {{"POSITION", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 0, D3D11_INPUT_PER_VERTEX_DATA, 0},
                                                        {"TEXCOORD", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 16, D3D11_INPUT_PER_VERTEX_DATA, 0}};
            for (UINT i = 0; i < 6; ++i) el.push_back({"POSITION", 1 + i, f4, 1, i * 16, D3D11_INPUT_PER_INSTANCE_DATA, 1});
            il = lib_->layout(*vs, "sea", el);
        }
    } catch (const std::exception& ex) {
        error(ex.what());
        return;
    }

    // constants the river's vertex shader reads for the terrain height: the game puts the ribbon on the low-res height map; here the mesh carries its own world height
    // (rebuilt from the tile's terrain), so the map's contribution is switched off (scale 0, a black map) and the dimensions are kept finite.
    cbs_.set_f("vs_height_map_cb", "g_height_map_lf_scale", {0.0f});
    cbs_.set_f("vs_height_map_cb", "g_height_map_hf_scale", {1.0f});
    cbs_.set_f("vs_height_map_cb", "g_height_map_vertical_offset", {0.0f});
    cbs_.set_f("vs_height_map_cb", "g_height_map_dimension", {256.0f, 256.0f, 1.0f / 256.0f, 1.0f / 256.0f});
    cbs_.set_f("vs_height_map_cb", "g_world_dimensions", {1024.0f, 1024.0f});
    cbs_.set_f("vs_height_map_cb", "g_lf_uv_offset", {0.0f});
    cbs_.set_f("river_CB", "g_world_size", {1.0f, 1.0f});
    cbs_.set_f("river_CB", "g_flow_rate", {0.0f});
    cbs_.set_f("river_CB", "g_vertical_offset", {0.0f});
    cbs_.upload(dev_, ctx_);

    ctx_->PSSetShaderResources(0, 16, std::array<ID3D11ShaderResourceView*, 16>{}.data());
    ID3D11RenderTargetView* rtvs[4] = {gb_[0].rtv.Get(), gb_[1].rtv.Get(), gb_[2].rtv.Get(), hdr_.rtv.Get()};
    ctx_->OMSetRenderTargets(4, rtvs, dsv_.Get());
    ctx_->OMSetDepthStencilState(water_ds_.Get(), 1);
    ctx_->OMSetBlendState(bs_opaque_.Get(), nullptr, 0xffffffff);
    ctx_->RSSetState(rs_none_.Get());
    ctx_->IASetInputLayout(il);
    ctx_->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    bind_shader(ctx_, *vs, cbs_);
    bind_shader(ctx_, *ps, cbs_);
    bind_srv(ctx_, *vs, "t_low_res_height_map", black_srv_.Get());
    for (auto& [n, slot] : ps->samplers) ctx_->PSSetSamplers(slot, 1, s_clamp_linear_.GetAddressOf());
    bind_sampler(ctx_, *ps, "s_normal", s_wrap_linear_.Get());
    bind_sampler(ctx_, *ps, "s_environment", s_wrap_linear_.Get());
    bind_sampler(ctx_, *ps, "s_terrain_refraction", s_clamp_point_.Get());
    bind_sampler(ctx_, *ps, "gbuffer_channel_4_sampler", s_clamp_point_.Get());
    bind_sampler(ctx_, *ps, "s_sky", s_clamp_linear_.Get());
    bind_srv(ctx_, *ps, "s_normal", nt ? nt->srv.Get() : black_srv_.Get());
    bind_srv(ctx_, *ps, "s_environment", env_valid_ ? env_cube_srv_.Get() : sky_cube_srv_.Get());
    bind_srv(ctx_, *ps, "s_sky", sky_cube_srv_.Get());
    bind_srv(ctx_, *ps, "s_terrain_refraction", refr_.srv.Get());
    bind_srv(ctx_, *ps, "gbuffer_channel_4_sampler", gb_[3].srv.Get());
    for (const Entity& e : scene.entities()) {
        const Water* w = e.get<Water>();
        auto it = w ? water_gpu_.find(e.id) : water_gpu_.end();
        if (it == water_gpu_.end() || w->river != rivers || !scene.drawable(e)) continue;
        ID3D11Buffer* vbs[2] = {it->second.vb.Get(), rivers ? river_inst_vb_.Get() : sea_inst_vb_.Get()};
        const UINT strides[2] = {sizeof(WaterVertex), rivers ? UINT(4 * sizeof(Row)) : UINT(6 * sizeof(Row))}, offs[2] = {0, 0};
        ctx_->IASetVertexBuffers(0, 2, vbs, strides, offs);
        ctx_->IASetIndexBuffer(it->second.ib.Get(), DXGI_FORMAT_R32_UINT, 0);
        ctx_->DrawIndexedInstanced(it->second.index_count, 1, 0, 0, 0);
        ++draw_stats_.draw_calls;
    }
    ctx_->PSSetShaderResources(0, 16, std::array<ID3D11ShaderResourceView*, 16>{}.data());
    ctx_->OMSetRenderTargets(1, hdr_.rtv.GetAddressOf(), dsv_.Get());
}

} // namespace wce
