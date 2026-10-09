#pragma once
// GPU-side resources the renderer builds from scene payloads: textures by VFS path, rigid models by VFS path, terrain meshes
// from height grids. Entities name their assets; this cache resolves them (a model that fails to load is reported once and skipped).
#include "foundation/math.h"
#include "render/texture.h"
#include "scene/render_scene.h"

#include <d3d11.h>
#include <wrl/client.h>

#include <cstdint>
#include <map>
#include <memory>
#include <string>
#include <vector>

namespace wce {

class IVfs;

struct GpuMesh {
    Microsoft::WRL::ComPtr<ID3D11Buffer> vb, ib;
    uint32_t index_count = 0;
    int alpha_mode = 0;
    float3 offset;                   // mesh-local origin (terrain patches keep half-float positions small), zero for models
    bool has_offset = false;
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

class TextureCache {
public:
    TextureCache(ID3D11Device* dev, IVfs& vfs);
    const Texture* get(const std::string& path, bool srgb); // nullptr if missing/unreadable
    const Texture* white() const { return &white_; }
    const Texture* black() const { return &black_; }
    const Texture* flat_normal() const { return &flat_normal_; }
    const std::vector<std::string>& errors() const { return errors_; }

private:
    ID3D11Device* dev_;
    IVfs& vfs_;
    std::map<std::string, std::unique_ptr<Texture>> cache_;
    Texture white_, black_, flat_normal_;
    std::vector<std::string> errors_;
};

class ModelCache {
public:
    ModelCache(ID3D11Device* dev, IVfs& vfs, TextureCache& tex) : dev_(dev), vfs_(vfs), tex_(tex) {}

    // RS_STANDARD_V5 meshes (vertex format 0) of LOD 0, or nullptr when the file has none / cannot be read.
    const GpuModel* model(const std::string& path);
    // Mesh of a Terrain entity, patches of 64 x 64 cells; rebuilt when the payload's version or layout changes.
    const GpuModel* terrain(EntityId id, const Terrain& t);
    // Drops terrain meshes of entities that no longer exist.
    void prune_terrain(const RenderScene& scene);

    const std::vector<std::string>& errors() const { return errors_; }

private:
    ID3D11Device* dev_;
    IVfs& vfs_;
    TextureCache& tex_;
    std::map<std::string, std::unique_ptr<GpuModel>> models_;
    struct TerrainEntry {
        size_t signature = 0;
        std::unique_ptr<GpuModel> model;
    };
    std::map<EntityId, TerrainEntry> terrain_;
    std::vector<std::string> errors_;
};

} // namespace wce
