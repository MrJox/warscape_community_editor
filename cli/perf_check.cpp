// Gate for the performance machinery (no window, no GPU): the thread pool, the frustum / LOD / size culling of the draw set, the DDS mip stream plan and the
// tile document's element index against the plain per-element transform walk.
//   perf_check <tile folder> [<tile folder> ...]
#include "foundation/profile.h"
#include "foundation/thread_pool.h"
#include "render/draw_set.h"
#include "render/texture.h"
#include "tile/tile_document.h"
#include "tile/world_transform.h"

#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <numeric>
#include <random>
#include <vector>

using namespace wce;

static int g_failed = 0;
#define CHECK(cond, what) \
    do { \
        if (!(cond)) { \
            std::printf("FAIL: %s\n", what); \
            ++g_failed; \
        } \
    } while (0)

static void thread_pool_checks() {
    ThreadPool pool(4);
    std::vector<int> v(100000, 1);
    std::atomic<long long> sum{0};
    pool.parallel_for(v.size(), 1000, [&](size_t b, size_t e, int worker) {
        CHECK(worker >= 0 && worker < pool.slots(), "worker index in range");
        long long s = 0;
        for (size_t i = b; i < e; ++i) s += v[i];
        sum += s;
    });
    CHECK(sum == 100000, "parallel_for covers every element exactly once");
    // nested: the inner loops run even when every worker is busy with the outer one
    std::atomic<int> inner{0};
    pool.parallel_for(16, 1, [&](size_t b, size_t e, int) {
        for (size_t i = b; i < e; ++i) pool.parallel_for(8, 1, [&](size_t ib, size_t ie, int) { inner += int(ie - ib); });
    });
    CHECK(inner == 16 * 8, "nested parallel_for completes");
    // jobs run in priority order (one thread: the order is exact), a cancelled job never runs
    ThreadPool one(1);
    std::mutex m;
    std::vector<int> order;
    std::atomic<bool> gate{false};
    one.post([&] { while (!gate) std::this_thread::yield(); }, -1.0f);   // keeps the single worker busy while the others are queued
    std::this_thread::sleep_for(std::chrono::milliseconds(20));
    auto note = [&](int id) { return [&, id] { std::lock_guard<std::mutex> l(m); order.push_back(id); }; };
    one.post(note(3), 3.0f);
    const auto h1 = one.post(note(1), 1.0f);
    one.post(note(2), 2.0f);
    const auto h9 = one.post(note(9), 9.0f);
    one.set_priority(h1, 5.0f);   // 1 is now the last of the kept ones
    CHECK(one.cancel(h9), "a queued job can be cancelled");
    gate = true;
    one.wait_idle();
    CHECK(order == (std::vector<int>{2, 3, 1}), "jobs run by priority, a changed priority counts, a cancelled one is gone");
}

static GpuModel make_model(int lods, float3 lo, float3 hi) {
    GpuModel m;
    m.bmin = lo;
    m.bmax = hi;
    m.meshes.resize(1);
    m.distance = 100;
    for (int l = 1; l < lods; ++l) {
        GpuLod lod;
        lod.meshes.resize(1);
        lod.distance = 100.0f * float(1 << l);
        m.lods.push_back(std::move(lod));
    }
    return m;
}

static void draw_set_checks() {
    // frustum planes: a point inside a perspective frustum is inside, one behind the camera or far off to the side is outside
    const float4x4 view = look_at_lh({0, 10, -50}, {0, 0, 0}, {0, 1, 0});
    const float4x4 proj = perspective_lh(1.0f, 16.0f / 9.0f, 1.0f, 500.0f);
    const Frustum f = Frustum::from_view_projection(proj * view);
    const float in[3] = {0, 5, 0}, behind[3] = {0, 10, -80}, side[3] = {400, 0, 0}, far_away[3] = {0, 0, 600};
    CHECK(!f.outside(in, 1), "a point in front of the camera is inside");
    CHECK(f.outside(behind, 1), "a point behind the camera is outside");
    CHECK(f.outside(side, 1), "a point far to the side is outside");
    CHECK(f.outside(far_away, 1), "a point beyond the far plane is outside");
    const float edge[3] = {0, 0, 600};
    CHECK(!f.outside(edge, 200), "a big sphere that reaches into the frustum is kept");

    // 4 LOD levels; items on a line away from the eye
    const GpuModel model = make_model(4, {-1, 0, -1}, {1, 2, 1});
    DrawSet set;
    const float3 eye{0, 10, -50};
    auto at = [&](float z) { return translation({0, 0, z}); };
    set.add(&model, at(-40), nullptr, true, true, 1);     // distance ~10
    set.add(&model, at(100), nullptr, true, true, 2);     // ~150: LOD 1
    set.add(&model, at(300), nullptr, true, true, 3);     // ~350: LOD 2
    set.add(&model, at(900), nullptr, true, true, 4);     // beyond the far plane
    set.add(&model, at(-40), nullptr, true, false, 5);    // not a LOD item: always LOD 0
    CHECK(set.items() == 5 && set.models() == 1 && set.batches() == 4, "one model entry, one batch per LOD");
    CHECK(set.lod_of(0, eye) == 0 && set.lod_of(1, eye) == 1 && set.lod_of(2, eye) == 2, "LOD by distance");
    DrawSet::View v;
    v.frustum = f;
    v.eye = eye;
    DrawSet::Visible vis;
    set.cull(v, vis);
    CHECK(vis.instances() == 4, "four items are inside the frustum");
    CHECK(vis.count[0] == 2 && vis.count[1] == 1 && vis.count[2] == 1, "instances grouped by the LOD batch they use");
    // the same culled set always gives the same order
    DrawSet::Visible again;
    set.cull(v, again);
    CHECK(again.order == vis.order, "culling is deterministic");
    // too small to see: skipped for LOD items only
    v.min_ratio = 0.05f;   // radius / distance below 5 %
    set.cull(v, vis);
    CHECK(vis.instances() == 2, "small distant LOD items are skipped, the fixed one stays");
    v.min_ratio = 0;
    v.shadow = true;
    set.cull(v, vis);
    CHECK(vis.instances() == 4, "shadow views keep casters");
    // gather writes the world matrix of every instance
    std::vector<RigidInstance> inst(vis.instances());
    set.gather(vis, inst.data());
    CHECK(inst.size() == 4 && std::fabs(inst[0].world[2][3] - (-40.0f)) < 1e-4f, "gather writes the world matrix of the first instance");
}

static void dds_checks() {
    // a DXT1 1024 x 1024 texture with a full mip chain: header + 11 mips
    std::vector<uint8_t> hdr(148, 0);
    auto put = [&](size_t off, uint32_t v) { std::memcpy(&hdr[off], &v, 4); };
    put(0, 0x20534444);   // "DDS "
    put(4, 124);
    put(12, 1024);        // height
    put(16, 1024);        // width
    put(28, 11);          // mips
    put(76, 32);
    put(80, 0x4);         // fourcc
    put(84, 0x31545844);  // DXT1
    size_t total = 128;
    for (int w = 1024, m = 0; m < 11; ++m, w = std::max(1, w / 2)) total += size_t(std::max(1, w / 4)) * size_t(std::max(1, w / 4)) * 8;
    const DdsStreamPlan p = plan_dds_stream(hdr, total, 256);
    CHECK(p.ok && p.skip == 2, "a 1024 texture streams from the 256 mip (skips 2)");
    // the tail starts after the two large mips: 1024^2/2 + 512^2/2 bytes
    CHECK(p.tail_offset == 128 + 1024 * 1024 / 2 + 512 * 512 / 2, "the tail starts at the right file offset");
    const DdsStreamPlan fits = plan_dds_stream(hdr, total, 2048);
    CHECK(!fits.ok, "a texture that already fits is loaded whole");
    put(112, 0x200);      // caps2: cube map
    CHECK(!plan_dds_stream(hdr, total, 256).ok, "cube maps are loaded whole");
}

static void tile_index_checks(const std::filesystem::path& dir) {
    std::unique_ptr<TileProject> project = TileProject::open(dir);
    TileDocument doc(std::move(project), nullptr);
    const TedScene& sc = doc.tscene();
    size_t checked = 0, bad = 0;
    for (size_t i = 0; i < sc.doc().elements().size(); ++i) {
        const int e = int(i);
        // the plain rules the index replaces
        const std::string& t = sc.doc().elements()[i].name;
        const bool plain = t == "point" || t == "points" || t == "children" || t == "camera" || sc.is_layer(e);
        const bool instance = !plain && sc.position(e).has_value();
        if (doc.is_instance(e) != instance) { ++bad; continue; }
        if (!instance) continue;
        ++checked;
        const WorldTransform w = world_transform(sc, e);
        const float3 p = doc.world_position(e);
        const double u = doc.unit();
        if (std::fabs(p.x - float(w.position[0] / u)) > 1e-3f || std::fabs(p.y - float(w.position[1])) > 1e-3f || std::fabs(p.z - float(w.position[2] / u)) > 1e-3f) ++bad;
        const float4x4 m = doc.world_matrix(e);
        const std::optional<Vec3> s = sc.scale(e);
        for (int r = 0; r < 3; ++r)
            for (int c = 0; c < 3; ++c)
                if (std::fabs(m.m[r][c] - float(w.rotation[size_t(r * 3 + c)] * (s ? (*s)[size_t(c)] : 1.0))) > 1e-4f) ++bad;
    }
    std::printf("%s: %zu instances checked against the transform walk, %zu mismatches\n", dir.filename().string().c_str(), checked, bad);
    CHECK(bad == 0, "the element index agrees with the transform walk");
    CHECK(checked == doc.instances().size(), "the index lists every instance");
    // ray picking through the grid finds the nearest instance sphere that a brute-force test finds
    std::mt19937 rng(7);
    int tested = 0, mismatches = 0;
    for (int k = 0; k < 200 && !doc.instances().empty(); ++k) {
        const int target = doc.instances()[rng() % doc.instances().size()];
        const float3 c = doc.world_position(target);
        const float3 origin{c.x + float(int(rng() % 200) - 100), c.y + 60.0f + float(rng() % 100), c.z + float(int(rng() % 200) - 100)};
        const float3 dir = normalize(float3{c.x, c.y + doc.marker_radius(target) * 0.3f, c.z} - origin);
        int best = -1;
        float best_t = 1e30f;
        for (int e : doc.instances()) {
            if (!doc.shown(e) || !doc.selectable(e)) continue;   // the grid only holds what can be picked (hidden / locked layers are left out)
            float3 q = doc.world_position(e);
            const float r = doc.marker_radius(e);
            if (sc.type(e) == "BUILDING_INSTANCE") q.y += r * 0.5f;
            const float3 oc = origin - q;
            const float b = dot(oc, dir), cc = dot(oc, oc) - r * r, disc = b * b - cc;
            if (disc < 0) continue;
            const float t = -b - std::sqrt(disc);
            if (t > 0 && t < best_t) { best_t = t; best = e; }
        }
        doc.display().lod = false;   // the brute force knows no draw distances
        const int hit = doc.pick_instance(origin, dir, false);
        ++tested;
        if (hit != best) ++mismatches;
    }
    std::printf("%s: %d pick rays, %d differ from the brute-force test\n", dir.filename().string().c_str(), tested, mismatches);
    CHECK(mismatches == 0, "the pick grid finds the same instance as testing every instance");
}

int main(int argc, char** argv) {
    thread_pool_checks();
    draw_set_checks();
    dds_checks();
    for (int i = 1; i < argc; ++i) tile_index_checks(argv[i]);
    std::printf("%s\n", g_failed ? "FAILED" : "ok");
    return g_failed ? 1 : 0;
}
