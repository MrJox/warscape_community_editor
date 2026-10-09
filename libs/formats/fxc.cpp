#include "fxc.h"

#include <cstring>
#include <regex>
#include <stdexcept>

namespace we {

namespace {

struct Reader {
    const std::vector<uint8_t>& b;
    size_t p = 0;
    void need(size_t n) const {
        if (p + n > b.size()) throw std::runtime_error("fxc: truncated header at " + std::to_string(p));
    }
    uint32_t u32() { need(4); uint32_t v; std::memcpy(&v, &b[p], 4); p += 4; return v; }
    uint16_t u16() { need(2); uint16_t v; std::memcpy(&v, &b[p], 2); p += 2; return v; }
    std::string str(size_t n) {
        need(n);
        std::string s(reinterpret_cast<const char*>(&b[p]), n);
        p += n;
        while (!s.empty() && s.back() == '\0') s.pop_back();
        return s;
    }
};

} // namespace

FxcFile parse_fxc(std::vector<uint8_t> bytes) {
    FxcFile f;
    Reader r{bytes};
    if (r.u32() != 0x075BCD15u) throw std::runtime_error("fxc: bad magic");
    if (uint32_t v = r.u32(); v != 4) throw std::runtime_error("fxc: unsupported version " + std::to_string(v));
    uint32_t nd = r.u32();
    for (uint32_t i = 0; i < nd; ++i) {
        std::string name = r.str(r.u32());
        std::string value = r.str(r.u32());
        f.defines.emplace_back(std::move(name), std::move(value));
    }
    f.source_timestamp = r.u32();
    uint32_t ni = r.u32();
    for (uint32_t i = 0; i < ni; ++i) {
        std::string path = r.str(r.u16());
        f.includes.emplace_back(std::move(path), r.u32());
    }
    r.need(32);
    if (std::memcmp(&bytes[r.p], "DXBC", 4) != 0) throw std::runtime_error("fxc: no DXBC after header");
    uint32_t dxbc_size;
    std::memcpy(&dxbc_size, &bytes[r.p + 24], 4);
    if (r.p + dxbc_size != bytes.size()) throw std::runtime_error("fxc: DXBC size mismatch");
    f.dxbc_offset = r.p;
    f.bytes = std::move(bytes);
    return f;
}

bool split_fxc_name(const std::string& file_name, FxcName& out) {
    static const std::regex re(R"(^(.*?_sm_\d_\d)_(.+?\.hlsl)_([a-z]s)_(.+?)(\.fxc)?$)", std::regex::icase);
    std::smatch m;
    if (!std::regex_match(file_name, m, re)) return false;
    out.prefix = m[1];
    out.source = m[2];
    out.stage = m[3];
    out.entry = m[4];
    return true;
}

} // namespace we
