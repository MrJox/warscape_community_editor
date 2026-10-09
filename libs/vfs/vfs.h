#pragma once
// Game file system. Consumers use IVfs; Vfs implements it over the game's packs, with pack access through RPFM (no own pack reader).
//
// Pack priority is ours (GameProfile::pack_order), whichever RPFM backend reads the packs:
//   0. loose overlay folders (Config::overlay_folders, e.g. an assembly kit's raw_data), in the order given;
//   1. mods from <game>/used_mods.txt, in file order (first listed wins);
//   2. vanilla packs: the profile's extra_vanilla DLC packs, then data/manifest.txt .pack entries.
// TODO(verify): exact engine priority between mods and between vanilla packs.
//
// Pack backends (pack_backend.h): rpfm_server is the primary one (one persistent process, MCP over HTTP), rpfm_cli the fallback;
// Auto tries the server and falls back to the cli with a note in backend_note().
//
// Each pack's file list is cached as <cache>/index/<pack>.<size>.<mtime>.txt, so a pack edited by the team is re-indexed
// automatically. Files are extracted on demand into <cache>/x/<pack-id>/<path>. Both caches are shared by the backends.
// Keep the cache root short: rpfm silently writes nothing past MAX_PATH (260).
#include "game/game_profile.h"
#include "vfs/pack_backend.h"

#include <cstdint>
#include <filesystem>
#include <map>
#include <memory>
#include <optional>
#include <string>
#include <vector>

namespace wce {

class IVfs {
public:
    struct Pack {
        std::string name;
        std::filesystem::path path;      // the pack file, or the overlay folder when loose
        bool is_mod = false;
        bool loose = false;              // a folder on disk, not a pack
        uint64_t size = 0;
        int64_t mtime = 0;
    };

    virtual ~IVfs() = default;

    virtual const std::vector<Pack>& packs() const = 0;                 // highest priority first
    // Mods named in used_mods.txt that are not in data/.
    virtual const std::vector<std::string>& missing_mods() const = 0;
    // Lower-case in-pack path -> index of the winning pack.
    virtual const std::map<std::string, int>& files() const = 0;

    virtual std::vector<std::string> list(const std::string& prefix, const std::string& extension = {}) const = 0;
    virtual std::optional<int> owner(const std::string& path) const = 0;
    // All packs that contain the path, highest priority first.
    virtual std::vector<int> owners(const std::string& path) const = 0;

    // Disk path of the winning version, extracting it if needed. nullopt if not in any pack.
    virtual std::optional<std::filesystem::path> fetch(const std::string& path) = 0;
    // Same for one specific pack.
    virtual std::optional<std::filesystem::path> fetch_from(int pack, const std::string& path) = 0;
    // Batch-extract many files (one backend call per pack); returns how many are available afterwards.
    virtual size_t prefetch(const std::vector<std::string>& paths) = 0;

    virtual std::vector<uint8_t> read(const std::string& path) = 0;     // throws if missing

    static std::string normalize(std::string path);                     // lower-case, forward slashes
};

class Vfs : public IVfs {
public:
    enum class BackendMode { Auto, Server, Cli };

    struct Config {
        std::filesystem::path game_dir;   // ...\Total War Attila
        std::filesystem::path cache_dir;  // e.g. %LOCALAPPDATA%\awe
        GameProfile profile = GameProfile::attila();
        bool use_mods = true;
        std::vector<std::filesystem::path> overlay_folders;

        BackendMode backend = BackendMode::Auto;
        std::filesystem::path rpfm_cli;   // ...\rpfm_cli.exe
        RpfmServerConfig server;
    };

    // From the settings file and the environment (foundation/settings.h): game_dir (WCE_GAME / ATTILA_DIR), rpfm_dir (WCE_RPFM_DIR) or
    // rpfm_cli (RPFM_CLI), rpfm_server_exe, rpfm_server_port, rpfm_server_autostart, rpfm_backend (WCE_RPFM_BACKEND: auto, server, cli),
    // cache_dir (WCE_CACHE_DIR / AWE_CACHE). Unset values stay empty: mount() then fails with a message that names the missing setting.
    static Config default_config();

    explicit Vfs(Config cfg);
    Vfs(Config cfg, std::unique_ptr<PackBackend> backend);   // a given backend, for tests and the backend comparison
    void mount();                    // builds the pack list and indexes; throws on failure

    // Which backend serves the packs (after the first use), and why the fallback was taken if it was.
    std::string backend_name() const { return backend_ ? backend_->name() : std::string("(not used yet)"); }
    const std::string& backend_note() const { return backend_note_; }

    const std::vector<Pack>& packs() const override { return packs_; }
    const std::vector<std::string>& missing_mods() const override { return missing_mods_; }
    const std::map<std::string, int>& files() const override { return files_; }
    std::vector<std::string> list(const std::string& prefix, const std::string& extension = {}) const override;
    std::optional<int> owner(const std::string& path) const override;
    std::vector<int> owners(const std::string& path) const override;
    std::optional<std::filesystem::path> fetch(const std::string& path) override;
    std::optional<std::filesystem::path> fetch_from(int pack, const std::string& path) override;
    size_t prefetch(const std::vector<std::string>& paths) override;
    std::vector<uint8_t> read(const std::string& path) override;

private:
    PackBackend& backend();
    std::vector<std::string> index_pack(const Pack& p);
    std::vector<std::string> index_folder(const Pack& p) const;
    std::filesystem::path extract_root(int pack) const;
    void extract(int pack, const std::vector<std::string>& in_pack_paths);

    Config cfg_;
    std::unique_ptr<PackBackend> backend_;
    std::string backend_note_;
    std::vector<Pack> packs_;
    std::vector<std::string> missing_mods_;
    std::map<std::string, int> files_;
    std::map<std::string, std::vector<int>> all_owners_;
    std::vector<std::map<std::string, std::string>> pack_case_;   // per pack: normalized -> path as the pack stores it, only where the case differs
};

} // namespace wce
