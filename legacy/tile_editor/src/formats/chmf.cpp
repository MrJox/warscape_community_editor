#include "formats/chmf.h"
#include "formats/registry.h"

#include <cstring>
#include <stdexcept>

namespace te {

static uint32_t rd32(const std::vector<uint8_t>& b, size_t o) {
    if (o + 4 > b.size()) throw std::runtime_error("truncated at " + std::to_string(o));
    uint32_t v;
    std::memcpy(&v, b.data() + o, 4);
    return v;
}

float ChmfFile::min_height() const { float f; std::memcpy(&f, &header[6], 4); return f; }
float ChmfFile::max_height() const { float f; std::memcpy(&f, &header[7], 4); return f; }

ChmfFile ChmfFile::parse(const std::vector<uint8_t>& b) {
    if (b.size() < 64 || std::memcmp(b.data(), "CHMF", 4) != 0) throw std::runtime_error("not CHMF");
    ChmfFile f;
    for (size_t i = 0; i < f.header.size(); ++i) f.header[i] = rd32(b, 4 * i);
    if (f.header[1] != 5) throw std::runtime_error("unsupported version " + std::to_string(f.header[1]));
    size_t p = 4 * f.header.size();
    uint32_t data_size = rd32(b, p);
    p += 4;
    for (uint32_t i = 0; i < f.header[15]; ++i, p += 4) f.offsets.push_back(rd32(b, p));
    if (p + data_size != b.size()) throw std::runtime_error("data size does not match file size");
    f.data.assign(b.begin() + p, b.end());
    return f;
}

std::vector<uint8_t> ChmfFile::write() const {
    std::vector<uint8_t> o;
    auto put = [&](uint32_t v) { o.insert(o.end(), reinterpret_cast<uint8_t*>(&v), reinterpret_cast<uint8_t*>(&v) + 4); };
    for (uint32_t v : header) put(v);
    put(static_cast<uint32_t>(data.size()));
    for (uint32_t v : offsets) put(v);
    o.insert(o.end(), data.begin(), data.end());
    return o;
}

void register_chmf_format() {
    Format f;
    f.name = "chmf";
    f.claims = [](const std::string&, const std::vector<uint8_t>& head) { return head.size() >= 4 && std::memcmp(head.data(), "CHMF", 4) == 0; };
    f.round_trip = [](const std::vector<uint8_t>& bytes) {
        RoundTripResult r;
        try {
            r.output = ChmfFile::parse(bytes).write();
            r.ok = r.output == bytes;
            if (!r.ok) r.error = describe_diff(bytes, r.output);
        } catch (const std::exception& e) {
            r.error = std::string("parse: ") + e.what();
        }
        return r;
    };
    formats().push_back(std::move(f));
}

} // namespace te
