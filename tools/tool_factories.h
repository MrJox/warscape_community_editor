#pragma once
// Factories of the registered tools, so that app/ does not need each tool's headers.
#include "editor/tool.h"

#include <memory>

namespace wce {

std::unique_ptr<Tool> make_weather_tool();
std::unique_ptr<Tool> make_tile_tool();         // placeholder until Phase 6
std::unique_ptr<Tool> make_unit_tool();         // placeholders for the future tools: no features
std::unique_ptr<Tool> make_vfx_tool();
std::unique_ptr<Tool> make_terrain_tool();
std::unique_ptr<Tool> make_cinematic_tool();

} // namespace wce
