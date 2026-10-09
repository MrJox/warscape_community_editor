#include "tool_factories.h"

namespace wce {

std::unique_ptr<Tool> make_vfx_tool() {
    return std::make_unique<PlaceholderTool>("vfx", "VFX", "Particle and effect editor on the existing particle system and vfx definitions: emitter graph, standalone preview scene, vfx XML export.");
}

} // namespace wce
