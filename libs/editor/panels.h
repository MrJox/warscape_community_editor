#pragma once
// The shared panels and the ImGui implementation of the inspector framework.
#include "editor/inspector.h"

namespace wce {

class Editor;

struct Panels {
    static void outliner(Editor& editor);
    static void inspector(Editor& editor);
    static void assets(Editor& editor);
    static void log(Editor& editor);
    static void history(Editor& editor);
    static void commands(Editor& editor);
};

// Draws one property per call as an ImGui row. edit_* return true on every frame the value changed; gesture_ended() is true on the frame an
// edit is finished (the widget lost focus after a change).
class ImGuiInspector : public Inspector {
public:
    void section(const std::string& title) override;
    void text(const std::string& field, const std::string& label, const std::string& value) override;
    bool edit_float(const std::string& field, const std::string& label, float& v, float speed, float lo, float hi) override;
    bool edit_float3(const std::string& field, const std::string& label, float v[3], float speed) override;
    bool edit_bool(const std::string& field, const std::string& label, bool& v) override;
    bool edit_string(const std::string& field, const std::string& label, std::string& v) override;
    bool edit_choice(const std::string& field, const std::string& label, int& index, const std::vector<std::string>& choices) override;
    bool gesture_ended() override { return ended_; }

private:
    void finish_widget() { ended_ = ImGuiInspector::item_deactivated_after_edit(); }
    static bool item_deactivated_after_edit();
    std::string id(const std::string& field) const { return "##" + path(field).to_string(); }
    bool ended_ = false;
};

} // namespace wce
