#include "formats/tile_index.h"
#include "formats/tile_entry.h"
#include "foundation/process.h"
#include "foundation/text_encoding.h"
#include "foundation/thread_pool.h"

#include <algorithm>
#include <cctype>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <functional>
#include <map>
#include <sstream>
#include <unordered_map>

namespace fs = std::filesystem;

namespace wce {

namespace {

std::mutex g_mutex;
std::map<std::string, std::shared_ptr<const TileIndex>> g_shared;
fs::path g_cache_dir;

std::string lower(std::string s) {
    for (char& c : s) c = char(std::tolower(static_cast<unsigned char>(c)));
    return s;
}

std::string generic_lower(const fs::path& p) { return TileEntry::normalize_location(narrow(p.generic_wstring())); }

std::vector<std::string> split(const std::string& s, char sep) {
    std::vector<std::string> out;
    size_t at = 0;
    while (at <= s.size()) {
        size_t e = s.find(sep, at);
        if (e == std::string::npos) e = s.size();
        out.push_back(s.substr(at, e - at));
        at = e + 1;
    }
    return out;
}

const char* kCacheMagic = "wce-tile-index\t1";

struct Candidate {
    fs::path xml;
    std::string rel;                        // relative to raw_root, '/'
    uint64_t size = 0;
    int64_t mtime = 0;
};

// One TSV row per entry: rel, size, mtime, location, tile_set, width, height, red, green, blue, links, targets, variations ('|' separated)
std::string row_of(const TileIndexEntry& e, const std::string& rel) {
    char b[160];
    std::snprintf(b, sizeof b, "\t%llu\t%lld\t", (unsigned long long)e.size, (long long)e.mtime);
    std::string s = rel + b + e.location + "\t" + e.tile_set + "\t";
    std::snprintf(b, sizeof b, "%u\t%u\t%.6f\t%.6f\t%.6f\t%u\t%u\t", e.width, e.height, double(e.red), double(e.green), double(e.blue), e.links, e.targets);
    s += b;
    for (size_t i = 0; i < e.variations.size(); ++i) s += (i ? "|" : "") + e.variations[i];
    return s;
}

bool fill_from_row(TileIndexEntry& e, const std::vector<std::string>& f) {
    if (f.size() != 13) return false;
    e.size = std::strtoull(f[1].c_str(), nullptr, 10);
    e.mtime = std::strtoll(f[2].c_str(), nullptr, 10);
    e.location = f[3];
    e.tile_set = f[4];
    e.width = uint32_t(std::strtoul(f[5].c_str(), nullptr, 10));
    e.height = uint32_t(std::strtoul(f[6].c_str(), nullptr, 10));
    e.red = std::strtof(f[7].c_str(), nullptr);
    e.green = std::strtof(f[8].c_str(), nullptr);
    e.blue = std::strtof(f[9].c_str(), nullptr);
    e.links = uint32_t(std::strtoul(f[10].c_str(), nullptr, 10));
    e.targets = uint32_t(std::strtoul(f[11].c_str(), nullptr, 10));
    e.variations = f[12].empty() ? std::vector<std::string>{} : split(f[12], '|');
    return true;
}

} // namespace

std::shared_ptr<const TileIndex> TileIndex::build(const fs::path& raw_root, const fs::path& cache) {
    auto index = std::make_shared<TileIndex>();
    index->raw_root_ = raw_root;
    std::error_code ec;
    // 1. what is on disk
    std::vector<Candidate> found;
    const fs::path tiles = raw_root / "terrain" / "tiles";
    if (fs::is_directory(tiles, ec))
        for (const fs::directory_entry& kind : fs::directory_iterator(tiles, ec)) {
            const fs::path dir = kind.path() / "_tile_database" / "TILES";
            if (!fs::is_directory(dir, ec)) continue;
            for (const fs::directory_entry& f : fs::directory_iterator(dir, ec)) {
                if (!f.is_regular_file(ec) || lower(f.path().extension().string()) != ".xml") continue;
                Candidate c;
                c.xml = f.path();
                c.rel = narrow(fs::relative(f.path(), raw_root, ec).generic_wstring());
                c.size = f.file_size(ec);
                c.mtime = int64_t(f.last_write_time(ec).time_since_epoch().count());
                found.push_back(std::move(c));
            }
        }
    std::sort(found.begin(), found.end(), [](const Candidate& a, const Candidate& b) { return a.rel < b.rel; });
    // 2. the cache
    std::unordered_map<std::string, std::vector<std::string>> cached;
    if (!cache.empty()) {
        std::ifstream in(cache, std::ios::binary);
        std::string line;
        if (in && std::getline(in, line) && line.compare(0, std::char_traits<char>::length(kCacheMagic), kCacheMagic) == 0)
            while (std::getline(in, line)) {
                if (!line.empty() && line.back() == '\r') line.pop_back();
                std::vector<std::string> f = split(line, '\t');
                if (!f.empty()) cached[f[0]] = std::move(f);
            }
    }
    // 3. rows: from the cache when the file is the one it describes, else parsed
    index->entries_.resize(found.size());
    std::vector<std::string> errors(found.size());
    std::vector<uint8_t> parsed(found.size(), 0);
    compute_pool().parallel_for(found.size(), 16, [&](size_t b, size_t e, int) {
        for (size_t i = b; i < e; ++i) {
            const Candidate& c = found[i];
            TileIndexEntry& row = index->entries_[i];
            row.xml = c.xml;
            row.name = c.xml.stem().string();
            auto it = cached.find(c.rel);
            if (it != cached.end() && fill_from_row(row, it->second) && row.size == c.size && row.mtime == c.mtime) {
                parsed[i] = 0;
            } else {
                parsed[i] = 1;
                try {
                    const TileEntry t = TileEntry::load(c.xml);
                    row.size = c.size;
                    row.mtime = c.mtime;
                    row.location = t.location();
                    row.tile_set = t.tile_set();
                    row.width = t.width();
                    row.height = t.height();
                    row.red = float(t.red());
                    row.green = float(t.green());
                    row.blue = float(t.blue());
                    row.variations.clear();
                    for (const TileVariation& v : t.variations()) row.variations.push_back(v.location);
                    row.links = uint32_t(t.links().size());
                    row.targets = uint32_t(t.link_targets().size());
                } catch (const std::exception& ex) {
                    errors[i] = ex.what();
                    row.location.clear();
                }
            }
            row.key = TileEntry::normalize_location(row.location);
            const std::string prefix = "terrain/tiles/";
            if (row.key.compare(0, prefix.size(), prefix) == 0) {
                const size_t slash = row.key.find('/', prefix.size());
                row.kind = row.key.substr(prefix.size(), slash == std::string::npos ? std::string::npos : slash - prefix.size());
            }
        }
    });
    // 4. drop the unreadable, count, sort, write the cache back
    std::vector<TileIndexEntry> rows;
    std::vector<std::string> kept_rel;
    for (size_t i = 0; i < found.size(); ++i) {
        if (!errors[i].empty() || index->entries_[i].key.empty()) {
            index->problems.push_back(found[i].rel + ": " + (errors[i].empty() ? "no location" : errors[i]));
            continue;
        }
        (parsed[i] ? index->parsed : index->from_cache)++;
        rows.push_back(std::move(index->entries_[i]));
        kept_rel.push_back(found[i].rel);
    }
    for (const auto& [rel, f] : cached) {
        (void)f;
        if (!std::binary_search(found.begin(), found.end(), Candidate{{}, rel, 0, 0}, [](const Candidate& a, const Candidate& b) { return a.rel < b.rel; })) ++index->removed;
    }
    if (!cache.empty() && (index->parsed > 0 || index->removed > 0 || cached.empty())) {
        fs::create_directories(cache.parent_path(), ec);
        std::ofstream out(cache, std::ios::binary | std::ios::trunc);
        if (out) {
            out << kCacheMagic << '\t' << narrow(raw_root.generic_wstring()) << '\n';
            for (size_t i = 0; i < rows.size(); ++i) out << row_of(rows[i], kept_rel[i]) << '\n';
        }
    }
    // the list order of the browser: by location, then by file name
    std::stable_sort(rows.begin(), rows.end(), [](const TileIndexEntry& a, const TileIndexEntry& b) { return a.key != b.key ? a.key < b.key : a.name < b.name; });
    index->entries_ = std::move(rows);
    for (size_t i = 0; i < index->entries_.size(); ++i) {
        if (i && index->entries_[i].key == index->entries_[i - 1].key) index->duplicate_locations.push_back(index->entries_[i].key);
        index->by_key_.emplace(index->entries_[i].key, i);
    }
    return index;
}

void TileIndex::set_cache_dir(const fs::path& dir) {
    std::lock_guard<std::mutex> lock(g_mutex);
    g_cache_dir = dir;
}

void TileIndex::forget_shared() {
    std::lock_guard<std::mutex> lock(g_mutex);
    g_shared.clear();
}

std::shared_ptr<const TileIndex> TileIndex::shared(const fs::path& raw_root) {
    const std::string key = generic_lower(raw_root);
    fs::path cache_dir;
    {
        std::lock_guard<std::mutex> lock(g_mutex);
        auto it = g_shared.find(key);
        if (it != g_shared.end()) return it->second;
        cache_dir = g_cache_dir;
    }
    fs::path cache;
    if (!cache_dir.empty()) {
        char h[32];
        std::snprintf(h, sizeof h, "%016llx", (unsigned long long)std::hash<std::string>{}(key));
        cache = cache_dir / (std::string("tile_index_") + h + ".tsv");
    }
    std::shared_ptr<const TileIndex> built = build(raw_root, cache);   // outside the lock: a second thread may build the same one, the first result wins
    std::lock_guard<std::mutex> lock(g_mutex);
    return g_shared.emplace(key, built).first->second;
}

const TileIndexEntry* TileIndex::find_location(std::string_view location) const {
    const auto it = by_key_.find(TileEntry::normalize_location(location));
    return it == by_key_.end() ? nullptr : &entries_[it->second];
}

const TileIndexEntry* TileIndex::find_name(std::string_view name) const {
    const std::string want = TileEntry::normalize_location(name);
    if (want.empty()) return nullptr;
    for (const TileIndexEntry& e : entries_)
        if (lower(e.name) == want) return &e;
    if (const TileIndexEntry* e = find_location(want)) return e;
    const std::string tail = "/" + want;
    for (const TileIndexEntry& e : entries_)
        if (e.key.size() > tail.size() && e.key.compare(e.key.size() - tail.size(), tail.size(), tail) == 0) return &e;
    return nullptr;
}

TileIndex::Resolved TileIndex::resolve(const fs::path& path) const {
    std::string g = generic_lower(path);
    const size_t at = g.find("terrain/tiles/");
    if (at == std::string::npos) return {};
    g = g.substr(at);
    const std::string leaf = "/tile.ted";
    if (g.size() > leaf.size() && g.compare(g.size() - leaf.size(), leaf.size(), leaf) == 0) g.resize(g.size() - leaf.size());
    // the variation is the last folder (or the last two when a variation location has a slash), else the path is the tile folder itself
    std::string loc = g, var;
    for (int up = 0; up < 3; ++up) {
        if (const TileIndexEntry* e = find_location(loc)) {
            if (up == 0) return {e, -1};
            for (size_t i = 0; i < e->variations.size(); ++i)
                if (TileEntry::normalize_location(e->variations[i]) == var) return {e, int(i)};
            return {e, -1};
        }
        const size_t slash = loc.rfind('/');
        if (slash == std::string::npos) break;
        var = up == 0 ? loc.substr(slash + 1) : loc.substr(slash + 1) + "/" + var;
        loc.resize(slash);
    }
    return {};
}

} // namespace wce
