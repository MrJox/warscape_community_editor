#pragma once
// Index of the tile database entries of the assembly kits' raw_data folders: one row per _tile_database/TILES/*.xml with what a tile list shows
// (location, set, size in cells, colour, variations, link counts) and the entry's file. Opening a tile resolves its entry from here instead of
// reading the 699 (battle) / 1,152 (campaign) files, and the tile browser lists it.
//
// The index is built once per process and raw_data folder. With a cache file the rows are keyed by file name, size and modification time: only
// new or changed entries are parsed again, so a warm start reads no entry at all (a directory listing gives sizes and times).
#include <cstdint>
#include <filesystem>
#include <map>
#include <memory>
#include <mutex>
#include <string>
#include <string_view>
#include <vector>

namespace wce {

struct TileIndexEntry {
    std::string name;                       // file name without ".xml" (e.g. assembly_kit_osgiliath_test)
    std::filesystem::path xml;              // the entry file
    std::string kind;                       // battle | campaign (from the location)
    std::string location;                   // TILE@location as written (backslashes)
    std::string key;                        // the location normalised (TileEntry::normalize_location)
    std::string tile_set;
    uint32_t width = 0, height = 0;         // cells
    float red = 0, green = 0, blue = 0;     // 0..255
    std::vector<std::string> variations;    // leaf folder names
    uint32_t links = 0, targets = 0;        // TILE_LINK / TILE_LINK_TARGET counts
    uint64_t size = 0;                      // of the entry file
    int64_t mtime = 0;                      // last write time ticks, as the file system gives them
};

class TileIndex {
public:
    // The entries of one raw_data folder (<raw_root>/terrain/tiles/<kind>/_tile_database/TILES/*.xml for every kind). `cache` is a file the rows are kept in
    // between runs; empty = no cache. Unreadable entries are skipped and listed in `problems`.
    static std::shared_ptr<const TileIndex> build(const std::filesystem::path& raw_root, const std::filesystem::path& cache = {});
    // The shared per-folder index of this process (built on first use; thread safe). The cache folder, when set (once, at start-up), keeps
    // tile_index_<hash of the folder>.tsv files.
    static std::shared_ptr<const TileIndex> shared(const std::filesystem::path& raw_root);
    static void set_cache_dir(const std::filesystem::path& dir);
    static void forget_shared();            // drops the shared indices (the files changed on disk)

    const std::filesystem::path& raw_root() const { return raw_root_; }
    const std::vector<TileIndexEntry>& entries() const { return entries_; }
    const TileIndexEntry* find_location(std::string_view location) const;      // by TILE@location, any spelling
    const TileIndexEntry* find_name(std::string_view name) const;              // by file name (without .xml) or by "<set>/<tile>" (case-insensitive)

    struct Resolved {
        const TileIndexEntry* entry = nullptr;
        int variation = -1;                 // index in entry->variations, -1 when the path names the tile folder itself
    };
    // A variation folder, a tile.ted or a tile folder of the kit -> its entry and variation (entry null when the tile has none).
    Resolved resolve(const std::filesystem::path& path) const;

    // How the index was built (for the log and the gate).
    size_t parsed = 0, from_cache = 0, removed = 0;
    std::vector<std::string> problems;
    std::vector<std::string> duplicate_locations;   // locations claimed by more than one entry

private:
    std::filesystem::path raw_root_;
    std::vector<TileIndexEntry> entries_;
    std::multimap<std::string, size_t> by_key_;
};

} // namespace wce
