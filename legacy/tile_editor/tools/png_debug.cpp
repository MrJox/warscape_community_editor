// png_debug <file.png>: compare the kit's filter choice with ours row by row, then the deflate output.
#include "formats/png_file.h"

#include <zlib.h>

#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <iterator>
#include <vector>

static std::vector<uint8_t> slurp(const char* p) {
    std::ifstream f(p, std::ios::binary);
    return std::vector<uint8_t>(std::istreambuf_iterator<char>(f), {});
}

int main(int argc, char** argv) {
    auto bytes = slurp(argv[1]);
    te::PngFile p = te::PngFile::parse(bytes);
    auto enc = p.encode();
    std::printf("original %zu bytes, ours %zu, first IDAT chunk %u\n", bytes.size(), enc.size(), p.idat_chunk_size);
    // inflate both IDAT streams and compare filtered data
    auto idat_of = [](const std::vector<uint8_t>& b) {
        std::vector<uint8_t> z;
        size_t i = 8;
        while (i + 12 <= b.size()) {
            uint32_t n = uint32_t(b[i]) << 24 | uint32_t(b[i + 1]) << 16 | uint32_t(b[i + 2]) << 8 | b[i + 3];
            if (!std::memcmp(&b[i + 4], "IDAT", 4)) z.insert(z.end(), b.begin() + i + 8, b.begin() + i + 8 + n);
            i += 12 + n;
        }
        return z;
    };
    auto inflate_all = [](const std::vector<uint8_t>& z, size_t expect) {
        std::vector<uint8_t> out(expect);
        uLongf n = uLongf(expect);
        uncompress(out.data(), &n, z.data(), uLong(z.size()));
        out.resize(n);
        return out;
    };
    size_t stride = size_t(p.width) * p.bytes_per_pixel();
    auto a = inflate_all(idat_of(bytes), (stride + 1) * p.height);
    auto b = inflate_all(idat_of(enc), (stride + 1) * p.height);
    size_t same_rows = 0, first_bad = SIZE_MAX;
    for (uint32_t y = 0; y < p.height; ++y) {
        bool same = std::memcmp(&a[y * (stride + 1)], &b[y * (stride + 1)], stride + 1) == 0;
        same_rows += same;
        if (!same && first_bad == SIZE_MAX) { first_bad = y; std::printf("row %u: original filter %d, ours %d\n", y, a[y * (stride + 1)], b[y * (stride + 1)]); }
    }
    std::printf("filtered rows identical: %zu of %u\n", same_rows, p.height);
    return 0;
}
