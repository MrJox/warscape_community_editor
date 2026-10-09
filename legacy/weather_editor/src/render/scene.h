#pragma once
// Test map: randomly picked building models (RS_STANDARD_V5 meshes, LOD0) on a grid.
#include "foundation/math.h"
#include "render/texture.h"

#include <d3d11.h>
#include <wrl/client.h>

#include <cstdint>
#include <map>
#include <memory>
#include <string>
#include <vector>

namespace wce {

class Vfs;

struct GpuMesh {
    Microsoft::WRL::ComPtr<ID3D11Buffer> vb, ib;
    uint32_t index_count = 0;
    int alpha_mode = 0;
    // Shader texture names (s_diffuse_map, ...) -> textures
    std::map<std::string, const Texture*> textures;
};

struct GpuModel {
    std::string path;
    std::vector<GpuMesh> meshes;
    float3 bmin, bmax;
};

// Per-instance vertex data, POSITION1..7 of rigidcommon.hlsl vs30_main (see docs/re/renderer.md).
struct RigidInstance {
    float world[3][4];      // POSITION1..3: rows of the 3x4 world matrix
    float p4[4] = {0, 0, 0, 0}; // POSITION4 -> PS v6 (.y also -> v9.w)
    float p5[4] = {0, 0, 0, 0}; // POSITION5 -> PS v7 (.w > 0: snow layer)
    float p6[4] = {0, 0, 0, 0}; // POSITION6 -> PS v8 (.y: damage brighten, .z: desaturate)
    float p7[4] = {0, 0, 0, 0}; // POSITION7 -> PS v11 (faction colour override, .w = which colour)
};

struct SceneObject {
    const GpuModel* model = nullptr;
    float4x4 world;
};

class TextureCache {
public:
    TextureCache(ID3D11Device* dev, Vfs& vfs);
    const Texture* get(const std::string& path, bool srgb); // nullptr if missing/unreadable
    const Texture* white() const { return &white_; }
    const Texture* black() const { return &black_; }
    const Texture* flat_normal() const { return &flat_normal_; }
    const std::vector<std::string>& errors() const { return errors_; }

private:
    ID3D11Device* dev_;
    Vfs& vfs_;
    std::map<std::string, std::unique_ptr<Texture>> cache_;
    Texture white_, black_, flat_normal_;
    std::vector<std::string> errors_;
};

class TestScene {
public:
    TestScene(ID3D11Device* dev, Vfs& vfs, TextureCache& tex) : dev_(dev), vfs_(vfs), tex_(tex) {}
    // Picks `count` random building models (seeded), lays them out on a grid. Returns models loaded.
    int generate(uint32_t seed, int count, const std::string& prefix = "rigidmodels/buildings/");
    const std::vector<SceneObject>& objects() const { return objects_; }
    float3 bounds_min() const { return bmin_; }
    float3 bounds_max() const { return bmax_; }
    const std::vector<std::string>& errors() const { return errors_; }
    int building_count() const { return buildings_; }

    // Flat ground: one RS_STANDARD_V5-style quad (vertex format 0) with a tiled battle terrain texture.
    void build_ground(const std::string& diffuse, float half_size = 1000.0f, float tile_metres = 8.0f);

private:
    const GpuModel* load_model(const std::string& path);
    std::unique_ptr<GpuModel> ground_;

    ID3D11Device* dev_;
    Vfs& vfs_;
    TextureCache& tex_;
    std::map<std::string, std::unique_ptr<GpuModel>> models_;
    std::vector<SceneObject> objects_;
    std::vector<std::string> errors_;
    float3 bmin_, bmax_;
    int buildings_ = 0;
};

} // namespace wce
