#pragma once
// Process-wide log: a ring of the last entries (the editor's Log panel shows them) plus an optional echo to stderr (CLI runs, scripts).
// Thread-safe; messages are UTF-8.
#include <cstdint>
#include <cstdio>
#include <deque>
#include <mutex>
#include <string>
#include <vector>

namespace wce {

enum class LogLevel { Info, Warning, Error };

struct LogEntry {
    uint64_t sequence = 0;
    LogLevel level = LogLevel::Info;
    std::string text;
};

class Log {
public:
    static Log& instance();

    void add(LogLevel level, std::string text);
    static void info(const std::string& t) { instance().add(LogLevel::Info, t); }
    static void warn(const std::string& t) { instance().add(LogLevel::Warning, t); }
    static void error(const std::string& t) { instance().add(LogLevel::Error, t); }

    std::vector<LogEntry> entries() const;             // oldest first
    uint64_t sequence() const;                         // entries added so far (changes whenever the log does)
    size_t count(LogLevel level) const;                // entries of that level currently kept
    void clear();
    void echo_to_stderr(bool on) { echo_ = on; }
    // Also writes every entry, flushed, to this file (truncated now): what an interactive run logged stays readable after it ends. Returns false when it cannot be opened.
    bool open_file(const std::string& path);

private:
    mutable std::mutex mutex_;
    std::deque<LogEntry> entries_;
    uint64_t sequence_ = 0;
    bool echo_ = false;
    std::FILE* file_ = nullptr;
    static constexpr size_t kKeep = 2000;
};

} // namespace wce
