#pragma once
// The editor shell: owns the VFS, the renderer, the shared scene and the open documents, draws the dockspace, menus, toolbar and panels,
// switches tools (with a layout file per tool), and runs commands and scripts. app/main.cpp creates one Editor, registers the tools and
// calls pre_frame() / frame() around ImGui's NewFrame / Render.
#include "editor/camera_controller.h"
#include "editor/command_registry.h"
#include "editor/document_manager.h"
#include "editor/selection.h"
#include "editor/tool.h"
#include "foundation/clock.h"
#include "foundation/settings.h"
#include "render/renderer.h"
#include "vfs/vfs.h"

#include <d3d11.h>

#include <algorithm>
#include <chrono>
#include <deque>
#include <filesystem>
#include <memory>
#include <string>
#include <vector>

namespace wce {

class Editor;

// Frame times of the last frames (the app loop records them): the menu bar shows the mean, the `perf` command prints mean / p95 / worst.
class PerfStats {
public:
    static constexpr int kKeep = 600;
    void record(double frame_ms, double cpu_ms) {
        frame_[n_ % kKeep] = float(frame_ms);
        cpu_[n_ % kKeep] = float(cpu_ms);
        ++n_;
    }
    void reset() { n_ = 0; }
    int count() const { return int(n_ < kKeep ? n_ : kKeep); }
    double mean_frame_ms(int last = kKeep) const { return mean(frame_, last); }
    double mean_cpu_ms(int last = kKeep) const { return mean(cpu_, last); }
    double percentile_frame_ms(double p) const;     // p in 0..1
    double worst_frame_ms() const;
    std::string report() const;

private:
    double mean(const float* v, int last) const {
        const int n = std::min(count(), last);
        double s = 0;
        for (int i = 0; i < n; ++i) s += v[(n_ - 1 - i) % kKeep];
        return n ? s / n : 0.0;
    }
    float frame_[kKeep] = {}, cpu_[kKeep] = {};
    uint64_t n_ = 0;
};

enum class GizmoMode { Off, Translate, Rotate, Scale };

// The 3D viewport panel: renders the scene through the renderer, drives the camera controller, picks, draws the gizmo.
class Viewport {
public:
    Viewport();
    void draw(Editor& editor);                                        // the "Viewport" window
    // Offscreen: `frames` fixed-step frames of the scene at w x h, last one written as PNG. No window is involved.
    bool capture(Editor& editor, int w, int h, int frames, const std::wstring& path);
    EntityId pick(Editor& editor, int x, int y);                      // in the last rendered image
    // The camera ray through a pixel of the last rendered image (world space, direction normalized). False before the first frame.
    bool ray(int x, int y, float3& origin, float3& direction) const;
    // Where a world point is in the last rendered image (pixels). False when it is behind the camera or there was no frame yet.
    bool project(const float3& world, float& x, float& y) const;
    void focus_selection(Editor& editor);

    CameraController& controller() { return *controller_; }
    void use_orbit();
    void use_fly();
    bool is_fly() const { return fly_active_; }
    Camera camera() const { return controller_->camera(); }
    RenderTarget& target() { return target_; }
    int width() const { return target_.width(); }
    ImVec2 image_origin() const { return image_origin_; }          // screen position of the viewport image's top-left corner (last frame)
    int height() const { return target_.height(); }

    GizmoMode gizmo = GizmoMode::Translate;
    bool gizmo_local = false;
    bool snap = false;
    float snap_move = 1.0f, snap_angle = 15.0f, snap_scale = 0.1f;
    bool show_grid = false;
    bool hovered = false;
    uint64_t frames_rendered = 0;
    // Run-length list of the sizes rendered so far ("694x824 x1, 680x824 x19"): a size change mid-run changes what exposure adaptation sees, so
    // two runs that differ in it are not expected to match pixel for pixel.
    const std::string& size_trace() const { return trace_; }

private:
    void render(Editor& editor, int w, int h);
    std::unique_ptr<OrbitCamera> orbit_;
    std::unique_ptr<FlyCamera> fly_;
    CameraController* controller_ = nullptr;
    bool fly_active_ = false;
    RenderTarget target_;
    // gizmo drag
    std::string trace_;
    int trace_w_ = 0, trace_h_ = 0, trace_n_ = 0;
    bool gizmo_was_using_ = false;
    uint64_t gesture_ = 0;
    float4x4 gizmo_matrix_;
    float2 press_{0, 0};
    bool pressed_ = false;
    bool tool_drag_ = false;          // the active tool owns the current left-button gesture
    ImVec2 image_origin_{0, 0};
};

class Editor {
public:
    Editor(ID3D11Device* dev, ID3D11DeviceContext* ctx);
    ~Editor();

    // ---- setup (before the first frame)
    void add_tool(std::unique_ptr<Tool> tool);
    // Mounts the packs and creates the renderer. False (with a message in the log) when the game is not configured: the first-run dialog opens.
    bool start();
    void set_fixed_step(double step) { clock_ = Clock::fixed(step); }

    // ---- per frame
    void pre_frame();     // before ImGui::NewFrame: applies a pending tool switch (layout files)
    void frame();         // between NewFrame and Render: dockspace, menus, panels, commands, scripts
    bool wants_quit() const { return quit_; }
    int exit_code() const { return exit_code_; }
    uint64_t frame_number() const { return frame_; }

    // ---- state
    PerfStats perf;
    std::chrono::steady_clock::time_point wait_loaded_start{};
    bool vsync = true;                // the app loop presents with it (command `vsync`, environment WCE_VSYNC=0)
    ID3D11Device* device() const { return dev_; }
    ID3D11DeviceContext* context() const { return ctx_; }
    Vfs* vfs() { return vfs_.get(); }
    Renderer* renderer() { return renderer_.get(); }
    RenderScene& scene() { return scene_; }
    DocumentManager& documents() { return docs_; }
    Selection& selection() { return selection_; }
    CommandRegistry& commands() { return commands_; }
    ScriptRunner& script() { return script_; }
    Viewport& viewport() { return viewport_; }
    Clock& clock() { return clock_; }
    Settings& settings() { return settings_; }
    const std::vector<std::unique_ptr<Tool>>& tools() const { return tools_; }
    Tool* active_tool() { return tools_.empty() ? nullptr : tools_[active_tool_].get(); }
    int active_tool_index() const { return active_tool_; }

    // ---- scripted mouse (fixed-step runs ignore the real mouse; the `mouse` commands drive the same ImGui events). Positions are pixels of the viewport image.
    struct MouseState {
        float x = 0, y = 0;
        bool down[3] = {false, false, false};
        float wheel = 0;
    };
    void queue_mouse(const MouseState& s) { mouse_queue_.push_back(s); mouse_active_ = true; }
    void park_mouse() { mouse_queue_.clear(); mouse_active_ = false; mouse_cur_ = MouseState{}; }
    // Called by the app between the platform's NewFrame and ImGui::NewFrame: replays the scripted mouse (or parks the cursor) in fixed-step runs.
    void inject_input();
    bool scripted_input() const { return clock_.fixed_step(); }

    // ---- actions (also reachable as commands)
    bool set_tool(const std::string& id);            // takes effect in the next pre_frame
    bool open_path(const std::string& path, bool from_packs, std::string* message);   // routed to the tool that claims the extension
    // Stores the game (and RPFM) folder in wce.ini and mounts: what the first-run dialog's button does, also the `setup` command.
    bool setup(const std::string& game_dir, const std::string& rpfm_dir, std::string* message);
    void request_quit(int exit_code = 0) { quit_ = true; exit_code_ = exit_code; }
    // Where "Save" writes loose files (under their in-pack path): output_dir in wce.ini, else <Documents>\AttilaWeatherEditor\out as the old weather editor did.
    std::filesystem::path output_dir() const;
    void set_output_dir(const std::filesystem::path& dir, bool persist);
    void save_layout();
    void reset_layout();
    std::filesystem::path layout_file(const std::string& tool_id) const;
    // Dock id of the main dockspace (default_layout builds into it).
    ImGuiID dockspace_id() const { return dockspace_; }

    // ---- panels
    struct PanelState {
        bool viewport = true, outliner = true, inspector = true, assets = true, log = true, history = true, commands = false;
    } panels;

    void status(const std::string& text, bool error = false);
    const std::string& status_text() const { return status_; }
    bool status_is_error() const { return status_error_; }
    // The path of the file the next --shot / `shot` writes, handled by the app loop.
    std::wstring pending_shot;

private:
    void register_builtin_commands();
    void draw_menu_bar();
    void draw_toolbar();
    void draw_panels();
    void draw_first_run();
    void apply_tool_switch();
    void release_test_keys();

    ID3D11Device* dev_;
    ID3D11DeviceContext* ctx_;
    Settings settings_;
    std::unique_ptr<Vfs> vfs_;
    std::unique_ptr<Renderer> renderer_;
    RenderScene scene_;
    DocumentManager docs_{scene_};
    Selection selection_;
    CommandRegistry commands_;
    ScriptRunner script_;
    Viewport viewport_;
    Clock clock_ = Clock::live();
    std::vector<std::unique_ptr<Tool>> tools_;
    int active_tool_ = 0;
    int pending_tool_ = -1;
    bool layout_pending_default_ = false;
    bool layout_loaded_ = false;
    ImGuiID dockspace_ = 0;
    bool quit_ = false;
    int exit_code_ = 0;
    uint64_t frame_ = 0;
    std::string status_;
    bool status_error_ = false;
    bool first_run_ = false;
    struct HeldKey {
        ImGuiKey key;
        int frames_left;
    };
    std::deque<MouseState> mouse_queue_;
    MouseState mouse_cur_;
    bool mouse_active_ = false;
    std::vector<HeldKey> test_keys_;   // keys the `key` command holds down for a couple of frames
    std::string first_run_game_, first_run_rpfm_, first_run_message_;
    friend class Viewport;
};

} // namespace wce
