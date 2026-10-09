#include "render/render_target.h"

#include "render/image_io.h"

#include <cstring>

namespace wce {

bool RenderTarget::create(ID3D11Device* dev, int width, int height) {
    tex_.Reset();
    rtv_.Reset();
    srv_.Reset();
    w_ = h_ = 0;
    if (width < 1 || height < 1) return false;
    D3D11_TEXTURE2D_DESC td{UINT(width), UINT(height), 1, 1, DXGI_FORMAT_R8G8B8A8_UNORM, {1, 0}, D3D11_USAGE_DEFAULT,
                            D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE};
    if (FAILED(dev->CreateTexture2D(&td, nullptr, &tex_)) || FAILED(dev->CreateRenderTargetView(tex_.Get(), nullptr, &rtv_)) ||
        FAILED(dev->CreateShaderResourceView(tex_.Get(), nullptr, &srv_))) {
        tex_.Reset();
        rtv_.Reset();
        srv_.Reset();
        return false;
    }
    w_ = width;
    h_ = height;
    return true;
}

bool RenderTarget::read_rgba8(ID3D11Device* dev, ID3D11DeviceContext* ctx, std::vector<uint8_t>& rgba) const {
    if (!tex_) return false;
    D3D11_TEXTURE2D_DESC d{};
    tex_->GetDesc(&d);
    d.Usage = D3D11_USAGE_STAGING;
    d.BindFlags = 0;
    d.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> staging;
    if (FAILED(dev->CreateTexture2D(&d, nullptr, &staging))) return false;
    ctx->CopyResource(staging.Get(), tex_.Get());
    D3D11_MAPPED_SUBRESOURCE ms;
    if (FAILED(ctx->Map(staging.Get(), 0, D3D11_MAP_READ, 0, &ms))) return false;
    rgba.resize(size_t(w_) * size_t(h_) * 4);
    for (int y = 0; y < h_; ++y)
        std::memcpy(rgba.data() + size_t(y) * size_t(w_) * 4, static_cast<const uint8_t*>(ms.pData) + size_t(y) * ms.RowPitch, size_t(w_) * 4);
    ctx->Unmap(staging.Get(), 0);
    return true;
}

bool RenderTarget::save_png(ID3D11Device* dev, ID3D11DeviceContext* ctx, const std::wstring& path) const {
    return tex_ && save_texture_png(dev, ctx, tex_.Get(), path);
}

} // namespace wce
