#pragma once
// Battle cloud planes (SKY_DOME > CLOUD_PLANE_MESH > CLOUD > CLOUD_ELEMENT), rebuilt from empire.retail.dll:
//   init      fn 0x18e5b10: every cell of a (grid_size + 2)^2 grid spawns a cloud when rand <= cloud_density
//   spawn     fn 0x18f0bf0: jitter, cloud rotation, template choice, per-element sprites
//   template  fn 0x1905f60: first CLOUD with this_cloud_only, else weighted by probability
//   sprite    fn 0x18d82a0: the per-instance data cloud_plane.hlsl vs30_main reads (TEXCOORD1..5)
// Random numbers: MSVC rand (state * 214013 + 2531011, 16 high bits / 65535).
// Dome constants per plane type come from a memory dump of the game (not derived from the XML):
//   near: radius 608000, centre -592800, jitter 0.5, atlas 2 x 2;  far: radius 972800, centre -912000, jitter 0, atlas 1 x 2.
// The cell angle is 2 * acos(-centre / radius) / grid_size (the dome cap down to the horizon), verified on both planes.
#include <cstdint>
#include <string>
#include <vector>

namespace we {

class XmlDoc;

struct CloudElement {
    bool enabled = false;
    float scale = 0, offset[3] = {0, 0, 0};
    int texture_index = 0, texture_row = 0, texture_column = 0; // 0 = random
};

struct CloudTemplate {
    bool this_cloud_only = false;
    float probability = 0;
    std::vector<CloudElement> elements;
};

struct CloudPlane {
    bool far_plane = false;
    int grid_size = 1;
    float height_scale = 1, velocity = 0, density = 0, flatten = 0;
    float transmitted[3] = {1, 1, 1}, reflected[3] = {1, 1, 1}, shadow[3] = {0, 0, 0};
    float transmit_falloff = 0, shadow_gradient = 0, shadow_falloff = 0, horizon_alpha_start = 0, horizon_alpha_end = 1;
    float height_variation = 0, rotation = 0;
    std::string cloudset = "CLOUDSET_1";
    std::vector<CloudTemplate> clouds;
};

// One sprite = one instance of the cloud quad (layout of TEXCOORD1..5 in cloud_plane.hlsl vs30_main).
struct CloudInstance {
    float t1[4]; // atlas u, v, tile u size, tile v size
    float t2[4]; // dome angles x, y; dome radius; dome centre (y)
    float t3[4]; // 1, sprite rotation, angular size, cloud rotation
    float t4[4]; // element offset in metres x, y, z; 1
    float t5[2]; // atlas texture (0/1), cloud radius
};

std::vector<CloudPlane> read_cloud_planes(const XmlDoc& doc);

// A plane's live clouds: initial fill (fn 0x18e5b10), drift (fn 0x192aa40: angle x += speed * dt, the cloud dies
// past the far edge) and new clouds entering at column 0 (fn 0x192a900).
// ASSUMPTIONS: a new column every cell_angle / velocity seconds; clouds die beyond (cells / 2) * cell_angle.
class CloudField {
public:
    void reset(const CloudPlane& plane, uint32_t seed);
    void update(const CloudPlane& plane, float dt); // returns nothing; instances() changes when clouds move
    const std::vector<CloudInstance>& instances() const { return inst_; }

private:
    void spawn(const CloudPlane& plane, int x, int y);
    struct Rng { uint32_t state = 1; float next(); } rng_;
    std::vector<CloudInstance> inst_;
    std::vector<float> speed_; // per instance (sprites of one cloud share it)
    float timer_ = 0;
};

// Deterministic for a given seed (the game's state is per session, so positions differ; the distribution matches).
std::vector<CloudInstance> generate_clouds(const CloudPlane& plane, uint32_t seed);
// "rigidmodels/skybox/cloudsets/cloudset_2/near_set1.dds" etc.
std::string cloud_texture(const CloudPlane& plane, int map);

} // namespace we
