#include "foundation/thread_pool.h"
#include "foundation/profile.h"
#include "tile/tile_document.h"

#include "formats/rmv2.h"
#include "foundation/log.h"
#include "foundation/process.h"
#include "vfs/vfs.h"

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <charconv>
#include <functional>

namespace fs = std::filesystem;

namespace wce {

namespace {

constexpr double kPi = 3.14159265358979323846;

using TypeStyle = TileTypeStyle;

TypeStyle style_of(const std::string& t) {
    if (t == "BUILDING_INSTANCE") return {overlay_colour(0.85f, 0.45f, 0.2f), 5.f, false};
    if (t == "VEGETATION_INSTANCE") return {overlay_colour(0.2f, 0.7f, 0.25f), 1.5f, true};
    if (t == "ProjectedDecalInstance") return {overlay_colour(0.55f, 0.45f, 0.35f), 1.5f, true};
    if (t == "VFXInstance") return {overlay_colour(0.75f, 0.35f, 0.9f), 2.f, false};
    if (t == "POINT_LIGHT_INSTANCE") return {overlay_colour(1.f, 0.95f, 0.4f), 1.5f, true};
    if (t == "SIMPLE_MESH_INSTANCE") return {overlay_colour(0.6f, 0.6f, 0.75f), 4.f, false};
    if (t == "CaptureLocationInstance") return {overlay_colour(0.2f, 0.6f, 1.f), 3.f, false};
    if (t == "SiegeAINodeInstance" || t == "SiegeAIEdgeInstance") return {overlay_colour(0.95f, 0.2f, 0.2f), 2.f, false};
    if (t.find("POLYGON") != std::string::npos || t.find("Polygon") != std::string::npos || t.find("Zone") != std::string::npos || t == "AISeparatorInstance")
        return {overlay_colour(0.3f, 0.9f, 0.9f), 1.5f, true};
    return {overlay_colour(0.8f, 0.8f, 0.8f), 2.f, false};
}

float degrees(double r) { return float(r * 180.0 / kPi); }
double radians(float d) { return double(d) * kPi / 180.0; }

bool same_overlay(const Overlay& a, const Overlay& b) {
    auto eq = [](const std::vector<OverlayVertex>& x, const std::vector<OverlayVertex>& y) {
        return x.size() == y.size() && (x.empty() || std::memcmp(x.data(), y.data(), x.size() * sizeof(OverlayVertex)) == 0);
    };
    return eq(a.lines, b.lines) && eq(a.triangles, b.triangles);
}

// A piece is usable when the renderer can draw it (the same test as the sample map, see ModelCache::model).
bool piece_bounds(IVfs& vfs, const std::string& path, float3& bmin, float3& bmax, std::vector<std::string>& textures) {
    try {
        Rmv2Model m = parse_rmv2(vfs.read(path), 0);
        if (m.lods.empty()) return false;
        bool any = false;
        for (auto& me : m.lods[0].meshes) {
            if (me.shader != RS_STANDARD_V5 || me.vertex_format != 0 || me.vertices.empty() || me.indices.empty()) continue;
            bmin = {std::min(bmin.x, me.aabb[0]), std::min(bmin.y, me.aabb[1]), std::min(bmin.z, me.aabb[2])};
            bmax = {std::max(bmax.x, me.aabb[3]), std::max(bmax.y, me.aabb[4]), std::max(bmax.z, me.aabb[5])};
            for (auto& t : me.textures) textures.push_back(t.second);
            any = true;
        }
        return any;
    } catch (const std::exception&) {
        return false;
    }
}

float4x4 matrix_of(const WorldTransform& w, double unit, const std::optional<Vec3>& scale) {
    float4x4 m;
    const Vec3 s = scale.value_or(Vec3{1, 1, 1});
    for (int r = 0; r < 3; ++r)
        for (int c = 0; c < 3; ++c) m.m[r][c] = float(w.rotation[r * 3 + c] * s[c]);
    m.m[0][3] = float(w.position[0] / unit);
    m.m[1][3] = float(w.position[1]);
    m.m[2][3] = float(w.position[2] / unit);
    return m;
}

} // namespace

// ------------------------------------------------------------------------------------------------ commands

class TedMoveCommand : public Command {
public:
    TedMoveCommand(TileDocument& d, int elem, std::optional<Vec3> op, std::optional<Vec3> np, std::optional<Vec3> oe, std::optional<Vec3> ne, uint64_t gesture)
        : doc_(d), elem_(elem), old_pos_(op), new_pos_(np), old_euler_(oe), new_euler_(ne), gesture_(gesture) {}
    std::string name() const override {
        const TedScene& s = doc_.tscene();
        std::string n = s.name(elem_);
        return "Move " + (n.empty() ? s.type(elem_) : n);
    }
    void apply() override { set(new_pos_, new_euler_); }
    void revert() override { set(old_pos_, old_euler_); }
    uint64_t merge_key() const override { return gesture_ ? gesture_ ^ (uint64_t(elem_) << 20) : 0; }
    bool merge_from(const Command& next) override {
        const auto* n = dynamic_cast<const TedMoveCommand*>(&next);
        if (!n || n->elem_ != elem_) return false;
        if (n->new_pos_) new_pos_ = n->new_pos_;
        if (n->new_euler_) new_euler_ = n->new_euler_;
        if (!old_pos_) old_pos_ = n->old_pos_;
        if (!old_euler_) old_euler_ = n->old_euler_;
        return true;
    }
    std::vector<uint64_t> dirty_items() const override { return {TileDocument::item_of(elem_)}; }

private:
    void set(const std::optional<Vec3>& p, const std::optional<Vec3>& e) {
        if (p) doc_.tscene().set_position(elem_, *p);
        if (e) doc_.tscene().set_euler(elem_, *e);
    }
    TileDocument& doc_;
    int elem_;
    std::optional<Vec3> old_pos_, new_pos_, old_euler_, new_euler_;
    uint64_t gesture_;
};

class TedPointCommand : public Command {
public:
    TedPointCommand(TileDocument& d, int elem, size_t index, Vec3 before, Vec3 after, uint64_t gesture)
        : doc_(d), elem_(elem), index_(index), before_(before), after_(after), gesture_(gesture) {}
    std::string name() const override { return "Move polygon point " + std::to_string(index_); }
    void apply() override { doc_.tscene().set_polygon_point(elem_, index_, after_); }
    void revert() override { doc_.tscene().set_polygon_point(elem_, index_, before_); }
    uint64_t merge_key() const override { return gesture_ ? gesture_ ^ (uint64_t(elem_) << 20) ^ (uint64_t(index_) << 44) : 0; }
    bool merge_from(const Command& next) override {
        const auto* n = dynamic_cast<const TedPointCommand*>(&next);
        if (!n || n->elem_ != elem_ || n->index_ != index_) return false;
        after_ = n->after_;
        return true;
    }
    std::vector<uint64_t> dirty_items() const override { return {TileDocument::item_of(elem_)}; }

private:
    TileDocument& doc_;
    int elem_;
    size_t index_;
    Vec3 before_, after_;
    uint64_t gesture_;
};

// A structural edit changes element indices; its undo record is the list of text splices it made (a few hundred bytes, not a copy of the document). The scene
// already holds the result when the command is pushed.
class TedSpliceCommand : public Command {
public:
    TedSpliceCommand(TileDocument& d, std::string name, std::vector<XmlSplice> record, std::vector<uint64_t> sel_before, std::vector<uint64_t> sel_after)
        : doc_(d), name_(std::move(name)), record_(std::move(record)), sel_before_(std::move(sel_before)), sel_after_(std::move(sel_after)) {}
    std::string name() const override { return name_; }
    void apply() override {
        if (first_) first_ = false;
        else doc_.tscene().apply(record_);
        doc_.idx_.valid = false;
        ++doc_.structure_version_;
        doc_.hint_ = sel_after_;
    }
    void revert() override {
        doc_.tscene().unapply(record_);
        doc_.idx_.valid = false;
        ++doc_.structure_version_;
        doc_.hint_ = sel_before_;
    }

private:
    TileDocument& doc_;
    std::string name_;
    std::vector<XmlSplice> record_;
    std::vector<uint64_t> sel_before_, sel_after_;
    bool first_ = true;
};

// The eye / cursor of one or more layers or instances: the flags text is replaced, so undo restores the original bytes. Visibility changes what is drawn below the
// element, so the document republishes everything (no dirty items).
class TedFlagCommand : public Command {
public:
    TedFlagCommand(TileDocument& d, std::vector<int> elems, const char* flag, bool on) : doc_(d), elems_(std::move(elems)), flag_(flag), on_(on) {
        for (int e : elems_) {
            old_.push_back(d.tscene().doc().get(e, "flags"));
            d.tscene().set_flag(e, flag, on);
            new_.push_back(d.tscene().doc().get(e, "flags"));
            d.tscene().set_attr(e, "flags", old_.back().value_or(""));   // the stack applies it
        }
    }
    std::string name() const override {
        const bool eye = flag_ == "OIF_VISIBLE";
        std::string n = elems_.size() == 1 ? doc_.tscene().name(elems_[0]) : std::to_string(elems_.size()) + " items";
        if (n.empty() && elems_.size() == 1) n = doc_.tscene().type(elems_[0]);
        return std::string(eye ? (on_ ? "Show " : "Hide ") : (on_ ? "Unlock " : "Lock ")) + n;
    }
    void apply() override { put(new_); }
    void revert() override { put(old_); }

private:
    void put(const std::vector<std::optional<std::string>>& v) {
        for (size_t i = 0; i < elems_.size(); ++i)
            if (v[i]) doc_.tscene().set_attr(elems_[i], "flags", *v[i]);
        doc_.idx_.valid = false;
        ++doc_.structure_version_;
    }
    TileDocument& doc_;
    std::vector<int> elems_;
    std::string flag_;
    bool on_;
    std::vector<std::optional<std::string>> old_, new_;
};

// One brush step; the steps of a stroke merge into one entry. The first step runs when pushed; undo and redo use the height field's own records
// (HeightField::end_edit stores them, so a stroke is closed before it can be undone).
class BrushStepCommand : public Command {
public:
    BrushStepCommand(TileDocument& d, BrushKind kind, float x, float z, BrushSettings s, float dt, uint64_t stroke)
        : doc_(d), kind_(kind), x_(x), z_(z), s_(s), dt_(dt), stroke_(stroke) {}
    std::string name() const override {
        static const char* n[] = {"Raise", "Lower", "Smooth", "Flatten", "Level"};
        return std::string("Terrain ") + n[int(kind_)];
    }
    void apply() override {
        if (first_) {
            first_ = false;
            doc_.apply_brush(kind_, x_, z_, s_, dt_);
        } else {
            doc_.end_stroke();
            doc_.project_->height->redo();
        }
    }
    void revert() override {
        doc_.end_stroke();
        doc_.project_->height->undo();
    }
    uint64_t merge_key() const override { return stroke_; }
    bool merge_from(const Command& next) override { return dynamic_cast<const BrushStepCommand*>(&next) != nullptr; }
    std::vector<uint64_t> dirty_items() const override { return {TileDocument::kTerrain}; }

private:
    TileDocument& doc_;
    BrushKind kind_;
    float x_, z_;
    BrushSettings s_;
    float dt_;
    uint64_t stroke_;
    bool first_ = true;
};

// ------------------------------------------------------------------------------------------------ document

TileDocument::TileDocument(std::unique_ptr<TileProject> project, IVfs* vfs) : project_(std::move(project)), vfs_(vfs) {
    auto since = [](std::chrono::steady_clock::time_point t0) { return std::chrono::duration<double>(std::chrono::steady_clock::now() - t0).count(); };
    const auto t0 = std::chrono::steady_clock::now();
    build_index();
    const double t_index = since(t0);
    resolve_buildings();
    const double t_models = since(t0) - t_index;
    rebuild_items(false);   // the markers: everything that is not a building with a model, and the buildings that have none
    if (project_->has_height) saved_height_depth_ = project_->height->edit_depth();
    char b[240];
    std::snprintf(b, sizeof b, "tile loaded: %zu instances in %.2f s (element index %.2f, building models %.2f, markers %.2f)", instances_.size(), since(t0), t_index, t_models,
                  since(t0) - t_index - t_models);
    load_info = b;
    Log::info(load_info);
}
TileDocument::~TileDocument() {
    if (prefetch_.valid()) prefetch_.wait();
}

std::string TileDocument::title() const {
    const fs::path& d = project_->dir;
    return d.filename().string() + "  (" + d.parent_path().filename().string() + ")";
}

bool TileDocument::modified() const { return history_.modified(); }

std::string TileDocument::source_key() const {
    std::string k = project_->dir.generic_string();
    for (char& c : k) c = char(std::tolower(static_cast<unsigned char>(c)));
    return k;
}

bool TileDocument::is_polygon(int elem) const {
    if (idx_.valid && elem >= 0 && size_t(elem) < idx_.kind.size()) return (idx_.kind[size_t(elem)] & kPolygonBit) != 0;
    return elem >= 0 && elem < int(tscene().doc().elements().size()) && !tscene().polygon_points(elem).empty();
}

bool TileDocument::is_instance(int elem) const {
    if (idx_.valid && elem >= 0 && size_t(elem) < idx_.kind.size()) return (idx_.kind[size_t(elem)] & kInstanceBit) != 0;
    const auto& els = tscene().doc().elements();
    if (elem < 0 || elem >= int(els.size())) return false;
    const std::string& t = els[elem].name;
    if (t == "point" || t == "points" || t == "children" || t == "camera" || tscene().is_layer(elem)) return false;
    return tscene().position(elem).has_value();
}

int TileDocument::layer_of(int elem) const {
    const TedScene& sc = tscene();
    for (int p = elem; p >= 0; p = sc.doc().elements()[p].parent)
        if (sc.is_layer(p)) return p;
    return sc.scene_graph();
}

float3 TileDocument::world_position(int elem) const {
    if (idx_.valid && elem >= 0 && size_t(elem) < idx_.slot.size()) {
        const WorldTransform& w = idx_.world[idx_.slot[size_t(elem)]];
        return {float(w.position[0] / unit()), float(w.position[1]), float(w.position[2] / unit())};
    }
    const WorldTransform w = world_transform(tscene(), elem);
    return {float(w.position[0] / unit()), float(w.position[1]), float(w.position[2] / unit())};
}

float4x4 TileDocument::world_matrix(int elem) const {
    if (idx_.valid && elem >= 0 && size_t(elem) < idx_.slot.size()) {
        const auto& s = idx_.scale[size_t(elem)];
        return matrix_of(idx_.world[idx_.slot[size_t(elem)]], unit(), Vec3{s[0], s[1], s[2]});
    }
    return matrix_of(world_transform(tscene(), elem), unit(), tscene().scale(elem));
}

const BuildingModel* TileDocument::building_model(const std::string& key) const {
    auto it = models_.find(key);
    return it == models_.end() ? nullptr : &it->second;
}

const BuildingModel* TileDocument::model_of(int elem) const {
    if (idx_.valid && elem >= 0 && size_t(elem) < idx_.key.size()) {
        const int32_t k = idx_.key[size_t(elem)];
        return k >= 0 && size_t(k) < key_model_.size() ? key_model_[size_t(k)] : nullptr;
    }
    return building_model(tscene().doc().get(elem, "key").value_or(""));
}

float TileDocument::marker_radius(int elem) const {
    if (const BuildingModel* m = model_of(elem); m && m->ok) {
        const float3 e = m->bmax - m->bmin;
        return std::clamp(0.5f * std::max({e.x, e.y, e.z}), 2.0f, 80.0f);
    }
    if (idx_.valid && elem >= 0 && size_t(elem) < idx_.type.size()) return idx_.style[idx_.type[size_t(elem)]].size * 0.75f;
    return style_of(tscene().type(elem)).size * 0.75f;
}

// ---- scene -> items

// ---- the element index

namespace {

// "vector(1,2,3)" / "euler(..)" (prefix) or "x,y,z" (empty prefix) without allocating; false when the text is anything else.
bool parse_triple(std::string_view s, std::string_view prefix, double out[3]) {
    if (!prefix.empty()) {
        if (s.size() < prefix.size() + 2 || s.substr(0, prefix.size()) != prefix || s[prefix.size()] != '(' || s.back() != ')') return false;
        s = s.substr(prefix.size() + 1, s.size() - prefix.size() - 2);
    }
    const char* p = s.data();
    const char* end = p + s.size();
    for (int i = 0; i < 3; ++i) {
        while (p < end && *p == ' ') ++p;
        const auto r = std::from_chars(p, end, out[i]);
        if (r.ec != std::errc()) {   // "+1", "inf" ...: the slow path knows more number syntaxes
            const auto v = parse_vec3(std::string(s), "");
            if (!v) return false;
            for (int k = 0; k < 3; ++k) out[k] = (*v)[size_t(k)];
            return true;
        }
        p = r.ptr;
        if (i < 2) {
            while (p < end && *p == ' ') ++p;
            if (p >= end || *p != ',') return false;
            ++p;
        }
    }
    return true;
}

} // namespace

void TileDocument::build_index() {
    WCE_PROF("tile.index");
    const XmlDoc& doc = tscene().doc();
    const auto& els = doc.elements();
    const size_t n = els.size();
    Index& ix = idx_;
    ix = Index{};
    ix.type.resize(n);
    ix.kind.assign(n, 0);
    ix.slot.assign(n, 0);
    ix.scale.assign(n, {1.0f, 1.0f, 1.0f});
    ix.key.assign(n, -1);
    // element type names -> small ids
    {
        std::unordered_map<std::string_view, uint16_t> ids;
        for (size_t i = 0; i < n; ++i) {
            auto it = ids.find(els[i].name);
            if (it == ids.end()) {
                it = ids.emplace(std::string_view(els[i].name), uint16_t(ix.type_names.size())).first;
                ix.type_names.push_back(els[i].name);
            }
            ix.type[i] = it->second;
        }
    }
    ix.style.reserve(ix.type_names.size());
    for (const std::string& t : ix.type_names) ix.style.push_back(style_of(t));
    // which type ids are layers / never instances
    std::vector<uint8_t> layer_type(ix.type_names.size(), 0), plain_type(ix.type_names.size(), 0);
    int key_type = -1;
    for (size_t t = 0; t < ix.type_names.size(); ++t) {
        const std::string& s = ix.type_names[t];
        layer_type[t] = (s == "LayerInstance" || s == "UserInstanceGroup" || s == "INSTANCE_GROUP" || s == "SCENE_GRAPH") ? 1 : 0;
        plain_type[t] = (s == "point" || s == "points" || s == "children" || s == "camera") ? 1 : 0;
        if (s == "BUILDING_INSTANCE") key_type = int(t);
    }
    // parse the local transforms in parallel (each element reads only its own attributes)
    struct Local {
        double pos[3], eul[3];
        uint8_t has_pos, has_eul;
    };
    std::vector<Local> local(n);
    std::vector<uint8_t> flagbits(n, 0);   // 1: has a flags attribute, 2: OIF_VISIBLE, 4: OIF_ACTIVE
    ix.state.assign(n, 3);
    compute_pool().parallel_for(n, 2048, [&](size_t b, size_t e, int) {
        for (size_t i = b; i < e; ++i) {
            Local& l = local[i];
            l.has_pos = l.has_eul = 0;
            const uint16_t t = ix.type[i];
            uint8_t kind = layer_type[t] ? kLayerBit : 0;
            if (plain_type[t] && !kind) continue;   // points, wrappers: nothing to parse (a point's position is a plain triple, not a vector)
            size_t k;
            if (doc.find_attr(int(i), "flags", &k)) {
                const std::string_view fv = doc.attr_raw(int(i), k);
                flagbits[i] = uint8_t(1 | (fv.find("OIF_VISIBLE") != std::string_view::npos ? 2 : 0) | (fv.find("OIF_ACTIVE") != std::string_view::npos ? 4 : 0));
            }
            if (doc.find_attr(int(i), "position", &k)) l.has_pos = parse_triple(doc.attr_raw(int(i), k), "vector", l.pos) ? 1 : 0;
            if (doc.find_attr(int(i), "orientation", &k)) l.has_eul = parse_triple(doc.attr_raw(int(i), k), "euler", l.eul) ? 1 : 0;
            if (!kind && l.has_pos) {
                kind |= kInstanceBit;
                if (doc.find_attr(int(i), "scale", &k)) {
                    double s3[3];
                    const std::string_view raw = doc.attr_raw(int(i), k);
                    if (parse_triple(raw, "vector", s3)) ix.scale[i] = {float(s3[0]), float(s3[1]), float(s3[2])};
                    else {
                        char* end_ptr = nullptr;
                        const std::string tmp(raw);
                        const double d = std::strtod(tmp.c_str(), &end_ptr);
                        if (end_ptr != tmp.c_str()) ix.scale[i] = {float(d), float(d), float(d)};
                    }
                }
                // a polygon: an instance with a <points> child holding <point>s
                for (int c : els[i].children)
                    if (els[size_t(c)].name == "points") {
                        for (int p : els[size_t(c)].children)
                            if (els[size_t(p)].name == "point") { kind |= kPolygonBit; break; }
                        break;
                    }
            }
            ix.kind[i] = kind;
        }
    });
    // compose the parent chains: elements are in document order, so a parent always comes before its children
    ix.world.assign(1, WorldTransform{});
    std::unordered_map<std::string, int32_t> key_ids;
    for (size_t i = 0; i < n; ++i) {
        const int p = els[i].parent;
        {   // visibility and selectability are inherited from the ancestors
            const uint8_t ps = p >= 0 ? ix.state[size_t(p)] : uint8_t(3), fb = flagbits[i];
            ix.state[i] = fb ? uint8_t(ps & ((fb & 2 ? 1 : 0) | (fb & 4 ? 2 : 0))) : ps;
        }
        uint32_t slot = p >= 0 ? ix.slot[size_t(p)] : 0;
        const Local& l = local[i];
        if (l.has_pos || l.has_eul) {
            WorldTransform w = ix.world[slot];
            if (l.has_pos) {
                const Vec3 r = rotate_vec(w.rotation, Vec3{l.pos[0], l.pos[1], l.pos[2]});
                for (int a = 0; a < 3; ++a) w.position[a] += r[a];
            }
            if (l.has_eul) w.rotation = mat_mul(w.rotation, euler_matrix(Vec3{l.eul[0], l.eul[1], l.eul[2]}));
            slot = uint32_t(ix.world.size());
            ix.world.push_back(w);
        }
        ix.slot[i] = slot;
        if (int(ix.type[i]) == key_type && (ix.kind[i] & kInstanceBit)) {
            size_t k;
            if (doc.find_attr(int(i), "key", &k)) {
                const std::string key(doc.attr_raw(int(i), k));
                if (!key.empty()) {
                    auto it = key_ids.find(key);
                    if (it == key_ids.end()) {
                        it = key_ids.emplace(key, int32_t(ix.keys.size())).first;
                        ix.keys.push_back(key);
                    }
                    ix.key[i] = it->second;
                }
            }
        }
    }
    ix.valid = true;
    grid_dirty_ = true;
}

void TileDocument::index_edited(int elem) {
    if (!idx_.valid || elem < 0 || size_t(elem) >= idx_.slot.size()) return;
    const int parent = tscene().doc().elements()[size_t(elem)].parent;
    const uint32_t own = idx_.slot[size_t(elem)];
    if (own != 0 && parent >= 0 && own != idx_.slot[size_t(parent)] && !(idx_.kind[size_t(elem)] & kLayerBit)) {
        idx_.world[own] = world_transform(tscene(), elem);   // an instance owns its slot: only it changes
    } else {
        build_index();                                       // a layer (or an element without a transform of its own): everything below it moved
    }
    grid_dirty_ = true;
}

// ---- buildings

void TileDocument::log_fallbacks() {
    if (fallbacks_.empty()) return;
    std::string list;
    for (size_t i = 0; i < fallbacks_.size() && i < 8; ++i) list += (i ? ", " : "") + fallbacks_[i];
    if (fallbacks_.size() > 8) list += ", ...";
    Log::warn("tile " + title() + ": " + std::to_string(fallbacks_.size()) + " of " + std::to_string(idx_.keys.size()) + " building keys have no drawable model and show as boxes: " + list);
}

void TileDocument::resolve_buildings() {
    WCE_PROF("tile.resolve_buildings");
    key_model_.assign(idx_.keys.size(), nullptr);
    if (idx_.keys.empty()) return;
    std::set<std::string> fresh;                    // keys no one has looked at yet
    for (size_t k = 0; k < idx_.keys.size(); ++k) {
        auto it = models_.find(idx_.keys[k]);
        if (it != models_.end()) key_model_[k] = &it->second;
        else fresh.insert(idx_.keys[k]);
    }
    std::map<std::string, std::vector<std::string>> folders;
    if (vfs_ && !fresh.empty()) {
        const std::string prefix = "rigidmodels/buildings/";
        for (const std::string& p : vfs_->list(prefix, ".rigid_model_v2")) {
            const size_t slash = p.rfind('/');
            const std::string folder = p.substr(prefix.size(), slash - prefix.size());
            if (fresh.count(folder) == 0) continue;
            const std::string file = p.substr(slash + 1);
            if (file.find("destruct") != std::string::npos && file.find("destruct01") == std::string::npos) continue;
            folders[folder].push_back(p);
        }
    }
    std::vector<std::string> wanted;
    for (size_t k = 0; k < idx_.keys.size(); ++k) {
        if (key_model_[k]) continue;
        const std::string& key = idx_.keys[k];
        BuildingModel bm;
        bm.pieces = std::move(folders[key]);
        bm.ok = !bm.pieces.empty();   // assumed until the pieces have loaded (poll_models)
        bm.resolved = !bm.ok;
        wanted.insert(wanted.end(), bm.pieces.begin(), bm.pieces.end());
        BuildingModel& slot = models_[key] = std::move(bm);
        key_model_[k] = &slot;
        if (!slot.ok) fallbacks_.push_back(key);
    }
    unresolved_.clear();
    for (size_t k = 0; k < idx_.keys.size(); ++k)
        if (key_model_[k] && key_model_[k]->ok && !key_model_[k]->resolved) unresolved_.push_back(int(k));
    if (unresolved_.empty() && !wanted.empty()) log_fallbacks();
    else if (unresolved_.empty() && fresh.size() == idx_.keys.size()) log_fallbacks();
    // The first time a tile is opened its models and textures are not extracted from the packs yet: fetch them in batches in the background (one call per
    // pack) so that the loads that follow find files instead of asking the pack tool for each of them.
    if (vfs_ && !wanted.empty()) {
        auto done = std::make_shared<std::promise<void>>();
        prefetch_ = done->get_future();
        IVfs* vfs = vfs_;
        io_pool().post(
            [vfs, wanted = std::move(wanted), done] {
                WCE_PROF("tile.prefetch");
                try {
                    vfs->prefetch(wanted);
                    std::set<std::string> textures;
                    for (const std::string& p : wanted) {
                        try {
                            const Rmv2Model m = parse_rmv2(vfs->read(p), 0, false);
                            if (m.lods.empty()) continue;
                            for (const auto& me : m.lods[0].meshes)
                                for (const auto& t : me.textures) textures.insert(t.second);
                        } catch (const std::exception&) {}
                    }
                    vfs->prefetch(std::vector<std::string>(textures.begin(), textures.end()));
                } catch (const std::exception& e) {
                    Log::warn(std::string("tile: could not prefetch the building models: ") + e.what());
                }
                done->set_value();
            },
            -1.0f);
    }
}

bool TileDocument::poll_models(const std::function<PieceState(const std::string&)>& peek) {
    if (unresolved_.empty()) return false;
    bool changed = false, any = false;
    for (auto it = unresolved_.begin(); it != unresolved_.end();) {
        BuildingModel& m = models_[idx_.keys[size_t(*it)]];
        int loading = 0, ready = 0;
        float3 lo{1e30f, 1e30f, 1e30f}, hi{-1e30f, -1e30f, -1e30f};
        for (const std::string& piece : m.pieces) {
            const PieceState s = peek(piece);
            if (s.status == PieceState::Status::Loading) ++loading;
            else if (s.status == PieceState::Status::Ready) {
                ++ready;
                lo = {std::min(lo.x, s.bmin.x), std::min(lo.y, s.bmin.y), std::min(lo.z, s.bmin.z)};
                hi = {std::max(hi.x, s.bmax.x), std::max(hi.y, s.bmax.y), std::max(hi.z, s.bmax.z)};
            }
        }
        if (loading) { ++it; continue; }
        m.ok = ready > 0;
        m.resolved = true;
        if (m.ok) { m.bmin = lo; m.bmax = hi; }
        else { fallbacks_.push_back(idx_.keys[size_t(*it)]); changed = true; }
        any = true;
        it = unresolved_.erase(it);
    }
    if (any) grid_dirty_ = true;
    if (unresolved_.empty() && any) log_fallbacks();
    if (changed) rebuild_items(false);   // the buildings without a model become markers
    return changed;
}

void TileDocument::rebuild_items(bool reindex) {
    WCE_PROF("tile.rebuild_items");
    if (reindex || !idx_.valid) {
        build_index();
        if (!models_.empty() || !idx_.keys.empty()) resolve_buildings();   // new key ids; a building with a key no one has seen yet is looked up
    }
    const Index& ix = idx_;
    const size_t n = ix.type.size(), types = ix.type_names.size();
    instances_.clear();
    type_counts_.clear();
    std::vector<uint32_t> counts(types, 0), marker_counts(types, 0);
    std::vector<uint8_t> is_marker(n, 0);
    for (size_t i = 0; i < n; ++i) {
        if (!(ix.kind[i] & kInstanceBit)) continue;
        ++counts[ix.type[i]];
        instances_.push_back(int(i));
        if (!(ix.state[i] & 1)) continue;   // hidden by its layer: no marker, no model entity
        if (ix.key[i] >= 0) {
            const BuildingModel* m = key_model_[size_t(ix.key[i])];
            if (m && m->ok) continue;   // drawn as a model
        }
        is_marker[i] = 1;
        ++marker_counts[ix.type[i]];
    }
    for (size_t t = 0; t < types; ++t)
        if (counts[t]) type_counts_[ix.type_names[t]] = int(counts[t]);
    // markers grouped by type (counting sort, stable: the element order inside a group)
    Markers& mk = markers_;
    mk.group_begin.assign(types + 1, 0);
    for (size_t t = 0; t < types; ++t) mk.group_begin[t + 1] = mk.group_begin[t] + marker_counts[t];
    const size_t total = mk.group_begin[types];
    mk.elem.resize(total);
    mk.x.resize(total);
    mk.y.resize(total);
    mk.z.resize(total);
    mk.slot.assign(n, -1);
    std::vector<uint32_t> cursor(mk.group_begin.begin(), mk.group_begin.end() - 1);
    for (size_t i = 0; i < n; ++i) {
        if (!is_marker[i]) continue;
        const uint32_t at = cursor[ix.type[i]]++;
        const WorldTransform& w = ix.world[ix.slot[i]];
        mk.elem[at] = int32_t(i);
        mk.x[at] = float(w.position[0] / unit());
        mk.y[at] = float(w.position[1]);
        mk.z[at] = float(w.position[2] / unit());
        mk.slot[i] = int32_t(at);
    }
    dirty_type_.assign(types, 0);
    markers_dirty_ = true;
    polygons_dirty_ = true;
    grid_dirty_ = true;
}

float TileDocument::lod_distance(const std::string& type) const {
    if (type == "VEGETATION_INSTANCE") return display_.veg_distance;
    if (type == "ProjectedDecalInstance") return display_.decal_distance;
    return display_.other_distance;
}

// ---- publishing

void TileDocument::update_terrain(RenderScene& scene, bool first) {
    TileProject& p = *project_;
    Terrain t;
    if (p.has_height) {
        const HeightField& hf = *p.height;
        terrain_stride_ = 1;   // the renderer cuts the grid into patches with LODs, the full resolution costs nothing far away
        t.nx = std::max(2, int((hf.width() - 1) / terrain_stride_) + 1);
        t.nz = std::max(2, int((hf.height() - 1) / terrain_stride_) + 1);
        const float k = float(p.mpp_z() / p.mpp_x());
        t.step = float(p.mpp_x() * terrain_stride_);
        t.origin_x = float(-p.border_m());
        t.origin_z = float(-p.border_m()) / k;
        t.heights.resize(size_t(t.nx) * t.nz);
        compute_pool().parallel_for(size_t(t.nz), 16, [&](size_t j0, size_t j1, int) {
            for (size_t j = j0; j < j1; ++j)
                for (int i = 0; i < t.nx; ++i) t.heights[j * size_t(t.nx) + size_t(i)] = float(hf.metres(uint32_t(std::min<int>(i * terrain_stride_, int(hf.width()) - 1)), uint32_t(std::min<int>(int(j) * terrain_stride_, int(hf.height()) - 1))));
        });
        t.lod = true;
        float4x4 w;
        w.m[2][2] = k;
        t.version = ++terrain_version_;
        if (first) terrain_entity_ = scene.add(layer_, w, std::move(t), SourceRef{id(), kTerrain});
        else scene.update(terrain_entity_, Payload{std::move(t)});
    } else {
        t = Terrain::flat(float(std::max(extent_x(), extent_z()) * 0.5), "terrain/textures/battle/default/grass/0_su_diffuse.dds", 8.0f);
        t.version = ++terrain_version_;
        const float4x4 w = translation({float(extent_x() * 0.5), 0, float(extent_z() * 0.5)});
        if (first) terrain_entity_ = scene.add(layer_, w, std::move(t), SourceRef{id(), kTerrain});
        else scene.update(terrain_entity_, Payload{std::move(t)});
    }
}

void TileDocument::add_item_entities(RenderScene& scene, LayerId layer, int elem) {
    if (!shown(elem)) return;
    if (const BuildingModel* m = model_of(elem); m && m->ok) {
        const float4x4 w = world_matrix(elem);
        EntityId first = 0;
        uint8_t count = 0;
        for (const std::string& piece : m->pieces) {
            MeshInstance mi;
            mi.model = piece;
            mi.lod = true;
            const EntityId e = scene.add(layer, w, std::move(mi), SourceRef{id(), item_of(elem)});
            if (!count) first = e;   // the pieces of a building get consecutive ids
            ++count;
        }
        first_entity_[size_t(elem)] = first;
        entity_count_[size_t(elem)] = count;
        if (!selectable(elem))
            for (uint8_t k = 0; k < count; ++k)
                if (Entity* en = scene.find(first + k)) en->locked = true;
    }
    if (display_.show_polygons && is_polygon(elem)) rebuild_polygon(scene, layer, elem);
}

void TileDocument::rebuild_polygon(RenderScene& scene, LayerId layer, int elem) {
    Overlay o;
    o.pickable = false;
    const TypeStyle st = idx_.valid ? idx_.style[idx_.type[size_t(elem)]] : style_of(tscene().type(elem));
    const std::vector<float3> pts = polygon_world_points(elem);
    const bool closed = tscene().doc().get(elem, "polygon_closed").value_or("1") != "0";
    std::vector<float3> lifted;
    for (const float3& p : pts) lifted.push_back({p.x, p.y + 0.3f, p.z});
    o.polygon_outline(lifted, closed, st.color);
    auto it = outline_entity_.find(elem);
    if (it != outline_entity_.end() && scene.find(it->second)) scene.update(it->second, Payload{std::move(o)});
    else outline_entity_[elem] = scene.add(layer, float4x4{}, std::move(o), SourceRef{id(), item_of(elem)});
}

void TileDocument::rebuild_batch(RenderScene& scene, uint16_t type) {
    WCE_PROF("tile.rebuild_batch");
    if (type >= batch_entity_.size() || !batch_entity_[type]) return;
    Overlay o;
    o.pickable = false;
    const std::string& name = idx_.type_names[type];
    const TypeStyle& st = idx_.style[type];
    if (display_.hidden_types.count(name) == 0) {
        const float lim = lod_distance(name);
        const float lim2 = lim * lim;
        const bool cull = display_.lod && lod_eye_.x < 1e29f;
        const uint32_t b = markers_.group_begin[type], e = markers_.group_begin[type + 1];
        size_t visible = 0;
        if (cull) {
            for (uint32_t i = b; i < e; ++i) {
                const float dx = markers_.x[i] - lod_eye_.x, dy = markers_.y[i] - lod_eye_.y, dz = markers_.z[i] - lod_eye_.z;
                visible += (dx * dx + dy * dy + dz * dz <= lim2) ? 1 : 0;
            }
        } else {
            visible = e - b;
        }
        o.lines.reserve(visible * (st.cross ? 6 : 24));
        const float h = st.size * 0.5f;
        for (uint32_t i = b; i < e; ++i) {
            const float px = markers_.x[i], py = markers_.y[i], pz = markers_.z[i];
            if (cull) {
                const float dx = px - lod_eye_.x, dy = py - lod_eye_.y, dz = pz - lod_eye_.z;
                if (dx * dx + dy * dy + dz * dz > lim2) continue;
            }
            if (st.cross) {
                o.line({px - h, py + 0.4f, pz}, {px + h, py + 0.4f, pz}, st.color);
                o.line({px, py + 0.4f, pz - h}, {px, py + 0.4f, pz + h}, st.color);
                o.line({px, py, pz}, {px, py + st.size, pz}, st.color);
            } else {
                o.box_wire({px - h, py - h, pz - h}, {px + h, py + h, pz + h}, st.color);
            }
        }
    }
    scene.update(batch_entity_[type], Payload{std::move(o)});
}

void TileDocument::publish(RenderScene& scene, LayerId layer) { publish_items(scene, layer, false); }

// keep_terrain: the heights did not change (a structural edit of the instances): the terrain entity stays, so its patches are not built again.
void TileDocument::publish_items(RenderScene& scene, LayerId layer, bool keep_terrain) {
    WCE_PROF("tile.publish");
    keep_terrain = keep_terrain && scene_ == &scene && terrain_entity_ && scene.find(terrain_entity_);
    if (keep_terrain) scene.clear_layer_except(layer, terrain_entity_);
    else scene.clear_layer(layer);
    scene_ = &scene;
    layer_ = layer;
    const size_t n = idx_.type.size();
    first_entity_.assign(n, 0);
    entity_count_.assign(n, 0);
    outline_entity_.clear();
    have_highlight_ = false;
    size_t pieces = 0;
    for (int e : instances_)
        if (const BuildingModel* m = model_of(e); m && m->ok) pieces += m->pieces.size();
    scene.reserve(pieces + instances_.size() / 16 + 64);
    if (!keep_terrain) update_terrain(scene, true);
    for (int e : instances_) add_item_entities(scene, layer, e);
    batch_entity_.assign(idx_.type_names.size(), 0);
    for (size_t t = 0; t < idx_.type_names.size(); ++t) {
        if (markers_.group_begin[t + 1] == markers_.group_begin[t]) continue;
        Overlay empty;
        empty.pickable = false;
        batch_entity_[t] = scene.add(layer, float4x4{}, std::move(empty), SourceRef{id(), kTerrain});
    }
    dirty_type_.assign(idx_.type_names.size(), 0);
    Overlay h;
    h.depth_test = false;
    h.pickable = false;
    overlay_entity_ = scene.add(layer, float4x4{}, std::move(h), SourceRef{id(), kTerrain});
    markers_dirty_ = true;
    hidden_signature_ = size_t(-1);
    lod_eye_ = {1e30f, 1e30f, 1e30f};
}

void TileDocument::refresh_item(RenderScene& scene, int elem) {
    if (!is_instance(elem)) return;
    index_edited(elem);
    if (size_t(elem) < first_entity_.size() && first_entity_[size_t(elem)]) {
        const float4x4 w = world_matrix(elem);
        for (uint8_t k = 0; k < entity_count_[size_t(elem)]; ++k) scene.update(first_entity_[size_t(elem)] + k, w);
    }
    if (size_t(elem) < markers_.slot.size() && markers_.slot[size_t(elem)] >= 0) {
        const size_t s = size_t(markers_.slot[size_t(elem)]);
        const float3 p = world_position(elem);
        markers_.x[s] = p.x;
        markers_.y[s] = p.y;
        markers_.z[s] = p.z;
        dirty_type_[idx_.type[size_t(elem)]] = 1;
    }
    if (is_polygon(elem) && display_.show_polygons) rebuild_polygon(scene, layer_, elem);
}

void TileDocument::republish(RenderScene& scene, LayerId layer, const CommandStack::Dirty& dirty) {
    if (dirty.all || scene_ != &scene) {
        rebuild_items();
        publish_items(scene, layer, true);
        return;
    }
    for (uint64_t item : dirty.items) {
        if (item == kTerrain) update_terrain(scene, false);
        else refresh_item(scene, elem_of(item));
    }
}

void TileDocument::update_view(RenderScene& scene, LayerId layer, const float3& eye) {
    WCE_PROF("tile.update_view");
    if (scene_ != &scene) return;
    (void)layer;
    // a type that has markers now but had none when the tile was published (buildings whose models turned out not to load) needs its overlay entity
    if (batch_entity_.size() < idx_.type_names.size()) batch_entity_.resize(idx_.type_names.size(), 0);
    for (size_t t = 0; t < idx_.type_names.size() && t + 1 < markers_.group_begin.size(); ++t)
        if (!batch_entity_[t] && markers_.group_begin[t + 1] != markers_.group_begin[t]) {
            Overlay empty;
            empty.pickable = false;
            batch_entity_[t] = scene.add(layer_, float4x4{}, std::move(empty), SourceRef{id(), kTerrain});
            markers_dirty_ = true;
        }
    bool all = markers_dirty_;
    if (!all && display_.lod) {
        const float3 d = eye - lod_eye_;
        const float hysteresis = std::max(20.0f, 0.05f * std::min(display_.veg_distance, display_.decal_distance));   // the markers pop at their draw distance by about that much
        if (lod_eye_.x > 1e29f || dot(d, d) > hysteresis * hysteresis) all = true;
    }
    if (all) {
        lod_eye_ = eye;
        for (size_t t = 0; t < batch_entity_.size(); ++t) rebuild_batch(scene, uint16_t(t));
        // the model pieces of buildings follow the type visibility
        size_t sig = 0;
        for (const std::string& h : display_.hidden_types) sig = sig * 1000003u + std::hash<std::string>{}(h);
        if (sig != hidden_signature_) {
            hidden_signature_ = sig;
            const uint16_t building = [&] {
                for (size_t t = 0; t < idx_.type_names.size(); ++t)
                    if (idx_.type_names[t] == "BUILDING_INSTANCE") return uint16_t(t);
                return uint16_t(0xffff);
            }();
            const bool hidden = display_.hidden_types.count("BUILDING_INSTANCE") != 0;
            for (int e : instances_) {
                if (idx_.type[size_t(e)] != building || !first_entity_[size_t(e)]) continue;
                for (uint8_t k = 0; k < entity_count_[size_t(e)]; ++k)
                    if (Entity* en = scene.find(first_entity_[size_t(e)] + k)) en->visible = !hidden;
            }
            scene.touch();
        }
        markers_dirty_ = false;
        std::fill(dirty_type_.begin(), dirty_type_.end(), uint8_t(0));
    }
    for (size_t t = 0; t < dirty_type_.size(); ++t)
        if (dirty_type_[t]) {
            rebuild_batch(scene, uint16_t(t));
            dirty_type_[t] = 0;
        }
    if (polygons_dirty_) {
        polygons_dirty_ = false;
        for (int e : instances_) {
            if (!is_polygon(e)) continue;
            if (display_.show_polygons) rebuild_polygon(scene, layer_, e);
            else if (auto it = outline_entity_.find(e); it != outline_entity_.end()) {
                // hide: an empty outline
                Overlay o;
                o.pickable = false;
                scene.update(it->second, Payload{std::move(o)});
            }
        }
    }
}

void TileDocument::set_highlight(RenderScene& scene, const std::vector<int>& selected, int polygon_point, float handle_size, const Overlay& tool_overlay) {
    WCE_PROF("tile.set_highlight");
    if (scene_ != &scene || !overlay_entity_) return;
    Overlay h;
    h.depth_test = false;
    h.pickable = false;
    const uint32_t yellow = overlay_colour(1.0f, 1.0f, 0.2f), hot = overlay_colour(1.0f, 0.9f, 0.1f), white = overlay_colour(1, 1, 1);
    auto cross = [&](const float3& p, uint32_t c, float r) {
        h.line({p.x - r, p.y + 0.4f, p.z}, {p.x + r, p.y + 0.4f, p.z}, c);
        h.line({p.x, p.y + 0.4f, p.z - r}, {p.x, p.y + 0.4f, p.z + r}, c);
        h.line({p.x, p.y, p.z}, {p.x, p.y + r * 1.5f, p.z}, c);
    };
    for (int e : selected) {
        if (!is_instance(e)) continue;
        const float3 p = world_position(e);
        const float r = marker_radius(e) / 0.75f * 0.5f * 1.2f;
        h.box_wire({p.x - r, p.y, p.z - r}, {p.x + r, p.y + 2 * r, p.z + r}, yellow);
        const float a = std::max(8.0f, r * 1.5f);
        h.line(p, {p.x + a, p.y, p.z}, overlay_colour(1, 0.2f, 0.2f));
        h.line(p, {p.x, p.y + a, p.z}, overlay_colour(0.2f, 1, 0.2f));
        h.line(p, {p.x, p.y, p.z + a}, overlay_colour(0.3f, 0.5f, 1));
        if (is_polygon(e)) {
            const auto pts = polygon_world_points(e);
            for (size_t i = 0; i < pts.size(); ++i) cross(pts[i], int(i) == polygon_point ? hot : white, handle_size);
        }
    }
    h.lines.insert(h.lines.end(), tool_overlay.lines.begin(), tool_overlay.lines.end());
    h.triangles.insert(h.triangles.end(), tool_overlay.triangles.begin(), tool_overlay.triangles.end());
    if (have_highlight_ && same_overlay(h, last_highlight_)) return;
    last_highlight_ = h;
    have_highlight_ = true;
    scene.update(overlay_entity_, Payload{std::move(h)});
}

// ---- outliner / inspector / gizmo

OutlinerNode TileDocument::outline() const {
    if (outline_version_ == structure_version_) return outline_cache_;
    const TedScene& sc = tscene();
    std::function<OutlinerNode(int)> build = [&](int e) {
        std::string label = sc.name(e);
        if (label.empty()) label = sc.type(e);
        const std::vector<int> kids = sc.children(e);
        OutlinerNode n{label + "  [" + sc.type(e) + (kids.empty() ? "" : ", " + std::to_string(kids.size())) + "]", {id(), item_of(e)}, sc.type(e) != "SCENE_GRAPH", {}};
        size_t shown = 0;
        for (int k : kids) {
            if (++shown > 300) {
                n.children.push_back({"... " + std::to_string(kids.size() - 300) + " more", {id(), kTerrain}, false, {}});
                break;
            }
            n.children.push_back(build(k));
        }
        return n;
    };
    OutlinerNode root{title(), {id(), kTerrain}, true, {}};
    root.children.push_back({"Terrain", {id(), kTerrain}, true, {}});
    const int g = sc.scene_graph();
    if (g >= 0) root.children.push_back(build(g));
    outline_cache_ = std::move(root);
    outline_version_ = structure_version_;
    return outline_cache_;
}

std::string TileDocument::describe(const SourceRef& item) const {
    if (item.item == kTerrain) return "Terrain";
    const int e = elem_of(item.item);
    if (e < 0 || e >= int(tscene().doc().elements().size())) return {};
    std::string n = tscene().name(e);
    return tscene().type(e) + (n.empty() ? "" : "  '" + n + "'");
}

void TileDocument::inspect(const SourceRef& item, Inspector& ins) {
    if (item.item == kTerrain) {
        ins.section("Terrain");
        if (project_->has_height) {
            const HeightField& h = *project_->height;
            ins.text("height_map", "height map", std::to_string(h.width()) + " x " + std::to_string(h.height()) + " px");
            char b[160];
            std::snprintf(b, sizeof b, "%.2f .. %.2f m (min %.3f scale %.3f)%s", h.min_height(), h.min_height() + h.scale(), project_->info.min_height, project_->info.scale,
                          project_->info.found ? "" : "  [tile database entry not found]");
            ins.text("range", "heights", b);
            std::snprintf(b, sizeof b, "%.3f m/px, border %.0f m", project_->mpp_x(), project_->border_m());
            ins.text("scale", "resolution", b);
        } else ins.text("height_map", "height map", "none (no height_map_0.png)");
        ins.text("tile", "tile", std::to_string(project_->info.cells_x) + " x " + std::to_string(project_->info.cells_y) + " cells, " + std::to_string(int(extent_x())) + " x " + std::to_string(int(extent_z())) + " m");
        ins.text("ted", "tile.ted", "version " + std::to_string(tscene().project_version()) + ", " + std::to_string(instances_.size()) + " instances");
        return;
    }
    const int e = elem_of(item.item);
    TedScene& sc = tscene();
    if (e < 0 || e >= int(sc.doc().elements().size())) return;
    ins.section(sc.type(e));
    ins.text("name", "name", sc.name(e));
    if (auto k = sc.doc().get(e, "key")) {
        std::string v = *k;
        if (const BuildingModel* m = building_model(*k)) v += m->ok ? "  (" + std::to_string(m->pieces.size()) + " pieces)" : "  (no model: box)";
        ins.text("key", "key", v);
    }
    ins.text("layer", "layer", sc.layer_path(e));
    if (sc.doc().get(e, "flags")) {   // TEd's eye and cursor arrow of the Scene tab
        bool vis = own_flag(e, "OIF_VISIBLE"), act = own_flag(e, "OIF_ACTIVE");
        if (ins.edit_bool("flag_visible", "visible (eye)", vis)) set_flag(e, "OIF_VISIBLE", vis);
        if (ins.edit_bool("flag_active", "selectable (cursor)", act)) set_flag(e, "OIF_ACTIVE", act);
        if (!shown(e)) ins.text("hidden", "", "hidden by a layer above");
        else if (!selectable(e)) ins.text("locked", "", "locked by a layer above");
    }
    if (sc.is_layer(e) && sc.type(e) != "SCENE_GRAPH" && sc.doc().get(e, "export_type")) {
        static const std::vector<std::string> kinds = {"Normal", "Escalation1", "Escalation2", "Escalation3"};
        const std::string cur = export_type(e);
        int idx = 0;
        for (size_t i = 0; i < kinds.size(); ++i)
            if (kinds[i] == cur) idx = int(i);
        if (ins.edit_choice("export_type", "export type", idx, kinds) && kinds[size_t(idx)] != cur) set_attribute(e, "export_type", "LayerExportType_" + kinds[size_t(idx)]);
    }
    const uint64_t base = (uint64_t(id()) << 40) ^ (uint64_t(e) << 8) ^ 0x31;
    if (is_instance(e)) {   // TEd's X / Y / Z fields: the world position in metres
        const float3 w = world_position(e);
        float v[3] = {w.x, w.y, w.z};
        if (ins.edit_float3("position_world", "position (world, m)", v, 0.05f)) set_world_metres(e, {v[0], v[1], v[2]}, base + 2);
        if (ins.gesture_ended()) history_.break_merge();
    }
    if (auto p = sc.position(e)) {
        float v[3] = {float((*p)[0]), float((*p)[1]), float((*p)[2])};
        if (ins.edit_float3("position", "position (scene)", v, 0.5f)) set_local(e, Vec3{v[0], v[1], v[2]}, std::nullopt, base);
        if (ins.gesture_ended()) history_.break_merge();
    }
    if (auto eu = sc.euler(e)) {
        float v[3] = {degrees((*eu)[0]), degrees((*eu)[1]), degrees((*eu)[2])};
        if (ins.edit_float3("euler", "euler (deg)", v, 0.5f)) set_local(e, std::nullopt, Vec3{radians(v[0]), radians(v[1]), radians(v[2])}, base + 1);
        if (ins.gesture_ended()) history_.break_merge();
    }
    if (is_polygon(e)) {
        ins.text("points", "points", std::to_string(sc.polygon_points(e).size()));
        char b[64];
        std::snprintf(b, sizeof b, "%.1f", sc.polygon_area(e));
        ins.text("area", "area", b);
        if (sc.type(e) == "AI_HINT_POLYGON_INSTANCE") {
            static const std::vector<std::string> hints = {"AIH_FORT_APPROACH", "AIH_DEFENSIVE_HILL", "AIH_FORT_PERIMETER", "AIH_AMBUSH_CAVALRY_LINE", "AIH_AMBUSH_MELEE_LINE",
                "AIH_AMBUSH_MISSILE_LINE", "AIH_AMBUSH_ASSAULT_AREA", "AIH_AMBUSH_DEFENDER_INITIAL_PATH", "AIH_AMBUSH_FOREST", "AIH_AMBUSH_FIREBALL_LINE", "AIH_BEACH_LANDING",
                "AIH_DEFAULT_DEPLOYMENT_LINE", "AIH_RIVER_EDGE", "AIH_BRIDGE_CROSSING", "AIH_COVER_LINE", "AIH_BEACH_LANDING_FORT_APPROACH"};
            const std::string cur = sc.doc().get(e, "hint_type").value_or("");
            int idx = 0;
            for (size_t i = 0; i < hints.size(); ++i)
                if (hints[i] == cur) idx = int(i);
            if (ins.edit_choice("hint_type", "hint type", idx, hints) && hints[size_t(idx)] != cur) set_attribute(e, "hint_type", hints[size_t(idx)]);
        }
    }
    ins.section("Attributes");
    const auto& el = sc.doc().elements()[e];
    for (size_t i = 0; i < el.attrs.size(); ++i) ins.text("attr_" + el.attrs[i].name, el.attrs[i].name, std::string(sc.doc().attr_raw(e, i)));
}

bool TileDocument::item_transform(const SourceRef& item, float4x4& world) const {
    if (item.doc != id()) return false;
    const int e = elem_of(item.item);
    if (!is_instance(e) || !shown(e) || !selectable(e)) return false;   // a locked item has no gizmo
    const WorldTransform w = world_transform(tscene(), e);
    world = matrix_of(w, unit(), std::nullopt);   // the pivot: no scale
    return true;
}

void TileDocument::transform_items(const std::vector<SourceRef>& items, const float4x4& delta, uint64_t gesture) {
    for (const SourceRef& r : items) {
        float4x4 before;
        if (r.doc != id() || !item_transform(r, before)) continue;
        const int e = elem_of(r.item);
        const float4x4 now = delta * before;
        const WorldTransform pw = [&] {
            const int parent = tscene().doc().elements()[e].parent;
            return parent >= 0 ? world_transform(tscene(), parent) : WorldTransform{};
        }();
        const double u = unit();
        // new world position in scene units, local to the parent
        const Vec3 wp{double(now.m[0][3]) * u, double(now.m[1][3]), double(now.m[2][3]) * u};
        const Vec3 d{wp[0] - pw.position[0], wp[1] - pw.position[1], wp[2] - pw.position[2]};
        const Vec3 lp = rotate_vec(mat_transpose(pw.rotation), d);
        Mat3 wr, br;
        for (int a = 0; a < 3; ++a)
            for (int b = 0; b < 3; ++b) {
                wr[a * 3 + b] = now.m[a][b];
                br[a * 3 + b] = before.m[a][b];
            }
        double dr = 0;
        for (int k = 0; k < 9; ++k) dr = std::max(dr, std::fabs(wr[k] - br[k]));
        std::optional<Vec3> le;
        if (dr > 1e-5) le = euler_from_matrix(mat_mul(mat_transpose(pw.rotation), wr));
        set_local(e, lp, le, gesture);
    }
}

// ---- picking

void TileDocument::build_pick_grid() const {
    WCE_PROF("tile.pick_grid");
    PickGrid& g = grid_;
    g.valid = true;
    std::vector<int> pickable;   // shown and selectable instances
    pickable.reserve(instances_.size());
    for (int e : instances_)
        if (shown(e) && selectable(e)) pickable.push_back(e);
    const size_t m = pickable.size();
    g.elem.resize(m);
    g.x.resize(m);
    g.y.resize(m);
    g.z.resize(m);
    g.r.resize(m);
    g.type.resize(m);
    g.stamp.assign(m, 0);
    g.stamp_counter = 0;
    float minx = 1e30f, maxx = -1e30f, minz = 1e30f, maxz = -1e30f;
    for (size_t i = 0; i < m; ++i) {
        const int e = pickable[i];
        const float3 p = world_position(e);
        g.elem[i] = e;
        g.x[i] = p.x;
        g.y[i] = p.y;
        g.z[i] = p.z;
        g.r[i] = marker_radius(e);
        g.type[i] = idx_.type[size_t(e)];
        minx = std::min(minx, p.x);
        maxx = std::max(maxx, p.x);
        minz = std::min(minz, p.z);
        maxz = std::max(maxz, p.z);
    }
    if (!m) {
        g.nx = g.nz = 0;
        g.start.assign(1, 0);
        g.items.clear();
        return;
    }
    const float pad = 90.0f;   // the largest pick radius is 80 m
    g.cell = 64.0f;
    while (((maxx - minx + 2 * pad) / g.cell + 1) * ((maxz - minz + 2 * pad) / g.cell + 1) > 1.0e6f) g.cell *= 2;
    g.x0 = minx - pad;
    g.z0 = minz - pad;
    g.nx = int((maxx + pad - g.x0) / g.cell) + 1;
    g.nz = int((maxz + pad - g.z0) / g.cell) + 1;
    auto range = [&](size_t i, int& cx0, int& cx1, int& cz0, int& cz1) {
        cx0 = std::clamp(int((g.x[i] - g.r[i] - g.x0) / g.cell), 0, g.nx - 1);
        cx1 = std::clamp(int((g.x[i] + g.r[i] - g.x0) / g.cell), 0, g.nx - 1);
        cz0 = std::clamp(int((g.z[i] - g.r[i] - g.z0) / g.cell), 0, g.nz - 1);
        cz1 = std::clamp(int((g.z[i] + g.r[i] - g.z0) / g.cell), 0, g.nz - 1);
    };
    g.start.assign(size_t(g.nx) * size_t(g.nz) + 1, 0);
    for (size_t i = 0; i < m; ++i) {
        int cx0, cx1, cz0, cz1;
        range(i, cx0, cx1, cz0, cz1);
        for (int cz = cz0; cz <= cz1; ++cz)
            for (int cx = cx0; cx <= cx1; ++cx) ++g.start[size_t(cz) * size_t(g.nx) + size_t(cx) + 1];
    }
    for (size_t c = 0; c + 1 < g.start.size(); ++c) g.start[c + 1] += g.start[c];
    g.items.resize(g.start.back());
    std::vector<uint32_t> cursor(g.start.begin(), g.start.end() - 1);
    for (size_t i = 0; i < m; ++i) {
        int cx0, cx1, cz0, cz1;
        range(i, cx0, cx1, cz0, cz1);
        for (int cz = cz0; cz <= cz1; ++cz)
            for (int cx = cx0; cx <= cx1; ++cx) g.items[cursor[size_t(cz) * size_t(g.nx) + size_t(cx)]++] = uint32_t(i);
    }
}

// The nearest instance sphere along a ray: a walk over the grid cells the ray crosses, stopping at the first cell that starts beyond the best hit.
int TileDocument::pick_instance(const float3& o, const float3& d, bool visible_only) const {
    WCE_PROF("tile.pick_instance");
    if (!idx_.valid) return -1;   // right after a structural edit, until the republish rebuilds the index
    if (grid_dirty_ || !grid_.valid) {
        build_pick_grid();
        grid_dirty_ = false;
    }
    PickGrid& g = grid_;
    if (!idx_.valid || !g.nx) return -1;
    const size_t types = idx_.type_names.size();
    std::vector<uint8_t> hidden(types, 0);
    std::vector<float> lim2(types, 0.0f);   // 0: no draw distance
    uint16_t building = 0xffff;
    for (size_t t = 0; t < types; ++t) {
        const std::string& name = idx_.type_names[t];
        if (name == "BUILDING_INSTANCE") building = uint16_t(t);
        if (visible_only && display_.hidden_types.count(name)) hidden[t] = 1;
        if (visible_only && display_.lod && name != "BUILDING_INSTANCE") lim2[t] = lod_distance(name) * lod_distance(name);
    }
    float t0 = 0, t1 = 20000.0f;
    auto clip = [&](float oo, float dd, float lo, float hi) {
        if (std::fabs(dd) < 1e-9f) return oo >= lo && oo <= hi;
        float a = (lo - oo) / dd, b = (hi - oo) / dd;
        if (a > b) std::swap(a, b);
        t0 = std::max(t0, a);
        t1 = std::min(t1, b);
        return t0 <= t1;
    };
    if (!clip(o.x, d.x, g.x0, g.x0 + float(g.nx) * g.cell) || !clip(o.z, d.z, g.z0, g.z0 + float(g.nz) * g.cell)) return -1;
    int cx = std::clamp(int((o.x + d.x * t0 - g.x0) / g.cell), 0, g.nx - 1), cz = std::clamp(int((o.z + d.z * t0 - g.z0) / g.cell), 0, g.nz - 1);
    const int sx = d.x > 0 ? 1 : -1, sz = d.z > 0 ? 1 : -1;
    const float inf = 1e30f;
    float tmx = d.x != 0 ? (float(cx + (sx > 0 ? 1 : 0)) * g.cell + g.x0 - o.x) / d.x : inf;
    float tmz = d.z != 0 ? (float(cz + (sz > 0 ? 1 : 0)) * g.cell + g.z0 - o.z) / d.z : inf;
    const float tdx = d.x != 0 ? g.cell / std::fabs(d.x) : inf, tdz = d.z != 0 ? g.cell / std::fabs(d.z) : inf;
    const uint32_t stamp = ++g.stamp_counter;
    int best = -1;
    float best_t = 1e30f;
    for (;;) {
        const size_t cell = size_t(cz) * size_t(g.nx) + size_t(cx);
        for (uint32_t k = g.start[cell]; k < g.start[cell + 1]; ++k) {
            const uint32_t i = g.items[k];
            if (g.stamp[i] == stamp) continue;
            g.stamp[i] = stamp;
            const uint16_t type = g.type[i];
            if (hidden[type]) continue;
            float3 c{g.x[i], g.y[i], g.z[i]};
            const float r = g.r[i];
            if (type == building) c.y += r * 0.5f;
            if (lim2[type] > 0) {
                const float3 dd = c - o;
                if (dot(dd, dd) > lim2[type]) continue;
            }
            const float3 oc = o - c;
            const float b = dot(oc, d), cc = dot(oc, oc) - r * r;
            const float disc = b * b - cc;
            if (disc < 0) continue;
            const float t = -b - std::sqrt(disc);
            if (t > 0 && t < best_t) { best_t = t; best = g.elem[i]; }
        }
        const float t_exit = std::min(tmx, tmz);
        if (best_t <= t_exit || t_exit > t1) break;
        if (tmx < tmz) {
            cx += sx;
            tmx += tdx;
            if (cx < 0 || cx >= g.nx) break;
        } else {
            cz += sz;
            tmz += tdz;
            if (cz < 0 || cz >= g.nz) break;
        }
    }
    return best;
}

bool TileDocument::pick_terrain(const float3& o, const float3& d, float3& hit) const {
    if (!project_->has_height) return false;
    const double lo_x = -project_->border_m(), hi_x = extent_x() + project_->border_m(), lo_z = -project_->border_m(), hi_z = extent_z() + project_->border_m();
    const float step = std::max(0.5f, float(project_->mpp_x()));
    const float tmax = 20000.0f;
    float prev_t = 0;
    bool prev_above = true;
    for (float t = 0; t < tmax; t += step) {
        const float3 p = o + d * t;
        if (p.x < lo_x || p.z < lo_z || p.x > hi_x || p.z > hi_z) { prev_t = t; prev_above = true; continue; }
        const bool above = p.y > terrain_height(p.x, p.z);
        if (!above && prev_above && t > 0) {
            float a = prev_t, b = t;
            for (int k = 0; k < 20; ++k) {
                const float mid = (a + b) * 0.5f;
                const float3 q = o + d * mid;
                if (q.y > terrain_height(q.x, q.z)) a = mid; else b = mid;
            }
            hit = o + d * b;
            return true;
        }
        prev_above = above;
        prev_t = t;
    }
    return false;
}

// ---- edits

bool TileDocument::set_local(int elem, std::optional<Vec3> position, std::optional<Vec3> euler, uint64_t gesture) {
    TedScene& sc = tscene();
    if (!is_instance(elem)) return false;
    std::optional<Vec3> op = position ? sc.position(elem) : std::nullopt, oe = euler ? sc.euler(elem) : std::nullopt;
    if (position && !op) position.reset();
    if (euler && !oe) euler.reset();
    if (!position && !euler) return false;
    history_.push(std::make_unique<TedMoveCommand>(*this, elem, op, position, oe, euler, gesture));
    return true;
}

bool TileDocument::set_world_metres(int elem, const float3& p, uint64_t gesture) {
    if (!is_instance(elem)) return false;
    const int parent = tscene().doc().elements()[elem].parent;
    const WorldTransform pw = parent >= 0 ? world_transform(tscene(), parent) : WorldTransform{};
    const double u = unit();
    const Vec3 d{double(p.x) * u - pw.position[0], double(p.y) - pw.position[1], double(p.z) * u - pw.position[2]};
    return set_local(elem, rotate_vec(mat_transpose(pw.rotation), d), std::nullopt, gesture);
}

void TileDocument::push_splice(const std::string& name, size_t mark, std::vector<uint64_t> sel_before, std::vector<uint64_t> sel_after) {
    std::vector<XmlSplice> record = tscene().end_edit(mark);
    idx_.valid = false;   // element indices moved: the index is rebuilt by the republish that follows
    history_.push(std::make_unique<TedSpliceCommand>(*this, name, std::move(record), std::move(sel_before), std::move(sel_after)));
}

bool TileDocument::is_instance_slow(int elem) const {
    const auto& els = tscene().doc().elements();
    if (elem < 0 || elem >= int(els.size())) return false;
    const std::string& t = els[size_t(elem)].name;
    if (t == "point" || t == "points" || t == "children" || t == "camera" || tscene().is_layer(elem)) return false;
    return tscene().position(elem).has_value();
}

bool TileDocument::rotate(int elem, float deg) {
    if (!is_instance(elem)) return false;
    TedScene& sc = tscene();
    const Vec3 old_euler = sc.euler(elem).value_or(Vec3{0, 0, 0});
    rotate_world_yaw(sc, elem, radians(deg));
    const Vec3 new_euler = sc.euler(elem).value_or(old_euler);
    sc.set_euler(elem, old_euler);   // the command applies it
    return set_local(elem, std::nullopt, new_euler, 0);
}

int TileDocument::duplicate(int elem, std::string* error) {
    WCE_PROF("tile.duplicate");
    if (!is_instance(elem) && !tscene().is_layer(elem)) { if (error) *error = "nothing to duplicate"; return -1; }
    const size_t mark = tscene().begin_edit();
    const int copy = tscene().duplicate(elem);
    if (copy < 0) { if (error) *error = "cannot duplicate this element"; return -1; }
    if (is_instance_slow(copy)) {
        const WorldTransform w = world_transform(tscene(), copy);
        set_world_position(tscene(), copy, {w.position[0] + 4.0 * unit(), w.position[1], w.position[2]});
    }
    push_splice("Duplicate " + tscene().type(copy), mark, {item_of(elem)}, {item_of(copy)});
    return copy;
}

bool TileDocument::remove(int elem) {
    WCE_PROF("tile.remove");
    if (elem < 0 || elem >= int(tscene().doc().elements().size())) return false;
    const size_t mark = tscene().begin_edit();
    const std::string label = tscene().type(elem);
    tscene().remove(elem);
    push_splice("Delete " + label, mark, {item_of(elem)}, {});
    return true;
}

int TileDocument::add_building(const std::string& key, int template_elem, const float3* at, std::string* error) {
    if (key.empty()) return -1;
    int tmpl = -1;
    if (template_elem >= 0 && tscene().type(template_elem) == "BUILDING_INSTANCE") tmpl = template_elem;
    else if (auto all = tscene().find_all("BUILDING_INSTANCE"); !all.empty()) tmpl = all.front();
    if (tmpl < 0) { if (error) *error = "no BUILDING_INSTANCE to use as a template in this tile"; return -1; }
    const size_t mark = tscene().begin_edit();
    const int copy = tscene().duplicate(tmpl);
    tscene().set_attr(copy, "key", key);
    tscene().set_attr(copy, "name", key);
    if (at) set_world_position(tscene(), copy, {double(at->x) * unit(), double(at->y), double(at->z) * unit()});
    push_splice("Add " + key, mark, {template_elem >= 0 ? item_of(template_elem) : 0}, {item_of(copy)});
    return copy;
}

Vec3 TileDocument::to_local(int elem, const float3& world) const {
    const WorldTransform w = world_transform(tscene(), elem);
    const double u = unit();
    const Vec3 d{world.x * u - w.position[0], double(world.y) - w.position[1], world.z * u - w.position[2]};
    return rotate_vec(mat_transpose(w.rotation), d);
}

std::vector<float3> TileDocument::polygon_world_points(int elem) const {
    std::vector<float3> out;
    const WorldTransform w = world_transform(tscene(), elem);
    const double u = unit();
    for (int p : tscene().polygon_points(elem))
        if (auto v = tscene().point_position(p)) {
            const Vec3 r = rotate_vec(w.rotation, *v);
            out.push_back({float((w.position[0] + r[0]) / u), float(w.position[1] + r[1]), float((w.position[2] + r[2]) / u)});
        }
    return out;
}

int TileDocument::add_polygon(int layer, const std::string& type, const std::string& hint, const std::vector<float3>& pts, std::string* error) {
    if (pts.size() < 2) { if (error) *error = "a polygon needs at least 2 points"; return -1; }
    if (layer < 0) layer = tscene().scene_graph();
    if (layer < 0) { if (error) *error = "this tile has no SCENE_GRAPH, so there is no layer to add a polygon to"; return -1; }
    std::vector<Vec3> local;
    for (const float3& p : pts) local.push_back(to_local(layer, p));
    std::map<std::string, std::string> extra;
    if (type == "AI_HINT_POLYGON_INSTANCE" && !hint.empty()) extra["hint_type"] = hint;
    const size_t mark = tscene().begin_edit();
    const int e = tscene().add_polygon(layer, type, type == "AI_HINT_POLYGON_INSTANCE" ? "AI hint polygon" : "Polygon", local, extra);
    push_splice("Add " + type, mark, {}, {item_of(e)});
    return e;
}

bool TileDocument::polygon_point(int elem, const std::string& op, int index, const float3& world, std::string* error, uint64_t gesture) {
    if (!is_polygon(elem)) { if (error) *error = "not a polygon"; return false; }
    TedScene& sc = tscene();
    const size_t n = sc.polygon_points(elem).size();
    if (op == "move") {
        if (index < 0 || size_t(index) >= n) { if (error) *error = "no such point"; return false; }
        const auto pts = sc.polygon_points(elem);
        const Vec3 before = sc.point_position(pts[size_t(index)]).value_or(Vec3{0, 0, 0});
        history_.push(std::make_unique<TedPointCommand>(*this, elem, size_t(index), before, to_local(elem, world), gesture));
        return true;
    }
    const size_t mark = sc.begin_edit();
    if (op == "insert") {
        if (index < 0 || size_t(index) >= n) { if (error) *error = "no such point"; return false; }
        sc.insert_polygon_point(elem, size_t(index), to_local(elem, world));
        push_splice("Insert polygon point", mark, {item_of(elem)}, {item_of(elem)});
        return true;
    }
    if (op == "remove") {
        if (index < 0 || !sc.remove_polygon_point(elem, size_t(index))) { if (error) *error = "a polygon needs at least 2 points"; return false; }
        push_splice("Remove polygon point", mark, {item_of(elem)}, {item_of(elem)});
        return true;
    }
    if (error) *error = "unknown point operation '" + op + "'";
    return false;
}

int TileDocument::add_layer(int parent, const std::string& name, std::string* error) {
    if (parent < 0) { if (error) *error = "this tile has no SCENE_GRAPH, so layers cannot be added"; return -1; }
    const size_t mark = tscene().begin_edit();
    const int layer = tscene().add_layer(parent, name.empty() ? "New layer" : name);
    push_splice("Add layer", mark, {}, {item_of(layer)});
    return layer;
}

bool TileDocument::move_to_layer(int elem, int layer, std::string* error) {
    TedScene& sc = tscene();
    if (elem < 0 || elem == layer) return false;
    if (sc.is_descendant(layer, elem)) { if (error) *error = "cannot move a layer into itself"; return false; }
    const size_t mark = sc.begin_edit();
    const WorldTransform w = world_transform(sc, elem);
    const bool has_pos = sc.position(elem).has_value();
    const int moved = sc.move_to_layer(elem, layer);
    if (has_pos) set_world_position(sc, moved, w.position);   // keep the world position and orientation
    if (sc.euler(moved)) set_world_rotation(sc, moved, w.rotation);
    push_splice("Move to layer", mark, {item_of(elem)}, {item_of(moved)});
    return true;
}

bool TileDocument::rename(int elem, const std::string& name) { return set_attribute(elem, "name", name); }

bool TileDocument::set_attribute(int elem, const std::string& attr, const std::string& value) {
    if (elem < 0 || elem >= int(tscene().doc().elements().size())) return false;
    if (tscene().doc().get(elem, attr).value_or("\x01") == value) return true;
    const size_t mark = tscene().begin_edit();
    tscene().set_attr(elem, attr.c_str(), value);
    push_splice("Set " + attr, mark, {item_of(elem)}, {item_of(elem)});
    return true;
}

bool TileDocument::remove_layer(int layer) {
    if (!tscene().is_layer(layer) || tscene().type(layer) == "SCENE_GRAPH") return false;
    return remove(layer);
}

// ---- layer flags and structure

bool TileDocument::own_flag(int elem, const char* flag) const {
    const auto f = tscene().doc().get(elem, "flags");
    return !f || f->find(flag) != std::string::npos;
}

bool TileDocument::shown(int elem) const {
    if (idx_.valid && elem >= 0 && size_t(elem) < idx_.state.size()) return (idx_.state[size_t(elem)] & 1) != 0;
    for (int p = elem; p >= 0; p = tscene().doc().elements()[size_t(p)].parent)
        if (!own_flag(p, "OIF_VISIBLE")) return false;
    return true;
}

bool TileDocument::selectable(int elem) const {
    if (idx_.valid && elem >= 0 && size_t(elem) < idx_.state.size()) return (idx_.state[size_t(elem)] & 2) != 0;
    for (int p = elem; p >= 0; p = tscene().doc().elements()[size_t(p)].parent)
        if (!own_flag(p, "OIF_ACTIVE")) return false;
    return true;
}

bool TileDocument::set_flags(const std::vector<int>& elems, const char* flag, bool on) {
    std::vector<int> todo;
    for (int e : elems)
        if (e >= 0 && e < int(tscene().doc().elements().size()) && own_flag(e, flag) != on) todo.push_back(e);
    if (todo.empty()) return false;
    history_.push(std::make_unique<TedFlagCommand>(*this, std::move(todo), flag, on));
    return true;
}

std::string TileDocument::export_type(int layer) const {
    std::string t = tscene().doc().get(layer, "export_type").value_or("LayerExportType_Normal");
    const std::string prefix = "LayerExportType_";
    return t.compare(0, prefix.size(), prefix) == 0 ? t.substr(prefix.size()) : t;
}

int TileDocument::find_layer(const std::string& path) const {
    const TedScene& sc = tscene();
    int cur = sc.scene_graph();
    if (cur < 0) return -1;
    size_t at = 0;
    while (at <= path.size()) {
        size_t e = path.find('/', at);
        if (e == std::string::npos) e = path.size();
        const std::string part = path.substr(at, e - at);
        at = e + 1;
        if (part.empty() || part == "root") continue;
        int next = -1;
        for (int c : sc.children(cur))
            if (sc.is_layer(c) && sc.name(c) == part) { next = c; break; }
        if (next < 0) return -1;
        cur = next;
    }
    return cur;
}

// The elements of `elems` that are not below another element of it, in document order.
static std::vector<int> top_level(const TedScene& sc, std::vector<int> elems) {
    std::sort(elems.begin(), elems.end());
    elems.erase(std::unique(elems.begin(), elems.end()), elems.end());
    std::vector<int> top;
    for (int e : elems) {
        bool nested = false;
        for (int p = sc.doc().elements()[size_t(e)].parent; p >= 0 && !nested; p = sc.doc().elements()[size_t(p)].parent) nested = std::binary_search(elems.begin(), elems.end(), p);
        if (!nested) top.push_back(e);
    }
    return top;
}

bool TileDocument::move_items(const std::vector<int>& elems_in, int layer, int before, std::string* error) {
    TedScene& sc = tscene();
    if (elems_in.empty() || !sc.is_layer(layer)) { if (error) *error = "nothing to move, or the target is not a layer"; return false; }
    for (int e : elems_in)
        if (e < 0 || e >= int(sc.doc().elements().size()) || sc.type(e) == "SCENE_GRAPH") { if (error) *error = "cannot move this element"; return false; }
    const std::vector<int> top = top_level(sc, elems_in);
    std::vector<WorldTransform> worlds;
    std::vector<uint64_t> sel_before;
    for (int e : top) {
        worlds.push_back(world_transform(sc, e));
        sel_before.push_back(item_of(e));
    }
    const size_t mark = sc.begin_edit();
    std::vector<int> moved;
    try {
        moved = sc.move_elements(top, layer, before);
    } catch (const std::exception& ex) {
        if (error) *error = ex.what();
        sc.unapply(sc.end_edit(mark));
        return false;
    }
    if (moved.empty()) { sc.end_edit(mark); return false; }
    // every moved element stays where it was in the world (its parents changed)
    std::vector<uint64_t> sel_after;
    for (size_t i = 0; i < moved.size(); ++i) {
        if (sc.position(moved[i])) set_world_position(sc, moved[i], worlds[i].position);
        if (sc.euler(moved[i])) set_world_rotation(sc, moved[i], worlds[i].rotation);
        sel_after.push_back(item_of(moved[i]));
    }
    push_splice(moved.size() == 1 ? "Move to layer" : "Move " + std::to_string(moved.size()) + " items to layer", mark, std::move(sel_before), std::move(sel_after));
    return true;
}

int TileDocument::group_items(const std::vector<int>& elems_in, const std::string& name, std::string* error) {
    TedScene& sc = tscene();
    if (elems_in.empty()) { if (error) *error = "nothing selected"; return -1; }
    for (int e : elems_in)
        if (e < 0 || e >= int(sc.doc().elements().size()) || sc.type(e) == "SCENE_GRAPH") { if (error) *error = "cannot group this element"; return -1; }
    const std::vector<int> top = top_level(sc, elems_in);
    int parent = -1;
    for (int p = sc.doc().elements()[size_t(top[0])].parent; p >= 0; p = sc.doc().elements()[size_t(p)].parent)
        if (sc.is_layer(p)) { parent = p; break; }
    if (parent < 0) { if (error) *error = "the selection is not inside a layer"; return -1; }
    for (int e : top)
        if (e == parent || sc.is_descendant(parent, e)) { if (error) *error = "cannot group a layer into a layer inside itself"; return -1; }
    std::vector<WorldTransform> worlds;
    std::vector<uint64_t> sel_before;
    for (int e : top) {
        worlds.push_back(world_transform(sc, e));
        sel_before.push_back(item_of(e));
    }
    const size_t mark = sc.begin_edit();
    std::vector<int> moved;
    try {
        const int layer = sc.add_layer(parent, name.empty() ? "New layer" : name);   // appended to the parent: the elements after it shift by its size
        const int shift = sc.doc().subtree_size(layer);
        std::vector<int> shifted;
        for (int e : top) shifted.push_back(e >= layer ? e + shift : e);
        moved = sc.move_elements(shifted, layer, -1);
    } catch (const std::exception& ex) {
        if (error) *error = ex.what();
        sc.unapply(sc.end_edit(mark));
        return -1;
    }
    for (size_t i = 0; i < moved.size(); ++i) {
        if (sc.position(moved[i])) set_world_position(sc, moved[i], worlds[i].position);
        if (sc.euler(moved[i])) set_world_rotation(sc, moved[i], worlds[i].rotation);
    }
    // moved[0] sits in <children> of the new layer
    const int new_layer = sc.doc().elements()[size_t(sc.doc().elements()[size_t(moved[0])].parent)].parent;
    push_splice("Group into layer", mark, std::move(sel_before), {item_of(new_layer)});
    return new_layer;
}

bool TileDocument::remove_items(const std::vector<int>& elems_in) {
    TedScene& sc = tscene();
    std::vector<int> elems;
    for (int e : elems_in)
        if (e >= 0 && e < int(sc.doc().elements().size()) && sc.type(e) != "SCENE_GRAPH") elems.push_back(e);
    std::sort(elems.begin(), elems.end());
    elems.erase(std::unique(elems.begin(), elems.end()), elems.end());
    std::vector<int> top;
    for (int e : elems) {
        bool nested = false;
        for (int p = sc.doc().elements()[size_t(e)].parent; p >= 0 && !nested; p = sc.doc().elements()[size_t(p)].parent) nested = std::binary_search(elems.begin(), elems.end(), p);
        if (!nested) top.push_back(e);
    }
    if (top.empty()) return false;
    std::vector<uint64_t> sel_before;
    for (int e : top) sel_before.push_back(item_of(e));
    const size_t mark = sc.begin_edit();
    for (size_t i = top.size(); i-- > 0;) sc.remove(top[i]);   // from the last one: the indices of the others stay
    push_splice(top.size() == 1 ? "Delete " + sc.type(top[0]) : "Delete " + std::to_string(top.size()) + " items", mark, std::move(sel_before), {});
    return true;
}

int TileDocument::duplicate_layer(int layer, std::string* error) {
    if (!tscene().is_layer(layer) || tscene().type(layer) == "SCENE_GRAPH") { if (error) *error = "not a layer"; return -1; }
    const size_t mark = tscene().begin_edit();
    const int copy = tscene().duplicate(layer);
    if (copy < 0) { if (error) *error = "cannot duplicate this layer"; return -1; }
    tscene().set_attr(copy, "name", tscene().name(layer) + " copy");
    push_splice("Duplicate layer", mark, {item_of(layer)}, {item_of(copy)});
    return copy;
}

// ---- height strokes

void TileDocument::apply_brush(BrushKind kind, float x, float z, const BrushSettings& s, float dt) {
    HeightField& hf = *project_->height;
    if (!stroke_open_) {
        hf.begin_edit();
        stroke_open_ = true;
    }
    Brush b;
    b.cx = project_->pixel_x(x);
    b.cy = project_->pixel_z(z);
    b.radius = std::max(1.0, s.radius_m / project_->mpp_x());
    b.falloff = s.falloff;
    dt = std::min(0.05f, dt);
    switch (kind) {
    case BrushKind::Raise: b.strength = 1; hf.raise(b, s.strength * dt); break;
    case BrushKind::Lower: b.strength = 1; hf.raise(b, -s.strength * dt); break;
    case BrushKind::Smooth: b.strength = std::min(1.0, s.strength * dt * 0.5); hf.smooth(b); break;
    case BrushKind::Flatten: b.strength = std::min(1.0, s.strength * dt * 0.5); hf.flatten(b, s.flatten_height); break;
    case BrushKind::Level: b.strength = std::min(1.0, s.strength * dt * 0.5); hf.set_level_under(b); break;
    }
    project_->height_dirty = true;
}

void TileDocument::brush_step(BrushKind kind, float x, float z, const BrushSettings& s, float dt) {
    if (!project_->has_height) return;
    if (!stroke_open_) ++stroke_id_;
    history_.push(std::make_unique<BrushStepCommand>(*this, kind, x, z, s, dt, stroke_id_));
}

void TileDocument::end_stroke() {
    if (!stroke_open_) return;
    project_->height->end_edit();
    stroke_open_ = false;
    history_.break_merge();
}

// ---- save

bool TileDocument::height_modified() const { return project_->has_height && project_->height->edit_depth() != saved_height_depth_; }
bool TileDocument::save_scene_needed() const { return tscene().modified() || ted_on_disk_edited_; }

bool TileDocument::save(const SaveTarget& target, std::string* message) {
    end_stroke();
    const bool elsewhere = !target.path.empty();
    fs::path out = elsewhere ? target.path : project_->dir;
    if (elsewhere && out.has_extension()) out = out.parent_path();   // "Save as..." hands over a file name: the tile goes into its folder
    project_->height_dirty = height_modified() || png_on_disk_edited_ || (elsewhere && project_->has_height);
    std::string msg;
    const bool wrote = project_->save(out, &msg, save_scene_needed() || elsewhere, false);
    if (!wrote && msg != "nothing to save") {
        if (message) *message = msg;
        return false;
    }
    if (!elsewhere) {
        ted_on_disk_edited_ = tscene().modified();
        png_on_disk_edited_ = height_modified();
        saved_height_depth_ = project_->has_height ? project_->height->edit_depth() : 0;
        project_->height_dirty = false;
        history_.mark_saved();
    }
    if (message) *message = wrote ? msg + "(" + narrow(out.wstring()) + ")" : msg;
    return true;
}

std::string TileDocument::diagnostics() const {
    std::string d = "tile " + title() + "\n";
    d += "instances " + std::to_string(instances_.size()) + ", markers " + std::to_string(markers_.elem.size()) + ", building models " + std::to_string(models_.size() - fallbacks_.size()) + " of " +
         std::to_string(models_.size()) + " keys\n";
    d += "terrain " + std::to_string(project_->has_height ? int(project_->height->width()) : 0) + " x " + std::to_string(project_->has_height ? int(project_->height->height()) : 0) +
         " px, stride " + std::to_string(terrain_stride_) + ", border " + std::to_string(int(project_->border_m())) + " m\n";
    d += load_info + "\n";
    if (scene_)
        for (size_t t = 0; t < batch_entity_.size(); ++t) {
            const Entity* e = batch_entity_[t] ? scene_->find(batch_entity_[t]) : nullptr;
            const Overlay* o = e ? e->get<Overlay>() : nullptr;
            if (o) d += "markers of " + idx_.type_names[t] + ": " + std::to_string(markers_.group_begin[t + 1] - markers_.group_begin[t]) + ", drawn as " + std::to_string(o->lines.size()) + " line vertices\n";
        }
    for (const std::string& k : fallbacks_) d += "no model: " + k + "\n";
    return d;
}

} // namespace wce
