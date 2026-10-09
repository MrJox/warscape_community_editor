#include "tool_factories.h"

namespace wce {

std::unique_ptr<Tool> make_unit_tool() {
    return std::make_unique<PlaceholderTool>("unit", "Unit", "Unit editor: skinned models, animations, variant mesh definition (VMD) assembly. Needs the SkinnedInstance render kind, bone palette and the anim/skeleton formats.");
}

} // namespace wce
