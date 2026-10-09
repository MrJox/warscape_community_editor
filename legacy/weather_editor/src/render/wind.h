#pragma once
// Battle wind as the engine simulates it (empire.retail.dll 1.6.0, verified against three memory dumps):
//   direction  fn 0x17958b0: (cos, sin) of WEATHER wind_dir_degrees; wind_dir_random (fn 0x175d940) replaces the
//              degrees once per battle with rand16 * 360 / 65535.
//   gusts      fn 0x18dfd30 (WEATHER node +0x7a0..+0x7cc): strength = wind_size_min, plus a gust shaped
//              exp(-0.5 * ((x - 1) * wind_gust_curve)^2), x = (t - start) / duration in 0..1.
//   shaders    fn 0x18ee0c0 (weather_shared, wind_VS, wind_PS), fn 0x1923990, fn 0x191e430 (cloud_plane_VS)
//   VFX        fn 0x18f8b10 (wind_velocity = 20 * wind), fn 0x18e51e0 (wind_variance_dir random walk).
#include "render/math.h"

#include <cstdint>

namespace we {

struct WeatherDefinition;

class WindSim {
public:
    // Engine variables read from the dumps (same in all three).
    static constexpr float kVfxWindScale = 20.0f;         // dll var +0x2a787c8
    static constexpr float kVarianceRate = 5.0f;          // dll var +0x2a776a8 (radians per second, random sign)
    static constexpr float kSmoothingPeriod = 60.0f;      // WEATHER node +0x80c
    static constexpr float kVegetationHStrength = 0.91f;  // wind_VS.wind_vegetation_hstrength
    static constexpr float kVegetationVStrength = 0.08f;  // wind_VS.wind_vegetation_vstrength

    void reset(uint32_t seed = 0x2545f491u);
    // time: battle clock (seconds), dt: frame time. def may be null (no weather node: VFX wind (0.1, 0, 0) * 20).
    void update(const WeatherDefinition* def, float direction_degrees, float time, float dt);

    // Random battle direction for wind_dir_random (fn 0x175d940).
    static float random_direction(uint32_t& seed);

    float2 direction{1, 0};   // wind_VS.wind_direction, wind_PS.wind_direction_PS
    float strength = 0;       // WEATHER node +0x7a8
    float gust_end = 0;       // wind_VS.gust_start_time (= start + duration of the last gust)
    float2 smoothed{0, 0};    // weather_shared.g_wind (running average over kSmoothingPeriod, length <= 1)
    float2 offset{0, 0};      // weather_shared.g_wind_offset (integral of g_wind)
    float3 vfx_velocity{2, 0, 0}, vfx_variance{2, 0, 0}; // cb_vfx_game_simulation_data
    float alignment() const;  // cloud_plane_VS.g_wind_alignment = atan2(direction.y, direction.x)

    // gust state (WEATHER node fields)
    float gust_size = 0, gust_duration = 0, gust_start = 0, cooldown = 0;

private:
    uint32_t gust_rng_ = 0, variance_rng_ = 0;
    float variance_angle_ = 0;
    float2 accum_{0, 0}, previous_{0, 0};
    float accum_time_ = 0;
    static float rand01(uint32_t& s); // engine LCG: s * 0x343fd + 0x269ec3, (s >> 16) / 65535
};

} // namespace we
