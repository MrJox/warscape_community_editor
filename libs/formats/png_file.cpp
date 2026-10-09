#include "formats/png_file.h"
#include "formats/registry.h"

#include <zlib.h>

#include <cstdlib>
#include <cstring>
#include <stdexcept>

namespace wce {
namespace {

uint32_t be32(const uint8_t* p) { return uint32_t(p[0]) << 24 | uint32_t(p[1]) << 16 | uint32_t(p[2]) << 8 | p[3]; }
void put_be32(std::vector<uint8_t>& o, uint32_t v) { for (int s = 24; s >= 0; s -= 8) o.push_back(uint8_t(v >> s)); }

void put_chunk(std::vector<uint8_t>& o, const char* type, const uint8_t* data, size_t n) {
    put_be32(o, uint32_t(n));
    size_t start = o.size();
    o.insert(o.end(), type, type + 4);
    if (n) o.insert(o.end(), data, data + n);
    put_be32(o, uint32_t(crc32(0, o.data() + start, uInt(o.size() - start))));
}

uint8_t paeth(int a, int b, int c) {
    int p = a + b - c, pa = std::abs(p - a), pb = std::abs(p - b), pc = std::abs(p - c);
    return uint8_t(pa <= pb && pa <= pc ? a : (pb <= pc ? b : c));
}

// filter `row` with type ft against `prev` (zeros for the first row)
void filter_row(int ft, const uint8_t* row, const uint8_t* prev, size_t n, size_t bpp, uint8_t* out) {
    for (size_t i = 0; i < n; ++i) {
        int a = i >= bpp ? row[i - bpp] : 0, b = prev[i], c = i >= bpp ? prev[i - bpp] : 0, x = row[i], p = 0;
        switch (ft) {
            case 0: p = 0; break;
            case 1: p = a; break;
            case 2: p = b; break;
            case 3: p = (a + b) >> 1; break;
            default: p = paeth(a, b, c); break;
        }
        out[i] = uint8_t(x - p);
    }
}

} // namespace

size_t PngFile::bytes_per_pixel() const {
    size_t ch = color_type == 0 ? 1 : color_type == 2 ? 3 : color_type == 4 ? 2 : color_type == 6 ? 4 : 0;
    return ch * (bit_depth == 16 ? 2 : 1);
}

PngFile PngFile::parse(const std::vector<uint8_t>& b) {
    static const uint8_t sig[8] = {0x89, 'P', 'N', 'G', '\r', '\n', 0x1a, '\n'};
    if (b.size() < 8 || std::memcmp(b.data(), sig, 8) != 0) throw std::runtime_error("not a PNG");
    PngFile f;
    f.original = b;
    std::vector<uint8_t> idat;
    bool have_ihdr = false, first_idat = true;
    size_t i = 8;
    while (i + 12 <= b.size()) {
        uint32_t n = be32(&b[i]);
        if (i + 12 + n > b.size()) throw std::runtime_error("truncated chunk");
        const char* t = reinterpret_cast<const char*>(&b[i + 4]);
        const uint8_t* d = &b[i + 8];
        if (!std::memcmp(t, "IHDR", 4)) {
            f.width = be32(d); f.height = be32(d + 4); f.bit_depth = d[8]; f.color_type = d[9];
            if (d[10] != 0 || d[11] != 0 || d[12] != 0) throw std::runtime_error("interlaced or unusual PNG");
            have_ihdr = true;
        } else if (!std::memcmp(t, "IDAT", 4)) {
            if (first_idat) { f.idat_chunk_size = n; first_idat = false; }
            idat.insert(idat.end(), d, d + n);
        } else if (std::memcmp(t, "IEND", 4) != 0) {
            f.has_ancillary = true;
        }
        i += 12 + n;
    }
    if (!have_ihdr) throw std::runtime_error("no IHDR");
    size_t bpp = f.bytes_per_pixel();
    if (!bpp || (f.bit_depth != 8 && f.bit_depth != 16)) throw std::runtime_error("unsupported PNG format");
    size_t stride = size_t(f.width) * bpp;
    std::vector<uint8_t> raw((stride + 1) * f.height);
    uLongf out_len = uLongf(raw.size());
    uLong src_len = uLong(idat.size());
    z_stream zs{};
    if (inflateInit(&zs) != Z_OK) throw std::runtime_error("inflateInit");
    zs.next_in = idat.data(); zs.avail_in = uInt(src_len);
    zs.next_out = raw.data(); zs.avail_out = uInt(out_len);
    int rc = inflate(&zs, Z_FINISH);
    inflateEnd(&zs);
    if (rc != Z_STREAM_END || zs.avail_out != 0) throw std::runtime_error("bad or short IDAT stream");
    f.pixels.assign(stride * f.height, 0);
    std::vector<uint8_t> zero(stride, 0);
    for (uint32_t y = 0; y < f.height; ++y) {
        int ft = raw[y * (stride + 1)];
        const uint8_t* src = &raw[y * (stride + 1) + 1];
        uint8_t* cur = &f.pixels[y * stride];
        const uint8_t* prev = y ? &f.pixels[(y - 1) * stride] : zero.data();
        for (size_t x = 0; x < stride; ++x) {
            int a = x >= bpp ? cur[x - bpp] : 0, bb = prev[x], c = x >= bpp ? prev[x - bpp] : 0, p = 0;
            switch (ft) {
                case 0: p = 0; break;
                case 1: p = a; break;
                case 2: p = bb; break;
                case 3: p = (a + bb) >> 1; break;
                case 4: p = paeth(a, bb, c); break;
                default: throw std::runtime_error("bad filter type");
            }
            cur[x] = uint8_t(src[x] + p);
        }
    }
    return f;
}

std::vector<uint8_t> PngFile::encode() const {
    size_t bpp = bytes_per_pixel(), stride = size_t(width) * bpp;
    std::vector<uint8_t> filtered((stride + 1) * height), zero(stride, 0), tmp(stride);
    for (uint32_t y = 0; y < height; ++y) {
        const uint8_t* row = &pixels[y * stride];
        const uint8_t* prev = y ? &pixels[(y - 1) * stride] : zero.data();
        int best = 0;
        long best_sum = -1;
        for (int ft = 0; ft < 5; ++ft) {
            filter_row(ft, row, prev, stride, bpp, tmp.data());
            long sum = 0;
            for (size_t x = 0; x < stride; ++x) sum += std::abs(int(int8_t(tmp[x])));
            if (best_sum < 0 || sum < best_sum) { best = ft; best_sum = sum; }
        }
        filtered[y * (stride + 1)] = uint8_t(best);
        filter_row(best, row, prev, stride, bpp, &filtered[y * (stride + 1) + 1]);
    }
    uLongf bound = compressBound(uLong(filtered.size()));
    std::vector<uint8_t> z(bound);
    z_stream zs{};
    if (deflateInit2(&zs, 6, Z_DEFLATED, 15, 8, Z_FILTERED) != Z_OK) throw std::runtime_error("deflateInit2");
    zs.next_in = filtered.data(); zs.avail_in = uInt(filtered.size());
    zs.next_out = z.data(); zs.avail_out = uInt(z.size());
    int rc = deflate(&zs, Z_FINISH);
    size_t zn = zs.total_out;
    deflateEnd(&zs);
    if (rc != Z_STREAM_END) throw std::runtime_error("deflate failed");
    z.resize(zn);

    std::vector<uint8_t> o{0x89, 'P', 'N', 'G', '\r', '\n', 0x1a, '\n'};
    uint8_t ihdr[13];
    std::vector<uint8_t> h;
    put_be32(h, width); put_be32(h, height);
    std::memcpy(ihdr, h.data(), 8);
    ihdr[8] = bit_depth; ihdr[9] = color_type; ihdr[10] = ihdr[11] = ihdr[12] = 0;
    put_chunk(o, "IHDR", ihdr, 13);
    size_t chunk = idat_chunk_size ? idat_chunk_size : 8192;
    for (size_t k = 0; k < z.size(); k += chunk) put_chunk(o, "IDAT", &z[k], std::min(chunk, z.size() - k));
    put_chunk(o, "IEND", nullptr, 0);
    return o;
}

std::vector<uint8_t> PngFile::write() const { return pixels_edited ? encode() : original; }

void register_png_format() {
    Format f;
    f.name = "png";
    f.claims = [](const std::string& path, const std::vector<uint8_t>& head) {
        return path.size() > 4 && (path.compare(path.size() - 4, 4, ".png") == 0 || path.compare(path.size() - 4, 4, ".PNG") == 0) &&
               head.size() >= 8 && head[1] == 'P';
    };
    // The gate checks the kit-encoder reproduction (decode, then encode), not just pass-through.
    f.round_trip = [](const std::vector<uint8_t>& bytes) {
        RoundTripResult r;
        try {
            PngFile p = PngFile::parse(bytes);
            r.output = p.encode();
            r.ok = r.output == bytes;
            if (!r.ok) r.error = std::string(p.has_ancillary ? "ancillary chunks (not kit encoder); " : "encoder differs; ") + describe_diff(bytes, r.output);
        } catch (const std::exception& e) {
            r.error = std::string("parse: ") + e.what();
        }
        return r;
    };
    formats().push_back(std::move(f));
}

} // namespace wce
