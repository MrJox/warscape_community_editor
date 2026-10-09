#include "editor/camera_controller.h"

#include <algorithm>
#include <cmath>

namespace wce {

namespace {
constexpr float kPi = 3.14159265f;
}

// ---- orbit (the formulas of the weather tool's viewport, so existing --cam values mean the same)

void OrbitCamera::update(const CameraInput& in) {
    if (in.rotate) {
        yaw += in.dx * 0.006f;
        pitch = std::clamp(pitch + in.dy * 0.006f, -1.2f, 1.5f);
    }
    if (in.pan) {
        const float3 right{std::cos(yaw), 0, std::sin(yaw)};
        const float3 fwd{-std::sin(yaw), 0, std::cos(yaw)};
        const float k = dist * 0.0015f;
        target = target - right * (in.dx * k) + fwd * (in.dy * k);
    }
    if (in.wheel != 0) dist = std::clamp(dist * std::pow(0.9f, in.wheel), 2.0f, 6000.0f);
}

Camera OrbitCamera::camera() const {
    Camera c;
    const float cp = std::cos(pitch), sp = std::sin(pitch);
    c.target = target;
    c.eye = target + float3{std::sin(yaw) * cp, sp, -std::cos(yaw) * cp} * dist;
    c.eye.y = std::max(c.eye.y, min_eye_height);
    c.fov_y = fov_y;
    c.znear = znear;
    c.zfar = zfar;
    return c;
}

void OrbitCamera::set_camera(const Camera& cam) {
    const float3 d = cam.eye - cam.target;
    dist = std::max(1.0f, length(d));
    target = cam.target;
    pitch = std::asin(std::clamp(d.y / dist, -1.0f, 1.0f));
    yaw = std::atan2(d.x, -d.z);
}

void OrbitCamera::focus(const float3& center, float radius) {
    target = center;
    dist = std::clamp(radius / std::tan(fov_y * 0.5f) * 1.3f, 5.0f, 6000.0f);
}

// ---- fly

void FlyCamera::update(const CameraInput& in) {
    if (in.rotate) {
        yaw += in.dx * 0.004f;
        pitch = std::clamp(pitch - in.dy * 0.004f, -1.5f, 1.5f);
    }
    if (in.wheel != 0) speed = std::clamp(speed * std::pow(1.2f, in.wheel), 1.0f, 2000.0f);
    const float cp = std::cos(pitch);
    const float3 fwd{std::sin(yaw) * cp, std::sin(pitch), std::cos(yaw) * cp};
    const float3 right{std::cos(yaw), 0, -std::sin(yaw)};
    const float3 up{0, 1, 0};
    float3 move{0, 0, 0};
    if (in.forward) move = move + fwd;
    if (in.back) move = move - fwd;
    if (in.right) move = move + right;
    if (in.left) move = move - right;
    if (in.up) move = move + up;
    if (in.down) move = move - up;
    eye = eye + move * (speed * (in.fast ? 4.0f : 1.0f) * in.dt);
}

Camera FlyCamera::camera() const {
    Camera c;
    const float cp = std::cos(pitch);
    c.eye = eye;
    c.target = eye + float3{std::sin(yaw) * cp, std::sin(pitch), std::cos(yaw) * cp} * 10.0f;
    c.fov_y = fov_y;
    c.znear = znear;
    c.zfar = zfar;
    return c;
}

void FlyCamera::set_camera(const Camera& cam) {
    eye = cam.eye;
    const float3 d = normalize(cam.target - cam.eye);
    pitch = std::asin(std::clamp(d.y, -1.0f, 1.0f));
    yaw = std::atan2(d.x, d.z);
}

void FlyCamera::focus(const float3& center, float radius) {
    const float3 d = normalize(float3{std::sin(yaw) * std::cos(pitch), std::sin(pitch), std::cos(yaw) * std::cos(pitch)});
    eye = center - d * std::clamp(radius / std::tan(fov_y * 0.5f) * 1.3f, 5.0f, 6000.0f);
}

} // namespace wce
