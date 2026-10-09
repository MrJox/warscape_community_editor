#include "vfs/mcp_client.h"

#include "foundation/process.h"

#include <windows.h>
#include <winhttp.h>

#include <vector>

namespace wce {

namespace {

std::string last_error_text(const char* what) { return std::string(what) + " (Windows error " + std::to_string(GetLastError()) + ")"; }

// The server answers with text/event-stream: blocks separated by blank lines, each with "data: <json>" lines.
std::vector<std::string> sse_events(const std::string& body) {
    std::vector<std::string> out;
    std::string cur;
    size_t at = 0;
    auto flush = [&] {
        if (!cur.empty()) out.push_back(std::move(cur));
        cur.clear();
    };
    while (at <= body.size()) {
        size_t eol = body.find('\n', at);
        if (eol == std::string::npos) eol = body.size();
        std::string line = body.substr(at, eol - at);
        if (!line.empty() && line.back() == '\r') line.pop_back();
        at = eol + 1;
        if (line.empty()) { flush(); continue; }
        if (line.rfind("data:", 0) == 0) {
            size_t v = line.find_first_not_of(' ', 5);
            if (v != std::string::npos) cur += line.substr(v);
        }
    }
    flush();
    return out;
}

} // namespace

McpClient::McpClient(std::string host, int port) : host_(std::move(host)), port_(port) {}

McpClient::~McpClient() {
    if (connection_) WinHttpCloseHandle(connection_);
    if (session_) WinHttpCloseHandle(session_);
}

void McpClient::disconnect() { session_id_.clear(); }

std::string McpClient::post(const std::string& body) {
    if (!session_) {
        session_ = WinHttpOpen(L"WarscapeCommunityEditor", WINHTTP_ACCESS_TYPE_NO_PROXY, WINHTTP_NO_PROXY_NAME, WINHTTP_NO_PROXY_BYPASS, 0);
        if (!session_) throw McpError(last_error_text("WinHttpOpen"));
        // resolve, connect, send, receive (a batch extract of a large pack can take minutes)
        WinHttpSetTimeouts(session_, 5000, 3000, 60000, 600000);
    }
    if (!connection_) {
        connection_ = WinHttpConnect(session_, widen(host_).c_str(), INTERNET_PORT(port_), 0);
        if (!connection_) throw McpError(last_error_text("WinHttpConnect"));
    }
    HINTERNET req = WinHttpOpenRequest(connection_, L"POST", L"/mcp", nullptr, WINHTTP_NO_REFERER, WINHTTP_DEFAULT_ACCEPT_TYPES, 0);
    if (!req) throw McpError(last_error_text("WinHttpOpenRequest"));
    struct Closer {
        HINTERNET h;
        ~Closer() { WinHttpCloseHandle(h); }
    } closer{req};

    std::wstring headers = L"Content-Type: application/json\r\nAccept: application/json, text/event-stream\r\n";
    if (!session_id_.empty()) headers += L"Mcp-Session-Id: " + widen(session_id_) + L"\r\n";
    if (!WinHttpSendRequest(req, headers.c_str(), DWORD(-1), const_cast<char*>(body.data()), DWORD(body.size()), DWORD(body.size()), 0)) {
        DWORD e = GetLastError();
        if (e == ERROR_WINHTTP_CANNOT_CONNECT || e == ERROR_WINHTTP_TIMEOUT || e == ERROR_WINHTTP_CONNECTION_ERROR)
            throw McpUnreachable("rpfm_server is not reachable on " + host_ + ":" + std::to_string(port_));
        throw McpError("WinHttpSendRequest failed (Windows error " + std::to_string(e) + ")");
    }
    if (!WinHttpReceiveResponse(req, nullptr)) throw McpError(last_error_text("WinHttpReceiveResponse"));

    DWORD status = 0, size = sizeof status;
    WinHttpQueryHeaders(req, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_HEADER_NAME_BY_INDEX, &status, &size, WINHTTP_NO_HEADER_INDEX);

    wchar_t sid[128] = {};
    size = sizeof sid;
    if (WinHttpQueryHeaders(req, WINHTTP_QUERY_CUSTOM, L"Mcp-Session-Id", sid, &size, WINHTTP_NO_HEADER_INDEX)) session_id_ = narrow(sid);

    std::string out;
    for (;;) {
        DWORD avail = 0;
        if (!WinHttpQueryDataAvailable(req, &avail)) throw McpError(last_error_text("WinHttpQueryDataAvailable"));
        if (!avail) break;
        size_t at = out.size();
        out.resize(at + avail);
        DWORD got = 0;
        if (!WinHttpReadData(req, out.data() + at, avail, &got)) throw McpError(last_error_text("WinHttpReadData"));
        out.resize(at + got);
    }
    if (status == 404) throw McpSessionLost("rpfm_server forgot the MCP session");
    if (status >= 400) throw McpError("rpfm_server answered HTTP " + std::to_string(status) + ": " + out.substr(0, 300));
    return out;
}

Json McpClient::rpc(const std::string& method, const Json& params, bool notification) {
    Json msg = Json::object();
    msg.set("jsonrpc", "2.0");
    msg.set("method", method);
    long long id = 0;
    if (!notification) {
        id = next_id_++;
        msg.set("id", id);
    }
    msg.set("params", params);
    const std::string body = post(msg.dump());
    if (notification) return Json();
    for (const std::string& ev : sse_events(body)) {
        Json j = Json::parse(ev);
        const Json* jid = j.find("id");
        if (!jid || !jid->is_number() || static_cast<long long>(jid->as_number()) != id) continue;
        if (const Json* err = j.find("error")) {
            const Json* m = err->find("message");
            throw McpError("rpfm_server: " + (m && m->is_string() ? m->as_string() : err->dump()));
        }
        const Json* r = j.find("result");
        if (!r) throw McpError("rpfm_server: reply without result");
        return *r;
    }
    // A plain JSON reply (no event stream).
    if (!body.empty() && body[0] == '{') {
        Json j = Json::parse(body);
        if (const Json* r = j.find("result")) return *r;
    }
    throw McpError("rpfm_server: no reply to " + method);
}

void McpClient::connect() {
    session_id_.clear();
    Json p = Json::object();
    p.set("protocolVersion", "2025-03-26");
    p.set("capabilities", Json::object());
    Json ci = Json::object();
    ci.set("name", "warscape-community-editor");
    ci.set("version", "0");
    p.set("clientInfo", std::move(ci));
    Json r = rpc("initialize", p);
    if (const Json* si = r.find("serverInfo"))
        if (const Json* n = si->find("name"); n && n->is_string()) server_name_ = n->as_string();
    if (session_id_.empty()) throw McpError("rpfm_server did not open a session");
    rpc("notifications/initialized", Json::object(), true);
}

Json McpClient::call(const std::string& tool, const Json& arguments) {
    Json p = Json::object();
    p.set("name", tool);
    p.set("arguments", arguments);
    Json r = rpc("tools/call", p);
    std::string text;
    if (const Json* c = r.find("content"); c && c->is_array())
        for (const Json& part : c->as_array())
            if (const Json* t = part.find("text"); t && t->is_string()) text += t->as_string();
    const Json* is_err = r.find("isError");
    if (is_err && is_err->is_bool() && is_err->as_bool()) {
        std::string msg = text;
        try {
            Json e = Json::parse(text);
            if (const Json* m = e.find("Error"); m && m->is_string()) msg = m->as_string();
        } catch (const std::exception&) {}
        throw McpError(tool + ": " + msg);
    }
    if (!text.empty() && (text[0] == '{' || text[0] == '[' || text[0] == '"')) {
        try { return Json::parse(text); } catch (const std::exception&) {}
    }
    return Json(text);
}

std::vector<std::string> McpClient::tool_names() {
    std::vector<std::string> names;
    Json r = rpc("tools/list", Json::object());
    if (const Json* t = r.find("tools"); t && t->is_array())
        for (const Json& e : t->as_array())
            if (const Json* n = e.find("name"); n && n->is_string()) names.push_back(n->as_string());
    return names;
}

} // namespace wce
