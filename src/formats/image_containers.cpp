#include "formats/image_containers.h"
#include "formats/registry.h"

#include <cstring>
#include <stdexcept>

namespace te {

static uint32_t rd32(const std::vector<uint8_t>& b, size_t o) {
    if (o + 4 > b.size()) throw std::runtime_error("truncated");
    uint32_t v;
    std::memcpy(&v, b.data() + o, 4);
    return v;
}
static uint16_t rd16(const std::vector<uint8_t>& b, size_t o) {
    if (o + 2 > b.size()) throw std::runtime_error("truncated");
    uint16_t v;
    std::memcpy(&v, b.data() + o, 2);
    return v;
}
static void put32(std::vector<uint8_t>& o, uint32_t v) { o.insert(o.end(), reinterpret_cast<uint8_t*>(&v), reinterpret_cast<uint8_t*>(&v) + 4); }

// ---- DDS
DdsFile DdsFile::parse(const std::vector<uint8_t>& b) {
    if (b.size() < 128 || std::memcmp(b.data(), "DDS ", 4) != 0) throw std::runtime_error("not a DDS file");
    DdsFile d;
    for (size_t i = 0; i < d.header.size(); ++i) d.header[i] = rd32(b, 4 + 4 * i);
    if (d.header[0] != 124) throw std::runtime_error("unexpected header size");
    size_t off = 128;
    // pixel format fourcc at header[20]; 'DX10' adds a 20-byte extension
    if (d.header[20] == 0x30315844u) {
        if (b.size() < 148) throw std::runtime_error("truncated DX10 header");
        d.has_dx10 = true;
        for (size_t i = 0; i < 5; ++i) d.dx10[i] = rd32(b, 128 + 4 * i);
        off = 148;
    }
    d.payload.assign(b.begin() + off, b.end());
    return d;
}

std::vector<uint8_t> DdsFile::write() const {
    std::vector<uint8_t> o{'D', 'D', 'S', ' '};
    for (uint32_t v : header) put32(o, v);
    if (has_dx10) for (uint32_t v : dx10) put32(o, v);
    o.insert(o.end(), payload.begin(), payload.end());
    return o;
}

// ---- TGA
TgaFile TgaFile::parse(const std::vector<uint8_t>& b) {
    if (b.size() < 18) throw std::runtime_error("truncated TGA");
    TgaFile t;
    std::memcpy(t.header.data(), b.data(), 18);
    if (t.header[2] != 2 && t.header[2] != 3 && t.header[2] != 10 && t.header[2] != 1) throw std::runtime_error("unsupported TGA type");
    t.rest.assign(b.begin() + 18, b.end());
    return t;
}
std::vector<uint8_t> TgaFile::write() const {
    std::vector<uint8_t> o(header.begin(), header.end());
    o.insert(o.end(), rest.begin(), rest.end());
    return o;
}

// ---- TIFF (little endian, first IFD)
TiffFile TiffFile::parse(const std::vector<uint8_t>& b) {
    if (b.size() < 16 || std::memcmp(b.data(), "II*\0", 4) != 0) throw std::runtime_error("not a little-endian TIFF");
    TiffFile t;
    uint32_t ifd = rd32(b, 4);
    uint16_t n = rd16(b, ifd);
    uint32_t strip_off = 0, strip_len = 0;
    for (uint16_t i = 0; i < n; ++i) {
        size_t o = ifd + 2 + 12u * i;
        TiffEntry e{rd16(b, o), rd16(b, o + 2), rd32(b, o + 4), rd32(b, o + 8)};
        t.entries.push_back(e);
        // SHORT values live in the low half of the value field
        uint32_t v = e.type == 3 && e.count == 1 ? (e.value & 0xFFFFu) : e.value;
        switch (e.tag) {
            case 256: t.width = v; break;
            case 257: t.height = v; break;
            case 277: t.samples_per_pixel = v; break;
            case 258: t.bits_per_sample = e.count == 1 ? v : rd16(b, e.value); break;
            case 273: strip_off = e.count == 1 ? v : rd32(b, e.value); break;
            case 279: strip_len = e.count == 1 ? v : rd32(b, e.value); break;
            default: break;
        }
    }
    if (strip_off == 0 || strip_off > b.size()) throw std::runtime_error("bad strip offset");
    t.strip_offset = strip_off;
    t.header.assign(b.begin(), b.begin() + strip_off);
    t.pixels.assign(b.begin() + strip_off, b.end());
    (void)strip_len;
    t.ted_layout = n == 15 && strip_off == 242;
    return t;
}
std::vector<uint8_t> TiffFile::write() const {
    std::vector<uint8_t> o = header;
    o.insert(o.end(), pixels.begin(), pixels.end());
    return o;
}

// ---- registry
static bool ends_with_ci(const std::string& s, const char* t) {
    size_t n = std::strlen(t);
    if (s.size() < n) return false;
    for (size_t i = 0; i < n; ++i)
        if (std::tolower(static_cast<unsigned char>(s[s.size() - n + i])) != t[i]) return false;
    return true;
}

template <class Model>
static Format make_format(const char* name, const char* ext, const char* magic, size_t magic_len) {
    Format f;
    f.name = name;
    f.claims = [ext, magic, magic_len](const std::string& path, const std::vector<uint8_t>& head) {
        if (!ends_with_ci(path, ext)) return false;
        return magic_len == 0 || (head.size() >= magic_len && std::memcmp(head.data(), magic, magic_len) == 0);
    };
    f.round_trip = [](const std::vector<uint8_t>& bytes) {
        RoundTripResult r;
        try {
            r.output = Model::parse(bytes).write();
            r.ok = r.output == bytes;
            if (!r.ok) r.error = describe_diff(bytes, r.output);
        } catch (const std::exception& e) {
            r.error = std::string("parse: ") + e.what();
        }
        return r;
    };
    return f;
}

void register_image_container_formats() {
    formats().push_back(make_format<DdsFile>("dds", ".dds", "DDS ", 4));
    formats().push_back(make_format<TgaFile>("tga", ".tga", "", 0));
    formats().push_back(make_format<TiffFile>("tiff", ".tif", "II*\0", 4));
}

} // namespace te
