#pragma once
// Tile height map as the editor edits it: height_map_0.png (16-bit grey) <-> metres.
//
// metres = min_height + raw / 65535 * scale, with min_height and scale taken from the tile's VARIATION in the tile
// database (TILES/*.xml). Brushes work in pixels and change `raw`; every edit goes through an undo record holding the
// touched rectangle, so edit + undo restores the exact samples (and the PNG bytes, through the kit-exact encoder).
#include "formats/png_file.h"

#include <cstdint>
#include <vector>

namespace te {

struct Brush {
    double cx = 0, cy = 0;       // centre in pixels
    double radius = 8;           // pixels
    double strength = 1;         // 0..1 multiplier
    double falloff = 0.5;        // 0 = hard edge, 1 = smooth bell over the whole radius
};

class HeightField {
public:
    static HeightField from_png(const PngFile& png, double min_height, double scale); // throws if not 16-bit grey
    // Replaces the pixels of `base` (header, chunk layout stay) and returns a PNG that write() encodes with the kit encoder.
    PngFile to_png(const PngFile& base) const;

    uint32_t width() const { return w_; }
    uint32_t height() const { return h_; }
    uint16_t raw(uint32_t x, uint32_t y) const { return raw_[size_t(y) * w_ + x]; }
    double metres(uint32_t x, uint32_t y) const { return to_metres(raw(x, y)); }
    double to_metres(uint16_t r) const { return min_ + double(r) / 65535.0 * scale_; }
    uint16_t to_raw(double m) const;                       // clamped, rounded to nearest
    double min_height() const { return min_; }
    double scale() const { return scale_; }
    double sample(double x, double y) const;               // bilinear, metres

    // Brush operations (metres are added / targeted; results are clamped to the representable range).
    void raise(const Brush& b, double metres);             // negative lowers
    void smooth(const Brush& b);                           // blend towards the local average
    void flatten(const Brush& b, double target_metres);    // blend towards a level
    void set_level_under(const Brush& b);                  // flatten to the height at the brush centre
    void ramp(double x0, double y0, double x1, double y1, double width, double metres0, double metres1); // strip between two points

    // Undo / redo of whole operations. begin_edit() marks the start; end_edit() stores the touched rectangle.
    void begin_edit();
    void end_edit();
    bool undo();
    bool redo();
    bool can_undo() const { return !undo_.empty(); }
    size_t edit_depth() const { return undo_.size(); }

    const std::vector<uint16_t>& samples() const { return raw_; }

private:
    struct Record { uint32_t x0, y0, x1, y1; std::vector<uint16_t> before, after; };
    void touch(uint32_t x, uint32_t y);
    double weight(const Brush& b, double dx, double dy) const;
    template <class F> void apply(const Brush& b, F&& f);
    uint32_t w_ = 0, h_ = 0;
    double min_ = 0, scale_ = 1;
    std::vector<uint16_t> raw_;
    bool editing_ = false;
    uint32_t tx0_ = 0, ty0_ = 0, tx1_ = 0, ty1_ = 0;      // touched rectangle of the open edit (exclusive end)
    bool touched_ = false;
    std::vector<uint16_t> snapshot_;                       // copy of raw_ at begin_edit (only the touched part is kept)
    std::vector<Record> undo_, redo_;
};

} // namespace te
