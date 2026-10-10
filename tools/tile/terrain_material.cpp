#include "tile/terrain_material.h"

#include "formats/image_containers.h"
#include "foundation/log.h"
#include "foundation/profile.h"
#include "foundation/thread_pool.h"

#include <algorithm>
#include <cmath>
#include <fstream>
#include <iterator>

namespace wce {

namespace {

std::vector<uint8_t> read_all(const std::filesystem::path& p) {
    std::ifstream f(p, std::ios::binary);
    return std::vector<uint8_t>((std::istreambuf_iterator<char>(f)), std::istreambuf_iterator<char>());
}

inline uint8_t q8(double v) { return uint8_t(std::clamp(v * 255.0 + 0.5, 0.0, 255.0)); }

} // namespace

std::shared_ptr<TerrainMaterial> build_terrain_material(const MaterialRequest& rq, const ClimateList& climates, IVfs& vfs, std::string* why, std::string* report) {
    WCE_PROF("tile.terrain_material");
    auto fail = [&](const std::string& m) -> std::shared_ptr<TerrainMaterial> {
        if (why) *why = m;
        return nullptr;
    };
    std::error_code ec;
    const std::filesystem::path blend_path = rq.folder / "blend_map.tif";
    if (!std::filesystem::exists(blend_path, ec)) return fail("no blend_map.tif");
    TiffFile tif;
    try {
        tif = TiffFile::parse(read_all(blend_path));
    } catch (const std::exception& e) {
        return fail(std::string("blend_map.tif: ") + e.what());
    }
    if (tif.samples_per_pixel != 16 || tif.bits_per_sample != 16 || !tif.width || !tif.height || tif.pixels.size() < size_t(tif.width) * tif.height * 32)
        return fail("blend_map.tif is not 16 samples of 16 bits per pixel");
    const uint32_t bw = tif.width, bh = tif.height;
    const uint16_t* wgt = reinterpret_cast<const uint16_t*>(tif.pixels.data());

    auto m = std::make_shared<TerrainMaterial>();
    m->id = rq.id;
    m->x0 = rq.x0;
    m->z0 = rq.z0;
    m->x1 = rq.x1;
    m->z1 = rq.z1;
    m->y0 = rq.y0 - 20;
    m->y1 = rq.y1 + 200;

    // blend / index maps (BOB's reduction of the 16 weights)
    auto blend_px = std::make_shared<std::vector<uint8_t>>(size_t(bw) * bh * 4, 0);
    auto index_px = std::make_shared<std::vector<uint8_t>>(size_t(bw) * bh * 4, 0);
    std::vector<uint32_t> peak(16, 0);
    compute_pool().parallel_for(size_t(bh), 16, [&](size_t r0, size_t r1, int) {
        for (size_t y = r0; y < r1; ++y)
            for (size_t x = 0; x < bw; ++x) {
                const uint16_t* p = wgt + (y * bw + x) * 16;
                uint8_t* b = &(*blend_px)[(y * bw + x) * 4];
                uint8_t* ix = &(*index_px)[(y * bw + x) * 4];
                for (int g = 0; g < 4; ++g) {
                    const double s = double(p[g * 4]) + p[g * 4 + 1] + p[g * 4 + 2] + p[g * 4 + 3];
                    b[g] = q8(s / 65535.0);
                    ix[g] = s > 0 ? q8((double(p[g * 4 + 1]) + 2.0 * p[g * 4 + 2] + 3.0 * p[g * 4 + 3]) / s / 3.0) : 0;
                }
            }
    });
    for (size_t i = 0; i < size_t(bw) * bh; ++i)
        for (int c = 0; c < 16; ++c) peak[size_t(c)] = std::max<uint32_t>(peak[size_t(c)], wgt[i * 16 + size_t(c)]);
    m->blend = {bw, bh, blend_px};
    m->index = {bw, bh, index_px};
    m->normal = {1, 1, std::make_shared<std::vector<uint8_t>>(std::vector<uint8_t>{255, 127, 0, 127})};   // flat until the heights are given

    // the textures of the climate: slot = 4 * group + variation
    const Climate* climate = climates.find(rq.look.climate);
    if (!climate) climate = climates.find("default");
    const std::string folder = climate ? climate->folder() : "terrain/textures/battle/default";
    const Climate* def = climates.find("default");
    const std::string groups[4] = {rq.groups.red, rq.groups.green, rq.groups.blue, rq.groups.alpha};
    auto exists = [&](const std::string& p) { return vfs.owner(p).has_value(); };
    int wanted = 0, missing = 0;
    for (int g = 0; g < 4; ++g) {
        const ClimateTexture* ct = climate ? climate->find(groups[g]) : nullptr;
        if (!ct && def) ct = def->find(groups[g]);
        if (ct) {
            m->pixel_scale[g] = float(ct->blend_pixel_scale > 0.5 ? ct->blend_pixel_scale : 40.1);
            m->outfield_scale[g] = float(ct->outfield_blend_pixel_scale);
            m->mid_strength[g] = float(ct->mid_distance_strength);
        }
        for (int v = 0; v < 4; ++v) {
            const int slot = 4 * g + v;
            if (ct) {
                m->smoothness[slot] = float(ct->lerp_smoothness[v]);
                m->reflectivity[slot] = float(ct->lerp_reflectivity[v]);
            }
            const bool used = peak[size_t(slot)] >= 128;
            if (groups[g].empty()) continue;
            m->diffuse[slot] = climate_texture_path(folder, groups[g], v, rq.look.season, false, exists);
            m->normal_tex[slot] = climate_texture_path(folder, groups[g], v, rq.look.season, true, exists);
            if (used) {
                ++wanted;
                if (m->diffuse[slot].empty()) ++missing;
            }
        }
    }
    int found = 0;
    for (const std::string& d : m->diffuse) found += d.empty() ? 0 : 1;
    if (!found) return fail("none of the climate's textures for this tile was found (" + rq.look.climate + ")");
    // the detail maps of the climate
    for (const std::string& root : {folder, std::string("terrain/textures/battle/default")}) {
        if (m->detail_colour.empty() && exists(root + "/medium_detail_map_colour.dds")) m->detail_colour = root + "/medium_detail_map_colour.dds";
        if (m->detail_normal.empty() && exists(root + "/medium_detail_map_normal.dds")) m->detail_normal = root + "/medium_detail_map_normal.dds";
    }
    if (report) {
        char b[240];
        std::snprintf(b, sizeof b, "%s %s: %d layer(s)%s, %u x %u blend map (the game's terrain shaders)", climate ? climate->name.c_str() : "default", rq.look.season.c_str(), wanted,
                      missing ? (", " + std::to_string(missing) + " texture(s) missing").c_str() : "", bw, bh);
        *report = b;
    }
    return m;
}

TerrainImage terrain_normal_image(const std::vector<float>& heights, int nx, int nz, double step_x, double step_z, uint32_t w, uint32_t h) {
    TerrainImage out;
    out.w = w;
    out.h = h;
    auto px = std::make_shared<std::vector<uint8_t>>(size_t(w) * h * 4, 0);
    if (nx < 2 || nz < 2 || heights.size() < size_t(nx) * size_t(nz) || !w || !h) {
        out.w = out.h = 1;
        *px = {255, 127, 0, 127};
        out.rgba = px;
        return out;
    }
    auto H = [&](int i, int j) { return double(heights[size_t(std::clamp(j, 0, nz - 1)) * size_t(nx) + size_t(std::clamp(i, 0, nx - 1))]); };
    compute_pool().parallel_for(size_t(h), 16, [&](size_t r0, size_t r1, int) {
        for (size_t y = r0; y < r1; ++y) {
            const int j = int(std::lround(double(y) * double(nz - 1) / double(std::max<uint32_t>(h - 1, 1))));
            for (size_t x = 0; x < w; ++x) {
                const int i = int(std::lround(double(x) * double(nx - 1) / double(std::max<uint32_t>(w - 1, 1))));
                const double dhdx = (H(i + 1, j) - H(i - 1, j)) / (2 * step_x);
                const double dhdz = (H(i, j + 1) - H(i, j - 1)) / (2 * step_z);
                const double len = std::sqrt(dhdx * dhdx + 1 + dhdz * dhdz);
                uint8_t* d = &(*px)[(y * w + x) * 4];
                d[0] = 255;
                d[1] = q8((-dhdz / len) * 0.5 + 0.5);
                d[2] = 0;
                d[3] = q8((-dhdx / len) * 0.5 + 0.5);
            }
        }
    });
    out.rgba = px;
    return out;
}

} // namespace wce
