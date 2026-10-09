#include "formats/markers.h"
#include "formats/registry.h"

#include <cstring>
#include <stdexcept>

namespace te {
namespace {

struct Reader {
    const std::vector<uint8_t>& b;
    size_t p = 0;
    const uint8_t* take(size_t n) {
        if (p + n > b.size()) throw std::runtime_error("truncated at " + std::to_string(p));
        const uint8_t* r = b.data() + p;
        p += n;
        return r;
    }
    template <class T> T get() { T v; std::memcpy(&v, take(sizeof(T)), sizeof(T)); return v; }
    std::string str16() {
        uint16_t n = get<uint16_t>();
        const uint8_t* s = take(size_t(n) * 2);
        std::string out;
        for (uint16_t i = 0; i < n; ++i) {
            if (s[2 * i + 1] != 0 || s[2 * i] >= 0x80) throw std::runtime_error("non-ASCII character in STR16");
            out.push_back(static_cast<char>(s[2 * i]));
        }
        return out;
    }
};

void put_str16(std::vector<uint8_t>& o, const std::string& s) {
    uint16_t n = static_cast<uint16_t>(s.size());
    o.insert(o.end(), reinterpret_cast<uint8_t*>(&n), reinterpret_cast<uint8_t*>(&n) + 2);
    for (char c : s) { o.push_back(static_cast<uint8_t>(c)); o.push_back(0); }
}

void put_u32(std::vector<uint8_t>& o, uint32_t v) {
    o.insert(o.end(), reinterpret_cast<uint8_t*>(&v), reinterpret_cast<uint8_t*>(&v) + 4);
}

} // namespace

// The prefix between the repository name and the class name is: 12 zero, repo tag, u32 1, u32 1, 10 zero?, class
// tag. It is identical modulo the class tag, but is kept as raw bytes (the class tag is read from its last 4).
MarkersFile MarkersFile::parse(const std::vector<uint8_t>& bytes) {
    Reader r{bytes};
    MarkersFile m;
    m.repository_name = r.str16();
    if (m.repository_name != "BASE_MARKER_REPOSITORY") throw std::runtime_error("not a markers file");
    // scan for the class name: the first plausible STR16 after the repository name
    size_t start = r.p;
    size_t found = 0;
    std::string cls;
    for (size_t off = start; off + 2 <= bytes.size() && off < start + 80; ++off) {
        Reader t{bytes, off};
        try {
            std::string s = t.str16();
            if (s.size() >= 3 && s.find("MARKER") != std::string::npos) { found = off; cls = s; break; }
        } catch (const std::exception&) {}
    }
    if (cls.empty()) throw std::runtime_error("class name not found");
    m.prefix.assign(bytes.begin() + start, bytes.begin() + found);
    if (m.prefix.size() < 4) throw std::runtime_error("short prefix");
    std::memcpy(&m.class_tag, m.prefix.data() + m.prefix.size() - 4, 4);
    r.p = found;
    m.class_name = r.str16();
    r.take(12);
    uint32_t tag2 = r.get<uint32_t>();
    if (tag2 != m.class_tag) throw std::runtime_error("class tag mismatch");
    m.class_version = r.get<uint32_t>();
    m.count = r.get<uint32_t>();
    m.payload.assign(bytes.begin() + r.p, bytes.end());
    return m;
}

std::vector<uint8_t> MarkersFile::write() const {
    std::vector<uint8_t> o;
    put_str16(o, repository_name);
    o.insert(o.end(), prefix.begin(), prefix.end());
    put_str16(o, class_name);
    o.insert(o.end(), 12, 0);
    put_u32(o, class_tag);
    put_u32(o, class_version);
    put_u32(o, count);
    o.insert(o.end(), payload.begin(), payload.end());
    return o;
}

static bool ends_with(const std::string& s, const char* t) {
    size_t n = std::strlen(t);
    return s.size() >= n && s.compare(s.size() - n, n, t) == 0;
}

void register_markers_format() {
    Format f;
    f.name = "markers";
    f.claims = [](const std::string& path, const std::vector<uint8_t>&) { return ends_with(path, ".markers"); };
    f.round_trip = [](const std::vector<uint8_t>& bytes) {
        RoundTripResult r;
        try {
            r.output = MarkersFile::parse(bytes).write();
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
