#pragma once
// Precipitation and weather particles of one weather definition:
//   .environment WEATHER definition_name -> weather/weather_definitions.xml WEATHER_DEFINITION (vfx1, wind)
//   -> vfx/<vfx1>.xml -> vfx_references -> library emitters (enable / tint overrides) + behaviour chains.
#include "vfx_constants.h"
#include "vfx_def.h"

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

struct WeatherDefinition {
    std::string name, type, vfx1;
    float wind_size_min = 0, wind_size_max = 0, wind_vegetation_multiplier = 0;
};

struct WeatherVfx {
    WeatherDefinition definition;
    std::vector<ActiveEmitter> emitters;
    std::vector<std::string> notes; // skipped emitters / missing files, for the UI
};

// Reads weather/weather_definitions.xml; empty name / "none" or no vfx1 gives an empty effect.
WeatherVfx load_weather_vfx(Vfs& vfs, const std::string& definition_name);

} // namespace we
