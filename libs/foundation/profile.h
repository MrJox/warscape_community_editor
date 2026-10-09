#pragma once
// Scope timers for finding hot paths: WCE_PROF("name") times the rest of the enclosing block into a named slot (total time, calls,
// worst call). Slots are registered once per call site and updated with relaxed atomics, so scopes can sit in worker threads.
// `profile_report` / the `profile` command print the slots sorted by total time; a few ns per scope, always compiled in.
#include <atomic>
#include <chrono>
#include <cstdint>
#include <string>

namespace wce::prof {

struct Slot {
    const char* name = nullptr;
    std::atomic<uint64_t> ns{0}, calls{0}, worst{0};
    Slot* next = nullptr;
};

Slot* slot(const char* name);                      // registers (or finds) the slot of that name
std::string report(bool reset, double min_ms = 0.0);
void reset();

inline uint64_t now_ns() {
    return uint64_t(std::chrono::duration_cast<std::chrono::nanoseconds>(std::chrono::steady_clock::now().time_since_epoch()).count());
}

class Scope {
public:
    explicit Scope(Slot* s) : slot_(s), t0_(now_ns()) {}
    ~Scope() {
        const uint64_t d = now_ns() - t0_;
        slot_->ns.fetch_add(d, std::memory_order_relaxed);
        slot_->calls.fetch_add(1, std::memory_order_relaxed);
        uint64_t w = slot_->worst.load(std::memory_order_relaxed);
        while (d > w && !slot_->worst.compare_exchange_weak(w, d, std::memory_order_relaxed)) {}
    }
    Scope(const Scope&) = delete;
    Scope& operator=(const Scope&) = delete;

private:
    Slot* slot_;
    uint64_t t0_;
};

} // namespace wce::prof

#define WCE_PROF_CAT2(a, b) a##b
#define WCE_PROF_CAT(a, b) WCE_PROF_CAT2(a, b)
#define WCE_PROF(name) \
    static ::wce::prof::Slot* const WCE_PROF_CAT(wce_prof_slot_, __LINE__) = ::wce::prof::slot(name); \
    ::wce::prof::Scope WCE_PROF_CAT(wce_prof_scope_, __LINE__)(WCE_PROF_CAT(wce_prof_slot_, __LINE__))
