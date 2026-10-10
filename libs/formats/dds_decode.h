#pragma once
// CPU decoding of the DDS files the terrain bake reads (DXT1 / DXT3 / DXT5 and uncompressed 32-bit BGRA / RGBA): one mip level to RGBA8. The renderer
// uploads DDS files as they are (render/texture.cpp); this is for code that has to read the pixels (the terrain colour bake).
#include <cstdint>
#include <string>
#include <vector>

namespace wce {

struct Rgba8Image {
    uint32_t w = 0, h = 0;
    std::vector<uint8_t> px;                 // w * h * 4, rows top to bottom, bytes r, g, b, a
    const uint8_t* at(uint32_t x, uint32_t y) const { return &px[(size_t(y) * w + x) * 4]; }
};

struct DdsInfo {
    uint32_t width = 0, height = 0, mips = 1;
    std::string format;                      // "DXT1", "DXT3", "DXT5", "BGRA8", "RGBA8" or "" when not supported
};

// Reads the header only (needs the first 128 bytes). format is empty for anything the decoder does not handle.
DdsInfo dds_info(const std::vector<uint8_t>& file);
// Decodes mip level `mip` (0 = the largest; clamped to the last one). False with `why` when the file is not a supported DDS.
bool dds_decode_mip(const std::vector<uint8_t>& file, uint32_t mip, Rgba8Image& out, std::string* why = nullptr);

// The two block formats on their own (a 4 x 4 block to 16 RGBA pixels), for tests.
void decode_bc1_block(const uint8_t* block, uint8_t out_rgba[16][4], bool allow_alpha);
void decode_bc3_alpha_block(const uint8_t* block, uint8_t out_alpha[16]);

} // namespace wce
