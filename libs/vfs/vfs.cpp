#include "vfs/vfs.h"

#include "foundation/process.h"
#include "foundation/text_encoding.h"

#include <algorithm>
#include <cstdlib>
#include <fstream>
#include <regex>
#include <set>
#include <sstream>
#include <stdexcept>

namespace fs = std::filesystem;

namespace wce {

namespace {

fs::path env_path(const char* name, const fs::path& fallback) {
    if (const char* v = std::getenv(name); v && *v) return fs::path(widen(v));
    return fallback;
}

std::vector<std::string> rpfm_lines(const std::string& out) {
    // rpfm_cli prints log lines ("hh:mm:ss [INFO] ...") before the payload.
    static const std::regex log(R"(^\d\d:\d\d:\d\d .*\[)");
    std::vector<std::string> lines;
    std::istringstream is(out);
    std::string l;
    while (std::getline(is, l)) {
        if (!l.empty() && l.back() == '\r') l.pop_back();
        if (l.empty() || std::regex_search(l, log)) continue;
        lines.push_back(l);
    }
    return lines;
}

} // namespace

Vfs::Config Vfs::default_config() {
    Config c;
    c.game_dir = env_path("ATTILA_DIR", LR"(D:\SteamLibrary\steamapps\common\Total War Attila)");
    c.rpfm_cli = env_path("RPFM_CLI", LR"(C:\Users\Khaiali\Downloads\rpfm-v4.3.14-x86_64-pc-windows-msvc\rpfm_cli.exe)");
    fs::path local = env_path("LOCALAPPDATA", fs::temp_directory_path());
    c.cache_dir = env_path("AWE_CACHE", local / "awe");
    return c;
}

Vfs::Vfs(Config cfg) : cfg_(std::move(cfg)) {}

std::string Vfs::normalize(std::string p) {
    for (char& c : p) {
        if (c == '\\') c = '/';
        else c = char(std::tolower(static_cast<unsigned char>(c)));
    }
    return p;
}

void Vfs::mount() {
    packs_.clear();
    missing_mods_.clear();
    files_.clear();
    all_owners_.clear();
    original_case_.clear();
    const fs::path data = cfg_.game_dir / "data";

    auto add = [&](const std::string& name, bool is_mod) {
        fs::path p = data / widen(name);
        if (!fs::exists(p)) {
            if (is_mod) missing_mods_.push_back(name);
            return;
        }
        for (auto& q : packs_)
            if (normalize(q.name) == normalize(name)) return;
        Pack pk;
        pk.name = name;
        pk.path = p;
        pk.is_mod = is_mod;
        pk.size = fs::file_size(p);
        pk.mtime = fs::last_write_time(p).time_since_epoch().count();
        packs_.push_back(std::move(pk));
    };

    if (cfg_.use_mods) {
        std::ifstream um(cfg_.game_dir / "used_mods.txt");
        static const std::regex re(R"re(^\s*mod\s+"([^"]+)"\s*;)re");
        std::string l;
        std::smatch m;
        while (std::getline(um, l))
            if (std::regex_search(l, m, re)) add(m[1], true);
    }
    for (auto& n : cfg_.extra_vanilla) add(n, false);
    {
        std::ifstream mf(data / "manifest.txt");
        std::string l;
        while (std::getline(mf, l)) {
            std::string name = l.substr(0, l.find('\t'));
            if (name.size() > 5 && normalize(name).ends_with(".pack") && name.find('/') == std::string::npos) add(name, false);
        }
    }

    for (int i = 0; i < int(packs_.size()); ++i) {
        for (auto& f : index_pack(packs_[i])) {
            std::string n = normalize(f);
            all_owners_[n].push_back(i);
            if (files_.emplace(n, i).second) original_case_[n] = f;
        }
    }
}

std::vector<std::string> Vfs::index_pack(const Pack& p) {
    fs::path dir = cfg_.cache_dir / "index";
    fs::create_directories(dir);
    fs::path idx = dir / widen(p.name + "." + std::to_string(p.size) + "." + std::to_string(p.mtime) + ".txt");
    std::vector<std::string> lines;
    if (fs::exists(idx)) {
        std::ifstream f(idx);
        std::string l;
        while (std::getline(f, l)) lines.push_back(l);
        return lines;
    }
    ProcessResult r = run_process(cfg_.rpfm_cli, {L"--game", L"attila", L"pack", L"list", L"--pack-path", p.path.wstring()});
    if (r.exit_code != 0) throw std::runtime_error("rpfm_cli list failed for " + p.name + ": " + r.out.substr(0, 400));
    lines = rpfm_lines(r.out);
    // Drop stale indexes of the same pack.
    for (auto& de : fs::directory_iterator(dir))
        if (de.path().filename().string().rfind(p.name + ".", 0) == 0) fs::remove(de.path());
    std::ofstream f(idx);
    for (auto& l : lines) f << l << "\n";
    return lines;
}

std::vector<std::string> Vfs::list(const std::string& prefix, const std::string& extension) const {
    std::string pre = normalize(prefix), ext = normalize(extension);
    std::vector<std::string> out;
    for (auto it = files_.lower_bound(pre); it != files_.end() && it->first.rfind(pre, 0) == 0; ++it)
        if (ext.empty() || it->first.ends_with(ext)) out.push_back(it->first);
    return out;
}

std::optional<int> Vfs::owner(const std::string& path) const {
    auto it = files_.find(normalize(path));
    if (it == files_.end()) return std::nullopt;
    return it->second;
}

std::vector<int> Vfs::owners(const std::string& path) const {
    auto it = all_owners_.find(normalize(path));
    return it == all_owners_.end() ? std::vector<int>{} : it->second;
}

fs::path Vfs::extract_root(int pack) const {
    const Pack& p = packs_[pack];
    // Short, unique per pack version.
    size_t h = std::hash<std::string>{}(p.name + std::to_string(p.size) + std::to_string(p.mtime));
    char id[17];
    std::snprintf(id, sizeof id, "%08x", unsigned(h));
    return cfg_.cache_dir / "x" / id;
}

void Vfs::extract(int pack, const std::vector<std::string>& paths) {
    fs::path root = extract_root(pack);
    fs::create_directories(root);
    const size_t kMaxCmd = 30000;
    std::vector<std::wstring> args;
    size_t len = 0;
    auto flush = [&] {
        if (args.empty()) return;
        std::vector<std::wstring> full = {L"--game", L"attila", L"pack", L"extract", L"--pack-path", packs_[pack].path.wstring()};
        full.insert(full.end(), args.begin(), args.end());
        ProcessResult r = run_process(cfg_.rpfm_cli, full);
        if (r.exit_code != 0) throw std::runtime_error("rpfm_cli extract failed for " + packs_[pack].name + ": " + r.out.substr(0, 400));
        args.clear();
        len = 0;
    };
    for (auto& p : paths) {
        auto oc = original_case_.find(normalize(p));
        std::string in_pack = oc != original_case_.end() ? oc->second : p;
        std::wstring a = widen(in_pack) + L";" + root.wstring();
        if (len + a.size() + 8 > kMaxCmd) flush();
        args.push_back(L"-f");
        args.push_back(a);
        len += a.size() + 8;
    }
    flush();
}

std::optional<fs::path> Vfs::fetch_from(int pack, const std::string& path) {
    fs::path disk = extract_root(pack) / widen(normalize(path));
    if (fs::exists(disk)) return disk;
    extract(pack, {path});
    if (fs::exists(disk)) return disk;
    if (disk.native().size() >= 260)
        throw std::runtime_error("path too long for rpfm_cli (MAX_PATH): " + narrow(disk.native()));
    return std::nullopt;
}

std::optional<fs::path> Vfs::fetch(const std::string& path) {
    auto o = owner(path);
    if (!o) return std::nullopt;
    return fetch_from(*o, path);
}

size_t Vfs::prefetch(const std::vector<std::string>& paths) {
    std::map<int, std::vector<std::string>> by_pack;
    size_t have = 0;
    for (auto& p : paths) {
        auto o = owner(p);
        if (!o) continue;
        if (fs::exists(extract_root(*o) / widen(normalize(p)))) { ++have; continue; }
        by_pack[*o].push_back(p);
    }
    for (auto& [pk, list] : by_pack) {
        extract(pk, list);
        for (auto& p : list)
            if (fs::exists(extract_root(pk) / widen(normalize(p)))) ++have;
    }
    return have;
}

std::vector<uint8_t> Vfs::read(const std::string& path) {
    auto disk = fetch(path);
    if (!disk) throw std::runtime_error("not found in any pack: " + path);
    return read_file(disk->string());
}

} // namespace wce
