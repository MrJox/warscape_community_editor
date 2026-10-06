#pragma once
// DDS loading straight into D3D11 (BCn uploaded as-is, no conversion).
// Game textures seen so far: DXT1 / DXT5 (buildings), cubemaps (campaign skybox).
#include <d3d11.h>
#include <wrl/client.h>

#include <cstdint>
#include <string>
#include <vector>

namespace we {

struct Texture {
    Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> srv;
    uint32_t width = 0, height = 0, mips = 0;
    bool cube = false;
    DXGI_FORMAT format = DXGI_FORMAT_UNKNOWN;
};

// srgb: create the view with the _SRGB variant of the format (colour maps).
// Assumption pending capture check: the engine samples diffuse/specular colour maps as sRGB.
Texture load_dds(ID3D11Device* dev, const std::vector<uint8_t>& bytes, bool srgb); // throws
Texture make_solid_texture(ID3D11Device* dev, uint32_t rgba);

} // namespace we
