#include "vfs/rpfm_client.h"

#include "foundation/process.h"

#ifdef _WIN32
#include <windows.h>
#include <winhttp.h>
#else
#include <curl/curl.h>
#include <poll.h>

#include <chrono>
#include <mutex>
#endif

#include <cstdio>
#include <vector>

namespace wce {

namespace {

// The oldest server that speaks this protocol.
constexpr int kMinMajor = 5, kMinMinor = 2;
// A request may legitimately keep the answer back for minutes (a batch extract of a large pack).
constexpr int kReceiveTimeoutMs = 600000;

// The connection broke (closed, reset, timed out): the client reconnects.
struct Broken : std::runtime_error {
    using std::runtime_error::runtime_error;
};

std::string where(const std::string& host, int port) { return host + ":" + std::to_string(port); }

#ifdef _WIN32
std::string last_error_text(const char* what) { return std::string(what) + " (Windows error " + std::to_string(GetLastError()) + ")"; }
bool unreachable(DWORD e) { return e == ERROR_WINHTTP_CANNOT_CONNECT || e == ERROR_WINHTTP_TIMEOUT || e == ERROR_WINHTTP_CONNECTION_ERROR; }

struct Handle {
    HINTERNET h = nullptr;
    ~Handle() { if (h) WinHttpCloseHandle(h); }
};
#else
void curl_init_once() {
    static std::once_flag global_init;
    std::call_once(global_init, [] { curl_global_init(CURL_GLOBAL_DEFAULT); });
}

size_t curl_body(char* data, size_t size, size_t n, void* user) {
    static_cast<std::string*>(user)->append(data, size * n);
    return size * n;
}

// Waits until the connection of `curl` can be read (or written). False on timeout.
bool wait_socket(CURL* curl, bool write, int timeout_ms) {
    curl_socket_t s = CURL_SOCKET_BAD;
    if (curl_easy_getinfo(curl, CURLINFO_ACTIVESOCKET, &s) != CURLE_OK || s == CURL_SOCKET_BAD) throw Broken("the connection is gone");
    pollfd p{static_cast<int>(s), short(write ? POLLOUT : POLLIN), 0};
    const int r = poll(&p, 1, timeout_ms);
    if (r < 0) throw Broken("poll failed");
    return r > 0;
}
#endif

// GET http://<host>:<port><path>: the body. Throws RpfmUnreachable when nothing listens.
std::string http_get(const std::string& host, int port, const std::string& path) {
#ifdef _WIN32
    Handle session{WinHttpOpen(L"WarscapeCommunityEditor", WINHTTP_ACCESS_TYPE_NO_PROXY, WINHTTP_NO_PROXY_NAME, WINHTTP_NO_PROXY_BYPASS, 0)};
    if (!session.h) throw RpfmError(last_error_text("WinHttpOpen"));
    WinHttpSetTimeouts(session.h, 5000, 3000, 10000, 10000);
    Handle connection{WinHttpConnect(session.h, widen(host).c_str(), INTERNET_PORT(port), 0)};
    if (!connection.h) throw RpfmError(last_error_text("WinHttpConnect"));
    Handle req{WinHttpOpenRequest(connection.h, L"GET", widen(path).c_str(), nullptr, WINHTTP_NO_REFERER, WINHTTP_DEFAULT_ACCEPT_TYPES, 0)};
    if (!req.h) throw RpfmError(last_error_text("WinHttpOpenRequest"));
    if (!WinHttpSendRequest(req.h, WINHTTP_NO_ADDITIONAL_HEADERS, 0, WINHTTP_NO_REQUEST_DATA, 0, 0, 0)) {
        const DWORD e = GetLastError();
        if (unreachable(e)) throw RpfmUnreachable("rpfm_server is not reachable on " + where(host, port));
        throw RpfmError("WinHttpSendRequest failed (Windows error " + std::to_string(e) + ")");
    }
    if (!WinHttpReceiveResponse(req.h, nullptr)) throw RpfmError(last_error_text("WinHttpReceiveResponse"));
    std::string out;
    for (;;) {
        DWORD avail = 0;
        if (!WinHttpQueryDataAvailable(req.h, &avail)) throw RpfmError(last_error_text("WinHttpQueryDataAvailable"));
        if (!avail) break;
        const size_t at = out.size();
        out.resize(at + avail);
        DWORD got = 0;
        if (!WinHttpReadData(req.h, out.data() + at, avail, &got)) throw RpfmError(last_error_text("WinHttpReadData"));
        out.resize(at + got);
    }
    return out;
#else
    curl_init_once();
    CURL* curl = curl_easy_init();
    if (!curl) throw RpfmError("curl_easy_init failed");
    struct Cleanup {
        CURL* c;
        ~Cleanup() { curl_easy_cleanup(c); }
    } cleanup{curl};
    const std::string url = "http://" + where(host, port) + path;
    std::string out;
    curl_easy_setopt(curl, CURLOPT_URL, url.c_str());
    curl_easy_setopt(curl, CURLOPT_USERAGENT, "WarscapeCommunityEditor");
    curl_easy_setopt(curl, CURLOPT_NOPROXY, "*");
    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, curl_body);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, &out);
    curl_easy_setopt(curl, CURLOPT_NOSIGNAL, 1L);
    curl_easy_setopt(curl, CURLOPT_CONNECTTIMEOUT_MS, 5000L);
    curl_easy_setopt(curl, CURLOPT_TIMEOUT_MS, 10000L);
    const CURLcode rc = curl_easy_perform(curl);
    if (rc == CURLE_COULDNT_CONNECT || rc == CURLE_COULDNT_RESOLVE_HOST || rc == CURLE_OPERATION_TIMEDOUT)
        throw RpfmUnreachable("rpfm_server is not reachable on " + where(host, port));
    if (rc != CURLE_OK) throw RpfmError(std::string("HTTP request to rpfm_server failed: ") + curl_easy_strerror(rc));
    return out;
#endif
}

} // namespace

// ---- the WebSocket

#ifdef _WIN32

struct RpfmClient::Socket {
    Handle session, connection, ws;

    Socket(const std::string& host, int port, const std::string& path) {
        session.h = WinHttpOpen(L"WarscapeCommunityEditor", WINHTTP_ACCESS_TYPE_NO_PROXY, WINHTTP_NO_PROXY_NAME, WINHTTP_NO_PROXY_BYPASS, 0);
        if (!session.h) throw RpfmError(last_error_text("WinHttpOpen"));
        // resolve, connect, send, receive: the WebSocket inherits the receive timeout
        WinHttpSetTimeouts(session.h, 5000, 3000, 60000, kReceiveTimeoutMs);
        connection.h = WinHttpConnect(session.h, widen(host).c_str(), INTERNET_PORT(port), 0);
        if (!connection.h) throw RpfmError(last_error_text("WinHttpConnect"));
        Handle req{WinHttpOpenRequest(connection.h, L"GET", widen(path).c_str(), nullptr, WINHTTP_NO_REFERER, WINHTTP_DEFAULT_ACCEPT_TYPES, 0)};
        if (!req.h) throw RpfmError(last_error_text("WinHttpOpenRequest"));
        if (!WinHttpSetOption(req.h, WINHTTP_OPTION_UPGRADE_TO_WEB_SOCKET, nullptr, 0)) throw RpfmError(last_error_text("WinHttpSetOption(UPGRADE_TO_WEB_SOCKET)"));
        if (!WinHttpSendRequest(req.h, WINHTTP_NO_ADDITIONAL_HEADERS, 0, WINHTTP_NO_REQUEST_DATA, 0, 0, 0)) {
            const DWORD e = GetLastError();
            if (unreachable(e)) throw RpfmUnreachable("rpfm_server is not reachable on " + where(host, port));
            throw RpfmError("WinHttpSendRequest failed (Windows error " + std::to_string(e) + ")");
        }
        if (!WinHttpReceiveResponse(req.h, nullptr)) throw RpfmError(last_error_text("WinHttpReceiveResponse"));
        DWORD status = 0, size = sizeof status;
        WinHttpQueryHeaders(req.h, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_HEADER_NAME_BY_INDEX, &status, &size, WINHTTP_NO_HEADER_INDEX);
        if (status != 101) throw RpfmError("rpfm_server refused the WebSocket (HTTP " + std::to_string(status) + ")");
        ws.h = WinHttpWebSocketCompleteUpgrade(req.h, 0);
        if (!ws.h) throw RpfmError(last_error_text("WinHttpWebSocketCompleteUpgrade"));
    }

    ~Socket() {
        if (ws.h) WinHttpWebSocketClose(ws.h, WINHTTP_WEB_SOCKET_SUCCESS_CLOSE_STATUS, nullptr, 0);
    }

    void send(const std::string& text) {
        const DWORD e = WinHttpWebSocketSend(ws.h, WINHTTP_WEB_SOCKET_UTF8_MESSAGE_BUFFER_TYPE, const_cast<char*>(text.data()), DWORD(text.size()));
        if (e != NO_ERROR) throw Broken("WebSocket send failed (Windows error " + std::to_string(e) + ")");
    }

    // One whole message.
    std::string receive() {
        std::string out;
        std::vector<char> buf(1 << 16);
        for (;;) {
            DWORD got = 0;
            WINHTTP_WEB_SOCKET_BUFFER_TYPE type{};
            const DWORD e = WinHttpWebSocketReceive(ws.h, buf.data(), DWORD(buf.size()), &got, &type);
            if (e != NO_ERROR) throw Broken("WebSocket receive failed (Windows error " + std::to_string(e) + ")");
            switch (type) {
            case WINHTTP_WEB_SOCKET_UTF8_FRAGMENT_BUFFER_TYPE:
            case WINHTTP_WEB_SOCKET_BINARY_FRAGMENT_BUFFER_TYPE: out.append(buf.data(), got); break;
            case WINHTTP_WEB_SOCKET_UTF8_MESSAGE_BUFFER_TYPE:
            case WINHTTP_WEB_SOCKET_BINARY_MESSAGE_BUFFER_TYPE: out.append(buf.data(), got); return out;
            case WINHTTP_WEB_SOCKET_CLOSE_BUFFER_TYPE: throw Broken("rpfm_server closed the connection");
            }
        }
    }
};

#else

struct RpfmClient::Socket {
    CURL* curl = nullptr;

    Socket(const std::string& host, int port, const std::string& path) {
        curl_init_once();
        curl = curl_easy_init();
        if (!curl) throw RpfmError("curl_easy_init failed");
        const std::string url = "ws://" + where(host, port) + path;
        curl_easy_setopt(curl, CURLOPT_URL, url.c_str());
        curl_easy_setopt(curl, CURLOPT_USERAGENT, "WarscapeCommunityEditor");
        curl_easy_setopt(curl, CURLOPT_NOPROXY, "*");
        curl_easy_setopt(curl, CURLOPT_NOSIGNAL, 1L);
        curl_easy_setopt(curl, CURLOPT_CONNECTTIMEOUT_MS, 5000L);
        curl_easy_setopt(curl, CURLOPT_CONNECT_ONLY, 2L);   // handshake only; frames go through curl_ws_send / curl_ws_recv
        const CURLcode rc = curl_easy_perform(curl);
        if (rc == CURLE_COULDNT_CONNECT || rc == CURLE_COULDNT_RESOLVE_HOST || rc == CURLE_OPERATION_TIMEDOUT) {
            curl_easy_cleanup(curl);
            throw RpfmUnreachable("rpfm_server is not reachable on " + where(host, port));
        }
        if (rc != CURLE_OK) {
            long status = 0;
            curl_easy_getinfo(curl, CURLINFO_RESPONSE_CODE, &status);
            curl_easy_cleanup(curl);
            throw RpfmError(std::string("WebSocket to rpfm_server failed: ") + curl_easy_strerror(rc) + (status ? " (HTTP " + std::to_string(status) + ")" : std::string()));
        }
    }

    ~Socket() {
        size_t sent = 0;
        curl_ws_send(curl, "", 0, &sent, 0, CURLWS_CLOSE);
        curl_easy_cleanup(curl);
    }

    void send(const std::string& text) {
        size_t at = 0;
        do {
            size_t sent = 0;
            const CURLcode rc = curl_ws_send(curl, text.data() + at, text.size() - at, &sent, 0, CURLWS_TEXT);
            if (rc == CURLE_AGAIN) {
                if (!wait_socket(curl, true, kReceiveTimeoutMs)) throw Broken("WebSocket send timed out");
                continue;
            }
            if (rc != CURLE_OK) throw Broken(std::string("WebSocket send failed: ") + curl_easy_strerror(rc));
            at += sent;
        } while (at < text.size());
    }

    // One whole message.
    std::string receive() {
        std::string out;
        std::vector<char> buf(1 << 16);
        const auto deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(kReceiveTimeoutMs);
        for (;;) {
            size_t got = 0;
            const curl_ws_frame* meta = nullptr;
            const CURLcode rc = curl_ws_recv(curl, buf.data(), buf.size(), &got, &meta);
            if (rc == CURLE_AGAIN) {
                const auto left = std::chrono::duration_cast<std::chrono::milliseconds>(deadline - std::chrono::steady_clock::now()).count();
                if (left <= 0 || !wait_socket(curl, false, int(left))) throw Broken("no answer from rpfm_server in " + std::to_string(kReceiveTimeoutMs / 1000) + " s");
                continue;
            }
            if (rc != CURLE_OK) throw Broken(std::string("WebSocket receive failed: ") + curl_easy_strerror(rc));
            if (meta->flags & CURLWS_CLOSE) throw Broken("rpfm_server closed the connection");
            if (meta->flags & (CURLWS_PING | CURLWS_PONG)) continue;   // libcurl answers pings itself
            out.append(buf.data(), got);
            // complete when this frame has no bytes left and is not followed by another fragment of the same message
            if (meta->bytesleft == 0 && !(meta->flags & CURLWS_CONT)) return out;
        }
    }
};

#endif

// ---- JSON-RPC

RpfmClient::RpfmClient(std::string host, int port) : host_(std::move(host)), port_(port) {}

RpfmClient::~RpfmClient() {
    if (!socket_) return;
    try {
        Json msg = Json::object();
        msg.set("jsonrpc", "2.0");
        msg.set("id", next_id_++);
        msg.set("method", "session.disconnect");
        socket_->send(msg.dump());
        socket_->receive();
    } catch (const std::exception&) {}
}

void RpfmClient::connect() {
    // Older servers answer on /ws too, with another protocol: check the version first, for a clear message.
    const std::string body = http_get(host_, port_, "/version");
    std::string version;
    try {
        if (const Json* v = Json::parse(body).find("version"); v && v->is_string()) version = v->as_string();
    } catch (const std::exception&) {}
    int major = 0, minor = 0;
    if (std::sscanf(version.c_str(), "%d.%d", &major, &minor) != 2)
        throw RpfmError("the program on " + where(host_, port_) + " is not rpfm_server (GET /version: " + body.substr(0, 100) + ")");
    if (major < kMinMajor || (major == kMinMajor && minor < kMinMinor))
        throw RpfmError("rpfm_server " + version + " on " + where(host_, port_) + " is too old: RPFM " + std::to_string(kMinMajor) + "." +
                        std::to_string(kMinMinor) + " or newer is needed");
    open_socket(0);
}

void RpfmClient::open_socket(long long reattach) {
    socket_.reset();
    socket_ = std::make_unique<Socket>(host_, port_, reattach ? "/ws?session_id=" + std::to_string(reattach) : std::string("/ws"));
    // The first message names the session the connection is attached to: ours again, or a new one if the server no longer has ours.
    const Json hello = Json::parse(socket_->receive());
    const Json* method = hello.find("method");
    const Json* params = hello.find("params");
    const Json* id = params ? params->find("session_id") : nullptr;
    if (!method || !method->is_string() || method->as_string() != "session.connected" || !id || !id->is_number())
        throw RpfmError("rpfm_server sent an unexpected greeting: " + hello.dump().substr(0, 200));
    session_id_ = static_cast<long long>(id->as_number());
}

// One request and its answer. Notifications (job.updated) and answers to other requests are skipped.
Json RpfmClient::exchange(const std::string& method, const Json& params) {
    const long long id = next_id_++;
    Json msg = Json::object();
    msg.set("jsonrpc", "2.0");
    msg.set("id", id);
    msg.set("method", method);
    msg.set("params", params);
    socket_->send(msg.dump());
    for (;;) {
        const Json reply = Json::parse(socket_->receive());
        const Json* rid = reply.find("id");
        if (!rid || !rid->is_number() || static_cast<long long>(rid->as_number()) != id) continue;
        if (const Json* err = reply.find("error")) {
            const Json* m = err->find("message");
            const Json* data = err->find("data");
            const Json* kind = data ? data->find("kind") : nullptr;
            throw RpfmError(method + ": " + (m && m->is_string() ? m->as_string() : err->dump()), kind && kind->is_string() ? kind->as_string() : std::string());
        }
        const Json* r = reply.find("result");
        if (!r) throw RpfmError(method + ": answer without a result");
        return *r;
    }
}

Json RpfmClient::call(const std::string& method, const Json& params) {
    if (!socket_) connect();
    try {
        return exchange(method, params);
    } catch (const Broken&) {
        // Reattach: the server keeps a session for a few minutes after its connection drops. The request is sent again, so it may run
        // twice; the ones this client sends are reads, or open a pack that is then simply open twice.
        const long long ours = session_id_;
        try {
            open_socket(ours);
        } catch (const std::exception& e) {   // the server is gone (unreachable) or broke again
            socket_.reset();
            session_id_ = 0;
            throw RpfmSessionLost(std::string("lost the connection to rpfm_server: ") + e.what());
        }
        if (session_id_ != ours) throw RpfmSessionLost("rpfm_server no longer has the session");
        return exchange(method, params);
    }
}

Json RpfmClient::run_job(const std::string& method, const Json& params) {
    const Json started = call(method, params);
    const Json* job = started.find("job");
    if (!job || !job->is_number()) throw RpfmError(method + ": expected a job, got " + started.dump().substr(0, 200));
    Json wait = Json::object();
    wait.set("job", static_cast<long long>(job->as_number()));
    wait.set("timeout_secs", 60);
    for (;;) {
        const Json status = call("job.wait", wait);
        const Json* state = status.find("state");
        const std::string s = state && state->is_string() ? state->as_string() : std::string();
        if (s == "finished") {
            const Json* r = status.find("result");
            return r ? *r : Json();
        }
        if (s == "failed") {
            const Json* err = status.find("error");
            const Json* m = err ? err->find("message") : nullptr;
            throw RpfmError(method + ": " + (m && m->is_string() ? m->as_string() : status.dump().substr(0, 300)));
        }
        if (s == "cancelled") throw RpfmError(method + ": the job was cancelled");
        if (s != "queued" && s != "running") throw RpfmError(method + ": unexpected job state " + status.dump().substr(0, 200));
    }
}

} // namespace wce
