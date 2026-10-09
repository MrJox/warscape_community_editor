#include "render/wind.h"

#include "assets/weather_vfx.h"

#include <algorithm>
#include <cmath>
#include <cstring>

namespace wce {

namespace {
constexpr float kTwoPi = 6.2831853f;

// The engine's |v| for wind_variance_dir: integer-halving square root on the float bits (fn 0x18e51e0),
// e.g. 5.954 for |v| = 5.624 (rain dump).
float approx_sqrt(float x) {
    int32_t i;
    std::memcpy(&i, &x, 4);
    i = ((i - 0x3f800000) >> 1) + 0x3f800000;
    float r;
    std::memcpy(&r, &i, 4);
    return r;
}
} // namespace

float WindSim::rand01(uint32_t& s) {
    s = s * 0x343fdu + 0x269ec3u;
    return float(s >> 16) * 1.5259022e-05f; // DAT 0x1afdce4
}

float WindSim::random_direction(uint32_t& seed) { return float((seed = seed * 0x343fdu + 0x269ec3u) >> 16) * 0.0054932478f; }

void WindSim::reset(uint32_t seed) {
    *this = WindSim{};
    gust_rng_ = seed;
    variance_rng_ = seed ^ 0x9e3779b9u;
}

float WindSim::alignment() const { return std::atan2(direction.y, direction.x); }

void WindSim::update(const WeatherDefinition* def, float direction_degrees, float t, float dt) {
    const float a = direction_degrees * 0.017453294f;
    direction = {std::cos(a), std::sin(a)};

    // --- gusts (fn 0x18dfd30) ---
    if (def) {
        strength = def->wind_size_min;
        const float end = gust_start + gust_duration;
        if (t > end && cooldown <= 0) {
            if (rand01(gust_rng_) <= def->wind_gust_probability) {
                const float sv = def->wind_gust_size_variation, dv = def->wind_gust_duration_variation;
                gust_size = (rand01(gust_rng_) * sv + (1 - sv)) * (def->wind_size_max - def->wind_size_min);
                gust_duration = (rand01(gust_rng_) * dv + (1 - dv)) * def->wind_gust_duration;
                gust_start = t;
            }
            cooldown = def->wind_gust_interval;
        } else if (gust_start < t && t < end) {
            // The engine doubles x when its "gust centred" flag (var +0x2a74990) is set; it is 0 in every dump.
            const float x = ((t - gust_start) / gust_duration - 1) * def->wind_gust_curve;
            strength = def->wind_size_min + std::pow(2.7182817f, x * x * -0.5f) * gust_size;
        } else if (cooldown > 0) {
            cooldown -= dt;
        }
        gust_end = gust_start + gust_duration;
    } else {
        strength = 0;
    }
    const float2 wind{direction.x * strength, direction.y * strength};

    // --- weather_shared (fn 0x18ee0c0): running average of the wind over kSmoothingPeriod ---
    accum_.x += wind.x * dt;
    accum_.y += wind.y * dt;
    accum_time_ += dt;
    float2 w{wind.x, wind.y};
    if (accum_time_ >= kSmoothingPeriod) {
        previous_ = {accum_.x / accum_time_, accum_.y / accum_time_};
        w = previous_;
        accum_ = {0, 0};
        accum_time_ = 0;
    } else if (accum_time_ > 0) {
        const float f = accum_time_ / kSmoothingPeriod;
        w = {accum_.x / accum_time_ * f + (1 - f) * previous_.x, accum_.y / accum_time_ * f + (1 - f) * previous_.y};
    }
    const float len = std::sqrt(w.x * w.x + w.y * w.y);
    if (len > 0) {
        const float k = std::min(len, 1.0f) / len;
        w = {w.x * k, w.y * k};
    }
    smoothed = w;
    offset.x += w.x * dt;
    offset.y += w.y * dt;

    // --- VFX (fn 0x18f8b10, fn 0x18e51e0) ---
    vfx_velocity = def ? float3{wind.x, 0, wind.y} * kVfxWindScale : float3{0.1f, 0, 0} * kVfxWindScale;
    const float r = rand01(variance_rng_);
    variance_angle_ = std::fmod(variance_angle_ + (r + r - 1) * dt * kVarianceRate, kTwoPi);
    const float m = approx_sqrt(dot(vfx_velocity, vfx_velocity));
    vfx_variance = {std::cos(variance_angle_) * m, 0, std::sin(variance_angle_) * m};
}

} // namespace wce
