#include "render/draw_set.h"

#include "foundation/profile.h"
#include "foundation/thread_pool.h"
#include "scene/render_scene.h"

#include <algorithm>
#include <cmath>
#include <cstring>
#include <limits>

namespace wce {

Frustum Frustum::from_view_projection(const float4x4& vp) {
    auto row = [&](int r, int s, int k) {   // s * rows[k] + rows[r] (or just rows[r] when k < 0)
        float v[4];
        for (int c = 0; c < 4; ++c) v[c] = vp.m[r][c] + (k >= 0 ? float(s) * vp.m[k][c] : 0.0f);
        return Plane{{v[0], v[1], v[2]}, v[3]};
    };
    Frustum f;
    f.p[0] = row(3, +1, 0);    // left:   w + x >= 0
    f.p[1] = row(3, -1, 0);    // right:  w - x >= 0
    f.p[2] = row(3, +1, 1);    // bottom: w + y >= 0
    f.p[3] = row(3, -1, 1);    // top:    w - y >= 0
    f.p[4] = row(2, 0, -1);    // near:   z >= 0 (D3D)
    f.p[5] = row(3, -1, 2);    // far:    w - z >= 0
    for (Plane& pl : f.p) {
        const float l = std::sqrt(pl.n[0] * pl.n[0] + pl.n[1] * pl.n[1] + pl.n[2] * pl.n[2]);
        if (l > 0) {
            pl.n[0] /= l;
            pl.n[1] /= l;
            pl.n[2] /= l;
            pl.d /= l;
        }
    }
    return f;
}

void DrawSet::clear() {
    sphere_.clear();
    model_.clear();
    flags_.clear();
    world_.clear();
    params_index_.clear();
    params_.clear();
    entity_.clear();
    models_.clear();
    model_of_.clear();
    batch_mesh_.clear();
}

void DrawSet::reserve(size_t n) {
    sphere_.reserve(n * 4);
    model_.reserve(n);
    flags_.reserve(n);
    world_.reserve(n);
    params_index_.reserve(n);
    entity_.reserve(n);
}

uint32_t DrawSet::model_index(const GpuModel* m) {
    auto it = model_of_.find(m);
    if (it != model_of_.end()) return it->second;
    ModelEntry me;
    me.bmin = m->bmin;
    me.bmax = m->bmax;
    me.centre = (m->bmin + m->bmax) * 0.5f;
    me.radius = length(m->bmax - m->bmin) * 0.5f;
    const float inf = std::numeric_limits<float>::infinity();
    auto add_lod = [&](const std::vector<GpuMesh>& meshes, float distance) {
        me.lod_first.push_back(uint32_t(me.lod_batch.size()));
        for (const GpuMesh& g : meshes) {
            me.lod_batch.push_back(uint32_t(batch_mesh_.size()));
            batch_mesh_.push_back(&g);
        }
        me.lod_distance.push_back(distance > 0 ? distance : inf);
    };
    add_lod(m->meshes, m->distance);
    for (const GpuLod& l : m->lods) add_lod(l.meshes, l.distance);
    me.lod_first.push_back(uint32_t(me.lod_batch.size()));
    me.lod_distance.back() = inf;   // the last LOD is used from there on
    const uint32_t idx = uint32_t(models_.size());
    models_.push_back(std::move(me));
    model_of_[m] = idx;
    return idx;
}

uint32_t DrawSet::add(const GpuModel* model, const float4x4& world, const float* params, bool casts_shadow, bool lod, EntityId entity) {
    const uint32_t mi = model_index(model);
    const ModelEntry& me = models_[mi];
    const uint32_t item = uint32_t(items());
    const float3 c = world.transform_point(me.centre);
    float s2 = 0;
    for (int col = 0; col < 3; ++col) s2 = std::max(s2, world.m[0][col] * world.m[0][col] + world.m[1][col] * world.m[1][col] + world.m[2][col] * world.m[2][col]);
    sphere_.insert(sphere_.end(), {c.x, c.y, c.z, me.radius * std::sqrt(s2)});
    model_.push_back(mi);
    uint8_t f = (casts_shadow ? kCastsShadow : 0) | (lod ? kLod : 0);
    Mat34 w;
    for (int r = 0; r < 3; ++r)
        for (int k = 0; k < 4; ++k) w.m[r][k] = world.m[r][k];
    world_.push_back(w);
    if (params) {
        bool any = false;
        for (int i = 0; i < 16; ++i) any = any || params[i] != kDefaultInstanceParams[i];
        if (any) {
            f |= kParams;
            params_index_.push_back(uint32_t(params_.size() / 16));
            params_.insert(params_.end(), params, params + 16);
        } else {
            params_index_.push_back(0);
        }
    } else {
        params_index_.push_back(0);
    }
    flags_.push_back(f);
    entity_.push_back(entity);
    return item;
}

void DrawSet::set_world(uint32_t item, const float4x4& world) {
    const ModelEntry& me = models_[model_[item]];
    const float3 c = world.transform_point(me.centre);
    float s2 = 0;
    for (int col = 0; col < 3; ++col) s2 = std::max(s2, world.m[0][col] * world.m[0][col] + world.m[1][col] * world.m[1][col] + world.m[2][col] * world.m[2][col]);
    sphere_[size_t(item) * 4 + 0] = c.x;
    sphere_[size_t(item) * 4 + 1] = c.y;
    sphere_[size_t(item) * 4 + 2] = c.z;
    sphere_[size_t(item) * 4 + 3] = me.radius * std::sqrt(s2);
    for (int r = 0; r < 3; ++r)
        for (int k = 0; k < 4; ++k) world_[item].m[r][k] = world.m[r][k];
}

bool DrawSet::item_box(uint32_t item, float4x4& world, float3& bmin, float3& bmax) const {
    if (item >= items()) return false;
    world = float4x4{};
    for (int r = 0; r < 3; ++r)
        for (int k = 0; k < 4; ++k) world.m[r][k] = world_[item].m[r][k];
    bmin = models_[model_[item]].bmin;
    bmax = models_[model_[item]].bmax;
    return true;
}

int DrawSet::lod_of(uint32_t item, const float3& eye, float lod_scale) const {
    const ModelEntry& me = models_[model_[item]];
    const float* s = &sphere_[size_t(item) * 4];
    const float dx = s[0] - eye.x, dy = s[1] - eye.y, dz = s[2] - eye.z;
    const float dist = std::sqrt(dx * dx + dy * dy + dz * dz) / lod_scale;
    int lod = 0;
    const int last = int(me.lod_distance.size()) - 1;
    while (lod < last && dist > me.lod_distance[lod]) ++lod;
    return lod;
}

void DrawSet::cull(const View& v, Visible& out) {
    WCE_PROF("drawset.cull");
    const size_t n = items(), nb = batches();
    constexpr size_t kGrain = 2048;
    const size_t chunks = (n + kGrain - 1) / kGrain;
    if (chunk_pairs_.size() < chunks) chunk_pairs_.resize(chunks);
    const float inv_scale = 1.0f / std::max(v.lod_scale, 1e-3f);
    compute_pool().parallel_for(n, kGrain, [&](size_t b, size_t e, int) {
        std::vector<uint64_t>& pairs = chunk_pairs_[b / kGrain];
        pairs.clear();
        for (size_t i = b; i < e; ++i) {
            const uint8_t f = flags_[i];
            if (v.shadow && !(f & kCastsShadow)) continue;
            const float* s = &sphere_[i * 4];
            if (v.frustum.outside(s, s[3])) continue;
            const ModelEntry& me = models_[model_[i]];
            int lod = 0;
            if (f & kLod) {
                const float dx = s[0] - v.eye.x, dy = s[1] - v.eye.y, dz = s[2] - v.eye.z;
                const float dist = std::sqrt(dx * dx + dy * dy + dz * dz);
                if (s[3] < dist * v.min_ratio || s[3] < v.min_radius) continue;
                const float d = dist * inv_scale;
                const int last = int(me.lod_distance.size()) - 1;
                while (lod < last && d > me.lod_distance[lod]) ++lod;
                lod = std::min(lod + v.coarser, last);
            }
            for (uint32_t k = me.lod_first[lod]; k < me.lod_first[lod + 1]; ++k) pairs.push_back((uint64_t(me.lod_batch[k]) << 32) | uint32_t(i));
        }
    });
    // counting sort by batch, chunks in order: the same culled set always gives the same draw order
    out.count.assign(nb, 0);
    size_t total = 0;
    for (size_t c = 0; c < chunks; ++c) {
        for (uint64_t p : chunk_pairs_[c]) ++out.count[uint32_t(p >> 32)];
        total += chunk_pairs_[c].size();
    }
    out.start.assign(nb, 0);
    out.active.clear();
    uint32_t at = 0;
    for (size_t b = 0; b < nb; ++b) {
        out.start[b] = at;
        at += out.count[b];
        if (out.count[b]) out.active.push_back(uint32_t(b));
    }
    cursor_.assign(out.start.begin(), out.start.end());
    out.order.resize(total);
    out.batch_of.resize(total);
    for (size_t c = 0; c < chunks; ++c)
        for (uint64_t p : chunk_pairs_[c]) {
            const uint32_t b = uint32_t(p >> 32), pos = cursor_[b]++;
            out.order[pos] = uint32_t(p);
            out.batch_of[pos] = b;
        }
}

void DrawSet::gather(const Visible& vis, RigidInstance* dst) const {
    WCE_PROF("drawset.gather");
    compute_pool().parallel_for(vis.order.size(), 4096, [&](size_t b, size_t e, int) {
        for (size_t k = b; k < e; ++k) {
            const uint32_t item = vis.order[k];
            RigidInstance& r = dst[k];
            const Mat34& w = world_[item];
            std::memcpy(r.world, w.m, sizeof r.world);
            const GpuMesh* mesh = batch_mesh_[vis.batch_of[k]];
            if (mesh->has_offset) {   // translation += rotation * offset
                for (int row = 0; row < 3; ++row)
                    r.world[row][3] += w.m[row][0] * mesh->offset.x + w.m[row][1] * mesh->offset.y + w.m[row][2] * mesh->offset.z;
            }
            if (flags_[item] & kParams) std::memcpy(r.p4, &params_[size_t(params_index_[item]) * 16], 64);
            else std::memcpy(r.p4, kDefaultInstanceParams, 64);
        }
    });
}

void DrawSet::gather_ids(const Visible& vis, uint32_t* dst) const {
    for (size_t k = 0; k < vis.order.size(); ++k) dst[k] = entity_[vis.order[k]];
}

} // namespace wce
