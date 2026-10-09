#pragma once
#include <d3d11.h>

#include <string>

namespace wce {

// Saves an RGBA8 texture (e.g. the swap chain back buffer) as an RGB PNG written by formats/png_file (the kit's encoder, alpha dropped).
// Returns false on failure.
bool save_texture_png(ID3D11Device* dev, ID3D11DeviceContext* ctx, ID3D11Texture2D* tex, const std::wstring& path);

} // namespace wce
