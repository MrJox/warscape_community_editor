#pragma once
// The Weather / Lighting tool: battle .environment, campaign .lighting, weather_definitions.xml and vfx XML as documents, a live preview of
// the active environment (cross-fadable with a second one) on the sample map, the weather selection database picker and editor.
// Port of the weather_editor application onto the shared shell; everything it did is a command here too (docs/editor.md).
#include "editor/tool.h"
#include "scene/environment.h"
#include "weather/weather_picker.h"
#include "weather/xml_file_document.h"

#include <string>
#include <vector>

namespace wce {

class Renderer;
class Editor;

class WeatherTool : public Tool {
public:
    const char* id() const override { return "weather"; }
    const char* title() const override { return "Weather"; }
    std::vector<std::string> opens() const override { return {".environment", ".lighting", ".xml"}; }
    void default_layout(ImGuiID dockspace) override;
    void register_commands(Editor& editor, CommandRegistry& commands) override;
    bool lights_scene() const override { return true; }
    void activated(Editor& editor) override;
    void update(Editor& editor) override;
    void draw_panels(Editor& editor) override;
    void draw_menus(Editor& editor) override;
    std::unique_ptr<Document> open(Editor& editor, const std::string& path, bool from_packs) override;

    // What the viewport shows: the document that lights the scene (the last active .environment / .lighting) and an optional cross-fade partner.
    XmlFileDocument* environment_document(Editor& editor);
    WeatherPicker& picker() { return picker_; }

private:
    friend struct WeatherCommands;
    XmlFileDocument* find_xml(Editor& editor, DocumentId id);
    XmlFileDocument* active_xml(Editor& editor);
    void draw_files(Editor& editor);
    void draw_scene_bar(Editor& editor);
    void draw_properties(Editor& editor);
    void draw_document(Editor& editor, XmlFileDocument& d);
    void draw_element(Editor& editor, XmlFileDocument& d, int e);
    void draw_attribute(Editor& editor, XmlFileDocument& d, int e, size_t a);
    void draw_changes(XmlFileDocument& d);
    void apply_pending(Editor& editor, XmlFileDocument& d);
    void reroll(Editor& editor);
    void ensure_sample_map(Editor& editor);
    bool tile_was_open_ = false;

    // lighting the scene
    EnvironmentParams default_env_;
    bool default_env_loaded_ = false;
    DocumentId env_doc_ = 0, blend_doc_ = 0;
    float blend_t_ = 0;
    float output_gamma_ = 2.2f, output_brightness_ = 1.0f;     // the game's preferences.script.txt
    EnvironmentParams last_params_;                             // what the viewport rendered last frame (menus read it)
    const Renderer* reader_for_ = nullptr;                      // the renderer the document-aware particle reader was installed on
    friend std::string weather_diagnostics(Editor& editor, WeatherTool& tool);

    // file browser
    int category_ = 0;
    char filter_[256] = {};
    std::vector<std::string> listing_;
    bool listing_dirty_ = true;

    // properties tree
    char attr_filter_[128] = {};
    char new_name_[96] = {}, new_value_[256] = {};
    uint64_t gesture_ = 1;
    int select_after_ = -1;
    struct Pending {
        bool active = false;
        XmlFileDocument::Op op = XmlFileDocument::Op::Remove;
        int elem = -1;
        size_t attr = 0;
        std::string name, value;
    } pending_;

    // sample map
    int scene_seed_ = 1, scene_count_ = 16;

    WeatherPicker picker_;
};

} // namespace wce
