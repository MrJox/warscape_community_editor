#pragma once
// GPU-side resources the renderer builds from scene payloads: textures by VFS path, rigid models by VFS path, terrain meshes
// from height grids. Entities name their assets; these caches resolve them (a model that fails to load is reported once and skipped).
//
// Loading is thread-safe and can run in the background: ModelCache::try_get queues a load on the io pool (priority = how close the model is)
// and returns nullptr until it is ready; ModelCache::model blocks (fixed-step runs, tools that need the bounds now). Direct3D 11 creates
// buffers and textures from any thread, so the whole job - read, parse, buffers, textures - runs in the worker.
#include "foundation/math.h"
#include "foundation/thread_pool.h"
#include "render/texture.h"
#include "scene/render_scene.h"

#include <d3d11.h>
#include <wrl/client.h>

#include <atomic>
#include <condition_variable>
#include <cstdint>
#include <map>
#include <memory>
#include <mutex>
#include <string>
#include <unordered_map>
#include <vector>

namespace wce {

class IVfs;

// The five texture slots of the rigid standard material, in the order of GpuMesh::tex.
enum GpuTexSlot { kTexDiffuse = 0, kTexSpecular, kTexGloss, kTexColourMask, kTexNormal, kTexSlots };
const char* gpu_texture_name(int slot);              // "s_diffuse_map", ...
int gpu_texture_slot(const std::string& shader_name);   // -1 for names that are not material textures

struct GpuMesh {
    Microsoft::WRL::ComPtr<ID3D11Buffer> vb, ib;
    uint32_t index_count = 0;
    int alpha_mode = 0;
    float3 offset;                   // mesh-local origin (terrain patches keep half-float positions small), zero for models
    bool has_offset = false;
    const Texture* tex[kTexSlots] = {};
    float3 bmin, bmax;               // bounds of the vertices (model space, before `offset`)
};

struct GpuLod {
    std::vector<GpuMesh> meshes;
    float distance = 0;              // used up to this camera distance
};

struct GpuModel {
    std::string path;
    std::vector<GpuMesh> meshes;     // LOD 0
    float distance = 0;              // LOD 0 is used up to this distance (0: always)
    std::vector<GpuLod> lods;        // coarser LODs 1.. (RMV2 files always have at least one LOD; terrain patches have several index buffers on one vertex buffer)
    float3 bmin, bmax;               // of LOD 0 meshes, with their offsets
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
    ~TextureCache();
    // Thread-safe; two threads asking for the same file wait for one load. nullptr if missing/unreadable.
    // While `streaming` is on, a large DDS file first loads its small mips only (the tail of the file: a fraction of the bytes and the time); the full
    // texture follows in the background and replaces the view at the next pump() (call it on the thread that draws, once per frame).
    const Texture* get(const std::string& path, bool srgb);
    void set_streaming(bool on) { streaming_ = on; }
    void pump();
    size_t upgrades_pending() const { return upgrades_inflight_.load(); }
    const Texture* white() const { return &white_; }
    const Texture* black() const { return &black_; }
    const Texture* flat_normal() const { return &flat_normal_; }
    std::vector<std::string> errors() const;
    size_t loaded() const { return loaded_.load(); }

private:
    struct Entry;
    ID3D11Device* dev_;
    IVfs& vfs_;
    mutable std::mutex mutex_;
    std::unordered_map<std::string, std::unique_ptr<Entry>> cache_;
    Texture white_, black_, flat_normal_;
    std::vector<std::string> errors_;
    std::atomic<size_t> loaded_{0};
    std::atomic<bool> streaming_{false};
    struct Upgrade {
        Texture* target;
        Texture full;
    };
    std::mutex upgrade_mutex_;
    std::vector<Upgrade> upgrades_;
    std::vector<ThreadPool::Handle> upgrade_jobs_;
    std::atomic<size_t> upgrades_inflight_{0};
    std::condition_variable upgrades_done_;
};

// One terrain entity on the GPU: patches of 64 x 64 cells, each a model of its own (own bounds, LODs that share the vertex buffer), so they are
// culled and detailed one by one.
struct TerrainGpu {
    std::vector<GpuModel> patches;
    std::vector<std::unique_ptr<Texture>> skins;   // the patches' pieces of the payload's skin (the meshes point at them)
    // what the patches were built from
    uint64_t layout = 0;
    uint64_t version = 0;
    int patches_x = 0, patches_z = 0;
    std::vector<float> heights;      // copy of the payload's heights, to find the patches a new payload changed
};

class ModelCache {
public:
    ModelCache(ID3D11Device* dev, IVfs& vfs, TextureCache& tex);
    ~ModelCache();

    enum class State { Absent, Queued, Loading, Ready, Failed };

    // Blocking: the model (loading it now, or waiting for the load in flight), or nullptr when the file has no drawable RS_STANDARD_V5 meshes
    // or cannot be read.
    const GpuModel* model(const std::string& path);
    // Not blocking: the model when it is ready; otherwise queues the load (smaller priority loads first) and returns nullptr.
    const GpuModel* try_get(const std::string& path, float priority = 0.0f);
    State state(const std::string& path) const;
    // The model if it is ready, without queueing anything (state is filled in when given; Absent counts as still to come).
    const GpuModel* peek(const std::string& path, State* state = nullptr) const;
    void set_priority(const std::string& path, float priority);   // while queued
    size_t pending() const { return pending_.load(); }            // queued + loading
    uint64_t revision() const { return revision_.load(); }        // changes whenever a load finishes
    void wait_all();                                              // blocks until nothing is pending

    // Terrain patches of a Terrain entity; rebuilt for the patches whose heights changed when the payload's version moves on.
    const TerrainGpu* terrain(EntityId id, const Terrain& t);
    // Drops terrain meshes of entities that no longer exist.
    void prune_terrain(const RenderScene& scene);

    std::vector<std::string> errors() const;

private:
    struct Entry {
        State state = State::Absent;
        std::unique_ptr<GpuModel> model;
        ThreadPool::Handle handle;
    };
    std::unique_ptr<GpuModel> build(const std::string& path);      // any thread; nullptr when unusable
    void run(const std::string& path, Entry* entry);               // load job
    void finish(Entry* entry, std::unique_ptr<GpuModel> model, const std::string& path);

    ID3D11Device* dev_;
    IVfs& vfs_;
    TextureCache& tex_;
    mutable std::mutex mutex_;
    std::condition_variable done_;
    std::unordered_map<std::string, std::unique_ptr<Entry>> models_;
    std::atomic<size_t> pending_{0};
    std::atomic<uint64_t> revision_{1};
    std::map<EntityId, std::unique_ptr<TerrainGpu>> terrain_;
    std::vector<std::string> errors_;
};

} // namespace wce
