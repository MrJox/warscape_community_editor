//! What the generator knows of a base wall (docs/re/buildings.md, "The families" and "The kit table"): its family's frame,
//! walkway, outer face and how it changes height. In the piece's frame (scene units, which are the models' units): x along the wall, -z
//! the outer side, y = 0 the ground the piece stands on.

/// The two families of the first iteration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// Earth bank and palisade; units walk up the bank behind. Steps: the culture's `_step_up_a` / `_step_down_a`.
    Eight,

    /// Stone; units go up through the door sections. Steps: the ramp of the stepped bastion.
    Fifteen,
}

/// A base wall.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Profile {
    pub key: &'static str,
    pub family: Family,

    /// Where the frame starts and ends along x (the 8 m pieces run 0..20 from their origin, the 15 m ones -10..10).
    pub x: (f64, f64),

    /// The walkway's height over the piece's ground.
    pub walkway: f64,

    /// The outer face's line (z): the docking line of the `models_building` row, where ladders and towers meet the wall; the ground the
    /// walkway keeps its height over is measured along it.
    pub outer_face: f64,

    /// The footprint across the wall (z), the outer side first.
    pub depth: (f64, f64),

    /// The most one stair rises, and its length along the wall at its own slope (8 m: one stair of the step piece, 2.5 over 3; 15 m: the
    /// stepped bastion's wooden stair, 10 over 12). A step of a smaller rise uses the start of the stair, at its own slope and size (cut,
    /// never squashed); larger rises chain stairs.
    pub step_rise: f64,
    pub step_run: f64,

    /// Between two chained step sections, flat.
    pub step_gap: f64,

    /// The steepest the wall itself may slope over a step (rise per unit along the wall): 8 m walls follow their stair's own slope (only
    /// the grass bank slopes, the stair replaces the walkway and palisade); 15 m walls keep the stone gentle and the stair is lengthened
    /// to it.
    pub max_slope: f64,

    /// The shortest flat run between two steps (no pile-ups).
    pub min_run: f64,

    /// A climb of about this much over a short straight stretch is one whole step piece (15 m: the stepped bastion, 10 over its 20, as
    /// vanilla); 0: never.
    pub whole_rise: f64,

    /// The pieces that close a free end, at the start and at the end: the game's own, placed past the ends as they are (not bent nor
    /// fitted to the ground), their joining x (the end of their walkway that meets the wall) on the wall's end. 8 m: the endcaps (2.6,
    /// joining at 0); 15 m: the tower ends (a tower and the start of a walkway, joining at 10 and -10).
    pub cap_start: &'static str,
    pub cap_end: &'static str,
    pub cap_join: (f64, f64),

    /// The foot: below `foot.1` the mesh stretches so its bottom (`foot.0`) reaches `bury` under the terrain where the terrain is lower
    /// than the piece's level (8 m: the buried 2 m under y = 0; 15 m: the bottom 2 m of the plinth, which stands on y = 0).
    pub foot: (f64, f64),
    pub bury: f64,

    /// The culture's step pieces, going up and going down along +x.
    pub step_up: &'static str,
    pub step_down: &'static str,

    /// The step section of a step piece (its frame): the x range going up and going down, the walkway's height where it starts (up,
    /// down), what of it is brought (z then y; none: its whole cross-section), and whether it replaces the base over its length (both
    /// families: the step piece's own walls, bank and stair, authored to join the straight where they are flat) or is added to it.
    pub slab_up: (f64, f64),
    pub slab_down: (f64, f64),
    pub slab_walk: (f64, f64),
    pub slab_zy: Option<((f64, f64), (f64, f64))>,
    pub step_replaces: bool,

    /// Only what lies within this (y) of the stair's slope is brought (15 m: the treads and stringers, not the stair well's walls).
    pub slab_band: Option<f64>,

    /// The culture's door section (15 m: units go up through its doors; 8 m: none, the bank is the way up), and where its doors come
    /// out on the ground (the pipes' ground ends, the piece's frame: x, z).
    pub door: &'static str,
    pub door_at: (f64, f64),

    /// The culture's in-line towers and bastions (the same slot as a straight), used in turn at the wall's defence spacing: 15 m, the
    /// tower (`*_fort_tower_c_straight_with_extension`) then the bastion (`*_fort_straight_with_solid_squared_bastion`); 8 m, the bastion
    /// (`*_curved_bastion`; their watchtowers stand alone). Placed as the game's pieces, not bent nor fitted to the ground.
    pub defences: &'static [&'static str],
}

impl Profile {
    /// The length of one piece along the wall.
    pub fn length(&self) -> f64 {
        self.x.1 - self.x.0
    }
}

const EIGHT: Profile = Profile {
    key: "",
    family: Family::Eight,
    x: (0.0, 20.0),
    walkway: 8.0,
    outer_face: -2.5,
    depth: (-3.0, 12.0),
    step_rise: 2.5,
    step_run: 3.0,
    step_gap: 2.0,
    max_slope: 2.5 / 3.0,
    min_run: 10.0,
    whole_rise: 0.0,
    cap_start: "",
    cap_end: "",
    cap_join: (0.0, 0.0),
    foot: (-2.0, 0.0),
    bury: 2.0,
    step_up: "",
    step_down: "",
    slab_up: (2.0, 5.0),
    slab_down: (2.0, 5.0),
    slab_walk: (8.0, 13.0),
    slab_zy: Some(((-3.5, 3.0), (-3.0, 16.0))),
    step_replaces: true,
    slab_band: None,
    door: "",
    door_at: (0.0, 0.0),
    defences: &[],
};

const FIFTEEN: Profile = Profile {
    key: "",
    family: Family::Fifteen,
    x: (-10.0, 10.0),
    walkway: 15.0,
    outer_face: 0.0,
    depth: (-6.0, 16.0),
    step_rise: 10.0,
    step_run: 12.0,
    step_gap: 4.0,
    max_slope: 0.4,
    min_run: 20.0,
    whole_rise: 10.0,
    cap_start: "",
    cap_end: "",
    cap_join: (10.0, -10.0),
    foot: (0.0, 2.0),
    bury: 0.5,
    step_up: "",
    step_down: "",
    slab_up: (-5.8, 6.2),
    slab_down: (-6.2, 5.8),
    slab_walk: (15.0, 25.0),
    slab_zy: Some(((5.1, 8.9), (14.0, 26.5))),
    step_replaces: false,
    slab_band: Some(0.9),
    door: "",
    door_at: (0.0, 13.0),
    defences: &[],
};

/// The bases of the first iteration (the game's and TDD's straights of both families; their walkways and frames checked in the logic
/// files, 2026-10-24), in the order the editor lists them.
pub const BASES: [Profile; 15] = [
    Profile { key: "barbarian_fort_straight", defences: &["barbarian_fort_curved_bastion"], cap_start: "barbarian_fort_endcap_left", cap_end: "barbarian_fort_endcap_right", step_up: "barbarian_fort_step_up_a", step_down: "barbarian_fort_step_down_a", ..EIGHT },
    Profile { key: "eastern_fort_small_wall_straight", defences: &["eastern_fort_small_wall_curved_bastion"], cap_start: "eastern_fort_small_wall_endcap_left", cap_end: "eastern_fort_small_wall_endcap_right", step_up: "eastern_fort_small_wall_step_up_a", step_down: "eastern_fort_small_wall_step_down_a", ..EIGHT },
    Profile { key: "sassanid_fort_small_wall_straight", defences: &["sassanid_fort_small_wall_curved_bastion"], cap_start: "sassanid_fort_small_wall_endcap_left", cap_end: "sassanid_fort_small_wall_endcap_right", step_up: "sassanid_fort_small_wall_step_up_a", step_down: "sassanid_fort_small_wall_step_down_a", ..EIGHT },
    Profile { key: "western_fort_small_wall_straight", defences: &["western_fort_small_wall_curved_bastion"], cap_start: "western_fort_small_wall_endcap_left", cap_end: "western_fort_small_wall_endcap_right", step_up: "western_fort_small_wall_step_up_a", step_down: "western_fort_small_wall_step_down_a", ..EIGHT },
    Profile { key: "khazad_fort_wall_small_straight", defences: &["khazad_fort_wall_small_curved_bastion"], cap_start: "khazad_fort_wall_small_endcap_left", cap_end: "khazad_fort_wall_small_endcap_right", step_up: "khazad_fort_wall_small_step_up", step_down: "khazad_fort_wall_small_step_down", ..EIGHT },
    Profile { key: "mordor_fort_small_wall_straight", defences: &["mordor_fort_small_wall_curved_bastion"], cap_start: "mordor_fort_small_wall_endcap_left", cap_end: "mordor_fort_small_wall_endcap_right", step_up: "mordor_fort_small_wall_step_up_a", step_down: "mordor_fort_small_wall_step_down_a", ..EIGHT },
    Profile { key: "roman_fort_straight", defences: &["roman_fort_tower_c_straight_with_extension", "roman_fort_straight_with_solid_squared_bastion"], cap_start: "roman_fort_tower_end_left", cap_end: "roman_fort_tower_end_right", door: "roman_fort_straight_with_doors_a", step_up: "roman_fort_straight_stepped_up_10m_square_bastion_a", step_down: "roman_fort_straight_stepped_down_10m_square_bastion_a", ..FIFTEEN },
    Profile { key: "eastern_fort_straight", defences: &["eastern_fort_tower_c_straight_with_extension", "eastern_fort_straight_with_solid_squared_bastion"], cap_start: "eastern_fort_tower_end_left", cap_end: "eastern_fort_tower_end_right", door: "eastern_fort_straight_with_doors_a", step_up: "eastern_fort_straight_stepped_up_10m_square_bastion_a", step_down: "eastern_fort_straight_stepped_down_10m_square_bastion_a", ..FIFTEEN },
    Profile { key: "sassanid_fort_straight", defences: &["sassanid_fort_tower_c_straight_with_extension", "sassanid_fort_straight_with_solid_squared_bastion"], cap_start: "sassanid_fort_tower_end_left", cap_end: "sassanid_fort_tower_end_right", door: "sassanid_fort_straight_with_doors_a", step_up: "sassanid_fort_straight_stepped_up_10m_square_bastion_a", step_down: "sassanid_fort_straight_stepped_down_10m_square_bastion_a", ..FIFTEEN },
    Profile { key: "greek_fort_straight", defences: &["greek_fort_tower_c_straight_with_extension", "greek_fort_straight_with_solid_squared_bastion"], cap_start: "greek_fort_tower_end_left", cap_end: "greek_fort_tower_end_right", door: "greek_fort_straight_with_doors_a", step_up: "greek_fort_straight_stepped_up_10m_square_bastion_a", step_down: "greek_fort_straight_stepped_down_10m_square_bastion_a", ..FIFTEEN },
    Profile { key: "egyptian_fort_straight", defences: &["egyptian_fort_tower_c_straight_with_extension", "egyptian_fort_straight_with_solid_squared_bastion"], cap_start: "egyptian_fort_tower_end_left", cap_end: "egyptian_fort_tower_end_right", door: "egyptian_fort_straight_with_doors_a", step_up: "egyptian_fort_straight_stepped_up_10m_square_bastion_a", step_down: "egyptian_fort_straight_stepped_down_10m_square_bastion_a", ..FIFTEEN },
    Profile { key: "persian_fort_straight", defences: &["persian_fort_tower_c_straight_with_extension", "persian_fort_straight_with_solid_squared_bastion"], cap_start: "persian_fort_tower_end_left", cap_end: "persian_fort_tower_end_right", door: "persian_fort_straight_with_doors_a", step_up: "persian_fort_straight_stepped_up_10m_square_bastion_a", step_down: "persian_fort_straight_stepped_down_10m_square_bastion_a", ..FIFTEEN },
    Profile { key: "easterling_fort_straight", defences: &["easterling_fort_tower_c_straight_with_extension", "easterling_fort_straight_with_solid_squared_bastion"], cap_start: "easterling_fort_tower_end_left", cap_end: "easterling_fort_tower_end_right", door: "easterling_fort_straight_with_doors_a", step_up: "easterling_fort_straight_stepped_up_10m_square_bastion_a", step_down: "easterling_fort_straight_stepped_down_10m_square_bastion_a", ..FIFTEEN },
    Profile { key: "gondorean_fort_straight", defences: &["gondorean_fort_tower_c_straight_with_extension", "gondorean_fort_straight_with_solid_squared_bastion"], cap_start: "gondorean_fort_tower_end_left", cap_end: "gondorean_fort_tower_end_right", door: "gondorean_fort_straight_with_doors_a", step_up: "gondorean_fort_straight_stepped_up_10m_square_bastion_a", step_down: "gondorean_fort_straight_stepped_down_10m_square_bastion_a", ..FIFTEEN },
    Profile { key: "gondor_fort_straight", defences: &["gondor_fort_tower_c_straight", "gondor_fort_bastion"], cap_start: "gondor_fort_tower_end_left", cap_end: "gondor_fort_tower_end_right", door: "gondor_fort_straight_with_doors_a", step_up: "gondor_fort_straight_stepped_up_bastion_a", step_down: "gondor_fort_straight_stepped_down_bastion_a", ..FIFTEEN },
];

/// A base by its key.
pub fn profile(key: &str) -> Option<&'static Profile> {
    BASES.iter().find(|p| p.key.eq_ignore_ascii_case(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_editors_list_is_the_bases() {
        let keys = BASES.iter().map(|p| p.key).collect::<Vec<_>>();
        let listed = wce_tile::property_enums::WALL_BASE.iter().map(|(k, _)| *k).collect::<Vec<_>>();
        assert_eq!(keys, listed);
    }
}
