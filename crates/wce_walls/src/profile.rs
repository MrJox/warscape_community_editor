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

    /// The most one step section rises, and how long it is along the wall (the 8 m step piece's ramps: 2.5 over 3; the stepped bastion's
    /// ramp: 10 over about 12).
    pub step_rise: f64,
    pub step_run: f64,

    /// Between two chained step sections, flat (the 8 m step piece has 10 between its two ramps).
    pub step_gap: f64,

    /// The foot: below `foot.1` the mesh stretches so its bottom (`foot.0`) reaches `bury` under the terrain where the terrain is lower
    /// than the piece's level (8 m: the buried 2 m under y = 0; 15 m: the bottom 2 m of the plinth, which stands on y = 0).
    pub foot: (f64, f64),
    pub bury: f64,
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
    step_gap: 10.0,
    foot: (-2.0, 0.0),
    bury: 2.0,
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
    step_gap: 8.0,
    foot: (0.0, 2.0),
    bury: 0.5,
};

/// The bases of the first iteration (the game's and TDD's straights of both families; their walkways and frames checked in the logic
/// files, 2026-10-24), in the order the editor lists them.
pub const BASES: [Profile; 15] = [
    Profile { key: "barbarian_fort_straight", ..EIGHT },
    Profile { key: "eastern_fort_small_wall_straight", ..EIGHT },
    Profile { key: "sassanid_fort_small_wall_straight", ..EIGHT },
    Profile { key: "western_fort_small_wall_straight", ..EIGHT },
    Profile { key: "khazad_fort_wall_small_straight", ..EIGHT },
    Profile { key: "mordor_fort_small_wall_straight", ..EIGHT },
    Profile { key: "roman_fort_straight", ..FIFTEEN },
    Profile { key: "eastern_fort_straight", ..FIFTEEN },
    Profile { key: "sassanid_fort_straight", ..FIFTEEN },
    Profile { key: "greek_fort_straight", ..FIFTEEN },
    Profile { key: "egyptian_fort_straight", ..FIFTEEN },
    Profile { key: "persian_fort_straight", ..FIFTEEN },
    Profile { key: "easterling_fort_straight", ..FIFTEEN },
    Profile { key: "gondorean_fort_straight", ..FIFTEEN },
    Profile { key: "gondor_fort_straight", ..FIFTEEN },
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
