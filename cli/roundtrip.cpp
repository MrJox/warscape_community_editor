// Round-trip gate over the corpus manifest (scripts/corpus_inventory.py).
//
//   roundtrip <manifest.tsv> <kits_parent_dir> [--format NAME] [--max-size MB] [--limit N]
//
// For every in-scope manifest row whose file a registered format claims: parse, write back, compare byte for
// byte. Prints one line per format (files, passed, failed, skipped for size) and the first failures. Files no
// format claims are counted per extension so unimplemented formats stay visible.
#include "formats/registry.h"

#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <map>
#include <sstream>

namespace fs = std::filesystem;

static std::vector<std::string> split_tab(const std::string& line) {
    std::vector<std::string> out;
    size_t start = 0;
    for (;;) {
        size_t t = line.find('\t', start);
        if (t == std::string::npos) { out.push_back(line.substr(start)); break; }
        out.push_back(line.substr(start, t - start));
        start = t + 1;
    }
    return out;
}

static std::vector<uint8_t> read_all(const fs::path& p, size_t max_bytes = SIZE_MAX) {
    std::ifstream f(p, std::ios::binary);
    std::vector<uint8_t> v;
    if (!f) return v;
    f.seekg(0, std::ios::end);
    size_t n = static_cast<size_t>(f.tellg());
    f.seekg(0);
    v.resize(std::min(n, max_bytes));
    f.read(reinterpret_cast<char*>(v.data()), static_cast<std::streamsize>(v.size()));
    return v;
}

int main(int argc, char** argv) {
    if (argc < 3) {
        std::fprintf(stderr, "usage: roundtrip <manifest.tsv> <kits_parent_dir> [--format NAME] [--max-size MB] [--limit N]\n");
        return 2;
    }
    std::string only_format;
    double max_mb = 64;
    size_t limit = SIZE_MAX, max_failures = 25;
    for (int i = 3; i < argc; ++i) {
        if (!std::strcmp(argv[i], "--format") && i + 1 < argc) only_format = argv[++i];
        else if (!std::strcmp(argv[i], "--max-size") && i + 1 < argc) max_mb = std::atof(argv[++i]);
        else if (!std::strcmp(argv[i], "--limit") && i + 1 < argc) limit = std::strtoull(argv[++i], nullptr, 10);
        else if (!std::strcmp(argv[i], "--all-failures")) max_failures = SIZE_MAX;
    }
    wce::register_all_formats();

    struct Stat { size_t files = 0, ok = 0, failed = 0, skipped = 0; std::vector<std::string> failures; };
    std::map<std::string, Stat> stats;
    std::map<std::string, size_t> unclaimed;

    std::ifstream m(argv[1]);
    std::string line;
    std::getline(m, line); // header
    size_t seen = 0;
    while (std::getline(m, line)) {
        auto c = split_tab(line);
        if (c.size() < 12 || c[11] != "in") continue;
        fs::path p = fs::path(argv[2]) / c[0] / c[1] / c[2];
        std::vector<uint8_t> head = read_all(p, 4096);
        const wce::Format* f = wce::find_format(c[2], head);
        if (!f) { ++unclaimed[c[5].empty() ? "(none)" : c[5]]; continue; }
        if (!only_format.empty() && f->name != only_format) continue;
        if (seen++ >= limit) break;
        Stat& s = stats[f->name];
        ++s.files;
        if (std::stod(c[3]) > max_mb * 1048576.0) { ++s.skipped; continue; }
        wce::RoundTripResult r = f->round_trip(read_all(p));
        if (r.ok) ++s.ok;
        else {
            ++s.failed;
            if (s.failures.size() < max_failures) s.failures.push_back(c[0] + "/" + c[1] + "/" + c[2] + ": " + r.error);
        }
    }
    int rc = 0;
    for (auto& [name, s] : stats) {
        std::printf("%-12s files %7zu  ok %7zu  failed %5zu  skipped(size) %4zu\n", name.c_str(), s.files, s.ok, s.failed, s.skipped);
        for (auto& f : s.failures) std::printf("    FAIL %s\n", f.c_str());
        if (s.failed) rc = 1;
    }
    std::printf("files no registered format claims, by extension:");
    for (auto& [ext, n] : unclaimed) std::printf(" %s=%zu", ext.c_str(), n);
    std::printf("\n");
    return rc;
}
