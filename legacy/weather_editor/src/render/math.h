#pragma once
// Minimal math for the renderer.
//
// Convention (from the game's vertex shaders: clip.i = dp4(pos, cb[row i])): matrices are stored
// row-major with column vectors, p' = M * p, and uploaded as-is, one row per 16-byte register.
// World is Y-up; depth is standard D3D (0 near, 1 far; the sky is drawn at z = w).
#include <cmath>

namespace we {

struct float2 {
    float x = 0, y = 0;
};

struct float3 {
    float x = 0, y = 0, z = 0;
    float3() = default;
    constexpr float3(float x_, float y_, float z_) : x(x_), y(y_), z(z_) {}
    float3 operator+(const float3& o) const { return {x + o.x, y + o.y, z + o.z}; }
    float3 operator-(const float3& o) const { return {x - o.x, y - o.y, z - o.z}; }
    float3 operator*(float s) const { return {x * s, y * s, z * s}; }
    float3 operator-() const { return {-x, -y, -z}; }
};
inline float dot(const float3& a, const float3& b) { return a.x * b.x + a.y * b.y + a.z * b.z; }
inline float3 cross(const float3& a, const float3& b) { return {a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x}; }
inline float length(const float3& a) { return std::sqrt(dot(a, a)); }
inline float3 normalize(const float3& a) { float l = length(a); return l > 0 ? a * (1.0f / l) : a; }

struct float4x4 {
    float m[4][4] = {{1, 0, 0, 0}, {0, 1, 0, 0}, {0, 0, 1, 0}, {0, 0, 0, 1}};
    const float* data() const { return &m[0][0]; }
    float4x4 operator*(const float4x4& o) const {
        float4x4 r;
        for (int i = 0; i < 4; ++i)
            for (int j = 0; j < 4; ++j) {
                r.m[i][j] = 0;
                for (int k = 0; k < 4; ++k) r.m[i][j] += m[i][k] * o.m[k][j];
            }
        return r;
    }
    float3 transform_point(const float3& p) const {
        float w = m[3][0] * p.x + m[3][1] * p.y + m[3][2] * p.z + m[3][3];
        return float3{m[0][0] * p.x + m[0][1] * p.y + m[0][2] * p.z + m[0][3], m[1][0] * p.x + m[1][1] * p.y + m[1][2] * p.z + m[1][3],
                      m[2][0] * p.x + m[2][1] * p.y + m[2][2] * p.z + m[2][3]} * (1.0f / w);
    }
};

float4x4 inverse(const float4x4& a);
float4x4 translation(const float3& t);
float4x4 rotation_x(float radians);
float4x4 rotation_y(float radians);
float4x4 rotation_z(float radians);
float4x4 scaling(float s);
// Left-handed look-at (camera looks down +Z in view space), D3D-style perspective (depth 0..1).
float4x4 look_at_lh(const float3& eye, const float3& target, const float3& up);
float4x4 perspective_lh(float fov_y, float aspect, float zn, float zf);
float4x4 ortho_lh(float w, float h, float zn, float zf);

} // namespace we
