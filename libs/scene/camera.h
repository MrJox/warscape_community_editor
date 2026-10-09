#pragma once
#include "foundation/math.h"

namespace wce {

struct Camera {
    float3 eye{0, 30, -80}, target{0, 5, 0};
    // The game's battle camera (memory dump): near 1, far 13000, vertical FOV 1.13446 rad (65 degrees).
    float fov_y = 1.13446f, znear = 1.0f, zfar = 13000.0f;
};

} // namespace wce
