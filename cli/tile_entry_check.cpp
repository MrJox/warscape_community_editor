// Gate of Phase 0b (docs/plans/tile_ted_parity.md): the tile database entry as the root of a tile.
//
//   tile_entry_check <game dir> [--all] [--time] [tile folder...]
//
//  1. Entry round trip: every _tile_database/TILES/*.xml of both kits parses (TileEntry) and saves back byte for byte; the typed access covers every
//     attribute and child (unknown names: 0); every entry has variations.
//  2. The index (TileIndex): built without a cache, with a cold cache and with the warm cache; the three give the same rows, the warm one parses nothing;
//     every variation folder, tile.ted path and tile folder resolves to its entry; no two entries claim one location.
//  3. Open by entry equals open by folder: for each given tile folder (--all: every kit tile that has a tile.ted and an entry) TileProject::open(folder),
//     ::open(tile.ted) and ::open_entry(entry, variation) give the same document (same folder, entry bytes, size, heights, scene bytes).
//  4. A tile.ted outside the kit opens with a warning and no entry (a copy of the first tile folder given).
#include "foundation/process.h"
#include "foundation/text_encoding.h"
#include "formats/tile_entry.h"
#include "formats/tile_index.h"
#include "tile/tile_project.h"

#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstdlib>
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

double seconds_since(std::chrono::steady_clock::time_point t0) { return std::chrono::duration<double>(std::chrono::steady_clock::now() - t0).count(); }

bool same_rows(const TileIndex& a, const TileIndex& b) {
    if (a.entries().size() != b.entries().size()) return false;
    for (size_t i = 0; i < a.entries().size(); ++i) {
        const TileIndexEntry &x = a.entries()[i], &y = b.entries()[i];
        if (x.name != y.name || x.xml != y.xml || x.kind != y.kind || x.location != y.location || x.key != y.key || x.tile_set != y.tile_set || x.width != y.width || x.height != y.height ||
            x.variations != y.variations || x.links != y.links || x.targets != y.targets || x.size != y.size || x.mtime != y.mtime)
            return false;
        if (std::abs(x.red - y.red) > 1e-4f || std::abs(x.green - y.green) > 1e-4f || std::abs(x.blue - y.blue) > 1e-4f) return false;
    }
    return true;
}

uint64_t hash_samples(const HeightField& h) {
    uint64_t x = 1469598103934665603ull;
    for (uint16_t s : h.samples()) x = (x ^ s) * 1099511628211ull;
    return x;
}

// Same document: the folder, the entry (bytes and what is read from it), the numbers, the height map and the scene.
std::string differ(const TileProject& a, const TileProject& b) {
    if (a.dir != b.dir) return "dir " + a.dir.string() + " / " + b.dir.string();
    if ((a.entry != nullptr) != (b.entry != nullptr)) return "entry present in one only";
    if (a.entry && a.entry->save() != b.entry->save()) return "entry bytes";
    if (a.variation != b.variation) return "variation index";
    if (a.raw_root != b.raw_root) return "raw root";
    const TileInfo &x = a.info, &y = b.info;
    if (x.found != y.found || x.location != y.location || x.variation != y.variation || x.cells_x != y.cells_x || x.cells_y != y.cells_y || x.min_height != y.min_height || x.scale != y.scale ||
        x.normal_strength != y.normal_strength || x.overlap_border_size != y.overlap_border_size || x.raw_data_tri_density != y.raw_data_tri_density)
        return "tile info";
    if (a.has_height != b.has_height) return "height map presence";
    if (a.has_height && (a.height->width() != b.height->width() || a.height->height() != b.height->height() || hash_samples(*a.height) != hash_samples(*b.height))) return "height map";
    if (a.scene.doc().elements().size() != b.scene.doc().elements().size()) return "instance count";
    if (a.scene.save() != b.scene.save()) return "scene bytes";
    return {};
}

} // namespace

int main(int argc, char** argv) {
    if (argc < 2) {
        std::fprintf(stderr, "usage: tile_entry_check <game dir> [--all] [--time] [tile folder...]\n");
        return 2;
    }
    const fs::path game = fs::path(widen(argv[1]));
    bool all = false, show_time = false;
    std::vector<fs::path> tiles;
    for (int i = 2; i < argc; ++i) {
        const std::string a = argv[i];
        if (a == "--all") all = true;
        else if (a == "--time") show_time = true;
        else tiles.push_back(fs::path(widen(a)));
    }
    const std::vector<fs::path> roots = {game / "assembly_kit_battle_tilemap" / "raw_data", game / "assembly_kit_campaign_tilemap" / "raw_data"};

    // ---- 1. entry round trip
    size_t files = 0, unknown = 0, no_variation = 0, with_links = 0, with_targets = 0, variations = 0;
    for (const fs::path& root : roots) {
        std::error_code ec;
        if (!fs::is_directory(root / "terrain" / "tiles", ec)) continue;
        for (const fs::directory_entry& kind : fs::directory_iterator(root / "terrain" / "tiles", ec)) {
            const fs::path dir = kind.path() / "_tile_database" / "TILES";
            if (!fs::is_directory(dir, ec)) continue;
            for (const fs::directory_entry& f : fs::directory_iterator(dir, ec)) {
                if (f.path().extension() != ".xml") continue;
                ++files;
                try {
                    const std::vector<uint8_t> bytes = read_file(narrow(f.path().wstring()));
                    const TileEntry e = TileEntry::load(f.path());
                    check(e.save() == bytes, "round trip " + f.path().string());
                    for (const std::string& u : e.unknown_names()) {
                        ++unknown;
                        std::printf("unknown: %s in %s\n", u.c_str(), f.path().filename().string().c_str());
                    }
                    const auto v = e.variations();
                    variations += v.size();
                    if (v.empty()) ++no_variation;
                    if (!e.links().empty()) ++with_links;
                    if (!e.link_targets().empty()) ++with_targets;
                    check(e.width() > 0 && e.height() > 0 && !e.location().empty() && !e.tile_set().empty(), "typed fields " + f.path().string());
                    check(e.raw_root() == root, "raw_root of " + f.path().string());
                    check(!e.kind().empty(), "kind of " + f.path().string());
                } catch (const std::exception& ex) {
                    check(false, std::string("parse ") + f.path().string() + ": " + ex.what());
                }
            }
        }
    }
    std::printf("entries: %zu files round-trip byte for byte, %zu unknown attributes or children, %zu without variations, %zu variations, %zu with links, %zu with link targets\n", files, unknown, no_variation,
                variations, with_links, with_targets);
    check(unknown == 0 && no_variation == 0, "typed access covers every entry");

    // ---- 2. the index
    const fs::path cache = fs::temp_directory_path() / "wce_tile_entry_check_index.tsv";
    std::error_code ec;
    fs::remove(cache, ec);
    size_t rows = 0, resolved = 0, dups = 0;
    for (const fs::path& root : roots) {
        if (!fs::is_directory(root, ec)) continue;
        const auto t0 = std::chrono::steady_clock::now();
        const auto plain = TileIndex::build(root);
        const double t_plain = seconds_since(t0);
        const auto cold = TileIndex::build(root, cache);   // writes the cache
        const auto t1 = std::chrono::steady_clock::now();
        const auto warm = TileIndex::build(root, cache);   // reads it
        const double t_warm = seconds_since(t1);
        const std::string kit = root.parent_path().filename().string();
        check(plain->problems.empty(), kit + ": index problems");
        check(cold->parsed == plain->entries().size() && cold->from_cache == 0, kit + ": cold cache parses every entry");
        check(warm->parsed == 0 && warm->from_cache == plain->entries().size(), kit + ": the warm cache parses nothing");
        check(same_rows(*plain, *cold) && same_rows(*plain, *warm), kit + ": plain, cold and warm index give the same rows");
        for (const std::string& d : plain->duplicate_locations) { ++dups; std::printf("%s: location claimed twice: %s\n", kit.c_str(), d.c_str()); }
        rows += plain->entries().size();
        if (show_time) std::printf("time: %s index %zu rows: scan %.3f s, warm cache %.3f s\n", kit.c_str(), plain->entries().size(), t_plain, t_warm);
        for (const TileIndexEntry& e : plain->entries()) {
            const TileEntry entry = TileEntry::load(e.xml);
            check(plain->find_location(entry.location()) != nullptr, kit + ": find_location " + e.key);
            check(plain->find_name(e.name) != nullptr, kit + ": find_name " + e.name);
            for (size_t v = 0; v < e.variations.size(); ++v) {
                const fs::path folder = entry.variation_folder(root, v);
                const TileIndex::Resolved r1 = plain->resolve(folder), r2 = plain->resolve(folder / "tile.ted"), r3 = plain->resolve(entry.tile_folder(root));
                const bool dup = std::find(plain->duplicate_locations.begin(), plain->duplicate_locations.end(), e.key) != plain->duplicate_locations.end();
                if (!dup) {
                    check(r1.entry && r1.entry->xml == e.xml && r1.variation == int(v), kit + ": resolve folder " + folder.string());
                    check(r2.entry && r2.entry->xml == e.xml && r2.variation == int(v), kit + ": resolve tile.ted " + folder.string());
                    check(r3.entry && r3.entry->xml == e.xml && r3.variation == -1, kit + ": resolve tile folder " + folder.string());
                }
                ++resolved;
            }
        }
    }
    fs::remove(cache, ec);
    std::printf("index: %zu rows in both kits, the plain, cold-cache and warm-cache builds agree, %zu variation folders resolve to their entry, %zu locations claimed twice\n", rows, resolved, dups);

    // ---- 3. open by folder / tile.ted / entry
    std::vector<fs::path> folders = tiles;
    if (all)
        for (const fs::path& root : roots) {
            if (!fs::is_directory(root / "terrain" / "tiles", ec)) continue;
            const auto idx = TileIndex::build(root);
            for (const TileIndexEntry& e : idx->entries()) {
                const TileEntry entry = TileEntry::load(e.xml);
                for (size_t v = 0; v < e.variations.size(); ++v)
                    if (fs::exists(entry.ted_path(root, v), ec)) folders.push_back(entry.variation_folder(root, v));
            }
        }
    size_t opened = 0, different = 0, unopenable = 0;
    fs::path first_opened;
    const auto t_open = std::chrono::steady_clock::now();
    for (const fs::path& folder : folders) {
        // each way gives a project or an error; a tile that cannot be opened (a corrupt height map in the kit) must fail the same way through all three
        std::unique_ptr<TileProject> by_folder, by_ted, by_entry;
        std::string e_folder, e_ted, e_entry;
        try { by_folder = TileProject::open(folder); } catch (const std::exception& ex) { e_folder = ex.what(); }
        try { by_ted = TileProject::open(folder / "tile.ted"); } catch (const std::exception& ex) { e_ted = ex.what(); }
        if (by_folder && by_folder->entry) {
            try { by_entry = TileProject::open_entry(by_folder->entry->file(), by_folder->variation); } catch (const std::exception& ex) { e_entry = ex.what(); }
        } else if (!by_folder) {
            const TileIndex::Resolved r = TileIndex::shared(kit_raw_root(folder))->resolve(folder);
            if (r.entry) try { by_entry = TileProject::open_entry(r.entry->xml, r.variation); } catch (const std::exception& ex) { e_entry = ex.what(); }
        }
        if (!by_folder) {
            if (!by_ted && e_folder == e_ted && e_folder == e_entry) { ++unopenable; std::printf("cannot be opened, the same error through all three ways: %s\n", e_folder.c_str()); }
            else check(false, "open " + folder.string() + ": " + e_folder + " | " + e_ted + " | " + e_entry);
            continue;
        }
        std::string d = by_ted ? differ(*by_folder, *by_ted) : "tile.ted open: " + e_ted;
        if (d.empty() && by_folder->entry) d = by_entry ? differ(*by_folder, *by_entry) : "entry open: " + e_entry;
        check(by_folder->has_entry(), "no tile database entry for " + folder.string());
        if (!d.empty()) { ++different; check(false, "open by folder / tile.ted / entry differ in " + d + ": " + folder.string()); }
        if (first_opened.empty()) first_opened = folder;
        ++opened;
    }
    std::printf("open: %zu tiles opened by folder, by tile.ted and by entry, %zu differ, %zu cannot be opened (identical errors)\n", opened, different, unopenable);
    if (show_time) std::printf("time: opens %.1f s\n", seconds_since(t_open));

    // ---- 4. outside a kit: no entry, a warning
    if (!first_opened.empty()) {
        const fs::path scratch = fs::temp_directory_path() / "wce_tile_entry_check_outside";
        fs::remove_all(scratch, ec);
        fs::create_directories(scratch, ec);
        fs::copy_file(first_opened / "tile.ted", scratch / "tile.ted", fs::copy_options::overwrite_existing, ec);
        if (fs::exists(first_opened / "height_map_0.png", ec)) fs::copy_file(first_opened / "height_map_0.png", scratch / "height_map_0.png", fs::copy_options::overwrite_existing, ec);
        try {
            const auto p = TileProject::open(scratch);
            check(!p->has_entry() && !p->info.found && !p->warning.empty(), "a tile.ted outside the kit opens with a warning and no entry");
            std::printf("outside a kit: opens without an entry, info.found %d, warning: %s\n", p->info.found ? 1 : 0, p->warning.empty() ? "(none)" : "yes");
        } catch (const std::exception& ex) {
            check(false, std::string("open outside the kit: ") + ex.what());
        }
        fs::remove_all(scratch, ec);
    }
    std::printf(g_failed ? "%d checks FAILED\n" : "all passed\n", g_failed);
    return g_failed ? 1 : 0;
}
