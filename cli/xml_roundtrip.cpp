// Round-trip gate for the lossless XML layer.
//
//   xml_roundtrip <dir|file>... [--schema out.tsv]
//
// For every .environment / .lighting / .xml file:
//   1. parse + save must be byte-identical;
//   2. editing every attribute and saving must re-parse to the same tree with the new values;
//   3. reverting those edits must be byte-identical again.
// --schema writes every (element, attribute) pair with its value kind, count, range and an example.
#include "formats/xml_doc.h"

#include <algorithm>
#include <cctype>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <map>
#include <regex>
#include <set>

namespace fs = std::filesystem;
using namespace wce;

namespace {

std::string kind_of(const std::string& v) {
    static const std::regex num(R"(^-?\d+(\.\d+)?([eE][-+]?\d+)?$)");
    if (v == "true" || v == "false") return "bool";
    if (std::regex_match(v, num)) return v.find('.') == std::string::npos ? "int" : "float";
    auto paren = v.find('(');
    if (paren != std::string::npos && !v.empty() && v.back() == ')') return v.substr(0, paren) + "()";
    if (v.empty()) return "empty";
    return "string";
}

struct AttrStats {
    std::map<std::string, size_t> kinds;
    size_t count = 0;
    double lo = 1e300, hi = -1e300;
    std::string example;
    std::set<std::string> files;
};

std::string diff_at(const std::vector<uint8_t>& a, const std::vector<uint8_t>& b) {
    size_t i = 0;
    while (i < a.size() && i < b.size() && a[i] == b[i]) ++i;
    return " (first difference at byte " + std::to_string(i) + ", sizes " + std::to_string(a.size()) + "/" + std::to_string(b.size()) + ")";
}

bool check(const fs::path& p, std::map<std::string, AttrStats>* schema, std::string& err) {
    std::vector<uint8_t> bytes = read_file(p.string());
    XmlDoc doc;
    try {
        doc = XmlDoc::parse(bytes);
    } catch (const std::exception& e) {
        err = e.what();
        if (err.find("odd byte count") != std::string::npos) err = "CORRUPT (UTF-16 with text-mode CRLF, broken in game data too)";
        return false;
    }
    if (doc.save() != bytes) { err = "save differs"; return false; }

    // Edit every attribute, re-parse, compare.
    XmlDoc edited = doc;
    const auto& els = doc.elements();
    for (int e = 0; e < int(els.size()); ++e)
        for (size_t a = 0; a < els[e].attrs.size(); ++a)
            edited.set_attr_raw(e, a, std::string(doc.attr_raw(e, a)) + "_x");
    XmlDoc re = XmlDoc::parse(edited.save());
    if (re.elements().size() != els.size()) { err = "edit changed element count"; return false; }
    for (int e = 0; e < int(els.size()); ++e) {
        if (re.elements()[e].name != els[e].name || re.elements()[e].attrs.size() != els[e].attrs.size()) {
            err = "edit changed tree at element " + std::to_string(e);
            return false;
        }
        for (size_t a = 0; a < els[e].attrs.size(); ++a)
            if (re.attr_raw(e, a) != std::string(doc.attr_raw(e, a)) + "_x") {
                err = "edit value mismatch at " + doc.path_of(e) + "@" + els[e].attrs[a].name;
                return false;
            }
    }
    // Append an attribute to every element, re-parse.
    XmlDoc app = doc;
    for (int e = 0; e < int(els.size()); ++e) app.set(e, "we_test", "a&b'c");
    XmlDoc re2 = XmlDoc::parse(app.save());
    for (int e = 0; e < int(els.size()); ++e)
        if (re2.get(e, "we_test") != std::optional<std::string>("a&b'c")) { err = "append failed"; return false; }
    app.revert_all();
    if (app.save() != bytes) { err = "revert after append differs"; return false; }
    // Revert value edits through the API.
    for (int e = 0; e < int(els.size()); ++e)
        for (size_t a = 0; a < els[e].attrs.size(); ++a)
            edited.set_attr_raw(e, a, std::string(doc.attr_raw(e, a)));
    if (edited.modified() || edited.save() != bytes) { err = "revert differs"; return false; }

    // Structural edits: each must re-parse to the expected tree and, where the inverse exists, restore the bytes.
    const int n = int(els.size());
    const int stride = std::max(1, n / 60);
    for (int e = 0; e < n; e += stride) {
        const std::string where = doc.path_of(e);
        try {
            int ni = -1;
            XmlDoc dup = doc.duplicated_element(e, &ni);
            if (int(dup.elements().size()) != n + doc.subtree_size(e) || dup.elements()[ni].name != els[e].name) {
                err = "duplicate: wrong tree at " + where;
                return false;
            }
            if (dup.removed_element(ni).save() != bytes) { err = "duplicate + remove differs at " + where + diff_at(dup.removed_element(ni).save(), bytes); return false; }

            if (els[e].parent >= 0) {
                const auto& sib = els[els[e].parent].children;
                if (sib.back() != e) { // has a next sibling
                    XmlDoc mv;
                    try {
                        mv = doc.moved_element(e, +1);
                    } catch (const std::exception& ex) {
                        if (std::string(ex.what()).find("adjacent lines") == std::string::npos) throw;
                        goto after_move; // inline elements cannot be swapped by whole lines
                    }
                    if (mv.elements().size() != els.size()) { err = "move changed element count at " + where; return false; }
                    if (mv.moved_element(e, +1).save() != bytes) { err = "move + move back differs at " + where + diff_at(mv.moved_element(e, +1).save(), bytes); return false; }
                }
            }

        after_move:
            try {
                int ci = -1;
                XmlDoc ch = doc.with_child(e, "we_new", &ci);
                if (int(ch.elements().size()) != n + 1 || ch.elements()[ci].name != "we_new" || ch.elements()[ci].parent != e) {
                    err = "add child: wrong tree at " + where;
                    return false;
                }
                if (!els[e].children.empty() && ch.removed_element(ci).save() != bytes) { err = "add child + remove differs at " + where + diff_at(ch.removed_element(ci).save(), bytes); return false; }

            } catch (const std::exception& ex) {
                if (std::string(ex.what()).find("text content") == std::string::npos) throw; // <property>1</property>
            }

            XmlDoc at = doc.with_attr_added(e, "we_x", "a&b");
            if (at.get(e, "we_x") != std::optional<std::string>("a&b")) { err = "add attribute failed at " + where; return false; }
            if (at.without_attr(e, els[e].attrs.size()).save() != bytes) { err = "add attribute + remove differs at " + where + diff_at(at.without_attr(e, els[e].attrs.size()).save(), bytes); return false; }
            if (!els[e].attrs.empty()) {
                XmlDoc rm = doc.without_attr(e, 0);
                if (rm.elements()[e].attrs.size() + 1 != els[e].attrs.size() || rm.find_attr(e, els[e].attrs[0].name)) {
                    err = "remove attribute failed at " + where;
                    return false;
                }
            }
        } catch (const std::exception& ex) {
            err = std::string("structural edit threw at ") + where + ": " + ex.what();
            return false;
        }
    }

    if (schema) {
        for (int e = 0; e < int(els.size()); ++e)
            for (size_t a = 0; a < els[e].attrs.size(); ++a) {
                std::string v = *doc.get(e, els[e].attrs[a].name);
                AttrStats& s = (*schema)[els[e].name + "\t" + els[e].attrs[a].name];
                std::string k = kind_of(v);
                s.kinds[k]++;
                s.count++;
                if (k == "float" || k == "int") {
                    double d = std::stod(v);
                    s.lo = std::min(s.lo, d);
                    s.hi = std::max(s.hi, d);
                }
                if (s.example.empty()) s.example = v.substr(0, 60);
                if (s.files.size() < 1) s.files.insert(p.filename().string());
            }
    }
    return true;
}

} // namespace

int main(int argc, char** argv) {
    std::vector<fs::path> inputs;
    std::string schema_out;
    for (int i = 1; i < argc; ++i) {
        std::string a = argv[i];
        if (a == "--schema" && i + 1 < argc) schema_out = argv[++i];
        else inputs.emplace_back(a);
    }
    std::vector<fs::path> files;
    for (auto& in : inputs) {
        if (fs::is_regular_file(in)) { files.push_back(in); continue; }
        for (auto& de : fs::recursive_directory_iterator(in)) {
            if (!de.is_regular_file()) continue;
            std::string ext = de.path().extension().string();
            for (char& c : ext) c = char(std::tolower(static_cast<unsigned char>(c)));
            if (ext == ".environment" || ext == ".lighting" || ext == ".xml") files.push_back(de.path());
        }
    }
    std::map<std::string, std::map<std::string, AttrStats>> schemas; // by extension
    std::map<std::string, std::pair<int, int>> by_ext;
    int fails = 0, corrupt_files = 0;
    for (auto& f : files) {
        std::string ext = f.extension().string();
        std::string err;
        bool ok = check(f, schema_out.empty() ? nullptr : &schemas[ext], err);
        auto& c = by_ext[ext];
        bool corrupt = !ok && err.rfind("CORRUPT", 0) == 0;
        if (corrupt) ++corrupt_files;
        else (ok ? c.first : c.second)++;
        if (!ok) {
            if (!corrupt) ++fails;
            std::printf("FAIL %s: %s\n", f.string().c_str(), err.c_str());
        }
    }
    for (auto& [ext, c] : by_ext) std::printf("%-14s ok %5d  fail %d\n", ext.c_str(), c.first, c.second);
    std::printf("total %zu files, %d failed, %d corrupt in source data\n", files.size(), fails, corrupt_files);

    if (!schema_out.empty()) {
        std::ofstream o(schema_out);
        o << "ext\telement\tattribute\tcount\tkinds\tmin\tmax\texample\tfile\n";
        for (auto& [ext, sch] : schemas)
            for (auto& [key, s] : sch) {
                std::string kinds;
                for (auto& [k, c] : s.kinds) kinds += (kinds.empty() ? "" : ",") + k + ":" + std::to_string(c);
                o << ext << "\t" << key << "\t" << s.count << "\t" << kinds << "\t";
                if (s.lo <= s.hi) o << s.lo << "\t" << s.hi;
                else o << "\t";
                o << "\t" << s.example << "\t" << *s.files.begin() << "\n";
            }
    }
    return fails ? 1 : 0;
}
