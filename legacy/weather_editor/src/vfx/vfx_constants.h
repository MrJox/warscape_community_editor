#pragma once
// Per-emitter GPU constants of particle_vfx_*.hlsl, built from an emitter and its behaviour chain.
// Mapping verified against the rain and snow battle dumps (docs/re/particles.md); unverified parts are marked.
#include <cstdint>
#include <string>
#include <vector>

namespace wce {

struct VfxEmitterDef;
struct VfxChain;

// behaviour_mask.x bits (fragment present), from the shaders and the dumped masks
namespace vfx_bits {
constexpr uint32_t kRotation = 0x1, kColour = 0x2, kCellTexture = 0x4, kAnimatedTexture = 0x8, kVertexAlpha = 0x10;
constexpr uint32_t kTranslation = 0x100, kTranslationVelocity = 0x200, kTranslationScreenSpace = 0x800;
constexpr uint32_t kPsColour = 0x10000, kPsTexture = 0x20000, kPsNormal = 0x40000, kPsLighting = 0x200000;
constexpr uint32_t kPsAlphaCrush = 0x400000, kPsSoftEdge = 0x800000, kPsFog = 0x1000000;
} // namespace vfx_bits

// View id the main pass draws with (cb_vfx.g_view_id must equal behaviour_mask.w; 30 for every weather row).
constexpr uint32_t kVfxMainViewId = 30;
// scene_view_type 1 (heat haze, lens water drops): drawn with ps_distortion into the distortion buffer.
constexpr uint32_t kVfxDistortionViewId = 12;

// Emitter-constants texture: 8 rows per emitter (min/max pairs), u = particle age / lifetime.
// Rows 0-1 scale (xy), 2-3 colour (rgb), 4-5 unused by the main VS, 6-7 alpha (x).
constexpr int kVfxCurveWidth = 64;

struct EmitterConstants {
    uint32_t behaviour_mask[4] = {0, 0, 0, kVfxMainViewId};
    // cb_vfx_emitter_constants_gs
    float vs_translation_params[4] = {}, vs_translation_params1[4] = {};
    // cb_vfx_emitter_constants_ps (simulation layout: spawn + simulation shaders)
    float ps_sim_game_logic_params[4] = {}, ps_sim_gravity_params[4] = {}, ps_sim_deceleration_params[4] = {};
    float ps_sim_following_emitter_params[4] = {}, ps_sim_turbulence_params[4] = {};
    // cb_vfx_emitter_constants_ps (render layout: main pixel shader)
    float ps_specular_params[4] = {}, ps_specular_params1[4] = {}, ps_receive_shadows_high_quality_params[4] = {};
    float ps_lighting_legacy_params[4] = {}, ps_lighting_params[4] = {}, ps_alpha_crush_params[4] = {}, ps_soft_edge_params[4] = {};
    // cb_vfx_emitter_constants_vs
    float vs_culling_distances[4] = {}, vs_rotation_params[4] = {}, vs_scroll_uvs_params[4] = {}, vs_atlas_texture_params[4] = {};
    uint32_t vs_cell_texture_params[4] = {}, vs_animated_texture_params[4] = {};
    float vs_animated_texture_params1[4] = {};

    float curves[8][kVfxCurveWidth][4] = {}; // texture rows

    std::string diffuse, normal; // ps_texture paths (vfx/textures/...), empty if none
    bool additive = false;
};

EmitterConstants build_emitter_constants(const VfxEmitterDef& emitter, const VfxChain* chain);

// Texture path as the game resolves it: bare file names live in vfx/textures/.
std::string vfx_texture_path(const std::string& path);

} // namespace wce
