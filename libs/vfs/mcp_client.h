#pragma once
// Minimal MCP client (JSON-RPC over HTTP, "streamable HTTP" transport) for rpfm_server, on WinHTTP. One session per client:
// rpfm_server keeps the selected game and the open packs per MCP session, so this client never touches the state of other sessions
// (the attila-toolkit bridge used by Claude sessions talks to the same server). Not thread-safe.
#include "foundation/json.h"

#include <stdexcept>
#include <string>
#include <vector>

namespace wce {

struct McpError : std::runtime_error {
    using std::runtime_error::runtime_error;
};
// The server forgot the session (it was restarted): the caller must reconnect and redo its per-session state.
struct McpSessionLost : McpError {
    using McpError::McpError;
};
// The server cannot be reached.
struct McpUnreachable : McpError {
    using McpError::McpError;
};

class McpClient {
public:
    McpClient(std::string host, int port);
    ~McpClient();
    McpClient(const McpClient&) = delete;
    McpClient& operator=(const McpClient&) = delete;

    // initialize + notifications/initialized. Throws McpUnreachable / McpError.
    void connect();
    bool connected() const { return !session_id_.empty(); }
    void disconnect();    // forgets the session (the server drops it on its own)

    // tools/call. Returns the tool's payload parsed as JSON (a text result that is not JSON comes back as a JSON string).
    // Throws McpError with the server's message when the tool reports an error, McpSessionLost when the session is gone.
    Json call(const std::string& tool, const Json& arguments);
    // Names from tools/list.
    std::vector<std::string> tool_names();

    const std::string& server_name() const { return server_name_; }

private:
    Json rpc(const std::string& method, const Json& params, bool notification = false);
    std::string post(const std::string& body);

    std::string host_;
    int port_;
    void* session_ = nullptr;   // HINTERNET
    void* connection_ = nullptr;
    std::string session_id_;
    std::string server_name_;
    long long next_id_ = 1;
};

} // namespace wce
