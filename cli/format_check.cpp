// Parses and re-formats every attribute value in the given files; reports values whose
// formatted text differs from the original (i.e. where an edit would change the number style).
//   format_check <dir>...
#include "formats/env_value.h"
#include "formats/xml_doc.h"

#include <cstdio>
#include <filesystem>
#include <map>

namespace fs = std::filesystem;
using namespace we;

int main(int argc, char** argv) {
    std::map<int, std::pair<size_t, size_t>> by_kind; // kind -> (same, differs)
    std::map<std::string, size_t> examples;
    for (int i = 1; i < argc; ++i)
        for (auto& de : fs::recursive_directory_iterator(argv[i])) {
            auto ext = de.path().extension().string();
            if (ext != ".environment" && ext != ".lighting" && ext != ".xml") continue;
            XmlDoc doc;
            try { doc = XmlDoc::parse(read_file(de.path().string())); } catch (...) { continue; }
            for (int e = 0; e < int(doc.elements().size()); ++e)
                for (size_t a = 0; a < doc.elements()[e].attrs.size(); ++a) {
                    std::string raw(doc.attr_raw(e, a));
                    EnvValue v = parse_value(raw);
                    if (v.kind == ValueKind::String) continue;
                    bool same = format_value(v) == raw;
                    auto& c = by_kind[int(v.kind)];
                    (same ? c.first : c.second)++;
                    if (!same && examples.size() < 40) examples[raw + "  ->  " + format_value(v)]++;
                }
        }
    const char* names[] = {"float", "int", "bool", "rgb", "argb", "variance", "euler", "string"};
    for (auto& [k, c] : by_kind) std::printf("%-9s same %7zu  differs %zu\n", names[k], c.first, c.second);
    for (auto& [s, n] : examples) std::printf("  %s\n", s.c_str());
}
