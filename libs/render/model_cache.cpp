#include "render/model_cache.h"

#include "vfs/vfs.h"
#include "formats/rmv2.h"

#include <algorithm>
#include <cmath>
#include <cstring>

namespace wce {

TextureCache::TextureCache(ID3D11Device* dev, IVfs& vfs) : dev_(dev), vfs_(vfs) {
    white_ = make_solid_texture(dev, 0xffffffff);
    black_ = make_solid_texture(dev, 0x00000000);
    flat_normal_ = make_solid_texture(dev, 0xff80ff80); // DXT5nm-style: x in .a, y in .g
}

const Texture* TextureCache::get(const std::string& path, bool srgb) {
    std::string key = Vfs::normalize(path) + (srgb ? "|s" : "|l");
    auto it = cache_.find(key);
    if (it != cache_.end()) return it->second.get();
    std::unique_ptr<Texture> t;
    try {
        t = std::make_unique<Texture>(load_dds(dev_, vfs_.read(path), srgb));
    } catch (const std::exception& e) {
        t.reset();
        if (errors_.size() < 50) errors_.push_back(path + ": " + e.what());
    }
    auto* raw = t.get();
    cache_[key] = std::move(t);
    return raw;
}

const GpuModel* ModelCache::model(const std::string& path) {
    auto it = models_.find(path);
    if (it != models_.end()) return it->second.get();
    auto gm = std::make_unique<GpuModel>();
    gm->path = path;
    gm->bmin = {1e30f, 1e30f, 1e30f};
    gm->bmax = {-1e30f, -1e30f, -1e30f};
    try {
        Rmv2Model m = parse_rmv2(vfs_.read(path), 0);
        if (m.lods.empty()) throw std::runtime_error("no LODs");
        for (auto& me : m.lods[0].meshes) {
            if (me.shader != RS_STANDARD_V5 || me.vertex_format != 0 || me.vertices.empty() || me.indices.empty()) continue;
            GpuMesh g;
            D3D11_BUFFER_DESC bd{};
            bd.Usage = D3D11_USAGE_IMMUTABLE;
            bd.BindFlags = D3D11_BIND_VERTEX_BUFFER;
            bd.ByteWidth = UINT(me.vertices.size());
            D3D11_SUBRESOURCE_DATA sd{me.vertices.data()};
            if (FAILED(dev_->CreateBuffer(&bd, &sd, &g.vb))) continue;
            bd.BindFlags = D3D11_BIND_INDEX_BUFFER;
            bd.ByteWidth = UINT(me.indices.size() * 2);
            sd.pSysMem = me.indices.data();
            if (FAILED(dev_->CreateBuffer(&bd, &sd, &g.ib))) continue;
            g.index_count = UINT(me.indices.size());
            g.alpha_mode = me.alpha_mode();
            auto tex = [&](int param, bool srgb, const Texture* fallback) {
                const std::string* p = me.texture(param);
                const Texture* t = p ? tex_.get(*p, srgb) : nullptr;
                return t ? t : fallback;
            };
            g.textures["s_diffuse_map"] = tex(TP_DIFFUSE, true, tex_.white());
            g.textures["s_specular_colour_map"] = tex(TP_SPECULAR, true, tex_.black());
            g.textures["s_gloss_map"] = tex(TP_GLOSS, false, tex_.black());
            g.textures["s_colour_mask_map"] = tex(TP_FACTION_MASK, false, tex_.black());
            g.textures["s_normal_map"] = tex(TP_NORMAL, false, tex_.flat_normal());
            gm->bmin = {std::min(gm->bmin.x, me.aabb[0]), std::min(gm->bmin.y, me.aabb[1]), std::min(gm->bmin.z, me.aabb[2])};
            gm->bmax = {std::max(gm->bmax.x, me.aabb[3]), std::max(gm->bmax.y, me.aabb[4]), std::max(gm->bmax.z, me.aabb[5])};
            gm->meshes.push_back(std::move(g));
        }
    } catch (const std::exception& e) {
        errors_.push_back(path + ": " + e.what());
    }
    auto* raw = gm.get();
    models_[path] = std::move(gm);
    return raw->meshes.empty() ? nullptr : raw;
}

namespace {

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

void pack_vec(uint8_t out[4], float x, float y, float z) { // memory order B, G, R, A -> shader x = R, y = G, z = B
    auto q = [](float v) { return uint8_t(std::lround((v * 0.5f + 0.5f) * 255.0f)); };
    out[0] = q(z);
    out[1] = q(y);
    out[2] = q(x);
    out[3] = 255;
}

} // namespace

const GpuModel* ModelCache::terrain(EntityId id, const Terrain& t) {
    size_t sig = std::hash<std::string>{}(t.diffuse);
    auto mix = [&](size_t v) { sig ^= v + 0x9e3779b97f4a7c15ull + (sig << 6) + (sig >> 2); };
    mix(size_t(t.version));
    mix(size_t(t.nx) * 65537u + size_t(t.nz));
    for (float f : {t.origin_x, t.origin_z, t.step, t.tile_metres}) {
        uint32_t bits;
        std::memcpy(&bits, &f, 4);
        mix(bits);
    }
    mix(t.heights.size());
    auto it = terrain_.find(id);
    if (it != terrain_.end() && it->second.signature == sig) return it->second.model.get();

    auto gm = std::make_unique<GpuModel>();
    gm->path = "(terrain)";
    gm->bmin = {1e30f, 1e30f, 1e30f};
    gm->bmax = {-1e30f, -1e30f, -1e30f};
    if (t.nx >= 2 && t.nz >= 2 && (t.heights.empty() || t.heights.size() == size_t(t.nx) * size_t(t.nz)) && t.step > 0) {
        const Texture* d = tex_.get(t.diffuse, true);
        const int kCells = 64;   // patch size: local half-float positions and UVs stay small
        const float tile = t.tile_metres > 0 ? t.tile_metres : 8.0f;
        auto height = [&](int i, int j) { return t.height_at(std::clamp(i, 0, t.nx - 1), std::clamp(j, 0, t.nz - 1)); };
        for (int cj0 = 0; cj0 < t.nz - 1; cj0 += kCells) {
            for (int ci0 = 0; ci0 < t.nx - 1; ci0 += kCells) {
                const int ci1 = std::min(ci0 + kCells, t.nx - 1), cj1 = std::min(cj0 + kCells, t.nz - 1);
                const int w = ci1 - ci0 + 1, h = cj1 - cj0 + 1;
                float miny = 1e30f, maxy = -1e30f;
                for (int j = cj0; j <= cj1; ++j)
                    for (int i = ci0; i <= ci1; ++i) {
                        miny = std::min(miny, height(i, j));
                        maxy = std::max(maxy, height(i, j));
                    }
                const float x0 = t.origin_x + float(ci0) * t.step, z0 = t.origin_z + float(cj0) * t.step;
                const float u0 = x0 / tile - std::floor(x0 / tile), v0 = z0 / tile - std::floor(z0 / tile);
                std::vector<Vertex0> v(size_t(w) * size_t(h));
                for (int j = 0; j < h; ++j) {
                    for (int i = 0; i < w; ++i) {
                        Vertex0& vx = v[size_t(j) * size_t(w) + size_t(i)];
                        const int gi = ci0 + i, gj = cj0 + j;
                        const float xl = float(i) * t.step, zl = float(j) * t.step;
                        vx.pos[0] = to_half(xl);
                        vx.pos[1] = to_half(height(gi, gj) - miny);
                        vx.pos[2] = to_half(zl);
                        vx.pos[3] = to_half(1);
                        vx.uv[0] = to_half(u0 + xl / tile);
                        vx.uv[1] = to_half(v0 + zl / tile);
                        const float dhdx = (height(gi + 1, gj) - height(gi - 1, gj)) / (2 * t.step);
                        const float dhdz = (height(gi, gj + 1) - height(gi, gj - 1)) / (2 * t.step);
                        const float3 n = normalize({-dhdx, 1, -dhdz}), tg = normalize({1, dhdx, 0}), bt = normalize({0, dhdz, 1});
                        pack_vec(vx.normal, n.x, n.y, n.z);
                        pack_vec(vx.tangent, tg.x, tg.y, tg.z);
                        pack_vec(vx.bitangent, bt.x, bt.y, bt.z);
                        vx.colour[0] = vx.colour[1] = vx.colour[2] = vx.colour[3] = 255;
                    }
                }
                std::vector<uint16_t> idx;
                idx.reserve(size_t(w - 1) * size_t(h - 1) * 6);
                for (int j = 0; j + 1 < h; ++j)
                    for (int i = 0; i + 1 < w; ++i) {
                        const uint16_t a = uint16_t(j * w + i), b = uint16_t((j + 1) * w + i), c = uint16_t((j + 1) * w + i + 1), e = uint16_t(j * w + i + 1);
                        for (uint16_t k : {a, b, c, a, c, e}) idx.push_back(k);
                    }
                GpuMesh g;
                D3D11_BUFFER_DESC bd{UINT(v.size() * sizeof(Vertex0)), D3D11_USAGE_IMMUTABLE, D3D11_BIND_VERTEX_BUFFER};
                D3D11_SUBRESOURCE_DATA sd{v.data()};
                if (FAILED(dev_->CreateBuffer(&bd, &sd, &g.vb))) continue;
                D3D11_BUFFER_DESC ib{UINT(idx.size() * 2), D3D11_USAGE_IMMUTABLE, D3D11_BIND_INDEX_BUFFER};
                D3D11_SUBRESOURCE_DATA isd{idx.data()};
                if (FAILED(dev_->CreateBuffer(&ib, &isd, &g.ib))) continue;
                g.index_count = UINT(idx.size());
                g.offset = {x0, miny, z0};
                g.has_offset = x0 != 0 || miny != 0 || z0 != 0;
                g.textures["s_diffuse_map"] = d ? d : tex_.white();
                g.textures["s_specular_colour_map"] = tex_.black();
                g.textures["s_gloss_map"] = tex_.black();
                g.textures["s_colour_mask_map"] = tex_.black();
                g.textures["s_normal_map"] = tex_.flat_normal();
                gm->bmin = {std::min(gm->bmin.x, x0), std::min(gm->bmin.y, miny), std::min(gm->bmin.z, z0)};
                gm->bmax = {std::max(gm->bmax.x, x0 + float(w - 1) * t.step), std::max(gm->bmax.y, maxy), std::max(gm->bmax.z, z0 + float(h - 1) * t.step)};
                gm->meshes.push_back(std::move(g));
            }
        }
    }
    const GpuModel* raw = gm->meshes.empty() ? nullptr : gm.get();
    terrain_[id] = TerrainEntry{sig, std::move(gm)};
    return raw;
}

void ModelCache::prune_terrain(const RenderScene& scene) {
    for (auto it = terrain_.begin(); it != terrain_.end();) {
        const Entity* e = scene.find(it->first);
        it = (e && e->get<Terrain>()) ? std::next(it) : terrain_.erase(it);
    }
}

} // namespace wce
