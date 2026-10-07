#include "particles.h"

#include "core/vfs.h"
#include "render/shader_lib.h"
#include "render/texture.h"

#include <algorithm>
#include <cmath>
#include <cstring>
#include <map>

namespace we {

namespace {

const char* kSimVS = "dx11_sm_4_0_particle_vfx_simulation.hlsl_vs_vs40_main";
const char* kSimPS = "dx11_sm_4_0_particle_vfx_simulation.hlsl_ps_ps_main_simulation";
const char* kKeysPS = "dx11_sm_4_0_particle_vfx_simulation.hlsl_ps_ps_main";
const char* kSortVS = "dx11_sm_4_0_particle_vfx_gpu_sort.hlsl_vs_vs_main";
const char* kSortPS = "dx11_sm_4_0_particle_vfx_gpu_sort.hlsl_ps_ps_main";
const char* kSpawnVS = "dx11_sm_4_0_particle_vfx_spawn.hlsl_vs_vs40_main";
const char* kSpawnPS = "dx11_sm_4_0_particle_vfx_spawn.hlsl_ps_ps_main";
const char* kMainVS = "dx11_sm_4_0_particle_vfx_main.hlsl_vs_vs40_main";
const char* kMainGS = "dx11_sm_4_0_particle_vfx_main.hlsl_gs_gs_main";
const char* kMainPS = "dx11_sm_4_0_particle_vfx_main.hlsl_ps_ps_main";

constexpr float kPi = 3.14159265f;
constexpr int kMaxEmitters = 512;
constexpr float kSubStep = 0.2f; // spawn shader pre-simulation step

float lerp(float a, float b, float t) { return a + (b - a) * t; }

// One float4 per emitter in a [512] array at `offset` bytes into a cbuffer image.
void put(std::vector<uint8_t>& cb, size_t offset, int index, const void* v) { std::memcpy(cb.data() + offset + 16 * size_t(index), v, 16); }

Microsoft::WRL::ComPtr<ID3D11Buffer> make_cb(ID3D11Device* dev, const std::vector<uint8_t>& data) {
    D3D11_BUFFER_DESC bd{UINT(data.size()), D3D11_USAGE_DEFAULT, D3D11_BIND_CONSTANT_BUFFER};
    D3D11_SUBRESOURCE_DATA sd{data.data()};
    Microsoft::WRL::ComPtr<ID3D11Buffer> b;
    dev->CreateBuffer(&bd, &sd, &b);
    return b;
}

} // namespace

ParticleSystem::ParticleSystem(ID3D11Device* dev, ID3D11DeviceContext* ctx, Vfs& vfs, ShaderLibrary& lib, ConstantBuffers& cbs)
    : dev_(dev), ctx_(ctx), vfs_(vfs), lib_(lib), cbs_(cbs) {
    create_resources();
    build_emitter_gpu();
}

ParticleSystem::~ParticleSystem() = default;

void ParticleSystem::error(const std::string& e) {
    if (std::find(errors_.begin(), errors_.end(), e) == errors_.end()) errors_.push_back(e);
}

float ParticleSystem::rand01() {
    rng_ = rng_ * 1664525u + 1013904223u;
    return float(rng_ >> 8) * (1.0f / 16777216.0f);
}

float3 ParticleSystem::wind_velocity() const {
    float speed = 22.5f * 0.5f * (effect_.definition.wind_size_min + effect_.definition.wind_size_max);
    float a = wind_azimuth_deg * kPi / 180.0f;
    return float3{std::sin(a), 0, std::cos(a)} * speed;
}

void ParticleSystem::create_resources() {
    auto make_target = [&](DXGI_FORMAT fmt, int slices) {
        Target t;
        D3D11_TEXTURE2D_DESC td{UINT(kWidth), UINT(kWidth), 1, UINT(slices), fmt, {1, 0}, D3D11_USAGE_DEFAULT,
                                D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE};
        dev_->CreateTexture2D(&td, nullptr, &t.tex);
        D3D11_SHADER_RESOURCE_VIEW_DESC sd{};
        sd.Format = fmt;
        if (slices > 1) {
            sd.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2DARRAY;
            sd.Texture2DArray.MipLevels = 1;
            sd.Texture2DArray.ArraySize = UINT(slices);
        } else {
            sd.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D;
            sd.Texture2D.MipLevels = 1;
        }
        dev_->CreateShaderResourceView(t.tex.Get(), &sd, &t.srv);
        for (int s = 0; s < slices; ++s) {
            D3D11_RENDER_TARGET_VIEW_DESC rd{};
            rd.Format = fmt;
            if (slices > 1) {
                rd.ViewDimension = D3D11_RTV_DIMENSION_TEXTURE2DARRAY;
                rd.Texture2DArray.FirstArraySlice = UINT(s);
                rd.Texture2DArray.ArraySize = 1;
            } else {
                rd.ViewDimension = D3D11_RTV_DIMENSION_TEXTURE2D;
            }
            dev_->CreateRenderTargetView(t.tex.Get(), &rd, &t.rtv[s]);
        }
        return t;
    };
    for (auto& p : particles_) p = make_target(DXGI_FORMAT_R32G32B32A32_FLOAT, 3);
    for (auto& k : keys_) k = make_target(DXGI_FORMAT_R32G32_FLOAT, 1);

    D3D11_TEXTURE2D_DESC ed{UINT(kEmitterWidth), 1, 1, UINT(kEmitterLayers), DXGI_FORMAT_R32G32B32A32_FLOAT, {1, 0}, D3D11_USAGE_DEFAULT,
                            D3D11_BIND_SHADER_RESOURCE};
    dev_->CreateTexture2D(&ed, nullptr, &emitter_tex_);
    dev_->CreateShaderResourceView(emitter_tex_.Get(), nullptr, &emitter_srv_);

    {   // flat normal (1 slice) and a still sea surface (2 slices, game logic collision is off for weather)
        uint32_t flat = 0xffff8080u;
        D3D11_TEXTURE2D_DESC td{1, 1, 1, 1, DXGI_FORMAT_R8G8B8A8_UNORM, {1, 0}, D3D11_USAGE_IMMUTABLE, D3D11_BIND_SHADER_RESOURCE};
        D3D11_SUBRESOURCE_DATA sd{&flat, 4, 4};
        Microsoft::WRL::ComPtr<ID3D11Texture2D> t;
        dev_->CreateTexture2D(&td, &sd, &t);
        D3D11_SHADER_RESOURCE_VIEW_DESC vd{};
        vd.Format = td.Format;
        vd.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2DARRAY;
        vd.Texture2DArray.MipLevels = 1;
        vd.Texture2DArray.ArraySize = 1;
        dev_->CreateShaderResourceView(t.Get(), &vd, &normal_srv_);
        float zero[2][4] = {};
        D3D11_TEXTURE2D_DESC sdsc{1, 1, 1, 2, DXGI_FORMAT_R32G32B32A32_FLOAT, {1, 0}, D3D11_USAGE_IMMUTABLE, D3D11_BIND_SHADER_RESOURCE};
        D3D11_SUBRESOURCE_DATA ss[2] = {{zero[0], 16, 16}, {zero[1], 16, 16}};
        Microsoft::WRL::ComPtr<ID3D11Texture2D> st;
        dev_->CreateTexture2D(&sdsc, ss, &st);
        dev_->CreateShaderResourceView(st.Get(), nullptr, &sea_srv_);
    }
    try { // 3D vector field the turbulence samples (g_tex_vector_field, string next to it in the DLL)
        vector_field_srv_ = load_dds(dev_, vfs_.read("vfx/textures/turbulence.dds"), false).srv;
    } catch (const std::exception& e) {
        error(std::string("vfx/textures/turbulence.dds: ") + e.what());
        uint32_t mid = 0xff808080u;
        D3D11_TEXTURE3D_DESC vd{1, 1, 1, 1, DXGI_FORMAT_R8G8B8A8_UNORM, D3D11_USAGE_IMMUTABLE, D3D11_BIND_SHADER_RESOURCE, 0, 0};
        D3D11_SUBRESOURCE_DATA sd{&mid, 4, 4};
        Microsoft::WRL::ComPtr<ID3D11Texture3D> t;
        dev_->CreateTexture3D(&vd, &sd, &t);
        dev_->CreateShaderResourceView(t.Get(), nullptr, &vector_field_srv_);
    }

    auto sampler = [&](D3D11_FILTER f, D3D11_TEXTURE_ADDRESS_MODE a, Microsoft::WRL::ComPtr<ID3D11SamplerState>& out) {
        D3D11_SAMPLER_DESC sd{};
        sd.Filter = f;
        sd.AddressU = sd.AddressV = sd.AddressW = a;
        sd.MaxLOD = D3D11_FLOAT32_MAX;
        dev_->CreateSamplerState(&sd, &out);
    };
    sampler(D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_TEXTURE_ADDRESS_CLAMP, s_linear_clamp_);
    sampler(D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_TEXTURE_ADDRESS_WRAP, s_linear_wrap_);
    sampler(D3D11_FILTER_MIN_MAG_MIP_POINT, D3D11_TEXTURE_ADDRESS_CLAMP, s_point_clamp_);

    D3D11_BLEND_DESC bd{};
    bd.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL;
    dev_->CreateBlendState(&bd, &bs_opaque_);
    // The pixel shader outputs premultiplied colour (its fog lerps towards fog colour * alpha).
    bd.RenderTarget[0].BlendEnable = TRUE;
    bd.RenderTarget[0].SrcBlend = D3D11_BLEND_ONE;
    bd.RenderTarget[0].DestBlend = D3D11_BLEND_INV_SRC_ALPHA;
    bd.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
    bd.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;
    bd.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_INV_SRC_ALPHA;
    bd.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
    dev_->CreateBlendState(&bd, &bs_premultiplied_);
    D3D11_DEPTH_STENCIL_DESC dd{};
    dd.DepthEnable = TRUE;
    dd.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ZERO;
    dd.DepthFunc = D3D11_COMPARISON_LESS_EQUAL;
    dev_->CreateDepthStencilState(&dd, &ds_test_);
    dd.DepthEnable = FALSE;
    dev_->CreateDepthStencilState(&dd, &ds_off_);
    D3D11_RASTERIZER_DESC rd{};
    rd.FillMode = D3D11_FILL_SOLID;
    rd.CullMode = D3D11_CULL_NONE;
    rd.DepthClipEnable = TRUE;
    dev_->CreateRasterizerState(&rd, &rs_none_);
}

void ParticleSystem::set_weather(const std::string& definition_name) {
    if (loaded_ && definition_name == definition_) return;
    loaded_ = true;
    definition_ = definition_name;
    try {
        effect_ = load_weather_vfx(vfs_, definition_name);
    } catch (const std::exception& e) {
        effect_ = {};
        error("weather vfx '" + definition_name + "': " + e.what());
    }
    if (effect_.emitters.size() > size_t(std::min(kMaxEmitters, kEmitterWidth))) effect_.emitters.resize(std::min(kMaxEmitters, kEmitterWidth));
    build_emitter_gpu();
    restart();
}

void ParticleSystem::restart() {
    state_.assign(effect_.emitters.size(), EmitterState{});
    clock_ = 0;
    alive_ = 0;
    clear_ = true;
}

// Emitter constant buffers, the curve texture array and the diffuse texture array of the current effect.
void ParticleSystem::build_emitter_gpu() {
    std::vector<uint8_t> common(8192), gs(16384), vs(57344), ps_render(57344), ps_sim(40960);
    std::map<std::string, uint32_t> slices;
    std::vector<Texture> diffuse;
    const size_t n = effect_.emitters.size();
    for (size_t i = 0; i < n; ++i) {
        EmitterConstants& k = effect_.emitters[i].k;
        if (!k.diffuse.empty() && !slices.count(k.diffuse)) {
            try {
                Texture t = load_dds(dev_, vfs_.read(k.diffuse), true);
                if (!diffuse.empty() && (t.width != diffuse[0].width || t.height != diffuse[0].height || t.format != diffuse[0].format))
                    error(k.diffuse + ": size/format differs from " + effect_.emitters[0].k.diffuse + ", drawn with slice 0");
                else {
                    slices[k.diffuse] = uint32_t(diffuse.size());
                    diffuse.push_back(t);
                }
            } catch (const std::exception& e) {
                error(k.diffuse + ": " + e.what());
            }
        }
        k.behaviour_mask[1] = slices.count(k.diffuse) ? slices[k.diffuse] : 0;
        if (slices.empty()) k.behaviour_mask[0] &= ~vfx_bits::kPsTexture;
        k.behaviour_mask[2] = 0;
        k.behaviour_mask[0] &= ~vfx_bits::kPsNormal; // ASSUMPTION: normal maps not loaded yet (weather chains have none)
        const int e = int(i);
        put(common, 0, e, k.behaviour_mask);
        put(gs, 0, e, k.vs_translation_params);
        put(gs, 8192, e, k.vs_translation_params1);
        put(ps_sim, 0, e, k.ps_sim_game_logic_params);
        put(ps_sim, 8192, e, k.ps_sim_gravity_params);
        put(ps_sim, 16384, e, k.ps_sim_deceleration_params);
        put(ps_sim, 24576, e, k.ps_sim_following_emitter_params);
        put(ps_sim, 32768, e, k.ps_sim_turbulence_params);
        put(ps_render, 0, e, k.ps_specular_params);
        put(ps_render, 8192, e, k.ps_specular_params1);
        put(ps_render, 16384, e, k.ps_receive_shadows_high_quality_params);
        put(ps_render, 24576, e, k.ps_lighting_legacy_params);
        put(ps_render, 32768, e, k.ps_lighting_params);
        put(ps_render, 40960, e, k.ps_alpha_crush_params);
        put(ps_render, 49152, e, k.ps_soft_edge_params);
        put(vs, 0, e, k.vs_culling_distances);
        put(vs, 8192, e, k.vs_rotation_params);
        put(vs, 16384, e, k.vs_scroll_uvs_params);
        put(vs, 24576, e, k.vs_atlas_texture_params);
        put(vs, 32768, e, k.vs_cell_texture_params);
        put(vs, 40960, e, k.vs_animated_texture_params);
        put(vs, 49152, e, k.vs_animated_texture_params1);
    }
    cb_common_ = make_cb(dev_, common);
    cb_gs_ = make_cb(dev_, gs);
    cb_vs_ = make_cb(dev_, vs);
    cb_ps_render_ = make_cb(dev_, ps_render);
    cb_ps_sim_ = make_cb(dev_, ps_sim);

    {   // curves: one 2D array slice per emitter, 8 rows
        UINT layers = UINT(std::max<size_t>(1, n));
        std::vector<float> texels(size_t(layers) * 8 * kVfxCurveWidth * 4, 1.0f);
        std::vector<D3D11_SUBRESOURCE_DATA> subs(layers);
        for (UINT l = 0; l < layers; ++l) {
            float* dst = texels.data() + size_t(l) * 8 * kVfxCurveWidth * 4;
            if (l < n) std::memcpy(dst, effect_.emitters[l].k.curves, sizeof(EmitterConstants::curves));
            subs[l] = {dst, UINT(kVfxCurveWidth * 16), UINT(kVfxCurveWidth * 16 * 8)};
        }
        D3D11_TEXTURE2D_DESC td{UINT(kVfxCurveWidth), 8, 1, layers, DXGI_FORMAT_R32G32B32A32_FLOAT, {1, 0}, D3D11_USAGE_IMMUTABLE,
                                D3D11_BIND_SHADER_RESOURCE};
        Microsoft::WRL::ComPtr<ID3D11Texture2D> t;
        dev_->CreateTexture2D(&td, subs.data(), &t);
        D3D11_SHADER_RESOURCE_VIEW_DESC sd{};
        sd.Format = td.Format;
        sd.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2DARRAY;
        sd.Texture2DArray.MipLevels = 1;
        sd.Texture2DArray.ArraySize = layers;
        curves_srv_.Reset();
        dev_->CreateShaderResourceView(t.Get(), &sd, &curves_srv_);
    }

    diffuse_srv_.Reset();
    if (!diffuse.empty()) { // all diffuse maps in one array (behaviour_mask.y = slice), as the game's pixel shader expects
        Microsoft::WRL::ComPtr<ID3D11Resource> r0;
        diffuse[0].srv->GetResource(&r0);
        Microsoft::WRL::ComPtr<ID3D11Texture2D> t0;
        r0.As(&t0);
        D3D11_TEXTURE2D_DESC td;
        t0->GetDesc(&td);
        UINT mips = td.MipLevels;
        td.ArraySize = UINT(diffuse.size());
        td.Usage = D3D11_USAGE_DEFAULT;
        td.MiscFlags = 0;
        Microsoft::WRL::ComPtr<ID3D11Texture2D> arr;
        if (SUCCEEDED(dev_->CreateTexture2D(&td, nullptr, &arr))) {
            for (UINT s = 0; s < td.ArraySize; ++s) {
                Microsoft::WRL::ComPtr<ID3D11Resource> r;
                diffuse[s].srv->GetResource(&r);
                for (UINT m = 0; m < mips; ++m) ctx_->CopySubresourceRegion(arr.Get(), D3D11CalcSubresource(m, s, mips), 0, 0, 0, r.Get(), m, nullptr);
            }
            D3D11_SHADER_RESOURCE_VIEW_DESC sd{};
            sd.Format = diffuse[0].format;
            sd.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2DARRAY;
            sd.Texture2DArray.MipLevels = mips;
            sd.Texture2DArray.ArraySize = td.ArraySize;
            dev_->CreateShaderResourceView(arr.Get(), &sd, &diffuse_srv_);
        }
    }
}

void ParticleSystem::bind_emitter_cbs(const GameShader& s, bool sim_layout) {
    for (auto& [name, slot] : s.cbuffers) {
        ID3D11Buffer* b = nullptr;
        if (name == "cb_vfx_emitter_constants_common") b = cb_common_.Get();
        else if (name == "cb_vfx_emitter_constants_gs") b = cb_gs_.Get();
        else if (name == "cb_vfx_emitter_constants_vs") b = cb_vs_.Get();
        else if (name == "cb_vfx_emitter_constants_ps") b = sim_layout ? cb_ps_sim_.Get() : cb_ps_render_.Get();
        if (!b) continue;
        switch (s.stage) {
        case Stage::VS: ctx_->VSSetConstantBuffers(slot, 1, &b); break;
        case Stage::GS: ctx_->GSSetConstantBuffers(slot, 1, &b); break;
        case Stage::PS: ctx_->PSSetConstantBuffers(slot, 1, &b); break;
        default: break;
        }
    }
}

void ParticleSystem::fullscreen(ID3D11RenderTargetView* const* rtvs, int n, int w, int h) {
    ctx_->OMSetRenderTargets(UINT(n), rtvs, nullptr);
    D3D11_VIEWPORT vp{0, 0, float(w), float(h), 0, 1};
    ctx_->RSSetViewports(1, &vp);
    ctx_->IASetInputLayout(nullptr);
    ctx_->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    ctx_->Draw(3, 0);
}

// CPU emission. ASSUMPTIONS (game code not traced yet, see docs/re/particles.md):
//  - rate = lerp(emission_min, emission_max, random) of EmissionRateCurve at the emitter's normalised age
//    (alive counts in the dumps match rate * ParticleLifeTime per reference);
//  - position: SpawnAreaBox gives the box size per axis (centred), else SpawnAreaCurve an annulus radius;
//  - velocity: SpawnSpeed along elevation SpawnSpread (degrees), azimuth in SpawnArc;
//  - StartTime < 0 pre-warms the emitter with the spawn shader's 0.2 s sub-steps.
void ParticleSystem::spawn(const Frame& f, std::vector<SpawnVertex>& out) {
    uint32_t free_slots = capacity() > alive_ ? capacity() - alive_ : 0;
    for (size_t i = 0; i < effect_.emitters.size() && i < state_.size(); ++i) {
        const ActiveEmitter& a = effect_.emitters[i];
        const VfxEmitterDef& d = a.def;
        EmitterState& st = state_[i];
        float3 origin = (a.camera_following ? f.eye : float3{}) + float3{a.pos[0], a.pos[1], a.pos[2]};
        float sp[3] = {0, 0, 0};
        d.vec("SpawnPosition", sp, 3);
        origin = origin + float3{sp[0], sp[1], sp[2]} * a.scale;

        float life_lo, life_hi, start_lo, start_hi, emitter_life_lo, emitter_life_hi;
        d.range("ParticleLifeTime", life_lo, life_hi, 1, 1);
        d.range("StartTime", start_lo, start_hi, 0, 0);
        d.range("LifeTime", emitter_life_lo, emitter_life_hi, 50, 50);
        float emitter_life = std::max(0.001f, 0.5f * (emitter_life_lo + emitter_life_hi));
        float pre = std::max(0.0f, -0.5f * (start_lo + start_hi));
        float window_start = clock_ - f.dt, window = f.dt;
        if (!st.started) {
            st.started = true;
            window_start = clock_ - pre;
            window = pre + f.dt;
        }
        float age = clock_ + pre;
        float t_norm = d.b("EmissionLoopAnim", false) ? std::fmod(age / emitter_life, 1.0f) : std::min(age / emitter_life, 1.0f);
        if (!d.b("InfiniteLife", false) && age > emitter_life) continue;
        const VfxCurve* emin = d.curve("EmissionRateCurve", "emission_min");
        const VfxCurve* emax = d.curve("EmissionRateCurve", "emission_max");
        float rate = lerp(emin ? emin->eval(t_norm) : 0.0f, emax ? emax->eval(t_norm) : 0.0f, rand01());
        st.rate_accum += rate * window;
        uint32_t count = uint32_t(st.rate_accum);
        st.rate_accum -= float(count);

        auto box_curve = [&](const char* n) { const VfxCurve* c = d.curve("SpawnAreaBox", n); return c ? c->eval(t_norm) : 0.0f; };
        float box_lo[3] = {box_curve("x_min"), box_curve("y_min"), box_curve("z_min")};
        float box_hi[3] = {box_curve("x_max"), box_curve("y_max"), box_curve("z_max")};
        bool use_box = box_hi[0] > 0 || box_hi[1] > 0 || box_hi[2] > 0;
        auto area_curve = [&](const char* n) { const VfxCurve* c = d.curve("SpawnAreaCurve", n); return c ? c->eval(t_norm) : 0.0f; };
        float r_in_lo = area_curve("inner_radius_min"), r_in_hi = area_curve("inner_radius_max");
        float r_out_lo = area_curve("outer_radius_min"), r_out_hi = area_curve("outer_radius_max");
        float speed_lo, speed_hi, spread_lo, spread_hi, lb_lo = a.particle_lifetime_bias[0], lb_hi = a.particle_lifetime_bias[1];
        d.range("SpawnSpeed", speed_lo, speed_hi, 0, 0);
        d.range("SpawnSpread", spread_lo, spread_hi, 90, 90);
        float arc[2] = {0, 360};
        d.vec("SpawnArc", arc, 2);

        uint32_t batch = 0;
        float latest_death = 0;
        for (uint32_t p = 0; p < count && free_slots > 0; ++p) {
            float spawn_t = window_start + window * (float(p) + rand01()) / float(std::max(1u, count));
            float life = std::max(0.01f, lerp(life_lo, life_hi, rand01()) + lerp(lb_lo, lb_hi, rand01()));
            if (spawn_t + life <= clock_) continue; // died during pre-warm
            float3 pos = origin;
            if (use_box) {
                pos.x += (rand01() - 0.5f) * lerp(box_lo[0], box_hi[0], rand01()) * a.scale;
                pos.y += (rand01() - 0.5f) * lerp(box_lo[1], box_hi[1], rand01()) * a.scale;
                pos.z += (rand01() - 0.5f) * lerp(box_lo[2], box_hi[2], rand01()) * a.scale;
            } else {
                float r = lerp(lerp(r_in_lo, r_in_hi, rand01()), lerp(r_out_lo, r_out_hi, rand01()), rand01()) * a.scale;
                float az = lerp(arc[0], arc[1], rand01()) * kPi / 180.0f;
                pos = pos + float3{std::cos(az), 0, std::sin(az)} * r;
            }
            float el = lerp(spread_lo, spread_hi, rand01()) * kPi / 180.0f;
            float az = lerp(arc[0], arc[1], rand01()) * kPi / 180.0f;
            float speed = lerp(speed_lo, speed_hi, rand01()) * a.scale;
            float3 vel = float3{std::cos(el) * std::cos(az), std::sin(el), std::cos(el) * std::sin(az)} * speed;
            float steps = spawn_t < clock_ - kSubStep ? std::floor((clock_ - spawn_t) / kSubStep) : 0.0f;
            out.push_back({{float(i), rand01(), steps, 0}, {pos.x, pos.y, pos.z, spawn_t}, {vel.x, vel.y, vel.z, life}});
            latest_death = std::max(latest_death, spawn_t + life);
            ++batch;
            --free_slots;
        }
        if (batch) st.deaths.push_back({latest_death, batch});
    }
}

void ParticleSystem::render(const Frame& f) {
    if (!enabled || effect_.emitters.empty()) return;
    const GameShader *sim_vs, *sim_ps, *keys_ps, *sort_vs, *sort_ps, *spawn_vs, *spawn_ps, *vs, *gs, *ps;
    try {
        sim_vs = &lib_.get(kSimVS);
        sim_ps = &lib_.get(kSimPS);
        keys_ps = &lib_.get(kKeysPS);
        sort_vs = &lib_.get(kSortVS);
        sort_ps = &lib_.get(kSortPS);
        spawn_vs = &lib_.get(kSpawnVS);
        spawn_ps = &lib_.get(kSpawnPS);
        vs = &lib_.get(kMainVS);
        gs = &lib_.get(kMainGS);
        ps = &lib_.get(kMainPS);
    } catch (const std::exception& e) {
        error(e.what());
        return;
    }
    const float dt = std::clamp(f.dt, 0.0f, 0.1f);
    clock_ += dt;

    // Alive estimate: particles whose batch has not fully expired (upper bound; dead slots are skipped by the VS).
    alive_ = 0;
    for (auto& st : state_) {
        while (!st.deaths.empty() && st.deaths.front().first <= clock_) st.deaths.pop_front();
        for (auto& b : st.deaths) alive_ += b.second;
    }
    alive_ = std::min(alive_, capacity());

    // --- emitter instances (g_emitter_instances, 6 layers) ---
    std::vector<float> layers(size_t(kEmitterLayers) * kEmitterWidth * 4, 0.0f);
    auto texel = [&](int layer, size_t e) { return &layers[(size_t(layer) * kEmitterWidth + e) * 4]; };
    for (size_t i = 0; i < effect_.emitters.size() && i < state_.size(); ++i) {
        const ActiveEmitter& a = effect_.emitters[i];
        EmitterState& st = state_[i];
        float3 pos = (a.camera_following ? f.eye : float3{}) + float3{a.pos[0], a.pos[1], a.pos[2]};
        float3 delta = st.started ? pos - st.last_pos : float3{};
        st.last_pos = pos;
        float* l0 = texel(0, i); l0[0] = pos.x; l0[1] = pos.y; l0[2] = pos.z; l0[3] = clock_;
        float* l1 = texel(1, i); l1[0] = delta.x; l1[1] = delta.y; l1[2] = delta.z; l1[3] = dt;
        float* l2 = texel(2, i); // ASSUMPTION: tint = middle of the reference's tint range
        for (int c = 0; c < 4; ++c) l2[c] = 0.5f * (a.tint_min[c] + a.tint_max[c]);
        float* l3 = texel(3, i); l3[0] = float(i); l3[1] = a.scale;
        float* l4 = texel(4, i); l4[3] = 1; // rotation quaternion
        float* l5 = texel(5, i); l5[3] = 1; // kill plane: never
    }
    for (int l = 0; l < kEmitterLayers; ++l)
        ctx_->UpdateSubresource(emitter_tex_.Get(), D3D11CalcSubresource(0, l, 1), nullptr, texel(l, 0), kEmitterWidth * 16, 0);

    // --- shared cbuffers ---
    float3 wind = f.wind_velocity;
    cbs_.set_f("cb_vfx_shared", "g_emitter_instance_tex_width", {float(kEmitterWidth)});
    cbs_.set_f("cb_vfx_shared", "g_particle_instance_tex_width", {float(kWidth)});
    cbs_.set_f("cb_vfx_shared", "g_sorted_data_tex_width", {float(kWidth)});
    cbs_.set_f("cb_vfx_shared", "g_num_alive_particles", {float(alive_)});
    cbs_.set_f("cb_vfx_game_simulation_data", "wind_velocity", {wind.x, wind.y, wind.z});
    cbs_.set_f("cb_vfx_game_simulation_data", "wind_variance_dir", {f.wind_variance.x, f.wind_variance.y, f.wind_variance.z});
    cbs_.set_f("cb_vfx", "g_camera_aligned_x_axis", {f.view.m[0][0], f.view.m[0][1], f.view.m[0][2]});
    cbs_.set_f("cb_vfx", "g_camera_aligned_y_axis", {f.view.m[1][0], f.view.m[1][1], f.view.m[1][2]});
    cbs_.set_f("cb_vfx", "g_camera_aligned_z_axis", {f.view.m[2][0], f.view.m[2][1], f.view.m[2][2]});
    cbs_.set_i("cb_vfx", "g_view_id", int(kVfxMainViewId));
    cbs_.upload(dev_, ctx_);

    ID3D11ShaderResourceView* null_srv[8] = {};
    auto unbind = [&] {
        ctx_->OMSetRenderTargets(0, nullptr, nullptr);
        ctx_->VSSetShaderResources(0, 8, null_srv);
        ctx_->PSSetShaderResources(0, 8, null_srv);
        ctx_->GSSetShaderResources(0, 8, null_srv);
    };
    unbind();
    ctx_->OMSetBlendState(bs_opaque_.Get(), nullptr, 0xffffffff);
    ctx_->OMSetDepthStencilState(ds_off_.Get(), 0);
    ctx_->RSSetState(rs_none_.Get());
    ctx_->GSSetShader(nullptr, nullptr, 0);

    if (clear_) {
        const float dead[4] = {-1, 0, 0, 0}, zero[4] = {0, 0, 0, 0};
        for (auto& p : particles_) {
            ctx_->ClearRenderTargetView(p.rtv[0].Get(), dead);
            ctx_->ClearRenderTargetView(p.rtv[1].Get(), zero);
            ctx_->ClearRenderTargetView(p.rtv[2].Get(), zero);
        }
        clear_ = false;
    }

    // 1. simulation: particles_[cur_] -> particles_[1 - cur_]
    const int src = cur_, dst = 1 - cur_;
    bind_shader(ctx_, *sim_vs, cbs_);
    bind_shader(ctx_, *sim_ps, cbs_);
    bind_emitter_cbs(*sim_ps, true);
    bind_srv(ctx_, *sim_ps, "g_emitter_instances", emitter_srv_.Get());
    bind_srv(ctx_, *sim_ps, "g_particle_instances", particles_[src].srv.Get());
    bind_srv(ctx_, *sim_ps, "g_tex_sea_surface_anim", sea_srv_.Get());
    bind_srv(ctx_, *sim_ps, "g_tex_vector_field", vector_field_srv_.Get());
    bind_sampler(ctx_, *sim_ps, "g_sam_terrain_surface", s_linear_clamp_.Get());
    bind_sampler(ctx_, *sim_ps, "g_sam_vector_field", s_linear_wrap_.Get());
    ID3D11RenderTargetView* state_rtvs[3] = {particles_[dst].rtv[0].Get(), particles_[dst].rtv[1].Get(), particles_[dst].rtv[2].Get()};
    fullscreen(state_rtvs, 3, kWidth, kWidth);
    unbind();
    cur_ = dst;

    // 2. sort keys (distance to the camera, 1e6 for dead slots) and 3. bitonic sort, ascending
    bind_shader(ctx_, *keys_ps, cbs_);
    bind_srv(ctx_, *keys_ps, "g_particle_instances", particles_[cur_].srv.Get());
    ID3D11RenderTargetView* key_rtv = keys_[0].rtv[0].Get();
    fullscreen(&key_rtv, 1, kWidth, kWidth);
    unbind();
    int k_src = 0;
    bind_shader(ctx_, *sort_vs, cbs_);
    const uint32_t n = uint32_t(kWidth) * kWidth;
    for (uint32_t size = 2; size <= n; size <<= 1)
        for (uint32_t j = size >> 1; j > 0; j >>= 1) {
            cbs_.set_f("cb_vfx_bitonicsort", "g_compop_direction_swap", {float(size >> 1)});
            cbs_.set_f("cb_vfx_bitonicsort", "g_compop_dist", {float(j)});
            cbs_.set_f("cb_vfx_bitonicsort", "g_compop_dist_coord2", {float(j % kWidth), float(j / kWidth)});
            cbs_.set_f("cb_vfx_bitonicsort", "g_tex_width", {float(kWidth)});
            cbs_.set_f("cb_vfx_bitonicsort", "g_ascending_order", {1.0f});
            cbs_.upload(dev_, ctx_);
            bind_shader(ctx_, *sort_ps, cbs_);
            bind_srv(ctx_, *sort_ps, "g_tex_packeddata", keys_[k_src].srv.Get());
            ID3D11RenderTargetView* out = keys_[1 - k_src].rtv[0].Get();
            fullscreen(&out, 1, kWidth, kWidth);
            unbind();
            k_src = 1 - k_src;
        }
    sorted_ = k_src;

    // 4. spawn into free slots (the tail of the sorted list)
    std::vector<SpawnVertex> verts;
    spawn(f, verts);
    if (!verts.empty()) {
        if (verts.size() > spawn_capacity_) {
            spawn_capacity_ = UINT(verts.size() + 4096);
            D3D11_BUFFER_DESC bd{UINT(spawn_capacity_ * sizeof(SpawnVertex)), D3D11_USAGE_DYNAMIC, D3D11_BIND_VERTEX_BUFFER, D3D11_CPU_ACCESS_WRITE};
            spawn_vb_.Reset();
            dev_->CreateBuffer(&bd, nullptr, &spawn_vb_);
        }
        D3D11_MAPPED_SUBRESOURCE ms;
        if (spawn_vb_ && SUCCEEDED(ctx_->Map(spawn_vb_.Get(), 0, D3D11_MAP_WRITE_DISCARD, 0, &ms))) {
            std::memcpy(ms.pData, verts.data(), verts.size() * sizeof(SpawnVertex));
            ctx_->Unmap(spawn_vb_.Get(), 0);
            const D3D11_INPUT_ELEMENT_DESC il[] = {
                {"COLOR", 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 0, D3D11_INPUT_PER_VERTEX_DATA, 0},
                {"COLOR", 1, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 16, D3D11_INPUT_PER_VERTEX_DATA, 0},
                {"COLOR", 2, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 32, D3D11_INPUT_PER_VERTEX_DATA, 0}};
            ctx_->IASetInputLayout(lib_.layout(*spawn_vs, "vfx_spawn", {il[0], il[1], il[2]}));
            UINT stride = sizeof(SpawnVertex), offset = 0;
            ctx_->IASetVertexBuffers(0, 1, spawn_vb_.GetAddressOf(), &stride, &offset);
            ctx_->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_POINTLIST);
            bind_shader(ctx_, *spawn_vs, cbs_);
            bind_srv(ctx_, *spawn_vs, "g_tex_sorted_data", keys_[sorted_].srv.Get());
            bind_shader(ctx_, *spawn_ps, cbs_);
            bind_emitter_cbs(*spawn_ps, true);
            bind_srv(ctx_, *spawn_ps, "g_emitter_instances", emitter_srv_.Get());
            bind_srv(ctx_, *spawn_ps, "g_tex_sea_surface_anim", sea_srv_.Get());
            bind_srv(ctx_, *spawn_ps, "g_tex_vector_field", vector_field_srv_.Get());
            bind_sampler(ctx_, *spawn_ps, "g_sam_terrain_surface", s_linear_clamp_.Get());
            bind_sampler(ctx_, *spawn_ps, "g_sam_vector_field", s_linear_wrap_.Get());
            ID3D11RenderTargetView* rtvs[3] = {particles_[cur_].rtv[0].Get(), particles_[cur_].rtv[1].Get(), particles_[cur_].rtv[2].Get()};
            ctx_->OMSetRenderTargets(3, rtvs, nullptr);
            D3D11_VIEWPORT vp{0, 0, float(kWidth), float(kWidth), 0, 1};
            ctx_->RSSetViewports(1, &vp);
            ctx_->Draw(UINT(verts.size()), 0);
            unbind();
            ID3D11Buffer* no_vb = nullptr;
            UINT zero = 0;
            ctx_->IASetVertexBuffers(0, 1, &no_vb, &zero, &zero);
        }
    }

    // 5. draw (sorted far to near: the VS reads entry num_alive - vertex_id)
    if (alive_ > 0 && f.target) {
        ctx_->OMSetRenderTargets(1, &f.target, f.depth);
        D3D11_VIEWPORT vp{0, 0, float(f.width), float(f.height), 0, 1};
        ctx_->RSSetViewports(1, &vp);
        ctx_->OMSetBlendState(bs_premultiplied_.Get(), nullptr, 0xffffffff);
        ctx_->OMSetDepthStencilState(f.depth ? ds_test_.Get() : ds_off_.Get(), 0);
        ctx_->IASetInputLayout(nullptr);
        ctx_->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_POINTLIST);
        bind_shader(ctx_, *vs, cbs_);
        bind_emitter_cbs(*vs, false);
        bind_srv(ctx_, *vs, "g_emitter_instances", emitter_srv_.Get());
        bind_srv(ctx_, *vs, "g_particle_instances", particles_[cur_].srv.Get());
        bind_srv(ctx_, *vs, "g_tex_sorted_data", keys_[sorted_].srv.Get());
        bind_srv(ctx_, *vs, "g_tex_emitter_constants", curves_srv_.Get());
        bind_sampler(ctx_, *vs, "g_sam_emitter_constants", s_linear_clamp_.Get());
        bind_shader(ctx_, *gs, cbs_);
        bind_emitter_cbs(*gs, false);
        bind_shader(ctx_, *ps, cbs_);
        bind_emitter_cbs(*ps, false);
        bind_srv(ctx_, *ps, "s_environment", f.sky);
        bind_srv(ctx_, *ps, "s_sky", f.sky);
        bind_srv(ctx_, *ps, "gbuffer_channel_4_sampler", f.depth_zw);
        bind_srv(ctx_, *ps, "g_tex_diffuse", diffuse_srv_.Get());
        bind_srv(ctx_, *ps, "g_tex_normal", normal_srv_.Get());
        bind_sampler(ctx_, *ps, "s_environment", s_linear_clamp_.Get());
        bind_sampler(ctx_, *ps, "s_sky", s_linear_clamp_.Get());
        bind_sampler(ctx_, *ps, "gbuffer_channel_4_sampler", s_point_clamp_.Get());
        bind_sampler(ctx_, *ps, "g_sam_diffuse", s_linear_wrap_.Get());
        bind_sampler(ctx_, *ps, "g_sam_normal", s_linear_wrap_.Get());
        ctx_->Draw(alive_ + 1, 0);
        ctx_->GSSetShader(nullptr, nullptr, 0);
        unbind();
        ctx_->OMSetBlendState(nullptr, nullptr, 0xffffffff);
    }
}

} // namespace we
