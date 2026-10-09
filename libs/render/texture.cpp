#include "render/texture.h"

#include <algorithm>
#include <cstring>
#include <stdexcept>

namespace wce {

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

namespace {

struct DdsLayout {
    uint32_t width = 0, height = 0, depth = 1, mips = 1, array = 1, faces = 1;
    DXGI_FORMAT fmt = DXGI_FORMAT_UNKNOWN;
    size_t data = 128;
    bool cube = false, volume = false;
};

DdsLayout parse_dds_layout(const uint8_t* b, size_t n) {
    auto u32 = [&](size_t o) {
        if (o + 4 > n) throw std::runtime_error("dds: truncated header");
        uint32_t v;
        std::memcpy(&v, b + o, 4);
        return v;
    };
    if (n < 128 || u32(0) != fourcc('D', 'D', 'S', ' ')) throw std::runtime_error("dds: bad magic");
    DdsLayout l;
    l.height = u32(12);
    l.width = u32(16);
    l.depth = u32(24);
    l.mips = std::max(1u, u32(28));
    const uint32_t pf_flags = u32(80), pf_fourcc = u32(84), bits = u32(88), caps2 = u32(112);
    l.cube = (caps2 & 0x200) != 0;
    l.volume = (caps2 & 0x200000) != 0 && l.depth > 1;
    if (pf_flags & 0x4) {
        switch (pf_fourcc) {
        case fourcc('D', 'X', 'T', '1'): l.fmt = DXGI_FORMAT_BC1_UNORM; break;
        case fourcc('D', 'X', 'T', '2'): case fourcc('D', 'X', 'T', '3'): l.fmt = DXGI_FORMAT_BC2_UNORM; break;
        case fourcc('D', 'X', 'T', '4'): case fourcc('D', 'X', 'T', '5'): l.fmt = DXGI_FORMAT_BC3_UNORM; break;
        case fourcc('A', 'T', 'I', '1'): case fourcc('B', 'C', '4', 'U'): l.fmt = DXGI_FORMAT_BC4_UNORM; break;
        case fourcc('A', 'T', 'I', '2'): case fourcc('B', 'C', '5', 'U'): l.fmt = DXGI_FORMAT_BC5_UNORM; break;
        case 113: l.fmt = DXGI_FORMAT_R16G16B16A16_FLOAT; break;
        case 116: l.fmt = DXGI_FORMAT_R32G32B32A32_FLOAT; break;
        case fourcc('D', 'X', '1', '0'):
            l.fmt = DXGI_FORMAT(u32(128));
            if (u32(132) == 3 /* TEXTURE2D */) l.array = std::max(1u, u32(140));
            if (u32(132) == 4 /* TEXTURE3D */) l.volume = l.depth > 1;
            if (u32(136) & 0x4) l.cube = true;
            l.data = 148;
            break;
        default: throw std::runtime_error("dds: unsupported fourcc");
        }
    } else {
        l.fmt = from_masks(bits, u32(92), u32(96), u32(100), u32(104));
        if (l.fmt == DXGI_FORMAT_UNKNOWN) throw std::runtime_error("dds: unsupported uncompressed layout");
    }
    if (!l.volume) l.depth = 1;
    l.faces = l.cube ? 6 * l.array : l.array;
    return l;
}

void mip_size(const DdsLayout& l, uint32_t w, uint32_t h, size_t& pitch, size_t& size) {
    if (is_block(l.fmt)) {
        pitch = size_t(std::max(1u, (w + 3) / 4)) * block_bytes(l.fmt);
        size = pitch * std::max(1u, (h + 3) / 4);
    } else {
        const uint32_t pb = pixel_bytes(l.fmt);
        if (!pb) throw std::runtime_error("dds: unsupported format");
        pitch = size_t(w) * pb;
        size = pitch * h;
    }
}

// Builds the texture from `data` (file bytes from `data_offset` on), leaving out the first `skip` mips of every face.
Texture build_dds(ID3D11Device* dev, const DdsLayout& l, const uint8_t* data, size_t data_size, size_t data_offset, uint32_t skip, bool srgb) {
    std::vector<D3D11_SUBRESOURCE_DATA> subs;
    size_t p = l.data;
    for (uint32_t f = 0; f < l.faces; ++f) {
        uint32_t w = l.width, h = l.height, d = l.depth;
        for (uint32_t m = 0; m < l.mips; ++m) {
            size_t pitch, size;
            mip_size(l, w, h, pitch, size);
            if (m >= skip) {
                if (p < data_offset || p + size * d - data_offset > data_size) throw std::runtime_error("dds: truncated data");
                subs.push_back({data + (p - data_offset), UINT(pitch), UINT(size)});
            }
            p += size * d;
            w = std::max(1u, w / 2);
            h = std::max(1u, h / 2);
            d = std::max(1u, d / 2);
        }
    }
    const uint32_t width = std::max(1u, l.width >> skip), height = std::max(1u, l.height >> skip), mips = l.mips - skip;

    if (l.volume) {
        D3D11_TEXTURE3D_DESC vd{width, height, l.depth, mips, srgb ? to_typeless(l.fmt) : l.fmt, D3D11_USAGE_IMMUTABLE, D3D11_BIND_SHADER_RESOURCE, 0, 0};
        Microsoft::WRL::ComPtr<ID3D11Texture3D> tex3;
        if (FAILED(dev->CreateTexture3D(&vd, subs.data(), &tex3))) throw std::runtime_error("dds: CreateTexture3D failed");
        Texture t;
        t.width = width;
        t.height = height;
        t.depth = l.depth;
        t.mips = mips;
        t.format = srgb ? to_srgb(l.fmt) : l.fmt;
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
    td.ArraySize = l.faces;
    td.Format = srgb ? to_typeless(l.fmt) : l.fmt;
    td.SampleDesc.Count = 1;
    td.Usage = D3D11_USAGE_IMMUTABLE;
    td.BindFlags = D3D11_BIND_SHADER_RESOURCE;
    td.MiscFlags = l.cube ? D3D11_RESOURCE_MISC_TEXTURECUBE : 0;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> tex;
    if (FAILED(dev->CreateTexture2D(&td, subs.data(), &tex))) throw std::runtime_error("dds: CreateTexture2D failed");

    Texture t;
    t.width = width;
    t.height = height;
    t.mips = mips;
    t.cube = l.cube;
    t.format = srgb ? to_srgb(l.fmt) : l.fmt;
    D3D11_SHADER_RESOURCE_VIEW_DESC sd{};
    sd.Format = t.format;
    if (l.cube) {
        sd.ViewDimension = D3D11_SRV_DIMENSION_TEXTURECUBE;
        sd.TextureCube.MipLevels = mips;
    } else if (l.array > 1) {
        sd.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2DARRAY;
        sd.Texture2DArray.MipLevels = mips;
        sd.Texture2DArray.ArraySize = l.array;
    } else {
        sd.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D;
        sd.Texture2D.MipLevels = mips;
    }
    if (FAILED(dev->CreateShaderResourceView(tex.Get(), &sd, &t.srv))) throw std::runtime_error("dds: CreateShaderResourceView failed");
    return t;
}

} // namespace

Texture load_dds(ID3D11Device* dev, const std::vector<uint8_t>& b, bool srgb) {
    const DdsLayout l = parse_dds_layout(b.data(), b.size());
    return build_dds(dev, l, b.data(), b.size(), 0, 0, srgb);
}

DdsStreamPlan plan_dds_stream(const std::vector<uint8_t>& header, size_t file_size, uint32_t max_dim) {
    DdsStreamPlan plan;
    try {
        const DdsLayout l = parse_dds_layout(header.data(), header.size());
        plan.width = l.width;
        plan.height = l.height;
        if (l.cube || l.volume || l.array > 1 || l.mips < 3 || std::max(l.width, l.height) <= max_dim) return plan;
        uint32_t skip = 0;
        while (skip + 1 < l.mips && std::max(l.width >> skip, l.height >> skip) > max_dim) ++skip;
        if (skip == 0) return plan;
        size_t p = l.data;
        uint32_t w = l.width, h = l.height;
        for (uint32_t m = 0; m < skip; ++m) {
            size_t pitch, size;
            mip_size(l, w, h, pitch, size);
            p += size;
            w = std::max(1u, w / 2);
            h = std::max(1u, h / 2);
        }
        if (p >= file_size) return plan;
        plan.ok = true;
        plan.skip = skip;
        plan.tail_offset = p;
    } catch (const std::exception&) {}
    return plan;
}

Texture load_dds_tail(ID3D11Device* dev, const std::vector<uint8_t>& header, const std::vector<uint8_t>& tail, const DdsStreamPlan& plan, bool srgb) {
    const DdsLayout l = parse_dds_layout(header.data(), header.size());
    return build_dds(dev, l, tail.data(), tail.size(), plan.tail_offset, plan.skip, srgb);
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

} // namespace wce
