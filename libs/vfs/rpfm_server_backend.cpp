#include "foundation/log.h"
#include "foundation/process.h"
#include "vfs/pack_backend.h"
#include "vfs/rpfm_client.h"

#include <algorithm>
#include <chrono>
#include <map>
#include <thread>

namespace fs = std::filesystem;

namespace wce {

namespace {

std::string slashes(const fs::path& p) {
    std::string s = narrow(p.wstring());
    std::replace(s.begin(), s.end(), '\\', '/');
    return s;
}

Json pack_source(const std::string& key) {
    Json s = Json::object();
    s.set("pack", key);
    return s;
}

class RpfmServerBackend : public PackBackend {
public:
    explicit RpfmServerBackend(const RpfmServerConfig& cfg) : cfg_(cfg), rpfm_(cfg.host, cfg.port) { start_session(); }

    ~RpfmServerBackend() override {
        // Only the packs this session opened. The client then ends the session, which closes anything left.
        try {
            for (const auto& [path, key] : keys_) {
                Json a = Json::object();
                a.set("pack", key);
                rpfm_.call("pack.close", a);
            }
        } catch (const std::exception&) {}
    }

    std::string name() const override { return "rpfm_server " + cfg_.host + ":" + std::to_string(cfg_.port); }

    std::vector<std::string> list(const fs::path& pack) override {
        Json r = with_session([&] {
            Json a = Json::object();
            a.set("source", pack_source(key_for(pack)));
            a.set("limit", 100000000LL);   // every file in one answer (the default page is 500)
            return rpfm_.call("files.list", a);
        });
        const Json* files = r.find("files");
        const Json* total = r.find("total");
        if (!files || !files->is_array()) throw std::runtime_error("rpfm_server: unexpected files.list answer for " + slashes(pack.filename()));
        if (total && total->is_number() && size_t(total->as_number()) != files->as_array().size())
            throw std::runtime_error("rpfm_server: files.list returned " + std::to_string(files->as_array().size()) + " of " +
                                     std::to_string(size_t(total->as_number())) + " files of " + slashes(pack.filename()));
        std::vector<std::string> out;
        out.reserve(files->as_array().size());
        for (const Json& f : files->as_array())
            if (const Json* p = f.find("path"); p && p->is_string()) out.push_back(p->as_string());
        return out;
    }

    void extract(const fs::path& pack, const std::vector<std::string>& paths, const fs::path& root) override {
        const size_t kBatch = 2000;
        for (size_t at = 0; at < paths.size(); at += kBatch) {
            Json list = Json::array();
            for (size_t i = at; i < std::min(paths.size(), at + kBatch); ++i) list.push(paths[i]);
            try {
                with_session([&] {
                    Json a = Json::object();
                    a.set("source", pack_source(key_for(pack)));
                    a.set("paths", list);
                    a.set("destination", slashes(root));
                    return rpfm_.call("files.extract", a);
                });
            } catch (const RpfmError& e) {
                // "There were N errors while extracting.": some requested files are not in the pack. The others were written; the caller
                // finds out by looking for the file.
                if (std::string(e.what()).find("errors while extracting") == std::string::npos) throw;
                Log::warn("rpfm_server: " + std::string(e.what()) + " (" + slashes(pack.filename()) + ")");
            }
        }
    }

private:
    void start_session() {
        try {
            rpfm_.connect();
        } catch (const RpfmUnreachable&) {
            if (!cfg_.autostart || cfg_.exe.empty() || !fs::exists(cfg_.exe)) throw;
            if (!start_detached(cfg_.exe.wstring(), {})) throw std::runtime_error("cannot start " + narrow(cfg_.exe.wstring()));
            // The port is fixed, so wait for the listener to appear.
            const auto until = std::chrono::steady_clock::now() + std::chrono::seconds(20);
            for (;;) {
                std::this_thread::sleep_for(std::chrono::milliseconds(250));
                try {
                    rpfm_.connect();
                    break;
                } catch (const RpfmUnreachable&) {
                    if (std::chrono::steady_clock::now() > until) throw;
                }
            }
        }
        // Per-session state: the game decides the schema and the pack format. No dependencies: packs open without them.
        Json a = Json::object();
        a.set("game", cfg_.game_key);
        a.set("rebuild_dependencies", false);
        rpfm_.run_job("session.set_game", a);
        keys_.clear();
    }

    // The server restarted or dropped the session: new session, nothing is open any more.
    template <class F>
    Json with_session(F&& f) {
        try {
            return f();
        } catch (const RpfmSessionLost& e) {
            Log::warn(std::string("rpfm_server: ") + e.what() + "; starting a new session");
            start_session();
            return f();
        }
    }

    std::string key_for(const fs::path& pack) {
        const std::string path = slashes(pack);
        auto it = keys_.find(path);
        if (it != keys_.end()) return it->second;
        Json a = Json::object();
        Json paths = Json::array();
        paths.push(path);
        a.set("paths", std::move(paths));
        a.set("lazy_loading", true);   // file data stays on disk until it is extracted
        const Json r = rpfm_.call("pack.open", a);
        const Json* key = r.find("key");
        if (!key || !key->is_string()) throw std::runtime_error("rpfm_server: unexpected pack.open answer for " + path);
        return keys_.emplace(path, key->as_string()).first->second;
    }

    RpfmServerConfig cfg_;
    RpfmClient rpfm_;
    std::map<std::string, std::string> keys_;   // pack path -> the key the server gave it in this session
};

} // namespace

std::unique_ptr<PackBackend> make_rpfm_server_backend(const RpfmServerConfig& cfg) {
    try {
        return std::make_unique<RpfmServerBackend>(cfg);
    } catch (const RpfmUnreachable& e) {
        throw std::runtime_error(std::string(e.what()) + (cfg.autostart ? "" : " (autostart is off)"));
    }
}

} // namespace wce
