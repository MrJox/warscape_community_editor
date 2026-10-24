//! What the generator knows of a base wall (docs/plans/procedural_walls.md, "The wall families" and "The kit table"): its family's frame,
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

    /// What one step section rises (always exactly: nothing is stretched vertically), and how long it is along the wall (8 m: one stair of
    /// the step piece, 2.5 over 3; 15 m: the whole stepped bastion, 10 over 20).
    pub step_rise: f64,
    pub step_run: f64,

    /// Between two chained step sections, flat (the 8 m step piece has 10 between its two ramps).
    pub step_gap: f64,

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
    step_gap: 1.0,
    foot: (-2.0, 0.0),
    bury: 2.0,
    step_up: "",
    step_down: "",
    slab_up: (2.0, 5.0),
    slab_down: (2.0, 5.0),
    slab_walk: (8.0, 13.0),
    slab_zy: None,
    step_replaces: true,
    slab_band: None,
    door: "",
    door_at: (0.0, 0.0),
};

const FIFTEEN: Profile = Profile {
    key: "",
    family: Family::Fifteen,
    x: (-10.0, 10.0),
    walkway: 15.0,
    outer_face: 0.0,
    depth: (-6.0, 16.0),
    step_rise: 10.0,
    step_run: 20.0,
    step_gap: 0.0,
    foot: (0.0, 2.0),
    bury: 0.5,
    step_up: "",
    step_down: "",
    slab_up: (-10.0, 10.0),
    slab_down: (-10.0, 10.0),
    slab_walk: (15.0, 25.0),
    slab_zy: None,
    step_replaces: true,
    slab_band: None,
    door: "",
    door_at: (0.0, 13.0),
};

/// The bases of the first iteration (the game's and TDD's straights of both families; their walkways and frames checked in the logic
/// files, 2026-10-24), in the order the editor lists them.
pub const BASES: [Profile; 15] = [
    Profile { key: "barbarian_fort_straight", step_up: "barbarian_fort_step_up_a", step_down: "barbarian_fort_step_down_a", ..EIGHT },
    Profile { key: "eastern_fort_small_wall_straight", step_up: "eastern_fort_small_wall_step_up_a", step_down: "eastern_fort_small_wall_step_down_a", ..EIGHT },
    Profile { key: "sassanid_fort_small_wall_straight", step_up: "sassanid_fort_small_wall_step_up_a", step_down: "sassanid_fort_small_wall_step_down_a", ..EIGHT },
    Profile { key: "western_fort_small_wall_straight", step_up: "western_fort_small_wall_step_up_a", step_down: "western_fort_small_wall_step_down_a", ..EIGHT },
    Profile { key: "khazad_fort_wall_small_straight", step_up: "khazad_fort_wall_small_step_up", step_down: "khazad_fort_wall_small_step_down", ..EIGHT },
    Profile { key: "mordor_fort_small_wall_straight", step_up: "mordor_fort_small_wall_step_up_a", step_down: "mordor_fort_small_wall_step_down_a", ..EIGHT },
    Profile { key: "roman_fort_straight", door: "roman_fort_straight_with_doors_a", step_up: "roman_fort_straight_stepped_up_10m_square_bastion_a", step_down: "roman_fort_straight_stepped_down_10m_square_bastion_a", ..FIFTEEN },
    Profile { key: "eastern_fort_straight", door: "eastern_fort_straight_with_doors_a", step_up: "eastern_fort_straight_stepped_up_10m_square_bastion_a", step_down: "eastern_fort_straight_stepped_down_10m_square_bastion_a", ..FIFTEEN },
    Profile { key: "sassanid_fort_straight", door: "sassanid_fort_straight_with_doors_a", step_up: "sassanid_fort_straight_stepped_up_10m_square_bastion_a", step_down: "sassanid_fort_straight_stepped_down_10m_square_bastion_a", ..FIFTEEN },
    Profile { key: "greek_fort_straight", door: "greek_fort_straight_with_doors_a", step_up: "greek_fort_straight_stepped_up_10m_square_bastion_a", step_down: "greek_fort_straight_stepped_down_10m_square_bastion_a", ..FIFTEEN },
    Profile { key: "egyptian_fort_straight", door: "egyptian_fort_straight_with_doors_a", step_up: "egyptian_fort_straight_stepped_up_10m_square_bastion_a", step_down: "egyptian_fort_straight_stepped_down_10m_square_bastion_a", ..FIFTEEN },
    Profile { key: "persian_fort_straight", door: "persian_fort_straight_with_doors_a", step_up: "persian_fort_straight_stepped_up_10m_square_bastion_a", step_down: "persian_fort_straight_stepped_down_10m_square_bastion_a", ..FIFTEEN },
    Profile { key: "easterling_fort_straight", door: "easterling_fort_straight_with_doors_a", step_up: "easterling_fort_straight_stepped_up_10m_square_bastion_a", step_down: "easterling_fort_straight_stepped_down_10m_square_bastion_a", ..FIFTEEN },
    Profile { key: "gondorean_fort_straight", door: "gondorean_fort_straight_with_doors_a", step_up: "gondorean_fort_straight_stepped_up_10m_square_bastion_a", step_down: "gondorean_fort_straight_stepped_down_10m_square_bastion_a", ..FIFTEEN },
    Profile { key: "gondor_fort_straight", door: "gondor_fort_straight_with_doors_a", step_up: "gondor_fort_straight_stepped_up_bastion_a", step_down: "gondor_fort_straight_stepped_down_bastion_a", ..FIFTEEN },
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
