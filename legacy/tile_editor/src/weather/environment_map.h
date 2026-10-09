#pragma once
// raw_data/warscape_asset_variation_db/environment_map.xml: which .environment file a battle uses (source form of the
// warscape_asset_variation_db FASTBIN that BOB_Terrain builds). Each <environment_file> lists, per category, the values
// it accepts as a comma separated list; a missing or empty attribute accepts everything. A battle picks uniformly among
// the matching files for its map; when none matches, the game asks again with map "default_map".
#include <cstdint>
#include <map>
#include <set>
#include <string>
#include <vector>

namespace te {

struct EnvironmentFile {
    std::string filename;                                   // as stored (backslashes)
    std::string map;
    std::set<std::string> climate, season, climate_change_index, precipitation, time_of_day, battle_types, snow_attrition;
};

struct WeatherQuery {
    std::string map = "default_map", climate, season, climate_change_index, precipitation, time_of_day, battle_type, snow_attrition;
};

class EnvironmentMap {
public:
    static EnvironmentMap parse(const std::vector<uint8_t>& xml);   // throws std::runtime_error
    const std::vector<EnvironmentFile>& files() const { return files_; }

    // Matching files (empty query fields accept every value). `used_default_map` tells whether the second round ran.
    std::vector<const EnvironmentFile*> match(const WeatherQuery& q, bool* used_default_map = nullptr) const;

    // Distinct values per category, sorted, for building pick lists.
    std::set<std::string> values(const std::string& category) const;   // map, climate, season, climate_change_index, precipitation, time_of_day, battle_types, snow_attrition

private:
    std::vector<EnvironmentFile> files_;
};

} // namespace te
