#pragma once
// The Tile tool: battle and campaign tiles (tile.ted + height_map_0.png) on the shared editor. Port of Tile Studio: terrain brushes, select / move /
// rotate / duplicate / delete / add instances, G-grab on the ground, layers, polygons and AI hints, the weather picker for the tile's map, per-type
// visibility and distance culling. Everything goes through the shared selection, gizmo, CommandStack and overlay pass; every action is a command too
// (tools/tile/tile_commands.cpp), so scripts and menus use the same code.
#include "editor/tool.h"
#include "formats/tile_index.h"
#include "formats/variation_db.h"
#include "tile/tile_document.h"

#include <map>
#include <memory>
#include <unordered_set>
#include <string>
#include <vector>

namespace wce {

class Editor;

enum class TileMode { Select, Raise, Lower, Smooth, Flatten, Level };

class TileTool : public Tool {
public:
    const char* id() const override { return "tile"; }
    const char* title() const override { return "Tile"; }
    std::vector<std::string> opens() const override { return {".ted"}; }
    void default_layout(ImGuiID dockspace) override;
    void register_commands(Editor& editor, CommandRegistry& commands) override;
    void activated(Editor& editor) override;
    void update(Editor& editor) override;
    void draw_panels(Editor& editor) override;
    void draw_menus(Editor& editor) override;
    bool viewport_input(Editor& editor, const ViewportEvent& event) override;
    bool claims_left_button(Editor& editor) const override { return mode_ != TileMode::Select || drawing_ || grabbing_; }
    std::unique_ptr<Document> open(Editor& editor, const std::string& path, bool from_packs) override;

    // The tile the tool works on: the active document when it is a tile, else the first open one.
    TileDocument* document(Editor& editor);
    // What the commands and the panels both do.
    bool open_folder(Editor& editor, const std::string& dir, std::string* message);
    // Opens a tile by its database entry (an _tile_database/TILES/*.xml path, or a name like "assembly_kit/osgiliath_test" / the entry's file name), by a
    // variation folder, a tile folder or a tile.ted. `variation` picks one of the entry's variations (a leaf folder name or a number; empty = the first).
    bool open_tile(Editor& editor, const std::string& target, const std::string& variation, std::string* message);
    // The raw_data folders the tile browser lists: `tile_kits` in wce.ini (';' separated), else the assembly kits under game_dir.
    std::vector<std::filesystem::path> kit_roots(Editor& editor) const;
    int primary(Editor& editor);                           // first selected instance element, -1 for none
    std::vector<int> selected(Editor& editor);
    void select_elem(Editor& editor, int elem, bool add = false);
    bool begin_grab(Editor& editor);
    void end_grab(Editor& editor, bool commit);
    void finish_polygon(Editor& editor);
    TileMode mode() const { return mode_; }
    void set_mode(TileMode m) { mode_ = m; if (m != TileMode::Select) { drawing_ = false; } }
    BrushSettings& brush() { return brush_; }
    bool drawing() const { return drawing_; }
    void start_drawing() { drawing_ = true; draw_pts_.clear(); }
    std::string status;
    static constexpr int kPolygonTypeCount = 5, kHintCount = 16;
    static const char* polygon_type(int i);
    static const char* hint(int i);

private:
    friend struct TileCommands;
    void draw_tile_panel(Editor& editor);
    void draw_layers_panel(Editor& editor);
    void draw_placement_section(Editor& editor, TileDocument& d);
    void rebuild_tree(TileDocument& d);
    void request_delete(Editor& editor);
    void begin_rename(Editor& editor, int elem);
    std::string tree_key_of(int elem) const;
    void draw_polygon_panel(Editor& editor, TileDocument& d);
    void draw_weather_panel(Editor& editor);
    void draw_browser_panel(Editor& editor);
    void draw_entry_section(Editor& editor, TileDocument& d);
    void handle_keys(Editor& editor);
    void load_environment_map(const std::string& tile_dir);
    bool cursor_ray(Editor& editor, int x, int y, float3& o, float3& d) const;
    void on_hover(Editor& editor, TileDocument& d, const ViewportEvent& ev);
    Overlay tool_overlay(Editor& editor, TileDocument& d);
    float handle_size(Editor& editor) const;

    TileMode mode_ = TileMode::Select;
    BrushSettings brush_;
    bool snap_grid_ = false;
    float grid_m_ = 6.0f, angle_step_ = 15.0f;
    char building_key_[128] = {};
    // placement (Phase 2): the mode of new buildings, the off-terrain report of the Tile panel
    TileDocument::Placement new_placement_ = TileDocument::Placement::Landscape;
    float off_tol_ = 0.5f;
    std::vector<TileDocument::OffTerrain> off_list_;
    uint64_t off_doc_version_ = ~0ull;
    DocumentId off_doc_ = 0;
    char rename_buf_[128] = {};
    // Scene panel: the flattened tree (open layers by name path), rename, delete confirmation, anchor of a shift-click range
    struct TreeRow { int elem; int depth; std::string key; bool layer; bool open; size_t kids; };
    std::vector<TreeRow> tree_rows_;
    std::unordered_set<std::string> tree_open_;
    DocumentId tree_doc_ = 0;
    uint64_t tree_version_ = ~0ull, tree_open_version_ = 0, tree_open_version_seen_ = ~0ull;
    int rename_elem_ = -1, anchor_ = -1, rename_wait_ = 0;
    bool rename_focus_ = false, rename_active_seen_ = false, begin_rename_next_ = false, delete_confirm_ = false;
    size_t delete_weight_ = 0;

    // cursor on the terrain (from the last hover / press)
    bool terrain_hit_ = false;
    float3 cursor_;
    // gestures
    bool stroke_active_ = false;
    bool grabbing_ = false;
    int grab_elem_ = -1;
    double grab_y_offset_ = 0;
    size_t grab_history_ = 0;
    uint64_t grab_gesture_ = 0;
    int poly_point_ = -1;
    bool point_drag_ = false;
    uint64_t point_gesture_ = 0;
    bool drawing_ = false;
    std::vector<float3> draw_pts_;
    int new_poly_type_ = 0, new_hint_ = 0;
    DocumentId camera_set_for_ = 0;
    std::map<std::string, float> marks_;      // mark_height values for the tests

    // The tile browser (Tiles panel) and the entry a pending open comes from (consumed by open())
    char browse_filter_[96] = {};
    int browse_kit_ = 0, browse_kind_ = 0;                // kit index into kit_roots, kind filter (all / battle / campaign)
    std::string browse_selected_;                         // the entry's key (normalised location) selected in the list
    std::filesystem::path pending_entry_;                 // entry xml + variation for the open() that follows
    int pending_variation_ = -1;
    std::string index_note_;                              // how the last index was built (log / status)
    std::vector<std::filesystem::path> browse_roots_;       // kit_roots, looked up at most every two seconds (the panel draws every frame)
    double browse_roots_time_ = -1e9;
    std::vector<const TileIndexEntry*> browse_rows_;      // the filtered, listed rows (rebuilt when the kit, kind, filter or index changed)
    std::string browse_rows_key_;
    std::shared_ptr<const TileIndex> browse_index_;

    // weather picker (data only: which .environment file the game picks for this map)
    std::unique_ptr<VariationDb> env_;
    VariationDb::Query wq_{{"map", "default_map"}};
    std::string env_status_;
};

} // namespace wce
