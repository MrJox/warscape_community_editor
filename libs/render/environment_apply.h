#pragma once
// Writes an environment's values into the game shaders' constant buffers.
#include "scene/environment.h"

namespace wce {

class ConstantBuffers;

// skydome_PS, lighting_VS_PS, fog_VS_PS, luma_stats_buffer, auto_exposure_buffer, colorimetry_VS_PS and hdr_to_screen_PS.
// Every transform not yet verified against the game is marked ASSUMPTION in the code and listed in docs/re/environment_to_shader.md.
void apply_environment(const EnvironmentParams& e, ConstantBuffers& cbs);

} // namespace wce
