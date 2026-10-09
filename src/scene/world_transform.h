#pragma once
// World transforms of tile.ted nodes, for display and editing (double precision, not the exporter's float32 path).
//
// Convention measured against TEd's building export (docs/formats/tile_ted.md): a node's orientation euler(a,b,c) is the
// rotation Rz(c) * Ry(a) * Rx(-b) (column vectors); parents compose before children; positions are rotated by the parent
// rotation and added. Scene positions are in half metres horizontally (unit scale 2), heights in metres.
#include "scene/ted_scene.h"

#include <array>

namespace te {

using Mat3 = std::array<double, 9>;           // row-major

Mat3 euler_matrix(const Vec3& e);              // Rz(c) * Ry(a) * Rx(-b)
Mat3 mat_mul(const Mat3& a, const Mat3& b);
Vec3 rotate_vec(const Mat3& m, const Vec3& v);
Mat3 mat_transpose(const Mat3& m);

struct WorldTransform {
    Vec3 position{};
    Mat3 rotation{1, 0, 0, 0, 1, 0, 0, 0, 1};
};

// Position / rotation of `elem` in scene space (all ancestors applied).
WorldTransform world_transform(const TedScene& scene, int elem);

// Euler angles (a, b, c) with euler_matrix(e) == m (Rz(c) Ry(a) Rx(-b)); gimbal lock resolves with c = 0.
Vec3 euler_from_matrix(const Mat3& m);
Mat3 rotation_y(double angle);

// Sets the local orientation of `elem` so that its world rotation becomes Ry(world_yaw_delta) * (current world rotation).
void rotate_world_yaw(TedScene& scene, int elem, double delta);

// Sets the local orientation of `elem` so that its world rotation becomes `world` (ancestors unchanged).
void set_world_rotation(TedScene& scene, int elem, const Mat3& world);

// Sets the local position of `elem` so that its world position becomes `world` (ancestors unchanged).
void set_world_position(TedScene& scene, int elem, const Vec3& world);

} // namespace te
