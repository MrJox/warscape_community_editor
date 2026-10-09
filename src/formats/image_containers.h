#pragma once
// Pixel-container readers/writers for the kit's images: DDS, TGA, TIFF. PNG is separate (needs zlib).
// Each keeps the header as parsed fields plus the raw payload, so write() reproduces the file byte for byte and
// edits of the payload keep the header consistent. Findings: docs/formats/image_encoders.md.
#include <array>
#include <cstdint>
#include <string>
#include <vector>

namespace te {

struct DdsFile {
    std::array<uint32_t, 31> header{};      // the 124 bytes after the magic, as dwords (size, flags, height, width, pitch, depth, mips, 11 reserved, pixel format x8, caps x4, reserved)
    bool has_dx10 = false;
    std::array<uint32_t, 5> dx10{};
    std::vector<uint8_t> payload;           // everything after the header(s)

    uint32_t flags() const { return header[1]; }
    uint32_t height() const { return header[2]; }
    uint32_t width() const { return header[3]; }
    uint32_t mip_count() const { return header[6]; }
    uint32_t pixel_format_flags() const { return header[19]; }
    uint32_t fourcc() const { return header[20]; }
    uint32_t bit_count() const { return header[21]; }

    static DdsFile parse(const std::vector<uint8_t>& bytes); // throws std::runtime_error
    std::vector<uint8_t> write() const;

    // The header TEd/BOB write for single-channel float textures (final_*.dds, protection_map.dds):
    // flags 0, no mips, no caps, pixel format fourcc 114 (D3DFMT_R32F), 32 bits. 5,099 shipped files equal this + payload.
    static DdsFile make_r32f(uint32_t width, uint32_t height, std::vector<uint8_t> float_payload);
    bool is_r32f_template() const;   // header equals make_r32f(width, height) and payload is width * height * 4 bytes
};

struct TgaFile {
    std::array<uint8_t, 18> header{};
    std::vector<uint8_t> rest;              // id field, colour map, pixels, footer: verbatim
    uint8_t image_type() const { return header[2]; }
    uint16_t width() const { return uint16_t(header[12] | header[13] << 8); }
    uint16_t height() const { return uint16_t(header[14] | header[15] << 8); }
    uint8_t bits_per_pixel() const { return header[16]; }

    static TgaFile parse(const std::vector<uint8_t>& bytes);
    std::vector<uint8_t> write() const;
};

struct TiffEntry { uint16_t tag, type; uint32_t count, value; };
struct TiffFile {
    std::vector<uint8_t> header;            // bytes before the first strip (IFD, out-of-line values), verbatim
    std::vector<TiffEntry> entries;         // decoded first IFD (read-only view of `header`)
    std::vector<uint8_t> pixels;            // the strip(s)
    uint32_t width = 0, height = 0, samples_per_pixel = 0, bits_per_sample = 0, strip_offset = 0;
    bool ted_layout = false;                // 15 entries, strip at 242: regenerable from width/height

    static TiffFile parse(const std::vector<uint8_t>& bytes);
    std::vector<uint8_t> write() const;
    // The 242-byte header TEd/BOB write for a blend map of 16 x 16-bit samples per pixel (width x height pixels).
    static std::vector<uint8_t> ted_header(uint32_t width, uint32_t height);
    // A TEd-layout file whose header equals ted_header(width, height) can be rebuilt from the pixels alone.
    bool header_is_regenerable() const;
};

} // namespace te
