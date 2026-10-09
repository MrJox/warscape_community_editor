#include "scene/render_scene.h"

#include <algorithm>

namespace wce {

Terrain Terrain::flat(float half_size, const std::string& diffuse_texture, float tile) {
    Terrain t;
    t.nx = t.nz = 2;
    t.origin_x = t.origin_z = -half_size;
    t.step = half_size * 2;
    t.diffuse = diffuse_texture;
    t.tile_metres = tile;
    return t;
}

void Overlay::box_wire(const float3& lo, const float3& hi, uint32_t rgba) { box_wire(float4x4{}, lo, hi, rgba); }

void Overlay::box_wire(const float4x4& m, const float3& lo, const float3& hi, uint32_t rgba) {
    const float3 c[8] = {m.transform_point({lo.x, lo.y, lo.z}), m.transform_point({hi.x, lo.y, lo.z}), m.transform_point({hi.x, lo.y, hi.z}),
                         m.transform_point({lo.x, lo.y, hi.z}), m.transform_point({lo.x, hi.y, lo.z}), m.transform_point({hi.x, hi.y, lo.z}),
                         m.transform_point({hi.x, hi.y, hi.z}), m.transform_point({lo.x, hi.y, hi.z})};
    for (int i = 0; i < 4; ++i) {
        line(c[i], c[(i + 1) % 4], rgba);          // bottom ring
        line(c[4 + i], c[4 + (i + 1) % 4], rgba);  // top ring
        line(c[i], c[4 + i], rgba);                // verticals
    }
}

void Overlay::polygon_outline(const std::vector<float3>& points, bool closed, uint32_t rgba) {
    for (size_t i = 0; i + 1 < points.size(); ++i) line(points[i], points[i + 1], rgba);
    if (closed && points.size() > 2) line(points.back(), points.front(), rgba);
}

Kind Entity::kind() const {
    static const Kind kinds[] = {Kind::MeshInstance, Kind::Terrain, Kind::VfxEmitter, Kind::Overlay, Kind::CameraRig};   // variant order
    return kinds[payload.index()];
}

EnvironmentParams EnvironmentSlot::resolved() const {
    EnvironmentParams a = active ? *active : EnvironmentParams{};
    if (active && blend && blend_t > 0) return blend_environments(a, *blend, blend_t);
    return a;
}

LayerId RenderScene::add_layer(DocumentId doc, std::string name) {
    Layer l;
    l.id = next_layer_++;
    l.doc = doc;
    l.name = std::move(name);
    l.order = int(layers_.size());
    layers_.push_back(std::move(l));
    touch();
    return layers_.back().id;
}

void RenderScene::remove_layer(LayerId id) {
    clear_layer(id);
    layers_.erase(std::remove_if(layers_.begin(), layers_.end(), [&](const Layer& l) { return l.id == id; }), layers_.end());
    touch();
}

Layer* RenderScene::layer(LayerId id) {
    for (Layer& l : layers_)
        if (l.id == id) return &l;
    return nullptr;
}
const Layer* RenderScene::layer(LayerId id) const { return const_cast<RenderScene*>(this)->layer(id); }

void RenderScene::set_layer_visible(LayerId id, bool visible) {
    if (Layer* l = layer(id); l && l->visible != visible) {
        l->visible = visible;
        touch();
    }
}

EntityId RenderScene::add(LayerId layer_id, const float4x4& world, Payload payload, SourceRef source) {
    Entity e;
    e.id = next_entity_++;
    e.layer = layer_id;
    e.world = world;
    e.source = source;
    e.payload = std::move(payload);
    index_[e.id] = entities_.size();
    entities_.push_back(std::move(e));
    touch();
    return entities_.back().id;
}

Entity* RenderScene::find(EntityId id) {
    auto it = index_.find(id);
    return it == index_.end() ? nullptr : &entities_[it->second];
}
const Entity* RenderScene::find(EntityId id) const { return const_cast<RenderScene*>(this)->find(id); }

void RenderScene::update(EntityId id, const float4x4& world) {
    if (Entity* e = find(id)) {
        e->world = world;
        touch();
    }
}
void RenderScene::update(EntityId id, Payload payload) {
    if (Entity* e = find(id)) {
        e->payload = std::move(payload);
        touch();
    }
}

void RenderScene::reindex() {
    index_.clear();
    for (size_t i = 0; i < entities_.size(); ++i) index_[entities_[i].id] = i;
}

void RenderScene::remove(EntityId id) {
    auto it = index_.find(id);
    if (it == index_.end()) return;
    entities_.erase(entities_.begin() + std::ptrdiff_t(it->second));
    reindex();
    touch();
}

void RenderScene::clear_layer(LayerId id) {
    const size_t before = entities_.size();
    entities_.erase(std::remove_if(entities_.begin(), entities_.end(), [&](const Entity& e) { return e.layer == id; }), entities_.end());
    if (entities_.size() != before) {
        reindex();
        touch();
    }
}

void RenderScene::clear() {
    entities_.clear();
    index_.clear();
    layers_.clear();
    touch();
}

bool RenderScene::drawable(const Entity& e) const {
    const Layer* l = layer(e.layer);
    return e.visible && l && l->visible;
}

bool RenderScene::pickable(const Entity& e) const {
    const Layer* l = layer(e.layer);
    return e.visible && l && l->visible && !l->locked;
}

} // namespace wce
