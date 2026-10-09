#include "render/gpu_timer.h"

#include <algorithm>
#include <cstdio>

namespace wce {

ID3D11Query* GpuTimer::new_query(D3D11_QUERY type, Microsoft::WRL::ComPtr<ID3D11Query>& q) {
    if (!q) {
        D3D11_QUERY_DESC d{type, 0};
        dev_->CreateQuery(&d, &q);
    }
    return q.Get();
}

void GpuTimer::begin_frame() {
    collect();
    Frame& f = ring_[cur_];
    if (f.pending) { in_frame_ = false; return; }   // the GPU is more than a ring behind: skip this frame's timing
    f.sections.clear();
    ctx_->Begin(new_query(D3D11_QUERY_TIMESTAMP_DISJOINT, f.disjoint));
    ctx_->End(new_query(D3D11_QUERY_TIMESTAMP, f.begin));
    in_frame_ = true;
    open_ = false;
}

void GpuTimer::mark(const char* name) {
    if (!in_frame_) return;
    Frame& f = ring_[cur_];
    if (open_) ctx_->End(f.sections.back().end.Get());
    Section s;
    s.name = name;
    new_query(D3D11_QUERY_TIMESTAMP, s.start);
    new_query(D3D11_QUERY_TIMESTAMP, s.end);
    ctx_->End(s.start.Get());
    f.sections.push_back(std::move(s));
    open_ = true;
}

void GpuTimer::end_frame() {
    if (!in_frame_) return;
    Frame& f = ring_[cur_];
    if (open_) ctx_->End(f.sections.back().end.Get());
    open_ = false;
    ctx_->End(new_query(D3D11_QUERY_TIMESTAMP, f.finish));
    ctx_->End(f.disjoint.Get());
    f.pending = true;
    in_frame_ = false;
    cur_ = (cur_ + 1) % kRing;
}

void GpuTimer::collect() {
    for (int k = 0; k < kRing; ++k) {
        Frame& f = ring_[(cur_ + k) % kRing];   // oldest first
        if (!f.pending) continue;
        D3D11_QUERY_DATA_TIMESTAMP_DISJOINT dj;
        if (ctx_->GetData(f.disjoint.Get(), &dj, sizeof dj, D3D11_ASYNC_GETDATA_DONOTFLUSH) != S_OK) break;
        f.pending = false;
        if (dj.Disjoint) continue;
        auto read = [&](ID3D11Query* q, UINT64& t) { return ctx_->GetData(q, &t, sizeof t, 0) == S_OK; };
        UINT64 t0 = 0, t1 = 0;
        if (!read(f.begin.Get(), t0) || !read(f.finish.Get(), t1)) continue;
        const double to_ms = 1000.0 / double(dj.Frequency);
        last_.clear();
        last_total_ = double(t1 - t0) * to_ms;
        for (Section& s : f.sections) {
            UINT64 a = 0, b = 0;
            if (!read(s.start.Get(), a) || !read(s.end.Get(), b)) continue;
            const double ms = double(b - a) * to_ms;
            last_.emplace_back(s.name, ms);
            sum_[s.name] += ms;
        }
        sum_total_ += last_total_;
        ++frames_;
    }
}

std::string GpuTimer::report() const {
    if (!frames_) return "  (no GPU timing yet)\n";
    std::vector<std::pair<std::string, double>> v(sum_.begin(), sum_.end());
    std::sort(v.begin(), v.end(), [](auto& a, auto& b) { return a.second > b.second; });
    std::string out;
    char line[128];
    std::snprintf(line, sizeof line, "  GPU frame (mean of %d): %.2f ms\n", frames_, sum_total_ / frames_);
    out += line;
    for (auto& [n, ms] : v) {
        std::snprintf(line, sizeof line, "    %-30s %8.3f ms\n", n.c_str(), ms / frames_);
        out += line;
    }
    std::snprintf(line, sizeof line, "  last frame in order (%.2f ms in all):", last_total_);
    out += line;
    for (auto& [n, ms] : last_) {
        std::snprintf(line, sizeof line, " %s %.2f |", n.c_str(), ms);
        out += line;
    }
    out += "\n";
    return out;
}

} // namespace wce
