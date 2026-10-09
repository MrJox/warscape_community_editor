// Rebuild tile_database.bin from its XML sources and compare with a shipped file.
//
//   tdb_build <_tile_database dir> <extra tiles dir or -> <shipped tile_database.bin or -> [--out file]
//
// With a shipped file the tile order is taken from it; the result is compared byte for byte and the first
// difference is reported. Without one the default order is used.
#include "formats/tile_database.h"

#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iterator>

namespace fs = std::filesystem;

static std::vector<uint8_t> slurp(const fs::path& p) {
    std::ifstream f(p, std::ios::binary);
    return std::vector<uint8_t>(std::istreambuf_iterator<char>(f), {});
}

static int run(int argc, char** argv) {
    if (argc < 4) { std::fprintf(stderr, "usage: tdb_build <_tile_database dir> <extra tiles dir|-> <shipped bin|-> [--out file]\n"); return 2; }
    fs::path dir = argv[1];
    wce::TileDbInput in;
    in.settings_xml = slurp(dir / "_settings.xml");
    std::vector<fs::path> files;
    for (auto& e : fs::directory_iterator(dir / "TILES")) if (e.path().extension() == ".xml") files.push_back(e.path());
    if (std::strcmp(argv[2], "-") != 0 && fs::exists(argv[2]))
        for (auto& e : fs::directory_iterator(argv[2])) if (e.path().extension() == ".xml") files.push_back(e.path());
    for (auto& f : files) in.tile_xml.push_back(slurp(f));
    in.add_user_tile_set = dir.string().find("battle") != std::string::npos;
    std::vector<uint8_t> ref;
    if (std::strcmp(argv[3], "-") != 0) { ref = slurp(argv[3]); in.order = wce::tile_database_order(ref); }
    std::vector<uint8_t> out = wce::build_tile_database(in);
    std::printf("built %zu bytes from %zu tiles\n", out.size(), files.size());
    for (int i = 4; i + 1 < argc; ++i)
        if (!std::strcmp(argv[i], "--out")) { std::ofstream(argv[i + 1], std::ios::binary).write(reinterpret_cast<const char*>(out.data()), std::streamsize(out.size())); }
    if (!ref.empty()) {
        size_t n = std::min(ref.size(), out.size()), diff = 0, first = SIZE_MAX;
        for (size_t i = 0; i < n; ++i) if (ref[i] != out[i]) { if (first == SIZE_MAX) first = i; ++diff; }
        std::printf("shipped %zu bytes; %zu differing bytes, first at %zu\n", ref.size(), diff + (ref.size() > n ? ref.size() - n : 0) + (out.size() > n ? out.size() - n : 0), first == SIZE_MAX ? 0 : first);
    }
    return 0;
}

int main(int argc, char** argv) {
    try {
        return run(argc, argv);
    } catch (const std::exception& e) {
        std::fprintf(stderr, "error: %s\n", e.what());
        return 1;
    }
}
