#pragma once
#include <string>
#include <vector>

namespace wce {

struct ProcessResult {
    int exit_code = -1;
    std::string out; // stdout and stderr, merged
};

// Runs a console program without a window and captures its output. Arguments are quoted as needed.
ProcessResult run_process(const std::wstring& exe, const std::vector<std::wstring>& args);

std::wstring widen(const std::string& utf8);
std::string narrow(const std::wstring& w);

} // namespace wce
