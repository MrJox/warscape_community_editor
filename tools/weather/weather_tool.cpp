#include "weather/weather_tool.h"

#include "editor/editor.h"
#include "formats/xml_doc.h"
#include "foundation/log.h"
#include "scene/environment.h"
#include "weather/sample_map_document.h"

#include <algorithm>

namespace wce {

namespace {

// The first environment the sandbox lights the sample map with: what the legacy weather editor's gates use.
constexpr const char* kDefaultEnvironment = "weather/charlemagne/med_sp_land_day_dry.environment";

SampleMapDocument* sample_doc(Editor& editor) {
    for (DocumentId id : editor.documents().order())
        if (auto* d = dynamic_cast<SampleMapDocument*>(editor.documents().find(id))) return d;
    return nullptr;
}

} // namespace

void WeatherTool::activated(Editor& editor) {
    if (!editor.vfs()) return;
    if (!environment_loaded_ && !editor.scene().environment.active) {
        try {
            EnvironmentParams env = read_environment(XmlDoc::parse(editor.vfs()->read(kDefaultEnvironment)));
            const GraphicsOptions go = read_graphics_options();
            env.output_gamma = go.gamma;
            env.output_brightness = go.brightness;
            editor.scene().environment.active = env;
            environment_loaded_ = true;
        } catch (const std::exception& e) {
            Log::warn(std::string("default environment: ") + e.what());
        }
    }
    if (!sample_doc(editor)) {
        editor.commands().run("sample 1 16");
    }
}

void WeatherTool::register_commands(Editor& editor, CommandRegistry& commands) {
    commands.add("sample", "sample <seed> <count>", "open a new sample map document", [&editor](const CommandRegistry::Args& a) {
        const uint32_t seed = a.size() > 0 ? uint32_t(std::strtoul(a[0].c_str(), nullptr, 10)) : 1;
        const int count = a.size() > 1 ? std::clamp(std::atoi(a[1].c_str()), 1, 100) : 16;
        if (!editor.vfs()) return CommandResult::fail("the game packs are not mounted");
        auto doc = std::make_unique<SampleMapDocument>(*editor.vfs(), seed, count);
        const float3 ext = doc->info().bounds_max - doc->info().bounds_min;
        const int n = doc->info().buildings;
        editor.documents().add(std::move(doc), "weather");
        editor.viewport().use_orbit();
        static_cast<OrbitCamera&>(editor.viewport().controller()).set_orbit(0.6f, 0.35f, std::max(60.0f, std::max(ext.x, ext.z) * 0.9f), {0, 5, 0});
        return CommandResult::success("sample map with " + std::to_string(n) + " buildings");
    });
    commands.add("assert_offset", "assert_offset <item> dx dy dz [yaw_degrees]", "test: a sample map building has this placement", [&editor](const CommandRegistry::Args& a) {
        auto* d = sample_doc(editor);
        if (!d || a.size() < 4) return CommandResult::fail("usage: assert_offset <item> dx dy dz [yaw_degrees] (needs a sample map)");
        const Placement p = d->placement(std::strtoull(a[0].c_str(), nullptr, 10));
        const float want[3] = {std::strtof(a[1].c_str(), nullptr), std::strtof(a[2].c_str(), nullptr), std::strtof(a[3].c_str(), nullptr)};
        const bool same = std::fabs(p.offset.x - want[0]) < 1e-3f && std::fabs(p.offset.y - want[1]) < 1e-3f && std::fabs(p.offset.z - want[2]) < 1e-3f &&
                          (a.size() < 5 || std::fabs(p.yaw * 57.2957795f - std::strtof(a[4].c_str(), nullptr)) < 1e-2f);
        return same ? CommandResult::success()
                    : CommandResult::fail("placement is (" + std::to_string(p.offset.x) + ", " + std::to_string(p.offset.y) + ", " + std::to_string(p.offset.z) +
                                          ") yaw " + std::to_string(p.yaw * 57.2957795f));
    });
}

void WeatherTool::draw_panels(Editor& editor) {
    (void)editor;   // Phase 5: environment tree, picker, variation editor
}

} // namespace wce

#include "tool_factories.h"

namespace wce {

std::unique_ptr<Tool> make_weather_tool() { return std::make_unique<WeatherTool>(); }

} // namespace wce
