//! What a river does to the terrain (docs/plans/rivers.md): the ground at its river links, shaped as the export shapes it, and (step 2) the
//! bed it cuts along its line. No map here: a [`RiverBed`] gives a height for a place over the ground below it, and the document writes
//! that into the layer the river owns. The same inputs give the same heights, so the layer is derived data.
//!
//! **The connections** (docs/re/tile_links.md, "Rivers at their links"): the export writes the linking tile's cross-section along a river
//! link's cell whatever the tile has there (`river/1x1_blend`, `river_shallow/1x1_blend`), and every kit river meets it with the same water
//! height and width. The tables are the medians of the kit's 123 deep and 46 shallow river ends' `final_heights.dds`, every 8 m across the
//! cell's centre: the same either side, so one side is kept.
//!
//! Across in the tile's metres (x, z: the editor's), heights in the game's units (`TileProject::height_at`, the river files' y, TEd's
//! scene heights): twice the editor's metres in a battle tile (`Tile::terrain_height` is `height_at` over the unit).

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

/// How far in from the edge a join holds the connection whole (metres): where the export's blend is mostly the connection (its weight
/// of the tile's heights is under 0.2 there, docs/re/battle_maps.md).
pub const JOIN_HELD: f64 = 16.0;

/// How far past that a join fades into what lies beyond (metres): to the end of the export's blend zone (64 m). Held further, it ran
/// straight on where a river bends away, and the river's water stood over its deep middle; a river's own bed, the same profile, follows
/// the river from there.
pub const JOIN_FADE: f64 = 48.0;

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

/// The kit's deep river across its bed, relative to its water, every [`PROFILE_STEP`] metres from the centre: [`DEEP`]'s profile less
/// its water. A river's bed is this, scaled across by its width over [`DEEP`]'s and down by its depth over [`KIT_DEPTH`].
pub const KIT_BED: [f64; 9] = [-5.7, -5.4, -0.6, 2.3, 3.8, 5.0, 5.7, 6.1, 6.2];

/// The kit's deep river's depth: its water over its bed's middle.
pub const KIT_DEPTH: f64 = 5.7;

/// The bed's settings: how deep under the water its middle is (metres), how steep its banks climb past the kit's profile (degrees).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BedSettings {
    pub depth: f64,
    pub bank_slope: f64,
}

impl BedSettings {
    /// The depth a river of a width gets unless set: the kit's, in proportion below its width (a 5 m creek 0.6 m), 0.5 .. 8 m.
    pub fn default_depth(width: f64) -> f64 {
        (KIT_DEPTH * width / DEEP.width).clamp(0.5, 8.0)
    }

    pub const DEFAULT_BANK_SLOPE: f64 = 30.0;
}

/// How far the banks climb past the profile before the bed stops (metres): the ground is met before that but on cliffs.
const BANK_RISE: f64 = 60.0;

/// A ford: a stretch of a river's bed raised to a wading depth (the kit's fords: 0.5 of the game's height units, 0.25 m in the
/// editor), its banks eased to [`FORD_BANK_SLOPE`]: where along the river its middle is (metres), how long it is held, its depth.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ford {
    pub at: f64,
    pub length: f64,
    pub depth: f64,
}

/// The kit's fords' water over their bed (the game's height units; `river_ford/*`: 0.36 .. 0.5).
pub const FORD_DEPTH: f64 = 0.5;

/// How steep a ford's banks climb (degrees): units walk down to it.
pub const FORD_BANK_SLOPE: f64 = 12.0;

/// A river's bed along its line: the centre line's samples (x, z), the water there (the game's units) and the width, and its settings
/// (the depth in the game's units); `vertical`: the game's height units a metre across (the tile's unit), for the banks' slope; its fords.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Channel {
    pub line: Vec<[f64; 2]>,
    pub water: Vec<f64>,
    pub width: Vec<f64>,
    pub depth: f64,
    pub bank_slope: f64,
    pub vertical: f64,
    pub fords: Vec<Ford>,

    /// Whether it cuts its bed along its whole line; else only its fords change the ground.
    pub cuts: bool,
}

/// A place of a bed's grid ([`Channel::fill_cells`]): the lowest bed over it (infinite where none reaches), how much of a ford is there
/// (at the nearest place of the line) and whether it is under the river's water (within half its width of its line), and how far the
/// line is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BedCell {
    pub bed: f64,
    pub ford: f64,
    pub wet: bool,
    pub near: f64,
}

impl Default for BedCell {
    fn default() -> Self {
        Self { bed: f64::INFINITY, ford: 0.0, wet: false, near: f64::INFINITY }
    }
}

impl Channel {
    /// The bed's height `r` metres from the centre where the water is `water` and the river `width` wide.
    pub fn across(&self, r: f64, water: f64, width: f64) -> f64 {
        self.across_at(r, water, width, f64::NAN)
    }

    /// How much of a ford is at a place along the river (0 .. 1: held over its length, fading over half of it more each side), and its
    /// depth.
    pub fn ford_at(&self, along: f64) -> (f64, f64) {
        self.fords
            .iter()
            .map(|f| {
                let d = (along - f.at).abs() - f.length * 0.5;
                let ramp = (f.length * 0.5).max(10.0);
                let t = (1.0 - d / ramp).clamp(0.0, 1.0);
                (if d <= 0.0 { 1.0 } else { t * t * (3.0 - 2.0 * t) }, f.depth)
            })
            .fold((0.0, self.depth), |a, b| if b.0 > a.0 { b } else { a })
    }

    /// [`Self::across`] at a place along the river (metres; NaN: no fords counted).
    pub fn across_at(&self, r: f64, water: f64, width: f64, along: f64) -> f64 {
        let k = (width / DEEP.width).max(0.05);
        let (w, ford_depth) = if along.is_nan() { (0.0, self.depth) } else { self.ford_at(along) };
        let depth = self.depth + (ford_depth - self.depth) * w;
        let rise = self.rise() + (Self::rise_of(FORD_BANK_SLOPE, self.vertical) - self.rise()) * w;
        let scale = depth / KIT_DEPTH;
        let t = r.abs() / (PROFILE_STEP * k);
        let last = KIT_BED.len() - 1;
        if t >= last as f64 {
            let past = r.abs() - last as f64 * PROFILE_STEP * k;
            return water + KIT_BED[last] * scale + past * rise;
        }

        let i = t.floor() as usize;
        let f = t - i as f64;
        water + (KIT_BED[i] + (KIT_BED[i + 1] - KIT_BED[i]) * f) * scale
    }

    /// How far from the centre it reaches at a width (metres): a ford's gentler banks reach further.
    pub fn reach(&self, width: f64) -> f64 {
        let k = (width / DEEP.width).max(0.05);
        let rise = if self.fords.is_empty() { self.rise() } else { self.rise().min(Self::rise_of(FORD_BANK_SLOPE, self.vertical)) };
        (KIT_BED.len() - 1) as f64 * PROFILE_STEP * k + BANK_RISE / rise
    }

    /// How far the banks climb a metre across (the game's units).
    fn rise(&self) -> f64 {
        Self::rise_of(self.bank_slope, self.vertical)
    }

    fn rise_of(slope: f64, vertical: f64) -> f64 {
        slope.to_radians().tan().max(0.05) * if vertical > 0.0 { vertical } else { 1.0 }
    }

    /// The bounds it touches (x0, z0, x1, z1), none without a line.
    pub fn bounds(&self) -> Option<[f64; 4]> {
        let reach = self.width.iter().copied().fold(0.0, f64::max);
        let reach = self.reach(reach);
        self.line
            .iter()
            .map(|p| [p[0] - reach, p[1] - reach, p[0] + reach, p[1] + reach])
            .reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])
    }

    /// The bed over a grid (`w` x `h` places, the first at `origin`, `step` apart in x and z): for each place the lowest of the bed
    /// across each segment of the line (the place's distance from the segment, the water and width there), infinite where none reaches.
    pub fn fill(&self, origin: [f64; 2], step: [f64; 2], size: (usize, usize)) -> Vec<f64> {
        self.fill_cells(origin, step, size).into_iter().map(|c| c.bed).collect()
    }

    /// [`Self::fill`] with the fords: each place's lowest bed, the most of a ford over it, and whether it is under the water.
    pub fn fill_cells(&self, origin: [f64; 2], step: [f64; 2], (w, h): (usize, usize)) -> Vec<BedCell> {
        let mut out = vec![BedCell::default(); w * h];
        if self.line.len() < 2 {
            return out;
        }

        // How far along each sample is (for the fords).
        let mut along = vec![0.0; self.line.len()];
        for i in 1..self.line.len() {
            along[i] = along[i - 1] + (self.line[i][0] - self.line[i - 1][0]).hypot(self.line[i][1] - self.line[i - 1][1]);
        }

        for i in 0..self.line.len() - 1 {
            let (a, b) = (self.line[i], self.line[i + 1]);
            let reach = self.reach(self.width[i].max(self.width[i + 1]));
            let (lo_x, hi_x) = (a[0].min(b[0]) - reach, a[0].max(b[0]) + reach);
            let (lo_z, hi_z) = (a[1].min(b[1]) - reach, a[1].max(b[1]) + reach);
            let col = |v: f64| ((v - origin[0]) / step[0]).clamp(0.0, w as f64) as usize;
            let row = |v: f64| ((v - origin[1]) / step[1]).clamp(0.0, h as f64) as usize;
            let ab = [b[0] - a[0], b[1] - a[1]];
            let len2 = ab[0] * ab[0] + ab[1] * ab[1];
            for j in row(lo_z)..(row(hi_z) + 1).min(h) {
                for k in col(lo_x)..(col(hi_x) + 1).min(w) {
                    let p = [origin[0] + k as f64 * step[0], origin[1] + j as f64 * step[1]];
                    let t = if len2 > 1e-12 { (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / len2).clamp(0.0, 1.0) } else { 0.0 };
                    let r = (p[0] - a[0] - ab[0] * t).hypot(p[1] - a[1] - ab[1] * t);
                    let width = self.width[i] + (self.width[i + 1] - self.width[i]) * t;
                    if r > self.reach(width) {
                        continue;
                    }

                    let at = along[i] + (along[i + 1] - along[i]) * t;
                    let water = self.water[i] + (self.water[i + 1] - self.water[i]) * t;
                    let v = self.across_at(r, water, width, at);
                    let c = &mut out[j * w + k];
                    if v < c.bed {
                        c.bed = v;
                    }

                    // The ford and the water of the nearest place of the line.
                    if r < c.near {
                        c.near = r;
                        c.ford = if self.fords.is_empty() { 0.0 } else { self.ford_at(at).0 };
                        c.wet = r <= width * 0.5;
                    }
                }
            }
        }

        out
    }
}

/// A lake's basin: dug inside its outline (x, z) to `depth` under its `water` (the game's units), its sides climbing `rise` a metre in
/// from the outline (the game's units).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Basin {
    pub outline: Vec<[f64; 2]>,
    pub water: f64,
    pub depth: f64,
    pub rise: f64,
}

impl Basin {
    /// The basin's floor at a place: inside the outline, the water less the depth, or less the distance from the outline times the rise
    /// nearer its edge; none outside.
    pub fn floor(&self, x: f64, z: f64) -> Option<f64> {
        let n = self.outline.len();
        if n < 3 {
            return None;
        }

        let mut inside = false;
        let mut near = f64::INFINITY;
        for i in 0..n {
            let (a, b) = (self.outline[i], self.outline[(i + n - 1) % n]);
            if (a[1] > z) != (b[1] > z) && x < (b[0] - a[0]) * (z - a[1]) / (b[1] - a[1]) + a[0] {
                inside = !inside;
            }

            let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
            let l2 = dx * dx + dz * dz;
            let t = if l2 > 1e-12 { (((x - a[0]) * dx + (z - a[1]) * dz) / l2).clamp(0.0, 1.0) } else { 0.0 };
            near = near.min((x - a[0] - dx * t).hypot(z - a[1] - dz * t));
        }

        inside.then(|| self.water - self.depth.min(near * self.rise))
    }

    fn bounds(&self) -> Option<[f64; 4]> {
        self.outline
            .iter()
            .map(|p| [p[0], p[1], p[0], p[1]])
            .reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])
    }
}

/// What a river does to the ground: the bed it cuts along its line (when it has one) and its joins to the tile's river links, laid over
/// the bed; or a lake's basin.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RiverBed {
    pub channel: Option<Channel>,
    pub joins: Vec<Join>,
    pub basin: Option<Basin>,
}

impl RiverBed {
    /// The ground at a place over `below` (what lies under the river's layer) and the bed there (`bed`: [`Channel::fill`]'s value,
    /// infinite where the bed does not reach), none where the river leaves it as it is. The bed only cuts.
    pub fn height_with(&self, x: f64, z: f64, below: f64, bed: f64) -> Option<f64> {
        self.height_of(x, z, below, BedCell { bed, ..Default::default() })
    }

    /// [`Self::height_with`] with a place's fords ([`Channel::fill_cells`]): a river that cuts its bed cuts it; a ford pulls the ground
    /// under the water to its bed (raised or cut) and cuts its eased banks; a river that cuts no bed changes the ground at its fords only.
    pub fn height_of(&self, x: f64, z: f64, below: f64, cell: BedCell) -> Option<f64> {
        let cuts = self.channel.as_ref().is_some_and(|c| c.cuts);
        let bed = cell.bed;
        let mut out = if cuts { (bed < below).then_some(bed) } else { None };
        if cell.ford > 0.0 && bed.is_finite() {
            let base = out.unwrap_or(below);
            let target = if cell.wet { bed } else { bed.min(base) };
            out = Some(base + (target - base) * cell.ford);
        }

        // A lake's basin only digs.
        if let Some(f) = self.basin.as_ref().and_then(|b| b.floor(x, z)) {
            let base = out.unwrap_or(below);
            if f < base {
                out = Some(f);
            }
        }

        for j in &self.joins {
            if let Some((g, w)) = j.at(x, z) {
                let base = out.unwrap_or(below);
                out = Some(if w >= 1.0 { g } else { base + (g - base) * w });
            }
        }

        out
    }

    /// [`Self::height_with`] without a bed (the joins alone).
    pub fn height_at(&self, x: f64, z: f64, below: f64) -> Option<f64> {
        self.height_with(x, z, below, f64::INFINITY)
    }

    /// The bounds of what it touches (x0, z0, x1, z1), none when nothing.
    pub fn bounds(&self) -> Option<[f64; 4]> {
        self.joins
            .iter()
            .map(Join::bounds)
            .chain(self.channel.as_ref().and_then(Channel::bounds))
            .chain(self.basin.as_ref().and_then(Basin::bounds))
            .reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])
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
        let bed = RiverBed { joins: vec![j], ..Default::default() };
        assert_eq!(bed.height_at(10.0, 192.0, 5.0), Some(-11.9));
        assert_eq!(bed.height_at(-4.0, 192.0, 5.0), Some(-11.9), "the border past the edge");
        assert_eq!(bed.height_at(12.0, 192.0 + 16.0, 5.0), Some(-6.8));
        let half = bed.height_at(40.0, 192.0, 5.0).unwrap();
        assert!((half - (5.0 + (-11.9 - 5.0) * 0.5)).abs() < 1e-9, "halfway through the fade: {half}");
        assert_eq!(bed.height_at(65.0, 192.0, 5.0), None);
        assert_eq!(bed.height_at(10.0, 192.0 + 97.0, 5.0), None);
        let side = bed.height_at(10.0, 192.0 + 80.0, 5.0).unwrap();
        assert!(side > 0.0 && side < 5.0, "fading sideways: {side}");
    }

    #[test]
    fn a_bed_is_the_kits_scaled_and_its_banks_climb_to_the_ground() {
        let c = Channel {
            line: vec![[0.0, 0.0], [100.0, 0.0]],
            water: vec![-6.2, -6.2],
            width: vec![45.0, 45.0],
            depth: 5.7,
            bank_slope: 45.0,
            vertical: 1.0,
            fords: vec![],
            cuts: true,
        };
        // The kit's river: its profile.
        assert!((c.across(0.0, -6.2, 45.0) - DEEP.profile[0]).abs() < 0.11);
        assert!((c.across(24.0, -6.2, 45.0) - DEEP.profile[3]).abs() < 0.11);
        assert!((c.across(64.0, -6.2, 45.0) - 0.0).abs() < 0.11);
        assert!((c.across(74.0, -6.2, 45.0) - 10.0).abs() < 0.11, "past it, the bank at 45 degrees");

        // Half as wide and half as deep: the same shape, halved.
        let half = Channel { depth: 2.85, ..c.clone() };
        assert!((half.across(12.0, 0.0, 22.5) - 2.3 * 0.5).abs() < 1e-9);

        // On a grid: lowest along the line, the ground left where it is lower.
        let g = c.fill([0.0, -100.0], [10.0, 10.0], (11, 21));
        assert!((g[10 * 11 + 5] + 11.9).abs() < 0.11, "the middle of the bed");
        assert!(g[0].is_infinite() || g[0] > 30.0, "far off: {}", g[0]);
        let bed = RiverBed { channel: Some(Channel { cuts: true, ..c }), ..Default::default() };
        assert_eq!(bed.height_with(50.0, 0.0, -20.0, -11.9), None, "lower ground stays");
        assert_eq!(bed.height_with(50.0, 0.0, 3.0, -11.9), Some(-11.9));
    }

    #[test]
    fn a_ford_raises_the_bed_to_its_depth_and_fades_out() {
        let c = Channel {
            line: vec![[0.0, 0.0], [300.0, 0.0]],
            water: vec![-6.2, -6.2],
            width: vec![45.0, 45.0],
            depth: 5.7,
            bank_slope: 30.0,
            vertical: 1.0,
            fords: vec![Ford { at: 150.0, length: 40.0, depth: FORD_DEPTH }],
            cuts: false,
        };
        assert!((c.across_at(0.0, -6.2, 45.0, 150.0) - (-6.2 + KIT_BED[0] * FORD_DEPTH / KIT_DEPTH)).abs() < 1e-9, "the ford's bed");
        assert!((c.across_at(0.0, -6.2, 45.0, 10.0) - (-6.2 + KIT_BED[0])).abs() < 1e-9, "the river's own away from it");
        let mid = c.across_at(0.0, -6.2, 45.0, 180.0);
        assert!(mid > -6.2 + KIT_BED[0] && mid < -6.2 + KIT_BED[0] * FORD_DEPTH / KIT_DEPTH, "fading: {mid}");
        let g = c.fill_cells([140.0, 0.0], [10.0, 1.0], (3, 1));
        assert!(g[1].bed > -7.0 && g[1].bed < -6.2 && g[1].ford == 1.0 && g[1].wet, "on the grid: {g:?}");

        // A river that cuts no bed: a kit bed at -12 filled to the ford's, nothing away from it.
        let bed = RiverBed { channel: Some(c.clone()), ..Default::default() };
        assert_eq!(bed.height_of(150.0, 0.0, -12.0, g[1]), Some(g[1].bed));
        let far = c.fill_cells([20.0, 0.0], [1.0, 1.0], (1, 1))[0];
        assert_eq!(bed.height_of(20.0, 0.0, -12.0, far), None);
    }
}
