#pragma once
// A Tool is a mode of the editor (Weather, Tile, and the future Unit, VFX, Terrain and Cinematic editors). It decides which documents it
// opens, which panels and menus it adds, what the viewport does with clicks, and the default dock layout. The shell (docking, outliner,
// inspector, asset browser, log, history, viewport, gizmos, commands, layouts) is shared.
#include "editor/document.h"

#include <imgui.h>

#include <memory>
#include <string>
#include <vector>

namespace wce {

class Editor;
class CommandRegistry;

// What the viewport tells the active tool about the mouse.
struct ViewportEvent {
    enum class Kind { Click, Drag, Hover } kind = Kind::Hover;
    int x = 0, y = 0;                // pixel in the viewport image
    bool ctrl = false, shift = false, alt = false;
    EntityId hit = 0;                // what is under the cursor (clicks only), 0 for nothing
    SourceRef hit_source;            // the document element behind `hit`
    int button = 0;                  // 0 left, 1 right, 2 middle
};

class Tool {
public:
    virtual ~Tool() = default;
    virtual const char* id() const = 0;                       // "weather", "tile", ...
    virtual const char* title() const = 0;
    // File extensions (with the dot, lower case) this tool opens from the packs or from disk; the asset browser filters by them.
    virtual std::vector<std::string> opens() const { return {}; }
    // Builds the first-run layout in the dockspace (DockBuilder calls). The default is the standard layout every tool starts from.
    virtual void default_layout(ImGuiID dockspace);
    // Extra panels and menus of the tool, called every frame while it is active.
    virtual void draw_panels(Editor& editor) { (void)editor; }
    virtual void draw_menus(Editor& editor) { (void)editor; }
    // Return true when the event was handled (the default click-to-select is then skipped).
    virtual bool viewport_input(Editor& editor, const ViewportEvent& event) { (void)editor; (void)event; return false; }
    virtual void register_commands(Editor& editor, CommandRegistry& commands) { (void)editor; (void)commands; }
    // Once per frame while the tool is active, after the documents were re-published and before the viewport renders: put the tool's
    // documents into the scene's environment slot, refresh anything the renderer reads.
    virtual void update(Editor& editor) { (void)editor; }
    // The tool became the active one (open a default document, set the camera, ...).
    virtual void activated(Editor& editor) { (void)editor; }
    // Opens a file of the packs or a disk file this tool claims; nullptr when it cannot.
    virtual std::unique_ptr<Document> open(Editor& editor, const std::string& path, bool from_packs) { (void)editor; (void)path; (void)from_packs; return nullptr; }
};

// A registered tool with no features yet: a panel that says what is planned. Keeps the switcher, layout files and document routing
// exercised from the first day (PLAN.md, future tools).
class PlaceholderTool : public Tool {
public:
    PlaceholderTool(std::string id, std::string title, std::string plan) : id_(std::move(id)), title_(std::move(title)), plan_(std::move(plan)) {}
    const char* id() const override { return id_.c_str(); }
    const char* title() const override { return title_.c_str(); }
    void draw_panels(Editor& editor) override;

private:
    std::string id_, title_, plan_;
};

} // namespace wce
