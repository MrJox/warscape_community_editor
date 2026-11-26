//! Per-emitter GPU constants of the game's `particle_vfx_*.hlsl` shaders, built from an emitter and its behaviour chain.
//!
//! The mapping is verified against the rain and snow battle dumps; unverified parts are marked as assumptions.

use crate::vfx_def::{VfxChain, VfxEmitterDef};

/// `behaviour_mask.x` bits (fragment present), from the shaders and the dumped masks.
pub mod vfx_bits {
    pub const ROTATION: u32 = 0x1;
    pub const COLOUR: u32 = 0x2;
    pub const CELL_TEXTURE: u32 = 0x4;
    pub const ANIMATED_TEXTURE: u32 = 0x8;
    pub const VERTEX_ALPHA: u32 = 0x10;
    pub const TRANSLATION: u32 = 0x100;
    pub const TRANSLATION_VELOCITY: u32 = 0x200;
    pub const TRANSLATION_SCREEN_SPACE: u32 = 0x800;
    pub const PS_COLOUR: u32 = 0x10000;
    pub const PS_TEXTURE: u32 = 0x20000;
    pub const PS_NORMAL: u32 = 0x40000;
    pub const PS_LIGHTING: u32 = 0x20_0000;
    pub const PS_ALPHA_CRUSH: u32 = 0x40_0000;
    pub const PS_SOFT_EDGE: u32 = 0x80_0000;
    pub const PS_FOG: u32 = 0x100_0000;
}

/// The view id the main pass draws with (`cb_vfx.g_view_id` must equal `behaviour_mask.w`; 30 for every weather row).
pub const VFX_MAIN_VIEW_ID: u32 = 30;

/// `scene_view_type` 1 (heat haze, lens water drops): drawn with ps_distortion into the distortion buffer.
pub const VFX_DISTORTION_VIEW_ID: u32 = 12;

/// `scene_view_type` 2 (SeaColourOverlay: foam on the sea, the naval wakes and splashes): drawn with ps_overlay into the sea's colour
/// overlay, which the ocean shader reads. The game's id (empire.retail.dll fn 0x1189b8c0 maps scene_view_type 0, 1, 2 to 30, 12, 3).
pub const VFX_SEA_OVERLAY_VIEW_ID: u32 = 3;

/// Width of the emitter-constants texture: 8 rows per emitter (min / max pairs), u = particle age / lifetime.
/// Rows 0-1 scale (xy), 2-3 colour (rgb), 4-5 unused by the main VS, 6-7 alpha (x).
/// 256 as the game's scale baker (it writes 256 entries per row as RGBA16F; here f32).
pub const VFX_CURVE_WIDTH: usize = 256;

#[derive(Clone, Debug, PartialEq)]
pub struct EmitterConstants {
    pub behaviour_mask: [u32; 4],

    // cb_vfx_emitter_constants_gs
    pub vs_translation_params: [f32; 4],
    pub vs_translation_params1: [f32; 4],

    // cb_vfx_emitter_constants_ps, simulation layout (spawn and simulation shaders)
    pub ps_sim_game_logic_params: [f32; 4],
    pub ps_sim_gravity_params: [f32; 4],
    pub ps_sim_deceleration_params: [f32; 4],
    pub ps_sim_following_emitter_params: [f32; 4],
    pub ps_sim_turbulence_params: [f32; 4],

    // cb_vfx_emitter_constants_ps, render layout (main pixel shader)
    pub ps_specular_params: [f32; 4],
    pub ps_specular_params1: [f32; 4],
    pub ps_receive_shadows_high_quality_params: [f32; 4],
    pub ps_lighting_legacy_params: [f32; 4],
    pub ps_lighting_params: [f32; 4],
    pub ps_alpha_crush_params: [f32; 4],
    pub ps_soft_edge_params: [f32; 4],

    // cb_vfx_emitter_constants_vs
    pub vs_culling_distances: [f32; 4],
    pub vs_rotation_params: [f32; 4],
    pub vs_scroll_uvs_params: [f32; 4],
    pub vs_atlas_texture_params: [f32; 4],
    pub vs_cell_texture_params: [u32; 4],
    pub vs_animated_texture_params: [u32; 4],
    pub vs_animated_texture_params1: [f32; 4],

    /// The texture rows.
    pub curves: Box<[[[f32; 4]; VFX_CURVE_WIDTH]; 8]>,

    /// ps_texture paths (`vfx/textures/...`); empty if none.
    pub diffuse: String,
    pub normal: String,
    pub additive: bool,
}

impl Default for EmitterConstants {
    fn default() -> Self {
        Self {
            behaviour_mask: [0, 0, 0, VFX_MAIN_VIEW_ID],
            vs_translation_params: [0.0; 4],
            vs_translation_params1: [0.0; 4],
            ps_sim_game_logic_params: [0.0; 4],
            ps_sim_gravity_params: [0.0; 4],
            ps_sim_deceleration_params: [0.0; 4],
            ps_sim_following_emitter_params: [0.0; 4],
            ps_sim_turbulence_params: [0.0; 4],
            ps_specular_params: [0.0; 4],
            ps_specular_params1: [0.0; 4],
            ps_receive_shadows_high_quality_params: [0.0; 4],
            ps_lighting_legacy_params: [0.0; 4],
            ps_lighting_params: [0.0; 4],
            ps_alpha_crush_params: [0.0; 4],
            ps_soft_edge_params: [0.0; 4],
            vs_culling_distances: [0.0; 4],
            vs_rotation_params: [0.0; 4],
            vs_scroll_uvs_params: [0.0; 4],
            vs_atlas_texture_params: [0.0; 4],
            vs_cell_texture_params: [0; 4],
            vs_animated_texture_params: [0; 4],
            vs_animated_texture_params1: [0.0; 4],
            curves: Box::new([[[0.0; 4]; VFX_CURVE_WIDTH]; 8]),
            diffuse: String::new(),
            normal: String::new(),
            additive: false,
        }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Piecewise start -> mid (at `mid_t`) -> end.
fn three_point(start: f32, mid: f32, end: f32, mid_t: f32, t: f32) -> f32 {
    let mid_t = mid_t.clamp(0.0001, 0.9999);
    if t <= mid_t { lerp(start, mid, t / mid_t) } else { lerp(mid, end, (t - mid_t) / (1.0 - mid_t)) }
}

/// The scale baker's segments (fn 0x182c3c0 -> fn 0x182ba40): start -> mid for t < mid_t, mid -> end from mid_t on, with mid_t not clamped
/// (0 never uses start, 1 never uses end).
fn scale_point(start: f32, mid: f32, end: f32, mid_t: f32, t: f32) -> f32 {
    if t < mid_t { lerp(start, mid, t / mid_t) } else { lerp(mid, end, (t - mid_t) / (1.0 - mid_t)) }
}

/// The curve texture's u of a column.
fn column_t(x: usize) -> f32 {
    x as f32 / (VFX_CURVE_WIDTH - 1) as f32
}

/// A texture path as the game resolves it: bare file names live in vfx/textures/.
pub fn vfx_texture_path(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }

    let p = path.replace('\\', "/").to_ascii_lowercase();
    if p.contains('/') { p } else { format!("vfx/textures/{p}") }
}

/// The atlas rows of vs_cell_texture and vs_animated_texture.
fn atlas(k: &mut EmitterConstants, ax: i32, ay: i32, cell: i32) {
    k.vs_atlas_texture_params = [1.0 / ax as f32, 1.0 / ay as f32, (cell % ax) as f32 / ax as f32, (cell / ax) as f32 / ay as f32];
}

fn flag(on: bool) -> u32 {
    u32::from(on)
}

fn flag_f(on: bool) -> f32 {
    if on { 1.0 } else { 0.0 }
}

pub fn build_emitter_constants(em: &VfxEmitterDef, chain: Option<&VfxChain>) -> EmitterConstants {
    use vfx_bits::*;
    let mut k = EmitterConstants::default();

    // Simulation (verified on rain_heavy_BTL_daytime_only and snow_BTL).
    (k.ps_sim_gravity_params[0], k.ps_sim_gravity_params[1]) = em.range("GravityAmount", 0.0, 0.0);
    k.ps_sim_gravity_params[2] = em.f("EffectedByWindFactor", 0.0);
    k.ps_sim_gravity_params[3] = em.f("WindVarianceFactor", 0.0);

    // Verified (sand_storm, Deceleration=false): the ranges are copied whatever the flag says.
    (k.ps_sim_deceleration_params[0], k.ps_sim_deceleration_params[1]) = em.range("DecelerationStartTime", 0.0, 0.0);
    (k.ps_sim_deceleration_params[2], k.ps_sim_deceleration_params[3]) = em.range("DecelerationAmount", 0.0, 0.0);
    let mut recycle_box = [10.0; 3];
    em.vec("RecycleClippedBox", &mut recycle_box);

    // x: 3 = RecycleClipped (wrap into the box around the emitter), 1 = ParticlesFollowEmitter, 2 = follow with rotation
    // (ParticlesLocalSpace; the property exists in the game XML, its mapping to 2 is inferred), 0 = world space.
    k.ps_sim_following_emitter_params[0] = if em.b("RecycleClipped", false) {
        3.0
    } else if em.b("ParticlesLocalSpace", false) {
        2.0
    } else if em.b("ParticlesFollowEmitter", false) {
        1.0
    } else {
        0.0
    };

    k.ps_sim_following_emitter_params[1..].copy_from_slice(&recycle_box);
    k.ps_sim_turbulence_params[0] = em.f("TurbulenceStart", 0.0);
    k.ps_sim_turbulence_params[1] = em.f("TurbulenceEnd", 0.0);
    k.ps_sim_turbulence_params[2] = em.f("VectorFieldScale", 1.0);

    // The property exists in the game XML, always false there.
    k.ps_sim_game_logic_params[0] = flag_f(em.b("ParticlesFollowTerrain", false));

    let mut cull = [0.0, 0.0, 500.0, 550.0];
    em.vec("ParticleCullDistances", &mut cull);
    k.vs_culling_distances = cull;
    k.vs_atlas_texture_params[0] = 1.0;
    k.vs_atlas_texture_params[1] = 1.0;

    // Scale rows 0 and 1: ParticleScaleStart / Mid / End (x, y) over life. Verified in the game's baker (fn 0x182c3c0, reading the
    // emitter descriptor): ParticleScaleUseSingleScale makes Mid and End equal to Start (x and y stay separate), so the scale does not
    // change over the life; row 0 = the curve * (1 - |ParticleScaleVariation|), row 1 = the curve * (1 + |ParticleScaleVariation|); the
    // vertex shader blends the two rows by the particle's seed. The baker takes VFX_CURVE_WIDTH samples at t = i / width.
    let (mut s0, mut s1, mut s2) = ([1.0; 2], [1.0; 2], [1.0; 2]);
    em.vec("ParticleScaleStart", &mut s0);
    em.vec("ParticleScaleMid", &mut s1);
    em.vec("ParticleScaleEnd", &mut s2);
    if em.b("ParticleScaleUseSingleScale", false) {
        s1 = s0;
        s2 = s0;
    }

    let mid_t = em.f("ParticleScaleMidPointTime", 0.5);
    let variation = em.f("ParticleScaleVariation", 0.0).abs();
    for x in 0..VFX_CURVE_WIDTH {
        let t = x as f32 / VFX_CURVE_WIDTH as f32;
        for c in 0..2 {
            let s = scale_point(s0[c], s1[c], s2[c], mid_t, t);
            k.curves[0][x][c] = s * (1.0 - variation);
            k.curves[1][x][c] = s * (1.0 + variation);
        }

        for row in &mut k.curves[2..] {
            row[x] = [1.0; 4];
        }
    }

    let Some(chain) = chain else { return k };
    for m in &chain.modifiers {
        match m.fragment.as_str() {
            "vs_rotation" => {
                k.behaviour_mask[0] |= ROTATION;
                (k.vs_rotation_params[0], k.vs_rotation_params[1]) = m.range("initial_rotation", 0.0, 0.0);
                (k.vs_rotation_params[2], k.vs_rotation_params[3]) = m.range("rotation_over_life", 0.0, 0.0);
            }

            f @ ("vs_translation" | "vs_translation_velocity_v2") => {
                let v2 = f == "vs_translation_velocity_v2";
                k.behaviour_mask[0] |= if v2 { TRANSLATION_VELOCITY } else { TRANSLATION };
                k.vs_translation_params[0] = m.f("pivot_x", 0.5);
                k.vs_translation_params[1] = m.f("pivot_y", 0.5);
                k.vs_translation_params[2] = if v2 { m.i("orientation_mode", 0) } else { m.i("restriction", 0) } as f32;
                k.vs_translation_params[3] = if v2 { m.f("velocity_stretch_mul", 0.0) } else { 0.0 };
            }

            // Verified (snowflakes_loop, snow dump row 393): the GS reads the particle position as clip-space xy.
            "vs_translation_screen_space" => {
                k.behaviour_mask[0] |= TRANSLATION_SCREEN_SPACE;
                k.vs_translation_params[0] = m.f("pivot_x", 0.5);
                k.vs_translation_params[1] = m.f("pivot_y", 0.5);
            }

            // (sharp_transition, alpha_crush min, max, variation), row 393.
            "ps_alpha_crush" => {
                k.behaviour_mask[0] |= PS_ALPHA_CRUSH;
                k.ps_alpha_crush_params[0] = flag_f(m.b("sharp_transition", false));
                (k.ps_alpha_crush_params[1], k.ps_alpha_crush_params[2]) = m.range("alpha_crush", 0.0, 0.0);
                k.ps_alpha_crush_params[3] = m.f("variation", 0.0);
            }

            // ASSUMPTION: start -> mid until lerp_time, mid -> end after it; the min row uses the min colours and lerp time.
            "vs_colour" => {
                k.behaviour_mask[0] |= COLOUR;
                let (mut a0, mut a1, mut b0, mut b1, mut c0, mut c1) = ([1.0; 4], [1.0; 4], [1.0; 4], [1.0; 4], [1.0; 4], [1.0; 4]);
                m.colour("start_colour", &mut a0, &mut a1);
                m.colour("mid_colour", &mut b0, &mut b1);
                m.colour("end_colour", &mut c0, &mut c1);
                let (lt0, lt1) = m.range("lerp_time", 0.5, 0.5);
                for x in 0..VFX_CURVE_WIDTH {
                    let t = column_t(x);
                    for c in 0..4 {
                        k.curves[2][x][c] = three_point(a0[c], b0[c], c0[c], lt0, t);
                        k.curves[3][x][c] = three_point(a1[c], b1[c], c1[c], lt1, t);
                    }
                }
            }

            "vs_cell_texture" => {
                k.behaviour_mask[0] |= CELL_TEXTURE;
                atlas(&mut k, m.i("atlas_number_of_cells_x", 1).max(1), m.i("atlas_number_of_cells_y", 1).max(1), m.i("atlas_cell", 0));
                k.vs_cell_texture_params = [
                    m.i("number_of_cells_x", 1).max(1) as u32,
                    m.i("number_of_cells_y", 1).max(1) as u32,
                    m.i("cell", 0).max(0) as u32,
                    flag(m.b("randomize_cell", false)),
                ];
            }

            // Verified on sand_storm: atlas and cell rows as vs_cell_texture; params = (cell_blend, loop, row_restricted,
            // randomize_row) (the VS selects with each), params1 = (no_loops.x, no_loops.y, row_restriction).
            "vs_animated_texture" => {
                k.behaviour_mask[0] |= ANIMATED_TEXTURE;
                atlas(&mut k, m.i("atlas_number_of_cells_x", 1).max(1), m.i("atlas_number_of_cells_y", 1).max(1), m.i("atlas_cell", 0));
                let (mut cells, mut loops) = ([1.0; 2], [1.0; 2]);
                m.float_n("no_cells_xy", &mut cells);
                m.float_n("no_loops", &mut loops);
                k.vs_cell_texture_params = [cells[0].max(1.0) as u32, cells[1].max(1.0) as u32, 0, flag(m.b("randomize_cell", false))];
                k.vs_animated_texture_params = [
                    flag(m.b("cell_blend", false)),
                    flag(m.b("loop", false)),
                    flag(m.b("row_restricted", false)),
                    flag(m.b("randomize_row", false)),
                ];

                k.vs_animated_texture_params1[0] = loops[0];
                k.vs_animated_texture_params1[1] = loops[1];
                k.vs_animated_texture_params1[2] = m.i("row_restriction", 0) as f32;
            }

            "vs_vertex_alpha" => {
                k.behaviour_mask[0] |= VERTEX_ALPHA;
                let lo = m.curve("curve_alpha", "alpha_min");
                let hi = m.curve("curve_alpha", "alpha_max");
                for x in 0..VFX_CURVE_WIDTH {
                    let t = column_t(x);
                    let a = lo.map_or(1.0, |curve| curve.eval(t));
                    let b = hi.map_or(a, |curve| curve.eval(t));
                    k.curves[6][x] = [a; 4];
                    k.curves[7][x] = [b; 4];
                }
            }

            "ps_colour" => k.behaviour_mask[0] |= PS_COLOUR,
            "ps_texture" => {
                k.diffuse = vfx_texture_path(&m.text("diffuse"));
                k.normal = vfx_texture_path(&m.text("normal"));
                if !k.diffuse.is_empty() {
                    k.behaviour_mask[0] |= PS_TEXTURE;
                }

                if !k.normal.is_empty() {
                    k.behaviour_mask[0] |= PS_NORMAL;
                }
            }

            "ps_lighting" => {
                k.behaviour_mask[0] |= PS_LIGHTING;

                // legacy_lighting exists in the game XML, always false there.
                k.ps_lighting_legacy_params[0] = flag_f(m.b("legacy_lighting", false));
                k.ps_lighting_legacy_params[2] = m.f("emissive", 0.0);
                k.ps_lighting_legacy_params[3] = m.f("directionality", 0.0);

                // Verified (rain and snow dumps): the scattering path divides by small + large, so these must be set.
                k.ps_lighting_params =
                    [m.f("average_size", 0.0), m.f("additive_blend_mul", 0.0), m.f("small_particles", 0.0), m.f("large_particles", 0.0)];
                k.additive = m.b("additive_blend", false);
            }

            // The property name is verified in the game's vfx XML (soft_factor).
            "ps_soft_edge" => {
                k.behaviour_mask[0] |= PS_SOFT_EDGE;
                k.ps_soft_edge_params[0] = m.f("soft_factor", m.f("soft_edge_distance", 1.0));
            }

            "ps_fog" => k.behaviour_mask[0] |= PS_FOG,
            _ => {}
        }
    }

    k
}

#[cfg(test)]
mod tests {
    use super::vfx_bits::*;
    use super::*;
    use crate::vfx_def::{VfxFile, parse_vfx};
    use wce_formats::xml_doc::XmlDoc;

    const LAST: usize = VFX_CURVE_WIDTH - 1;

    fn vfx(text: &str) -> VfxFile {
        parse_vfx(&XmlDoc::parse(text.as_bytes().to_vec()).unwrap())
    }

    /// The constants of an emitter with the given properties, drawn with a chain of the given modifiers (none: no chain).
    fn constants(emitter: &str, modifiers: Option<&str>) -> EmitterConstants {
        let f = vfx(&format!(
            "<vfx><emitters><emitter id=\"e\" type=\"continuous_mesh\">{emitter}</emitter></emitters>\
             <behaviour_chains><behaviour_chain id=\"c\">{}</behaviour_chain></behaviour_chains></vfx>",
            modifiers.unwrap_or_default()
        ));
        build_emitter_constants(f.emitter("e").unwrap(), modifiers.and(f.chain("c")))
    }

    #[test]
    fn texture_paths() {
        assert_eq!(vfx_texture_path(""), "");
        assert_eq!(vfx_texture_path("Rain_Drop.DDS"), "vfx/textures/rain_drop.dds");
        assert_eq!(vfx_texture_path("VFX\\Other\\x.dds"), "vfx/other/x.dds");
        assert_eq!(vfx_texture_path("/x.dds"), "/x.dds");
    }

    #[test]
    fn segment_helpers() {
        assert_eq!((three_point(0.0, 1.0, 2.0, 0.5, 0.25), three_point(0.0, 1.0, 2.0, 0.5, 0.75)), (0.5, 1.5));
        // The colour curve clamps its middle time, so 0 still starts at the start.
        assert_eq!((three_point(5.0, 1.0, 2.0, 0.0, 0.0), three_point(0.0, 1.0, 5.0, 1.0, 1.0)), (5.0, 5.0));
        // The scale baker does not: 0 never uses the start, 1 never the end.
        assert_eq!((scale_point(5.0, 1.0, 2.0, 0.0, 0.0), scale_point(0.0, 2.0, 5.0, 1.0, 0.5)), (1.0, 1.0));
        assert_eq!((column_t(0), column_t(LAST)), (0.0, 1.0));
        assert_eq!((flag(true), flag(false), flag_f(true), flag_f(false)), (1, 0, 1.0, 0.0));
    }

    #[test]
    fn an_emitter_without_properties_or_chain_gets_the_defaults() {
        let k = constants("", None);
        assert_eq!(k.behaviour_mask, [0, 0, 0, VFX_MAIN_VIEW_ID]);
        assert_eq!(k.ps_sim_gravity_params, [0.0; 4]);
        assert_eq!(k.ps_sim_following_emitter_params, [0.0, 10.0, 10.0, 10.0]);
        assert_eq!(k.ps_sim_turbulence_params, [0.0, 0.0, 1.0, 0.0]);
        assert_eq!(k.vs_culling_distances, [0.0, 0.0, 500.0, 550.0]);
        assert_eq!(k.vs_atlas_texture_params, [1.0, 1.0, 0.0, 0.0]);
        assert!(k.curves[..2].iter().all(|row| row.iter().all(|c| *c == [1.0, 1.0, 0.0, 0.0])));
        assert!(k.curves[2..].iter().all(|row| row.iter().all(|c| *c == [1.0; 4])));
        assert!(k.diffuse.is_empty() && k.normal.is_empty() && !k.additive);

        // A chain without modifiers changes nothing.
        assert_eq!(constants("", Some("")), k);
    }

    #[test]
    fn simulation_parameters() {
        let k = constants(
            r#"<ranged_property name="GravityAmount" min="-9.8" max="-12"/><property name="EffectedByWindFactor" value="0.5"/>
               <property name="WindVarianceFactor" value="0.25"/><ranged_property name="DecelerationStartTime" min="1" max="2"/>
               <ranged_property name="DecelerationAmount" min="3" max="4"/><property name="Deceleration" value="false"/>
               <property name="RecycleClippedBox" x="5"/><property name="ParticlesLocalSpace" value="true"/>
               <property name="ParticlesFollowEmitter" value="true"/><property name="TurbulenceStart" value="1"/>
               <property name="TurbulenceEnd" value="2"/><property name="VectorFieldScale" value="0"/>
               <property name="ParticlesFollowTerrain" value="1"/><property name="ParticleCullDistances" x="10" y="20"/>"#,
            None,
        );
        assert_eq!(k.ps_sim_gravity_params, [-9.8, -12.0, 0.5, 0.25]);
        assert_eq!(k.ps_sim_deceleration_params, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(k.ps_sim_following_emitter_params, [2.0, 5.0, 10.0, 10.0]);
        assert_eq!(k.ps_sim_turbulence_params, [1.0, 2.0, 0.0, 0.0]);
        assert_eq!(k.ps_sim_game_logic_params[0], 1.0);
        assert_eq!(k.vs_culling_distances, [10.0, 20.0, 500.0, 550.0]);

        // RecycleClipped wins over the follow modes; following alone is 1.
        let k = constants(r#"<property name="RecycleClipped" value="1"/><property name="ParticlesLocalSpace" value="true"/>"#, None);
        assert_eq!(k.ps_sim_following_emitter_params[0], 3.0);
        let k = constants(r#"<property name="ParticlesFollowEmitter" value="true"/>"#, None);
        assert_eq!(k.ps_sim_following_emitter_params[0], 1.0);
    }

    #[test]
    fn scale_rows_follow_start_mid_and_end() {
        let k = constants(
            r#"<property name="ParticleScaleStart" x="1" y="2"/><property name="ParticleScaleMid" x="3" y="4"/>
               <property name="ParticleScaleEnd" x="5" y="6"/><property name="ParticleScaleVariation" value="-0.5"/>"#,
            None,
        );
        // A negative variation counts as positive: row 0 is the curve * 0.5, row 1 * 1.5. Column x samples t = x / 256.
        assert_eq!((&k.curves[0][0][..2], &k.curves[1][0][..2]), (&[0.5, 1.0][..], &[1.5, 3.0][..]));
        assert_eq!(k.curves[0][64][..2], [1.0, 1.5]);
        assert_eq!(k.curves[0][128][..2], [1.5, 2.0]);

        // Mid point at 0: no start at all.
        let k = constants(r#"<property name="ParticleScaleStart" x="9"/><property name="ParticleScaleMidPointTime" value="0"/>"#, None);
        assert_eq!(k.curves[0][0][0], 1.0);
    }

    #[test]
    fn rotation_translation_and_alpha_crush() {
        let k = constants(
            "",
            Some(
                r#"<modifier fragment="vs_rotation"><ranged_property name="initial_rotation" min="1" max="2"/>
                   <ranged_property name="rotation_over_life" min="-1" max="3"/></modifier>
                   <modifier fragment="vs_translation"><property name="pivot_x" value="0.25"/><property name="restriction" value="2"/></modifier>
                   <modifier fragment="ps_alpha_crush"><property name="sharp_transition" value="true"/>
                   <ranged_property name="alpha_crush" min="0.2" max="0.8"/><property name="variation" value="0.1"/></modifier>"#,
            ),
        );
        assert_eq!(k.behaviour_mask[0], ROTATION | TRANSLATION | PS_ALPHA_CRUSH);
        assert_eq!(k.vs_rotation_params, [1.0, 2.0, -1.0, 3.0]);
        assert_eq!(k.vs_translation_params, [0.25, 0.5, 2.0, 0.0]);
        assert_eq!(k.ps_alpha_crush_params, [1.0, 0.2, 0.8, 0.1]);

        let k = constants(
            "",
            Some(
                r#"<modifier fragment="vs_translation_velocity_v2"><property name="orientation_mode" value="3"/>
                   <property name="restriction" value="9"/><property name="velocity_stretch_mul" value="1.5"/></modifier>"#,
            ),
        );
        assert_eq!((k.behaviour_mask[0], k.vs_translation_params), (TRANSLATION_VELOCITY, [0.5, 0.5, 3.0, 1.5]));

        let k =
            constants("", Some(r#"<modifier fragment="vs_translation_screen_space"><property name="pivot_y" value="0.1"/></modifier>"#));
        assert_eq!((k.behaviour_mask[0], k.vs_translation_params), (TRANSLATION_SCREEN_SPACE, [0.5, 0.1, 0.0, 0.0]));
    }

    #[test]
    fn colour_and_alpha_rows() {
        let k = constants(
            "",
            Some(
                r#"<modifier fragment="vs_colour"><ranged_property name="start_colour" min="float4(0,0,0,0)" max="float4(1,1,1,1)"/>
                   <ranged_property name="end_colour" min="float4(1,0,0,1)" max="float4(0,1,0,1)"/></modifier>
                   <modifier fragment="vs_vertex_alpha"><property name="curve_alpha">alpha_min,1;0,0;1,1</property></modifier>"#,
            ),
        );
        assert_eq!(k.behaviour_mask[0], COLOUR | VERTEX_ALPHA);
        assert_eq!((k.curves[2][0], k.curves[3][0]), ([0.0; 4], [1.0; 4]));
        assert_eq!((k.curves[2][LAST], k.curves[3][LAST]), ([1.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 1.0]));
        // No alpha_max: the max row is the min row.
        assert_eq!((k.curves[6][0], k.curves[6][LAST], k.curves[7][LAST]), ([0.0; 4], [1.0; 4], [1.0; 4]));

        // A vertex alpha without its curve is opaque.
        let k = constants("", Some(r#"<modifier fragment="vs_vertex_alpha"/>"#));
        assert!(k.curves[6].iter().chain(k.curves[7].iter()).all(|c| *c == [1.0; 4]));
    }

    #[test]
    fn atlas_cells_and_animation() {
        let k = constants(
            "",
            Some(
                r#"<modifier fragment="vs_cell_texture"><property name="atlas_number_of_cells_x" value="4"/>
                   <property name="atlas_number_of_cells_y" value="2"/><property name="atlas_cell" value="5"/>
                   <property name="number_of_cells_x" value="0"/><property name="number_of_cells_y" value="3"/>
                   <property name="cell" value="-2"/><property name="randomize_cell" value="true"/></modifier>"#,
            ),
        );
        assert_eq!(k.behaviour_mask[0], CELL_TEXTURE);
        assert_eq!(k.vs_atlas_texture_params, [0.25, 0.5, 0.25, 0.5]);
        assert_eq!(k.vs_cell_texture_params, [1, 3, 0, 1]);

        let k = constants(
            "",
            Some(
                r#"<modifier fragment="vs_animated_texture"><property name="no_cells_xy" value="float2(8,4)"/>
                   <property name="no_loops" value="float2(2,0.5)"/><property name="cell_blend" value="true"/>
                   <property name="loop" value="1"/><property name="randomize_row" value="true"/>
                   <property name="row_restriction" value="3"/></modifier>"#,
            ),
        );
        assert_eq!(k.behaviour_mask[0], ANIMATED_TEXTURE);
        assert_eq!(k.vs_atlas_texture_params, [1.0, 1.0, 0.0, 0.0]);
        assert_eq!(k.vs_cell_texture_params, [8, 4, 0, 0]);
        assert_eq!(k.vs_animated_texture_params, [1, 1, 0, 1]);
        assert_eq!(k.vs_animated_texture_params1, [2.0, 0.5, 3.0, 0.0]);
    }

    #[test]
    fn bad_atlas_numbers_never_divide_by_zero() {
        for (x, y, cell) in [("0", "0", "0"), ("abc", "", "7"), ("-4", "-1", "-1"), ("2147483647", "1", "-2147483648")] {
            for fragment in ["vs_cell_texture", "vs_animated_texture"] {
                let k = constants(
                    "",
                    Some(&format!(
                        r#"<modifier fragment="{fragment}"><property name="atlas_number_of_cells_x" value="{x}"/>
                           <property name="atlas_number_of_cells_y" value="{y}"/><property name="atlas_cell" value="{cell}"/>
                           <property name="number_of_cells_x" value="{x}"/><property name="no_cells_xy" value="float2(-3,abc)"/></modifier>"#
                    )),
                );
                let atlas = k.vs_atlas_texture_params;
                assert!(atlas.iter().all(|v| v.is_finite()), "{fragment} {x} {y} {cell}: {atlas:?}");
                assert!(k.vs_cell_texture_params[0] >= 1 && k.vs_cell_texture_params[1] == 1, "{fragment} {x}");
            }
        }
    }

    #[test]
    fn pixel_shader_fragments() {
        let k = constants(
            "",
            Some(
                r#"<modifier fragment="ps_texture"><texture_property name="normal" path="N.dds"/></modifier>
                   <modifier fragment="ps_lighting"><property name="legacy_lighting" value="true"/><property name="emissive" value="0.5"/>
                   <property name="directionality" value="0.25"/><property name="average_size" value="1"/>
                   <property name="additive_blend_mul" value="2"/><property name="small_particles" value="3"/>
                   <property name="large_particles" value="4"/><property name="additive_blend" value="1"/></modifier>
                   <modifier fragment="ps_soft_edge"><property name="soft_edge_distance" value="2"/></modifier>
                   <modifier fragment="ps_colour"/><modifier fragment="ps_fog"/><modifier fragment="not_a_fragment"/><modifier/>"#,
            ),
        );
        assert_eq!(k.behaviour_mask[0], PS_NORMAL | PS_LIGHTING | PS_SOFT_EDGE | PS_COLOUR | PS_FOG);
        assert_eq!((k.diffuse.as_str(), k.normal.as_str()), ("", "vfx/textures/n.dds"));
        assert_eq!(k.ps_lighting_legacy_params, [1.0, 0.0, 0.5, 0.25]);
        assert_eq!(k.ps_lighting_params, [1.0, 2.0, 3.0, 4.0]);
        assert!(k.additive);
        assert_eq!(k.ps_soft_edge_params[0], 2.0);

        // soft_factor wins over soft_edge_distance, and the default is 1.
        let k = constants(
            "",
            Some(
                r#"<modifier fragment="ps_soft_edge"><property name="soft_factor" value="0.5"/><property name="soft_edge_distance" value="2"/></modifier>"#,
            ),
        );
        assert_eq!(k.ps_soft_edge_params[0], 0.5);
        let k = constants("", Some(r#"<modifier fragment="ps_soft_edge"/>"#));
        assert_eq!(k.ps_soft_edge_params[0], 1.0);
    }
}
