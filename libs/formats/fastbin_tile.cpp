#include "formats/fastbin_tile.h"
#include "formats/registry.h"

#include <cstring>

namespace wce {
namespace {

constexpr char kMagic[8] = {'F', 'A', 'S', 'T', 'B', 'I', 'N', '0'};

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
    std::string str() { uint16_t n = get<uint16_t>(); const uint8_t* s = take(n); return std::string(reinterpret_cast<const char*>(s), n); }
    void header(uint16_t expect) {
        if (std::memcmp(take(8), kMagic, 8) != 0) throw std::runtime_error("not FASTBIN0");
        uint16_t v = get<uint16_t>();
        if (v != expect) throw std::runtime_error("unsupported version " + std::to_string(v));
    }
    void finish() { if (p != b.size()) throw std::runtime_error(std::to_string(b.size() - p) + " trailing bytes"); }
};

struct Writer {
    std::vector<uint8_t> out;
    void raw(const void* d, size_t n) { const uint8_t* s = static_cast<const uint8_t*>(d); out.insert(out.end(), s, s + n); }
    template <class T> void put(T v) { raw(&v, sizeof(T)); }
    void str(const std::string& s) { put<uint16_t>(static_cast<uint16_t>(s.size())); raw(s.data(), s.size()); }
    void header(uint16_t v) { raw(kMagic, 8); put<uint16_t>(v); }
};

} // namespace

TreeList TreeList::parse(const std::vector<uint8_t>& bytes) {
    Reader r{bytes};
    TreeList t;
    r.header(3);
    t.version = 3;
    uint32_t n = r.get<uint32_t>();
    for (uint32_t i = 0; i < n; ++i) {
        TreeType tt;
        tt.name = r.str();
        uint32_t m = r.get<uint32_t>();
        for (uint32_t j = 0; j < m; ++j) {
            TreeInstance in;
            in.x = r.get<float>(); in.y = r.get<float>(); in.z = r.get<float>(); in.w = r.get<uint16_t>();
            tt.instances.push_back(in);
        }
        t.types.push_back(std::move(tt));
    }
    r.finish();
    return t;
}

std::vector<uint8_t> TreeList::write() const {
    Writer w;
    w.header(version);
    w.put<uint32_t>(static_cast<uint32_t>(types.size()));
    for (const TreeType& t : types) {
        w.str(t.name);
        w.put<uint32_t>(static_cast<uint32_t>(t.instances.size()));
        for (const TreeInstance& i : t.instances) { w.put(i.x); w.put(i.y); w.put(i.z); w.put(i.w); }
    }
    return w.out;
}

GrassList GrassList::parse(const std::vector<uint8_t>& bytes) {
    Reader r{bytes};
    GrassList g;
    r.header(1);
    uint32_t n = r.get<uint32_t>();
    for (uint32_t i = 0; i < n; ++i) {
        GrassGroup gg;
        gg.name = r.str();
        uint32_t m = r.get<uint32_t>();
        const uint8_t* d = r.take(size_t(m) * 6);
        gg.data.assign(d, d + size_t(m) * 6);
        g.groups.push_back(std::move(gg));
    }
    r.finish();
    return g;
}

std::vector<uint8_t> GrassList::write() const {
    Writer w;
    w.header(version);
    w.put<uint32_t>(static_cast<uint32_t>(groups.size()));
    for (const GrassGroup& g : groups) {
        w.str(g.name);
        w.put<uint32_t>(static_cast<uint32_t>(g.data.size() / 6));
        w.raw(g.data.data(), g.data.size());
    }
    return w.out;
}

CivilianDeployment CivilianDeployment::parse(const std::vector<uint8_t>& bytes) {
    Reader r{bytes};
    CivilianDeployment c;
    r.header(1);
    uint32_t n = r.get<uint32_t>();
    for (uint32_t i = 0; i < n; ++i) {
        CivilianDeploymentItem it;
        it.flag = r.get<uint16_t>();
        it.kind = r.str();
        it.size = r.str();
        uint32_t m = r.get<uint32_t>();
        for (uint32_t j = 0; j < m; ++j) { float x = r.get<float>(); float z = r.get<float>(); it.points.push_back({x, z}); }
        c.items.push_back(std::move(it));
    }
    r.finish();
    return c;
}

std::vector<uint8_t> CivilianDeployment::write() const {
    Writer w;
    w.header(version);
    w.put<uint32_t>(static_cast<uint32_t>(items.size()));
    for (const CivilianDeploymentItem& it : items) {
        w.put(it.flag); w.str(it.kind); w.str(it.size);
        w.put<uint32_t>(static_cast<uint32_t>(it.points.size()));
        for (auto& p : it.points) { w.put(p[0]); w.put(p[1]); }
    }
    return w.out;
}

CivilianShelter CivilianShelter::parse(const std::vector<uint8_t>& bytes) {
    Reader r{bytes};
    CivilianShelter c;
    r.header(1);
    uint32_t n = r.get<uint32_t>();
    for (uint32_t i = 0; i < n; ++i) {
        CivilianShelterItem it;
        it.flag = r.get<uint16_t>();
        for (float& f : it.rect) f = r.get<float>();
        it.capacity = r.get<uint32_t>();
        c.items.push_back(it);
    }
    r.finish();
    return c;
}

std::vector<uint8_t> CivilianShelter::write() const {
    Writer w;
    w.header(version);
    w.put<uint32_t>(static_cast<uint32_t>(items.size()));
    for (const CivilianShelterItem& it : items) {
        w.put(it.flag);
        for (float f : it.rect) w.put(f);
        w.put(it.capacity);
    }
    return w.out;
}

// ---- registry entries
static bool name_is(const std::string& path, const char* tail) {
    size_t n = std::strlen(tail);
    return path.size() >= n && path.compare(path.size() - n, n, tail) == 0;
}

template <class Model>
static Format make(const char* name, bool (*match)(const std::string&)) {
    Format f;
    f.name = name;
    f.claims = [match](const std::string& path, const std::vector<uint8_t>& head) {
        return match(path) && head.size() >= 8 && std::memcmp(head.data(), kMagic, 8) == 0;
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

void register_fastbin_tile_formats() {
    formats().push_back(make<TreeList>("tree_list_bin", [](const std::string& p) { return name_is(p, ".tree_list.bin"); }));
    formats().push_back(make<GrassList>("grass_list_bin", [](const std::string& p) { return name_is(p, "grass_list.bin"); }));
    formats().push_back(make<CivilianDeployment>("civilian_deployment", [](const std::string& p) { return name_is(p, "civilian_deployment.bin"); }));
    formats().push_back(make<CivilianShelter>("civilian_shelter", [](const std::string& p) { return name_is(p, "civilian_shelter.bin"); }));
}

} // namespace wce
