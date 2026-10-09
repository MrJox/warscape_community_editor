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
#include <functional>

namespace fs = std::filesystem;

namespace wce {

namespace {

constexpr double kPi = 3.14159265358979323846;

struct TypeStyle {
    uint32_t color;
    float size;
    bool cross;       // many small instances: three lines instead of a box
};

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

// A structural edit changes element indices, so it keeps whole-document snapshots (the scene already holds `after` when the command is pushed).
class TedSnapshotCommand : public Command {
public:
    TedSnapshotCommand(TileDocument& d, std::string name, TedScene::Snapshot before, TedScene::Snapshot after, std::vector<uint64_t> sel_before, std::vector<uint64_t> sel_after)
        : doc_(d), name_(std::move(name)), before_(std::move(before)), after_(std::move(after)), sel_before_(std::move(sel_before)), sel_after_(std::move(sel_after)) {}
    std::string name() const override { return name_; }
    void apply() override {
        if (first_) first_ = false;
        else doc_.tscene().restore(after_);
        ++doc_.structure_version_;
        doc_.hint_ = sel_after_;
    }
    void revert() override {
        doc_.tscene().restore(before_);
        ++doc_.structure_version_;
        doc_.hint_ = sel_before_;
    }

private:
    TileDocument& doc_;
    std::string name_;
    TedScene::Snapshot before_, after_;
    std::vector<uint64_t> sel_before_, sel_after_;
    bool first_ = true;
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
    rebuild_items();
    const double t_items = since(t0);
    resolve_buildings();
    const double t_models = since(t0) - t_items;
    rebuild_items();   // markers skip the buildings that have a model
    if (project_->has_height) saved_height_depth_ = project_->height->edit_depth();
    char b[200];
    std::snprintf(b, sizeof b, "tile loaded: %zu instances in %.2f s, building models %.2f s, markers %.2f s", instances_.size(), t_items, t_models, since(t0) - t_items - t_models);
    load_info = b;
    Log::info(load_info);
}
TileDocument::~TileDocument() = default;

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

bool TileDocument::is_polygon(int elem) const { return elem >= 0 && elem < int(tscene().doc().elements().size()) && !tscene().polygon_points(elem).empty(); }

bool TileDocument::is_instance(int elem) const {
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
    const WorldTransform w = world_transform(tscene(), elem);
    return {float(w.position[0] / unit()), float(w.position[1]), float(w.position[2] / unit())};
}

float4x4 TileDocument::world_matrix(int elem) const { return matrix_of(world_transform(tscene(), elem), unit(), tscene().scale(elem)); }

const BuildingModel* TileDocument::building_model(const std::string& key) const {
    auto it = models_.find(key);
    return it == models_.end() ? nullptr : &it->second;
}

float TileDocument::marker_radius(int elem) const {
    const std::string type = tscene().type(elem);
    if (type == "BUILDING_INSTANCE") {
        if (const BuildingModel* m = building_model(tscene().doc().get(elem, "key").value_or("")); m && m->ok) {
            const float3 e = m->bmax - m->bmin;
            return std::clamp(0.5f * std::max({e.x, e.y, e.z}), 2.0f, 80.0f);
        }
    }
    return style_of(type).size * 0.75f;
}

// ---- scene -> items

void TileDocument::resolve_buildings() {
    models_.clear();
    fallbacks_.clear();
    std::set<std::string> keys;
    for (int e : tscene().find_all("BUILDING_INSTANCE"))
        if (auto k = tscene().doc().get(e, "key"); k && !k->empty()) keys.insert(*k);
    if (keys.empty()) return;
    std::map<std::string, std::vector<std::string>> folders;
    if (vfs_) {
        const std::string prefix = "rigidmodels/buildings/";
        for (const std::string& p : vfs_->list(prefix, ".rigid_model_v2")) {
            const size_t slash = p.rfind('/');
            const std::string folder = p.substr(prefix.size(), slash - prefix.size());
            if (keys.count(folder) == 0) continue;
            const std::string file = p.substr(slash + 1);
            if (file.find("destruct") != std::string::npos && file.find("destruct01") == std::string::npos) continue;
            folders[folder].push_back(p);
        }
    }
    std::vector<std::string> wanted;
    for (auto& [k, v] : folders) wanted.insert(wanted.end(), v.begin(), v.end());
    if (vfs_ && !wanted.empty()) vfs_->prefetch(wanted);
    std::vector<std::string> textures;
    for (const std::string& key : keys) {
        BuildingModel bm;
        bm.bmin = {1e30f, 1e30f, 1e30f};
        bm.bmax = {-1e30f, -1e30f, -1e30f};
        for (const std::string& p : folders[key]) {
            float3 lo{1e30f, 1e30f, 1e30f}, hi{-1e30f, -1e30f, -1e30f};
            if (!piece_bounds(*vfs_, p, lo, hi, textures)) continue;
            bm.bmin = {std::min(bm.bmin.x, lo.x), std::min(bm.bmin.y, lo.y), std::min(bm.bmin.z, lo.z)};
            bm.bmax = {std::max(bm.bmax.x, hi.x), std::max(bm.bmax.y, hi.y), std::max(bm.bmax.z, hi.z)};
            bm.pieces.push_back(p);
        }
        bm.ok = !bm.pieces.empty();
        if (!bm.ok) fallbacks_.push_back(key);
        models_[key] = std::move(bm);
    }
    if (vfs_ && !textures.empty()) vfs_->prefetch(textures);
    if (!fallbacks_.empty()) {
        std::string list;
        for (size_t i = 0; i < fallbacks_.size() && i < 8; ++i) list += (i ? ", " : "") + fallbacks_[i];
        if (fallbacks_.size() > 8) list += ", ...";
        Log::warn("tile " + title() + ": " + std::to_string(fallbacks_.size()) + " of " + std::to_string(keys.size()) + " building keys have no drawable model and show as boxes: " + list);
    }
}

void TileDocument::rebuild_items() {
    instances_.clear();
    markers_.clear();
    marker_of_.clear();
    type_counts_.clear();
    const TedScene& sc = tscene();
    const auto& els = sc.doc().elements();
    for (size_t i = 0; i < els.size(); ++i) {
        const int e = int(i);
        if (!is_instance(e)) continue;
        const std::string& t = els[i].name;
        ++type_counts_[t];
        instances_.push_back(e);
        if (t == "BUILDING_INSTANCE") {
            const BuildingModel* m = building_model(sc.doc().get(e, "key").value_or(""));
            if (m && m->ok) continue;   // drawn as a model
        }
        const TypeStyle st = style_of(t);
        Marker mk;
        mk.elem = e;
        mk.pos = world_position(e);
        mk.size = st.size;
        mk.color = st.color;
        mk.cross = st.cross;
        mk.type = t;
        marker_of_[e] = markers_.size();
        markers_.push_back(std::move(mk));
    }
    markers_dirty_ = true;
    polygons_dirty_ = true;
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
        terrain_stride_ = std::max(1, int(std::ceil(std::max(hf.width(), hf.height()) / 512.0)));
        t.nx = std::max(2, int((hf.width() - 1) / terrain_stride_) + 1);
        t.nz = std::max(2, int((hf.height() - 1) / terrain_stride_) + 1);
        const float k = float(p.mpp_z() / p.mpp_x());
        t.step = float(p.mpp_x() * terrain_stride_);
        t.origin_x = float(-p.border_m());
        t.origin_z = float(-p.border_m()) / k;
        t.heights.resize(size_t(t.nx) * t.nz);
        for (int j = 0; j < t.nz; ++j)
            for (int i = 0; i < t.nx; ++i) t.heights[size_t(j) * t.nx + i] = float(hf.metres(uint32_t(std::min<int>(i * terrain_stride_, int(hf.width()) - 1)), uint32_t(std::min<int>(j * terrain_stride_, int(hf.height()) - 1))));
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
    const std::string type = tscene().type(elem);
    if (type == "BUILDING_INSTANCE") {
        const BuildingModel* m = building_model(tscene().doc().get(elem, "key").value_or(""));
        if (m && m->ok) {
            const float4x4 w = world_matrix(elem);
            auto& ids = item_entities_[elem];
            for (const std::string& piece : m->pieces) {
                MeshInstance mi;
                mi.model = piece;
                ids.push_back(scene.add(layer, w, std::move(mi), SourceRef{id(), item_of(elem)}));
            }
        }
    }
    if (display_.show_polygons && is_polygon(elem)) rebuild_polygon(scene, layer, elem);
}

void TileDocument::rebuild_polygon(RenderScene& scene, LayerId layer, int elem) {
    Overlay o;
    o.pickable = false;
    const TypeStyle st = style_of(tscene().type(elem));
    const std::vector<float3> pts = polygon_world_points(elem);
    const bool closed = tscene().doc().get(elem, "polygon_closed").value_or("1") != "0";
    std::vector<float3> lifted;
    for (const float3& p : pts) lifted.push_back({p.x, p.y + 0.3f, p.z});
    o.polygon_outline(lifted, closed, st.color);
    auto& ids = item_entities_[elem];
    // the outline entity is the last one of the item (after any model pieces)
    const bool has_outline = !ids.empty() && scene.find(ids.back()) && scene.find(ids.back())->get<Overlay>();
    if (has_outline) scene.update(ids.back(), Payload{std::move(o)});
    else ids.push_back(scene.add(layer, float4x4{}, std::move(o), SourceRef{id(), item_of(elem)}));
}

void TileDocument::rebuild_batch(RenderScene& scene, const std::string& type) {
    auto it = batch_entity_.find(type);
    if (it == batch_entity_.end()) return;
    Overlay o;
    o.pickable = false;
    if (display_.hidden_types.count(type) == 0) {
        const float lim = lod_distance(type);
        for (const Marker& m : markers_) {
            if (m.type != type) continue;
            if (display_.lod && lod_eye_.x < 1e29f) {
                const float3 d = m.pos - lod_eye_;
                if (dot(d, d) > lim * lim) continue;
            }
            const float h = m.size * 0.5f;
            if (m.cross) {
                o.line({m.pos.x - h, m.pos.y + 0.4f, m.pos.z}, {m.pos.x + h, m.pos.y + 0.4f, m.pos.z}, m.color);
                o.line({m.pos.x, m.pos.y + 0.4f, m.pos.z - h}, {m.pos.x, m.pos.y + 0.4f, m.pos.z + h}, m.color);
                o.line({m.pos.x, m.pos.y, m.pos.z}, {m.pos.x, m.pos.y + m.size, m.pos.z}, m.color);
            } else {
                o.box_wire({m.pos.x - h, m.pos.y - h, m.pos.z - h}, {m.pos.x + h, m.pos.y + h, m.pos.z + h}, m.color);
            }
        }
    }
    scene.update(it->second, Payload{std::move(o)});
}

void TileDocument::publish(RenderScene& scene, LayerId layer) {
    scene.clear_layer(layer);
    scene_ = &scene;
    layer_ = layer;
    item_entities_.clear();
    batch_entity_.clear();
    dirty_types_.clear();
    have_highlight_ = false;
    update_terrain(scene, true);
    for (int e : instances_) add_item_entities(scene, layer, e);
    std::set<std::string> types;
    for (const Marker& m : markers_) types.insert(m.type);
    for (const std::string& t : types) {
        Overlay empty;
        empty.pickable = false;
        batch_entity_[t] = scene.add(layer, float4x4{}, std::move(empty), SourceRef{id(), kTerrain});
    }
    Overlay h;
    h.depth_test = false;
    h.pickable = false;
    overlay_entity_ = scene.add(layer, float4x4{}, std::move(h), SourceRef{id(), kTerrain});
    markers_dirty_ = true;
    lod_eye_ = {1e30f, 1e30f, 1e30f};
}

void TileDocument::refresh_item(RenderScene& scene, int elem) {
    if (!is_instance(elem)) return;
    const std::string type = tscene().type(elem);
    if (auto it = item_entities_.find(elem); it != item_entities_.end()) {
        const float4x4 w = world_matrix(elem);
        for (EntityId e : it->second)
            if (const Entity* ent = scene.find(e); ent && ent->get<MeshInstance>()) scene.update(e, w);
    }
    if (auto it = marker_of_.find(elem); it != marker_of_.end()) {
        markers_[it->second].pos = world_position(elem);
        dirty_types_.insert(type);
    }
    if (is_polygon(elem) && display_.show_polygons) rebuild_polygon(scene, layer_, elem);
}

void TileDocument::republish(RenderScene& scene, LayerId layer, const CommandStack::Dirty& dirty) {
    if (dirty.all || scene_ != &scene) {
        rebuild_items();
        publish(scene, layer);
        return;
    }
    for (uint64_t item : dirty.items) {
        if (item == kTerrain) update_terrain(scene, false);
        else refresh_item(scene, elem_of(item));
    }
}

void TileDocument::update_view(RenderScene& scene, LayerId layer, const float3& eye) {
    if (scene_ != &scene) return;
    (void)layer;
    bool all = markers_dirty_;
    if (!all && display_.lod) {
        const float3 d = eye - lod_eye_;
        if (lod_eye_.x > 1e29f || dot(d, d) > 20.0f * 20.0f) all = true;
    }
    if (all) {
        lod_eye_ = eye;
        for (auto& [t, e] : batch_entity_) rebuild_batch(scene, t);
        for (auto& [elem, ids] : item_entities_)   // the model pieces of buildings follow the type visibility
            for (EntityId eid : ids)
                if (Entity* en = scene.find(eid); en && en->get<MeshInstance>()) en->visible = display_.hidden_types.count(tscene().type(elem)) == 0;
        scene.touch();
        markers_dirty_ = false;
        dirty_types_.clear();
    }
    for (const std::string& t : dirty_types_) rebuild_batch(scene, t);
    dirty_types_.clear();
    if (polygons_dirty_) {
        polygons_dirty_ = false;
        for (int e : instances_) {
            if (!is_polygon(e)) continue;
            auto& ids = item_entities_[e];
            if (display_.show_polygons) rebuild_polygon(scene, layer_, e);
            else if (!ids.empty()) {
                // hide: an empty outline
                Overlay o;
                o.pickable = false;
                scene.update(ids.back(), Payload{std::move(o)});
            }
        }
    }
}

void TileDocument::set_highlight(RenderScene& scene, const std::vector<int>& selected, int polygon_point, float handle_size, const Overlay& tool_overlay) {
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
    const uint64_t base = (uint64_t(id()) << 40) ^ (uint64_t(e) << 8) ^ 0x31;
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
    if (!is_instance(e)) return false;
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

int TileDocument::pick_instance(const float3& o, const float3& d, bool visible_only) const {
    int best = -1;
    float best_t = 1e30f;
    for (int e : instances_) {
        const std::string& type = tscene().doc().elements()[e].name;
        if (visible_only && display_.hidden_types.count(type)) continue;
        float3 c = world_position(e);
        const float r = marker_radius(e);
        if (type == "BUILDING_INSTANCE") c.y += r * 0.5f;
        if (visible_only && display_.lod && type != "BUILDING_INSTANCE") {
            const float lim = lod_distance(type);
            const float3 dd = c - o;
            if (dot(dd, dd) > lim * lim) continue;
        }
        const float3 oc = o - c;
        const float b = dot(oc, d), cc = dot(oc, oc) - r * r;
        const float disc = b * b - cc;
        if (disc < 0) continue;
        const float t = -b - std::sqrt(disc);
        if (t > 0 && t < best_t) { best_t = t; best = e; }
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

void TileDocument::push_snapshot(const std::string& name, const TedScene::Snapshot& before, std::vector<uint64_t> sel_before, std::vector<uint64_t> sel_after) {
    history_.push(std::make_unique<TedSnapshotCommand>(*this, name, before, snap(), std::move(sel_before), std::move(sel_after)));
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
    if (!is_instance(elem) && !tscene().is_layer(elem)) { if (error) *error = "nothing to duplicate"; return -1; }
    const auto before = snap();
    const int copy = tscene().duplicate(elem);
    if (copy < 0) { if (error) *error = "cannot duplicate this element"; return -1; }
    if (is_instance(copy)) {
        const WorldTransform w = world_transform(tscene(), copy);
        set_world_position(tscene(), copy, {w.position[0] + 4.0 * unit(), w.position[1], w.position[2]});
    }
    push_snapshot("Duplicate " + tscene().type(copy), before, {item_of(elem)}, {item_of(copy)});
    return copy;
}

bool TileDocument::remove(int elem) {
    if (elem < 0 || elem >= int(tscene().doc().elements().size())) return false;
    const auto before = snap();
    const std::string label = tscene().type(elem);
    tscene().remove(elem);
    push_snapshot("Delete " + label, before, {item_of(elem)}, {});
    return true;
}

int TileDocument::add_building(const std::string& key, int template_elem, const float3* at, std::string* error) {
    if (key.empty()) return -1;
    int tmpl = -1;
    if (template_elem >= 0 && tscene().type(template_elem) == "BUILDING_INSTANCE") tmpl = template_elem;
    else if (auto all = tscene().find_all("BUILDING_INSTANCE"); !all.empty()) tmpl = all.front();
    if (tmpl < 0) { if (error) *error = "no BUILDING_INSTANCE to use as a template in this tile"; return -1; }
    const auto before = snap();
    const int copy = tscene().duplicate(tmpl);
    tscene().set_attr(copy, "key", key);
    tscene().set_attr(copy, "name", key);
    if (at) set_world_position(tscene(), copy, {double(at->x) * unit(), double(at->y), double(at->z) * unit()});
    push_snapshot("Add " + key, before, {template_elem >= 0 ? item_of(template_elem) : 0}, {item_of(copy)});
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
    const auto before = snap();
    const int e = tscene().add_polygon(layer, type, type == "AI_HINT_POLYGON_INSTANCE" ? "AI hint polygon" : "Polygon", local, extra);
    push_snapshot("Add " + type, before, {}, {item_of(e)});
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
    const auto before = snap();
    if (op == "insert") {
        if (index < 0 || size_t(index) >= n) { if (error) *error = "no such point"; return false; }
        sc.insert_polygon_point(elem, size_t(index), to_local(elem, world));
        push_snapshot("Insert polygon point", before, {item_of(elem)}, {item_of(elem)});
        return true;
    }
    if (op == "remove") {
        if (index < 0 || !sc.remove_polygon_point(elem, size_t(index))) { if (error) *error = "a polygon needs at least 2 points"; return false; }
        push_snapshot("Remove polygon point", before, {item_of(elem)}, {item_of(elem)});
        return true;
    }
    if (error) *error = "unknown point operation '" + op + "'";
    return false;
}

int TileDocument::add_layer(int parent, const std::string& name, std::string* error) {
    if (parent < 0) { if (error) *error = "this tile has no SCENE_GRAPH, so layers cannot be added"; return -1; }
    const auto before = snap();
    const int layer = tscene().add_layer(parent, name.empty() ? "New layer" : name);
    push_snapshot("Add layer", before, {}, {item_of(layer)});
    return layer;
}

bool TileDocument::move_to_layer(int elem, int layer, std::string* error) {
    TedScene& sc = tscene();
    if (elem < 0 || elem == layer) return false;
    if (sc.is_descendant(layer, elem)) { if (error) *error = "cannot move a layer into itself"; return false; }
    const auto before = snap();
    const WorldTransform w = world_transform(sc, elem);
    const bool has_pos = sc.position(elem).has_value();
    const int moved = sc.move_to_layer(elem, layer);
    if (has_pos) set_world_position(sc, moved, w.position);   // keep the world position and orientation
    if (sc.euler(moved)) set_world_rotation(sc, moved, w.rotation);
    push_snapshot("Move to layer", before, {item_of(elem)}, {item_of(moved)});
    return true;
}

bool TileDocument::rename(int elem, const std::string& name) { return set_attribute(elem, "name", name); }

bool TileDocument::set_attribute(int elem, const std::string& attr, const std::string& value) {
    if (elem < 0 || elem >= int(tscene().doc().elements().size())) return false;
    if (tscene().doc().get(elem, attr).value_or("\x01") == value) return true;
    const auto before = snap();
    tscene().set_attr(elem, attr.c_str(), value);
    push_snapshot("Set " + attr, before, {item_of(elem)}, {item_of(elem)});
    return true;
}

bool TileDocument::remove_layer(int layer) {
    if (!tscene().is_layer(layer) || tscene().type(layer) == "SCENE_GRAPH") return false;
    return remove(layer);
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
    d += "instances " + std::to_string(instances_.size()) + ", markers " + std::to_string(markers_.size()) + ", building models " + std::to_string(models_.size() - fallbacks_.size()) + " of " +
         std::to_string(models_.size()) + " keys\n";
    d += "terrain " + std::to_string(project_->has_height ? int(project_->height->width()) : 0) + " x " + std::to_string(project_->has_height ? int(project_->height->height()) : 0) +
         " px, stride " + std::to_string(terrain_stride_) + ", border " + std::to_string(int(project_->border_m())) + " m\n";
    d += load_info + "\n";
    for (const std::string& k : fallbacks_) d += "no model: " + k + "\n";
    return d;
}

} // namespace wce
