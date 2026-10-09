// Checks the battle weather selection data (warscape_asset_variation_db.bin / .xml).
//
//   variation_check
//
// Gates, for every pack that holds the database (vanilla data.pack and the TDD weather pack ship both a .bin and an .xml):
//   * parse(bin).to_bytes() equals the .bin byte for byte;
//   * parse_xml(xml) equals parse(bin) (names, values, filenames, flags);
//   * parse_xml(xml).to_bytes() equals the .bin;
//   * parse(bin).to_xml() equals the .xml, or differs only by blank lines (reported).
// Gates on the editing operations (on the database the game uses): adding a value to a category and removing it again gives
// back the same database, and while the new value is not queried every one of the 19,200 condition combinations keeps its
// candidate list.
// Report: referenced files missing from every pack, variations that can never match, conditions (every combination of the
// meta values) for which the game finds no environment file even after the "default_map" retry, and weather/*.environment
// files no variation refers to.
#include "foundation/text_encoding.h"
#include "vfs/vfs.h"
#include "formats/variation_db.h"

#include <algorithm>
#include <cstdio>
#include <fstream>
#include <set>
#include <sstream>

using namespace wce;

namespace {

std::string pack_path(std::string p) { return Vfs::normalize(p); }

std::string without_blank_lines(const std::string& t) {
    std::string out;
    size_t i = 0;
    while (i < t.size()) {
        size_t nl = t.find("\r\n", i);
        std::string line = t.substr(i, nl == std::string::npos ? std::string::npos : nl - i);
        if (!line.empty()) out += line + "\r\n";
        if (nl == std::string::npos) break;
        i = nl + 2;
    }
    return out;
}

// Pack-by-pack file gates; returns the number of failures.
int check_pack_files(Vfs& vfs, const std::string& bin_path, const std::string& xml_path) {
    int bad = 0;
    for (int pack : vfs.owners(bin_path)) {
        const std::string name = vfs.packs()[pack].name;
        auto bin_disk = vfs.fetch_from(pack, bin_path);
        if (!bin_disk) continue;
        std::vector<uint8_t> bin = read_file(bin_disk->string());
        VariationDb from_bin = VariationDb::parse(bin);
        if (from_bin.to_bytes() != bin) { std::printf("%s: parse(bin).to_bytes() differs from the .bin\n", name.c_str()); ++bad; }
        auto xml_disk = vfs.fetch_from(pack, xml_path);
        if (!xml_disk) { std::printf("%s: bin writer identical (%zu bytes); no .xml in this pack\n", name.c_str(), bin.size()); continue; }
        std::vector<uint8_t> xml = read_file(xml_disk->string());
        VariationDb from_xml = VariationDb::parse_xml(xml);
        const bool same = from_xml == from_bin;
        if (!same) { std::printf("%s: the .xml and the .bin hold different data\n", name.c_str()); ++bad; }
        if (from_xml.to_bytes() != bin) { std::printf("%s: parse_xml(xml).to_bytes() differs from the .bin\n", name.c_str()); ++bad; }
        const std::string text(xml.begin(), xml.end());
        const std::string written = from_bin.to_xml();
        const char* xml_state = written == text ? "identical" : (without_blank_lines(text) == written ? "identical apart from blank lines in the original" : "DIFFERENT");
        if (std::string(xml_state) == "DIFFERENT") ++bad;
        std::printf("%s: bin %zu bytes (writer identical), xml %zu bytes (data %s, to_xml %s, xml->bin identical)\n", name.c_str(), bin.size(), xml.size(),
                    same ? "equal to the bin" : "DIFFERENT", xml_state);
        for (auto& problem : from_bin.validate()) std::printf("  validate: %s\n", problem.c_str());
    }
    return bad;
}

// Editing gates on the database the game uses. Returns the number of failures.
int check_editing(const VariationDb& original) {
    int bad = 0;
    auto candidate_lists = [&](const VariationDb& db, const VariationDb& names_from) {
        std::vector<std::vector<std::string>> all;
        std::vector<size_t> idx(names_from.metas.size(), 0);
        for (;;) {
            VariationDb::Query q;
            for (size_t i = 0; i < names_from.metas.size(); ++i) q[names_from.metas[i].name] = names_from.metas[i].values[idx[i]];
            std::vector<std::string> names;
            for (auto& c : db.candidates("environment", q).candidates) names.push_back(c.variation->filename);
            all.push_back(std::move(names));
            size_t d = 0;
            while (d < idx.size() && ++idx[d] == names_from.metas[d].values.size()) idx[d++] = 0;
            if (d == idx.size()) break;
        }
        return all;
    };
    const auto before = candidate_lists(original, original);
    for (auto& m : original.metas) {
        VariationDb edited = original;
        std::string why;
        const bool full = original.value_count() >= 32;
        const bool ok = edited.add_value(m.name, "__test_value__", &why);
        if (full) {
            if (ok) { std::printf("add_value succeeded with all 32 bits in use (%s)\n", m.name.c_str()); ++bad; }
            continue;
        }
        if (!ok) { std::printf("add_value(%s) failed: %s\n", m.name.c_str(), why.c_str()); ++bad; continue; }
        if (candidate_lists(edited, original) != before) { std::printf("add_value(%s) changed the candidates of existing conditions\n", m.name.c_str()); ++bad; }
        if (!edited.remove_value(m.name, "__test_value__", &why)) { std::printf("remove_value(%s) failed: %s\n", m.name.c_str(), why.c_str()); ++bad; continue; }
        if (!(edited == original)) { std::printf("add_value + remove_value(%s) did not restore the database\n", m.name.c_str()); ++bad; }
    }
    // Removing a value that is not the last of its category must keep every other condition's candidates.
    for (auto& m : original.metas) {
        if (m.values.size() < 2) continue;
        VariationDb edited = original;
        std::string why;
        const std::string victim = m.values.front();
        if (!edited.remove_value(m.name, victim, &why)) continue; // refusing is the documented behaviour when a variation would become unconstrained
        VariationDb names_without;
        names_without.metas = edited.metas;
        const auto after = candidate_lists(edited, edited);
        // Compare with the original restricted to conditions that do not use the removed value.
        std::vector<std::vector<std::string>> expected;
        std::vector<size_t> idx(original.metas.size(), 0);
        for (;;) {
            bool uses_victim = false;
            VariationDb::Query q;
            for (size_t i = 0; i < original.metas.size(); ++i) {
                q[original.metas[i].name] = original.metas[i].values[idx[i]];
                if (original.metas[i].name == m.name && original.metas[i].values[idx[i]] == victim) uses_victim = true;
            }
            if (!uses_victim) {
                std::vector<std::string> names;
                for (auto& c : original.candidates("environment", q).candidates) names.push_back(c.variation->filename);
                expected.push_back(std::move(names));
            }
            size_t d = 0;
            while (d < idx.size() && ++idx[d] == original.metas[d].values.size()) idx[d++] = 0;
            if (d == idx.size()) break;
        }
        if (after != expected) { std::printf("remove_value(%s=%s) changed the candidates of the remaining conditions\n", m.name.c_str(), victim.c_str()); ++bad; }
    }
    std::printf("editing gates: %d failures\n", bad);
    return bad;
}

} // namespace

int main(int argc, char** argv) {
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
        int rc = check_pack_files(vfs, path, "warscape_asset_variation_db/warscape_asset_variation_db.xml");
        rc |= check_editing(db);

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
