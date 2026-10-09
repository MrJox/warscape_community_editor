// Mounts the game packs and prints what the editor would see.
//   vfs_probe [--backend auto|server|cli] [path-to-fetch...]
//   vfs_probe --compare [--per-type N] [--time]    (--time adds "timing:" lines; without it the output is the same on every run)
//
// --compare is the Phase 2 gate: it mounts the packs once through rpfm_server and once through rpfm_cli, each into its own empty cache
// (cold), and checks that both give the same pack order, the same file lists, the same winner for every path, and the same bytes
// for a fixed sample: per pack and per file type (shader, model, texture, xml, environment, lua) the first N files (default 2)
// in the pack's own order, fetched from that pack, so vanilla, DLC and mod packs are all covered. Exit 1 on any difference.
#include "foundation/process.h"
#include "foundation/text_encoding.h"
#include "vfs/vfs.h"

#include <chrono>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <map>

using namespace wce;
namespace fs = std::filesystem;

namespace {

double since(std::chrono::steady_clock::time_point t0) { return std::chrono::duration<double>(std::chrono::steady_clock::now() - t0).count(); }

int compare(int per_type, bool times) {
    Vfs::Config base = Vfs::default_config();
    const fs::path tmp = base.cache_dir / "compare";
    std::error_code ec;
    fs::remove_all(tmp, ec);
    struct Side {
        const char* name;
        Vfs::BackendMode mode;
        std::unique_ptr<Vfs> vfs;
        double mount_s = 0;
    } sides[2] = {{"rpfm_server", Vfs::BackendMode::Server, nullptr}, {"rpfm_cli", Vfs::BackendMode::Cli, nullptr}};
    for (Side& s : sides) {
        Vfs::Config c = base;
        c.backend = s.mode;
        c.cache_dir = tmp / s.name;
        s.vfs = std::make_unique<Vfs>(c);
        auto t0 = std::chrono::steady_clock::now();
        s.vfs->mount();
        s.mount_s = since(t0);
        std::printf("%-12s cold mount: %zu packs, %zu files, read by %s\n", s.name, s.vfs->packs().size(), s.vfs->files().size(), s.vfs->backend_name().substr(0, s.vfs->backend_name().find(' ')).c_str());
        if (times) std::printf("timing: %-12s cold mount %.1f s\n", s.name, s.mount_s);
    }
    Vfs& a = *sides[0].vfs;
    Vfs& b = *sides[1].vfs;
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
        fail("file lists: " + std::to_string(only_a) + " only via server, " + std::to_string(only_b) + " only via cli, " + std::to_string(other_winner) + " with another winner");
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
            if (owner_pack != p) continue;   // files this pack wins; a shadowed file is fetched below via fetch_from as well
            for (const char* t : kTypes)
                if (path.ends_with(t) && taken[t] < per_type) {
                    ++taken[t];
                    auto da = a.fetch_from(p, path), db = b.fetch_from(p, path);
                    if (!da || !db) { fail(path + ": " + (da ? "missing via cli" : "missing via server")); continue; }
                    auto ba = read_file(da->string()), bb = read_file(db->string());
                    if (ba != bb) fail(path + ": bytes differ (" + std::to_string(ba.size()) + " vs " + std::to_string(bb.size()) + ")");
                    ++compared;
                    ++per_pack[a.packs()[p].name];
                    bytes += ba.size();
                }
        }
    }
    // A path two packs provide, read from both (shadowed copies must also match).
    size_t shadow = 0;
    for (auto& [path, o] : a.files()) {
        auto os = a.owners(path);
        if (os.size() < 2 || shadow >= 20) continue;
        for (int p : os) {
            auto da = a.fetch_from(p, path), db = b.fetch_from(p, path);
            if (!da || !db || read_file(da->string()) != read_file(db->string())) fail("shadowed " + path + " in pack " + a.packs()[p].name);
        }
        ++shadow;
    }
    std::printf("compared %zu sample files (%.1f MB) in %zu packs, %zu shadowed paths: %s\n", compared, bytes / 1048576.0, per_pack.size(), shadow, failures ? "DIFFERENCES" : "identical");
    return failures ? 1 : 0;
}

} // namespace

int main(int argc, char** argv) {
    try {
        Vfs::Config cfg = Vfs::default_config();
        bool do_compare = false;
        int per_type = 2;
        bool times = false;
        std::vector<std::string> fetch;
        for (int i = 1; i < argc; ++i) {
            if (!std::strcmp(argv[i], "--compare")) do_compare = true;
            else if (!std::strcmp(argv[i], "--time")) times = true;
            else if (!std::strcmp(argv[i], "--per-type") && i + 1 < argc) per_type = std::atoi(argv[++i]);
            else if (!std::strcmp(argv[i], "--backend") && i + 1 < argc) {
                std::string m = argv[++i];
                cfg.backend = m == "server" ? Vfs::BackendMode::Server : m == "cli" ? Vfs::BackendMode::Cli : Vfs::BackendMode::Auto;
            } else fetch.push_back(argv[i]);
        }
        if (do_compare) return compare(per_type, times);

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
        for (const std::string& f : fetch) {
            auto t1 = std::chrono::steady_clock::now();
            auto o = vfs.owner(f);
            auto d = vfs.fetch(f);
            std::printf("%s -> %s : %s (%.2f s)\n", f.c_str(), o ? vfs.packs()[*o].name.c_str() : "-", d ? d->string().c_str() : "missing", since(t1));
        }
        std::printf("backend: %s\n", vfs.backend_name().c_str());
        if (!vfs.backend_note().empty()) std::printf("note: %s\n", vfs.backend_note().c_str());
    } catch (const std::exception& e) {
        std::printf("error: %s\n", e.what());
        return 1;
    }
    return 0;
}
