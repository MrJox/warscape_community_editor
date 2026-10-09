#pragma once
// CHMF height-field files: hf_height_map.data, hf_water_map.data (per tile), lf_height_map.data (per map).
// Layout (little endian), verified on all 171 shipped files, see docs/formats/chmf.md:
//   "CHMF"  u32 version(5)  u32 width  u32 height  u32 a  u32 b  f32 min  f32 max  u32 c  u32 d  u32 0  u32 0
//   u32 width  u32 height  u32 0xFFFF0000  u32 block_count  u32 data_size  u32 offsets[block_count]  u8 data[data_size]
// Fields whose meaning is not settled are kept as raw dwords (a, b, c, d); the block payload stays opaque.
#include <array>
#include <cstdint>
#include <vector>

namespace te {

struct ChmfFile {
    std::array<uint32_t, 16> header{};   // all dwords from the magic to block_count, raw
    std::vector<uint32_t> offsets;
    std::vector<uint8_t> data;

    uint32_t version() const { return header[1]; }
    uint32_t width() const { return header[2]; }
    uint32_t height() const { return header[3]; }
    float min_height() const;
    float max_height() const;
    uint32_t block_count() const { return header[15]; }

    static ChmfFile parse(const std::vector<uint8_t>& bytes); // throws std::runtime_error
    std::vector<uint8_t> write() const;
};

} // namespace te
