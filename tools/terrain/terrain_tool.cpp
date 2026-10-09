#include "tool_factories.h"

namespace wce {

std::unique_ptr<Tool> make_terrain_tool() {
    return std::make_unique<PlaceholderTool>("terrain", "Terrain", "Battle map and Terry terrain: tilemap painting and the pixel-close terrain pass (see docs/re/terrain.md), tile_list/blm export, BOB terrain stages.");
}

} // namespace wce
