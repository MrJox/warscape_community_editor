#include "formats/variation_db.h"

#include <bit>
#include <stdexcept>

namespace we {

namespace {

struct Reader {
    const std::vector<uint8_t>& b;
    size_t p = 0;
    void need(size_t n) const {
        if (p + n > b.size()) throw std::runtime_error("variation db: truncated at byte " + std::to_string(p));
    }
    uint32_t u16() { need(2); uint32_t v = b[p] | (b[p + 1] << 8); p += 2; return v; }
    uint32_t u32() { need(4); uint32_t v = b[p] | (b[p + 1] << 8) | (b[p + 2] << 16) | (uint32_t(b[p + 3]) << 24); p += 4; return v; }
    std::string str() {
        uint32_t n = u16();
        need(n);
        std::string s(reinterpret_cast<const char*>(b.data() + p), n);
        p += n;
        return s;
    }
    void expect_version() {
        if (u16() != 1) throw std::runtime_error("variation db: unsupported serialise version at byte " + std::to_string(p - 2));
    }
};

} // namespace

VariationDb VariationDb::parse(const std::vector<uint8_t>& bytes) {
    static const char magic[] = "FASTBIN0";
    if (bytes.size() < 8 || !std::equal(magic, magic + 8, bytes.begin())) throw std::runtime_error("variation db: not a FASTBIN0 file");
    Reader r{bytes, 8};
    r.expect_version();
    VariationDb db;
    for (uint32_t n = r.u32(); n > 0; --n) {
        Meta m;
        r.expect_version();
        m.name = r.str();
        for (uint32_t c = r.u32(); c > 0; --c) m.values.push_back(r.str());
        db.metas.push_back(std::move(m));
    }
    for (uint32_t n = r.u32(); n > 0; --n) {
        Entry e;
        e.key = r.str();
        r.expect_version();
        for (uint32_t c = r.u32(); c > 0; --c) {
            Variation v;
            r.expect_version();
            v.filename = r.str();
            v.flags = r.u32();
            e.variations.push_back(std::move(v));
        }
        db.entries.push_back(std::move(e));
    }
    if (r.p != bytes.size()) throw std::runtime_error("variation db: " + std::to_string(bytes.size() - r.p) + " unread trailing bytes");
    return db;
}

const VariationDb::Meta* VariationDb::meta(const std::string& name) const {
    for (auto& m : metas)
        if (m.name == name) return &m;
    return nullptr;
}

const VariationDb::Entry* VariationDb::entry(const std::string& key) const {
    for (auto& e : entries)
        if (e.key == key) return &e;
    return nullptr;
}

int VariationDb::first_bit(const std::string& category) const {
    int index = 0;
    for (auto& m : metas) {
        if (m.name == category) return index;
        index += int(m.values.size());
    }
    return -1;
}

uint32_t VariationDb::bit(const std::string& category, const std::string& value) const {
    const Meta* m = meta(category);
    int first = first_bit(category);
    if (!m || first < 0) return 0;
    for (size_t i = 0; i < m->values.size(); ++i)
        if (m->values[i] == value) return 1u << ((first + int(i)) & 31);
    return 0;
}

uint32_t VariationDb::query_mask(const Query& q) const {
    uint32_t mask = 0;
    for (auto& [category, value] : q) mask |= bit(category, value);
    return mask;
}

VariationDb::Result VariationDb::candidates(const std::string& key, Query q) const {
    Result out;
    const Entry* e = entry(key);
    if (!e) return out;
    auto run = [&](const Query& query) {
        const uint32_t mask = query_mask(query);
        for (auto& v : e->variations)
            if (v.flags == 0 || (v.flags & mask) == mask) out.candidates.push_back({&v, std::popcount(v.flags)});
    };
    run(q);
    if (out.candidates.empty()) {
        q["map"] = "default_map";
        run(q);
        out.used_default_map = true;
    }
    return out;
}

std::vector<std::pair<std::string, std::vector<std::string>>> VariationDb::decode(uint32_t flags) const {
    std::vector<std::pair<std::string, std::vector<std::string>>> out;
    int index = 0;
    for (auto& m : metas) {
        std::vector<std::string> allowed;
        for (size_t i = 0; i < m.values.size(); ++i)
            if (flags == 0 || (flags >> ((index + int(i)) & 31) & 1)) allowed.push_back(m.values[i]);
        out.emplace_back(m.name, std::move(allowed));
        index += int(m.values.size());
    }
    return out;
}

} // namespace we
