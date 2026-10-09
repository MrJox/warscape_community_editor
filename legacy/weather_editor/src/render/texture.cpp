#include "texture.h"

#include <algorithm>
#include <cstring>
#include <stdexcept>

namespace we {

namespace {

constexpr uint32_t fourcc(char a, char b, char c, char d) {
    return uint32_t(uint8_t(a)) | uint32_t(uint8_t(b)) << 8 | uint32_t(uint8_t(c)) << 16 | uint32_t(uint8_t(d)) << 24;
}

DXGI_FORMAT to_srgb(DXGI_FORMAT f) {
    switch (f) {
    case DXGI_FORMAT_BC1_UNORM: return DXGI_FORMAT_BC1_UNORM_SRGB;
    case DXGI_FORMAT_BC2_UNORM: return DXGI_FORMAT_BC2_UNORM_SRGB;
    case DXGI_FORMAT_BC3_UNORM: return DXGI_FORMAT_BC3_UNORM_SRGB;
    case DXGI_FORMAT_BC7_UNORM: return DXGI_FORMAT_BC7_UNORM_SRGB;
    case DXGI_FORMAT_R8G8B8A8_UNORM: return DXGI_FORMAT_R8G8B8A8_UNORM_SRGB;
    case DXGI_FORMAT_B8G8R8A8_UNORM: return DXGI_FORMAT_B8G8R8A8_UNORM_SRGB;
    case DXGI_FORMAT_B8G8R8X8_UNORM: return DXGI_FORMAT_B8G8R8X8_UNORM_SRGB;
    default: return f;
    }
}

// Resource format that allows both the UNORM and the _SRGB view.
DXGI_FORMAT to_typeless(DXGI_FORMAT f) {
    switch (f) {
    case DXGI_FORMAT_BC1_UNORM: return DXGI_FORMAT_BC1_TYPELESS;
    case DXGI_FORMAT_BC2_UNORM: return DXGI_FORMAT_BC2_TYPELESS;
    case DXGI_FORMAT_BC3_UNORM: return DXGI_FORMAT_BC3_TYPELESS;
    case DXGI_FORMAT_BC7_UNORM: return DXGI_FORMAT_BC7_TYPELESS;
    case DXGI_FORMAT_R8G8B8A8_UNORM: return DXGI_FORMAT_R8G8B8A8_TYPELESS;
    case DXGI_FORMAT_B8G8R8A8_UNORM: return DXGI_FORMAT_B8G8R8A8_TYPELESS;
    case DXGI_FORMAT_B8G8R8X8_UNORM: return DXGI_FORMAT_B8G8R8X8_TYPELESS;
    default: return f;
    }
}

bool is_block(DXGI_FORMAT f) {
    return (f >= DXGI_FORMAT_BC1_TYPELESS && f <= DXGI_FORMAT_BC5_SNORM) || (f >= DXGI_FORMAT_BC6H_TYPELESS && f <= DXGI_FORMAT_BC7_UNORM_SRGB);
}

uint32_t block_bytes(DXGI_FORMAT f) {
    switch (f) {
    case DXGI_FORMAT_BC1_UNORM: case DXGI_FORMAT_BC1_UNORM_SRGB: case DXGI_FORMAT_BC4_UNORM: case DXGI_FORMAT_BC4_SNORM: return 8;
    default: return 16;
    }
}

uint32_t pixel_bytes(DXGI_FORMAT f) {
    switch (f) {
    case DXGI_FORMAT_R8G8B8A8_UNORM: case DXGI_FORMAT_B8G8R8A8_UNORM: case DXGI_FORMAT_B8G8R8X8_UNORM: case DXGI_FORMAT_R16G16_UNORM:
    case DXGI_FORMAT_R32_FLOAT: case DXGI_FORMAT_R10G10B10A2_UNORM: return 4;
    case DXGI_FORMAT_R16G16B16A16_FLOAT: case DXGI_FORMAT_R16G16B16A16_UNORM: return 8;
    case DXGI_FORMAT_R32G32B32A32_FLOAT: return 16;
    case DXGI_FORMAT_R8_UNORM: case DXGI_FORMAT_A8_UNORM: return 1;
    case DXGI_FORMAT_R8G8_UNORM: case DXGI_FORMAT_R16_FLOAT: case DXGI_FORMAT_B5G6R5_UNORM: case DXGI_FORMAT_B5G5R5A1_UNORM: return 2;
    default: return 0;
    }
}

DXGI_FORMAT from_masks(uint32_t bits, uint32_t r, uint32_t g, uint32_t b, uint32_t a) {
    if (bits == 32 && r == 0xff && g == 0xff00 && b == 0xff0000) return DXGI_FORMAT_R8G8B8A8_UNORM;
    if (bits == 32 && r == 0xff0000 && g == 0xff00 && b == 0xff) return a ? DXGI_FORMAT_B8G8R8A8_UNORM : DXGI_FORMAT_B8G8R8X8_UNORM;
    if (bits == 32 && r == 0xffff && g == 0xffff0000) return DXGI_FORMAT_R16G16_UNORM;
    if (bits == 16 && r == 0xf800 && g == 0x7e0 && b == 0x1f) return DXGI_FORMAT_B5G6R5_UNORM;
    if (bits == 8 && (r == 0xff || a == 0xff)) return r ? DXGI_FORMAT_R8_UNORM : DXGI_FORMAT_A8_UNORM;
    return DXGI_FORMAT_UNKNOWN;
}

} // namespace

Texture load_dds(ID3D11Device* dev, const std::vector<uint8_t>& b, bool srgb) {
    auto u32 = [&](size_t o) {
        if (o + 4 > b.size()) throw std::runtime_error("dds: truncated header");
        uint32_t v;
        std::memcpy(&v, &b[o], 4);
        return v;
    };
    if (b.size() < 128 || u32(0) != fourcc('D', 'D', 'S', ' ')) throw std::runtime_error("dds: bad magic");
    uint32_t height = u32(12), width = u32(16), depth = u32(24), mips = std::max(1u, u32(28));
    uint32_t pf_flags = u32(80), pf_fourcc = u32(84), bits = u32(88);
    uint32_t caps2 = u32(112);
    size_t data = 128;
    uint32_t array = 1;
    bool cube = (caps2 & 0x200) != 0;
    bool volume = (caps2 & 0x200000) != 0 && depth > 1;
    DXGI_FORMAT fmt = DXGI_FORMAT_UNKNOWN;
    if (pf_flags & 0x4) {
        switch (pf_fourcc) {
        case fourcc('D', 'X', 'T', '1'): fmt = DXGI_FORMAT_BC1_UNORM; break;
        case fourcc('D', 'X', 'T', '2'): case fourcc('D', 'X', 'T', '3'): fmt = DXGI_FORMAT_BC2_UNORM; break;
        case fourcc('D', 'X', 'T', '4'): case fourcc('D', 'X', 'T', '5'): fmt = DXGI_FORMAT_BC3_UNORM; break;
        case fourcc('A', 'T', 'I', '1'): case fourcc('B', 'C', '4', 'U'): fmt = DXGI_FORMAT_BC4_UNORM; break;
        case fourcc('A', 'T', 'I', '2'): case fourcc('B', 'C', '5', 'U'): fmt = DXGI_FORMAT_BC5_UNORM; break;
        case 113: fmt = DXGI_FORMAT_R16G16B16A16_FLOAT; break;
        case 116: fmt = DXGI_FORMAT_R32G32B32A32_FLOAT; break;
        case fourcc('D', 'X', '1', '0'):
            fmt = DXGI_FORMAT(u32(128));
            if (u32(132) == 3 /* TEXTURE2D */) array = std::max(1u, u32(140));
            if (u32(132) == 4 /* TEXTURE3D */) volume = depth > 1;
            if (u32(136) & 0x4) cube = true;
            data = 148;
            break;
        default: throw std::runtime_error("dds: unsupported fourcc");
        }
    } else {
        fmt = from_masks(bits, u32(92), u32(96), u32(100), u32(104));
        if (fmt == DXGI_FORMAT_UNKNOWN) throw std::runtime_error("dds: unsupported uncompressed layout");
    }
    if (!volume) depth = 1;
    uint32_t faces = cube ? 6 * array : array;

    std::vector<D3D11_SUBRESOURCE_DATA> subs;
    size_t p = data;
    for (uint32_t f = 0; f < faces; ++f) {
        uint32_t w = width, h = height, d = depth;
        for (uint32_t m = 0; m < mips; ++m) {
            size_t pitch, size;
            if (is_block(fmt)) {
                pitch = size_t(std::max(1u, (w + 3) / 4)) * block_bytes(fmt);
                size = pitch * std::max(1u, (h + 3) / 4);
            } else {
                uint32_t pb = pixel_bytes(fmt);
                if (!pb) throw std::runtime_error("dds: unsupported format");
                pitch = size_t(w) * pb;
                size = pitch * h;
            }
            if (p + size * d > b.size()) throw std::runtime_error("dds: truncated data");
            subs.push_back({&b[p], UINT(pitch), UINT(size)});
            p += size * d;
            w = std::max(1u, w / 2);
            h = std::max(1u, h / 2);
            d = std::max(1u, d / 2);
        }
    }

    if (volume) {
        D3D11_TEXTURE3D_DESC vd{width, height, depth, mips, srgb ? to_typeless(fmt) : fmt, D3D11_USAGE_IMMUTABLE, D3D11_BIND_SHADER_RESOURCE, 0, 0};
        Microsoft::WRL::ComPtr<ID3D11Texture3D> tex3;
        if (FAILED(dev->CreateTexture3D(&vd, subs.data(), &tex3))) throw std::runtime_error("dds: CreateTexture3D failed");
        Texture t;
        t.width = width;
        t.height = height;
        t.depth = depth;
        t.mips = mips;
        t.format = srgb ? to_srgb(fmt) : fmt;
        D3D11_SHADER_RESOURCE_VIEW_DESC sd{};
        sd.Format = t.format;
        sd.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE3D;
        sd.Texture3D.MipLevels = mips;
        if (FAILED(dev->CreateShaderResourceView(tex3.Get(), &sd, &t.srv))) throw std::runtime_error("dds: CreateShaderResourceView failed");
        return t;
    }

    D3D11_TEXTURE2D_DESC td{};
    td.Width = width;
    td.Height = height;
    td.MipLevels = mips;
    td.ArraySize = faces;
    td.Format = srgb ? to_typeless(fmt) : fmt;
    td.SampleDesc.Count = 1;
    td.Usage = D3D11_USAGE_IMMUTABLE;
    td.BindFlags = D3D11_BIND_SHADER_RESOURCE;
    td.MiscFlags = cube ? D3D11_RESOURCE_MISC_TEXTURECUBE : 0;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> tex;
    if (FAILED(dev->CreateTexture2D(&td, subs.data(), &tex))) throw std::runtime_error("dds: CreateTexture2D failed");

    Texture t;
    t.width = width;
    t.height = height;
    t.mips = mips;
    t.cube = cube;
    t.format = srgb ? to_srgb(fmt) : fmt;
    D3D11_SHADER_RESOURCE_VIEW_DESC sd{};
    sd.Format = t.format;
    if (cube) {
        sd.ViewDimension = D3D11_SRV_DIMENSION_TEXTURECUBE;
        sd.TextureCube.MipLevels = mips;
    } else if (array > 1) {
        sd.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2DARRAY;
        sd.Texture2DArray.MipLevels = mips;
        sd.Texture2DArray.ArraySize = array;
    } else {
        sd.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D;
        sd.Texture2D.MipLevels = mips;
    }
    if (FAILED(dev->CreateShaderResourceView(tex.Get(), &sd, &t.srv))) throw std::runtime_error("dds: CreateShaderResourceView failed");
    return t;
}

Texture make_solid_texture(ID3D11Device* dev, uint32_t rgba) {
    D3D11_TEXTURE2D_DESC td{};
    td.Width = td.Height = 1;
    td.MipLevels = td.ArraySize = 1;
    td.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    td.SampleDesc.Count = 1;
    td.Usage = D3D11_USAGE_IMMUTABLE;
    td.BindFlags = D3D11_BIND_SHADER_RESOURCE;
    D3D11_SUBRESOURCE_DATA sd{&rgba, 4, 4};
    Microsoft::WRL::ComPtr<ID3D11Texture2D> tex;
    dev->CreateTexture2D(&td, &sd, &tex);
    Texture t;
    t.width = t.height = t.mips = 1;
    t.format = td.Format;
    dev->CreateShaderResourceView(tex.Get(), nullptr, &t.srv);
    return t;
}

} // namespace we
