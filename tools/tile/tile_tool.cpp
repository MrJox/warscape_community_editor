#include "tool_factories.h"

namespace wce {

std::unique_ptr<Tool> make_tile_tool() {
    return std::make_unique<PlaceholderTool>("tile", "Tile", "Battle tile editor (TEd and BOB replacement): terrain brushes, instances, layers, polygons and AI hints on the shared scene. Arrives in Phase 6; the libraries (formats, TedScene, height field, tile project) are already in tools/tile.");
}

} // namespace wce
