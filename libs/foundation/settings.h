#pragma once
// Machine settings: a key=value text file (wce.ini in the cache folder, %LOCALAPPDATA%\awe by default, ~/.local/share/awe on Linux) with environment overrides.
// Nothing about the machine lives in code; the first-run dialog (Phase 4) writes this file. Lines starting with # are comments.
#include <filesystem>
#include <map>
#include <optional>
#include <string>

namespace wce {

class Settings {
public:
    // The folder that holds wce.ini: $WCE_SETTINGS_DIR, else %LOCALAPPDATA%\awe (Linux: $XDG_DATA_HOME/awe, else ~/.local/share/awe).
    static std::filesystem::path default_dir();
    static Settings load(const std::filesystem::path& dir = default_dir());
    // The VFS cache folder: $WCE_CACHE_DIR, else $AWE_CACHE, else cache_dir in the file, else the settings folder.
    // Keep it short: rpfm silently writes nothing past MAX_PATH (260).
    std::filesystem::path cache_dir() const;

    // The environment variable `env` (when given and non-empty) wins over the file entry `key`.
    std::optional<std::string> get(const std::string& key, const char* env = nullptr) const;
    void set(const std::string& key, const std::string& value) { values_[key] = value; }
    bool save() const; // rewrites wce.ini with the current values

    const std::filesystem::path& file() const { return file_; }

private:
    std::filesystem::path file_;
    std::map<std::string, std::string> values_;
};

} // namespace wce
