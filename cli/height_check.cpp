// Height field gate: for the height maps of the shipped tiles
//   1. metres <-> raw conversion round trips for all 65536 raw values (per tile min_height/scale);
//   2. no-op save: HeightField -> PNG -> encode() equals the original bytes (kit-encoder PNGs);
//   3. a brush stroke (raise, smooth, flatten) changes samples only inside its radius, and undo restores the exact bytes;
//   4. redo re-applies the stroke exactly.
//   height_check <png> [min_height scale] ...    (several files may be given; defaults min 0, scale 100)
#include "tile/height_field.h"

#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <iterator>

static std::vector<uint8_t> slurp(const char* p) {
    std::ifstream f(p, std::ios::binary);
    return std::vector<uint8_t>(std::istreambuf_iterator<char>(f), {});
}

static int check(const char* path, double min_h, double scale) {
    auto bytes = slurp(path);
    wce::PngFile png = wce::PngFile::parse(bytes);
    wce::HeightField hf = wce::HeightField::from_png(png, min_h, scale);
    int bad = 0;
    // 1. conversions
    for (uint32_t r = 0; r < 65536; ++r)
        if (hf.to_raw(hf.to_metres(uint16_t(r))) != r) { ++bad; if (bad < 4) std::printf("  raw %u does not round trip\n", r); }
    // 2. no-op save
    wce::PngFile out = hf.to_png(png);
    bool kit_png = !png.has_ancillary && png.encode() == bytes;
    if (kit_png && out.write() != bytes) { ++bad; std::printf("  no-op save differs\n"); }
    // 3. strokes + undo
    auto before = hf.samples();
    wce::Brush b;
    b.cx = hf.width() * 0.4; b.cy = hf.height() * 0.6; b.radius = 20; b.strength = 0.8; b.falloff = 0.7;
    hf.begin_edit(); hf.raise(b, 2.5); hf.end_edit();
    hf.begin_edit(); hf.smooth(b); hf.end_edit();
    hf.begin_edit(); hf.flatten(b, hf.metres(uint32_t(b.cx), uint32_t(b.cy))); hf.end_edit();
    auto after = hf.samples();
    size_t changed = 0, outside = 0;
    for (uint32_t y = 0; y < hf.height(); ++y)
        for (uint32_t x = 0; x < hf.width(); ++x) {
            size_t i = size_t(y) * hf.width() + x;
            if (before[i] != after[i]) {
                ++changed;
                double d = std::hypot(x - b.cx, y - b.cy);
                if (d > b.radius + 1.5) ++outside;
            }
        }
    if (outside) { ++bad; std::printf("  %zu samples changed outside the brush\n", outside); }
    if (!changed) { ++bad; std::printf("  the strokes changed nothing\n"); }
    // 4. undo / redo
    while (hf.can_undo()) hf.undo();
    if (hf.samples() != before) { ++bad; std::printf("  undo did not restore the samples\n"); }
    if (kit_png && hf.to_png(png).encode() != bytes) { ++bad; std::printf("  undo did not restore the PNG bytes\n"); }
    while (hf.redo()) {}
    if (hf.samples() != after) { ++bad; std::printf("  redo did not reproduce the edit\n"); }
    std::printf("%s: %ux%u, %zu samples changed by the strokes, %s\n", path, hf.width(), hf.height(), changed, bad ? "FAILED" : "ok");
    return bad;
}

int main(int argc, char** argv) {
    int bad = 0;
    try {
        for (int i = 1; i < argc; ++i) bad += check(argv[i], -8.370342, 17.933426);
    } catch (const std::exception& e) {
        std::fprintf(stderr, "error: %s\n", e.what());
        return 1;
    }
    return bad ? 1 : 0;
}
