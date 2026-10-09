#include "formats/bmd_bin.h"
#include "formats/registry.h"

#include <cstring>
#include <stdexcept>

namespace te {
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
    template <size_t N> void floats(std::array<float, N>& a) { for (float& f : a) f = get<float>(); }
    template <size_t N> void bytes(std::array<uint8_t, N>& a) { for (uint8_t& f : a) f = get<uint8_t>(); }
    std::string str() { uint16_t n = get<uint16_t>(); const uint8_t* s = take(n); return std::string(reinterpret_cast<const char*>(s), n); }
    // `u16 version(=1)` + `u32 count(=0)` sections that are empty in all shipped data
    void empty_versioned(const char* what) {
        if (get<uint16_t>() != 1) throw std::runtime_error(std::string(what) + ": version != 1");
        empty(what);
    }
    void empty(const char* what) {
        if (get<uint32_t>() != 0) throw std::runtime_error(std::string(what) + " is non-empty: layout unknown");
    }
};

struct Writer {
    std::vector<uint8_t> out;
    void raw(const void* d, size_t n) { const uint8_t* s = static_cast<const uint8_t*>(d); out.insert(out.end(), s, s + n); }
    template <class T> void put(T v) { raw(&v, sizeof(T)); }
    template <size_t N> void floats(const std::array<float, N>& a) { for (float f : a) put(f); }
    template <size_t N> void bytes(const std::array<uint8_t, N>& a) { for (uint8_t f : a) put(f); }
    void str(const std::string& s) { put<uint16_t>(static_cast<uint16_t>(s.size())); raw(s.data(), s.size()); }
    void count(size_t n) { put<uint32_t>(static_cast<uint32_t>(n)); }
    void empty_versioned() { put<uint16_t>(1); put<uint32_t>(0); }
};

std::vector<Outline> read_outlines(Reader& r) {
    std::vector<Outline> v;
    for (uint32_t n = r.get<uint32_t>(); n; --n) {
        Outline o;
        for (uint32_t k = r.get<uint32_t>(); k; --k) {
            float x = r.get<float>();
            float y = r.get<float>();
            o.push_back({x, y});
        }
        v.push_back(std::move(o));
    }
    return v;
}

void write_outlines(Writer& w, const std::vector<Outline>& v) {
    w.count(v.size());
    for (const Outline& o : v) {
        w.count(o.size());
        for (auto& p : o) { w.put(p[0]); w.put(p[1]); }
    }
}

} // namespace

BmdBin BmdBin::parse(const std::vector<uint8_t>& bytes) {
    Reader r{bytes};
    BmdBin m;
    if (std::memcmp(r.take(8), kMagic, 8) != 0) throw std::runtime_error("not FASTBIN0");
    m.version = r.get<uint16_t>();
    if (m.version != 6) throw std::runtime_error("unsupported version " + std::to_string(m.version));
    for (uint32_t n = r.get<uint32_t>(); n; --n) {
        BmdProp p;
        p.key = r.str();
        p.marker_version = r.get<uint16_t>();
        r.floats(p.transform);
        p.clamp_to_surface = r.get<uint8_t>();
        p.type = r.get<uint32_t>();
        p.allow_in_outfield = r.get<uint8_t>();
        r.floats(p.decal_params);
        r.bytes(p.flags);
        m.props.push_back(std::move(p));
    }
    for (uint32_t n = r.get<uint32_t>(); n; --n) {
        BmdEmitter e;
        e.key = r.str();
        e.marker_version = r.get<uint16_t>();
        e.emission_rate = r.get<float>();
        r.floats(e.transform);
        e.instance_name = r.str();
        e.autoplay = r.get<uint8_t>();
        e.scale = r.get<float>();
        r.bytes(e.flags);
        m.emitters.push_back(std::move(e));
    }
    r.empty("AI hint polylines");
    r.empty("AI hint polyline lists");
    r.empty("AI separators");
    r.empty_versioned("deployment area manager");
    m.buildings_version = r.get<uint16_t>();
    for (uint32_t n = r.get<uint32_t>(); n; --n) {
        BmdBuilding b;
        b.version = r.get<uint16_t>();
        b.key = r.str();
        b.position_type = r.str();
        r.floats(b.position);
        r.floats(b.orientation);
        r.floats(b.scale);
        b.starting_damage = r.get<float>();
        r.bytes(b.flags);
        m.buildings.push_back(std::move(b));
    }
    r.empty_versioned("far buildings");
    m.capture_version = r.get<uint16_t>();
    for (uint32_t sets = r.get<uint32_t>(); sets; --sets) {
        std::vector<BmdCapture> set;
        for (uint32_t n = r.get<uint32_t>(); n; --n) {
            BmdCapture c;
            r.floats(c.location);
            c.radius = r.get<float>();
            c.min_players = r.get<uint32_t>();
            c.max_players = r.get<uint32_t>();
            c.type = r.str();
            for (uint32_t k = r.get<uint32_t>(); k; --k) {
                float x = r.get<float>();
                float y = r.get<float>();
                c.points.push_back({x, y});
            }
            for (uint32_t k = r.get<uint32_t>(); k; --k) c.building_indices.push_back(r.get<uint32_t>());
            c.database_key = r.str();
            r.floats(c.flag_facing);
            set.push_back(std::move(c));
        }
        m.capture_sets.push_back(std::move(set));
    }
    r.empty("EF lines");
    m.go_outlines = read_outlines(r);
    m.non_terrain_outlines = read_outlines(r);
    r.empty_versioned("zones");
    r.empty_versioned("prefab instances");
    r.empty_versioned("BMD outlines");
    for (const char* what : {"terrain outlines", "lite building outlines", "civilian deployment list", "civilian shelter list"}) r.empty(what);
    if (r.p != bytes.size()) throw std::runtime_error(std::to_string(bytes.size() - r.p) + " trailing bytes");
    return m;
}

std::vector<uint8_t> BmdBin::write() const {
    Writer w;
    w.raw(kMagic, 8);
    w.put<uint16_t>(version);
    w.count(props.size());
    for (const BmdProp& p : props) {
        w.str(p.key);
        w.put(p.marker_version);
        w.floats(p.transform);
        w.put(p.clamp_to_surface);
        w.put(p.type);
        w.put(p.allow_in_outfield);
        w.floats(p.decal_params);
        w.bytes(p.flags);
    }
    w.count(emitters.size());
    for (const BmdEmitter& e : emitters) {
        w.str(e.key);
        w.put(e.marker_version);
        w.put(e.emission_rate);
        w.floats(e.transform);
        w.str(e.instance_name);
        w.put(e.autoplay);
        w.put(e.scale);
        w.bytes(e.flags);
    }
    w.count(0); w.count(0); w.count(0);
    w.empty_versioned();
    w.put(buildings_version);
    w.count(buildings.size());
    for (const BmdBuilding& b : buildings) {
        w.put(b.version);
        w.str(b.key);
        w.str(b.position_type);
        w.floats(b.position);
        w.floats(b.orientation);
        w.floats(b.scale);
        w.put(b.starting_damage);
        w.bytes(b.flags);
    }
    w.empty_versioned();
    w.put(capture_version);
    w.count(capture_sets.size());
    for (const auto& set : capture_sets) {
        w.count(set.size());
        for (const BmdCapture& c : set) {
            w.floats(c.location);
            w.put(c.radius);
            w.put(c.min_players);
            w.put(c.max_players);
            w.str(c.type);
            w.count(c.points.size());
            for (auto& p : c.points) { w.put(p[0]); w.put(p[1]); }
            w.count(c.building_indices.size());
            for (uint32_t i : c.building_indices) w.put(i);
            w.str(c.database_key);
            w.floats(c.flag_facing);
        }
    }
    w.count(0); // EF lines
    write_outlines(w, go_outlines);
    write_outlines(w, non_terrain_outlines);
    w.empty_versioned(); w.empty_versioned(); w.empty_versioned();
    w.count(0); w.count(0); w.count(0); w.count(0);
    return w.out;
}

// ---- registry entry
static bool ends_with(const std::string& s, const char* t) {
    size_t n = std::strlen(t);
    return s.size() >= n && s.compare(s.size() - n, n, t) == 0;
}

void register_bmd_bin_format() {
    Format f;
    f.name = "bmd_bin";
    f.claims = [](const std::string& path, const std::vector<uint8_t>& head) {
        bool name = ends_with(path, "_procedural_bmd_data.bin") ||
                    (path.find("bmd_data_escalation") != std::string::npos && ends_with(path, ".bin"));
        return name && head.size() >= 8 && std::memcmp(head.data(), kMagic, 8) == 0;
    };
    f.round_trip = [](const std::vector<uint8_t>& bytes) {
        RoundTripResult r;
        try {
            r.output = BmdBin::parse(bytes).write();
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
