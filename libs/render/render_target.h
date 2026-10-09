#pragma once
// The surface the renderer draws a view into: RGBA8, bindable as a render target and as a shader resource (ImGui::Image shows it),
// any size. Offscreen capture is rendering into a RenderTarget with no window involved and reading it back.
#include <d3d11.h>
#include <wrl/client.h>

#include <cstdint>
#include <string>
#include <vector>

namespace wce {

class RenderTarget {
public:
    RenderTarget() = default;
    RenderTarget(ID3D11Device* dev, int width, int height) { create(dev, width, height); }

    bool create(ID3D11Device* dev, int width, int height);   // false when the device refuses
    // Reallocates when the size differs; true if the surface is usable afterwards.
    bool resize(ID3D11Device* dev, int width, int height) { return (width == w_ && height == h_ && tex_) ? true : create(dev, width, height); }

    bool valid() const { return tex_ != nullptr; }
    int width() const { return w_; }
    int height() const { return h_; }
    ID3D11Texture2D* texture() const { return tex_.Get(); }
    ID3D11RenderTargetView* rtv() const { return rtv_.Get(); }
    ID3D11ShaderResourceView* srv() const { return srv_.Get(); }

    // Copies the pixels to the CPU as tightly packed RGBA8 rows, top row first. Waits for the GPU.
    bool read_rgba8(ID3D11Device* dev, ID3D11DeviceContext* ctx, std::vector<uint8_t>& rgba) const;
    // PNG through formats/png_file (RGB, the kit's encoder).
    bool save_png(ID3D11Device* dev, ID3D11DeviceContext* ctx, const std::wstring& path) const;

private:
    int w_ = 0, h_ = 0;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> tex_;
    Microsoft::WRL::ComPtr<ID3D11RenderTargetView> rtv_;
    Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> srv_;
};

} // namespace wce
