#include "tool_factories.h"

namespace wce {

std::unique_ptr<Tool> make_cinematic_tool() {
    return std::make_unique<PlaceholderTool>("cinematic", "Cinematic", "Keyframed scene composition and movie recording over PropertyPaths, with a camera rig, a second viewport and PNG sequence export.");
}

} // namespace wce
