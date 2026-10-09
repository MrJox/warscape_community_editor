#include "foundation/thread_pool.h"
#include "foundation/process.h"
#include "foundation/text_encoding.h"
#include "foundation/profile.h"
#include "tile/tile_project.h"

#include <algorithm>
#include <cstdlib>
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

TileInfo find_tile_info(const fs::path& tile_dir) {
    TileInfo info;
    // split at "terrain/tiles/<battle|campaign>/"
    std::string generic = tile_dir.generic_string();
    size_t at = generic.find("/terrain/tiles/");
    if (at == std::string::npos) return info;
    std::string root = generic.substr(0, at);
    std::string rest = generic.substr(at + 1);          // terrain/tiles/battle/...
    size_t third = rest.find('/', std::string("terrain/tiles/").size());
    if (third == std::string::npos) return info;
    std::string kind_dir = rest.substr(0, third);        // terrain/tiles/battle
    size_t leaf = rest.rfind('/');
    std::string tile_loc = rest.substr(0, leaf), variation = rest.substr(leaf + 1);
    for (char& c : tile_loc) if (c == '/') c = char(92);
    fs::path tiles = fs::path(root) / kind_dir / "_tile_database" / "TILES";
    if (!fs::is_directory(tiles)) return info;
    for (auto& e : fs::directory_iterator(tiles)) {
        if (e.path().extension() != ".xml") continue;
        try {
            wce::XmlDoc doc = wce::XmlDoc::parse(slurp(e.path()));
            int r = doc.roots().at(0);
            if (doc.get(r, "location").value_or("") != tile_loc) continue;
            info.location = tile_loc;
            info.variation = variation;
            info.cells_x = uint32_t(std::atoi(doc.get(r, "width").value_or("0").c_str()));
            info.cells_y = uint32_t(std::atoi(doc.get(r, "height").value_or("0").c_str()));
            for (int c : doc.elements()[r].children) {
                if (doc.elements()[c].name != "VARIATIONS") continue;
                for (int v : doc.elements()[c].children) {
                    if (doc.get(v, "location").value_or("") != variation) continue;
                    info.min_height = std::atof(doc.get(v, "min_height").value_or("0").c_str());
                    info.scale = std::atof(doc.get(v, "scale").value_or("100").c_str());
                    info.found = true;
                }
            }
            return info;
        } catch (const std::exception&) {}
    }
    return info;
}

std::unique_ptr<TileProject> TileProject::open(const fs::path& tile_dir) {
    WCE_PROF("tile.open");
    auto p = std::make_unique<TileProject>();
    p->dir = tile_dir;
    fs::path ted = tile_dir / "tile.ted";
    if (!fs::exists(ted)) throw std::runtime_error("no tile.ted in " + tile_dir.string());
    // the scene (the long one) and the tile database + height map are independent: read them side by side
    std::exception_ptr failure[2];
    compute_pool().parallel_for(2, 1, [&](size_t b, size_t e, int) {
        for (size_t task = b; task < e; ++task) {
            try {
                if (task == 0) {
                    std::vector<uint8_t> ted_bytes;
                    { WCE_PROF("tile.open.slurp_ted"); ted_bytes = slurp(ted); }
                    { WCE_PROF("tile.open.parse_ted"); p->scene = TedScene::parse(std::move(ted_bytes)); }
                } else {
                    { WCE_PROF("tile.open.tile_info"); p->info = find_tile_info(tile_dir); }
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
    return p;
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
