#pragma once
// DDS loading straight into D3D11 (BCn uploaded as-is, no conversion).
// Game textures seen so far: DXT1 / DXT5 (buildings), cubemaps (campaign skybox).
#include <d3d11.h>
#include <wrl/client.h>

#include <cstdint>
#include <string>
#include <vector>

namespace wce {

struct Texture {
    Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> srv;
    uint32_t width = 0, height = 0, mips = 0;
    bool cube = false;
    uint32_t depth = 1; // > 1: volume texture (Texture3D)
    DXGI_FORMAT format = DXGI_FORMAT_UNKNOWN;
};

// srgb: create the view with the _SRGB variant of the format (colour maps).
// Assumption pending capture check: the engine samples diffuse/specular colour maps as sRGB.
Texture load_dds(ID3D11Device* dev, const std::vector<uint8_t>& bytes, bool srgb); // throws

// Mip streaming: DDS files store the largest mip first, so the small ones sit at the end of the file. plan_dds_stream looks at the first bytes of
// a file (at least 148) and says where the mips that fit in `max_dim` start; load_dds_tail builds a texture from the header and that tail only
// (a 2048 x 2048 DXT5 texture becomes 256 x 256 and 1/64 of the bytes). plan.ok is false for what is not worth it or not supported (cube maps, volumes,
// arrays, small textures): load those whole.
struct DdsStreamPlan {
    bool ok = false;
    uint32_t skip = 0;                // mips not loaded
    size_t tail_offset = 0;           // file offset of the first loaded mip
    uint32_t width = 0, height = 0;   // of the full texture
};
DdsStreamPlan plan_dds_stream(const std::vector<uint8_t>& header, size_t file_size, uint32_t max_dim);
Texture load_dds_tail(ID3D11Device* dev, const std::vector<uint8_t>& header, const std::vector<uint8_t>& tail, const DdsStreamPlan& plan, bool srgb); // throws
Texture make_solid_texture(ID3D11Device* dev, uint32_t rgba);
// An RGBA8 image (w * h * 4 bytes) with a box-filtered mip chain down to 1 x 1; sRGB view when `srgb`. Thread-safe like the device. Empty srv on failure.
Texture make_rgba_texture(ID3D11Device* dev, uint32_t w, uint32_t h, const uint8_t* rgba, bool srgb);

} // namespace wce
