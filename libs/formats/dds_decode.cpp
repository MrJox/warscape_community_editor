#include "formats/dds_decode.h"

#include <algorithm>
#include <cstring>

namespace wce {

namespace {

uint32_t u32(const std::vector<uint8_t>& b, size_t at) {
    return uint32_t(b[at]) | uint32_t(b[at + 1]) << 8 | uint32_t(b[at + 2]) << 16 | uint32_t(b[at + 3]) << 24;
}

void rgb565(uint16_t c, uint8_t out[3]) {
    const int r = (c >> 11) & 31, g = (c >> 5) & 63, b = c & 31;
    out[0] = uint8_t((r << 3) | (r >> 2));
    out[1] = uint8_t((g << 2) | (g >> 4));
    out[2] = uint8_t((b << 3) | (b >> 2));
}

} // namespace

void decode_bc1_block(const uint8_t* blk, uint8_t out[16][4], bool allow_alpha) {
    const uint16_t c0 = uint16_t(blk[0] | blk[1] << 8), c1 = uint16_t(blk[2] | blk[3] << 8);
    uint8_t pal[4][4];
    rgb565(c0, pal[0]);
    rgb565(c1, pal[1]);
    pal[0][3] = pal[1][3] = pal[2][3] = pal[3][3] = 255;
    if (c0 > c1 || !allow_alpha) {
        for (int k = 0; k < 3; ++k) {
            pal[2][k] = uint8_t((2 * pal[0][k] + pal[1][k]) / 3);
            pal[3][k] = uint8_t((pal[0][k] + 2 * pal[1][k]) / 3);
        }
    } else {
        for (int k = 0; k < 3; ++k) {
            pal[2][k] = uint8_t((pal[0][k] + pal[1][k]) / 2);
            pal[3][k] = 0;
        }
        pal[3][3] = 0;
    }
    const uint32_t bits = uint32_t(blk[4]) | uint32_t(blk[5]) << 8 | uint32_t(blk[6]) << 16 | uint32_t(blk[7]) << 24;
    for (int i = 0; i < 16; ++i) std::memcpy(out[i], pal[(bits >> (2 * i)) & 3], 4);
}

void decode_bc3_alpha_block(const uint8_t* blk, uint8_t out[16]) {
    uint8_t a[8];
    a[0] = blk[0];
    a[1] = blk[1];
    if (a[0] > a[1]) {
        for (int k = 1; k < 7; ++k) a[k + 1] = uint8_t(((7 - k) * a[0] + k * a[1]) / 7);
    } else {
        for (int k = 1; k < 5; ++k) a[k + 1] = uint8_t(((5 - k) * a[0] + k * a[1]) / 5);
        a[6] = 0;
        a[7] = 255;
    }
    uint64_t bits = 0;
    for (int i = 0; i < 6; ++i) bits |= uint64_t(blk[2 + i]) << (8 * i);
    for (int i = 0; i < 16; ++i) out[i] = a[(bits >> (3 * i)) & 7];
}

DdsInfo dds_info(const std::vector<uint8_t>& f) {
    DdsInfo i;
    if (f.size() < 128 || std::memcmp(f.data(), "DDS ", 4) != 0) return i;
    i.height = u32(f, 12);
    i.width = u32(f, 16);
    i.mips = std::max<uint32_t>(1, u32(f, 28));
    const uint32_t pf_flags = u32(f, 80);
    if (pf_flags & 4) {   // DDPF_FOURCC
        char cc[5] = {char(f[84]), char(f[85]), char(f[86]), char(f[87]), 0};
        const std::string s = cc;
        if (s == "DXT1" || s == "DXT3" || s == "DXT5") i.format = s;
    } else if ((pf_flags & 0x40) && u32(f, 88) == 32) {   // DDPF_RGB, 32 bits
        const uint32_t rm = u32(f, 92), bm = u32(f, 100);
        if (rm == 0x00ff0000u && bm == 0x000000ffu) i.format = "BGRA8";
        else if (rm == 0x000000ffu && bm == 0x00ff0000u) i.format = "RGBA8";
    }
    return i;
}

bool dds_decode_mip(const std::vector<uint8_t>& f, uint32_t mip, Rgba8Image& out, std::string* why) {
    auto bad = [&](const char* m) {
        if (why) *why = m;
        return false;
    };
    const DdsInfo info = dds_info(f);
    if (info.format.empty()) return bad("not a DDS file, or a format the decoder does not read (DXT1/3/5, 32-bit RGB)");
    mip = std::min(mip, info.mips - 1);
    const bool block = info.format[0] == 'D';
    const uint32_t blk_bytes = info.format == "DXT1" ? 8 : 16;
    size_t off = 128;
    uint32_t w = info.width, h = info.height;
    for (uint32_t m = 0; m < mip; ++m) {
        const size_t sz = block ? size_t(std::max<uint32_t>(1, (w + 3) / 4)) * std::max<uint32_t>(1, (h + 3) / 4) * blk_bytes : size_t(w) * h * 4;
        off += sz;
        w = std::max<uint32_t>(1, w / 2);
        h = std::max<uint32_t>(1, h / 2);
    }
    const size_t need = block ? size_t(std::max<uint32_t>(1, (w + 3) / 4)) * std::max<uint32_t>(1, (h + 3) / 4) * blk_bytes : size_t(w) * h * 4;
    if (off + need > f.size()) return bad("the file is shorter than its header says");
    out.w = w;
    out.h = h;
    out.px.assign(size_t(w) * h * 4, 255);
    if (!block) {
        for (size_t i = 0; i < size_t(w) * h; ++i) {
            const uint8_t* s = &f[off + i * 4];
            uint8_t* d = &out.px[i * 4];
            if (info.format == "BGRA8") { d[0] = s[2]; d[1] = s[1]; d[2] = s[0]; d[3] = s[3]; }
            else std::memcpy(d, s, 4);
        }
        return true;
    }
    const uint32_t bw = std::max<uint32_t>(1, (w + 3) / 4), bh = std::max<uint32_t>(1, (h + 3) / 4);
    for (uint32_t by = 0; by < bh; ++by)
        for (uint32_t bx = 0; bx < bw; ++bx) {
            const uint8_t* blk = &f[off + (size_t(by) * bw + bx) * blk_bytes];
            uint8_t px[16][4];
            if (info.format == "DXT1") decode_bc1_block(blk, px, true);
            else {
                decode_bc1_block(blk + 8, px, false);
                if (info.format == "DXT5") {
                    uint8_t a[16];
                    decode_bc3_alpha_block(blk, a);
                    for (int i = 0; i < 16; ++i) px[i][3] = a[i];
                } else {   // DXT3: 4-bit alpha
                    for (int i = 0; i < 16; ++i) {
                        const uint8_t v = (blk[i / 2] >> (4 * (i & 1))) & 15;
                        px[i][3] = uint8_t(v * 17);
                    }
                }
            }
            for (int i = 0; i < 16; ++i) {
                const uint32_t x = bx * 4 + uint32_t(i & 3), y = by * 4 + uint32_t(i >> 2);
                if (x < w && y < h) std::memcpy(&out.px[(size_t(y) * w + x) * 4], px[i], 4);
            }
        }
    return true;
}

} // namespace wce
