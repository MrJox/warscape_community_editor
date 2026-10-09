#include "weather/sample_map.h"

#include "formats/rmv2.h"
#include "vfs/vfs.h"

#include <algorithm>
#include <cmath>
#include <map>
#include <random>

namespace wce {

namespace {

struct PieceInfo {
    std::string path;
    float3 bmin{1e30f, 1e30f, 1e30f}, bmax{-1e30f, -1e30f, -1e30f};
};

// A piece is usable when the renderer can draw it: RS_STANDARD_V5 meshes of vertex format 0 in LOD 0 (see ModelCache::model).
bool piece_info(IVfs& vfs, const std::string& path, PieceInfo& out, std::vector<std::string>& errors) {
    try {
        Rmv2Model m = parse_rmv2(vfs.read(path), 0);
        if (m.lods.empty()) throw std::runtime_error("no LODs");
        out.path = path;
        bool any = false;
        for (auto& me : m.lods[0].meshes) {
            if (me.shader != RS_STANDARD_V5 || me.vertex_format != 0 || me.vertices.empty() || me.indices.empty()) continue;
            out.bmin = {std::min(out.bmin.x, me.aabb[0]), std::min(out.bmin.y, me.aabb[1]), std::min(out.bmin.z, me.aabb[2])};
            out.bmax = {std::max(out.bmax.x, me.aabb[3]), std::max(out.bmax.y, me.aabb[4]), std::max(out.bmax.z, me.aabb[5])};
            any = true;
        }
        return any;
    } catch (const std::exception& e) {
        errors.push_back(path + ": " + e.what());
        return false;
    }
}

} // namespace

SampleMapResult publish_sample_map(IVfs& vfs, RenderScene& scene, LayerId layer, DocumentId doc, uint32_t seed, int count, const std::string& prefix) {
    SampleMapResult res;
    scene.clear_layer(layer);
    scene.add(layer, float4x4{}, Terrain::flat(1000.0f, "terrain/textures/battle/default/grass/0_su_diffuse.dds", 8.0f), SourceRef{doc, 0});

    std::map<std::string, std::vector<std::string>> folders;
    for (auto& p : vfs.list(prefix, ".rigid_model_v2")) {
        std::string file = p.substr(p.rfind('/') + 1);
        if (file.find("destruct") != std::string::npos && file.find("destruct01") == std::string::npos) continue;
        folders[p.substr(0, p.rfind('/'))].push_back(p);
    }
    std::vector<std::string> names;
    for (auto& [k, v] : folders) names.push_back(k);
    if (names.empty()) return res;
    std::mt19937 rng(seed);
    std::shuffle(names.begin(), names.end(), rng);
    names.resize(std::min<size_t>(names.size(), size_t(count) * 3));

    // Batch extraction: every candidate model, then every texture they use (one backend call per pack).
    std::vector<std::string> model_paths, tex_paths;
    for (auto& n : names) model_paths.insert(model_paths.end(), folders[n].begin(), folders[n].end());
    vfs.prefetch(model_paths);
    for (auto& mp : model_paths) {
        try {
            Rmv2Model m = parse_rmv2(vfs.read(mp), 0);
            for (auto& lod : m.lods)
                for (auto& me : lod.meshes)
                    for (auto& t : me.textures) tex_paths.push_back(t.second);
        } catch (...) {
        }
    }
    vfs.prefetch(tex_paths);

    struct Building { std::vector<PieceInfo> pieces; float3 bmin, bmax; };
    std::vector<Building> picked;
    for (auto& n : names) {
        if (int(picked.size()) >= count) break;
        Building b{{}, {1e30f, 1e30f, 1e30f}, {-1e30f, -1e30f, -1e30f}};
        for (auto& mp : folders[n]) {
            PieceInfo pi;
            if (!piece_info(vfs, mp, pi, res.errors)) continue;
            b.bmin = {std::min(b.bmin.x, pi.bmin.x), std::min(b.bmin.y, pi.bmin.y), std::min(b.bmin.z, pi.bmin.z)};
            b.bmax = {std::max(b.bmax.x, pi.bmax.x), std::max(b.bmax.y, pi.bmax.y), std::max(b.bmax.z, pi.bmax.z)};
            b.pieces.push_back(std::move(pi));
        }
        if (b.pieces.empty()) continue;
        float3 ext = b.bmax - b.bmin;
        if (ext.x < 2 || ext.z < 2 || ext.x > 80 || ext.z > 80 || ext.y < 1.5f) continue; // house-sized only
        picked.push_back(std::move(b));
    }
    // Grid layout with spacing from the largest footprint.
    float cell = 0;
    for (auto& b : picked) cell = std::max({cell, b.bmax.x - b.bmin.x, b.bmax.z - b.bmin.z});
    cell += 4;
    int side = int(std::ceil(std::sqrt(double(picked.size()))));
    res.bounds_min = {1e30f, 1e30f, 1e30f};
    res.bounds_max = {-1e30f, -1e30f, -1e30f};
    for (size_t i = 0; i < picked.size(); ++i) {
        const Building& b = picked[i];
        float cx = (float(i % side) - (side - 1) * 0.5f) * cell;
        float cz = (float(i / side) - (side - 1) * 0.5f) * cell;
        float3 c = (b.bmin + b.bmax) * 0.5f;
        float4x4 world = translation({cx - c.x, -b.bmin.y, cz - c.z});
        for (auto& piece : b.pieces) {
            MeshInstance mi;
            mi.model = piece.path;
            scene.add(layer, world, std::move(mi), SourceRef{doc, uint64_t(i + 1)});
        }
        res.bounds_min = {std::min(res.bounds_min.x, cx - cell * 0.5f), 0, std::min(res.bounds_min.z, cz - cell * 0.5f)};
        res.bounds_max = {std::max(res.bounds_max.x, cx + cell * 0.5f), std::max(res.bounds_max.y, b.bmax.y - b.bmin.y), std::max(res.bounds_max.z, cz + cell * 0.5f)};
    }
    res.buildings = int(picked.size());
    return res;
}

} // namespace wce
