#include "foundation/profile.h"
#include "foundation/log.h"
#include "vfs/vfs.h"

#include "foundation/process.h"
#include "foundation/settings.h"
#include "foundation/text_encoding.h"

#include <algorithm>
#include <chrono>
#include <condition_variable>
#include <cstdlib>
#include <functional>
#include <mutex>
#include <fstream>
#include <sstream>
#include <set>
#include <stdexcept>
#include <thread>

namespace fs = std::filesystem;

namespace wce {

namespace {

std::optional<std::string> env(const char* name) {
    const char* v = std::getenv(name);
    if (!v || !*v) return std::nullopt;
    return std::string(v);
}

uint64_t pack_index_checksum(const std::vector<std::string>& lines) {
    uint64_t hash = 14695981039346656037ull;
    auto feed = [&](unsigned char c) {
        hash ^= c;
        hash *= 1099511628211ull;
    };
    for (const std::string& line : lines) {
        for (unsigned char c : line) feed(c);
        feed('\n');
    }
    return hash;
}

std::string pack_index_header(const std::vector<std::string>& lines) {
    std::ostringstream out;
    out << "# WCE_PACK_INDEX_V1 " << lines.size() << ' ' << std::hex << pack_index_checksum(lines) << '\n';
    return out.str();
}

bool read_pack_index(const fs::path& path, std::vector<std::string>& lines) {
    std::ifstream f(path, std::ios::binary);
    std::string header;
    if (!std::getline(f, header)) return false;
    std::istringstream fields(header);
    std::string magic;
    uint64_t expected_count = 0, expected_checksum = 0;
    std::string version;
    if (!(fields >> magic >> version >> expected_count >> std::hex >> expected_checksum) || magic != "#" || version != "WCE_PACK_INDEX_V1") return false;
    fields >> std::ws;
    if (!fields.eof()) return false;

    lines.clear();
    std::string line;
    while (std::getline(f, line)) {
        if (!line.empty() && line.back() == '\r') line.pop_back();
        lines.push_back(std::move(line));
    }
    return f.eof() && lines.size() == expected_count && pack_index_checksum(lines) == expected_checksum;
}

} // namespace

Vfs::Config Vfs::default_config() {
    Config c;
    Settings s = Settings::load();
    c.cache_dir = s.cache_dir();
    if (auto g = s.get("game_dir", "WCE_GAME")) c.game_dir = fs::path(widen(*g));
    else if (auto a = env("ATTILA_DIR")) c.game_dir = fs::path(widen(*a));

    fs::path rpfm_dir;
    if (auto d = s.get("rpfm_dir", "WCE_RPFM_DIR")) rpfm_dir = fs::path(widen(*d));
    if (auto e = s.get("rpfm_cli", "RPFM_CLI")) c.rpfm_cli = fs::path(widen(*e));
    else if (!rpfm_dir.empty()) c.rpfm_cli = rpfm_dir / "rpfm_cli.exe";

    if (auto e = s.get("rpfm_server_exe", "WCE_RPFM_SERVER")) c.server.exe = fs::path(widen(*e));
    else if (!rpfm_dir.empty()) c.server.exe = rpfm_dir / "rpfm_server.exe";
    if (auto p = s.get("rpfm_server_port", "WCE_RPFM_PORT")) c.server.port = std::atoi(p->c_str());
    if (auto a = s.get("rpfm_server_autostart", "WCE_RPFM_AUTOSTART")) c.server.autostart = *a != "0" && *a != "false" && *a != "off";
    if (auto b = s.get("rpfm_backend", "WCE_RPFM_BACKEND")) {
        if (*b == "server") c.backend = BackendMode::Server;
        else if (*b == "cli") c.backend = BackendMode::Cli;
    }
    c.server.game_key = c.profile.key;
    return c;
}

struct Vfs::ExtractQueue {
    std::mutex m;
    std::condition_variable cv;
    std::map<int, std::set<std::string>> wanted;   // pack -> in-pack paths nobody has fetched yet
    bool busy = false;
    uint64_t seq = 0, done = 0;
};

Vfs::Vfs(Config cfg) : queue_(std::make_unique<ExtractQueue>()), cfg_(std::move(cfg)) {}
Vfs::Vfs(Config cfg, std::unique_ptr<PackBackend> backend) : queue_(std::make_unique<ExtractQueue>()), cfg_(std::move(cfg)), backend_(std::move(backend)) {}
Vfs::~Vfs() = default;

std::string IVfs::normalize(std::string p) {
    for (char& c : p) {
        if (c == '\\') c = '/';
        else c = char(std::tolower(static_cast<unsigned char>(c)));
    }
    return p;
}

PackBackend& Vfs::backend() {
    if (backend_) return *backend_;
    const std::string& game = cfg_.profile.key;
    switch (cfg_.backend) {
    case BackendMode::Server: backend_ = make_rpfm_server_backend(cfg_.server); break;
    case BackendMode::Cli: backend_ = make_rpfm_cli_backend(cfg_.rpfm_cli, game); break;
    case BackendMode::Auto:
        try {
            backend_ = make_rpfm_server_backend(cfg_.server);
        } catch (const std::exception& server_error) {
            try {
                backend_ = make_rpfm_cli_backend(cfg_.rpfm_cli, game);
            } catch (const std::exception& cli_error) {
                throw std::runtime_error(std::string("no way to read packs: ") + server_error.what() + "; " + cli_error.what());
            }
            backend_note_ = std::string("rpfm_server unavailable (") + server_error.what() + "); using rpfm_cli";
        }
        break;
    }
    return *backend_;
}

void Vfs::mount() {
    WCE_PROF("vfs.mount");
    packs_.clear();
    missing_mods_.clear();
    files_.clear();
    all_owners_.clear();
    pack_case_.clear();
    if (cfg_.game_dir.empty() || !fs::exists(cfg_.game_dir))
        throw std::runtime_error("the game folder is not set or does not exist (game_dir in wce.ini, or the environment variable WCE_GAME)");
    // rpfm_server and rpfm_cli run in another working directory: every path handed to them must be absolute.
    cfg_.game_dir = fs::absolute(cfg_.game_dir);
    cfg_.cache_dir = fs::absolute(cfg_.cache_dir);
    if (!cfg_.rpfm_cli.empty()) cfg_.rpfm_cli = fs::absolute(cfg_.rpfm_cli);
    for (fs::path& f : cfg_.overlay_folders) f = fs::absolute(f);

    for (const fs::path& folder : cfg_.overlay_folders) {
        Pack pk;
        pk.name = narrow(folder.filename().wstring());
        pk.path = folder;
        pk.loose = true;
        packs_.push_back(std::move(pk));
    }
    GameProfile::PackOrder order = cfg_.profile.pack_order(cfg_.game_dir, cfg_.use_mods);
    missing_mods_ = std::move(order.missing_mods);
    for (const GameProfile::PackRef& r : order.packs) {
        Pack pk;
        pk.name = r.name;
        pk.path = r.path;
        pk.is_mod = r.is_mod;
        pk.size = fs::file_size(r.path);
        pk.mtime = fs::last_write_time(r.path).time_since_epoch().count();
        packs_.push_back(std::move(pk));
    }

    pack_case_.resize(packs_.size());
    for (int i = 0; i < int(packs_.size()); ++i) {
        for (auto& f : packs_[i].loose ? index_folder(packs_[i]) : index_pack(packs_[i])) {
            std::string n = normalize(f);
            if (n != f) pack_case_[i].emplace(n, f);
            all_owners_[n].push_back(i);
            files_.emplace(n, i);
        }
    }
}

std::vector<std::string> Vfs::index_folder(const Pack& p) const {
    std::vector<std::string> out;
    std::error_code ec;
    for (fs::recursive_directory_iterator it(p.path, fs::directory_options::skip_permission_denied, ec), end; !ec && it != end; it.increment(ec))
        if (it->is_regular_file(ec)) out.push_back(narrow(it->path().lexically_relative(p.path).generic_wstring()));
    return out;
}

std::vector<std::string> Vfs::index_pack(const Pack& p) {
    WCE_PROF("vfs.index_pack");
    fs::path dir = cfg_.cache_dir / "index";
    fs::create_directories(dir);
    fs::path idx = dir / widen(p.name + "." + std::to_string(p.size) + "." + std::to_string(p.mtime) + ".txt");
    std::vector<std::string> lines;
    if (fs::exists(idx)) {
        if (read_pack_index(idx, lines)) return lines;
        Log::warn("refreshing incomplete or outdated pack index: " + p.name);
    }
    lines = backend().list(p.path);
    // The previous implementation wrote directly to the final path. If the process was stopped mid-write,
    // the next run treated that truncated list as valid and silently hid assets from the VFS. Write a
    // checksummed file beside the cache entry, then rename it into place only after the stream is complete.
    const auto tick = std::chrono::steady_clock::now().time_since_epoch().count();
    const auto thread = std::hash<std::thread::id>{}(std::this_thread::get_id());
    fs::path tmp = idx;
    tmp += ".tmp." + std::to_string(uint64_t(tick) ^ uint64_t(thread));
    {
        std::ofstream f(tmp, std::ios::binary | std::ios::trunc);
        if (f) {
            f << pack_index_header(lines);
            for (const auto& l : lines) f << l << "\n";
            f.flush();
            if (!f) {
                f.close();
                std::error_code ec;
                fs::remove(tmp, ec);
                throw std::runtime_error("cannot write pack index cache: " + narrow(tmp.wstring()));
            }
        } else {
            throw std::runtime_error("cannot create pack index cache: " + narrow(tmp.wstring()));
        }
    }
    std::error_code ec;
    fs::rename(tmp, idx, ec);
    if (ec) {
        fs::remove(tmp);
        throw std::runtime_error("cannot commit pack index cache: " + narrow(idx.wstring()) + ": " + ec.message());
    }
    // Drop stale indexes of the same pack after the replacement is safely installed.
    for (auto& de : fs::directory_iterator(dir))
        if (de.path() != idx && de.path().filename().wstring().rfind(widen(p.name + "."), 0) == 0) fs::remove(de.path());
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
    ExtractQueue& q = *queue_;
    std::unique_lock<std::mutex> lock(q.m);
    for (auto& p : paths) {
        const std::string n = normalize(p);
        auto oc = pack_case_[size_t(pack)].find(n);
        q.wanted[pack].insert(oc != pack_case_[size_t(pack)].end() ? oc->second : n);   // the pack stores the path in this case
    }
    const uint64_t ticket = ++q.seq;
    std::exception_ptr failure;
    while (q.done < ticket) {
        if (q.busy) {
            q.cv.wait(lock);
            continue;
        }
        // this thread does the call for everything requested so far
        q.busy = true;
        const uint64_t upto = q.seq;
        std::map<int, std::set<std::string>> batch = std::move(q.wanted);
        q.wanted.clear();
        lock.unlock();
        try {
            WCE_PROF("vfs.extract (rpfm)");
            for (auto& [pk, set] : batch) {
                fs::path root = extract_root(pk);
                fs::create_directories(root);
                // what an earlier call (or another process) already left on disk is not asked for again
                std::vector<std::string> in_pack;
                for (const std::string& n : set)
                    if (!fs::exists(root / widen(normalize(n)))) in_pack.push_back(n);
                if (in_pack.empty()) continue;

                const std::string reader = backend().name();
                backend_->extract(packs_[size_t(pk)].path, in_pack, root);

                // The server can report a successful tool call even when it did not write every requested file.
                // Don't leave those misses looking like absent game assets: retry them with the CLI backend.
                std::vector<std::string> missing;
                for (const std::string& n : in_pack)
                    if (!fs::exists(root / widen(normalize(n)))) missing.push_back(n);
                if (!missing.empty() && reader.rfind("rpfm_server", 0) == 0) {
                    Log::warn("rpfm_server left " + std::to_string(missing.size()) + " file(s) unextracted from " + packs_[size_t(pk)].name + " (" + missing.front() + "); retrying with rpfm_cli");
                    backend_ = make_rpfm_cli_backend(cfg_.rpfm_cli, cfg_.profile.key);
                    backend_note_ = "rpfm_server left files unextracted; using rpfm_cli";
                    try {
                        backend_->extract(packs_[size_t(pk)].path, missing, root);
                    } catch (const std::exception& e) {
                        Log::warn("rpfm_cli retry failed for " + packs_[size_t(pk)].name + ": " + e.what());
                    }
                    missing.erase(std::remove_if(missing.begin(), missing.end(), [&](const std::string& n) {
                        return fs::exists(root / widen(normalize(n)));
                    }), missing.end());
                    if (!missing.empty()) Log::warn("rpfm_cli also left " + std::to_string(missing.size()) + " file(s) unextracted from " + packs_[size_t(pk)].name + ": " + missing.front());
                }
            }
        } catch (...) {
            failure = std::current_exception();
        }
        lock.lock();
        q.busy = false;
        q.done = upto;
        q.cv.notify_all();
    }
    if (failure) std::rethrow_exception(failure);
}

std::optional<fs::path> Vfs::fetch_from(int pack, const std::string& path) {
    if (packs_[pack].loose) {
        fs::path disk = packs_[pack].path / widen(normalize(path));
        if (fs::exists(disk)) return disk;
        return std::nullopt;
    }
    fs::path disk = extract_root(pack) / widen(normalize(path));
    if (fs::exists(disk)) return disk;
    extract(pack, {path});
    if (fs::exists(disk)) return disk;
    if (disk.native().size() >= 260)
        throw std::runtime_error("path too long for rpfm (MAX_PATH): " + narrow(disk.native()));
    return std::nullopt;
}

std::optional<fs::path> Vfs::fetch(const std::string& path) {
    // A stale pack index or a pack backend that omitted a file must not hide a
    // lower-priority copy of the same asset.
    std::string failure;
    for (int pack : owners(path)) {
        try {
            if (auto disk = fetch_from(pack, path)) return disk;
        } catch (const std::exception& e) {
            failure = e.what();
            Log::warn("could not fetch " + path + " from " + packs_[size_t(pack)].name + ": " + failure);
        }
    }
    if (!failure.empty()) throw std::runtime_error(failure);
    return std::nullopt;
}

size_t Vfs::prefetch(const std::vector<std::string>& paths) {
    std::map<int, std::vector<std::string>> by_pack;
    size_t have = 0;
    for (auto& p : paths) {
        auto o = owner(p);
        if (!o) continue;
        if (packs_[*o].loose || fs::exists(extract_root(*o) / widen(normalize(p)))) { ++have; continue; }
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
    WCE_PROF("vfs.read");
    auto disk = fetch(path);
    if (!disk) throw std::runtime_error("not found in any pack: " + path);
    return read_file(disk->string());
}

} // namespace wce
