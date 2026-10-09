#pragma once
// One tile as a Document of the shared editor: tile.ted (TedScene, lossless) + height_map_0.png (HeightField) of a TileProject.
//
// What it shows (RenderScene layer): the terrain from the height map, every BUILDING_INSTANCE as the real model pieces of
// rigidmodels/buildings/<key>/ (a wire box when the key has no drawable model, reported in the log), every other instance as a small marker
// batched per type, polygon outlines, and a highlight overlay for the selection and the active tool's cursor. Items of the document are
// element indices + 1 (item 0 is the terrain). Edits are Commands: attribute edits on the TedScene (moves, rotations), whole-document snapshots
// for structural edits (duplicate, delete, layers, polygons), and brush strokes on the height field that merge into one entry per stroke.
//
// World space is metres: x = scene x / unit_scale, y = scene y, z = scene z / unit_scale (the legacy Studio's frame); the height map covers
// [-128, extent + 128] m, see TileProject.
#include "editor/document.h"
#include "tile/tile_project.h"
#include "tile/world_transform.h"

#include <array>
#include <functional>
#include <future>
#include <map>
#include <memory>
#include <optional>
#include <set>
#include <string>
#include <unordered_map>
#include <vector>

namespace wce {

class IVfs;

enum class BrushKind { Raise, Lower, Smooth, Flatten, Level };

struct BrushSettings {
    float radius_m = 12, strength = 6, falloff = 0.7f, flatten_height = 0;
};

struct TileDisplay {
    std::set<std::string> hidden_types;
    bool show_polygons = true;
    bool lod = true;                       // distance culling of the markers
    float veg_distance = 450, decal_distance = 300, other_distance = 3000;
    bool show_border = true;               // the tile's own rectangle (inside the bordered height map)
};

struct BuildingModel {
    std::vector<std::string> pieces;       // VFS paths of the rigid models (intact state of every piece)
    float3 bmin{0, 0, 0}, bmax{0, 0, 0};   // union of the pieces' bounds once they have loaded
    bool ok = false;                       // at least one drawable piece (assumed while the pieces load in the background)
    bool resolved = false;                 // every piece has loaded or failed
};

// What the renderer's model cache says about one piece (the tool fills it in; the document does not know the renderer).
struct PieceState {
    enum class Status { Loading, Ready, Failed } status = Status::Loading;
    float3 bmin{0, 0, 0}, bmax{0, 0, 0};
};

struct TileTypeStyle {
    uint32_t color;
    float size;
    bool cross;       // many small instances: three lines instead of a box
};

class TileDocument : public Document {
public:
    static constexpr uint64_t kTerrain = 0;

    TileDocument(std::unique_ptr<TileProject> project, IVfs* vfs);
    ~TileDocument() override;

    TileProject& project() { return *project_; }
    const TileProject& project() const { return *project_; }
    TedScene& tscene() { return project_->scene; }
    const TedScene& tscene() const { return project_->scene; }
    double unit() const { return project_->unit_scale; }

    // ---- Document
    std::string title() const override;
    std::string kind() const override { return "tile"; }
    bool can_save() const override { return true; }
    bool modified() const override;
    bool save(const SaveTarget& target, std::string* message) override;
    void publish(RenderScene& scene, LayerId layer) override;
    void republish(RenderScene& scene, LayerId layer, const CommandStack::Dirty& dirty) override;
    OutlinerNode outline() const override;
    uint64_t outline_stamp() const override { return structure_version_; }
    void inspect(const SourceRef& item, Inspector& inspector) override;
    std::string describe(const SourceRef& item) const override;
    std::string source_key() const override;
    bool item_transform(const SourceRef& item, float4x4& world) const override;
    void transform_items(const std::vector<SourceRef>& items, const float4x4& delta, uint64_t gesture) override;

    // ---- items
    static uint64_t item_of(int elem) { return uint64_t(elem) + 1; }
    static int elem_of(uint64_t item) { return item == 0 ? -1 : int(item - 1); }
    SourceRef ref_of(int elem) const { return {id(), item_of(elem)}; }
    bool is_instance(int elem) const;                  // an instance with a position (not a layer, not a polygon point)
    // Per frame: looks at the building pieces that were still loading; true when a building turned out to have no drawable model (it is a box
    // now: the markers were rebuilt).
    bool poll_models(const std::function<PieceState(const std::string&)>& peek);
    bool is_polygon(int elem) const;
    int layer_of(int elem) const;                      // nearest layer at or above `elem` (the scene graph for loose ones)
    double extent_x() const { return project_->extent_x(); }
    double extent_z() const { return project_->extent_z(); }
    // Metres: terrain height under (x, z); the world position / world matrix of an instance.
    float terrain_height(float x, float z) const { return float(project_->height_at(x, z)); }
    float3 world_position(int elem) const;
    float4x4 world_matrix(int elem) const;
    float marker_radius(int elem) const;               // what a click ray tests against
    const std::map<std::string, int>& type_counts() const { return type_counts_; }
    const std::vector<int>& instances() const { return instances_; }
    const BuildingModel* building_model(const std::string& key) const;
    const BuildingModel* model_of(int elem) const;      // of a BUILDING_INSTANCE element, nullptr for other elements
    const std::vector<std::string>& model_fallbacks() const { return fallbacks_; }
    // The nearest instance along a ray (spheres around the instances), or -1.
    int pick_instance(const float3& origin, const float3& dir, bool visible_only = true) const;
    bool pick_terrain(const float3& origin, const float3& dir, float3& hit) const;

    // ---- display
    TileDisplay& display() { return display_; }
    void display_changed() { markers_dirty_ = true; polygons_dirty_ = true; }
    // Per frame (the tool's update): rebuilds the marker batches when the camera moved enough or the display settings changed, and updates the highlight overlay.
    void update_view(RenderScene& scene, LayerId layer, const float3& eye);
    void set_highlight(RenderScene& scene, const std::vector<int>& selected, int polygon_point, float handle_size, const Overlay& tool_overlay);

    // ---- edits (each one undo entry; false with a message in the log / *error)
    uint64_t next_gesture() { return ++gesture_; }
    bool set_local(int elem, std::optional<Vec3> position, std::optional<Vec3> euler, uint64_t gesture);   // scene units / radians, local to the parent
    bool set_world_metres(int elem, const float3& position, uint64_t gesture);                               // keeps the orientation
    bool rotate(int elem, float degrees);                                                                    // about the world Y axis
    int duplicate(int elem, std::string* error = nullptr);                                                   // copy 4 m to the side, returns its element
    bool remove(int elem);
    int add_building(const std::string& key, int template_elem, const float3* at, std::string* error = nullptr);
    int add_polygon(int layer, const std::string& type, const std::string& hint, const std::vector<float3>& world_points, std::string* error = nullptr);
    bool polygon_point(int elem, const std::string& op, int index, const float3& world, std::string* error = nullptr, uint64_t gesture = 0);   // move | insert | remove
    int add_layer(int parent, const std::string& name, std::string* error = nullptr);
    bool move_to_layer(int elem, int layer, std::string* error = nullptr);
    bool rename(int elem, const std::string& name);
    bool set_attribute(int elem, const std::string& attr, const std::string& value);
    bool remove_layer(int layer);
    // Local polygon point of a world position / world positions of the points.
    Vec3 to_local(int elem, const float3& world) const;
    std::vector<float3> polygon_world_points(int elem) const;

    // Height strokes: begin on the first step; end when the mouse goes up (also done by undo/redo and save).
    void brush_step(BrushKind kind, float x, float z, const BrushSettings& s, float dt);
    void end_stroke();
    bool stroke_open() const { return stroke_open_; }

    // After a structural command the element indices moved: the selection the command wants (undo gives the selection before it).
    std::optional<std::vector<uint64_t>> take_selection_hint() { auto h = std::move(hint_); hint_.reset(); return h; }
    uint64_t structure_version() const { return structure_version_; }
    std::string diagnostics() const;
    std::string last_message;
    std::string load_info;                             // how long opening took, by phase

private:
    friend class TedMoveCommand;
    friend class TedSpliceCommand;
    friend class BrushStepCommand;

    using TypeStyle = TileTypeStyle;
    enum : uint8_t { kLayerBit = 1, kInstanceBit = 2, kPolygonBit = 4 };
    // Structure of arrays over the elements of the scene (element index = array index): what the document asks about every instance (is it one, where
    // is it, which building). Built in one parallel pass over the XML elements plus one sequential pass for the parent chains, instead of walking and
    // re-parsing the ancestors of every instance; rebuilt after structural edits, patched for one element after an attribute edit.
    struct Index {
        std::vector<uint16_t> type;                 // into type_names
        std::vector<uint8_t> kind;                  // kLayerBit | kInstanceBit | kPolygonBit
        std::vector<uint32_t> slot;                 // into world: the nearest ancestor-or-self that has a position or an orientation
        std::vector<WorldTransform> world;          // dense; world[0] is the identity
        std::vector<std::array<float, 3>> scale;
        std::vector<int32_t> key;                   // building key id (into keys) of BUILDING_INSTANCE elements, -1 otherwise
        std::vector<std::string> type_names, keys;
        std::vector<TypeStyle> style;               // per type id
        bool valid = false;
    } idx_;
    void build_index();
    void index_edited(int elem);                    // the position / orientation of one element changed
    void resolve_buildings();
    void rebuild_items(bool reindex = true);    // instances_, type counts, markers (from the scene and the index)
    void log_fallbacks();
    void add_item_entities(RenderScene& scene, LayerId layer, int elem);
    void refresh_item(RenderScene& scene, int elem);
    void update_terrain(RenderScene& scene, bool first);
    void rebuild_batch(RenderScene& scene, uint16_t type);
    void publish_items(RenderScene& scene, LayerId layer, bool keep_terrain);
    void rebuild_polygon(RenderScene& scene, LayerId layer, int elem);
    float lod_distance(const std::string& type) const;
    void build_pick_grid() const;
    bool save_scene_needed() const;
    bool height_modified() const;
    void apply_brush(BrushKind kind, float x, float z, const BrushSettings& s, float dt);
    // A structural edit is recorded as the text splices it made (TedScene::begin_edit / end_edit): `mark` is what begin_edit returned.
    void push_splice(const std::string& name, size_t mark, std::vector<uint64_t> sel_before, std::vector<uint64_t> sel_after);
    bool is_instance_slow(int elem) const;                // the element's own attributes, not the index (which is stale right after a structural edit)

    std::unique_ptr<TileProject> project_;
    IVfs* vfs_ = nullptr;
    TileDisplay display_;

    // display state
    RenderScene* scene_ = nullptr;
    LayerId layer_ = 0;
    EntityId terrain_entity_ = 0, overlay_entity_ = 0;
    uint64_t terrain_version_ = 1;
    int terrain_stride_ = 1;
    std::vector<int> instances_;
    // The markers (instances without a model), grouped by type: group t is [group_begin[t], group_begin[t + 1]) of the parallel arrays.
    struct Markers {
        std::vector<int32_t> elem;
        std::vector<float> x, y, z;
        std::vector<uint32_t> group_begin;
        std::vector<int32_t> slot;                  // per element: index into the arrays, -1 when it has no marker
    } markers_;
    std::map<std::string, int> type_counts_;
    std::vector<EntityId> batch_entity_;                              // per type id: the overlay entity of that type's markers
    std::vector<uint8_t> dirty_type_;                                 // per type id: rebuild the overlay at the next update
    std::vector<EntityId> first_entity_;                              // per element: its first model piece entity (the pieces have consecutive ids), 0 for none
    std::vector<uint8_t> entity_count_;
    std::unordered_map<int, EntityId> outline_entity_;                // polygon outlines by element
    std::map<std::string, BuildingModel> models_;
    std::vector<const BuildingModel*> key_model_;                     // per building key id
    std::vector<int> unresolved_;                                     // key ids whose pieces are still loading
    std::vector<std::string> fallbacks_;
    std::future<void> prefetch_;                                      // background extraction of the pieces and textures (first open of a tile only)
    bool markers_dirty_ = true, polygons_dirty_ = false;
    size_t hidden_signature_ = 0;
    float3 lod_eye_{1e30f, 1e30f, 1e30f};
    // pick grid: the instances in cells of the ground plane, for the ray test of a click or a hover
    struct PickGrid {
        bool valid = false;
        float cell = 64, x0 = 0, z0 = 0;
        int nx = 0, nz = 0;
        std::vector<uint32_t> start;                // nx * nz + 1
        std::vector<uint32_t> items;                // indices into the arrays below
        std::vector<int32_t> elem;
        std::vector<float> x, y, z, r;
        std::vector<uint16_t> type;
        std::vector<uint32_t> stamp;
        uint32_t stamp_counter = 0;
    };
    mutable PickGrid grid_;
    mutable bool grid_dirty_ = true;
    Overlay last_highlight_;
    bool have_highlight_ = false;

    // edit state
    uint64_t gesture_ = 0x100;
    uint64_t structure_version_ = 1;
    std::optional<std::vector<uint64_t>> hint_;
    bool stroke_open_ = false;
    uint64_t stroke_id_ = 0x5000;
    size_t saved_height_depth_ = 0;
    bool ted_on_disk_edited_ = false, png_on_disk_edited_ = false;

    // outline cache
    mutable OutlinerNode outline_cache_;
    mutable uint64_t outline_version_ = 0;
};

} // namespace wce
