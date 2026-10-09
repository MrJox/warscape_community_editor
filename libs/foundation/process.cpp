#include "foundation/process.h"

#include <windows.h>

namespace wce {

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

ProcessResult run_process(const std::wstring& exe, const std::vector<std::wstring>& args) {
    ProcessResult r;
    std::wstring cmd;
    append_quoted(cmd, exe);
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
        r.out = "cannot start " + narrow(exe);
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

bool start_detached(const std::wstring& exe, const std::vector<std::wstring>& args) {
    std::wstring cmd;
    append_quoted(cmd, exe);
    for (auto& a : args) {
        cmd += L' ';
        append_quoted(cmd, a);
    }
    STARTUPINFOW si{};
    si.cb = sizeof si;
    PROCESS_INFORMATION pi{};
    // The working directory is the program's own folder: rpfm_server finds its data next to itself.
    std::wstring dir = exe.substr(0, exe.find_last_of(L"\\/"));
    BOOL ok = CreateProcessW(exe.c_str(), cmd.data(), nullptr, nullptr, FALSE, CREATE_NO_WINDOW | DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP, nullptr,
                             dir.empty() || dir == exe ? nullptr : dir.c_str(), &si, &pi);
    if (!ok) return false;
    CloseHandle(pi.hProcess);
    CloseHandle(pi.hThread);
    return true;
}

} // namespace wce
