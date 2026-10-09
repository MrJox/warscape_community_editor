#pragma once
// Viewport camera controllers. They turn mouse and key state into a Camera; the viewport feeds them and passes the result to the renderer.
// Both keep the game's battle camera lens (vertical FOV 65 degrees, near 1, far 13000, from the memory dump) unless the lens is edited.
#include "scene/camera.h"

#include <memory>

namespace wce {

struct CameraInput {
    float dx = 0, dy = 0;              // mouse movement in pixels this frame
    float wheel = 0;                   // wheel notches
    bool rotate = false;               // left or right button held over the viewport
    bool pan = false;                  // middle button
    bool forward = false, back = false, left = false, right = false, up = false, down = false;
    bool fast = false;                 // shift
    float dt = 0;                      // seconds
};

class CameraController {
public:
    virtual ~CameraController() = default;
    virtual const char* name() const = 0;
    virtual void update(const CameraInput& in) = 0;
    virtual Camera camera() const = 0;
    virtual void set_camera(const Camera& cam) = 0;                  // continue from this view (switching controllers)
    virtual void focus(const float3& center, float radius) = 0;      // frame a sphere
    float fov_y = 1.13446f, znear = 1.0f, zfar = 13000.0f;           // the game's battle camera
};

// Turns around a target: rotate with the mouse, pan with the middle button, wheel zooms.
class OrbitCamera : public CameraController {
public:
    const char* name() const override { return "Orbit"; }
    void update(const CameraInput& in) override;
    Camera camera() const override;
    void set_camera(const Camera& cam) override;
    void focus(const float3& center, float radius) override;
    void set_orbit(float yaw_, float pitch_, float dist_, float3 target_) { yaw = yaw_; pitch = pitch_; dist = dist_; target = target_; }

    float yaw = 0.6f, pitch = 0.35f, dist = 120.0f;
    float3 target{0, 5, 0};
    float min_eye_height = 2.0f;      // stay above the ground plane when looking up
};

// Free flight: right button looks around, WASD moves, Q/E down and up, the wheel changes the speed.
class FlyCamera : public CameraController {
public:
    const char* name() const override { return "Fly"; }
    void update(const CameraInput& in) override;
    Camera camera() const override;
    void set_camera(const Camera& cam) override;
    void focus(const float3& center, float radius) override;

    float3 eye{0, 30, -80};
    float yaw = 0, pitch = -0.2f;
    float speed = 40.0f;              // metres per second
};

} // namespace wce
