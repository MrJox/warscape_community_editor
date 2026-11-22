//! What a river does to the terrain (docs/plans/rivers.md): the ground at its river links, shaped as the export shapes it, and (step 2) the
//! bed it cuts along its line. No map here: a [`RiverBed`] gives a height for a place over the ground below it, and the document writes
//! that into the layer the river owns. The same inputs give the same heights, so the layer is derived data.
//!
//! **The connections** (docs/re/tile_links.md, "Rivers at their links"): the export writes the linking tile's cross-section along a river
//! link's cell whatever the tile has there (`river/1x1_blend`, `river_shallow/1x1_blend`), and every kit river meets it with the same water
//! height and width. The tables are the medians of the kit's 123 deep and 46 shallow river ends' `final_heights.dds`, every 8 m across the
//! cell's centre: the same either side, so one side is kept.
//!
//! World metres throughout (the tile's x, z; heights as `TileProject::height_at`).

/// A standard river connection: the water and width a river has at the link, and the ground across the link's cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Connection {
    /// The link set's kind, for messages: `river` or `river_shallow`.
    pub name: &'static str,

    /// The water's height at the link (the river file's y, which is the game's).
    pub water: f64,

    /// The river's width there (TEd's `width`, the ribbon's full width in metres).
    pub width: f64,

    /// The ground from the cell's centre out, every [`PROFILE_STEP`] metres: `profile[i]` at `i * PROFILE_STEP` either side.
    pub profile: [f64; 9],
}

/// The spacing of a connection's profile (metres).
pub const PROFILE_STEP: f64 = 8.0;

/// A deep river's connection (`river`, and the `river_*` families that link with it: crossings, fords, starts; the settlements' rivers
/// have their water at -7.0 instead, a step the kit lives with).
pub const DEEP: Connection =
    Connection { name: "river", water: -6.2, width: 45.0, profile: [-11.9, -11.6, -6.8, -3.9, -2.4, -1.2, -0.5, -0.1, 0.0] };

/// A shallow river's (`river_shallow`, `river_start_shallow`, `river_mouth_shallow`).
pub const SHALLOW: Connection =
    Connection { name: "river_shallow", water: 0.42, width: 45.0, profile: [-1.0, -1.0, -0.2, 0.1, 0.2, 0.2, 0.1, 0.0, 0.0] };

/// The connection of a link target's or rule's tile set, if it is a river's.
pub fn connection_of(tile_set: &str) -> Option<&'static Connection> {
    if !tile_set.contains("river") {
        None
    } else if tile_set.contains("shallow") {
        Some(&SHALLOW)
    } else {
        Some(&DEEP)
    }
}

impl Connection {
    /// The ground `off` metres from the cell's centre along the edge (either side): the profile, its last value past it.
    pub fn ground_across(&self, off: f64) -> f64 {
        let t = off.abs() / PROFILE_STEP;
        let i = t.floor() as usize;
        if i + 1 >= self.profile.len() {
            return self.profile[self.profile.len() - 1];
        }

        let f = t - i as f64;
        self.profile[i] + (self.profile[i + 1] - self.profile[i]) * f
    }
}

/// How far in from the edge a join holds the connection whole (metres): the export's blend zone, so that the export leaves it as it is.
pub const JOIN_HELD: f64 = 64.0;

/// How far past that a join fades into what lies beyond (metres).
pub const JOIN_FADE: f64 = 64.0;

/// Half the width a join holds along the edge (the link's cell), and how far past that it fades sideways (metres).
pub const JOIN_HALF: f64 = 64.0;
pub const JOIN_SIDE_FADE: f64 = 32.0;

/// A river's join to a link: where the link's cell meets the edge (its centre on the edge's line), the direction into the tile, the
/// connection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Join {
    pub at: [f64; 2],
    pub inward: [f64; 2],
    pub connection: Connection,
}

fn smooth(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Join {
    /// The connection's ground at a place and how much of it holds there (1 over the held zone, fading to 0), none outside the join. The
    /// border past the edge is held too (the height map's overlap).
    pub fn at(&self, x: f64, z: f64) -> Option<(f64, f64)> {
        let (dx, dz) = (x - self.at[0], z - self.at[1]);
        let d = dx * self.inward[0] + dz * self.inward[1];
        let along = dx * -self.inward[1] + dz * self.inward[0];
        if d > JOIN_HELD + JOIN_FADE || along.abs() > JOIN_HALF + JOIN_SIDE_FADE {
            return None;
        }

        let w_in = if d <= JOIN_HELD { 1.0 } else { 1.0 - smooth((d - JOIN_HELD) / JOIN_FADE) };
        let w_side = if along.abs() <= JOIN_HALF { 1.0 } else { 1.0 - smooth((along.abs() - JOIN_HALF) / JOIN_SIDE_FADE) };
        let w = w_in * w_side;
        (w > 0.0).then(|| (self.connection.ground_across(along), w))
    }

    /// The bounds it touches (x0, z0, x1, z1).
    pub fn bounds(&self) -> [f64; 4] {
        let r = (JOIN_HELD + JOIN_FADE).hypot(JOIN_HALF + JOIN_SIDE_FADE);
        [self.at[0] - r, self.at[1] - r, self.at[0] + r, self.at[1] + r]
    }
}

/// What a river does to the ground: its joins (and, step 2, its bed).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RiverBed {
    pub joins: Vec<Join>,
}

impl RiverBed {
    /// The ground at a place over `below` (what lies under the river's layer), none where the river leaves it as it is.
    pub fn height_at(&self, x: f64, z: f64, below: f64) -> Option<f64> {
        let mut out: Option<f64> = None;
        for j in &self.joins {
            if let Some((g, w)) = j.at(x, z) {
                let base = out.unwrap_or(below);
                out = Some(if w >= 1.0 { g } else { base + (g - base) * w });
            }
        }

        out
    }

    /// The bounds of what it touches (x0, z0, x1, z1), none when nothing.
    pub fn bounds(&self) -> Option<[f64; 4]> {
        self.joins.iter().map(Join::bounds).reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_connections_are_the_link_sets() {
        assert_eq!(connection_of("river").map(|c| c.name), Some("river"));
        assert_eq!(connection_of("river_crossing").map(|c| c.name), Some("river"));
        assert_eq!(connection_of("river_shallow").map(|c| c.name), Some("river_shallow"));
        assert_eq!(connection_of("roads"), None);
    }

    #[test]
    fn the_profile_is_the_same_either_side_and_flat_past_its_end() {
        assert_eq!(DEEP.ground_across(0.0), -11.9);
        assert!((DEEP.ground_across(12.0) - DEEP.ground_across(-12.0)).abs() < 1e-12);
        assert!((DEEP.ground_across(12.0) - (-11.6 + (-6.8 + 11.6) * 0.5)).abs() < 1e-9);
        assert_eq!(DEEP.ground_across(200.0), 0.0);
        assert_eq!(SHALLOW.ground_across(4.0), -1.0);
    }

    #[test]
    fn a_join_holds_the_connection_then_fades() {
        // A link at the west edge (x 0), its cell's centre at z 192, the tile to the east.
        let j = Join { at: [0.0, 192.0], inward: [1.0, 0.0], connection: DEEP };
        let bed = RiverBed { joins: vec![j] };
        assert_eq!(bed.height_at(10.0, 192.0, 5.0), Some(-11.9));
        assert_eq!(bed.height_at(-4.0, 192.0, 5.0), Some(-11.9), "the border past the edge");
        assert_eq!(bed.height_at(64.0, 192.0 + 16.0, 5.0), Some(-6.8));
        let half = bed.height_at(96.0, 192.0, 5.0).unwrap();
        assert!((half - (5.0 + (-11.9 - 5.0) * 0.5)).abs() < 1e-9, "halfway through the fade: {half}");
        assert_eq!(bed.height_at(129.0, 192.0, 5.0), None);
        assert_eq!(bed.height_at(10.0, 192.0 + 97.0, 5.0), None);
        let side = bed.height_at(10.0, 192.0 + 80.0, 5.0).unwrap();
        assert!(side > 0.0 && side < 5.0, "fading sideways: {side}");
    }
}
