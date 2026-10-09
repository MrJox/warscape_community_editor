#include "clouds.h"

#include "formats/env_value.h"
#include "formats/xml_doc.h"

#include <algorithm>
#include <cctype>
#include <cmath>

namespace we {

namespace {

struct Rand {
    uint32_t state;
    float next() { // fn 0x18f0bf0 et al.: imul 0x343fd, add 0x269ec3, shr 16, * 1/65535
        state = state * 0x343fdu + 0x269ec3u;
        return float(state >> 16) * (1.0f / 65535.0f);
    }
};

struct PlaneConstants {
    float radius, centre, jitter;
    int u_tiles, v_tiles;
};

PlaneConstants constants(const CloudPlane& p) {
    return p.far_plane ? PlaneConstants{972800.0f, -912000.0f, 0.0f, 1, 2} : PlaneConstants{608000.0f, -592800.0f, 0.5f, 2, 2};
}

float attr(const XmlDoc& d, int e, const char* name, float def) {
    auto v = d.get(e, name);
    if (!v) return def;
    EnvValue x = parse_value(*v);
    if (x.kind == ValueKind::Bool) return x.b ? 1.0f : 0.0f;
    if ((x.kind == ValueKind::Float || x.kind == ValueKind::Int) && !x.f.empty()) return x.f[0];
    return def;
}

void colour(const XmlDoc& d, int e, const char* name, float out[3]) {
    if (auto v = d.get(e, name)) {
        EnvValue x = parse_value(*v);
        if (x.kind == ValueKind::Rgb)
            for (int i = 0; i < 3; ++i) out[i] = x.f[i];
    }
}

const CloudTemplate* choose(const CloudPlane& p, Rand& rng) {
    auto valid = [](const CloudTemplate& c) {
        return std::any_of(c.elements.begin(), c.elements.end(), [](const CloudElement& e) { return e.enabled; });
    };
    float total = 0;
    int n = 0;
    for (auto& c : p.clouds) {
        if (c.this_cloud_only) return &c;
        if (valid(c)) {
            total += c.probability;
            ++n;
        }
    }
    if (!n) return nullptr;
    float pick = rng.next() * total, acc = 0;
    for (auto& c : p.clouds)
        if (valid(c) && c.probability > 0.0f) {
            acc += c.probability;
            if (acc >= pick) return &c;
        }
    return nullptr;
}

// round-to-nearest of the original (the SSE "add/sub 2^23" idiom with a correction step).
int round_index(float max_index, float r) { return int(std::floor(max_index * r + 0.5f)); }

} // namespace

std::vector<CloudPlane> read_cloud_planes(const XmlDoc& doc) {
    std::vector<CloudPlane> planes;
    const auto& els = doc.elements();
    for (int i = 0; i < int(els.size()); ++i) {
        if (els[i].name != "CLOUD_PLANE_MESH") continue;
        CloudPlane p;
        auto nm = doc.get(i, "name");
        std::string lname = nm ? *nm : "";
        std::transform(lname.begin(), lname.end(), lname.begin(), [](char c) { return char(std::tolower(uint8_t(c))); });
        p.far_plane = lname.find("far") != std::string::npos;
        p.grid_size = std::max(1, int(attr(doc, i, "grid_size", 1)));
        p.height_scale = attr(doc, i, "height_scale", 1);
        p.velocity = attr(doc, i, "cloud_velocity", 0);
        p.density = attr(doc, i, "cloud_density", 0);
        p.flatten = attr(doc, i, "flatten_factor", 0);
        colour(doc, i, "sun_transmitted_colour", p.transmitted);
        colour(doc, i, "sun_reflected_colour", p.reflected);
        colour(doc, i, "cloud_shadow_colour", p.shadow);
        p.transmit_falloff = attr(doc, i, "sun_transmit_falloff", 0);
        p.shadow_gradient = attr(doc, i, "cloud_shadow_gradient", 0);
        p.shadow_falloff = attr(doc, i, "cloud_shadow_falloff", 0);
        p.horizon_alpha_start = attr(doc, i, "horizon_alpha_start", 0);
        p.horizon_alpha_end = attr(doc, i, "horizon_alpha_end", 1);
        p.height_variation = attr(doc, i, "cloud_height_variation", 0);
        p.rotation = attr(doc, i, "cloud_rotation", 0);
        if (auto cs = doc.get(i, "cloudset")) p.cloudset = *cs;
        for (int c : els[i].children) {
            if (els[c].name != "CLOUD") continue;
            CloudTemplate t;
            t.this_cloud_only = attr(doc, c, "this_cloud_only", 0) != 0;
            t.probability = attr(doc, c, "probability", 0);
            for (int e : els[c].children) {
                if (els[e].name != "CLOUD_ELEMENT") continue;
                CloudElement ce;
                ce.enabled = attr(doc, e, "enabled", 0) != 0;
                ce.scale = attr(doc, e, "scale", 0);
                ce.offset[0] = attr(doc, e, "offset_x", 0);
                ce.offset[1] = attr(doc, e, "offset_y", 0);
                ce.offset[2] = attr(doc, e, "offset_z", 0);
                ce.texture_index = int(attr(doc, e, "texture_index", 0));
                ce.texture_row = int(attr(doc, e, "texture_row", 0));
                ce.texture_column = int(attr(doc, e, "texture_column", 0));
                t.elements.push_back(ce);
            }
            p.clouds.push_back(std::move(t));
        }
        planes.push_back(std::move(p));
    }
    return planes;
}

float CloudField::Rng::next() {
    state = state * 0x343fdu + 0x269ec3u;
    return float(state >> 16) * (1.0f / 65535.0f);
}

namespace {
struct Geometry {
    float cell_angle, cell_m;
    int cells;
};
Geometry geometry(const CloudPlane& p) {
    PlaneConstants k = constants(p);
    float cap = std::acos(-k.centre / k.radius);
    float a = 2.0f * cap / float(p.grid_size);
    return {a, a * k.radius, p.grid_size + 2};
}
} // namespace

void CloudField::reset(const CloudPlane& p, uint32_t seed) {
    inst_.clear();
    speed_.clear();
    timer_ = 0;
    rng_.state = seed;
    const int cells = p.grid_size + 2;
    for (int x = 0; x < cells; ++x)
        for (int y = 0; y < cells; ++y)
            if (p.density >= rng_.next()) spawn(p, x, y);
}

void CloudField::update(const CloudPlane& p, float dt) {
    if (!(p.velocity * 0.1f > 0.001f) || dt <= 0) return;
    const Geometry g = geometry(p);
    const float limit = g.cells * 0.5f * g.cell_angle;
    for (size_t i = 0; i < inst_.size();) {
        inst_[i].t2[0] += speed_[i] * dt;
        if (inst_[i].t2[0] > limit) {
            inst_[i] = inst_.back();
            speed_[i] = speed_.back();
            inst_.pop_back();
            speed_.pop_back();
        } else {
            ++i;
        }
    }
    timer_ += dt;
    const float interval = g.cell_angle / p.velocity;
    if (timer_ >= interval) { // fn 0x1192a900: one roll per column, then the timer restarts from 0 (not decremented)
        timer_ = 0;
        for (int y = 0; y < g.cells; ++y)
            if (p.density >= rng_.next()) spawn(p, 0, y);
    }
}

void CloudField::spawn(const CloudPlane& p, int x, int y) {
    const PlaneConstants k = constants(p);
    const Geometry geo = geometry(p);
    const float cell_angle = geo.cell_angle, cell_m = geo.cell_m;
    const int cells = geo.cells;
    const float rot_range = p.rotation * 3.14159265f;
    auto& rng = rng_;
    std::vector<CloudInstance>& out = inst_;
    {
        {
            // --- fn 0x18f0bf0 ---
            float jx = 0, jy = 0;
            if (k.jitter > 0) {
                jx = (rng.next() - 0.5f) * k.jitter;
                jy = (rng.next() - 0.5f) * k.jitter;
            }
            float cloud_rot = rng.next() * rot_range * 2.0f - rot_range;
            float ax = (float(x) + jx - cells * 0.5f + 0.5f) * cell_angle;
            float ay = (float(y) + jy - cells * 0.5f + 0.5f) * cell_angle;
            float speed = 0;
            if (p.velocity * 0.1f > 0.001f) speed = (1.0f + rng.next() * 65535.0f * 7.629511e-7f - 0.025f) * p.velocity; // 1 +- 2.5%
            Rand pick{rng.state};
            const CloudTemplate* t = choose(p, pick);
            rng.state = pick.state;
            if (!t) return;
            float cloud_radius = 0;
            for (auto& e : t->elements) {
                if (!e.enabled) continue;
                float s = std::min(e.scale, 4.0f);
                float r = std::sqrt(e.offset[0] * cell_m * e.offset[0] * cell_m + e.offset[2] * cell_m * e.offset[2] * cell_m) + s * cell_m * 0.5f;
                cloud_radius = std::max(cloud_radius, float(int(r)));
            }
            float hv = (rng.next() - 0.5f) * p.height_variation * 2.0f * cell_m;
            for (auto& e : t->elements) {
                float r6 = rng.next(), r5 = rng.next(), r7 = rng.next(), r4 = rng.next();
                if (!e.enabled) continue;
                CloudInstance ci{};
                int map = 0;
                if (k.v_tiles > 1) map = e.texture_index > 0 ? std::min(e.texture_index - 1, k.v_tiles - 1) : round_index(float(k.v_tiles - 1), r6);
                float du = 1.0f / float(k.u_tiles), u = 0, v = 0;
                if (k.u_tiles > 1) {
                    u = (e.texture_column > 0 ? float(e.texture_column - 1) : float(round_index(float(k.u_tiles - 1), r7))) * du;
                    v = (e.texture_row > 0 ? float(e.texture_row - 1) : float(round_index(float(k.u_tiles - 1), r4))) * du;
                }
                ci.t1[0] = u; ci.t1[1] = v; ci.t1[2] = du; ci.t1[3] = du;
                ci.t2[0] = ax; ci.t2[1] = ay; ci.t2[2] = k.radius; ci.t2[3] = k.centre;
                ci.t3[0] = 1.0f;
                ci.t3[1] = r5 * rot_range * 2.0f - rot_range;
                ci.t3[2] = std::min(e.scale, 4.0f) * cell_angle;
                ci.t3[3] = cloud_rot;
                ci.t4[0] = e.offset[0] * cell_m; ci.t4[1] = e.offset[1] * cell_m + hv; ci.t4[2] = e.offset[2] * cell_m; ci.t4[3] = 1.0f;
                ci.t5[0] = float(map);
                ci.t5[1] = cloud_radius;
                out.push_back(ci);
                speed_.push_back(speed);
            }
        }
    }
}

std::vector<CloudInstance> generate_clouds(const CloudPlane& p, uint32_t seed) {
    CloudField f;
    f.reset(p, seed);
    return f.instances();
}

std::string cloud_texture(const CloudPlane& p, int map) {
    std::string set = p.cloudset;
    std::transform(set.begin(), set.end(), set.begin(), [](char c) { return char(std::tolower(uint8_t(c))); });
    std::string base = "rigidmodels/skybox/cloudsets/" + set + "/";
    return base + (p.far_plane ? (map ? "far2.dds" : "far1.dds") : (map ? "near_set2.dds" : "near_set1.dds"));
}

} // namespace we
