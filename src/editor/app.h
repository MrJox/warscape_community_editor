#pragma once
#include "core/vfs.h"
#include "render/renderer.h"
#include "render/scene.h"
#include "formats/xml_doc.h"

#include <d3d11.h>

#include <filesystem>
#include <memory>
#include <string>
#include <vector>

namespace we {

struct Document {
    std::string vfs_path;              // in-pack path (lower case); empty for files opened from disk
    std::filesystem::path source_disk; // file it was loaded from
    std::string source_label;          // pack name or "loose"
    XmlDoc doc;

    struct Change { int elem; size_t attr; std::string before, after; };
    std::vector<Change> undo, redo;

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
    std::string diagnostics() const;
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
    void draw_viewport();
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

    // 3D preview
    std::unique_ptr<TextureCache> textures_;
    std::unique_ptr<TestScene> scene_;
    std::unique_ptr<Renderer> renderer_;
    Camera camera_;
    EnvironmentParams last_params_; // what the viewport rendered last frame (menus read it)
    float cam_yaw_ = 0.6f, cam_pitch_ = 0.35f, cam_dist_ = 120.0f;
    float3 cam_target_{0, 5, 0};
    int scene_seed_ = 1, scene_count_ = 16;
    double start_time_ = 0;
    int env_doc_ = -1; // document driving the environment (last active .environment/.lighting)
    int blend_doc_ = -1;  // optional second environment for the cross-fade slider
    float blend_t_ = 0;
    float output_gamma_ = 2.2f, output_brightness_ = 1.0f; // from the game's preferences.script.txt

    std::filesystem::path output_dir_;
    std::filesystem::path settings_path_;
    std::string status_;
    bool status_error_ = false;
};

} // namespace we
