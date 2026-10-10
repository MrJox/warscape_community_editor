#include "tile/terrain_bake.h"

#include "formats/dds_decode.h"
#include "formats/image_containers.h"
#include "formats/png_file.h"
#include "foundation/log.h"
#include "foundation/process.h"
#include "foundation/profile.h"
#include "foundation/thread_pool.h"

#include <algorithm>
#include <cmath>
#include <cstring>
#include <fstream>
#include <iterator>

namespace wce {

namespace {

struct Layer {
    int channel = 0;                         // 0..15 = group * 4 + variation
    Rgba8Image img;                          // the mip used
    double scale = 40.1;                     // metres per repeat
    bool ok = false;
    std::string path;
};

std::vector<uint8_t> read_all(const std::filesystem::path& p) {
    std::ifstream f(p, std::ios::binary);
    return std::vector<uint8_t>((std::istreambuf_iterator<char>(f)), std::istreambuf_iterator<char>());
}

// Bilinear sample with wrap (u, v in repeats), bytes r g b.
inline void sample_wrap(const Rgba8Image& im, double u, double v, float out[3]) {
    u = (u - std::floor(u)) * im.w - 0.5;
    v = (v - std::floor(v)) * im.h - 0.5;
    const double fu = std::floor(u), fv = std::floor(v);
    const float tx = float(u - fu), ty = float(v - fv);
    const int64_t w = im.w, h = im.h;
    const int64_t x0 = (int64_t(fu) % w + w) % w, y0 = (int64_t(fv) % h + h) % h, x1 = (x0 + 1) % w, y1 = (y0 + 1) % h;
    const uint8_t *a = im.at(uint32_t(x0), uint32_t(y0)), *b = im.at(uint32_t(x1), uint32_t(y0)), *c = im.at(uint32_t(x0), uint32_t(y1)), *d = im.at(uint32_t(x1), uint32_t(y1));
    for (int k = 0; k < 3; ++k) {
        const float top = float(a[k]) + (float(b[k]) - float(a[k])) * tx;
        const float bot = float(c[k]) + (float(d[k]) - float(c[k])) * tx;
        out[k] = top + (bot - top) * ty;
    }
}

} // namespace

std::shared_ptr<BakedColour> bake_terrain_colour(const BakeRequest& req, const ClimateList& climates, IVfs& vfs, std::string* why) {
    WCE_PROF("tile.terrain_bake");
    auto fail = [&](const std::string& m) -> std::shared_ptr<BakedColour> {
        if (why) *why = m;
        return nullptr;
    };
    std::error_code ec;
    if (!std::filesystem::exists(req.blend_map, ec)) return fail("no blend_map.tif");
    TiffFile tif;
    try {
        tif = TiffFile::parse(read_all(req.blend_map));
    } catch (const std::exception& e) {
        return fail(std::string("blend_map.tif: ") + e.what());
    }
    if (tif.samples_per_pixel != 16 || tif.bits_per_sample != 16 || !tif.width || !tif.height || tif.pixels.size() < size_t(tif.width) * tif.height * 32)
        return fail("blend_map.tif is not 16 samples of 16 bits per pixel");
    const uint32_t bw = tif.width, bh = tif.height;
    const uint16_t* blend = reinterpret_cast<const uint16_t*>(tif.pixels.data());   // little endian samples, 16 per pixel

    // the layers with weight somewhere: (group g, variation v) = channel 4g + v
    std::vector<uint32_t> peak(16, 0);
    for (size_t i = 0; i < size_t(bw) * bh; ++i)
        for (int c = 0; c < 16; ++c) peak[size_t(c)] = std::max<uint32_t>(peak[size_t(c)], blend[i * 16 + size_t(c)]);

    // output size
    const double ppm = std::min(req.max_texels_per_metre, double(req.max_dim) / std::max(req.w_m, req.h_m));
    const uint32_t ow = std::max<uint32_t>(2, uint32_t(std::lround(req.w_m * ppm))), oh = std::max<uint32_t>(2, uint32_t(std::lround(req.h_m * ppm)));
    const double out_ppm_x = double(ow) / req.w_m, out_ppm_z = double(oh) / req.h_m;

    const Climate* climate = climates.find(req.look.climate);
    if (!climate) climate = climates.find("default");
    const std::string folder = climate ? climate->folder() : "terrain/textures/battle/default";
    const std::string groups[4] = {req.groups.red, req.groups.green, req.groups.blue, req.groups.alpha};
    auto exists = [&](const std::string& p) { return vfs.owner(p).has_value(); };

    std::vector<Layer> layers;
    int missing = 0, wanted = 0;
    for (int c = 0; c < 16; ++c) {
        if (peak[size_t(c)] < 128) continue;   // under 0.2 %: nothing to draw
        ++wanted;
        const std::string& group = groups[c / 4];
        Layer L;
        L.channel = c;
        if (req.look.view == TerrainView::BlendGroups) {   // the debug view: every group one flat colour (red, green, blue, yellow)
            static const uint8_t col[4][3] = {{230, 60, 60}, {60, 200, 70}, {70, 110, 240}, {235, 210, 60}};
            L.img.w = L.img.h = 1;
            L.img.px = {col[c / 4][0], col[c / 4][1], col[c / 4][2], 255};
            L.ok = true;
            layers.push_back(std::move(L));
            continue;
        }
        const ClimateTexture* ct = climate ? climate->find(group) : nullptr;
        if (!ct)
            if (const Climate* d = climates.find("default")) ct = d->find(group);
        L.scale = ct && ct->blend_pixel_scale > 0.5 ? ct->blend_pixel_scale : 40.1;
        L.path = group.empty() ? std::string() : climate_texture_path(folder, group, c % 4, req.look.season, false, exists);
        if (L.path.empty()) { ++missing; continue; }
        try {
            const std::vector<uint8_t> bytes = vfs.read(L.path);
            const DdsInfo info = dds_info(bytes);
            // the mip whose texel density is nearest the output's (so the bake neither aliases nor blurs more than it has to)
            const double density = double(info.width) / L.scale;
            uint32_t mip = 0;
            while (mip + 1 < info.mips && density / double(1u << (mip + 1)) >= std::max(out_ppm_x, 0.5)) ++mip;
            std::string err;
            if (!dds_decode_mip(bytes, mip, L.img, &err)) { Log::warn("terrain bake: " + L.path + ": " + err); ++missing; continue; }
            L.ok = true;
        } catch (const std::exception& e) {
            Log::warn(std::string("terrain bake: ") + L.path + ": " + e.what());
            ++missing;
        }
        layers.push_back(std::move(L));
    }
    int usable = 0;
    for (const Layer& L : layers) usable += L.ok ? 1 : 0;
    if (req.look.view == TerrainView::BlendGroups) missing = 0;
    if (!usable) return fail("none of the climate's textures for this tile was found (" + req.look.climate + ")");

    auto out = std::make_shared<BakedColour>();
    out->x0 = req.x0;
    out->z0 = req.z0;
    out->w_m = req.w_m;
    out->h_m = req.h_m;
    out->w = ow;
    out->h = oh;
    out->rgba.assign(size_t(ow) * oh * 4, 255);
    out->climate = climate ? climate->name : "default";
    out->season = req.look.season;
    out->layers = wanted;
    out->missing = missing;

    // the weights of the layers that could not be read are dropped and the rest renormalised, so a missing texture shows as its neighbours
    compute_pool().parallel_for(size_t(oh), 8, [&](size_t r0, size_t r1, int) {
        std::vector<float> wgt(layers.size());
        for (size_t row = r0; row < r1; ++row) {
            const double z = req.z0 + (double(row) + 0.5) / out_ppm_z;
            // blend map sample row (pixel centres)
            const double by = (z - req.z0) / req.h_m * bh - 0.5;
            const int y0 = std::clamp(int(std::floor(by)), 0, int(bh) - 1), y1 = std::min(y0 + 1, int(bh) - 1);
            const float ty = float(std::clamp(by - std::floor(by), 0.0, 1.0));
            for (uint32_t col = 0; col < ow; ++col) {
                const double x = req.x0 + (double(col) + 0.5) / out_ppm_x;
                const double bx = (x - req.x0) / req.w_m * bw - 0.5;
                const int x0 = std::clamp(int(std::floor(bx)), 0, int(bw) - 1), x1 = std::min(x0 + 1, int(bw) - 1);
                const float tx = float(std::clamp(bx - std::floor(bx), 0.0, 1.0));
                float sum = 0;
                for (size_t k = 0; k < layers.size(); ++k) {
                    if (!layers[k].ok) { wgt[k] = 0; continue; }
                    const size_t c = size_t(layers[k].channel);
                    const float a = float(blend[(size_t(y0) * bw + size_t(x0)) * 16 + c]), b = float(blend[(size_t(y0) * bw + size_t(x1)) * 16 + c]);
                    const float cc = float(blend[(size_t(y1) * bw + size_t(x0)) * 16 + c]), d = float(blend[(size_t(y1) * bw + size_t(x1)) * 16 + c]);
                    const float top = a + (b - a) * tx, bot = cc + (d - cc) * tx;
                    wgt[k] = top + (bot - top) * ty;
                    sum += wgt[k];
                }
                float rgb[3] = {0, 0, 0};
                if (sum > 1e-3f) {
                    for (size_t k = 0; k < layers.size(); ++k) {
                        if (wgt[k] <= 0) continue;
                        float s[3];
                        sample_wrap(layers[k].img, x / layers[k].scale, z / layers[k].scale, s);
                        const float f = wgt[k] / sum;
                        rgb[0] += s[0] * f;
                        rgb[1] += s[1] * f;
                        rgb[2] += s[2] * f;
                    }
                } else rgb[0] = rgb[1] = rgb[2] = 96;
                uint8_t* o = &out->rgba[(row * ow + col) * 4];
                for (int k = 0; k < 3; ++k) o[k] = uint8_t(std::clamp(rgb[k] + 0.5f, 0.f, 255.f));
            }
        }
    });
    char b[200];
    if (req.look.view == TerrainView::BlendGroups) out->climate = "blend groups";
    std::snprintf(b, sizeof b, "%s %s: %d layer(s)%s, %u x %u texels (%.2f / m)", out->climate.c_str(), out->season.c_str(), wanted, missing ? (", " + std::to_string(missing) + " texture(s) missing").c_str() : "", ow, oh, out_ppm_x);
    out->report = b;
    return out;
}

// The tile's ground type map (ground_type_map.png, RGBA over the bordered tile) and protection map (protection_map.dds, R32F over the tile without border; 1 =
// protected) as images over the same area, for TEd's debug views.
std::shared_ptr<BakedColour> bake_terrain_map_view(const BakeRequest& req, std::string* why) {
    auto fail = [&](const std::string& m) -> std::shared_ptr<BakedColour> {
        if (why) *why = m;
        return nullptr;
    };
    auto out = std::make_shared<BakedColour>();
    if (req.look.view == TerrainView::GroundTypes) {
        const std::filesystem::path p = req.folder / "ground_type_map.png";
        std::error_code ec;
        if (!std::filesystem::exists(p, ec)) return fail("no ground_type_map.png");
        PngFile png;
        try {
            png = PngFile::parse(read_all(p));
        } catch (const std::exception& e) {
            return fail(std::string("ground_type_map.png: ") + e.what());
        }
        if (png.bit_depth != 8 || (png.color_type != 6 && png.color_type != 2)) return fail("ground_type_map.png is not 8-bit RGB(A)");
        const size_t bpp = png.bytes_per_pixel();
        out->x0 = req.x0;
        out->z0 = req.z0;
        out->w_m = req.w_m;
        out->h_m = req.h_m;
        out->w = png.width;
        out->h = png.height;
        out->rgba.assign(size_t(png.width) * png.height * 4, 255);
        size_t nonzero = 0;
        for (size_t i = 0; i < size_t(png.width) * png.height; ++i) {
            const uint8_t* s = &png.pixels[i * bpp];
            uint8_t* d = &out->rgba[i * 4];
            d[0] = s[0];
            d[1] = s[1];
            d[2] = s[2];
            if (s[0] | s[1] | s[2]) ++nonzero;
        }
        out->climate = "ground types";
        out->report = nonzero ? "ground types: the map's RGB as stored (" + std::to_string(nonzero) + " pixels set)" : "ground types: the map is empty (all pixels 0: no ground types painted)";
        return out;
    }
    // protection
    const std::filesystem::path p = req.folder / "protection_map.dds";
    std::error_code ec;
    if (!std::filesystem::exists(p, ec)) return fail("no protection_map.dds");
    const std::vector<uint8_t> bytes = read_all(p);
    if (bytes.size() < 132 || std::memcmp(bytes.data(), "DDS ", 4) != 0) return fail("protection_map.dds is not a DDS file");
    uint32_t h = 0, w = 0;
    std::memcpy(&h, &bytes[12], 4);
    std::memcpy(&w, &bytes[16], 4);
    if (!w || !h || bytes.size() < 128 + size_t(w) * h * 4) return fail("protection_map.dds is not an R32F image");
    // the map covers the tile itself (no border): it sits at 0 .. extent in the bordered area's frame
    out->x0 = 0;
    out->z0 = 0;
    out->w_m = req.w_m - 2 * (-req.x0);
    out->h_m = req.h_m - 2 * (-req.z0);
    out->w = w;
    out->h = h;
    out->rgba.assign(size_t(w) * h * 4, 255);
    size_t set = 0;
    for (size_t i = 0; i < size_t(w) * h; ++i) {
        float f;
        std::memcpy(&f, &bytes[128 + i * 4], 4);
        uint8_t* d = &out->rgba[i * 4];
        if (f > 0.5f) { d[0] = 220; d[1] = 50; d[2] = 50; ++set; }
        else { d[0] = 70; d[1] = 120; d[2] = 70; }
    }
    out->climate = "protection";
    out->report = "protection: " + std::to_string(set) + " of " + std::to_string(size_t(w) * h) + " cells protected (red)";
    return out;
}

} // namespace wce
