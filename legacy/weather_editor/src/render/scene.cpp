#include "scene.h"

#include "vfs/vfs.h"
#include "formats/rmv2.h"

#include <algorithm>
#include <cmath>
#include <cstring>
#include <random>

namespace wce {

TextureCache::TextureCache(ID3D11Device* dev, Vfs& vfs) : dev_(dev), vfs_(vfs) {
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

const GpuModel* TestScene::load_model(const std::string& path) {
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

void TestScene::build_ground(const std::string& diffuse, float s, float tile) {
    ground_ = std::make_unique<GpuModel>();
    ground_->path = "(ground)";
    float t = s * 2 / tile;
    const float corners[4][4] = {{-s, -s, 0, 0}, {-s, s, 0, t}, {s, s, t, t}, {s, -s, t, 0}}; // x, z, u, v
    Vertex0 v[4]{};
    for (int i = 0; i < 4; ++i) {
        v[i].pos[0] = to_half(corners[i][0]);
        v[i].pos[1] = to_half(0);
        v[i].pos[2] = to_half(corners[i][1]);
        v[i].pos[3] = to_half(1);
        v[i].uv[0] = to_half(corners[i][2]);
        v[i].uv[1] = to_half(corners[i][3]);
        pack_vec(v[i].normal, 0, 1, 0);
        pack_vec(v[i].tangent, 1, 0, 0);
        pack_vec(v[i].bitangent, 0, 0, 1);
        v[i].colour[0] = v[i].colour[1] = v[i].colour[2] = v[i].colour[3] = 255;
    }
    const uint16_t idx[6] = {0, 1, 2, 0, 2, 3};
    GpuMesh g;
    D3D11_BUFFER_DESC bd{sizeof v, D3D11_USAGE_IMMUTABLE, D3D11_BIND_VERTEX_BUFFER};
    D3D11_SUBRESOURCE_DATA sd{v};
    dev_->CreateBuffer(&bd, &sd, &g.vb);
    D3D11_BUFFER_DESC ib{sizeof idx, D3D11_USAGE_IMMUTABLE, D3D11_BIND_INDEX_BUFFER};
    D3D11_SUBRESOURCE_DATA isd{idx};
    dev_->CreateBuffer(&ib, &isd, &g.ib);
    g.index_count = 6;
    const Texture* d = tex_.get(diffuse, true);
    g.textures["s_diffuse_map"] = d ? d : tex_.white();
    g.textures["s_specular_colour_map"] = tex_.black();
    g.textures["s_gloss_map"] = tex_.black();
    g.textures["s_colour_mask_map"] = tex_.black();
    g.textures["s_normal_map"] = tex_.flat_normal();
    ground_->meshes.push_back(std::move(g));
    ground_->bmin = {-s, 0, -s};
    ground_->bmax = {s, 0, s};
}

int TestScene::generate(uint32_t seed, int count, const std::string& prefix) {
    objects_.clear();
    errors_.clear();
    if (!ground_) build_ground("terrain/textures/battle/default/grass/0_su_diffuse.dds");
    objects_.push_back({ground_.get(), float4x4{}});
    // A building is a folder of pieces; "<name>_pieceNN_destruct01" is each piece's intact state.
    // Piece placement nodes in <name>_tech.cs2.parsed are identity in every file checked, so the
    // pieces are drawn at the origin of the building (see docs/re/renderer.md).
    std::map<std::string, std::vector<std::string>> folders;
    for (auto& p : vfs_.list(prefix, ".rigid_model_v2")) {
        std::string file = p.substr(p.rfind('/') + 1);
        if (file.find("destruct") != std::string::npos && file.find("destruct01") == std::string::npos) continue;
        folders[p.substr(0, p.rfind('/'))].push_back(p);
    }
    std::vector<std::string> names;
    for (auto& [k, v] : folders) names.push_back(k);
    if (names.empty()) return 0;
    std::mt19937 rng(seed);
    std::shuffle(names.begin(), names.end(), rng);
    names.resize(std::min<size_t>(names.size(), size_t(count) * 3));

    // Batch extraction: every candidate model, then every texture they use (one rpfm call per pack).
    std::vector<std::string> model_paths, tex_paths;
    for (auto& n : names) model_paths.insert(model_paths.end(), folders[n].begin(), folders[n].end());
    vfs_.prefetch(model_paths);
    for (auto& mp : model_paths) {
        try {
            Rmv2Model m = parse_rmv2(vfs_.read(mp), 0);
            for (auto& lod : m.lods)
                for (auto& me : lod.meshes)
                    for (auto& t : me.textures) tex_paths.push_back(t.second);
        } catch (...) {
        }
    }
    vfs_.prefetch(tex_paths);

    struct Building { std::vector<const GpuModel*> pieces; float3 bmin, bmax; };
    std::vector<Building> picked;
    for (auto& n : names) {
        if (int(picked.size()) >= count) break;
        Building b{{}, {1e30f, 1e30f, 1e30f}, {-1e30f, -1e30f, -1e30f}};
        for (auto& mp : folders[n])
            if (const GpuModel* m = load_model(mp)) {
                b.pieces.push_back(m);
                b.bmin = {std::min(b.bmin.x, m->bmin.x), std::min(b.bmin.y, m->bmin.y), std::min(b.bmin.z, m->bmin.z)};
                b.bmax = {std::max(b.bmax.x, m->bmax.x), std::max(b.bmax.y, m->bmax.y), std::max(b.bmax.z, m->bmax.z)};
            }
        if (b.pieces.empty()) continue;
        float3 ext = b.bmax - b.bmin;
        if (ext.x < 2 || ext.z < 2 || ext.x > 80 || ext.z > 80 || ext.y < 1.5f) continue; // house-sized only
        picked.push_back(b);
    }
    // Grid layout with spacing from the largest footprint.
    float cell = 0;
    for (auto& b : picked) cell = std::max({cell, b.bmax.x - b.bmin.x, b.bmax.z - b.bmin.z});
    cell += 4;
    int side = int(std::ceil(std::sqrt(double(picked.size()))));
    bmin_ = {1e30f, 1e30f, 1e30f};
    bmax_ = {-1e30f, -1e30f, -1e30f};
    for (size_t i = 0; i < picked.size(); ++i) {
        const Building& b = picked[i];
        float cx = (float(i % side) - (side - 1) * 0.5f) * cell;
        float cz = (float(i / side) - (side - 1) * 0.5f) * cell;
        float3 c = (b.bmin + b.bmax) * 0.5f;
        float4x4 world = translation({cx - c.x, -b.bmin.y, cz - c.z});
        for (auto* m : b.pieces) objects_.push_back({m, world});
        bmin_ = {std::min(bmin_.x, cx - cell * 0.5f), 0, std::min(bmin_.z, cz - cell * 0.5f)};
        bmax_ = {std::max(bmax_.x, cx + cell * 0.5f), std::max(bmax_.y, b.bmax.y - b.bmin.y), std::max(bmax_.z, cz + cell * 0.5f)};
    }
    buildings_ = int(picked.size());
    return buildings_;
}

} // namespace wce
