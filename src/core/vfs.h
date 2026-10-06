#pragma once
// Game file system on top of rpfm_cli (no own pack reader, by design).
//
// Packs, highest priority first:
//   1. mods from <game>/used_mods.txt, in file order (first listed wins);
//   2. vanilla packs: data/manifest.txt .pack entries plus the DLC packs in Config::extra_vanilla.
// TODO(verify): exact engine priority between mods and between vanilla packs.
//
// Each pack's file list is cached as <cache>/index/<pack>.<size>.<mtime>.txt, so a pack edited by the
// team is re-indexed automatically. Files are extracted on demand into <cache>/x/<pack-id>/<path>.
// Keep the cache root short: rpfm_cli silently writes nothing past MAX_PATH (260).
#include <filesystem>
#include <map>
#include <optional>
#include <string>
#include <vector>

namespace we {

class Vfs {
public:
    struct Config {
        std::filesystem::path game_dir;   // ...\Total War Attila
        std::filesystem::path rpfm_cli;   // ...\rpfm_cli.exe
        std::filesystem::path cache_dir;  // e.g. %LOCALAPPDATA%\awe
        std::vector<std::string> extra_vanilla = {"charlemagne.pack", "belisarius.pack", "blood.pack", "slavs.pack"};
        bool use_mods = true;
    };

    struct Pack {
        std::string name;
        std::filesystem::path path;
        bool is_mod = false;
        uint64_t size = 0;
        int64_t mtime = 0;
    };

    // Defaults from the environment: ATTILA_DIR, RPFM_CLI, AWE_CACHE (else this machine's known paths).
    static Config default_config();

    explicit Vfs(Config cfg);
    void mount(); // builds the pack list and indexes; throws on rpfm failure

    const std::vector<Pack>& packs() const { return packs_; }
    // Mods named in used_mods.txt that are not in data/.
    const std::vector<std::string>& missing_mods() const { return missing_mods_; }
    // Lower-case in-pack path -> index of the winning pack.
    const std::map<std::string, int>& files() const { return files_; }

    std::vector<std::string> list(const std::string& prefix, const std::string& extension = {}) const;
    std::optional<int> owner(const std::string& path) const;
    // All packs that contain the path, highest priority first.
    std::vector<int> owners(const std::string& path) const;

    // Disk path of the winning version, extracting it if needed. nullopt if not in any pack.
    std::optional<std::filesystem::path> fetch(const std::string& path);
    // Same for one specific pack.
    std::optional<std::filesystem::path> fetch_from(int pack, const std::string& path);
    // Batch-extract many files (one rpfm call per pack); returns how many are available afterwards.
    size_t prefetch(const std::vector<std::string>& paths);

    std::vector<uint8_t> read(const std::string& path); // throws if missing

    static std::string normalize(std::string path); // lower-case, forward slashes

private:
    std::vector<std::string> index_pack(const Pack& p);
    std::filesystem::path extract_root(int pack) const;
    void extract(int pack, const std::vector<std::string>& in_pack_paths);

    Config cfg_;
    std::vector<Pack> packs_;
    std::vector<std::string> missing_mods_;
    std::map<std::string, int> files_;
    std::map<std::string, std::vector<int>> all_owners_;
    std::map<std::string, std::string> original_case_; // normalized -> path as rpfm prints it
};

} // namespace we
