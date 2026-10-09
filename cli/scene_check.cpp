// Scene gate over all tile.ted files in the corpus manifest:
//   1. parse + save is byte-identical;
//   2. for every instance with a position: set_position(read value) is a no-op (bytes unchanged);
//   3. translate +1 on x/z of a sample of instances, save, re-parse, translate back, save: the final bytes differ from the
//      original only where TEd's formatting of the moved value differs (counted), and revert_all restores the original;
//   4. format_ted_number(parse(v)) == v for position / orientation / scale values (counted, with the failures listed).
//   scene_check <manifest.tsv> <kits_parent_dir> [max_mb]
#include "tile/ted_scene.h"

#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <map>
#include <sstream>

namespace fs = std::filesystem;

static std::vector<uint8_t> slurp(const fs::path& p) {
    std::ifstream f(p, std::ios::binary);
    return std::vector<uint8_t>(std::istreambuf_iterator<char>(f), {});
}

static int run(int argc, char** argv) {
    if (argc < 3) { std::fprintf(stderr, "usage: scene_check <manifest.tsv> <kits_parent_dir> [max_mb]\n"); return 2; }
    double max_mb = argc > 3 ? std::atof(argv[3]) : 100;
    std::ifstream m(argv[1]);
    std::string line;
    std::getline(m, line);
    size_t files = 0, parse_fail = 0, save_diff = 0, noop_diff = 0, revert_diff = 0, instances = 0, fmt_ok = 0, fmt_total = 0;
    std::map<std::string, size_t> types;
    std::vector<std::string> notes;
    while (std::getline(m, line)) {
        std::vector<std::string> c;
        std::stringstream ss(line);
        std::string cell;
        while (std::getline(ss, cell, '\t')) c.push_back(cell);
        if (c.size() < 12 || c[11] != "in") continue;
        fs::path p = fs::path(argv[2]) / c[0] / c[1] / c[2];
        if (p.filename() != "tile.ted" || std::stod(c[3]) > max_mb * 1048576.0) continue;
        auto bytes = slurp(p);
        wce::TedScene sc;
        try { sc = wce::TedScene::parse(bytes); } catch (const std::exception& e) { ++parse_fail; if (notes.size() < 5) notes.push_back(c[2] + ": " + e.what()); continue; }
        ++files;
        if (sc.save() != bytes) { ++save_diff; continue; }
        size_t n = 0;
        for (size_t i = 0; i < sc.doc().elements().size(); ++i) {
            int e = static_cast<int>(i);
            auto pos = sc.position(e);
            if (!pos) continue;
            ++instances;
            ++types[sc.type(e)];
            sc.set_position(e, *pos);
            for (const char* a : {"position", "orientation", "scale"}) {
                auto txt = sc.doc().get(e, a);
                if (!txt) continue;
                const char* pre = a[0] == 'p' ? "vector" : a[0] == 'o' ? "euler" : "vector";
                auto v = wce::parse_vec3(*txt, pre);
                if (!v) continue;
                ++fmt_total;
                std::string back = std::string(pre) + "(" + wce::format_ted_number((*v)[0]) + "," + wce::format_ted_number((*v)[1]) + "," + wce::format_ted_number((*v)[2]) + ")";
                if (back == *txt) ++fmt_ok; else if (notes.size() < 8) notes.push_back("format: " + *txt + " -> " + back);
            }
            if (++n % 997 == 0) { sc.translate(e, {1, 0, 1}); }
        }
        if (n && sc.modified() == false && sc.save() != bytes) ++noop_diff;
        sc.revert_all();
        if (sc.save() != bytes) ++revert_diff;
    }
    std::printf("tile.ted files %zu (parse failures %zu, parse/save differs %zu)\n", files, parse_fail, save_diff);
    std::printf("instances with a position: %zu\n", instances);
    std::printf("no-op set_position changed bytes: %zu files; revert_all did not restore: %zu files\n", noop_diff, revert_diff);
    std::printf("number formatting reproduced: %zu of %zu (%.3f%%)\n", fmt_ok, fmt_total, fmt_total ? 100.0 * fmt_ok / fmt_total : 0.0);
    for (auto& [t, k] : types) std::printf("  %9zu %s\n", k, t.c_str());
    for (auto& n : notes) std::printf("note: %s\n", n.c_str());
    return 0;
}

int main(int argc, char** argv) {
    try { return run(argc, argv); } catch (const std::exception& e) { std::fprintf(stderr, "error: %s\n", e.what()); return 1; }
}
