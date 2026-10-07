#include "vfx_constants.h"

#include "vfx_def.h"

#include <algorithm>
#include <cctype>
#include <cmath>

namespace we {

namespace {

float lerp(float a, float b, float t) { return a + (b - a) * t; }

// Piecewise start -> mid (at mid_t) -> end.
float three_point(float start, float mid, float end, float mid_t, float t) {
    mid_t = std::clamp(mid_t, 0.0001f, 0.9999f);
    return t <= mid_t ? lerp(start, mid, t / mid_t) : lerp(mid, end, (t - mid_t) / (1.0f - mid_t));
}

} // namespace

std::string vfx_texture_path(const std::string& path) {
    if (path.empty()) return {};
    std::string p = path;
    std::replace(p.begin(), p.end(), '\\', '/');
    std::transform(p.begin(), p.end(), p.begin(), [](char c) { return char(std::tolower(uint8_t(c))); });
    if (p.find('/') == std::string::npos) p = "vfx/textures/" + p;
    return p;
}

EmitterConstants build_emitter_constants(const VfxEmitterDef& em, const VfxChain* chain) {
    using namespace vfx_bits;
    EmitterConstants k;

    // --- simulation (verified on rain_heavy_BTL_daytime_only and snow_BTL) ---
    em.range("GravityAmount", k.ps_sim_gravity_params[0], k.ps_sim_gravity_params[1], 0, 0);
    k.ps_sim_gravity_params[2] = em.f("EffectedByWindFactor", 0);
    k.ps_sim_gravity_params[3] = em.f("WindVarianceFactor", 0);
    // Verified (sand_storm, Deceleration=false): the ranges are copied whatever the flag says.
    em.range("DecelerationStartTime", k.ps_sim_deceleration_params[0], k.ps_sim_deceleration_params[1], 0, 0);
    em.range("DecelerationAmount", k.ps_sim_deceleration_params[2], k.ps_sim_deceleration_params[3], 0, 0);
    float box[3] = {10, 10, 10};
    em.vec("RecycleClippedBox", box, 3);
    // x: 3 = RecycleClipped (wrap into the box around the emitter), 1 = ParticlesFollowEmitter,
    // 2 = follow with rotation (ASSUMPTION: ParticlesLocalSpace), 0 = world space
    k.ps_sim_following_emitter_params[0] = em.b("RecycleClipped", false) ? 3.0f
                                         : em.b("ParticlesLocalSpace", false) ? 2.0f
                                         : em.b("ParticlesFollowEmitter", false) ? 1.0f : 0.0f;
    for (int i = 0; i < 3; ++i) k.ps_sim_following_emitter_params[1 + i] = box[i];
    k.ps_sim_turbulence_params[0] = em.f("TurbulenceStart", 0);
    k.ps_sim_turbulence_params[1] = em.f("TurbulenceEnd", 0);
    k.ps_sim_turbulence_params[2] = em.f("VectorFieldScale", 1);
    k.ps_sim_game_logic_params[0] = em.b("ParticlesFollowTerrain", false) ? 1.0f : 0.0f; // ASSUMPTION

    float cull[4] = {0, 0, 500, 550};
    em.vec("ParticleCullDistances", cull, 4);
    for (int i = 0; i < 4; ++i) k.vs_culling_distances[i] = cull[i];
    k.vs_atlas_texture_params[0] = k.vs_atlas_texture_params[1] = 1;

    // --- scale rows 0/1: ParticleScaleStart/Mid/End (x, y) over life.
    // ASSUMPTION: min row = scale * (1 - ParticleScaleVariation), max row = scale. ---
    float s0[2] = {1, 1}, s1[2] = {1, 1}, s2[2] = {1, 1};
    em.vec("ParticleScaleStart", s0, 2);
    em.vec("ParticleScaleMid", s1, 2);
    em.vec("ParticleScaleEnd", s2, 2);
    if (em.b("ParticleScaleUseSingleScale", false)) s0[1] = s0[0], s1[1] = s1[0], s2[1] = s2[0];
    float mid_t = em.f("ParticleScaleMidPointTime", 0.5f), variation = em.f("ParticleScaleVariation", 0);
    for (int x = 0; x < kVfxCurveWidth; ++x) {
        float t = float(x) / float(kVfxCurveWidth - 1);
        for (int c = 0; c < 2; ++c) {
            float s = three_point(s0[c], s1[c], s2[c], mid_t, t);
            k.curves[0][x][c] = s * (1.0f - variation);
            k.curves[1][x][c] = s;
        }
        for (int r = 2; r < 8; ++r)
            for (int c = 0; c < 4; ++c) k.curves[r][x][c] = 1.0f;
    }

    if (!chain) return k;
    for (const VfxModifier& m : chain->modifiers) {
        const std::string& f = m.fragment;
        if (f == "vs_rotation") {
            k.behaviour_mask[0] |= kRotation;
            m.range("initial_rotation", k.vs_rotation_params[0], k.vs_rotation_params[1], 0, 0);
            m.range("rotation_over_life", k.vs_rotation_params[2], k.vs_rotation_params[3], 0, 0);
        } else if (f == "vs_translation" || f == "vs_translation_velocity_v2") {
            bool v2 = f == "vs_translation_velocity_v2";
            k.behaviour_mask[0] |= v2 ? kTranslationVelocity : kTranslation;
            k.vs_translation_params[0] = m.f("pivot_x", 0.5f);
            k.vs_translation_params[1] = m.f("pivot_y", 0.5f);
            k.vs_translation_params[2] = float(v2 ? m.i("orientation_mode", 0) : m.i("restriction", 0));
            k.vs_translation_params[3] = v2 ? m.f("velocity_stretch_mul", 0) : 0.0f;
        } else if (f == "vs_colour") {
            k.behaviour_mask[0] |= kColour;
            float a0[4] = {1, 1, 1, 1}, a1[4] = {1, 1, 1, 1}, b0[4] = {1, 1, 1, 1}, b1[4] = {1, 1, 1, 1}, c0[4] = {1, 1, 1, 1}, c1[4] = {1, 1, 1, 1};
            m.colour("start_colour", a0, a1);
            m.colour("mid_colour", b0, b1);
            m.colour("end_colour", c0, c1);
            float lt0, lt1;
            m.range("lerp_time", lt0, lt1, 0.5f, 0.5f);
            // ASSUMPTION: start -> mid until lerp_time, mid -> end after it; min row uses the min colours and lerp time.
            for (int x = 0; x < kVfxCurveWidth; ++x) {
                float t = float(x) / float(kVfxCurveWidth - 1);
                for (int c = 0; c < 4; ++c) {
                    k.curves[2][x][c] = three_point(a0[c], b0[c], c0[c], lt0, t);
                    k.curves[3][x][c] = three_point(a1[c], b1[c], c1[c], lt1, t);
                }
            }
        } else if (f == "vs_cell_texture") {
            k.behaviour_mask[0] |= kCellTexture;
            int ax = std::max(1, m.i("atlas_number_of_cells_x", 1)), ay = std::max(1, m.i("atlas_number_of_cells_y", 1));
            int cell = m.i("atlas_cell", 0);
            k.vs_atlas_texture_params[0] = 1.0f / float(ax);
            k.vs_atlas_texture_params[1] = 1.0f / float(ay);
            k.vs_atlas_texture_params[2] = float(cell % ax) / float(ax);
            k.vs_atlas_texture_params[3] = float(cell / ax) / float(ay);
            k.vs_cell_texture_params[0] = uint32_t(std::max(1, m.i("number_of_cells_x", 1)));
            k.vs_cell_texture_params[1] = uint32_t(std::max(1, m.i("number_of_cells_y", 1)));
            k.vs_cell_texture_params[2] = uint32_t(std::max(0, m.i("cell", 0)));
            k.vs_cell_texture_params[3] = m.b("randomize_cell", false) ? 1u : 0u;
        } else if (f == "vs_animated_texture") {
            // Verified on sand_storm: atlas and cell rows as vs_cell_texture; params = (cell_blend, loop,
            // row_restricted, randomize_row) (the VS selects with each), params1 = (no_loops.x, no_loops.y, row_restriction).
            k.behaviour_mask[0] |= kAnimatedTexture;
            int ax = std::max(1, m.i("atlas_number_of_cells_x", 1)), ay = std::max(1, m.i("atlas_number_of_cells_y", 1));
            int cell = m.i("atlas_cell", 0);
            k.vs_atlas_texture_params[0] = 1.0f / float(ax);
            k.vs_atlas_texture_params[1] = 1.0f / float(ay);
            k.vs_atlas_texture_params[2] = float(cell % ax) / float(ax);
            k.vs_atlas_texture_params[3] = float(cell / ax) / float(ay);
            float cells[2] = {1, 1}, loops[2] = {1, 1};
            m.float_n("no_cells_xy", cells, 2);
            m.float_n("no_loops", loops, 2);
            k.vs_cell_texture_params[0] = uint32_t(std::max(1.0f, cells[0]));
            k.vs_cell_texture_params[1] = uint32_t(std::max(1.0f, cells[1]));
            k.vs_cell_texture_params[2] = 0;
            k.vs_cell_texture_params[3] = m.b("randomize_cell", false) ? 1u : 0u;
            k.vs_animated_texture_params[0] = m.b("cell_blend", false) ? 1u : 0u;
            k.vs_animated_texture_params[1] = m.b("loop", false) ? 1u : 0u;
            k.vs_animated_texture_params[2] = m.b("row_restricted", false) ? 1u : 0u;
            k.vs_animated_texture_params[3] = m.b("randomize_row", false) ? 1u : 0u;
            k.vs_animated_texture_params1[0] = loops[0];
            k.vs_animated_texture_params1[1] = loops[1];
            k.vs_animated_texture_params1[2] = float(m.i("row_restriction", 0));
        } else if (f == "vs_vertex_alpha") {
            k.behaviour_mask[0] |= kVertexAlpha;
            const VfxCurve* lo = m.curve("curve_alpha", "alpha_min");
            const VfxCurve* hi = m.curve("curve_alpha", "alpha_max");
            for (int x = 0; x < kVfxCurveWidth; ++x) {
                float t = float(x) / float(kVfxCurveWidth - 1);
                float a = lo ? lo->eval(t) : 1.0f, b = hi ? hi->eval(t) : a;
                for (int c = 0; c < 4; ++c) {
                    k.curves[6][x][c] = a;
                    k.curves[7][x][c] = b;
                }
            }
        } else if (f == "ps_colour") {
            k.behaviour_mask[0] |= kPsColour;
        } else if (f == "ps_texture") {
            k.diffuse = vfx_texture_path(m.text("diffuse"));
            k.normal = vfx_texture_path(m.text("normal"));
            if (!k.diffuse.empty()) k.behaviour_mask[0] |= kPsTexture;
            if (!k.normal.empty()) k.behaviour_mask[0] |= kPsNormal;
        } else if (f == "ps_lighting") {
            k.behaviour_mask[0] |= kPsLighting;
            k.ps_lighting_legacy_params[0] = m.b("legacy_lighting", false) ? 1.0f : 0.0f; // ASSUMPTION
            k.ps_lighting_legacy_params[2] = m.f("emissive", 0);
            k.ps_lighting_legacy_params[3] = m.f("directionality", 0);
            // verified (rain + snow dumps): the scattering path divides by small + large, so these must be set
            k.ps_lighting_params[0] = m.f("average_size", 0);
            k.ps_lighting_params[1] = m.f("additive_blend_mul", 0);
            k.ps_lighting_params[2] = m.f("small_particles", 0);
            k.ps_lighting_params[3] = m.f("large_particles", 0);
            k.additive = m.b("additive_blend", false);
        } else if (f == "ps_soft_edge") {
            k.behaviour_mask[0] |= kPsSoftEdge;
            k.ps_soft_edge_params[0] = m.f("soft_edge_distance", m.f("distance", 1.0f)); // ASSUMPTION: property name
        } else if (f == "ps_fog") {
            k.behaviour_mask[0] |= kPsFog;
        }
    }
    return k;
}

} // namespace we
