#include "foundation/process.h"
#include "vfs/mcp_client.h"
#include "vfs/pack_backend.h"

#include <algorithm>
#include <chrono>
#include <map>
#include <thread>

namespace fs = std::filesystem;

namespace wce {

namespace {

// The tools this backend calls. A server that lacks one is a different RPFM generation: refuse it, the caller falls back to rpfm_cli.
const char* const kRequiredTools[] = {"set_game_selected", "open_packfiles", "open_pack_info", "extract_packed_files", "close_pack"};

std::string slashes(const fs::path& p) {
    std::string s = narrow(p.wstring());
    std::replace(s.begin(), s.end(), '\\', '/');
    return s;
}

class RpfmServerBackend : public PackBackend {
public:
    explicit RpfmServerBackend(const RpfmServerConfig& cfg) : cfg_(cfg), mcp_(cfg.host, cfg.port) { start_session(); }

    ~RpfmServerBackend() override {
        // Only the packs this session opened, and only in this session: other clients of the server are not affected.
        try {
            for (const auto& [path, key] : keys_) {
                Json a = Json::object();
                a.set("pack_key", key);
                mcp_.call("close_pack", a);
            }
        } catch (const std::exception&) {}
    }

    std::string name() const override { return "rpfm_server " + cfg_.host + ":" + std::to_string(cfg_.port); }

    std::vector<std::string> list(const fs::path& pack) override {
        Json r = with_session([&] {
            Json a = Json::object();
            a.set("pack_key", key_for(pack));
            return mcp_.call("open_pack_info", a);
        });
        // {"ContainerInfoVecRFileInfo": [<container info>, [{"path": ..., ...}, ...]]}
        const Json* v = r.find("ContainerInfoVecRFileInfo");
        const Json* files = v ? v->at(1) : nullptr;
        if (!files || !files->is_array()) throw std::runtime_error("rpfm_server: unexpected open_pack_info reply for " + slashes(pack.filename()));
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
            for (size_t i = at; i < std::min(paths.size(), at + kBatch); ++i) {
                Json f = Json::object();
                f.set("File", paths[i]);
                list.push(std::move(f));
            }
            Json src = Json::object();
            src.set("PackFile", std::move(list));
            try {
                with_session([&] {
                    Json a = Json::object();
                    a.set("pack_key", key_for(pack));
                    a.set("source_paths", src.dump());   // the tool takes the map as a JSON string
                    a.set("destination_path", slashes(root));
                    a.set("export_as_tsv", false);
                    return mcp_.call("extract_packed_files", a);
                });
            } catch (const McpError& e) {
                // "There were N errors while extracting.": some requested files are not in the pack. The others were written;
                // like rpfm_cli, the caller finds out by looking for the file.
                if (std::string(e.what()).find("errors while extracting") == std::string::npos) throw;
            }
        }
    }

private:
    void start_session() {
        try {
            mcp_.connect();
        } catch (const McpUnreachable&) {
            if (!cfg_.autostart || cfg_.exe.empty() || !fs::exists(cfg_.exe)) throw;
            if (!start_detached(cfg_.exe.wstring(), {})) throw std::runtime_error("cannot start " + narrow(cfg_.exe.wstring()));
            // The port is fixed, so wait for the listener to appear.
            const auto until = std::chrono::steady_clock::now() + std::chrono::seconds(20);
            for (;;) {
                std::this_thread::sleep_for(std::chrono::milliseconds(250));
                try {
                    mcp_.connect();
                    break;
                } catch (const McpUnreachable&) {
                    if (std::chrono::steady_clock::now() > until) throw;
                }
            }
        }
        std::vector<std::string> have = mcp_.tool_names();
        for (const char* t : kRequiredTools)
            if (std::find(have.begin(), have.end(), t) == have.end()) throw std::runtime_error(std::string("rpfm_server has no tool ") + t + " (a different RPFM version?)");
        // Per-session state: the game decides the schema and the pack format. No dependency rebuild: packs open without it.
        Json a = Json::object();
        a.set("game_name", cfg_.game_key);
        a.set("rebuild_dependencies", false);
        mcp_.call("set_game_selected", a);
        keys_.clear();
    }

    // The server restarted: new session, nothing is open any more.
    template <class F>
    Json with_session(F&& f) {
        try {
            return f();
        } catch (const McpSessionLost&) {
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
        Json r = mcp_.call("open_packfiles", a);
        // {"StringContainerInfo": [<key>, <container info>]}
        const Json* v = r.find("StringContainerInfo");
        const Json* key = v ? v->at(0) : nullptr;
        if (!key || !key->is_string()) throw std::runtime_error("rpfm_server: unexpected open_packfiles reply for " + path);
        return keys_.emplace(path, key->as_string()).first->second;
    }

    RpfmServerConfig cfg_;
    McpClient mcp_;
    std::map<std::string, std::string> keys_;   // pack path -> the key the server gave it in this session
};

} // namespace

std::unique_ptr<PackBackend> make_rpfm_server_backend(const RpfmServerConfig& cfg) {
    try {
        return std::make_unique<RpfmServerBackend>(cfg);
    } catch (const McpUnreachable& e) {
        throw std::runtime_error(std::string(e.what()) + (cfg.autostart ? "" : " (autostart is off)"));
    }
}

} // namespace wce
