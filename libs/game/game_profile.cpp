#include "game/game_profile.h"

#include "foundation/process.h"

#include <algorithm>
#include <fstream>
#include <regex>

namespace fs = std::filesystem;

namespace wce {

namespace {

std::string lower(std::string s) {
    for (char& c : s) c = char(std::tolower(static_cast<unsigned char>(c)));
    return s;
}

} // namespace

const GameProfile& GameProfile::attila() {
    static const GameProfile p = [] {
        GameProfile g;
        g.key = "attila";
        g.display_name = "Total War: Attila";
        g.extra_vanilla = {"charlemagne.pack", "belisarius.pack", "blood.pack", "slavs.pack"};
        return g;
    }();
    return p;
}

GameProfile::PackOrder GameProfile::pack_order(const fs::path& game_dir, bool use_mods) const {
    PackOrder out;
    const fs::path data = game_dir / widen(data_dir);
    auto add = [&](const std::string& name, bool is_mod) {
        fs::path p = data / widen(name);
        if (!fs::exists(p)) {
            if (is_mod) out.missing_mods.push_back(name);
            return;
        }
        for (const PackRef& q : out.packs)
            if (lower(q.name) == lower(name)) return;
        out.packs.push_back({name, p, is_mod});
    };

    if (use_mods) {
        std::ifstream um(game_dir / widen(used_mods_file));
        static const std::regex re(R"re(^\s*mod\s+"([^"]+)"\s*;)re");
        std::string l;
        std::smatch m;
        while (std::getline(um, l))
            if (std::regex_search(l, m, re)) add(m[1], true);
    }
    for (const std::string& n : extra_vanilla) add(n, false);
    {
        std::ifstream mf(data / widen(manifest_file));
        std::string l;
        while (std::getline(mf, l)) {
            std::string name = l.substr(0, l.find('\t'));
            if (name.size() > 5 && lower(name).ends_with(".pack") && name.find('/') == std::string::npos)
                add(name, false);
        }
    }
    return out;
}

} // namespace wce
