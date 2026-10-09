#pragma once
// RenderScene: the shared render/editor scene. A flat set of entities with stable handles; documents own the truth and publish
// what they show into it (one layer per open document), the renderer draws it, picking and selection map back to the document
// through SourceRef. It is not serialized and holds no GPU objects: models and textures are named by VFS path and resolved by the
// renderer's caches.
//
// Draw order is insertion order. `revision()` changes whenever entities, layers or visibility change, so the renderer rebuilds its
// draw lists only then.
#include "foundation/math.h"
#include "scene/environment.h"

#include <cstdint>
#include <optional>
#include <string>
#include <unordered_map>
#include <variant>
#include <vector>

namespace wce {

using DocumentId = uint32_t;
using LayerId = uint32_t;
using EntityId = uint32_t;   // 0 is "none"

// Back-reference from an entity to the element of a document it shows (picking -> document element).
struct SourceRef {
    DocumentId doc = 0;
    uint64_t item = 0;
};

enum class Kind {
    MeshInstance,      // rigid model, drawn through the game's rigid shaders
    SkinnedInstance,   // declared for the Unit editor (bone palette, weighted vertex formats); not built yet
    Terrain,           // regular height grid
    VfxEmitter,        // particle emitter (the weather precipitation stays with the environment for now)
    Overlay,           // lines and triangles: markers, polygons, boxes, grid, gizmos
    CameraRig,         // declared for the Cinematic editor
};

struct MeshInstance {
    std::string model;               // VFS path of the .rigid_model_v2
    bool casts_shadow = true;
    // Per-instance pixel shader inputs of rigidcommon (POSITION4..7, docs/re/renderer.md); zero = the engine's default.
    float p4[4] = {0, 0, 0, 0}, p5[4] = {0, 0, 0, 0}, p6[4] = {0, 0, 0, 0}, p7[4] = {0, 0, 0, 0};
};

// A regular grid of heights: vertex (i, j) is at x = origin_x + i * step, z = origin_z + j * step, y = heights[j * nx + i]
// (before the entity's world transform). The interim material tiles one diffuse texture every `tile_metres` and shades with the
// game's rigid G-buffer shader (docs/re/terrain.md has what the game's own terrain shaders do).
struct Terrain {
    int nx = 2, nz = 2;
    float origin_x = -1, origin_z = -1, step = 2;
    std::vector<float> heights;      // nx * nz, empty = flat at 0
    std::string diffuse = "terrain/textures/battle/default/grass/0_su_diffuse.dds";
    float tile_metres = 8;
    bool casts_shadow = false;       // receiver only
    uint64_t version = 0;            // change it when heights or the texture change: the renderer rebuilds the mesh

    static Terrain flat(float half_size, const std::string& diffuse, float tile_metres);
    float height_at(int i, int j) const { return heights.empty() ? 0.0f : heights[size_t(j) * nx + i]; }
};

struct VfxEmitter {
    std::string effect;              // vfx definition name; not drawn yet
};

struct OverlayVertex {
    float3 p;
    uint32_t rgba = 0xffffffffu;     // 0xAABBGGRR as a little-endian uint, i.e. bytes r, g, b, a
};
inline uint32_t overlay_colour(float r, float g, float b, float a = 1.0f) {
    auto q = [](float v) { return uint32_t(v <= 0 ? 0 : v >= 1 ? 255 : v * 255.0f + 0.5f); };
    return q(r) | (q(g) << 8) | (q(b) << 16) | (q(a) << 24);
}

// Immediate-style batch: `lines` are vertex pairs (1 px wide), `triangles` vertex triples (alpha blended).
struct Overlay {
    std::vector<OverlayVertex> lines;
    std::vector<OverlayVertex> triangles;
    bool depth_test = true;          // against the scene depth; false draws on top
    bool pickable = true;            // picked in screen space (distance to a line, inside a triangle)

    void line(const float3& a, const float3& b, uint32_t rgba) { lines.push_back({a, rgba}); lines.push_back({b, rgba}); }
    void triangle(const float3& a, const float3& b, const float3& c, uint32_t rgba) {
        triangles.push_back({a, rgba}); triangles.push_back({b, rgba}); triangles.push_back({c, rgba});
    }
    void box_wire(const float3& lo, const float3& hi, uint32_t rgba);
    void box_wire(const float4x4& m, const float3& lo, const float3& hi, uint32_t rgba);   // oriented: the box corners go through m
    void polygon_outline(const std::vector<float3>& points, bool closed, uint32_t rgba);
};

struct CameraRig {};                 // placeholder for the Cinematic editor

using Payload = std::variant<MeshInstance, Terrain, VfxEmitter, Overlay, CameraRig>;

struct Entity {
    EntityId id = 0;
    LayerId layer = 0;
    float4x4 world;
    SourceRef source;
    bool visible = true;
    Payload payload;

    Kind kind() const;
    template <class T> T* get() { return std::get_if<T>(&payload); }
    template <class T> const T* get() const { return std::get_if<T>(&payload); }
};

struct Layer {
    LayerId id = 0;
    DocumentId doc = 0;
    std::string name;
    bool visible = true;
    bool locked = false;             // not pickable
    int order = 0;
};

// Which environment lights the scene. The active weather document's environment wins; otherwise the tile tool's picker result
// applies; otherwise the game profile's default (all handled by whoever fills the slot).
struct EnvironmentSlot {
    std::optional<EnvironmentParams> active;
    std::optional<EnvironmentParams> blend;   // cross-fade partner
    float blend_t = 0;                        // 0: active, 1: blend
    DocumentId owner = 0;

    EnvironmentParams resolved() const;       // active (or defaults), cross-faded with blend when blend_t > 0
};

class RenderScene {
public:
    LayerId add_layer(DocumentId doc, std::string name);
    void remove_layer(LayerId layer);                      // and every entity in it
    Layer* layer(LayerId id);
    const Layer* layer(LayerId id) const;
    const std::vector<Layer>& layers() const { return layers_; }
    void set_layer_visible(LayerId id, bool visible);

    EntityId add(LayerId layer, const float4x4& world, Payload payload, SourceRef source = {});
    // Entities are edited in place through find(); call touch() afterwards (update() does both).
    Entity* find(EntityId id);
    const Entity* find(EntityId id) const;
    void update(EntityId id, const float4x4& world);
    void update(EntityId id, Payload payload);
    void remove(EntityId id);
    void clear_layer(LayerId layer);                       // keeps the layer, drops its entities
    void clear();

    // All entities in draw order (insertion order); hidden ones are included, see drawable().
    const std::vector<Entity>& entities() const { return entities_; }
    // The entities that show one element of a document (a building has one entity per model piece), in draw order.
    std::vector<EntityId> entities_of(const SourceRef& ref) const;
    // Visible entity in a visible layer.
    bool drawable(const Entity& e) const;
    // Entities of a visible layer that picking may hit.
    bool pickable(const Entity& e) const;

    EnvironmentSlot environment;

    uint64_t revision() const { return revision_; }
    void touch() { ++revision_; }

private:
    void reindex();

    std::vector<Layer> layers_;
    std::vector<Entity> entities_;
    std::unordered_map<EntityId, size_t> index_;
    LayerId next_layer_ = 1;
    EntityId next_entity_ = 1;
    uint64_t revision_ = 1;
};

} // namespace wce
