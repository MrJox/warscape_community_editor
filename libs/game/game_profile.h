#pragma once
// Everything that is specific to one Total War game, in one place, so that Rome 2 or Thrones of Britannia could be added later.
// Phase 2 holds the pack facts; shader names and engine constants from the reverse engineering join in Phase 3.
#include <filesystem>
#include <string>
#include <vector>

namespace wce {

struct GameProfile {
    std::string key;                       // the game key rpfm knows ("attila")
    std::string display_name;
    std::string data_dir = "data";         // under the game folder
    std::string used_mods_file = "used_mods.txt";   // launcher output: mod "<name>.pack"; lines, first listed wins
    std::string manifest_file = "manifest.txt";     // under data_dir: <pack>\t<...> lines naming the vanilla packs
    std::vector<std::string> extra_vanilla;         // DLC packs that the manifest does not list; searched before the manifest packs

    static const GameProfile& attila();

    struct PackRef {
        std::string name;                  // file name in data_dir
        std::filesystem::path path;
        bool is_mod = false;
    };
    struct PackOrder {
        std::vector<PackRef> packs;        // highest priority first
        std::vector<std::string> missing_mods;   // named in used_mods.txt, not present in data_dir
    };
    // Pack priority, our own rule (the engine's exact order is TODO(verify)): mods in used_mods.txt order (first listed wins),
    // then extra_vanilla, then the manifest packs in file order. Duplicates (case-insensitive) keep their first position.
    PackOrder pack_order(const std::filesystem::path& game_dir, bool use_mods) const;
};

} // namespace wce
