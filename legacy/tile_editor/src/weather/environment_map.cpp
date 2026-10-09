#include "weather/environment_map.h"

#include "formats/xml_doc.h"

#include <sstream>
#include <stdexcept>

namespace te {

static std::set<std::string> split_list(const std::string& s) {
    std::set<std::string> out;
    std::stringstream ss(s);
    std::string item;
    while (std::getline(ss, item, ',')) {
        size_t a = item.find_first_not_of(" \t"), b = item.find_last_not_of(" \t");
        if (a != std::string::npos) out.insert(item.substr(a, b - a + 1));
    }
    return out;
}

EnvironmentMap EnvironmentMap::parse(const std::vector<uint8_t>& xml) {
    we::XmlDoc doc = we::XmlDoc::parse(xml);
    EnvironmentMap m;
    const auto& els = doc.elements();
    for (size_t i = 0; i < els.size(); ++i) {
        if (els[i].name != "environment_file") continue;
        int e = static_cast<int>(i);
        EnvironmentFile f;
        f.filename = doc.get(e, "filename").value_or("");
        f.map = doc.get(e, "map").value_or("");
        f.climate = split_list(doc.get(e, "climate").value_or(""));
        f.season = split_list(doc.get(e, "season").value_or(""));
        f.climate_change_index = split_list(doc.get(e, "climate_change_index").value_or(""));
        f.precipitation = split_list(doc.get(e, "precipitation").value_or(""));
        f.time_of_day = split_list(doc.get(e, "time_of_day").value_or(""));
        f.battle_types = split_list(doc.get(e, "battle_types").value_or(""));
        f.snow_attrition = split_list(doc.get(e, "snow_attrition").value_or(""));
        m.files_.push_back(std::move(f));
    }
    if (m.files_.empty()) throw std::runtime_error("no <environment_file> entries");
    return m;
}

static bool accepts(const std::set<std::string>& allowed, const std::string& v) { return allowed.empty() || v.empty() || allowed.count(v) > 0; }

std::vector<const EnvironmentFile*> EnvironmentMap::match(const WeatherQuery& q, bool* used_default_map) const {
    auto run = [&](const std::string& map) {
        std::vector<const EnvironmentFile*> out;
        for (const EnvironmentFile& f : files_) {
            if (!f.map.empty() && !map.empty() && f.map != map) continue;
            if (accepts(f.climate, q.climate) && accepts(f.season, q.season) && accepts(f.climate_change_index, q.climate_change_index) &&
                accepts(f.precipitation, q.precipitation) && accepts(f.time_of_day, q.time_of_day) && accepts(f.battle_types, q.battle_type) &&
                accepts(f.snow_attrition, q.snow_attrition))
                out.push_back(&f);
        }
        return out;
    };
    if (used_default_map) *used_default_map = false;
    auto out = run(q.map);
    if (out.empty() && q.map != "default_map") {
        out = run("default_map");
        if (used_default_map) *used_default_map = true;
    }
    return out;
}

std::set<std::string> EnvironmentMap::values(const std::string& category) const {
    std::set<std::string> out;
    for (const EnvironmentFile& f : files_) {
        const std::set<std::string>* s = nullptr;
        if (category == "map") { if (!f.map.empty()) out.insert(f.map); continue; }
        else if (category == "climate") s = &f.climate;
        else if (category == "season") s = &f.season;
        else if (category == "climate_change_index") s = &f.climate_change_index;
        else if (category == "precipitation") s = &f.precipitation;
        else if (category == "time_of_day") s = &f.time_of_day;
        else if (category == "battle_types") s = &f.battle_types;
        else if (category == "snow_attrition") s = &f.snow_attrition;
        if (s) out.insert(s->begin(), s->end());
    }
    return out;
}

} // namespace te
