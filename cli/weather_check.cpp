// weather_check <environment_map.xml>: parses the assembly kit's source form into a VariationDb and runs a few queries.
#include "formats/variation_db.h"

#include <cstdio>
#include <fstream>
#include <iterator>

int main(int argc, char** argv) {
    if (argc < 2) return 2;
    try {
        std::ifstream in(argv[1], std::ios::binary);
        std::vector<uint8_t> b((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
        wce::VariationDb db = wce::VariationDb::parse_environment_map(b);
        const auto& files = db.entries[0].variations;
        std::printf("%zu environment files\n", files.size());
        for (const auto& m : db.metas) {
            std::printf("%-22s %zu values:", m.name.c_str(), m.values.size());
            size_t k = 0;
            for (auto& x : m.values) { if (++k > 12) { std::printf(" ..."); break; } std::printf(" %s", x.c_str()); }
            std::printf("\n");
        }
        wce::VariationDb::Query q = {{"map", "main_attila_map"}, {"climate", "ROME_Desert"}, {"season", "su"}, {"precipitation", "dry"},
                                     {"time_of_day", "day"}, {"battle_types", "land"}, {"snow_attrition", "snow_attrition_off"}};
        auto r = db.candidates_listed_map("environment", q);
        std::printf("query desert/summer/dry/day: %zu files%s\n", r.candidates.size(), r.used_default_map ? " (default_map retry)" : "");
        for (auto& c : r.candidates) std::printf("  %s\n", c.variation->filename.c_str());
        std::printf("unconstrained query on default_map: %zu files\n", db.candidates_listed_map("environment", {{"map", "default_map"}}).candidates.size());
    } catch (const std::exception& e) { std::fprintf(stderr, "error: %s\n", e.what()); return 1; }
    return 0;
}
