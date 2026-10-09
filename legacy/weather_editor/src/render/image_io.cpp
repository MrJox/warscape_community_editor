#include "image_io.h"

#include <wincodec.h>
#include <wrl/client.h>

#include <vector>

using Microsoft::WRL::ComPtr;

namespace we {

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
    std::vector<uint8_t> px(size_t(d.Width) * d.Height * 4);
    for (UINT y = 0; y < d.Height; ++y) {
        const uint8_t* src = static_cast<const uint8_t*>(ms.pData) + size_t(y) * ms.RowPitch;
        uint8_t* dst = px.data() + size_t(y) * d.Width * 4;
        for (UINT x = 0; x < d.Width; ++x) {
            dst[x * 4 + 0] = src[x * 4 + 2]; // RGBA -> BGRA
            dst[x * 4 + 1] = src[x * 4 + 1];
            dst[x * 4 + 2] = src[x * 4 + 0];
            dst[x * 4 + 3] = 255;
        }
    }
    ctx->Unmap(staging.Get(), 0);

    CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    ComPtr<IWICImagingFactory> f;
    if (FAILED(CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&f)))) return false;
    ComPtr<IWICStream> stream;
    ComPtr<IWICBitmapEncoder> enc;
    ComPtr<IWICBitmapFrameEncode> frame;
    if (FAILED(f->CreateStream(&stream)) || FAILED(stream->InitializeFromFilename(path.c_str(), GENERIC_WRITE)) ||
        FAILED(f->CreateEncoder(GUID_ContainerFormatPng, nullptr, &enc)) || FAILED(enc->Initialize(stream.Get(), WICBitmapEncoderNoCache)) ||
        FAILED(enc->CreateNewFrame(&frame, nullptr)) || FAILED(frame->Initialize(nullptr)) || FAILED(frame->SetSize(d.Width, d.Height)))
        return false;
    WICPixelFormatGUID fmt = GUID_WICPixelFormat32bppBGRA;
    frame->SetPixelFormat(&fmt);
    if (FAILED(frame->WritePixels(d.Height, d.Width * 4, UINT(px.size()), px.data())) || FAILED(frame->Commit()) || FAILED(enc->Commit()))
        return false;
    return true;
}

} // namespace we
