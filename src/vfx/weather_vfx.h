#pragma once
// Precipitation and weather particles of one weather definition:
//   .environment WEATHER definition_name -> weather/weather_definitions.xml WEATHER_DEFINITION (vfx1, wind)
//   -> vfx/<vfx1>.xml -> vfx_references -> library emitters (enable / tint overrides) + behaviour chains.
#include "vfx_constants.h"
#include "vfx_def.h"

#include <cstdint>
#include <functional>
#include <string>
#include <vector>

namespace we {

class Vfs;

struct ActiveEmitter {
    std::string library, instance, name; // library file, reference inst_name, emitter id
    VfxEmitterDef def;
    VfxChain chain;
    EmitterConstants k;
    float tint_min[4] = {1, 1, 1, 1}, tint_max[4] = {1, 1, 1, 1};
    float particle_lifetime_bias[2] = {0, 0};
    float scale = 1, pos[3] = {0, 0, 0};
    bool camera_following = false;
};

// weather/weather_definitions.xml WEATHER_DEFINITION. Defaults = the engine's constructor (fn 0x1704210); field
// offsets of the engine struct (loader fn 0x1738a60) in comments. The gust names in old .environment files
// (wind_gust_prob, _rampup, _max, _size_variety ...) are not read by the game.
struct WeatherDefinition {
    std::string name, type, vfx1, vfx2; // type +0x1c (WT_NONE 0, WT_RAIN 1, WT_SNOW 2 -> weather_shared.g_weather_type)
    int severity = 0;                   // +0x20
    float wind_size_min = 0, wind_size_max = 1;     // +0x3c, +0x40
    float wind_gust_size_variation = 0.5f;          // +0x44
    float wind_gust_duration = 2, wind_gust_duration_variation = 0.5f; // +0x48, +0x4c
    float wind_gust_interval = 3, wind_gust_probability = 1;           // +0x50, +0x54
    float wind_gust_curve = 4, wind_vegetation_multiplier = 0;         // +0x58, +0x5c
    int type_index() const { return type == "WT_RAIN" ? 1 : type == "WT_SNOW" ? 2 : type == "WT_DUST" ? 3 : 0; }
};

struct WeatherVfx {
    WeatherDefinition definition;
    std::vector<ActiveEmitter> emitters;
    std::vector<std::string> notes; // skipped emitters / missing files, for the UI
};

// Bytes of an in-pack file; throws if missing. The editor passes one that prefers open (edited) documents.
using VfxReader = std::function<std::vector<uint8_t>(const std::string& path)>;

// Reads weather/weather_definitions.xml (empty name = "none"); no vfx1/vfx2 gives an effect without emitters.
WeatherVfx load_weather_vfx(const VfxReader& read, const std::string& definition_name);
WeatherVfx load_weather_vfx(Vfs& vfs, const std::string& definition_name);

} // namespace we
