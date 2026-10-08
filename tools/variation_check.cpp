// Checks the battle weather selection data (warscape_asset_variation_db.bin).
//
//   variation_check [--xml <export.xml>]
//
// Gate: with --xml the parsed metas, filenames and flags must equal the XML export of the same file.
// Report: referenced files missing from every pack, variations that can never match (a category with no allowed value),
// conditions (every combination of the meta values) for which the game finds no environment file even after the
// "default_map" retry, and weather/*.environment files no variation refers to.
#include "core/vfs.h"
#include "formats/variation_db.h"

#include <algorithm>
#include <cstdio>
#include <fstream>
#include <regex>
#include <set>
#include <sstream>

using namespace we;

namespace {

std::string pack_path(std::string p) { return Vfs::normalize(p); }

int compare_xml(const VariationDb& db, const char* path) {
    std::ifstream in(path, std::ios::binary);
    if (!in) { std::printf("cannot read %s\n", path); return 1; }
    std::stringstream ss;
    ss << in.rdbuf();
    const std::string x = ss.str();
    int bad = 0;
    // metas: <meta_data_entry ... name='map'> <values value='a' value='b'/>
    std::regex meta_re("<meta_data_entry[^>]*name='([^']*)'>\\s*<values([^>]*)/>");
    std::vector<VariationDb::Meta> metas;
    for (auto it = std::sregex_iterator(x.begin(), x.end(), meta_re); it != std::sregex_iterator(); ++it) {
        VariationDb::Meta m;
        m.name = (*it)[1];
        std::string vals = (*it)[2];
        std::regex val_re("value='([^']*)'");
        for (auto v = std::sregex_iterator(vals.begin(), vals.end(), val_re); v != std::sregex_iterator(); ++v) m.values.push_back((*v)[1]);
        metas.push_back(std::move(m));
    }
    if (metas.size() != db.metas.size()) { std::printf("XML: %zu metas, bin: %zu\n", metas.size(), db.metas.size()); ++bad; }
    for (size_t i = 0; i < std::min(metas.size(), db.metas.size()); ++i)
        if (metas[i].name != db.metas[i].name || metas[i].values != db.metas[i].values) { std::printf("meta %zu differs (%s)\n", i, metas[i].name.c_str()); ++bad; }
    std::regex var_re("filename='([^']*)' flags='(\\d+)'");
    std::vector<std::pair<std::string, uint32_t>> xml_vars;
    for (auto it = std::sregex_iterator(x.begin(), x.end(), var_re); it != std::sregex_iterator(); ++it)
        xml_vars.emplace_back((*it)[1], uint32_t(std::stoul((*it)[2])));
    const VariationDb::Entry* e = db.entry("environment");
    size_t n = e ? e->variations.size() : 0;
    if (xml_vars.size() != n) { std::printf("XML: %zu variations, bin: %zu\n", xml_vars.size(), n); ++bad; }
    for (size_t i = 0; i < std::min(xml_vars.size(), n); ++i)
        if (xml_vars[i].first != e->variations[i].filename || xml_vars[i].second != e->variations[i].flags) { std::printf("variation %zu differs: %s\n", i, xml_vars[i].first.c_str()); ++bad; }
    std::printf("XML comparison: %zu metas, %zu variations, %d differences\n", metas.size(), xml_vars.size(), bad);
    return bad ? 1 : 0;
}

} // namespace

int main(int argc, char** argv) {
    const char* xml = nullptr;
    for (int i = 1; i + 1 < argc; ++i)
        if (std::string(argv[i]) == "--xml") xml = argv[++i];
    try {
        Vfs vfs(Vfs::default_config());
        vfs.mount();
        const std::string path = "warscape_asset_variation_db/warscape_asset_variation_db.bin";
        auto owner = vfs.owner(path);
        if (!owner) { std::printf("%s is in no pack\n", path.c_str()); return 1; }
        VariationDb db = VariationDb::parse(vfs.read(path));
        const VariationDb::Entry* env = db.entry("environment");
        if (!env) { std::printf("no 'environment' entry\n"); return 1; }
        int bits = 0;
        for (auto& m : db.metas) bits += int(m.values.size());
        std::printf("%s from %s: %zu metas (%d values, %d of 32 bits), %zu variations\n", path.c_str(), vfs.packs()[*owner].name.c_str(),
                    db.metas.size(), bits, std::min(bits, 32), env->variations.size());
        int rc = xml ? compare_xml(db, xml) : 0;

        std::set<std::string> referenced;
        int missing = 0, impossible = 0;
        for (auto& v : env->variations) {
            const std::string p = pack_path(v.filename);
            referenced.insert(p);
            if (!vfs.owner(p)) { std::printf("MISSING file: %s\n", v.filename.c_str()); ++missing; }
            for (auto& [cat, allowed] : db.decode(v.flags))
                if (allowed.empty()) { std::printf("NEVER matches (no %s allowed): %s\n", cat.c_str(), v.filename.c_str()); ++impossible; break; }
        }

        // Self-consistency: the query made of each variation's own first allowed value per category must find it.
        int self_bad = 0;
        for (auto& v : env->variations) {
            VariationDb::Query q;
            for (auto& [cat, allowed] : db.decode(v.flags))
                if (!allowed.empty()) q[cat] = allowed.front();
            auto r = db.candidates("environment", q);
            bool found = false;
            for (auto& c : r.candidates) found |= c.variation == &v;
            if (!found) { std::printf("NOT found by its own conditions: %s\n", v.filename.c_str()); ++self_bad; }
        }
        std::printf("self-consistency: %zu variations, %d not found by their own conditions\n", env->variations.size(), self_bad);
        if (self_bad) rc = 1;

        // Every combination of the values of all categories.
        std::vector<size_t> idx(db.metas.size(), 0);
        size_t combos = 0, empty = 0, fallback = 0;
        std::map<std::string, size_t> uses;
        std::map<std::string, std::pair<size_t, size_t>> by_value; // "category=value" -> (combinations, without a file)
        for (;;) {
            VariationDb::Query q;
            for (size_t i = 0; i < db.metas.size(); ++i) q[db.metas[i].name] = db.metas[i].values[idx[i]];
            auto r = db.candidates("environment", q);
            ++combos;
            for (size_t i = 0; i < db.metas.size(); ++i) {
                auto& c = by_value[db.metas[i].name + "=" + db.metas[i].values[idx[i]]];
                ++c.first;
                if (r.candidates.empty()) ++c.second;
            }
            if (r.candidates.empty()) {
                ++empty;
            } else {
                if (r.used_default_map) ++fallback;
                for (auto& c : r.candidates) ++uses[pack_path(c.variation->filename)];
            }
            size_t d = 0;
            while (d < idx.size() && ++idx[d] == db.metas[d].values.size()) idx[d++] = 0;
            if (d == idx.size()) break;
        }
        std::printf("%zu condition combinations: %zu find no file, %zu need the default_map retry\n", combos, empty, fallback);
        std::printf("combinations without a file, by value (a hole is data, not a bug: the database has no variation for it):\n");
        for (auto& [k, c] : by_value) std::printf("  %-40s %5zu / %zu\n", k.c_str(), c.second, c.first);

        size_t unused = 0;
        for (auto& f : vfs.list("weather/", ".environment"))
            if (!referenced.count(f)) {
                if (++unused <= 10) std::printf("not referenced by any variation: %s\n", f.c_str());
            }
        std::printf("%zu .environment files under weather/ are not referenced; %zu referenced files never selectable by any condition\n", unused,
                    size_t(std::count_if(referenced.begin(), referenced.end(), [&](const std::string& f) { return !uses.count(f); })));
        std::printf("missing files %d, never-matching variations %d\n", missing, impossible);
        return rc ? rc : (missing || impossible) ? 2 : 0;
    } catch (const std::exception& e) {
        std::printf("error: %s\n", e.what());
        return 1;
    }
}
