#pragma once
#include "foundation/clock.h"
#include "imgui.h"
#include "vfs/vfs.h"
#include "render/renderer.h"
#include "render/render_target.h"
#include "scene/render_scene.h"
#include "weather/sample_map.h"
#include "editor/variation_editor.h"
#include "formats/variation_db.h"
#include "formats/xml_doc.h"

#include <d3d11.h>

#include <filesystem>
#include <memory>
#include <string>
#include <vector>

namespace wce {

struct Document {
    std::string vfs_path;              // in-pack path (lower case); empty for files opened from disk
    std::filesystem::path source_disk; // file it was loaded from
    std::string source_label;          // pack name or "loose"
    XmlDoc doc;

    // An attribute edit, or (when doc_before is set) a structural edit that swapped the whole document.
    struct Change {
        int elem;
        size_t attr;
        std::string before, after;
        std::shared_ptr<XmlDoc> doc_before, doc_after;
    };
    std::vector<Change> undo, redo;

    // Structural edit requested while the tree is drawn; applied afterwards because it invalidates element indices.
    struct PendingOp {
        enum Kind { None, Remove, Duplicate, MoveUp, MoveDown, AddChild, AddAttr, RemoveAttr } kind = None;
        int elem = -1;
        size_t attr = 0;
        std::string name, value;
    } pending;
    int select_after = -1; // element to scroll to after a structural edit

    std::string title() const;
};

class App {
public:
    App(ID3D11Device* dev, ID3D11DeviceContext* ctx);
    void frame();
    void open_path(const std::string& path); // in-pack path or a file on disk
    void set_view(int v) { if (renderer_) renderer_->view = Renderer::View(v); }
    Renderer* renderer() { return renderer_.get(); }
    void set_blend(int doc, float t) { blend_doc_ = doc; blend_t_ = t; }
    void set_orbit(float yaw, float pitch, float dist, float target_y = 5) { cam_yaw_ = yaw; cam_pitch_ = pitch; cam_dist_ = dist; cam_target_.y = target_y; }
    // Fixed-step simulation (--fixed-step, implied by --shot): every rendered frame advances the clock by `step`, so frame N is the same every run.
    void set_fixed_step(double step) { clock_ = Clock::fixed(step); }
    uint64_t frames_rendered() const { return clock_.frames(); }
    // Renders the open documents into an offscreen w x h surface for `frames` fixed-step frames and writes the last one as PNG (--capture).
    bool capture(int w, int h, int frames, const std::wstring& path);
    std::string diagnostics() const;
    void set_vdb_script(const std::string& s) { vdb_script_ = s; }
    void show_picker(bool on, bool edit_tab = false) { show_picker_ = on; if (edit_tab) vdb_select_pick_ = false; }
    void set_pick(const std::string& category, const std::string& value) { pick_overrides_[category] = value; }
    bool confirm_close(); // false = cancel closing

private:
    void mount();
    void open_vfs(const std::string& path);
    void open_disk(const std::filesystem::path& path);
    bool save(Document& d, const std::filesystem::path& to);
    std::filesystem::path default_save_path(const Document& d) const;

    void draw_menu();
    void draw_browser();
    void draw_document(Document& d);
    void draw_element(Document& d, int elem);
    void draw_attribute(Document& d, int elem, size_t attr);
    void draw_changes(Document& d);
    void apply_pending(Document& d);
    void undo_redo(Document& d, bool undo);
    void draw_viewport();
    void draw_viewport_image(const ImVec2& size);
    void prepare_scene();
    void draw_picker();
    void load_variation_db();
    void load_vdb_source(int index);
    void load_vdb_file(const std::string& path);
    void save_variation_db(bool xml, bool bin);
    void run_vdb_script();
    void init_renderer();
    void load_settings();
    void store_settings() const;
    void set_status(std::string s, bool error = false);

    ID3D11Device* dev_;
    ID3D11DeviceContext* ctx_;
    std::unique_ptr<Vfs> vfs_;
    std::vector<std::unique_ptr<Document>> docs_;
    int active_ = -1;
    int focus_request_ = -1;

    // browser
    int category_ = 0;
    char filter_[256] = {};
    std::vector<std::string> listing_;
    bool listing_dirty_ = true;

    // inspector
    char attr_filter_[128] = {};
    std::string pending_before_; // raw value when the active widget was activated
    char new_name_[96] = {}, new_value_[256] = {}; // inputs of the element context menu

    // 3D preview
    RenderScene scene_;                       // what the viewport shows: the sample map layer and the active environment
    LayerId sample_layer_ = 0;
    SampleMapResult sample_;
    RenderTarget target_;
    std::string viewport_trace_;               // run-length list of the viewport sizes rendered so far (diagnostics: a size change mid-run changes the simulation)
    int trace_w_ = 0, trace_h_ = 0, trace_n_ = 0;
    std::unique_ptr<Renderer> renderer_;
    Camera camera_;
    EnvironmentParams last_params_; // what the viewport rendered last frame (menus read it)
    float cam_yaw_ = 0.6f, cam_pitch_ = 0.35f, cam_dist_ = 120.0f;
    float3 cam_target_{0, 5, 0};
    int scene_seed_ = 1, scene_count_ = 16;
    Clock clock_ = Clock::live();
    int env_doc_ = -1; // document driving the environment (last active .environment/.lighting)
    int blend_doc_ = -1;  // optional second environment for the cross-fade slider
    float blend_t_ = 0;
    float output_gamma_ = 2.2f, output_brightness_ = 1.0f; // from the game's preferences.script.txt

    // "What would the game pick": warscape_asset_variation_db and the current conditions
    std::unique_ptr<VariationDb> vdb_;
    std::string vdb_error_, vdb_source_, vdb_message_;
    struct VdbSource { std::string label; int pack; bool xml; };
    std::vector<VdbSource> vdb_sources_;
    int vdb_source_index_ = 0;
    char vdb_loose_[512] = {};
    bool vdb_save_anyway_ = false;
    VariationDb vdb_orig_; // as loaded, to tell whether the working copy was edited
    VariationEditState vedit_;
    std::string vdb_script_; // --vdb-script, run once after the database loaded
    bool vdb_select_pick_ = true; // the Pick tab is shown first
    bool vdb_loaded_ = false, show_picker_ = false;
    VariationDb::Query pick_query_, pick_overrides_; // overrides: command line --pick, applied when the database loads

    std::filesystem::path output_dir_;
    std::filesystem::path settings_path_;
    std::string status_;
    bool status_error_ = false;
};

} // namespace wce
