#pragma once
// PNG as written by the kit's tools (TEd / BOB, libpng defaults): chunks IHDR, IDAT..., IEND, no ancillary data.
//
// parse() decodes the pixels (non-interlaced, 8/16 bit grey / RGB / grey+alpha / RGBA). encode() reproduces the kit's
// encoder: per row the libpng "minimum sum of absolute differences" filter choice (None, Sub, Up, Average, Paeth; a
// later filter only wins when strictly smaller), zlib level 6 (memLevel 8, default strategy), IDAT chunks of
// `idat_chunk_size` bytes. Files that carry ancillary chunks (Photoshop output) or were produced by another encoder
// are not reproduced by encode(); write() returns their original bytes unless the pixels were edited
// (see docs/formats/image_encoders.md).
#include <cstdint>
#include <vector>

namespace te {

struct PngFile {
    uint32_t width = 0, height = 0;
    uint8_t bit_depth = 8, color_type = 0;
    uint32_t idat_chunk_size = 0;          // size of the first IDAT chunk of the source (0 = default 8192)
    bool has_ancillary = false;            // any chunk besides IHDR / IDAT / IEND
    std::vector<uint8_t> pixels;           // unfiltered rows, width * bytes_per_pixel each, big-endian samples as stored
    std::vector<uint8_t> original;         // source bytes

    size_t bytes_per_pixel() const;
    static PngFile parse(const std::vector<uint8_t>& bytes);   // throws std::runtime_error
    std::vector<uint8_t> encode() const;                        // the kit's encoder
    std::vector<uint8_t> write() const;                         // original bytes when the pixels are unchanged, else encode()
    bool pixels_edited = false;
};

} // namespace te
