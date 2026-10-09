// Phase 0 oracle of the Tile tool parity plan (docs/plans/tile_ted_parity.md): compares the world position / orientation the tool computes for every
// BUILDING_INSTANCE (TileDocument::world_matrix, TedScene positions and the height map) with TEd's own stage A export, `building_list.xml`
// (working_data/.../building_list.xml: key, position_x/y/z, position_type, quaternion, scale). Read-only: it reports, it does not judge.
//
// Columns: e_abs = list y - world y (the export's y is absolute: 0 for everything but 15 crates); "above" = world y - the tool's terrain height
// (0 for a landscape building sitting on the height map).
//   placement_check <kit root> [--filter <substring of the tile path>] [--limit N] [--worst N] [--csv <file>]
//
// <kit root> is an assembly_kit_*_tilemap folder (raw_data/... holds tile.ted, working_data/... holds the export). Heights are reported under the
// hypotheses the plan lists: the export's y is absolute (list_y = world_y) or relative to the tool's terrain (list_y = world_y - terrain).
#include "formats/xml_doc.h"
#include "foundation/process.h"
#include "foundation/text_encoding.h"
#include "tile/tile_document.h"
#include "tile/tile_project.h"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <map>
#include <string>
#include <unordered_map>
#include <vector>

namespace fs = std::filesystem;
using namespace wce;

namespace {

struct ListEntry {
    std::string key;
    double x = 0, y = 0, z = 0;
    double q[4] = {0, 0, 0, 1};   // i j k w
    double scale[3] = {1, 1, 1};
    std::string position_type;
    bool used = false;
};

double num(const XmlDoc& d, int e, const char* a, double def = 0) {
    const auto v = d.get(e, a);
    return v ? std::atof(v->c_str()) : def;
}

std::vector<ListEntry> read_list(const fs::path& p) {
    const XmlDoc doc = XmlDoc::parse(read_file(narrow(p.wstring())));
    std::vector<ListEntry> out;
    for (int root : doc.roots())
        for (int c : doc.elements()[size_t(root)].children) {
            if (doc.elements()[size_t(c)].name != "BATTLEFIELD_BUILDING") continue;
            ListEntry e;
            e.key = doc.get(c, "key").value_or("");
            e.x = num(doc, c, "position_x");
            e.y = num(doc, c, "position_y");
            e.z = num(doc, c, "position_z");
            e.position_type = doc.get(c, "position_type").value_or("");
            e.q[0] = num(doc, c, "orientation_i");
            e.q[1] = num(doc, c, "orientation_j");
            e.q[2] = num(doc, c, "orientation_k");
            e.q[3] = num(doc, c, "orientation_w", 1);
            e.scale[0] = num(doc, c, "scale_x", 1);
            e.scale[1] = num(doc, c, "scale_y", 1);
            e.scale[2] = num(doc, c, "scale_z", 1);
            out.push_back(std::move(e));
        }
    return out;
}

Mat3 quat_matrix(const double q[4]) {
    const double x = q[0], y = q[1], z = q[2], w = q[3];
    const double n = std::sqrt(x * x + y * y + z * z + w * w);
    const double X = x / n, Y = y / n, Z = z / n, W = w / n;
    return {1 - 2 * (Y * Y + Z * Z), 2 * (X * Y - Z * W), 2 * (X * Z + Y * W),
            2 * (X * Y + Z * W), 1 - 2 * (X * X + Z * Z), 2 * (Y * Z - X * W),
            2 * (X * Z - Y * W), 2 * (Y * Z + X * W), 1 - 2 * (X * X + Y * Y)};
}

// F * M * F with F = diag(1, 1, -1): the same rotation seen from the frame with z mirrored
Mat3 mirror_z(const Mat3& m) { return {m[0], m[1], -m[2], m[3], m[4], -m[5], -m[6], -m[7], m[8]}; }

double angle_between(const Mat3& a, const Mat3& b) {
    const Mat3 r = mat_mul(mat_transpose(a), b);
    const double c = std::clamp((r[0] + r[4] + r[8] - 1.0) * 0.5, -1.0, 1.0);
    return std::acos(c) * 57.29577951308232;
}

struct Stat {
    std::vector<double> v;
    void add(double x) { v.push_back(x); }
    double mean() const { if (v.empty()) return 0; double s = 0; for (double x : v) s += x; return s / double(v.size()); }
    double median() { if (v.empty()) return 0; std::sort(v.begin(), v.end()); return v[v.size() / 2]; }
    double max_abs() const { double m = 0; for (double x : v) m = std::max(m, std::fabs(x)); return m; }
    double mean_abs() const { if (v.empty()) return 0; double s = 0; for (double x : v) s += std::fabs(x); return s / double(v.size()); }
    double median_abs() const { std::vector<double> a(v); for (double& x : a) x = std::fabs(x); if (a.empty()) return 0; std::sort(a.begin(), a.end()); return a[a.size() / 2]; }
    double signed_median() const { std::vector<double> a(v); if (a.empty()) return 0; std::sort(a.begin(), a.end()); return a[a.size() / 2]; }
};

struct Fit {
    std::string tile;
    size_t n = 0;
    double min = 0, scale = 0, entry_min = 0, entry_scale = 0, span = 0, rms = 0;
};

struct Worst {
    double err;
    std::string what;
};

struct Row {
    std::string tile, key, mode, parent_mode;
    int depth = 0;
    double world_y = 0, list_y = 0, terrain = 0, terrain2 = 0, dx = 0, dz = 0, rot = 0, rot_m = 0, lf_off = 0;
};

} // namespace

int main(int argc, char** argv) {
    if (argc < 2) {
        std::fprintf(stderr, "usage: placement_check <kit root> [--filter s] [--limit N] [--worst N] [--csv file]\n");
        return 2;
    }
    const fs::path kit = fs::path(widen(argv[1]));
    std::string filter, csv;
    size_t limit = SIZE_MAX, worst_n = 20;
    for (int i = 2; i < argc; ++i) {
        const std::string a = argv[i];
        if (a == "--filter" && i + 1 < argc) filter = argv[++i];
        else if (a == "--limit" && i + 1 < argc) limit = size_t(std::atoi(argv[++i]));
        else if (a == "--worst" && i + 1 < argc) worst_n = size_t(std::atoi(argv[++i]));
        else if (a == "--csv" && i + 1 < argc) csv = argv[++i];
    }
    const fs::path raw_root = kit / "raw_data", work_root = kit / "working_data";
    std::vector<fs::path> tiles;
    for (auto it = fs::recursive_directory_iterator(raw_root / "terrain" / "tiles"); it != fs::recursive_directory_iterator(); ++it) {
        if (!it->is_regular_file() || it->path().filename() != "tile.ted") continue;
        const fs::path dir = it->path().parent_path();
        const fs::path list = work_root / fs::relative(dir, raw_root) / "building_list.xml";
        std::error_code ec;
        if (!fs::exists(list, ec) || fs::file_size(list, ec) < 1024) continue;
        if (!filter.empty() && dir.generic_string().find(filter) == std::string::npos) continue;
        tiles.push_back(dir);
    }
    std::sort(tiles.begin(), tiles.end());
    if (tiles.size() > limit) tiles.resize(limit);
    std::printf("placement_check: %zu tiles with a TEd building_list.xml\n", tiles.size());

    std::vector<Row> rows;
    std::vector<Fit> fits;
    size_t unmatched_tile = 0, unmatched_list = 0, matched = 0, ordered = 0, fallback_tiles = 0, no_entry = 0;
    std::printf("\n%-62s %7s %7s %5s  %-7s %9s %9s  %9s %9s  %8s %8s\n", "tile", "list", "match", "order", "landsc.", "med e_abs", "med above", "freef. abs", "med above", "max xz", "max rot");
    for (const fs::path& dir : tiles) {
        try {
            const fs::path list_path = work_root / fs::relative(dir, raw_root) / "building_list.xml";
            std::vector<ListEntry> list = read_list(list_path);
            auto project = TileProject::open(dir);
            if (!project->info.found) ++no_entry;
            TileDocument doc(std::move(project), nullptr);
            const TedScene& sc = doc.tscene();
            std::vector<int> inst;
            for (int e : doc.instances())
                if (sc.type(e) == "BUILDING_INSTANCE") inst.push_back(e);
            std::sort(inst.begin(), inst.end());
            const double extent_z = doc.extent_z();
            // pairing: the same order when the keys line up, else key + position
            std::vector<int> pair(inst.size(), -1);
            bool same_order = inst.size() == list.size();
            for (size_t i = 0; same_order && i < inst.size(); ++i) same_order = sc.doc().get(inst[i], "key").value_or("") == list[i].key;
            if (same_order) {
                for (size_t i = 0; i < inst.size(); ++i) pair[i] = int(i);
                ++ordered;
            } else {
                ++fallback_tiles;
                std::unordered_map<std::string, std::vector<size_t>> by_key;
                for (size_t i = 0; i < list.size(); ++i) by_key[list[i].key].push_back(i);
                for (size_t i = 0; i < inst.size(); ++i) {
                    const float3 p = doc.world_position(inst[i]);
                    auto it = by_key.find(sc.doc().get(inst[i], "key").value_or(""));
                    if (it == by_key.end()) continue;
                    double best = 0.05;
                    int bi = -1;
                    for (size_t li : it->second) {
                        if (list[li].used) continue;
                        const double d = std::hypot(list[li].x - p.x, list[li].z - (extent_z - p.z));
                        if (d < best) { best = d; bi = int(li); }
                    }
                    if (bi >= 0) { pair[i] = bi; list[size_t(bi)].used = true; }
                }
            }
            Stat land_abs, land_rel, free_abs, free_rel, xz, rot;
            size_t n_match = 0;
            double sx = 0, sy = 0, sxx = 0, sxy = 0, syy = 0;   // y = a + b * raw over the landscape buildings (raw: the tool's height sample as the 16-bit value)
            size_t nfit = 0;
            for (size_t i = 0; i < inst.size(); ++i) {
                if (pair[i] < 0) { ++unmatched_tile; continue; }
                const ListEntry& le = list[size_t(pair[i])];
                const int e = inst[i];
                const WorldTransform w = world_transform(sc, e);
                const float3 p = doc.world_position(e);
                Row r;
                r.tile = fs::relative(dir, raw_root / "terrain" / "tiles" / "battle").generic_string();
                r.key = le.key;
                r.mode = sc.doc().get(e, "surface_mode").value_or("");
                r.lf_off = sc.doc().get(e, "lf_offset") ? std::atof(sc.doc().get(e, "lf_offset")->c_str()) : 0;
                for (int a = sc.doc().elements()[size_t(e)].parent; a >= 0; a = sc.doc().elements()[size_t(a)].parent) {
                    const std::string& t = sc.doc().elements()[size_t(a)].name;
                    if (t == "UserInstanceGroup" || t == "INSTANCE_GROUP") {
                        if (r.depth == 0) r.parent_mode = sc.doc().get(a, "surface_mode").value_or("");
                        ++r.depth;
                    }
                }
                r.world_y = p.y;
                r.list_y = le.y;
                r.terrain = doc.terrain_height(p.x, p.z);
                {   // the other candidate formula: the 16-bit sample read as the campaign LF height, raw * lf_height / 65535 - vertical_offset (_settings.xml: 3000 and 20)
                    const TileInfo& ti = doc.project().info;
                    const double raw = ti.scale != 0 ? (r.terrain - ti.min_height) / ti.scale * 65535.0 : 0;
                    r.terrain2 = raw * 3000.0 / 65535.0 - 20.0;
                }
                r.dx = le.x - p.x;
                r.dz = le.z - (extent_z - p.z);
                const Mat3 rl = quat_matrix(le.q);
                r.rot = angle_between(w.rotation, rl);                // the tool's matrix against the export's quaternion as is
                r.rot_m = angle_between(mirror_z(w.rotation), rl);    // against the z mirrored frame
                const double e_abs = le.y - r.world_y, e_rel = r.world_y - r.terrain;
                if (r.mode == "MSM_LANDSCAPE" && doc.project().info.scale != 0) {
                    const TileInfo& ti = doc.project().info;
                    const double raw = (r.terrain - ti.min_height) / ti.scale * 65535.0;
                    sx += raw; sy += r.world_y; sxx += raw * raw; sxy += raw * r.world_y; syy += r.world_y * r.world_y;
                    ++nfit;
                }
                (r.mode == "MSM_LANDSCAPE" ? land_abs : free_abs).add(e_abs);
                (r.mode == "MSM_LANDSCAPE" ? land_rel : free_rel).add(e_rel);
                xz.add(std::hypot(r.dx, r.dz));
                rot.add(std::min(r.rot, r.rot_m));
                rows.push_back(std::move(r));
                ++n_match;
                ++matched;
            }
            for (const ListEntry& le : list) if (!same_order && !le.used) ++unmatched_list;
            if (nfit >= 8) {
                const double n = double(nfit), den = n * sxx - sx * sx;
                if (std::fabs(den) > 1e-6) {
                    Fit f;
                    f.tile = fs::relative(dir, raw_root / "terrain" / "tiles" / "battle").generic_string();
                    f.n = nfit;
                    f.scale = (n * sxy - sx * sy) / den * 65535.0;
                    f.min = (sy - (n * sxy - sx * sy) / den * sx) / n;
                    f.entry_min = doc.project().info.min_height;
                    f.entry_scale = doc.project().info.scale;
                    f.span = (n * sxx - sx * sx) / n / n;           // variance of raw: a fit over a flat tile says nothing
                    f.rms = std::sqrt(std::max(0.0, syy / n - (sy / n) * (sy / n) - (f.scale / 65535.0) * (f.scale / 65535.0) * (sxx / n - (sx / n) * (sx / n))));
                    fits.push_back(std::move(f));
                }
            }
            std::printf("%-62s %7zu %7zu %5s  %-7zu %9.3f %9.3f  %9.3f %9.3f  %8.4f %8.3f\n", rows.empty() ? "?" : rows.back().tile.c_str(), list.size(), n_match, same_order ? "yes" : "no",
                        land_abs.v.size(), land_abs.median(), land_rel.median(), free_abs.median(), free_rel.median(), xz.max_abs(), rot.max_abs());
        } catch (const std::exception& ex) {
            std::printf("%s: %s\n", dir.string().c_str(), ex.what());
        }
    }
    std::printf("\ntiles: %zu in the same order as the export, %zu paired by key and position, %zu without a tile database entry\n", ordered, fallback_tiles, no_entry);
    std::printf("buildings: %zu paired, %zu of the tiles unpaired, %zu of the exports unpaired\n", matched, unmatched_tile, unmatched_list);

    // ---- which height formula fits: y of the landscape buildings against the tool's raw sample, per tile
    std::printf("\nlandscape buildings: y = min + scale * raw / 65535 fitted per tile against the tile database entry (tiles whose fit differs from the entry by more than 5 cm at the low end or 0.5 %% of the scale)\n");
    size_t shown = 0;
    for (const Fit& f : fits) {
        const double dmin = f.min - f.entry_min, dsc = f.scale - f.entry_scale;
        if (std::fabs(dmin) < 0.05 && std::fabs(dsc) < 0.005 * std::fabs(f.entry_scale)) continue;
        std::printf("  %-60s n %6zu  fit min %9.4f scale %9.4f (rms %.3f m) | entry min %9.4f scale %9.4f | d min %8.4f d scale %8.4f\n", f.tile.c_str(), f.n, f.min, f.scale, f.rms, f.entry_min, f.entry_scale, dmin, dsc);
        ++shown;
    }
    std::printf("  %zu of %zu fitted tiles differ; the other %zu agree with the entry's min_height and scale\n", shown, fits.size(), fits.size() - shown);

    // ---- report by surface mode, depth in groups
    auto bucket = [&](auto pred, const char* name) {
        Stat dxz, e_abs, e_rel, e_rel2, rot0, rotm;
        for (const Row& r : rows) {
            if (!pred(r)) continue;
            dxz.add(std::hypot(r.dx, r.dz));
            e_abs.add(r.list_y - r.world_y);
            e_rel.add(r.world_y - r.terrain);
            e_rel2.add(r.world_y - r.terrain2);
            rot0.add(r.rot);
            rotm.add(r.rot_m);
        }
        if (dxz.v.empty()) return;
        std::printf("%-34s n %7zu | xz mean %.4f max %.4f | y absolute: mean|e| %.3f med|e| %.3f max %.3f | world y above the tool's terrain (min_height + scale * raw): mean|d| %.3f med|d| %.3f max %.3f, signed median %.3f | above raw * 3000 / 65535 - 20: med|d| %.3f | rot as is: mean %.3f max %.2f, z mirrored: mean %.3f max %.2f\n",
                    name, dxz.v.size(), dxz.mean_abs(), dxz.max_abs(), e_abs.mean_abs(), e_abs.median_abs(), e_abs.max_abs(), e_rel.mean_abs(), e_rel.median_abs(), e_rel.max_abs(), e_rel.signed_median(), e_rel2.median_abs(), rot0.mean(), rot0.max_abs(),
                    rotm.mean(), rotm.max_abs());
    };
    std::printf("\nby surface mode and depth in groups (units: metres, degrees)\n");
    bucket([](const Row&) { return true; }, "all");
    for (const char* mode : {"MSM_LANDSCAPE", "MSM_FREEFORM"}) {
        bucket([&](const Row& r) { return r.mode == mode; }, mode);
        for (int d = 0; d < 4; ++d) {
            char name[96];
            std::snprintf(name, sizeof name, "%s, depth %d%s", mode, d, d == 3 ? "+" : "");
            bucket([&](const Row& r) { return r.mode == mode && (d == 3 ? r.depth >= 3 : r.depth == d); }, name);
        }
    }
    std::printf("\nby the mode of the nearest group (depth >= 1) against the building's own\n");
    for (const char* own : {"MSM_LANDSCAPE", "MSM_FREEFORM"})
        for (const char* par : {"MSM_LANDSCAPE", "MSM_FREEFORM", ""}) {
            char name[96];
            std::snprintf(name, sizeof name, "own %s / group %s", own + 4, *par ? par + 4 : "(none)");
            bucket([&](const Row& r) { return r.mode == own && r.parent_mode == par && (*par ? r.depth >= 1 : r.depth == 0); }, name);
        }
    std::printf("\nby lf_offset\n");
    bucket([](const Row& r) { return r.lf_off == 0; }, "lf_offset = 0");
    bucket([](const Row& r) { return r.lf_off != 0; }, "lf_offset != 0");

    // ---- the worst ones (by the better of the two y hypotheses for their mode, then by xz, then by rotation)
    auto worst = [&](const char* title, auto score) {
        std::vector<std::pair<double, size_t>> order;
        for (size_t i = 0; i < rows.size(); ++i) order.push_back({score(rows[i]), i});
        std::sort(order.begin(), order.end(), [](auto& a, auto& b) { return a.first > b.first; });
        std::printf("\nworst %zu by %s\n", worst_n, title);
        for (size_t k = 0; k < std::min(worst_n, order.size()); ++k) {
            const Row& r = rows[order[k].second];
            std::printf("  %9.3f  %-54s %-34s %-14s depth %d  y world %.3f list %.3f terrain %.3f  dxz %.4f %.4f rot %.2f / %.2f\n", order[k].first, r.tile.c_str(), r.key.c_str(), r.mode.c_str() + 4, r.depth,
                        r.world_y, r.list_y, r.terrain, r.dx, r.dz, r.rot, r.rot_m);
        }
    };
    worst("|y error| under the better hypothesis", [](const Row& r) { return std::min(std::fabs(r.list_y - r.world_y), std::fabs(r.list_y - (r.world_y - r.terrain))); });
    worst("xz error", [](const Row& r) { return std::hypot(r.dx, r.dz); });
    worst("rotation error (degrees, better of as is / mirrored)", [](const Row& r) { return std::min(r.rot, r.rot_m); });

    if (!csv.empty()) {
        std::ofstream f(csv);
        f << "tile\tkey\tmode\tgroup_mode\tdepth\tlf_offset\tworld_y\tlist_y\tterrain\tdx\tdz\trot\trot_mirrored\n";
        for (const Row& r : rows)
            f << r.tile << '\t' << r.key << '\t' << r.mode << '\t' << r.parent_mode << '\t' << r.depth << '\t' << r.lf_off << '\t' << r.world_y << '\t' << r.list_y << '\t' << r.terrain << '\t' << r.dx << '\t'
              << r.dz << '\t' << r.rot << '\t' << r.rot_m << '\n';
    }
    return 0;
}
