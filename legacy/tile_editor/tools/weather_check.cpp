// weather_check <environment_map.xml>: parses the file and runs a few queries.
#include "weather/environment_map.h"

#include <cstdio>
#include <fstream>
#include <iterator>

int main(int argc, char** argv) {
    if (argc < 2) return 2;
    try {
        std::ifstream in(argv[1], std::ios::binary);
        std::vector<uint8_t> b((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
        te::EnvironmentMap m = te::EnvironmentMap::parse(b);
        std::printf("%zu environment files\n", m.files().size());
        for (const char* c : {"map", "climate", "season", "climate_change_index", "precipitation", "time_of_day", "battle_types", "snow_attrition"}) {
            auto v = m.values(c);
            std::printf("%-22s %zu values:", c, v.size());
            size_t k = 0;
            for (auto& x : v) { if (++k > 12) { std::printf(" ..."); break; } std::printf(" %s", x.c_str()); }
            std::printf("\n");
        }
        te::WeatherQuery q;
        q.map = "main_attila_map"; q.climate = "ROME_Desert"; q.season = "su"; q.precipitation = "dry"; q.time_of_day = "day"; q.battle_type = "land"; q.snow_attrition = "snow_attrition_off";
        bool dflt = false;
        auto r = m.match(q, &dflt);
        std::printf("query desert/summer/dry/day: %zu files%s\n", r.size(), dflt ? " (default_map retry)" : "");
        for (auto* f : r) std::printf("  %s\n", f->filename.c_str());
        te::WeatherQuery all;
        std::printf("unconstrained query on default_map: %zu files\n", m.match(all).size());
    } catch (const std::exception& e) { std::fprintf(stderr, "error: %s\n", e.what()); return 1; }
    return 0;
}
