#pragma once
// Simulation clock. Live: wall time since start(). Fixed-step: every tick() advances time by exactly `step` seconds, whatever the
// machine does, so wind, clouds, particles and tone-map adaptation (all driven by the clock's time and dt) give the same frame N
// every run. Screenshots, the render regression gate and the future cinematic recorder use the fixed-step clock.
#include <chrono>
#include <cstdint>

namespace wce {

class Clock {
public:
    static Clock live() { return Clock(0.0); }
    static Clock fixed(double step_seconds) { return Clock(step_seconds > 0 ? step_seconds : 1.0 / 60.0); }

    bool fixed_step() const { return step_ > 0; }
    double step() const { return step_; }

    // Start counting from now / from zero (a restarted clock makes time go backwards for its users, who reset their simulations).
    void restart() {
        frame_ = 0;
        time_ = 0;
        dt_ = 0;
        start_ = std::chrono::steady_clock::now();
        last_ = start_;
    }
    // Call once per rendered frame, before the frame uses time()/dt(). The first tick gives time 0.
    void tick() {
        if (fixed_step()) {
            time_ = double(frame_) * step_;
            dt_ = frame_ == 0 ? 0.0f : float(step_);
        } else {
            const auto now = std::chrono::steady_clock::now();
            time_ = std::chrono::duration<double>(now - start_).count();
            dt_ = frame_ == 0 ? 0.0f : float(std::chrono::duration<double>(now - last_).count());
            last_ = now;
        }
        ++frame_;
    }

    double time() const { return time_; }          // seconds since restart
    float dt() const { return dt_; }               // seconds since the previous tick (0 on the first)
    uint64_t frames() const { return frame_; }     // ticks so far

private:
    explicit Clock(double step) : step_(step) { restart(); }

    double step_ = 0;
    double time_ = 0;
    float dt_ = 0;
    uint64_t frame_ = 0;
    std::chrono::steady_clock::time_point start_, last_;
};

} // namespace wce
