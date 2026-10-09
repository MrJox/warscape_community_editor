#include "foundation/thread_pool.h"
#include "foundation/process.h"
#include "foundation/text_encoding.h"
#include "foundation/profile.h"
#include "formats/tile_index.h"
#include "tile/tile_project.h"

#include <algorithm>
#include <cstdlib>
#include <cwctype>
#include <fstream>
#include <iterator>
#include <stdexcept>

namespace fs = std::filesystem;

namespace wce {

static std::vector<uint8_t> slurp(const fs::path& p) { return read_file(narrow(p.wstring())); }

static bool spit(const fs::path& p, const std::vector<uint8_t>& b) {
    std::ofstream f(p, std::ios::binary);
    if (!f) return false;
    f.write(reinterpret_cast<const char*>(b.data()), std::streamsize(b.size()));
    return bool(f);
}

fs::path kit_raw_root(const fs::path& path) {
    std::wstring g = path.generic_wstring(), l = g;
    for (wchar_t& c : l) c = wchar_t(std::towlower(c));
    const size_t at = l.find(L"/terrain/tiles/");
    return at == std::wstring::npos ? fs::path() : fs::path(g.substr(0, at));
}

// The numbers of TileInfo from the entry and one of its variations (-1: the tile folder itself, no variation numbers).
static void fill_info(const TileEntry& e, int variation, TileInfo& info) {
    info.location = e.location();
    info.cells_x = e.width();
    info.cells_y = e.height();
    const std::vector<TileVariation> vars = e.variations();
    if (variation < 0 || size_t(variation) >= vars.size()) return;
    const TileVariation& v = vars[size_t(variation)];
    info.variation = v.location;
    info.min_height = v.min_height;
    info.scale = v.scale;
    info.normal_strength = v.normal_strength;
    info.overlap_border_size = v.overlap_border_size;
    info.raw_data_tri_density = v.raw_data_tri_density;
    info.found = true;
}

TileInfo find_tile_info(const fs::path& tile_dir, std::unique_ptr<TileEntry>* entry_out, int* variation_out, fs::path* raw_root_out) {
    TileInfo info;
    const fs::path root = kit_raw_root(tile_dir);
    if (root.empty()) return info;
    const std::shared_ptr<const TileIndex> index = TileIndex::shared(root);
    const TileIndex::Resolved r = index->resolve(tile_dir);
    if (!r.entry) return info;
    try {
        auto entry = std::make_unique<TileEntry>(TileEntry::load(r.entry->xml));
        fill_info(*entry, r.variation, info);
        if (info.variation.empty()) {   // the folder is not one of the entry's variations: say which tile it is anyway
            const std::string g = tile_dir.generic_string();
            info.variation = g.substr(g.rfind('/') + 1);
        }
        if (variation_out) *variation_out = r.variation;
        if (raw_root_out) *raw_root_out = root;
        if (entry_out) *entry_out = std::move(entry);
    } catch (const std::exception&) {
        info = TileInfo();
    }
    return info;
}

// Reads the three parts of a tile side by side: the scene (the long one), the height map (needs the entry's min_height / scale) and, when the caller
// did not bring it, the entry.
static std::unique_ptr<TileProject> open_dir(std::unique_ptr<TileProject> p) {
    WCE_PROF("tile.open");
    const fs::path tile_dir = p->dir;
    fs::path ted = tile_dir / "tile.ted";
    if (!fs::exists(ted)) throw std::runtime_error("no tile.ted in " + tile_dir.string());
    std::exception_ptr failure[2];
    compute_pool().parallel_for(2, 1, [&](size_t b, size_t e, int) {
        for (size_t task = b; task < e; ++task) {
            try {
                if (task == 0) {
                    std::vector<uint8_t> ted_bytes;
                    { WCE_PROF("tile.open.slurp_ted"); ted_bytes = slurp(ted); }
                    { WCE_PROF("tile.open.parse_ted"); p->scene = TedScene::parse(std::move(ted_bytes)); }
                } else {
                    if (!p->entry) { WCE_PROF("tile.open.tile_info"); p->info = find_tile_info(tile_dir, &p->entry, &p->variation, &p->raw_root); }
                    fs::path hm = tile_dir / "height_map_0.png";
                    if (fs::exists(hm)) {
                        WCE_PROF("tile.open.height");
                        p->height_png = PngFile::parse(slurp(hm));
                        p->height = std::make_unique<HeightField>(HeightField::from_png(p->height_png, p->info.min_height, p->info.scale));
                        p->has_height = true;
                    }
                }
            } catch (...) {
                failure[task] = std::current_exception();
            }
        }
    });
    for (auto& f : failure)
        if (f) std::rethrow_exception(f);
    if (!p->entry)
        p->warning = "no tile database entry for " + tile_dir.string() + ": the tile opens without its height scale, size, colour and links";
    else if (!p->info.found)
        p->warning = "the folder '" + tile_dir.filename().string() + "' is not a variation of the tile database entry " + p->entry->file().filename().string();
    return p;
}

std::unique_ptr<TileProject> TileProject::open(const fs::path& path) {
    auto p = std::make_unique<TileProject>();
    std::error_code ec;
    p->dir = fs::is_regular_file(path, ec) && path.filename() == "tile.ted" ? path.parent_path() : path;
    if (!fs::exists(p->dir / "tile.ted", ec)) {
        // a tile folder that holds its variations: the first one (the entry lists them in order)
        const fs::path root = kit_raw_root(p->dir);
        if (!root.empty()) {
            const std::shared_ptr<const TileIndex> index = TileIndex::shared(root);
            const TileIndex::Resolved r = index->resolve(p->dir);
            if (r.entry && r.variation < 0 && !r.entry->variations.empty()) {
                auto e = std::make_unique<TileEntry>(TileEntry::load(r.entry->xml));
                return open_entry_resolved(std::move(e), 0, root);
            }
        }
    }
    return open_dir(std::move(p));
}

std::unique_ptr<TileProject> TileProject::open_entry(const fs::path& entry_xml, int variation) {
    auto e = std::make_unique<TileEntry>(TileEntry::load(entry_xml));
    const fs::path root = e->raw_root();
    if (root.empty()) throw std::runtime_error(entry_xml.string() + " is not inside <kit>/raw_data/terrain/tiles/<kind>/_tile_database/TILES");
    return open_entry_resolved(std::move(e), variation, root);
}

std::unique_ptr<TileProject> TileProject::open_entry_resolved(std::unique_ptr<TileEntry> e, int variation, const fs::path& root) {
    const std::vector<TileVariation> vars = e->variations();
    if (vars.empty()) throw std::runtime_error(e->file().string() + " lists no variations");
    const int v = variation < 0 ? 0 : variation;
    if (size_t(v) >= vars.size()) throw std::runtime_error(e->file().filename().string() + " has " + std::to_string(vars.size()) + " variations, no number " + std::to_string(v));
    auto p = std::make_unique<TileProject>();
    p->dir = e->variation_folder(root, size_t(v));
    p->raw_root = root;
    p->variation = v;
    fill_info(*e, v, p->info);
    p->entry = std::move(e);
    return open_dir(std::move(p));
}

bool TileProject::save(const fs::path& out_dir, std::string* message, bool force_scene, bool force_height) const {
    std::string msg;
    bool wrote = false;
    fs::create_directories(out_dir);
    if (scene.modified() || force_scene) {
        fs::path target = out_dir / "tile.ted";
        if (fs::exists(target) && !fs::exists(target.string() + ".bak")) fs::copy_file(target, target.string() + ".bak");
        if (!spit(target, scene.save())) { if (message) *message = "could not write " + target.string(); return false; }
        msg += "tile.ted saved. ";
        wrote = true;
    }
    if (has_height && (height_dirty || force_height)) {
        fs::path target = out_dir / "height_map_0.png";
        if (fs::exists(target) && !fs::exists(target.string() + ".bak")) fs::copy_file(target, target.string() + ".bak");
        PngFile png = height->to_png(height_png);
        std::vector<uint8_t> bytes = png.has_ancillary ? png.encode() : png.write();
        if (!spit(target, bytes)) { if (message) *message = "could not write " + target.string(); return false; }
        msg += png.has_ancillary ? "height_map_0.png saved (source had extra chunks: re-encoded). " : "height_map_0.png saved. ";
        wrote = true;
    }
    if (message) *message = wrote ? msg : "nothing to save";
    return wrote;
}

} // namespace wce
