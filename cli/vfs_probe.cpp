// Mounts the game packs and prints what the editor would see.
//   vfs_probe [--find <substring>]... [path-to-fetch...]    (--find lists the paths containing every substring, with the pack that owns each)
//   vfs_probe --compare [--reference <cache dir>] [--per-type N] [--time]    (--time adds "timing:" lines; without it the output is the same on every run)
//
// --compare is the VFS gate: it mounts the packs through rpfm_server into an empty cache (cold) and checks the result against a reference
// cache (default: the configured cache_dir) written earlier, by this or another RPFM version: the same pack order, the same file lists, the same
// winner for every path, and the same bytes for a fixed sample: per pack and per file type (shader, model, texture, xml, environment, lua) the
// first N files (default 2) in the pack's own order among those the reference has extracted, fetched from that pack, plus up to 20 paths that
// several packs provide, read from each. The reference is read as it is (nothing is extracted into it). Exit 1 on any difference, or when there
// is nothing to compare.
#include "foundation/process.h"
#include "foundation/text_encoding.h"
#include "vfs/vfs.h"

#include <chrono>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <map>
#include <stdexcept>

using namespace wce;
namespace fs = std::filesystem;

namespace {

double since(std::chrono::steady_clock::time_point t0) { return std::chrono::duration<double>(std::chrono::steady_clock::now() - t0).count(); }

// The reference cache as it is: pack lists from its index, files only where it already extracted them.
class CacheOnly : public PackBackend {
public:
    std::string name() const override { return "reference cache"; }
    std::vector<std::string> list(const fs::path& pack) override {
        throw std::runtime_error("the reference cache has no index of " + narrow(pack.filename().wstring()) + " (written for another version of the pack?)");
    }
    void extract(const fs::path&, const std::vector<std::string>&, const fs::path&) override {}
};

int compare(const fs::path& reference, int per_type, bool times) {
    Vfs::Config base = Vfs::default_config();
    Vfs::Config ref_cfg = base;
    ref_cfg.cache_dir = reference.empty() ? base.cache_dir : reference;
    Vfs::Config cold_cfg = base;
    cold_cfg.cache_dir = base.cache_dir / "compare" / "rpfm_server";
    std::error_code ec;
    fs::remove_all(cold_cfg.cache_dir, ec);

    Vfs b(ref_cfg, std::make_unique<CacheOnly>());
    b.mount();
    std::printf("reference    %zu packs, %zu files\n", b.packs().size(), b.files().size());
    Vfs a(cold_cfg);
    auto t0 = std::chrono::steady_clock::now();
    a.mount();
    std::printf("rpfm_server  cold mount: %zu packs, %zu files\n", a.packs().size(), a.files().size());
    if (times) std::printf("timing: rpfm_server cold mount %.1f s\n", since(t0));

    int failures = 0;
    auto fail = [&](const std::string& m) {
        ++failures;
        if (failures <= 30) std::printf("  DIFF: %s\n", m.c_str());
    };

    if (a.packs().size() != b.packs().size()) fail("pack count");
    for (size_t i = 0; i < std::min(a.packs().size(), b.packs().size()); ++i)
        if (a.packs()[i].name != b.packs()[i].name) fail("pack " + std::to_string(i) + ": " + a.packs()[i].name + " vs " + b.packs()[i].name);
    if (a.files() != b.files()) {
        size_t only_a = 0, only_b = 0, other_winner = 0;
        for (auto& [p, o] : a.files()) {
            auto it = b.files().find(p);
            if (it == b.files().end()) ++only_a;
            else if (it->second != o) ++other_winner;
        }
        for (auto& [p, o] : b.files()) if (!a.files().count(p)) ++only_b;
        fail("file lists: " + std::to_string(only_a) + " only via rpfm_server, " + std::to_string(only_b) + " only in the reference, " + std::to_string(other_winner) + " with another winner");
    }
    size_t mods = 0;
    for (auto& p : a.packs()) mods += p.is_mod;
    std::printf("same pack order (%zu mod packs, %zu missing mods), %zu files, same winners: %s\n", mods, a.missing_mods().size(), a.files().size(), failures ? "NO" : "yes");

    static const char* const kTypes[] = {".fxc", ".rigid_model_v2", ".dds", ".xml", ".environment", ".lua"};
    size_t compared = 0, bytes = 0;
    std::map<std::string, size_t> per_pack;
    for (int p = 0; p < int(a.packs().size()) && p < int(b.packs().size()); ++p) {
        if (a.packs()[p].loose) continue;
        std::map<std::string, int> taken;
        for (auto& [path, owner_pack] : a.files()) {
            if (owner_pack != p) continue;   // files this pack wins; shadowed copies are read below
            for (const char* t : kTypes)
                if (path.ends_with(t) && taken[t] < per_type) {
                    auto db = b.fetch_from(p, path);
                    if (!db) continue;   // not in the reference
                    ++taken[t];
                    auto da = a.fetch_from(p, path);
                    if (!da) { fail(path + ": missing via rpfm_server"); continue; }
                    auto ba = read_file(da->string()), bb = read_file(db->string());
                    if (ba != bb) fail(path + ": bytes differ (" + std::to_string(ba.size()) + " vs " + std::to_string(bb.size()) + ")");
                    ++compared;
                    ++per_pack[a.packs()[p].name];
                    bytes += ba.size();
                }
        }
    }
    // Paths several packs provide, read from each (shadowed copies must also match).
    size_t shadow = 0;
    for (auto& [path, o] : a.files()) {
        auto os = a.owners(path);
        if (os.size() < 2 || shadow >= 20) continue;
        bool in_reference = true;
        for (int p : os) in_reference = in_reference && !a.packs()[p].loose && b.fetch_from(p, path).has_value();
        if (!in_reference) continue;
        for (int p : os) {
            auto da = a.fetch_from(p, path), db = b.fetch_from(p, path);
            if (!da || read_file(da->string()) != read_file(db->string())) fail("shadowed " + path + " in pack " + a.packs()[p].name);
        }
        ++shadow;
    }
    std::printf("compared %zu sample files (%.1f MB) in %zu packs, %zu shadowed paths: %s\n", compared, bytes / 1048576.0, per_pack.size(), shadow,
                failures ? "DIFFERENCES" : compared ? "identical" : "NOTHING TO COMPARE (the reference has no extracted files)");
    return failures || !compared ? 1 : 0;
}

} // namespace

int main(int argc, char** argv) {
    try {
        Vfs::Config cfg = Vfs::default_config();
        bool do_compare = false;
        fs::path reference;
        int per_type = 2;
        bool times = false;
        std::vector<std::string> fetch, finds;
        for (int i = 1; i < argc; ++i) {
            if (!std::strcmp(argv[i], "--compare")) do_compare = true;
            else if (!std::strcmp(argv[i], "--time")) times = true;
            else if (!std::strcmp(argv[i], "--per-type") && i + 1 < argc) per_type = std::atoi(argv[++i]);
            else if (!std::strcmp(argv[i], "--reference") && i + 1 < argc) reference = fs::path(widen(argv[++i]));
            else if (!std::strcmp(argv[i], "--find") && i + 1 < argc) finds.push_back(argv[++i]);
            else fetch.push_back(argv[i]);
        }
        if (do_compare) return compare(reference, per_type, times);

        auto t0 = std::chrono::steady_clock::now();
        Vfs vfs(cfg);
        vfs.mount();
        double s = since(t0);
        std::printf("mounted %zu packs, %zu files in %.1f s\n", vfs.packs().size(), vfs.files().size(), s);
        for (auto& m : vfs.missing_mods()) std::printf("  MISSING mod %s (in used_mods.txt, not in data/)\n", m.c_str());
        for (auto& p : vfs.packs()) std::printf("  %s %s\n", p.loose ? "loose  " : p.is_mod ? "mod    " : "vanilla", p.name.c_str());
        std::printf(".environment %zu, .lighting %zu, vfx xml %zu, fxc %zu, building rmv2 %zu\n",
                    vfs.list("", ".environment").size(), vfs.list("", ".lighting").size(), vfs.list("vfx/", ".xml").size(),
                    vfs.list("fxc/", ".fxc").size(), vfs.list("rigidmodels/buildings/", ".rigid_model_v2").size());
        if (!finds.empty())
            for (const auto& [path, pack] : vfs.files()) {
                bool all = true;
                for (const std::string& f : finds) all = all && path.find(f) != std::string::npos;
                if (all) std::printf("find: %s  (%s)\n", path.c_str(), vfs.packs()[size_t(pack)].name.c_str());
            }
        for (const std::string& f : fetch) {
            auto t1 = std::chrono::steady_clock::now();
            auto o = vfs.owner(f);
            auto d = vfs.fetch(f);
            std::printf("%s -> %s : %s (%.2f s)\n", f.c_str(), o ? vfs.packs()[*o].name.c_str() : "-", d ? d->string().c_str() : "missing", since(t1));
        }
        std::printf("backend: %s\n", vfs.backend_name().c_str());
    } catch (const std::exception& e) {
        std::printf("error: %s\n", e.what());
        return 1;
    }
    return 0;
}
