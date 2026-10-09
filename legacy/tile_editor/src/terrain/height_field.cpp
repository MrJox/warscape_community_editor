#include "terrain/height_field.h"

#include <algorithm>
#include <cmath>
#include <stdexcept>

namespace te {

HeightField HeightField::from_png(const PngFile& png, double min_height, double scale) {
    if (png.bit_depth != 16 || png.color_type != 0) throw std::runtime_error("height map must be 16-bit grey");
    HeightField f;
    f.w_ = png.width;
    f.h_ = png.height;
    f.min_ = min_height;
    f.scale_ = scale;
    f.raw_.resize(size_t(f.w_) * f.h_);
    for (size_t i = 0; i < f.raw_.size(); ++i) f.raw_[i] = uint16_t(png.pixels[2 * i] << 8 | png.pixels[2 * i + 1]);
    return f;
}

PngFile HeightField::to_png(const PngFile& base) const {
    PngFile p = base;
    p.pixels.resize(raw_.size() * 2);
    for (size_t i = 0; i < raw_.size(); ++i) { p.pixels[2 * i] = uint8_t(raw_[i] >> 8); p.pixels[2 * i + 1] = uint8_t(raw_[i] & 255); }
    p.pixels_edited = true;
    return p;
}

uint16_t HeightField::to_raw(double m) const {
    double r = (m - min_) / scale_ * 65535.0;
    return uint16_t(std::lround(std::clamp(r, 0.0, 65535.0)));
}

double HeightField::sample(double x, double y) const {
    x = std::clamp(x, 0.0, double(w_ - 1));
    y = std::clamp(y, 0.0, double(h_ - 1));
    uint32_t x0 = uint32_t(x), y0 = uint32_t(y), x1 = std::min(x0 + 1, w_ - 1), y1 = std::min(y0 + 1, h_ - 1);
    double fx = x - x0, fy = y - y0;
    double a = metres(x0, y0) * (1 - fx) + metres(x1, y0) * fx, b = metres(x0, y1) * (1 - fx) + metres(x1, y1) * fx;
    return a * (1 - fy) + b * fy;
}

double HeightField::weight(const Brush& b, double dx, double dy) const {
    double d = std::sqrt(dx * dx + dy * dy) / b.radius;
    if (d >= 1.0) return 0.0;
    // hard edge at falloff 0, cosine bell at falloff 1
    double bell = 0.5 * (1 + std::cos(d * 3.14159265358979323846));
    return b.strength * (b.falloff * bell + (1 - b.falloff));
}

void HeightField::touch(uint32_t x, uint32_t y) {
    if (!touched_) { tx0_ = x; ty0_ = y; tx1_ = x + 1; ty1_ = y + 1; touched_ = true; return; }
    tx0_ = std::min(tx0_, x); ty0_ = std::min(ty0_, y); tx1_ = std::max(tx1_, x + 1); ty1_ = std::max(ty1_, y + 1);
}

template <class F> void HeightField::apply(const Brush& b, F&& f) {
    if (!editing_) throw std::runtime_error("begin_edit() before editing");
    int x0 = std::max(0, int(std::floor(b.cx - b.radius))), x1 = std::min(int(w_) - 1, int(std::ceil(b.cx + b.radius)));
    int y0 = std::max(0, int(std::floor(b.cy - b.radius))), y1 = std::min(int(h_) - 1, int(std::ceil(b.cy + b.radius)));
    for (int y = y0; y <= y1; ++y)
        for (int x = x0; x <= x1; ++x) {
            double wgt = weight(b, x - b.cx, y - b.cy);
            if (wgt <= 0) continue;
            uint16_t before = raw_[size_t(y) * w_ + x];
            uint16_t after = f(x, y, wgt, before);
            if (after != before) { raw_[size_t(y) * w_ + x] = after; touch(uint32_t(x), uint32_t(y)); }
        }
}

void HeightField::raise(const Brush& b, double metres) {
    apply(b, [&](int, int, double w, uint16_t cur) { return to_raw(to_metres(cur) + metres * w); });
}

void HeightField::smooth(const Brush& b) {
    std::vector<uint16_t> src = raw_;     // average from the state before this stroke step
    apply(b, [&](int x, int y, double w, uint16_t cur) {
        double sum = 0;
        int n = 0;
        for (int dy = -1; dy <= 1; ++dy)
            for (int dx = -1; dx <= 1; ++dx) {
                int xx = x + dx, yy = y + dy;
                if (xx < 0 || yy < 0 || xx >= int(w_) || yy >= int(h_)) continue;
                sum += src[size_t(yy) * w_ + xx];
                ++n;
            }
        double avg = sum / n;
        return uint16_t(std::lround(cur + (avg - cur) * std::min(1.0, w)));
    });
}

void HeightField::flatten(const Brush& b, double target_metres) {
    uint16_t target = to_raw(target_metres);
    apply(b, [&](int, int, double w, uint16_t cur) { return uint16_t(std::lround(cur + (double(target) - cur) * std::min(1.0, w))); });
}

void HeightField::set_level_under(const Brush& b) {
    uint32_t cx = uint32_t(std::clamp(b.cx, 0.0, double(w_ - 1))), cy = uint32_t(std::clamp(b.cy, 0.0, double(h_ - 1)));
    flatten(b, metres(cx, cy));
}

void HeightField::ramp(double x0, double y0, double x1, double y1, double width, double m0, double m1) {
    if (!editing_) throw std::runtime_error("begin_edit() before editing");
    double dx = x1 - x0, dy = y1 - y0, len2 = dx * dx + dy * dy;
    if (len2 <= 0) return;
    int xa = std::max(0, int(std::floor(std::min(x0, x1) - width))), xb = std::min(int(w_) - 1, int(std::ceil(std::max(x0, x1) + width)));
    int ya = std::max(0, int(std::floor(std::min(y0, y1) - width))), yb = std::min(int(h_) - 1, int(std::ceil(std::max(y0, y1) + width)));
    for (int y = ya; y <= yb; ++y)
        for (int x = xa; x <= xb; ++x) {
            double t = ((x - x0) * dx + (y - y0) * dy) / len2;
            if (t < 0 || t > 1) continue;
            double px = x0 + t * dx, py = y0 + t * dy, d = std::hypot(x - px, y - py);
            if (d > width) continue;
            double target = m0 + (m1 - m0) * t, k = 1.0 - d / width;      // linear edge falloff
            uint16_t cur = raw_[size_t(y) * w_ + x];
            uint16_t after = uint16_t(std::lround(cur + (double(to_raw(target)) - cur) * k));
            if (after != cur) { raw_[size_t(y) * w_ + x] = after; touch(uint32_t(x), uint32_t(y)); }
        }
}

void HeightField::begin_edit() {
    if (editing_) throw std::runtime_error("edit already open");
    editing_ = true;
    touched_ = false;
    snapshot_ = raw_;
}

void HeightField::end_edit() {
    if (!editing_) return;
    editing_ = false;
    if (!touched_) return;
    Record r{tx0_, ty0_, tx1_, ty1_, {}, {}};
    for (uint32_t y = ty0_; y < ty1_; ++y)
        for (uint32_t x = tx0_; x < tx1_; ++x) {
            r.before.push_back(snapshot_[size_t(y) * w_ + x]);
            r.after.push_back(raw_[size_t(y) * w_ + x]);
        }
    undo_.push_back(std::move(r));
    redo_.clear();
    snapshot_.clear();
}

bool HeightField::undo() {
    if (undo_.empty()) return false;
    Record r = std::move(undo_.back());
    undo_.pop_back();
    size_t i = 0;
    for (uint32_t y = r.y0; y < r.y1; ++y)
        for (uint32_t x = r.x0; x < r.x1; ++x) raw_[size_t(y) * w_ + x] = r.before[i++];
    redo_.push_back(std::move(r));
    return true;
}

bool HeightField::redo() {
    if (redo_.empty()) return false;
    Record r = std::move(redo_.back());
    redo_.pop_back();
    size_t i = 0;
    for (uint32_t y = r.y0; y < r.y1; ++y)
        for (uint32_t x = r.x0; x < r.x1; ++x) raw_[size_t(y) * w_ + x] = r.after[i++];
    undo_.push_back(std::move(r));
    return true;
}

} // namespace te
