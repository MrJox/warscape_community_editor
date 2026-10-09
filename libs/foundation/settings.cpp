#include "foundation/settings.h"

#include "foundation/process.h"

#include <cstdlib>
#include <fstream>

namespace fs = std::filesystem;

namespace wce {

namespace {

std::optional<std::string> env_value(const char* name) {
    if (!name) return std::nullopt;
    const char* v = std::getenv(name);
    if (!v || !*v) return std::nullopt;
    return std::string(v);
}

} // namespace

fs::path Settings::default_dir() {
    if (auto v = env_value("WCE_SETTINGS_DIR")) return fs::path(widen(*v));
    fs::path local = fs::temp_directory_path();
    if (auto v = env_value("LOCALAPPDATA")) local = fs::path(widen(*v));
    return local / "awe";
}

fs::path Settings::cache_dir() const {
    if (auto v = get("cache_dir", "WCE_CACHE_DIR")) return fs::path(widen(*v));
    if (auto v = env_value("AWE_CACHE")) return fs::path(widen(*v));
    return file_.parent_path();
}

Settings Settings::load(const fs::path& dir) {
    Settings s;
    s.file_ = dir / "wce.ini";
    std::ifstream f(s.file_);
    std::string l;
    while (std::getline(f, l)) {
        if (!l.empty() && l.back() == '\r') l.pop_back();
        size_t eq = l.find('=');
        if (l.empty() || l[0] == '#' || eq == std::string::npos) continue;
        s.values_[l.substr(0, eq)] = l.substr(eq + 1);
    }
    return s;
}

std::optional<std::string> Settings::get(const std::string& key, const char* env) const {
    if (auto v = env_value(env)) return v;
    auto it = values_.find(key);
    if (it == values_.end() || it->second.empty()) return std::nullopt;
    return it->second;
}

bool Settings::save() const {
    std::error_code ec;
    fs::create_directories(file_.parent_path(), ec);
    std::ofstream f(file_);
    if (!f) return false;
    for (const auto& [k, v] : values_) f << k << "=" << v << "\n";
    return bool(f);
}

} // namespace wce
