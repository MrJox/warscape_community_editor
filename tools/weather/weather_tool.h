#pragma once
// The Weather / Lighting tool. In Phase 4 it is the host for the shared shell's first real document, the sample map; the environment,
// lighting, weather-definition and vfx documents, the variation database editor and the picker arrive in Phase 5.
#include "editor/tool.h"

namespace wce {

class WeatherTool : public Tool {
public:
    const char* id() const override { return "weather"; }
    const char* title() const override { return "Weather"; }
    void register_commands(Editor& editor, CommandRegistry& commands) override;
    void activated(Editor& editor) override;
    void draw_panels(Editor& editor) override;

private:
    bool environment_loaded_ = false;
};

} // namespace wce
