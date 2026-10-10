#include "render/model_cache.h"

#include "foundation/process.h"
#include "foundation/profile.h"
#include "foundation/text_encoding.h"
#include "formats/rmv2.h"
#include "vfs/vfs.h"

#include <algorithm>
#include <cmath>
#include <cstring>

namespace wce {

namespace {

const char* const kTexNames[kTexSlots] = {"s_diffuse_map", "s_specular_colour_map", "s_gloss_map", "s_colour_mask_map", "s_normal_map"};

float half_to_float(uint16_t h) {
    const uint32_t s = (h >> 15) & 1, e = (h >> 10) & 31, m = h & 1023;
    uint32_t f;
    if (e == 0) {
        if (m == 0) f = s << 31;
        else return (s ? -1.0f : 1.0f) * float(m) * (1.0f / 16777216.0f);   // denormal
    } else if (e == 31) {
        f = (s << 31) | 0x7f800000u | (m << 13);
    } else {
        f = (s << 31) | ((e + 112) << 23) | (m << 13);
    }
    float r;
    std::memcpy(&r, &f, 4);
    return r;
}

uint16_t to_half(float f) {
    uint32_t x;
    std::memcpy(&x, &f, 4);
    uint32_t sign = (x >> 16) & 0x8000, exp = (x >> 23) & 0xff, man = x & 0x7fffff;
    if (exp == 0) return uint16_t(sign);
    int e = int(exp) - 127 + 15;
    if (e >= 31) return uint16_t(sign | 0x7c00);
    if (e <= 0) return uint16_t(sign);
    return uint16_t(sign | (e << 10) | (man >> 13));
}

// STANDARD_RIGID_MESH_VERTEX: half4 pos, half4 uv, BGRA8 normal/tangent/bitangent, BGRA8 colour.
struct Vertex0 {
    uint16_t pos[4], uv[4];
    uint8_t normal[4], tangent[4], bitangent[4], colour[4];
};
static_assert(sizeof(Vertex0) == 32);

void pack_vec(uint8_t out[4], float x, float y, float z) { // memory order B, G, R, A -> shader x = R, y = G, z = B
    auto q = [](float v) { return uint8_t(std::lround((v * 0.5f + 0.5f) * 255.0f)); };
    out[0] = q(z);
    out[1] = q(y);
    out[2] = q(x);
    out[3] = 255;
}

} // namespace

const char* gpu_texture_name(int slot) { return slot >= 0 && slot < kTexSlots ? kTexNames[slot] : ""; }

int gpu_texture_slot(const std::string& name) {
    for (int i = 0; i < kTexSlots; ++i)
        if (name == kTexNames[i]) return i;
    return -1;
}

// ------------------------------------------------------------------------------------------------ textures

struct TextureCache::Entry {
    std::once_flag once;
    std::unique_ptr<Texture> tex;
};

TextureCache::TextureCache(ID3D11Device* dev, IVfs& vfs) : dev_(dev), vfs_(vfs) {
    white_ = make_solid_texture(dev, 0xffffffff);
    black_ = make_solid_texture(dev, 0x00000000);
    flat_normal_ = make_solid_texture(dev, 0xff80ff80); // DXT5nm-style: x in .a, y in .g
}

TextureCache::~TextureCache() {
    std::unique_lock<std::mutex> lock(upgrade_mutex_);
    for (ThreadPool::Handle h : upgrade_jobs_)
        if (io_pool().cancel(h)) upgrades_inflight_.fetch_sub(1);
    upgrades_done_.wait(lock, [&] { return upgrades_inflight_.load() == 0; });
}

void TextureCache::pump() {
    std::vector<Upgrade> ready;
    {
        std::lock_guard<std::mutex> lock(upgrade_mutex_);
        ready.swap(upgrades_);
    }
    for (Upgrade& u : ready) {
        u.target->srv = std::move(u.full.srv);
        u.target->width = u.full.width;
        u.target->height = u.full.height;
        u.target->mips = u.full.mips;
    }
}

const Texture* TextureCache::get(const std::string& path, bool srgb) {
    std::string key = Vfs::normalize(path);
    key += srgb ? "|s" : "|l";
    Entry* e;
    {
        std::lock_guard<std::mutex> lock(mutex_);
        auto& slot = cache_[key];
        if (!slot) slot = std::make_unique<Entry>();
        e = slot.get();
    }
    std::call_once(e->once, [&] {
        WCE_PROF("texture.load");
        try {
            bool done = false;
            if (streaming_) {   // stage one: only the small mips, read from the tail of the file
                WCE_PROF("texture.stream_first");
                if (auto disk = vfs_.fetch(path)) {
                    const std::string file = narrow(disk->wstring());
                    const uint64_t size = file_size_of(file);
                    std::vector<uint8_t> header = read_file_range(file, 0, 148);
                    const DdsStreamPlan plan = plan_dds_stream(header, size_t(size), 256);
                    if (plan.ok) {
                        const std::vector<uint8_t> tail = read_file_range(file, plan.tail_offset, 0);
                        e->tex = std::make_unique<Texture>(load_dds_tail(dev_, header, tail, plan, srgb));
                        loaded_.fetch_add(1);
                        done = true;
                        Texture* target = e->tex.get();
                        upgrades_inflight_.fetch_add(1);
                        const ThreadPool::Handle h = io_pool().post(
                            [this, target, path, srgb] {
                                try {
                                    WCE_PROF("texture.upgrade");
                                    Texture full = load_dds(dev_, vfs_.read(path), srgb);
                                    std::lock_guard<std::mutex> lock(upgrade_mutex_);
                                    upgrades_.push_back({target, std::move(full)});
                                } catch (const std::exception& ex) {
                                    std::lock_guard<std::mutex> lock(mutex_);
                                    if (errors_.size() < 50) errors_.push_back(path + ": " + ex.what());
                                }
                                std::lock_guard<std::mutex> lock(upgrade_mutex_);
                                upgrades_inflight_.fetch_sub(1);
                                upgrades_done_.notify_all();
                            },
                            ThreadPool::current_priority() + 1.0e6f);   // after every model load that is queued
                        std::lock_guard<std::mutex> lock(upgrade_mutex_);
                        upgrade_jobs_.push_back(h);
                    }
                }
            }
            if (!done) {
                std::vector<uint8_t> bytes;
                {
                    WCE_PROF("texture.read");
                    bytes = vfs_.read(path);
                }
                WCE_PROF("texture.create");
                e->tex = std::make_unique<Texture>(load_dds(dev_, bytes, srgb));
                loaded_.fetch_add(1);
            }
        } catch (const std::exception& ex) {
            std::lock_guard<std::mutex> lock(mutex_);
            if (errors_.size() < 50) errors_.push_back(path + ": " + ex.what());
        }
    });
    return e->tex.get();
}

std::vector<std::string> TextureCache::errors() const {
    std::lock_guard<std::mutex> lock(mutex_);
    return errors_;
}

// ------------------------------------------------------------------------------------------------ models

ModelCache::ModelCache(ID3D11Device* dev, IVfs& vfs, TextureCache& tex) : dev_(dev), vfs_(vfs), tex_(tex) {}

ModelCache::~ModelCache() {
    std::unique_lock<std::mutex> lock(mutex_);
    for (auto& [path, e] : models_)
        if (e->state == State::Queued) {
            io_pool().cancel(e->handle);
            e->state = State::Failed;
            pending_.fetch_sub(1);
        }
    done_.wait(lock, [&] { return pending_.load() == 0; });
}

std::unique_ptr<GpuModel> ModelCache::build(const std::string& path) {
    WCE_PROF("model.build");
    auto gm = std::make_unique<GpuModel>();
    gm->path = path;
    std::vector<uint8_t> bytes;
    {
        WCE_PROF("model.read");
        bytes = vfs_.read(path);
    }
    Rmv2Model m;
    {
        WCE_PROF("model.parse");
        m = parse_rmv2(bytes, -1, false);
    }
    if (m.lods.empty()) throw std::runtime_error("no LODs");

    auto make_mesh = [&](const Rmv2Mesh& me, GpuMesh& g) -> bool {
        if (me.shader != RS_STANDARD_V5 || me.vertex_format != 0 || !me.has_geometry() || me.vertex_stride != sizeof(Vertex0)) return false;
        D3D11_BUFFER_DESC bd{};
        bd.Usage = D3D11_USAGE_IMMUTABLE;
        bd.BindFlags = D3D11_BIND_VERTEX_BUFFER;
        bd.ByteWidth = UINT(size_t(me.vertex_count) * me.vertex_stride);
        D3D11_SUBRESOURCE_DATA sd{me.vertex_data};
        if (FAILED(dev_->CreateBuffer(&bd, &sd, &g.vb))) return false;
        bd.BindFlags = D3D11_BIND_INDEX_BUFFER;
        bd.ByteWidth = UINT(size_t(me.index_count) * 2);
        sd.pSysMem = me.index_data;
        if (FAILED(dev_->CreateBuffer(&bd, &sd, &g.ib))) return false;
        g.index_count = me.index_count;
        g.alpha_mode = me.alpha_mode();
        auto tex = [&](int param, bool srgb, const Texture* fallback) {
            const std::string* p = me.texture(param);
            const Texture* t = p ? tex_.get(*p, srgb) : nullptr;
            return t ? t : fallback;
        };
        g.tex[kTexDiffuse] = tex(TP_DIFFUSE, true, tex_.white());
        g.tex[kTexSpecular] = tex(TP_SPECULAR, true, tex_.black());
        g.tex[kTexGloss] = tex(TP_GLOSS, false, tex_.black());
        g.tex[kTexColourMask] = tex(TP_FACTION_MASK, false, tex_.black());
        g.tex[kTexNormal] = tex(TP_NORMAL, false, tex_.flat_normal());
        // bounds from the vertices (the file's aabb is not trusted: a culling sphere that is too small drops visible geometry)
        float lo[3] = {1e30f, 1e30f, 1e30f}, hi[3] = {-1e30f, -1e30f, -1e30f};
        for (uint32_t v = 0; v < me.vertex_count; ++v) {
            uint16_t h[3];
            std::memcpy(h, me.vertex_data + size_t(v) * me.vertex_stride, sizeof h);
            for (int a = 0; a < 3; ++a) {
                const float f = half_to_float(h[a]);
                lo[a] = std::min(lo[a], f);
                hi[a] = std::max(hi[a], f);
            }
        }
        g.bmin = {lo[0], lo[1], lo[2]};
        g.bmax = {hi[0], hi[1], hi[2]};
        return true;
    };

    gm->bmin = {1e30f, 1e30f, 1e30f};
    gm->bmax = {-1e30f, -1e30f, -1e30f};
    for (size_t l = 0; l < m.lods.size(); ++l) {
        std::vector<GpuMesh> meshes;
        for (const Rmv2Mesh& me : m.lods[l].meshes) {
            GpuMesh g;
            if (make_mesh(me, g)) meshes.push_back(std::move(g));
        }
        if (meshes.empty()) {
            if (l == 0) return nullptr;
            continue;
        }
        if (l == 0) {
            for (const GpuMesh& g : meshes) {
                gm->bmin = {std::min(gm->bmin.x, g.bmin.x), std::min(gm->bmin.y, g.bmin.y), std::min(gm->bmin.z, g.bmin.z)};
                gm->bmax = {std::max(gm->bmax.x, g.bmax.x), std::max(gm->bmax.y, g.bmax.y), std::max(gm->bmax.z, g.bmax.z)};
            }
            gm->meshes = std::move(meshes);
            gm->distance = m.lods[0].distance;
        } else {
            GpuLod lod;
            lod.meshes = std::move(meshes);
            lod.distance = m.lods[l].distance;
            gm->lods.push_back(std::move(lod));
        }
    }
    return gm;
}

void ModelCache::finish(Entry* entry, std::unique_ptr<GpuModel> model, const std::string& path) {
    (void)path;
    std::lock_guard<std::mutex> lock(mutex_);
    entry->model = std::move(model);
    entry->state = entry->model ? State::Ready : State::Failed;
    pending_.fetch_sub(1);
    revision_.fetch_add(1);
    done_.notify_all();
}

void ModelCache::run(const std::string& path, Entry* entry) {
    {
        std::lock_guard<std::mutex> lock(mutex_);
        if (entry->state != State::Queued) return;   // a blocking model() call took it
        entry->state = State::Loading;
    }
    std::unique_ptr<GpuModel> gm;
    try {
        gm = build(path);
    } catch (const std::exception& e) {
        std::lock_guard<std::mutex> lock(mutex_);
        errors_.push_back(path + ": " + e.what());
    }
    finish(entry, std::move(gm), path);
}

const GpuModel* ModelCache::model(const std::string& path) {
    std::unique_lock<std::mutex> lock(mutex_);
    auto& slot = models_[path];
    if (!slot) slot = std::make_unique<Entry>();
    Entry* e = slot.get();
    for (;;) {
        switch (e->state) {
        case State::Ready: return e->model.get();
        case State::Failed: return nullptr;
        case State::Loading: done_.wait(lock); continue;
        case State::Absent: pending_.fetch_add(1); [[fallthrough]];
        case State::Queued: {
            if (e->state == State::Queued) io_pool().cancel(e->handle);
            e->state = State::Loading;
            lock.unlock();
            std::unique_ptr<GpuModel> gm;
            try {
                gm = build(path);
            } catch (const std::exception& ex) {
                std::lock_guard<std::mutex> g(mutex_);
                errors_.push_back(path + ": " + ex.what());
            }
            finish(e, std::move(gm), path);
            lock.lock();
            continue;
        }
        }
    }
}

const GpuModel* ModelCache::try_get(const std::string& path, float priority) {
    std::lock_guard<std::mutex> lock(mutex_);
    auto& slot = models_[path];
    if (!slot) slot = std::make_unique<Entry>();
    Entry* e = slot.get();
    switch (e->state) {
    case State::Ready: return e->model.get();
    case State::Absent:
        e->state = State::Queued;
        pending_.fetch_add(1);
        e->handle = io_pool().post([this, path, e] { run(path, e); }, priority);
        return nullptr;
    case State::Queued: io_pool().set_priority(e->handle, priority); return nullptr;
    default: return nullptr;
    }
}

ModelCache::State ModelCache::state(const std::string& path) const {
    std::lock_guard<std::mutex> lock(mutex_);
    auto it = models_.find(path);
    return it == models_.end() ? State::Absent : it->second->state;
}

const GpuModel* ModelCache::peek(const std::string& path, State* state) const {
    std::lock_guard<std::mutex> lock(mutex_);
    auto it = models_.find(path);
    const State s = it == models_.end() ? State::Absent : it->second->state;
    if (state) *state = s;
    return s == State::Ready ? it->second->model.get() : nullptr;
}

void ModelCache::set_priority(const std::string& path, float priority) {
    std::lock_guard<std::mutex> lock(mutex_);
    auto it = models_.find(path);
    if (it != models_.end() && it->second->state == State::Queued) io_pool().set_priority(it->second->handle, priority);
}

void ModelCache::wait_all() {
    std::unique_lock<std::mutex> lock(mutex_);
    done_.wait(lock, [&] { return pending_.load() == 0; });
}

std::vector<std::string> ModelCache::errors() const {
    std::lock_guard<std::mutex> lock(mutex_);
    return errors_;
}

// ------------------------------------------------------------------------------------------------ terrain

namespace {

constexpr int kCells = 64;   // patch size in cells: local half-float positions and UVs stay small

uint64_t mix64(uint64_t h, uint64_t v) { return h ^ (v + 0x9e3779b97f4a7c15ull + (h << 6) + (h >> 2)); }

uint64_t layout_hash(const Terrain& t) {
    uint64_t h = std::hash<std::string>{}(t.diffuse);
    h = mix64(h, uint64_t(t.nx) * 65537u + uint64_t(t.nz));
    for (float f : {t.origin_x, t.origin_z, t.step, t.tile_metres}) {
        uint32_t bits;
        std::memcpy(&bits, &f, 4);
        h = mix64(h, bits);
    }
    h = mix64(h, t.heights.size());
    h = mix64(h, t.lod ? 1 : 0);
    h = mix64(h, t.skin ? t.skin->id + 1 : 0);
    return h;
}

std::vector<int> axis_positions(int cells, int stride) {
    std::vector<int> p;
    for (int k = 0; k < cells; k += stride) p.push_back(k);
    p.push_back(cells);
    return p;
}

// One patch: a vertex buffer (cells + 1)^2 vertices plus a skirt around it, and an index buffer per LOD (stride 1, 2, 4, ...).
bool build_patch(ID3D11Device* dev, const Terrain& t, const Texture* diffuse, TextureCache& tex, int pi, int pj, GpuModel& out, std::unique_ptr<Texture>* skin_out) {
    const float tile = t.tile_metres > 0 ? t.tile_metres : 8.0f;
    auto height = [&](int i, int j) { return t.height_at(std::clamp(i, 0, t.nx - 1), std::clamp(j, 0, t.nz - 1)); };
    const int ci0 = pi * kCells, cj0 = pj * kCells;
    const int ci1 = std::min(ci0 + kCells, t.nx - 1), cj1 = std::min(cj0 + kCells, t.nz - 1);
    const int w = ci1 - ci0 + 1, h = cj1 - cj0 + 1;
    float miny = 1e30f, maxy = -1e30f;
    for (int j = cj0; j <= cj1; ++j)
        for (int i = ci0; i <= ci1; ++i) {
            const float y = height(i, j);
            miny = std::min(miny, y);
            maxy = std::max(maxy, y);
        }
    const float x0 = t.origin_x + float(ci0) * t.step, z0 = t.origin_z + float(cj0) * t.step;
    const float u0 = x0 / tile - std::floor(x0 / tile), v0 = z0 / tile - std::floor(z0 / tile);
    // The skin: this patch's rectangle of the image (one texel more on every side, edge texels repeated) becomes the patch's own texture, uv 0..1 across it.
    int sk_i0 = 0, sk_j0 = 0, sk_w = 0, sk_h = 0;
    double sk_dx = 1, sk_dz = 1;
    if (t.skin && t.skin->w > 1 && t.skin->h > 1 && skin_out) {
        const TerrainSkin& s = *t.skin;
        sk_dx = s.w_m / double(s.w);
        sk_dz = s.h_m / double(s.h);
        const double px0 = double(x0), px1 = double(x0) + double(w - 1) * double(t.step), pz0 = double(z0), pz1 = double(z0) + double(h - 1) * double(t.step);
        sk_i0 = int(std::floor((px0 - s.x0) / sk_dx)) - 1;
        sk_j0 = int(std::floor((pz0 - s.z0) / sk_dz)) - 1;
        const int i1 = int(std::ceil((px1 - s.x0) / sk_dx)) + 1, j1 = int(std::ceil((pz1 - s.z0) / sk_dz)) + 1;
        sk_w = std::max(2, i1 - sk_i0);
        sk_h = std::max(2, j1 - sk_j0);
        std::vector<uint8_t> crop(size_t(sk_w) * size_t(sk_h) * 4);
        for (int j = 0; j < sk_h; ++j) {
            const int sj = std::clamp(sk_j0 + j, 0, int(s.h) - 1);
            for (int i = 0; i < sk_w; ++i) {
                const int si = std::clamp(sk_i0 + i, 0, int(s.w) - 1);
                std::memcpy(&crop[(size_t(j) * size_t(sk_w) + size_t(i)) * 4], &s.rgba[(size_t(sj) * s.w + size_t(si)) * 4], 4);
            }
        }
        auto tx = std::make_unique<Texture>(make_rgba_texture(dev, uint32_t(sk_w), uint32_t(sk_h), crop.data(), true));
        if (tx->srv) *skin_out = std::move(tx);
    }
    const bool skinned = skin_out && *skin_out;
    const size_t grid = size_t(w) * size_t(h);
    const size_t skirt = size_t(2) * size_t(w) + size_t(2) * size_t(h);
    std::vector<Vertex0> v(grid + skirt);
    for (int j = 0; j < h; ++j) {
        for (int i = 0; i < w; ++i) {
            Vertex0& vx = v[size_t(j) * size_t(w) + size_t(i)];
            const int gi = ci0 + i, gj = cj0 + j;
            const float xl = float(i) * t.step, zl = float(j) * t.step;
            vx.pos[0] = to_half(xl);
            vx.pos[1] = to_half(height(gi, gj) - miny);
            vx.pos[2] = to_half(zl);
            vx.pos[3] = to_half(1);
            if (skinned) {
                const double X = double(x0) + double(xl), Z = double(z0) + double(zl);
                vx.uv[0] = to_half(float((X - t.skin->x0 - double(sk_i0) * sk_dx) / (double(sk_w) * sk_dx)));
                vx.uv[1] = to_half(float((Z - t.skin->z0 - double(sk_j0) * sk_dz) / (double(sk_h) * sk_dz)));
            } else {
                vx.uv[0] = to_half(u0 + xl / tile);
                vx.uv[1] = to_half(v0 + zl / tile);
            }
            vx.uv[2] = vx.uv[3] = 0;
            const float dhdx = (height(gi + 1, gj) - height(gi - 1, gj)) / (2 * t.step);
            const float dhdz = (height(gi, gj + 1) - height(gi, gj - 1)) / (2 * t.step);
            const float3 n = normalize({-dhdx, 1, -dhdz}), tg = normalize({1, dhdx, 0}), bt = normalize({0, dhdz, 1});
            pack_vec(vx.normal, n.x, n.y, n.z);
            pack_vec(vx.tangent, tg.x, tg.y, tg.z);
            pack_vec(vx.bitangent, bt.x, bt.y, bt.z);
            vx.colour[0] = vx.colour[1] = vx.colour[2] = vx.colour[3] = 255;
        }
    }
    // Skirts hang under the border vertices (south, north, west, east), so the cracks between patches of different detail stay closed.
    const float depth = std::max(1.0f, t.step * 4.0f);
    const uint32_t south = uint32_t(grid), north = south + uint32_t(w), west = north + uint32_t(w), east = west + uint32_t(h);
    auto lowered = [&](size_t from, size_t to) {
        v[to] = v[from];
        v[to].pos[1] = to_half(half_to_float(v[from].pos[1]) - depth);
    };
    for (int i = 0; i < w; ++i) {
        lowered(size_t(i), south + size_t(i));
        lowered(size_t(h - 1) * size_t(w) + size_t(i), north + size_t(i));
    }
    for (int j = 0; j < h; ++j) {
        lowered(size_t(j) * size_t(w), west + size_t(j));
        lowered(size_t(j) * size_t(w) + size_t(w - 1), east + size_t(j));
    }

    GpuMesh base;
    D3D11_BUFFER_DESC bd{UINT(v.size() * sizeof(Vertex0)), D3D11_USAGE_IMMUTABLE, D3D11_BIND_VERTEX_BUFFER};
    D3D11_SUBRESOURCE_DATA sd{v.data()};
    if (FAILED(dev->CreateBuffer(&bd, &sd, &base.vb))) return false;
    base.offset = {x0, miny, z0};
    base.has_offset = x0 != 0 || miny != 0 || z0 != 0;
    const Texture* d = skinned ? skin_out->get() : diffuse ? diffuse : tex.white();
    base.tex[kTexDiffuse] = d;
    base.tex[kTexSpecular] = tex.black();
    base.tex[kTexGloss] = tex.black();
    base.tex[kTexColourMask] = tex.black();
    base.tex[kTexNormal] = tex.flat_normal();
    base.bmin = {0, 0, 0};
    base.bmax = {float(w - 1) * t.step, maxy - miny, float(h - 1) * t.step};

    const int cells = std::max(ci1 - ci0, cj1 - cj0);
    const int max_lods = t.lod ? 5 : 1;
    const float patch_m = float(kCells) * t.step;
    out = GpuModel{};
    out.path = "(terrain patch)";
    for (int lod = 0, stride = 1; lod < max_lods && (lod == 0 || stride < cells); ++lod, stride *= 2) {
        const std::vector<int> xs = axis_positions(w - 1, stride), zs = axis_positions(h - 1, stride);
        std::vector<uint16_t> idx;
        idx.reserve((xs.size() - 1) * (zs.size() - 1) * 6 + (xs.size() + zs.size()) * 12);
        for (size_t j = 0; j + 1 < zs.size(); ++j)
            for (size_t i = 0; i + 1 < xs.size(); ++i) {
                const uint16_t a = uint16_t(zs[j] * w + xs[i]), b = uint16_t(zs[j + 1] * w + xs[i]), c = uint16_t(zs[j + 1] * w + xs[i + 1]), e = uint16_t(zs[j] * w + xs[i + 1]);
                for (uint16_t k : {a, b, c, a, c, e}) idx.push_back(k);
            }
        auto strip = [&](const std::vector<int>& pos, auto border, uint32_t skirt_base) {
            for (size_t k = 0; k + 1 < pos.size(); ++k) {
                const uint16_t b0 = uint16_t(border(pos[k])), b1 = uint16_t(border(pos[k + 1]));
                const uint16_t s0 = uint16_t(skirt_base + uint32_t(pos[k])), s1 = uint16_t(skirt_base + uint32_t(pos[k + 1]));
                for (uint16_t q : {b0, b1, s1, b0, s1, s0, b0, s1, b1, b0, s0, s1}) idx.push_back(q);   // both windings: no back-face culling decision for the skirt
            }
        };
        strip(xs, [&](int i) { return i; }, south);
        strip(xs, [&](int i) { return (h - 1) * w + i; }, north);
        strip(zs, [&](int j) { return j * w; }, west);
        strip(zs, [&](int j) { return j * w + (w - 1); }, east);
        GpuMesh g = base;
        D3D11_BUFFER_DESC ib{UINT(idx.size() * 2), D3D11_USAGE_IMMUTABLE, D3D11_BIND_INDEX_BUFFER};
        D3D11_SUBRESOURCE_DATA isd{idx.data()};
        if (FAILED(dev->CreateBuffer(&ib, &isd, &g.ib))) return false;
        g.index_count = UINT(idx.size());
        const bool last = lod + 1 >= max_lods || stride * 2 >= cells;
        const float dist = last ? 0.0f : patch_m * 1.5f * float(1 << lod);
        if (lod == 0) {
            out.meshes.push_back(std::move(g));
            out.distance = dist;
        } else {
            GpuLod l;
            l.meshes.push_back(std::move(g));
            l.distance = dist;
            out.lods.push_back(std::move(l));
        }
        if (last) break;
    }
    out.bmin = {x0, miny, z0};
    out.bmax = {x0 + float(w - 1) * t.step, maxy, z0 + float(h - 1) * t.step};
    return true;
}

} // namespace

const TerrainGpu* ModelCache::terrain(EntityId id, const Terrain& t) {
    WCE_PROF("terrain.update");
    const uint64_t layout = layout_hash(t);
    std::unique_ptr<TerrainGpu>& slot = terrain_[id];
    if (slot && slot->layout == layout && slot->version == t.version) return slot.get();
    if (!(t.nx >= 2 && t.nz >= 2 && (t.heights.empty() || t.heights.size() == size_t(t.nx) * size_t(t.nz)) && t.step > 0)) {
        slot.reset();
        return nullptr;
    }
    const int px = (t.nx - 1 + kCells - 1) / kCells, pz = (t.nz - 1 + kCells - 1) / kCells;
    const bool same_layout = slot && slot->layout == layout && slot->patches_x == px && slot->patches_z == pz;
    if (!same_layout) {
        slot = std::make_unique<TerrainGpu>();
        slot->patches_x = px;
        slot->patches_z = pz;
        slot->patches.resize(size_t(px) * size_t(pz));
        slot->skins.clear();
    }
    if (slot->skins.size() != slot->patches.size()) slot->skins.resize(slot->patches.size());
    // which patches changed: all on a new layout, otherwise the ones whose heights (plus the margin the normals read) differ from the last build
    std::vector<uint8_t> dirty(slot->patches.size(), 1);
    if (same_layout && !t.heights.empty() && slot->heights.size() == t.heights.size()) {
        compute_pool().parallel_for(slot->patches.size(), 4, [&](size_t b, size_t e, int) {
            for (size_t p = b; p < e; ++p) {
                const int pi = int(p % size_t(px)), pj = int(p / size_t(px));
                const int i0 = std::max(0, pi * kCells - 1), i1 = std::min(t.nx - 1, (pi + 1) * kCells + 1);
                const int j0 = std::max(0, pj * kCells - 1), j1 = std::min(t.nz - 1, (pj + 1) * kCells + 1);
                bool same = true;
                for (int j = j0; j <= j1 && same; ++j)
                    same = std::memcmp(&t.heights[size_t(j) * t.nx + i0], &slot->heights[size_t(j) * t.nx + i0], size_t(i1 - i0 + 1) * sizeof(float)) == 0;
                dirty[p] = same ? 0 : 1;
            }
        });
    }
    const Texture* d = tex_.get(t.diffuse, true);
    TerrainGpu& tg = *slot;
    std::vector<uint8_t> failed(tg.patches.size(), 0);
    compute_pool().parallel_for(tg.patches.size(), 1, [&](size_t b, size_t e, int) {
        for (size_t p = b; p < e; ++p) {
            if (!dirty[p]) continue;
            GpuModel gm;
            if (build_patch(dev_, t, d, tex_, int(p % size_t(px)), int(p / size_t(px)), gm, &tg.skins[p])) tg.patches[p] = std::move(gm);
            else failed[p] = 1;
        }
    });
    tg.heights = t.heights;
    tg.layout = layout;
    tg.version = t.version;
    for (uint8_t f : failed)
        if (f) {
            std::lock_guard<std::mutex> lock(mutex_);
            errors_.push_back("terrain: a patch could not be created");
            break;
        }
    return slot.get();
}

void ModelCache::prune_terrain(const RenderScene& scene) {
    for (auto it = terrain_.begin(); it != terrain_.end();) {
        const Entity* e = scene.find(it->first);
        it = (e && e->get<Terrain>()) ? std::next(it) : terrain_.erase(it);
    }
}

} // namespace wce
