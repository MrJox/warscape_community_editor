#include "foundation/profile.h"

#include <algorithm>
#include <cstdio>
#include <cstring>
#include <mutex>
#include <vector>

namespace wce::prof {

namespace {
std::mutex g_mutex;
Slot* g_head = nullptr;
}

Slot* slot(const char* name) {
    std::lock_guard<std::mutex> lock(g_mutex);
    for (Slot* s = g_head; s; s = s->next)
        if (std::strcmp(s->name, name) == 0) return s;
    Slot* s = new Slot;   // lives for the whole process
    s->name = name;
    s->next = g_head;
    g_head = s;
    return s;
}

void reset() {
    std::lock_guard<std::mutex> lock(g_mutex);
    for (Slot* s = g_head; s; s = s->next) {
        s->ns = 0;
        s->calls = 0;
        s->worst = 0;
    }
}

std::string report(bool do_reset, double min_ms) {
    std::vector<Slot*> v;
    {
        std::lock_guard<std::mutex> lock(g_mutex);
        for (Slot* s = g_head; s; s = s->next) v.push_back(s);
    }
    std::sort(v.begin(), v.end(), [](const Slot* a, const Slot* b) { return a->ns.load() > b->ns.load(); });
    std::string out;
    char line[256];
    for (const Slot* s : v) {
        const double ms = double(s->ns.load()) * 1e-6;
        const uint64_t calls = s->calls.load();
        if (ms < min_ms || calls == 0) continue;
        std::snprintf(line, sizeof line, "  %-34s %10.2f ms %9llu calls %9.4f ms/call  worst %8.2f ms\n", s->name, ms, (unsigned long long)calls, ms / double(calls),
                      double(s->worst.load()) * 1e-6);
        out += line;
    }
    if (do_reset) reset();
    return out;
}

} // namespace wce::prof
