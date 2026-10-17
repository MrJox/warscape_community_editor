#pragma once
// How the VFS reads a pack: list its files and extract some of them to disk, through RPFM (there is no pack reader of our own, by
// design). The implementation talks to rpfm_server (RPFM 5.2+, WebSocket API); tests can give Vfs another one. Pack priority,
// caching and path handling stay in Vfs.
#include <filesystem>
#include <memory>
#include <string>
#include <vector>

namespace wce {

class PackBackend {
public:
    virtual ~PackBackend() = default;
    virtual std::string name() const = 0;           // "rpfm_server <host>:<port>"

    // The files of one pack, as paths inside the pack in the case the pack stores them.
    virtual std::vector<std::string> list(const std::filesystem::path& pack) = 0;
    // Extracts the given in-pack paths (as returned by list) to <root>/<path>. Files that do not exist in the pack are skipped.
    // Throws std::runtime_error when the tool itself fails.
    virtual void extract(const std::filesystem::path& pack, const std::vector<std::string>& paths, const std::filesystem::path& root) = 0;
};

struct RpfmServerConfig {
    std::string host = "127.0.0.1";
    int port = 45127;                    // fixed by rpfm_server (no command line option): the editor uses the running server, in a session of its own
    std::filesystem::path exe;           // rpfm_server.exe, started when nothing listens and autostart is on
    bool autostart = true;
    std::string game_key = "attila";
};

// Throws std::runtime_error when the server cannot be used (the message says why).
std::unique_ptr<PackBackend> make_rpfm_server_backend(const RpfmServerConfig& cfg);

} // namespace wce
