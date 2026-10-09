#include "foundation/log.h"

#include <cstdio>

namespace wce {

Log& Log::instance() {
    static Log log;
    return log;
}

void Log::add(LogLevel level, std::string text) {
    std::lock_guard<std::mutex> lock(mutex_);
    if (echo_) std::fprintf(stderr, "[%s] %s\n", level == LogLevel::Error ? "error" : level == LogLevel::Warning ? "warn" : "info", text.c_str());
    entries_.push_back({++sequence_, level, std::move(text)});
    while (entries_.size() > kKeep) entries_.pop_front();
}

std::vector<LogEntry> Log::entries() const {
    std::lock_guard<std::mutex> lock(mutex_);
    return {entries_.begin(), entries_.end()};
}

uint64_t Log::sequence() const {
    std::lock_guard<std::mutex> lock(mutex_);
    return sequence_;
}

size_t Log::count(LogLevel level) const {
    std::lock_guard<std::mutex> lock(mutex_);
    size_t n = 0;
    for (const LogEntry& e : entries_) n += e.level == level;
    return n;
}

void Log::clear() {
    std::lock_guard<std::mutex> lock(mutex_);
    entries_.clear();
}

} // namespace wce
