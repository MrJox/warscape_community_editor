// Converts the battle weather selection database between its XML form and FASTBIN0.
//
//   variation_convert <in.xml|in.bin> <out.bin|out.xml> [--force]
//
// The direction follows the file extensions. The data is validated first (more than 32 values, duplicate values, flag bits that
// belong to no value, variations that can never match, no 'default_map'); problems stop the conversion unless --force is given.
// XML output uses the canonical layout (CRLF, tabs, single quotes), which is byte-identical to the vanilla file.
#include "foundation/text_encoding.h"
#include "formats/variation_db.h"

#include <cstdio>
#include <filesystem>
#include <fstream>

namespace fs = std::filesystem;
using namespace wce;

namespace {

std::string ext(const std::string& p) {
    std::string e = fs::path(p).extension().string();
    for (char& c : e) c = char(std::tolower(static_cast<unsigned char>(c)));
    return e;
}

} // namespace

int main(int argc, char** argv) {
    std::vector<std::string> args;
    bool force = false;
    for (int i = 1; i < argc; ++i) {
        if (std::string(argv[i]) == "--force") force = true;
        else args.push_back(argv[i]);
    }
    if (args.size() != 2 || !((ext(args[0]) == ".xml" && ext(args[1]) == ".bin") || (ext(args[0]) == ".bin" && ext(args[1]) == ".xml"))) {
        std::printf("usage: variation_convert <in.xml|in.bin> <out.bin|out.xml> [--force]\n");
        return 2;
    }
    try {
        const std::vector<uint8_t> in = read_file(args[0]);
        const bool to_bin = ext(args[1]) == ".bin";
        VariationDb db = to_bin ? VariationDb::parse_xml(in) : VariationDb::parse(in);
        auto problems = db.validate();
        for (auto& p : problems) std::printf("%s: %s\n", force ? "warning" : "problem", p.c_str());
        if (!problems.empty() && !force) {
            std::printf("nothing written (use --force to write it anyway)\n");
            return 1;
        }
        std::vector<uint8_t> out;
        if (to_bin) {
            out = db.to_bytes();
        } else {
            const std::string x = db.to_xml();
            out.assign(x.begin(), x.end());
        }
        std::ofstream f(args[1], std::ios::binary);
        f.write(reinterpret_cast<const char*>(out.data()), std::streamsize(out.size()));
        f.close();
        if (!f) throw std::runtime_error("cannot write " + args[1]);
        size_t variations = 0;
        for (auto& e : db.entries) variations += e.variations.size();
        std::printf("%s -> %s: %zu categories (%d values), %zu variations, %zu bytes\n", args[0].c_str(), args[1].c_str(), db.metas.size(), db.value_count(), variations, out.size());
        return 0;
    } catch (const std::exception& e) {
        std::printf("error: %s\n", e.what());
        return 1;
    }
}
