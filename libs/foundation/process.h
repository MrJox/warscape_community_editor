#pragma once
#include <filesystem>
#include <string>
#include <vector>

namespace wce {

struct ProcessResult {
    int exit_code = -1;
    std::string out; // stdout and stderr, merged
};

// Runs a console program without a window and captures its output. Arguments are quoted as needed.
ProcessResult run_process(const std::filesystem::path& exe, const std::vector<std::wstring>& args);

// Starts a program without a window and without waiting for it; it keeps running after this process exits. Returns false when it
// cannot be started. The child gets no inherited handles and its output is discarded.
bool start_detached(const std::filesystem::path& exe, const std::vector<std::wstring>& args);

// A program's file name: "name.exe" on Windows, "name" elsewhere.
inline std::string exe_name(const std::string& base) {
#ifdef _WIN32
    return base + ".exe";
#else
    return base;
#endif
}

std::wstring widen(const std::string& utf8);
std::string narrow(const std::wstring& w);

} // namespace wce
