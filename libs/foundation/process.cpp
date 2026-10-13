#include "foundation/process.h"

#ifdef _WIN32
#include <windows.h>
#else
#include <cerrno>
#include <cstdint>
#include <fcntl.h>
#include <spawn.h>
#include <sys/wait.h>
#include <unistd.h>

extern char** environ;
#endif

namespace wce {

#ifdef _WIN32

std::wstring widen(const std::string& s) {
    if (s.empty()) return {};
    int n = MultiByteToWideChar(CP_UTF8, 0, s.data(), int(s.size()), nullptr, 0);
    std::wstring w(n, L'\0');
    MultiByteToWideChar(CP_UTF8, 0, s.data(), int(s.size()), w.data(), n);
    return w;
}

std::string narrow(const std::wstring& w) {
    if (w.empty()) return {};
    int n = WideCharToMultiByte(CP_UTF8, 0, w.data(), int(w.size()), nullptr, 0, nullptr, nullptr);
    std::string s(n, '\0');
    WideCharToMultiByte(CP_UTF8, 0, w.data(), int(w.size()), s.data(), n, nullptr, nullptr);
    return s;
}

static void append_quoted(std::wstring& cmd, const std::wstring& arg) {
    if (!arg.empty() && arg.find_first_of(L" \t\"") == std::wstring::npos) {
        cmd += arg;
        return;
    }
    cmd += L'"';
    size_t backslashes = 0;
    for (wchar_t c : arg) {
        if (c == L'\\') { ++backslashes; continue; }
        if (c == L'"') cmd.append(backslashes * 2 + 1, L'\\');
        else cmd.append(backslashes, L'\\');
        backslashes = 0;
        cmd += c;
    }
    cmd.append(backslashes * 2, L'\\');
    cmd += L'"';
}

ProcessResult run_process(const std::filesystem::path& exe, const std::vector<std::wstring>& args) {
    ProcessResult r;
    std::wstring cmd;
    append_quoted(cmd, exe.wstring());
    for (auto& a : args) {
        cmd += L' ';
        append_quoted(cmd, a);
    }

    SECURITY_ATTRIBUTES sa{sizeof sa, nullptr, TRUE};
    HANDLE rd = nullptr, wr = nullptr;
    if (!CreatePipe(&rd, &wr, &sa, 0)) return r;
    SetHandleInformation(rd, HANDLE_FLAG_INHERIT, 0);

    STARTUPINFOW si{};
    si.cb = sizeof si;
    si.dwFlags = STARTF_USESTDHANDLES;
    si.hStdOutput = wr;
    si.hStdError = wr;
    si.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
    PROCESS_INFORMATION pi{};
    BOOL ok = CreateProcessW(exe.c_str(), cmd.data(), nullptr, nullptr, TRUE, CREATE_NO_WINDOW, nullptr, nullptr, &si, &pi);
    CloseHandle(wr);
    if (!ok) {
        CloseHandle(rd);
        r.out = "cannot start " + narrow(exe.wstring());
        return r;
    }
    char buf[65536];
    DWORD got;
    while (ReadFile(rd, buf, sizeof buf, &got, nullptr) && got) r.out.append(buf, got);
    CloseHandle(rd);
    WaitForSingleObject(pi.hProcess, INFINITE);
    DWORD code = 0;
    GetExitCodeProcess(pi.hProcess, &code);
    r.exit_code = int(code);
    CloseHandle(pi.hProcess);
    CloseHandle(pi.hThread);
    return r;
}

bool start_detached(const std::filesystem::path& exe, const std::vector<std::wstring>& args) {
    std::wstring cmd;
    append_quoted(cmd, exe.wstring());
    for (auto& a : args) {
        cmd += L' ';
        append_quoted(cmd, a);
    }
    STARTUPINFOW si{};
    si.cb = sizeof si;
    PROCESS_INFORMATION pi{};
    // The working directory is the program's own folder: rpfm_server finds its data next to itself.
    const std::wstring exe_w = exe.wstring();
    std::wstring dir = exe_w.substr(0, exe_w.find_last_of(L"\\/"));
    BOOL ok = CreateProcessW(exe_w.c_str(), cmd.data(), nullptr, nullptr, FALSE, CREATE_NO_WINDOW | DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP, nullptr,
                             dir.empty() || dir == exe_w ? nullptr : dir.c_str(), &si, &pi);
    if (!ok) return false;
    CloseHandle(pi.hProcess);
    CloseHandle(pi.hThread);
    return true;
}

#else

// wchar_t is UTF-32 here.
std::wstring widen(const std::string& s) {
    std::wstring w;
    w.reserve(s.size());
    for (size_t i = 0; i < s.size();) {
        const unsigned char c = static_cast<unsigned char>(s[i]);
        uint32_t cp;
        int n;
        if (c < 0x80) { cp = c; n = 1; }
        else if ((c & 0xE0) == 0xC0) { cp = c & 0x1F; n = 2; }
        else if ((c & 0xF0) == 0xE0) { cp = c & 0x0F; n = 3; }
        else if ((c & 0xF8) == 0xF0) { cp = c & 0x07; n = 4; }
        else { w += wchar_t(0xFFFD); ++i; continue; }
        if (i + n > s.size()) { w += wchar_t(0xFFFD); break; }
        for (int k = 1; k < n; ++k) cp = (cp << 6) | (static_cast<unsigned char>(s[i + k]) & 0x3F);
        w += wchar_t(cp);
        i += n;
    }
    return w;
}

std::string narrow(const std::wstring& w) {
    std::string s;
    s.reserve(w.size());
    for (wchar_t wc : w) {
        const uint32_t cp = uint32_t(wc);
        if (cp < 0x80) {
            s += char(cp);
        } else if (cp < 0x800) {
            s += char(0xC0 | (cp >> 6));
            s += char(0x80 | (cp & 0x3F));
        } else if (cp < 0x10000) {
            s += char(0xE0 | (cp >> 12));
            s += char(0x80 | ((cp >> 6) & 0x3F));
            s += char(0x80 | (cp & 0x3F));
        } else {
            s += char(0xF0 | (cp >> 18));
            s += char(0x80 | ((cp >> 12) & 0x3F));
            s += char(0x80 | ((cp >> 6) & 0x3F));
            s += char(0x80 | (cp & 0x3F));
        }
    }
    return s;
}

namespace {

// argv for exec: the strings and the pointer array, built before any fork.
struct Argv {
    std::vector<std::string> strings;
    std::vector<char*> ptrs;
    Argv(const std::filesystem::path& exe, const std::vector<std::wstring>& args) {
        strings.push_back(exe.string());
        for (const auto& a : args) strings.push_back(narrow(a));
        for (auto& s : strings) ptrs.push_back(s.data());
        ptrs.push_back(nullptr);
    }
};

} // namespace

ProcessResult run_process(const std::filesystem::path& exe, const std::vector<std::wstring>& args) {
    ProcessResult r;
    Argv argv(exe, args);
    int fds[2];
    if (pipe2(fds, O_CLOEXEC) != 0) return r;
    posix_spawn_file_actions_t fa;
    posix_spawn_file_actions_init(&fa);
    posix_spawn_file_actions_adddup2(&fa, fds[1], STDOUT_FILENO);
    posix_spawn_file_actions_adddup2(&fa, fds[1], STDERR_FILENO);
    pid_t pid = 0;
    const int err = posix_spawn(&pid, argv.ptrs[0], &fa, nullptr, argv.ptrs.data(), environ);
    posix_spawn_file_actions_destroy(&fa);
    close(fds[1]);
    if (err != 0) {
        close(fds[0]);
        r.out = "cannot start " + exe.string();
        return r;
    }
    char buf[65536];
    for (;;) {
        const ssize_t got = read(fds[0], buf, sizeof buf);
        if (got > 0) r.out.append(buf, size_t(got));
        else if (got < 0 && errno == EINTR) continue;
        else break;
    }
    close(fds[0]);
    int status = 0;
    while (waitpid(pid, &status, 0) < 0 && errno == EINTR) {}
    r.exit_code = WIFEXITED(status) ? WEXITSTATUS(status) : -1;
    return r;
}

bool start_detached(const std::filesystem::path& exe, const std::vector<std::wstring>& args) {
    Argv argv(exe, args);
    // The working directory is the program's own folder: rpfm_server finds its data next to itself.
    const std::string dir = exe.parent_path().string();
    int status_pipe[2];   // the grandchild's exec failure (errno) comes back through it; a successful exec closes it
    if (pipe2(status_pipe, O_CLOEXEC) != 0) return false;
    const pid_t child = fork();
    if (child < 0) {
        close(status_pipe[0]);
        close(status_pipe[1]);
        return false;
    }
    if (child == 0) {   // only async-signal-safe calls from here on
        setsid();
        const pid_t grandchild = fork();   // reparented to init: no zombie, and it outlives this process
        if (grandchild != 0) _exit(grandchild < 0 ? 1 : 0);
        const int null = open("/dev/null", O_RDWR);
        if (null >= 0) {
            dup2(null, STDIN_FILENO);
            dup2(null, STDOUT_FILENO);
            dup2(null, STDERR_FILENO);
        }
        if (!dir.empty() && chdir(dir.c_str()) != 0) {}
        execv(argv.ptrs[0], argv.ptrs.data());
        const int e = errno;
        if (write(status_pipe[1], &e, sizeof e) < 0) {}
        _exit(127);
    }
    close(status_pipe[1]);
    int status = 0;
    while (waitpid(child, &status, 0) < 0 && errno == EINTR) {}
    int exec_errno = 0;
    ssize_t got;
    while ((got = read(status_pipe[0], &exec_errno, sizeof exec_errno)) < 0 && errno == EINTR) {}
    close(status_pipe[0]);
    return WIFEXITED(status) && WEXITSTATUS(status) == 0 && got == 0;
}

#endif

} // namespace wce
