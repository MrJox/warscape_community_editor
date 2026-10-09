#pragma once
// GPU particles with the game's particle_vfx_*.hlsl shaders (layouts in docs/re/particles.md).
//
// Per frame: simulation (ping-pong of the particle state arrays) -> sort keys -> bitonic sort -> spawn (CPU-made
// points written into free slots taken from the end of the sorted list) -> draw (VS over the sorted list, GS quads).
// The CPU side (emission rate, spawn positions and velocities) follows the XML semantics; the exact game code is
// not reverse-engineered yet, see the ASSUMPTION notes in particles.cpp.
#include "foundation/math.h"
#include "render/texture.h"
#include "vfx/weather_vfx.h"

#include <d3d11.h>
#include <wrl/client.h>

#include <deque>
#include <map>
#include <memory>
#include <string>
#include <vector>

namespace wce {

class Vfs;
class ShaderLibrary;
class ConstantBuffers;
struct GameShader;

class ParticleSystem {
public:
    struct Frame {
        float3 eye;
        float4x4 view;     // rows 0..2: camera right, up, forward
        float time = 0, dt = 0;
        float3 wind_velocity, wind_variance; // cb_vfx_game_simulation_data (render/wind.h)
        ID3D11RenderTargetView* target = nullptr;
        ID3D11DepthStencilView* depth = nullptr;
        ID3D11ShaderResourceView* depth_zw = nullptr; // gbuffer channel 4 (z/w)
        ID3D11ShaderResourceView* sky = nullptr;      // sky cube
        int width = 0, height = 0;
    };

    ParticleSystem(ID3D11Device* dev, ID3D11DeviceContext* ctx, Vfs& vfs, ShaderLibrary& lib, ConstantBuffers& cbs);
    ~ParticleSystem();

    // Loads the effect of a weather definition (no-op when unchanged). Restarts the simulation.
    // Reloads when the name or `revision` changes (revision: the editor's edit counter of open vfx/weather XML).
    // Live particles are kept when the reload yields the same emitters.
    void set_weather(const std::string& definition_name, uint64_t revision = 0);
    void set_reader(VfxReader reader) { reader_ = std::move(reader); }
    void restart();
    void render(const Frame& f);

    bool enabled = true;
    const WeatherVfx& effect() const { return effect_; }
    uint32_t alive() const { return alive_; }
    // Upper bound of live particles of one emitter (same estimate as alive()).
    uint32_t alive(size_t emitter) const {
        uint32_t n = 0;
        if (emitter < state_.size())
            for (auto& b : state_[emitter].deaths) n += b.second;
        return n;
    }
    uint32_t capacity() const { return kWidth * kWidth; }
    const std::vector<std::string>& errors() const { return errors_; }
    // Distortion buffer of this frame (heat haze, lens drops), or nullptr when no such emitter drew.
    ID3D11ShaderResourceView* distortion() const { return distortion_active_ ? dist_srv_.Get() : nullptr; }

private:
    static constexpr int kWidth = 512;          // particle state and sorted-data textures (kWidth^2 slots)
    static constexpr int kEmitterWidth = 256;   // g_emitter_instance_tex_width (dump)
    static constexpr int kEmitterLayers = 6;

    struct Target {
        Microsoft::WRL::ComPtr<ID3D11Texture2D> tex;
        Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> srv;
        Microsoft::WRL::ComPtr<ID3D11RenderTargetView> rtv[3];
    };
    struct EmitterState {
        float rate_accum = 0;
        float rate_blend = -1; // emission min->max blend, random once per emitter instance (game: instance +0x100)
        // Burst gate (fn 0x1180c680, instance +0x104 timer, +0x108 interval, +0x10c length, +0xf4 gate): with a
        // BurstInterval > 0 the emitter only spawns while timer <= length, then waits `interval`; both re-rolled
        // from BurstInterval / BurstIntervalLength each cycle (fn 0x1180bf70).
        float burst_timer = 0, burst_interval = -1, burst_length = 0;
        std::deque<std::pair<float, uint32_t>> deaths; // (latest death time, count) per spawned batch
        float3 last_pos;
        bool started = false;
    };
    struct SpawnVertex {
        float id_seed_steps[4]; // emitter instance, random seed, pre-simulation sub-steps (0.2 s each), 0
        float pos_time[4];      // position, spawn time
        float vel_life[4];      // velocity, lifetime
    };

    void create_resources();
    void build_emitter_gpu();
    void bind_emitter_cbs(const GameShader& s, bool sim_layout);
    void fullscreen(ID3D11RenderTargetView* const* rtvs, int n, int w, int h);
    void spawn(const Frame& f, std::vector<SpawnVertex>& out);
    float rand01();
    void error(const std::string& e);

    ID3D11Device* dev_;
    ID3D11DeviceContext* ctx_;
    Vfs& vfs_;
    ShaderLibrary& lib_;
    ConstantBuffers& cbs_;
    std::vector<std::string> errors_;

    std::string definition_;
    uint64_t revision_ = 0;
    VfxReader reader_;
    std::map<std::string, Texture> texture_cache_; // path|srgb -> loaded DDS
    bool loaded_ = false;
    WeatherVfx effect_;
    std::vector<EmitterState> state_;
    float clock_ = 0;
    uint32_t alive_ = 0, rng_ = 0x1234567u;
    bool clear_ = true;

    Target particles_[2]; // 3-slice arrays: (id, seed, steps), (pos, spawn time), (velocity, lifetime)
    int cur_ = 0;
    Target keys_[2];      // (distance, index) sort keys, ping-pong
    int sorted_ = 0;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> emitter_tex_;
    Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> emitter_srv_;
    Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> curves_srv_, diffuse_srv_, normal_srv_, flat_normal_srv_, vector_field_srv_, sea_srv_;
    bool has_distortion_ = false;
    Microsoft::WRL::ComPtr<ID3D11Buffer> cb_common_, cb_gs_, cb_vs_, cb_ps_render_, cb_ps_sim_;
    Microsoft::WRL::ComPtr<ID3D11Buffer> spawn_vb_;
    UINT spawn_capacity_ = 0;
    Microsoft::WRL::ComPtr<ID3D11SamplerState> s_linear_clamp_, s_linear_wrap_, s_point_clamp_;
    Microsoft::WRL::ComPtr<ID3D11BlendState> bs_premultiplied_, bs_opaque_, bs_alpha_;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> dist_tex_;
    Microsoft::WRL::ComPtr<ID3D11RenderTargetView> dist_rtv_;
    Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> dist_srv_;
    int dist_w_ = 0, dist_h_ = 0;
    bool distortion_active_ = false;
    Microsoft::WRL::ComPtr<ID3D11DepthStencilState> ds_test_, ds_off_;
    Microsoft::WRL::ComPtr<ID3D11RasterizerState> rs_none_;
};

} // namespace wce
