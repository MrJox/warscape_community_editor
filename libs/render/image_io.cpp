#include "render/image_io.h"

#include "formats/png_file.h"

#include <wrl/client.h>

#include <cstdio>
#include <vector>

using Microsoft::WRL::ComPtr;

namespace wce {

bool save_texture_png(ID3D11Device* dev, ID3D11DeviceContext* ctx, ID3D11Texture2D* tex, const std::wstring& path) {
    D3D11_TEXTURE2D_DESC d{};
    tex->GetDesc(&d);
    if (d.Format != DXGI_FORMAT_R8G8B8A8_UNORM && d.Format != DXGI_FORMAT_R8G8B8A8_UNORM_SRGB) return false;
    d.Usage = D3D11_USAGE_STAGING;
    d.BindFlags = 0;
    d.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    d.MiscFlags = 0;
    d.MipLevels = 1;
    ComPtr<ID3D11Texture2D> staging;
    if (FAILED(dev->CreateTexture2D(&d, nullptr, &staging))) return false;
    ctx->CopySubresourceRegion(staging.Get(), 0, 0, 0, 0, tex, 0, nullptr);
    D3D11_MAPPED_SUBRESOURCE ms;
    if (FAILED(ctx->Map(staging.Get(), 0, D3D11_MAP_READ, 0, &ms))) return false;
    PngFile png;
    png.width = d.Width;
    png.height = d.Height;
    png.bit_depth = 8;
    png.color_type = 2; // RGB
    png.pixels.resize(size_t(d.Width) * d.Height * 3);
    for (UINT y = 0; y < d.Height; ++y) {
        const uint8_t* src = static_cast<const uint8_t*>(ms.pData) + size_t(y) * ms.RowPitch;
        uint8_t* dst = png.pixels.data() + size_t(y) * d.Width * 3;
        for (UINT x = 0; x < d.Width; ++x) {
            dst[x * 3 + 0] = src[x * 4 + 0];
            dst[x * 3 + 1] = src[x * 4 + 1];
            dst[x * 3 + 2] = src[x * 4 + 2];
        }
    }
    ctx->Unmap(staging.Get(), 0);

    std::vector<uint8_t> bytes;
    try { bytes = png.encode(); } catch (...) { return false; }
    FILE* f = _wfopen(path.c_str(), L"wb");
    if (!f) return false;
    const bool ok = std::fwrite(bytes.data(), 1, bytes.size(), f) == bytes.size();
    return std::fclose(f) == 0 && ok;
}

} // namespace wce
