#pragma once
// GPU time per named section of a frame, from D3D11 timestamp queries. begin_frame() / end_frame() bracket a frame, mark("name")
// starts a section (and ends the previous one). Results arrive a few frames later (ring of queries); `last()` holds the newest complete
// frame, `average()` a running mean per section since the last reset.
#include <d3d11.h>
#include <wrl/client.h>

#include <map>
#include <string>
#include <vector>

namespace wce {

class GpuTimer {
public:
    GpuTimer(ID3D11Device* dev, ID3D11DeviceContext* ctx) : dev_(dev), ctx_(ctx) {}

    void begin_frame();
    void mark(const char* name);          // ends the open section, starts `name`
    void end_frame();                     // ends the open section, closes the frame
    double frame_ms() const { return last_total_; }
    const std::vector<std::pair<std::string, double>>& last() const { return last_; }
    std::string report() const;           // mean over the frames since reset(), sorted by time
    void reset() { sum_.clear(); frames_ = 0; }

private:
    struct Section { std::string name; Microsoft::WRL::ComPtr<ID3D11Query> start, end; };
    struct Frame {
        Microsoft::WRL::ComPtr<ID3D11Query> disjoint, begin, finish;
        std::vector<Section> sections;
        bool pending = false;
    };
    void collect();
    ID3D11Query* new_query(D3D11_QUERY type, Microsoft::WRL::ComPtr<ID3D11Query>& q);

    static constexpr int kRing = 6;
    ID3D11Device* dev_;
    ID3D11DeviceContext* ctx_;
    Frame ring_[kRing];
    int cur_ = 0;
    bool open_ = false, in_frame_ = false;
    std::vector<std::pair<std::string, double>> last_;
    double last_total_ = 0;
    std::map<std::string, double> sum_;
    double sum_total_ = 0;
    int frames_ = 0;
};

} // namespace wce
