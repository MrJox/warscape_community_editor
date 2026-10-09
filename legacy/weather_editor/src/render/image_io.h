#pragma once
#include <d3d11.h>

#include <string>

namespace we {

// Saves an RGBA8 texture (e.g. the swap chain back buffer) to a PNG file through WIC. Returns false on failure.
bool save_texture_png(ID3D11Device* dev, ID3D11DeviceContext* ctx, ID3D11Texture2D* tex, const std::wstring& path);

} // namespace we
