#pragma once
// Minimal client of rpfm_server's WebSocket API (RPFM 5.2+): JSON-RPC 2.0 over ws://<host>:<port>/ws, on WinHTTP (libcurl on Linux).
// Each connection gets a session of its own on the server (selected game, open packs), isolated from every other client: the RPFM UI,
// other tools, and the MCP clients (which all share one session of their own). Protocol: rpfm/docs/server/ws-protocol.md. Not thread-safe.
#include "foundation/json.h"

#include <memory>
#include <stdexcept>
#include <string>

namespace wce {

// An error answer of the server, or a broken exchange. `kind` is the server's error kind ("pack_not_found", "internal", ...) when it sent one.
struct RpfmError : std::runtime_error {
    explicit RpfmError(const std::string& what, std::string kind = {}) : std::runtime_error(what), kind(std::move(kind)) {}
    std::string kind;
};
// The connection broke and the server no longer has our session (it was restarted, or dropped the session after its grace period): the caller
// must redo its per-session state (game, open packs).
struct RpfmSessionLost : RpfmError {
    using RpfmError::RpfmError;
};
// Nothing listens on the port.
struct RpfmUnreachable : RpfmError {
    using RpfmError::RpfmError;
};

class RpfmClient {
public:
    RpfmClient(std::string host, int port);
    ~RpfmClient();   // ends the session (session.disconnect), so the server does not keep it for its grace period
    RpfmClient(const RpfmClient&) = delete;
    RpfmClient& operator=(const RpfmClient&) = delete;

    // Opens the connection and a new session. Throws RpfmUnreachable, or RpfmError (for instance when the server is an older RPFM).
    void connect();
    bool connected() const { return session_id_ != 0; }
    long long session_id() const { return session_id_; }

    // One request; returns its result. A broken connection is reopened on the same session and the request sent again; if the server lost
    // the session meanwhile, throws RpfmSessionLost. Throws RpfmError with the server's message when the request fails.
    Json call(const std::string& method, const Json& params = Json::object());
    // A method that runs as a job (session.set_game, ...): starts it and waits for it to end. Returns the job's result.
    Json run_job(const std::string& method, const Json& params = Json::object());

private:
    struct Socket;   // the platform WebSocket
    void open_socket(long long reattach);
    Json exchange(const std::string& method, const Json& params);

    std::string host_;
    int port_;
    std::unique_ptr<Socket> socket_;
    long long session_id_ = 0;
    long long next_id_ = 1;
};

} // namespace wce
