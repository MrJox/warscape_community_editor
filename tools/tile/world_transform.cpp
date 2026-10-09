#include "tile/world_transform.h"

#include <algorithm>
#include <cmath>
#include <vector>

namespace wce {

static Mat3 rot_x(double a) { double c = std::cos(a), s = std::sin(a); return {1, 0, 0, 0, c, -s, 0, s, c}; }
static Mat3 rot_y(double a) { double c = std::cos(a), s = std::sin(a); return {c, 0, s, 0, 1, 0, -s, 0, c}; }
static Mat3 rot_z(double a) { double c = std::cos(a), s = std::sin(a); return {c, -s, 0, s, c, 0, 0, 0, 1}; }

Mat3 mat_mul(const Mat3& a, const Mat3& b) {
    Mat3 r{};
    for (int i = 0; i < 3; ++i)
        for (int j = 0; j < 3; ++j)
            r[i * 3 + j] = a[i * 3] * b[j] + a[i * 3 + 1] * b[3 + j] + a[i * 3 + 2] * b[6 + j];
    return r;
}

Vec3 rotate_vec(const Mat3& m, const Vec3& v) {
    return {m[0] * v[0] + m[1] * v[1] + m[2] * v[2], m[3] * v[0] + m[4] * v[1] + m[5] * v[2], m[6] * v[0] + m[7] * v[1] + m[8] * v[2]};
}

Mat3 mat_transpose(const Mat3& m) { return {m[0], m[3], m[6], m[1], m[4], m[7], m[2], m[5], m[8]}; }

Mat3 euler_matrix(const Vec3& e) { return mat_mul(mat_mul(rot_z(e[2]), rot_y(e[0])), rot_x(-e[1])); }

Mat3 rotation_y(double angle) { return rot_y(angle); }

Vec3 euler_from_matrix(const Mat3& m) {
    // m = Rz(c) Ry(a) Rx(beta), beta = -b:  m[6] = -sin a, m[7] = sin(beta) cos a, m[8] = cos(beta) cos a, m[3] = sin c cos a, m[0] = cos c cos a
    double sa = std::max(-1.0, std::min(1.0, -m[6]));
    double a = std::asin(sa), beta, c;
    if (std::fabs(sa) < 1.0 - 1e-12) {
        beta = std::atan2(m[7], m[8]);
        c = std::atan2(m[3], m[0]);
    } else {
        c = 0;
        beta = std::atan2(-m[5], m[4]);
    }
    return {a, -beta, c};
}

void set_world_rotation(TedScene& scene, int elem, const Mat3& world) {
    int parent = scene.doc().elements()[elem].parent;
    WorldTransform pw;
    if (parent >= 0) pw = world_transform(scene, parent);
    scene.set_euler(elem, euler_from_matrix(mat_mul(mat_transpose(pw.rotation), world)));
}

void rotate_world_yaw(TedScene& scene, int elem, double delta) {
    int parent = scene.doc().elements()[elem].parent;
    WorldTransform pw;
    if (parent >= 0) pw = world_transform(scene, parent);
    Mat3 local = euler_matrix(scene.euler(elem).value_or(Vec3{0, 0, 0}));
    Mat3 world = mat_mul(pw.rotation, local);
    Mat3 rotated = mat_mul(rot_y(delta), world);
    scene.set_euler(elem, euler_from_matrix(mat_mul(mat_transpose(pw.rotation), rotated)));
}

WorldTransform world_transform(const TedScene& scene, int elem) {
    std::vector<int> chain;
    for (int e = elem; e >= 0; e = scene.doc().elements()[e].parent) chain.push_back(e);
    WorldTransform w;
    for (size_t k = chain.size(); k-- > 0;) {
        int e = chain[k];
        auto p = scene.position(e);
        auto o = scene.euler(e);
        if (p) {
            Vec3 r = rotate_vec(w.rotation, *p);
            for (int i = 0; i < 3; ++i) w.position[i] += r[i];
        }
        if (o) w.rotation = mat_mul(w.rotation, euler_matrix(*o));
    }
    return w;
}

void set_world_position(TedScene& scene, int elem, const Vec3& world) {
    int parent = scene.doc().elements()[elem].parent;
    WorldTransform pw;
    // skip the `children` wrapper: world_transform walks real ancestors, wrappers have no position/orientation
    if (parent >= 0) pw = world_transform(scene, parent);
    Vec3 d = {world[0] - pw.position[0], world[1] - pw.position[1], world[2] - pw.position[2]};
    scene.set_position(elem, rotate_vec(mat_transpose(pw.rotation), d));
}

} // namespace wce
