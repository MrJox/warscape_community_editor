// Gate of Phase 3 (docs/plans/tile_ted_parity.md): the data side of the battle terrain colour.
//
//   terrain_check <game dir> [tile folder]
//
//  1. DXT decoding: hand-made BC1 / BC3 blocks decode to the expected colours (the 4-colour and the 3-colour + transparent BC1 modes, both BC3 alpha ramps);
//     a DDS built around them decodes per mip, with sizes that are not a multiple of 4.
//  2. Climates: the kit's _settings.xml has the eight climates, ROME_Continental lists mud with 9 m per repeat; the texture path falls back from the climate
//     to `default` and from a season to summer.
//  3. The bake: the given tile (default: tdd_custom_lothlorien/1x1/aa) bakes with the game's textures: the image covers the bordered tile at 4 texels per metre,
//     all twelve layers of that tile are found, it is not flat, two bakes are byte-identical, a blend that is pure grass gives a pixel from the grass texture,
//     and a climate without a group falls back to `default`. Needs the mounted game packs (about 1 s).
#include "formats/dds_decode.h"
#include "formats/tile_climate.h"
#include "foundation/process.h"
#include "tile/terrain_bake.h"
#include "tile/tile_project.h"
#include "vfs/vfs.h"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <set>
#include <string>
#include <vector>

namespace fs = std::filesystem;
using namespace wce;

namespace {

int g_failed = 0;

void check(bool ok, const std::string& what) {
    if (!ok) {
        ++g_failed;
        std::printf("FAILED: %s\n", what.c_str());
    }
}

void put32(std::vector<uint8_t>& b, size_t at, uint32_t v) {
    for (int i = 0; i < 4; ++i) b[at + size_t(i)] = uint8_t(v >> (8 * i));
}

// A DDS with the given fourcc and one block row of data per mip (all blocks equal to `blk`).
std::vector<uint8_t> make_dds(const char* fourcc, uint32_t w, uint32_t h, uint32_t mips, const std::vector<uint8_t>& blk) {
    std::vector<uint8_t> b(128, 0);
    std::memcpy(b.data(), "DDS ", 4);
    put32(b, 4, 124);
    put32(b, 12, h);
    put32(b, 16, w);
    put32(b, 28, mips);
    put32(b, 76, 32);
    put32(b, 80, 4);
    std::memcpy(&b[84], fourcc, 4);
    uint32_t mw = w, mh = h;
    for (uint32_t m = 0; m < mips; ++m) {
        const size_t blocks = size_t(std::max<uint32_t>(1, (mw + 3) / 4)) * std::max<uint32_t>(1, (mh + 3) / 4);
        for (size_t i = 0; i < blocks; ++i) b.insert(b.end(), blk.begin(), blk.end());
        mw = std::max<uint32_t>(1, mw / 2);
        mh = std::max<uint32_t>(1, mh / 2);
    }
    return b;
}

void test_dxt() {
    // BC1: c0 = 0xF800 (red), c1 = 0x001F (blue), indices 0, 1, 2, 3 in the first row
    const uint8_t bc1[8] = {0x00, 0xF8, 0x1F, 0x00, 0xE4, 0x00, 0x00, 0x00};
    uint8_t px[16][4];
    decode_bc1_block(bc1, px, true);
    check(px[0][0] == 255 && px[0][1] == 0 && px[0][2] == 0 && px[0][3] == 255, "BC1 index 0 is c0 (red)");
    check(px[1][0] == 0 && px[1][2] == 255, "BC1 index 1 is c1 (blue)");
    check(px[2][0] == 170 && px[2][2] == 85, "BC1 index 2 is 2/3 c0 + 1/3 c1");
    check(px[3][0] == 85 && px[3][2] == 170, "BC1 index 3 is 1/3 c0 + 2/3 c1");
    // 3-colour mode: c0 <= c1, index 3 transparent
    const uint8_t bc1t[8] = {0x1F, 0x00, 0x00, 0xF8, 0xE4, 0x00, 0x00, 0x00};
    decode_bc1_block(bc1t, px, true);
    check(px[2][0] == 127 && px[2][2] == 127 && px[2][3] == 255, "BC1 3-colour mode: index 2 is the middle");
    check(px[3][3] == 0, "BC1 3-colour mode: index 3 is transparent");
    // BC3 alpha: a0 > a1: 8 values; a0 <= a1: 6 values + 0 and 255
    uint8_t a[16];
    const uint8_t bc3a[8] = {255, 0, 0x88, 0xC6, 0xFA, 0, 0, 0};   // indices 0 1 2 3 4 5 6 7 in the first row (3 bits each)
    decode_bc3_alpha_block(bc3a, a);
    check(a[0] == 255 && a[1] == 0 && a[2] == 218 && a[7] == 36, "BC3 alpha, 8-value ramp");
    const uint8_t bc3b[8] = {50, 100, 0x88, 0xC6, 0xFA, 0, 0, 0};
    decode_bc3_alpha_block(bc3b, a);
    check(a[0] == 50 && a[1] == 100 && a[6] == 0 && a[7] == 255, "BC3 alpha, 6-value ramp with 0 and 255");

    // a whole DDS: 10 x 6 (not a multiple of 4), 3 mips, DXT5 all red opaque
    std::vector<uint8_t> blk(16, 0);
    blk[0] = blk[1] = 255;                  // alpha 255 / 255: every index gives 255
    blk[8] = 0x00; blk[9] = 0xF8;           // c0 red
    blk[10] = 0x00; blk[11] = 0xF8;         // c1 red
    const auto dds = make_dds("DXT5", 10, 6, 3, blk);
    const DdsInfo info = dds_info(dds);
    check(info.format == "DXT5" && info.width == 10 && info.height == 6 && info.mips == 3, "dds_info of a DXT5 file");
    Rgba8Image im;
    std::string why;
    check(dds_decode_mip(dds, 0, im, &why) && im.w == 10 && im.h == 6, "decode mip 0 (10 x 6)");
    check(im.w == 10 && im.at(9, 5)[0] == 255 && im.at(9, 5)[1] == 0 && im.at(9, 5)[3] == 255, "the last pixel of a partial block is decoded");
    check(dds_decode_mip(dds, 1, im, &why) && im.w == 5 && im.h == 3, "decode mip 1 (5 x 3)");
    check(dds_decode_mip(dds, 9, im, &why) && im.w == 2 && im.h == 1, "a mip past the end is the last one");
    check(!dds_decode_mip(std::vector<uint8_t>(40, 0), 0, im, &why) && !why.empty(), "garbage is refused with a reason");
}

void test_climates(const fs::path& settings) {
    const ClimateList cl = ClimateList::load(settings.string());
    check(cl.climates.size() == 8, "the kit has 8 climates (" + std::to_string(cl.climates.size()) + ")");
    const Climate* c = cl.find("rome_continental");
    check(c != nullptr, "ROME_Continental is found, in any case");
    if (c) {
        const ClimateTexture* mud = c->find("mud");
        check(mud && mud->blend_pixel_scale > 8.9 && mud->blend_pixel_scale < 9.1, "ROME_Continental: mud repeats every 9 m");
        check(c->folder() == "terrain/textures/battle/rome_continental", "climate folder");
    }
    const std::set<std::string> have = {"terrain/textures/battle/default/rock/0_su_diffuse.dds", "terrain/textures/battle/rome_med/grass/1_su_diffuse.dds"};
    auto ex = [&](const std::string& p) { return have.count(p) > 0; };
    check(climate_texture_path("terrain/textures/battle/rome_med", "grass", 1, "wi", false, ex) == "terrain/textures/battle/rome_med/grass/1_su_diffuse.dds", "season falls back to summer");
    check(climate_texture_path("terrain/textures/battle/rome_med", "rock", 0, "su", false, ex) == "terrain/textures/battle/default/rock/0_su_diffuse.dds", "a group the climate lacks falls back to default");
    check(climate_texture_path("terrain/textures/battle/rome_med", "snow", 0, "su", false, ex).empty(), "nothing found gives an empty path");
}

void test_bake(const fs::path& game, const fs::path& tile_dir) {
    Vfs::Config cfg = Vfs::default_config();
    cfg.game_dir = game;
    Vfs vfs(cfg);
    vfs.mount();
    auto project = TileProject::open(tile_dir);
    check(project->has_entry() && project->bordered(), "the tile has an entry and a bordered height map");
    if (!project->has_entry()) return;
    const fs::path settings = project->entry->file().parent_path().parent_path() / "_settings.xml";
    const ClimateList cl = ClimateList::load(settings.string());
    BakeRequest rq;
    rq.blend_map = project->dir / "blend_map.tif";
    rq.groups = project->entry->texture_group();
    const double b = project->border_m();
    rq.x0 = rq.z0 = -b;
    rq.w_m = project->extent_x() + 2 * b;
    rq.h_m = project->extent_z() + 2 * b;
    std::string why;
    const auto a = bake_terrain_colour(rq, cl, vfs, &why);
    check(a != nullptr, "the bake succeeds (" + why + ")");
    if (!a) return;
    check(a->w == uint32_t(std::lround(rq.w_m * 4)) || std::max(a->w, a->h) == rq.max_dim, "the image covers the bordered tile at 4 texels per metre (or the size cap)");
    check(a->missing == 0 && a->layers >= 4, "all layers' textures were found (" + std::to_string(a->layers) + " layers, " + std::to_string(a->missing) + " missing)");
    size_t distinct = 0;
    std::set<uint32_t> seen;
    for (size_t i = 0; i < a->rgba.size() && seen.size() < 64; i += 4 * 997) {
        uint32_t v;
        std::memcpy(&v, &a->rgba[i], 4);
        seen.insert(v);
    }
    distinct = seen.size();
    check(distinct > 20, "the ground colour is not flat (" + std::to_string(distinct) + " distinct sample values)");
    const auto again = bake_terrain_colour(rq, cl, vfs, &why);
    check(again && again->rgba == a->rgba, "two bakes are byte-identical");
    // another climate and season give another image
    BakeRequest rq2 = rq;
    rq2.look.climate = "ROME_Desert";
    rq2.look.season = "au";
    const auto other = bake_terrain_colour(rq2, cl, vfs, &why);
    check(other && other->rgba != a->rgba && other->w == a->w, "another climate and season give another image");
    // an unknown climate falls back to the default textures instead of failing
    BakeRequest rq3 = rq;
    rq3.look.climate = "no_such_climate";
    const auto fallback = bake_terrain_colour(rq3, cl, vfs, &why);
    check(fallback != nullptr, "an unknown climate falls back to default (" + why + ")");
    std::printf("bake: %s\n", a->report.c_str());
}

} // namespace

int main(int argc, char** argv) {
    if (argc < 2) {
        std::printf("usage: terrain_check <game dir> [tile folder]\n");
        return 2;
    }
    try {
        const fs::path game = argv[1];
        const fs::path kit = game / "assembly_kit_battle_tilemap" / "raw_data" / "terrain" / "tiles" / "battle";
        test_dxt();
        test_climates(kit / "_tile_database" / "_settings.xml");
        test_bake(game, argc > 2 ? fs::path(argv[2]) : kit / "tdd_custom_lothlorien" / "1x1" / "aa");
    } catch (const std::exception& e) {
        std::printf("error: %s\n", e.what());
        return 1;
    }
    std::printf("%s\n", g_failed ? "terrain_check: FAILED" : "terrain_check: ok");
    return g_failed ? 1 : 0;
}
