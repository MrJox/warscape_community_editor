#pragma once
// What the renderer draws, laid out for the CPU cache and for the GPU's instancing: every mesh / terrain-patch entity is one *item* in parallel arrays
// (bounding sphere, world matrix, model, flags), each distinct mesh at each LOD is a *batch*. A view (the camera, a shadow cascade, an environment
// cube face) culls the items in parallel (frustum, screen size, LOD by distance) and sorts the survivors by batch; the renderer then writes one
// instance buffer per view and issues one DrawIndexedInstanced per visible batch instead of one draw per mesh per entity.
#include "foundation/math.h"
#include "render/model_cache.h"

#include <cstdint>
#include <unordered_map>
#include <vector>

namespace wce {

struct Plane {
    float n[3];
    float d;     // inside: dot(n, p) + d >= 0
};

struct Frustum {
    Plane p[6];
    // Planes of a D3D clip volume (x, y in -w..w, z in 0..w) from a view-projection matrix in this code base's convention (clip = M * p).
    static Frustum from_view_projection(const float4x4& vp);
    bool outside(const float c[3], float r) const {
        for (const Plane& pl : p)
            if (pl.n[0] * c[0] + pl.n[1] * c[1] + pl.n[2] * c[2] + pl.d < -r) return true;
        return false;
    }
};

class DrawSet {
public:
    struct Mat34 {
        float m[3][4];
    };

    // ---- building (once per scene change)
    void clear();
    void reserve(size_t items);
    // Returns the item index. `params` = 16 floats (POSITION4..7) or nullptr for the engine's defaults.
    uint32_t add(const GpuModel* model, const float4x4& world, const float* params, bool casts_shadow, bool lod, EntityId entity);
    void set_world(uint32_t item, const float4x4& world);
    size_t items() const { return model_.size(); }
    size_t batches() const { return batch_mesh_.size(); }
    size_t models() const { return models_.size(); }
    EntityId entity(uint32_t item) const { return entity_[item]; }
    const GpuMesh& batch_mesh(uint32_t batch) const { return *batch_mesh_[batch]; }

    // ---- one view
    struct View {
        Frustum frustum;
        float3 eye;
        bool shadow = false;            // only items that cast shadows
        float lod_scale = 1.0f;         // multiplies the LOD distances (below 1: coarser sooner)
        int coarser = 0;                // added to the LOD index (shadow maps need less detail)
        float min_ratio = 0.0f;         // LOD items with radius / distance below this are skipped (too small to see)
        float min_radius = 0.0f;        // LOD items with a smaller radius are skipped (shadow texels)
    };
    struct Visible {
        std::vector<uint32_t> start, count;   // per batch: first instance, number of instances
        std::vector<uint32_t> active;         // batches with count > 0, ascending
        std::vector<uint32_t> order;          // item index of every instance, grouped by batch
        std::vector<uint32_t> batch_of;       // batch of every instance
        size_t instances() const { return order.size(); }
    };
    void cull(const View& view, Visible& out);
    // Fills `instances()` records: the item's world matrix (with the mesh offset applied) and its pixel-shader parameters.
    void gather(const Visible& vis, RigidInstance* dst) const;
    void gather_ids(const Visible& vis, uint32_t* dst) const;     // entity id of every instance (the id pass)
    // Drops the instances whose entity `keep` rejects (the id pass skips locked layers), keeping the grouping by batch.
    template <class F>
    void retain(Visible& vis, F&& keep) const {
        size_t out = 0;
        for (uint32_t b : vis.active) {
            const uint32_t first = vis.start[b], n = vis.count[b];
            uint32_t kept = 0;
            vis.start[b] = uint32_t(out);
            for (uint32_t i = first; i < first + n; ++i)
                if (keep(entity_[vis.order[i]])) {
                    vis.order[out] = vis.order[i];
                    vis.batch_of[out] = vis.batch_of[i];
                    ++out;
                    ++kept;
                }
            vis.count[b] = kept;
        }
        vis.order.resize(out);
        vis.batch_of.resize(out);
        size_t w = 0;
        for (uint32_t b : vis.active)
            if (vis.count[b]) vis.active[w++] = b;
        vis.active.resize(w);
    }

    // World matrix and model-space bounds of an item (the selection outline).
    bool item_box(uint32_t item, float4x4& world, float3& bmin, float3& bmax) const;
    // Chosen LOD of an item for an eye position (tests, statistics).
    int lod_of(uint32_t item, const float3& eye, float lod_scale = 1.0f) const;

private:
    enum Flag : uint8_t { kCastsShadow = 1, kLod = 2, kParams = 4 };
    struct ModelEntry {
        float3 centre, bmin, bmax;
        float radius = 0;
        std::vector<float> lod_distance;       // per LOD, +inf for the last
        std::vector<uint32_t> lod_first;       // lod_first[l] .. lod_first[l + 1] index lod_batch
        std::vector<uint32_t> lod_batch;       // batch ids
    };
    uint32_t model_index(const GpuModel* m);

    // items (structure of arrays)
    std::vector<float> sphere_;                // x, y, z, r per item (4 floats)
    std::vector<uint32_t> model_;
    std::vector<uint8_t> flags_;
    std::vector<Mat34> world_;
    std::vector<uint32_t> params_index_;       // into params_, valid with kParams
    std::vector<float> params_;                // 16 floats per entry
    std::vector<EntityId> entity_;
    // models and batches
    std::vector<ModelEntry> models_;
    std::unordered_map<const GpuModel*, uint32_t> model_of_;
    std::vector<const GpuMesh*> batch_mesh_;
    // scratch of cull()
    std::vector<std::vector<uint64_t>> chunk_pairs_;
    std::vector<uint32_t> cursor_;
};

} // namespace wce
