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

    /// How much of its bed's texture goes at a place: within the join, the river's width and a bank ([`paint_band`]). The rest of the
    /// join only levels the ground to the connection's.
    pub fn paint(&self, x: f64, z: f64) -> f64 {
        let (dx, dz) = (x - self.at[0], z - self.at[1]);
        let d = dx * self.inward[0] + dz * self.inward[1];
        let along = dx * -self.inward[1] + dz * self.inward[0];
        if d > JOIN_HELD + JOIN_FADE {
            return 0.0;
        }

        paint_band(along.abs() - self.connection.width * 0.5, PAINT_BANK)
    }

    /// The ground type at a place within its width, `ground` there (the game's units): by how deep the connection's water stands over
    /// it ([`WaterGround::of_depth`]); none where the ground is not under that water.
    pub fn water_ground(&self, x: f64, z: f64, ground: f64) -> Option<WaterGround> {
        let (dx, dz) = (x - self.at[0], z - self.at[1]);
        let d = dx * self.inward[0] + dz * self.inward[1];
        let along = dx * -self.inward[1] + dz * self.inward[0];
        if d > JOIN_HELD + JOIN_FADE || along.abs() > self.connection.width * 0.5 {
            return None;
        }

        WaterGround::of_depth(self.connection.water - ground, false)
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

/// How steep a ford's ends slope down to the river's bed (degrees): gentle, so the shallows run into the deep water as a slope.
pub const FORD_RAMP_SLOPE: f64 = 8.0;

impl Ford {
    /// Where its held stretch starts and ends along the river at a place across it (`u`: -1 at one edge of the water, 1 at the other):
    /// not quite straight across, but slightly bowed out in its middle and skewed, by its place (each ford its own shape).
    pub fn ends(&self, u: f64) -> (f64, f64) {
        let u = u.clamp(-1.2, 1.2);
        // Slight: a metre or two over the river's width (the user: just enough not to be straight).
        let amp = (self.length * 0.05).clamp(0.8, 2.0);
        let phase = self.at * 0.137;
        let shape = |side: f64| amp * (0.6 * (1.0 - u * u) - 0.3 + 0.25 * u * side * phase.sin() + 0.05 * (2.3 * u + phase * side).sin());
        (self.at - self.length * 0.5 - shape(-1.0), self.at + self.length * 0.5 + shape(1.0))
    }
}

/// The kit's fords' water over their bed (the game's height units; `river_ford/*`: 0.36 .. 0.5).
pub const FORD_DEPTH: f64 = 0.5;

/// How far short of the water's drawn edge a ford's flat stops (metres): a ford is narrower than its river, its sides climbing to the land.
pub const FORD_SIDE_INSET: f64 = 4.0;

/// How far over the water the ground stands at its drawn edge at least (metres).
pub const EDGE_LIFT: f64 = 0.3;

/// How steep a ford's sides climb from its water to the land (degrees): units walk down to it, and the water does not lie over land nearly
/// as high as it (12° left so little between them that the water drew over the land from afar).
pub const FORD_BANK_SLOPE: f64 = 25.0;

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

    /// Its cliff banks: the ground behind them raised to their top.
    pub cliffs: Vec<CliffBank>,
}

/// A cliff bank (docs/plans/river_bridges.md, step 6.5): a stretch of a river (metres along its line, `from` to `to`) whose bank on one
/// side (`side`: 1 to the left as the river runs, -1 to the right) stands a cliff, its pieces along it (`wce_tile::shores`) and the ground
/// behind them raised to their top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CliffBank {
    pub from: f64,
    pub to: f64,
    pub side: f64,
}

/// A cliff bank's shape (metres; the kit's `attila_cliff_01..03` are 18 m long, 2 m under their middle to 4.15 m over it, their rock face
/// sloping from their foot 3.5 m in front of their middle up to their top's edge near it): their middles stand [`CLIFF_PIECE_OUT`] past
/// the water's drawn edge (negative: inside it; the river's bank meets its water some metres inside its drawn edge, the kit's profile
/// about 5 m for a 45 m river: their faces at the water); the ground keeps the river's bank in front of them, climbs from [`CLIFF_FOOT`]
/// over [`CLIFF_CLIMB`] (inside the pieces, under their grassy top) to [`CLIFF_TOP`] over the water, is held so for [`CLIFF_BACK`], and
/// eases back down to the land over [`CLIFF_FALL`]. (Climbing from the water, it buried their faces.)
///
/// Along the river its face falls to the water over [`CLIFF_RAMP`] at each end of its stretch, inside it (a stretch shorter than two
/// ramps never stands its whole height: shorter ramps tilted its pieces 30 to 40 degrees): its pieces sink and tilt with it ([`CliffBank::face`]), and nothing is raised past its ends at the water (raised ground there
/// stood with no face in front of it). Behind its face the raised land reaches past its ends [`CLIFF_SHOULDER`] metres a metre back, so
/// that its end slopes off as a shoulder rather than a wall across the bank.
///
/// In front of its foot, over [`CLIFF_FRONT_BAND`], the river's bank is cut [`CLIFF_FRONT_DEPTH`] under the water (along its ramps the
/// less): the bank meets the water some metres inside the drawn edge, more the wider the river (10 m for a 90 m one), and it stood as a
/// strip of dry ground in front of the face.
pub const CLIFF_PIECE_OUT: f64 = -3.0;
pub const CLIFF_FRONT_BAND: f64 = 10.0;
pub const CLIFF_FRONT_DEPTH: f64 = 1.0;
pub const CLIFF_FOOT: f64 = CLIFF_PIECE_OUT - 1.0;
pub const CLIFF_CLIMB: f64 = 2.0;
pub const CLIFF_TOP: f64 = 5.0;

/// How far over its pieces' top ([`CLIFF_TOP`]) the ground behind a cliff is raised (metres): their tops are bare rock, and level with
/// the ground they showed as a shelf with the grass starting behind it (more where they lean along a ramp); a little over, the ground
/// buries them and meets their face's edge.
pub const CLIFF_GROUND_OVER: f64 = 0.5;
pub const CLIFF_BACK: f64 = 8.0;
pub const CLIFF_FALL: f64 = 25.0;
pub const CLIFF_RAMP: f64 = 24.0;
pub const CLIFF_SHOULDER: f64 = 0.3;

impl CliffBank {
    /// How much it cuts the bank in front of its foot under the water (0 .. 1) `past` metres past the water's edge, `along` the river.
    #[must_use]
    pub fn front(&self, past: f64, along: f64) -> f64 {
        if past >= CLIFF_FOOT || past < CLIFF_FOOT - CLIFF_FRONT_BAND {
            return 0.0;
        }

        // Fading out over its last 2 m towards the river's middle (its bed is deeper there anyway).
        let fade = ((past - (CLIFF_FOOT - CLIFF_FRONT_BAND)) / 2.0).min(1.0);
        self.face(along) * smooth(fade)
    }

    /// How much of the cliff's top the ground stands at (0 .. 1) `past` metres past the water's edge on its side and `along` the river.
    #[must_use]
    pub fn rise(&self, past: f64, along: f64) -> f64 {
        self.rise_and_hold(past, along).0
    }

    /// How much of its top its face stands at `along` the river (0 .. 1): whole along its stretch but its ends' ramps, none past them.
    #[must_use]
    pub fn face(&self, along: f64) -> f64 {
        self.ramp_share(along, 0.0)
    }

    /// Its ramps' share `along` the river, `behind` metres back from its face's top (reaching past its ends [`CLIFF_SHOULDER`] a metre).
    fn ramp_share(&self, along: f64, behind: f64) -> f64 {
        let ramp = CLIFF_RAMP;
        let inside = (along - self.from).min(self.to - along) + behind.max(0.0) * CLIFF_SHOULDER;
        smooth((inside / ramp).clamp(0.0, 1.0))
    }

    /// [`Self::rise`], and how much the land behind its face is kept from the river's bank cut (0 .. 1: whole along its stretch, from
    /// its climb's start on however far, less along its ramps, none past its ends at the water).
    #[must_use]
    pub fn rise_and_hold(&self, past: f64, along: f64) -> (f64, f64) {
        if past < CLIFF_FOOT {
            return (0.0, 0.0);
        }

        let p = past - CLIFF_FOOT;
        let along_share = self.ramp_share(along, p - CLIFF_CLIMB);
        if along_share <= 0.0 {
            return (0.0, 0.0);
        }

        let across = if p < CLIFF_CLIMB {
            smooth(p / CLIFF_CLIMB)
        } else if p < CLIFF_CLIMB + CLIFF_BACK {
            1.0
        } else {
            1.0 - smooth(((p - CLIFF_CLIMB - CLIFF_BACK) / CLIFF_FALL).min(1.0))
        };
        (across * along_share, along_share)
    }
}

/// How far past the water's edge a cliff bank changes the ground (metres).
pub const CLIFF_REACH: f64 = CLIFF_FOOT.max(0.0) + CLIFF_CLIMB + CLIFF_BACK + CLIFF_FALL;

/// A place of a bed's grid ([`Channel::fill_cells`]): the lowest bed over it (infinite where none reaches), how much of a ford is there
/// (at the nearest place of the line) and whether it is under the river's water (within half its width of its line), and how far the
/// line is and the river's half width there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BedCell {
    pub bed: f64,
    pub ford: f64,
    pub wet: bool,
    pub near: f64,
    pub half: f64,

    /// The water at the nearest place of the line (NaN where none reaches).
    pub water: f64,

    /// The height a cliff bank raises the ground to (the game's units; minus infinity where none), and how much of it holds there (0 .. 1):
    /// its bank is not cut so much (the land behind a cliff's face stays as high as it is).
    pub cliff: f64,
    pub cliff_hold: f64,

    /// How much a cliff bank cuts the bank in front of its face under the water (0 .. 1; [`CLIFF_FRONT_BAND`]).
    pub cliff_front: f64,
}

impl Default for BedCell {
    fn default() -> Self {
        Self {
            bed: f64::INFINITY,
            ford: 0.0,
            wet: false,
            near: f64::INFINITY,
            half: 0.0,
            water: f64::NAN,
            cliff: f64::NEG_INFINITY,
            cliff_hold: 0.0,
            cliff_front: 0.0,
        }
    }
}

/// How far past a river's edge (or a lake's outline) its bed's texture goes whole, and how far it then fades out (metres).
pub const PAINT_BANK: f64 = 4.0;
pub const PAINT_FADE: f64 = 4.0;

/// The bed's texture `past` metres past the water's edge: whole up to [`PAINT_BANK`], fading out over [`PAINT_FADE`].
fn paint_band(past: f64, bank: f64) -> f64 {
    if past <= bank {
        1.0
    } else if past >= bank + PAINT_FADE {
        0.0
    } else {
        1.0 - smooth((past - bank) / PAINT_FADE)
    }
}

/// How far over a river's water its bed's texture goes whole, and how far over that it fades out (metres).
pub const PAINT_ABOVE: f64 = 0.3;
pub const PAINT_ABOVE_FADE: f64 = 1.2;

/// How deep a bed (the game's height units, under the water) is for its ground to be *deep water* rather than *shallow water* (the kit's
/// `river` tiles: deep water under a bed 5.7 deep; `river_shallow`: shallow water under one 1.4 deep).
pub const DEEP_GROUND_DEPTH: f64 = 3.0;

/// The ground type a river's water gives the ground under it, as the kit's river tiles have it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaterGround {
    Deep,
    Shallow,
}

impl WaterGround {
    /// The ground type under `depth` of water (the game's units): *deep water* from [`DEEP_GROUND_DEPTH`], *shallow water* under it or
    /// where `shallow` says (a ford); none where the ground is not under the water (it would slow units on dry ground: the game's
    /// water types are speed only, docs/re/river_hints.md).
    #[must_use]
    pub fn of_depth(depth: f64, shallow: bool) -> Option<Self> {
        if !(depth > 0.0) {
            return None;
        }

        Some(if shallow || depth < DEEP_GROUND_DEPTH { Self::Shallow } else { Self::Deep })
    }
}

impl Channel {
    /// The bed's height `r` metres from the centre where the water is `water` and the river `width` wide.
    pub fn across(&self, r: f64, water: f64, width: f64) -> f64 {
        self.across_at(r, water, width, f64::NAN)
    }

    /// How much of a ford is at a place along the river and across it (`u`, [`Ford::ends`]): 0 .. 1, held over its stretch, fading
    /// down to the river's bed at [`FORD_RAMP_SLOPE`] (over half its length at least); and its depth.
    pub fn ford_at(&self, along: f64, u: f64) -> (f64, f64) {
        let v = if self.vertical > 0.0 { self.vertical } else { 1.0 };
        self.fords
            .iter()
            .map(|f| {
                let (s, e) = f.ends(u);
                let d = if along < s {
                    s - along
                } else if along > e {
                    along - e
                } else {
                    0.0
                };
                let drop = (self.depth - f.depth).max(0.0) / v;
                let ramp = (f.length * 0.5).max(10.0).max(drop / FORD_RAMP_SLOPE.to_radians().tan());
                let t = (1.0 - d / ramp).clamp(0.0, 1.0);
                (if d <= 0.0 { 1.0 } else { t * t * (3.0 - 2.0 * t) }, f.depth)
            })
            .fold((0.0, self.depth), |a, b| if b.0 > a.0 { b } else { a })
    }

    /// [`Self::across`] at a place along the river (metres; NaN: no fords counted). `r` is signed: which side of the line (a ford's
    /// ends curve across it).
    ///
    /// A ford there: its bed flat at its depth across the water, then climbing at [`FORD_BANK_SLOPE`] to the land, blended in by how much
    /// of it is there ([`Self::ford_at`]); out of the water it only ever lowers the river's own profile (it was the whole profile scaled
    /// to the ford's depth, a flat valley as wide as the profile, 128 m for a 45 m river, its banks on past it: a great ring of cut ground).
    pub fn across_at(&self, r: f64, water: f64, width: f64, along: f64) -> f64 {
        let k = (width / DEEP.width).max(0.05);
        let half = width * 0.5;
        let river = self.profile(r, water, k).max(self.edge_floor(r, water, half));
        if along.is_nan() || self.fords.is_empty() {
            return river;
        }

        let (w, ford_depth) = self.ford_at(along, r / half.max(1e-6));
        if w <= 0.0 {
            return river;
        }

        // Narrower than the river: flat a few metres short of its edges, then up to the land.
        let floor = (half - FORD_SIDE_INSET).max(half * 0.6);
        let ford = water - ford_depth + (r.abs() - floor).max(0.0) * Self::rise_of(FORD_BANK_SLOPE, self.vertical);
        let blended = river + (ford - river) * w;
        if r.abs() <= floor { blended } else { river.min(blended).max(self.edge_floor(r, water, half)) }
    }

    /// The least the ground stands at near the water's drawn edge: over the water at the edge ([`EDGE_LIFT`]), down at the bank's slope
    /// inside it, so that the edge of the water never hangs over lower ground (it showed the void under the river from low down).
    fn edge_floor(&self, r: f64, water: f64, half: f64) -> f64 {
        let v = if self.vertical > 0.0 { self.vertical } else { 1.0 };
        water + EDGE_LIFT * v - (half - r.abs()).max(0.0) * self.rise()
    }

    /// The river's own bed `r` metres from its centre (the kit's profile scaled by its depth, its banks past it).
    fn profile(&self, r: f64, water: f64, k: f64) -> f64 {
        let scale = self.depth / KIT_DEPTH;
        let t = r.abs() / (PROFILE_STEP * k);
        let last = KIT_BED.len() - 1;
        if t >= last as f64 {
            let past = r.abs() - last as f64 * PROFILE_STEP * k;
            return water + KIT_BED[last] * scale + past * self.rise();
        }

        let i = t.floor() as usize;
        let f = t - i as f64;
        water + (KIT_BED[i] + (KIT_BED[i + 1] - KIT_BED[i]) * f) * scale
    }

    /// How far from the centre it reaches at a width (metres).
    pub fn reach(&self, width: f64) -> f64 {
        let k = (width / DEEP.width).max(0.05);
        let bed = (KIT_BED.len() - 1) as f64 * PROFILE_STEP * k + BANK_RISE / self.rise();
        if self.cliffs.is_empty() { bed } else { bed.max(width * 0.5 + CLIFF_REACH) }
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
                    // Which side of the line: a ford's ends curve across the river.
                    let side = if ab[0] * (p[1] - a[1]) - ab[1] * (p[0] - a[0]) < 0.0 { -1.0 } else { 1.0 };
                    let width = self.width[i] + (self.width[i + 1] - self.width[i]) * t;
                    if r > self.reach(width) {
                        continue;
                    }

                    let at = along[i] + (along[i + 1] - along[i]) * t;
                    let water = self.water[i] + (self.water[i + 1] - self.water[i]) * t;
                    let v = self.across_at(r * side, water, width, at);
                    let c = &mut out[j * w + k];
                    if v < c.bed {
                        c.bed = v;
                    }

                    // The ford, the water and a cliff bank's ground of the nearest place of the line (a cliff by a farther stretch's
                    // measure would rise out of the water at a bend; its rise here, its height once all is seen).
                    if r < c.near {
                        (c.cliff, c.cliff_hold) = self.cliff_rise_and_hold(r - width * 0.5, at, side);
                        c.cliff_front =
                            self.cliffs.iter().filter(|cl| cl.side == side).map(|cl| cl.front(r - width * 0.5, at)).fold(0.0, f64::max);
                        c.near = r;
                        c.ford = if self.fords.is_empty() { 0.0 } else { self.ford_at(at, r * side / (width * 0.5).max(1e-6)).0 };
                        c.wet = r <= width * 0.5;
                        c.half = width * 0.5;
                        c.water = water;
                    }
                }
            }
        }

        if !self.cliffs.is_empty() {
            self.blend_cliffs(&mut out, &along, origin, step, (w, h));
            let v = if self.vertical > 0.0 { self.vertical } else { 1.0 };
            for c in &mut out {
                c.cliff = if c.cliff_hold > 0.0 { c.water + (CLIFF_TOP + CLIFF_GROUND_OVER) * v * c.cliff } else { f64::NEG_INFINITY };
            }
        }

        out
    }

    /// The cliff banks' rise and hold at a place `past` metres past the water's edge on a `side` of the line, `along` it (the most of
    /// them).
    fn cliff_rise_and_hold(&self, past: f64, along: f64, side: f64) -> (f64, f64) {
        self.cliffs
            .iter()
            .filter(|cl| cl.side == side)
            .map(|cl| cl.rise_and_hold(past, along))
            .fold((0.0, 0.0), |a: (f64, f64), b| (a.0.max(b.0), a.1.max(b.1)))
    }

    /// A cliff bank's raised land over the seam of two places of the line: by the inside of a bend, the cells nearest one leg and those
    /// nearest the next part on the bend's bisector, and a cliff ending at the bend stood as a wall along it. Each cell takes the rise of
    /// the line's other places within [`CLIFF_BLEND`] of its nearest, the less the farther, and only behind the bank of its nearest place
    /// ([`CLIFF_BLEND_BACK`] from its water's drawn edge on: none rises out of its water or stands at its bank with no cliff in front).
    /// `out`'s cliffs hold their rise (0 .. 1), not their height yet.
    fn blend_cliffs(&self, out: &mut [BedCell], along: &[f64], origin: [f64; 2], step: [f64; 2], (w, h): (usize, usize)) {
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
                    let c = &mut out[j * w + k];
                    let behind = c.near - c.half;
                    if !c.near.is_finite() || behind <= 0.0 {
                        continue;
                    }

                    let p = [origin[0] + k as f64 * step[0], origin[1] + j as f64 * step[1]];
                    let t = if len2 > 1e-12 { (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / len2).clamp(0.0, 1.0) } else { 0.0 };
                    let r = (p[0] - a[0] - ab[0] * t).hypot(p[1] - a[1] - ab[1] * t);
                    let off = r - c.near;
                    if off <= 1e-9 || off >= CLIFF_BLEND {
                        continue;
                    }

                    let side = if ab[0] * (p[1] - a[1]) - ab[1] * (p[0] - a[0]) < 0.0 { -1.0 } else { 1.0 };
                    let width = self.width[i] + (self.width[i + 1] - self.width[i]) * t;
                    let at = along[i] + (along[i + 1] - along[i]) * t;
                    let (rise, hold) = self.cliff_rise_and_hold(r - width * 0.5, at, side);
                    let k = smooth(1.0 - off / CLIFF_BLEND) * smooth((behind / CLIFF_BLEND_BACK).min(1.0));
                    c.cliff = c.cliff.max(rise * k);
                    c.cliff_hold = c.cliff_hold.max(hold * k);
                }
            }
        }
    }
}

/// How far from its nearest place of the line a cell takes a cliff bank's rise of another (metres), and how far behind its own water's
/// drawn edge it takes it whole ([`Channel::blend_cliffs`]).
pub const CLIFF_BLEND: f64 = 20.0;
pub const CLIFF_BLEND_BACK: f64 = 14.0;

/// A lake's ground: its basin dug inside its outline (x, z) to `depth` under its `water` (the game's units; 0: none), its sides
/// climbing `rise` a metre in from the outline (the game's units); and its shore, the ground outside the outline lower than `rim` over
/// the water raised to it within `shore` metres of the outline, fading out (0: none), so that the water stays inside it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Basin {
    pub outline: Vec<[f64; 2]>,
    pub water: f64,
    pub depth: f64,
    pub rise: f64,
    pub shore: f64,
    pub rim: f64,

    /// The water plane's rectangle (x0, z0, x1, z1; empty: none): the shore holds over all of it outside the outline (water shows
    /// wherever the ground is under it), then fades over `shore` metres past it.
    pub plane: [f64; 4],
}

impl Basin {
    /// Whether a place is inside the outline, and how far the outline is.
    fn place(&self, x: f64, z: f64) -> Option<(bool, f64)> {
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

        Some((inside, near))
    }

    /// The basin's floor at a place: inside the outline, the water less the depth, or less the distance from the outline times the rise
    /// nearer its edge; none outside, or without a basin.
    pub fn floor(&self, x: f64, z: f64) -> Option<f64> {
        let (inside, near) = self.place(x, z)?;
        (inside && self.depth > 0.0).then(|| self.water - self.depth.min(near * self.rise))
    }

    /// The lake's ground at a place over `below` (what lies under it): dug to the floor inside, raised to the rim on the shore; none
    /// where it leaves the ground as it is.
    pub fn shape(&self, x: f64, z: f64, below: f64) -> Option<f64> {
        let (inside, near) = self.place(x, z)?;
        if inside {
            let f = (self.depth > 0.0).then(|| self.water - self.depth.min(near * self.rise))?;
            return (f < below).then_some(f);
        }

        let top = self.water + self.rim;
        let w = self.shore_weight(x, z, near);
        if w <= 0.0 || below >= top {
            return None;
        }

        Some(below + (top - below) * w)
    }

    /// How much of the shore holds at a place outside the outline `near` metres from it: all of it over the plane, fading over `shore`
    /// metres past the plane and the outline.
    fn shore_weight(&self, x: f64, z: f64, near: f64) -> f64 {
        if self.shore <= 0.0 {
            return 0.0;
        }

        let [x0, z0, x1, z1] = self.plane;
        let from_plane = if x1 > x0 && z1 > z0 {
            let (dx, dz) = ((x0 - x).max(x - x1).max(0.0), (z0 - z).max(z - z1).max(0.0));
            dx.hypot(dz)
        } else {
            f64::INFINITY
        };

        let d = near.min(from_plane);
        if d >= self.shore { 0.0 } else { 1.0 - smooth(d / self.shore) }
    }

    /// Whether it changes the ground at a place: inside its outline (with a basin) or on its shore.
    pub fn reaches(&self, x: f64, z: f64) -> bool {
        self.place(x, z).is_some_and(|(inside, near)| (inside && self.depth > 0.0) || (!inside && self.shore_weight(x, z, near) > 0.0))
    }

    fn bounds(&self) -> Option<[f64; 4]> {
        let m = self.shore.max(0.0);
        let [x0, z0, x1, z1] = self.plane;
        let plane = (x1 > x0 && z1 > z0).then_some([x0 - m, z0 - m, x1 + m, z1 + m]);
        self.outline
            .iter()
            .map(|p| [p[0] - m, p[1] - m, p[0] + m, p[1] + m])
            .chain(plane)
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

        // A lake's basin digs; its shore is raised to keep the water in.
        if let Some(b) = self.basin.as_ref() {
            let base = out.unwrap_or(below);
            if let Some(h) = b.shape(x, z, base) {
                out = Some(h);
            }
        }

        for j in &self.joins {
            if let Some((g, w)) = j.at(x, z) {
                let base = out.unwrap_or(below);
                out = Some(if w >= 1.0 { g } else { base + (g - base) * w });
            }
        }

        // A cliff bank raises the ground behind it, and keeps its bank from being cut as it holds (higher land stays behind its face).
        if cell.cliff_hold > 0.0 {
            let cut = out.unwrap_or(below).max(cell.cliff);
            let h = cut + (below.max(cell.cliff) - cut) * cell.cliff_hold;
            out = (h != below).then_some(h);
        }

        // In front of its face, the bank cut under the water.
        if cell.cliff_front > 0.0 && cell.water.is_finite() {
            let v = self.channel.as_ref().map_or(1.0, |c| if c.vertical > 0.0 { c.vertical } else { 1.0 });
            let base = out.unwrap_or(below);
            let target = cell.water - CLIFF_FRONT_DEPTH * v;
            if target < base {
                out = Some(base + (target - base) * cell.cliff_front);
            }
        }

        out
    }

    /// How much of the bed's texture goes at a place (0 .. 1): along a river that cuts its bed (or at its fords), its width and a bank
    /// past it, fading out ([`paint_band`]) however far its banks cut into a hill; in a join, its river's ([`Join::paint`]); in a lake's
    /// basin and a bank round it.
    ///
    /// `ground`: the ground there as the river leaves it (the game's units; NaN: not known). A river's bed is painted where that ground is
    /// under its water or just over it ([`PAINT_ABOVE`], fading out over [`PAINT_ABOVE_FADE`] more), not up its banks: they keep their
    /// own texture, and the shore's decals show on it.
    pub fn paint(&self, x: f64, z: f64, cell: BedCell, ground: f64) -> f64 {
        let mut w = 0.0f64;
        if let Some(c) = &self.channel
            && cell.near.is_finite()
            && (c.cuts || cell.ford > 0.0)
        {
            let by_distance = paint_band(cell.near - cell.half, PAINT_BANK);
            let by_height = if ground.is_finite() && cell.water.is_finite() {
                let v = if c.vertical > 0.0 { c.vertical } else { 1.0 };
                let above = (ground - cell.water) / v;
                if above <= PAINT_ABOVE {
                    1.0
                } else if above >= PAINT_ABOVE + PAINT_ABOVE_FADE {
                    0.0
                } else {
                    1.0 - smooth((above - PAINT_ABOVE) / PAINT_ABOVE_FADE)
                }
            } else {
                1.0
            };
            w = w.max(by_distance.min(by_height));
        }

        if let Some(b) = &self.basin
            && let Some((inside, near)) = b.place(x, z)
        {
            w = w.max(if inside { 1.0 } else { paint_band(near, PAINT_BANK * 0.5) });
        }

        self.joins.iter().fold(w, |w, j| w.max(j.paint(x, z)))
    }

    /// The ground type at a place where the ground (`ground`, the game's units, as the river leaves it) is under its water, by how deep
    /// ([`WaterGround::of_depth`]): within a river's ribbon (its bed cut or a ford there), its water at the nearest place of its line, a
    /// ford's shallow whatever its depth; within a join, the connection's water; within a lake's plane, the plane's water. None on dry
    /// ground, even under a ribbon's edge (the kit's 45 m ribbon is wider than the water it shows).
    pub fn water_ground(&self, x: f64, z: f64, cell: BedCell, ground: f64) -> Option<WaterGround> {
        if !ground.is_finite() {
            return None;
        }

        if let Some(c) = &self.channel
            && cell.wet
            && (c.cuts || cell.ford > 0.0)
            && let Some(k) = WaterGround::of_depth(cell.water - ground, cell.ford >= 0.5)
        {
            return Some(k);
        }

        if let Some(k) = self.joins.iter().find_map(|j| j.water_ground(x, z, ground)) {
            return Some(k);
        }

        let b = self.basin.as_ref()?;
        let on_plane = x >= b.plane[0] && x <= b.plane[2] && z >= b.plane[1] && z <= b.plane[3];
        if on_plane { WaterGround::of_depth(b.water - ground, false) } else { None }
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
    fn a_join_paints_its_river_not_all_the_ground_it_levels() {
        let j = Join { at: [0.0, 192.0], inward: [1.0, 0.0], connection: DEEP };
        let bed = RiverBed { joins: vec![j], ..Default::default() };
        let none = BedCell::default();
        assert_eq!(bed.paint(10.0, 192.0, none, f64::NAN), 1.0, "the river");
        assert_eq!(bed.paint(10.0, 192.0 + 25.0, none, f64::NAN), 1.0, "its bank");
        assert!(bed.paint(10.0, 192.0 + 28.0, none, f64::NAN) > 0.0 && bed.paint(10.0, 192.0 + 28.0, none, f64::NAN) < 1.0, "fading");
        assert_eq!(bed.paint(10.0, 192.0 + 60.0, none, f64::NAN), 0.0, "levelled, not painted");
        assert!(bed.height_at(10.0, 192.0 + 60.0, 5.0).is_some(), "but levelled there");
        assert_eq!(bed.paint(100.0, 192.0, none, f64::NAN), 0.0, "past the join");
        // DEEP's water at -6.2: its middle (bed -11.9) deep, a bank under a metre of it shallow, a bank over it dry.
        assert_eq!(bed.water_ground(10.0, 192.0, none, -11.9), Some(WaterGround::Deep));
        assert_eq!(bed.water_ground(10.0, 192.0 + 16.0, none, -6.8), Some(WaterGround::Shallow));
        assert_eq!(bed.water_ground(10.0, 192.0 + 20.0, none, -3.9), None, "under its ribbon, but dry");
        assert_eq!(bed.water_ground(10.0, 192.0 + 30.0, none, -11.9), None, "past its width");
    }

    #[test]
    fn only_the_ground_under_a_rivers_water_takes_a_water_type_by_its_depth() {
        let c = Channel {
            line: vec![[0.0, 0.0], [100.0, 0.0]],
            water: vec![-6.2, -6.2],
            width: vec![45.0, 45.0],
            depth: 5.7,
            bank_slope: 45.0,
            vertical: 1.0,
            fords: vec![],
            cuts: true,
            cliffs: vec![],
        };
        let bed = RiverBed { channel: Some(c), ..Default::default() };
        let wet = BedCell { wet: true, water: -6.2, ..BedCell::default() };
        assert_eq!(bed.water_ground(50.0, 0.0, wet, -11.9), Some(WaterGround::Deep), "its deep middle");
        assert_eq!(bed.water_ground(50.0, 0.0, wet, -7.0), Some(WaterGround::Shallow), "under 0.8 of water");
        assert_eq!(bed.water_ground(50.0, 0.0, wet, -3.9), None, "within its ribbon but over its water: dry bank");
        assert_eq!(bed.water_ground(50.0, 0.0, wet, f64::NAN), None);
        let ford = BedCell { ford: 1.0, ..wet };
        assert_eq!(bed.water_ground(50.0, 0.0, ford, -11.9), Some(WaterGround::Shallow), "a ford is shallow");
        assert_eq!(bed.water_ground(50.0, 0.0, ford, -5.0), None, "but not over its water");
        assert_eq!(bed.water_ground(50.0, 0.0, BedCell { wet: false, ..wet }, -11.9), None, "off its ribbon");
    }

    #[test]
    fn only_the_ground_under_a_lakes_water_takes_a_water_type_by_its_depth() {
        let b = Basin {
            outline: vec![[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]],
            water: -2.0,
            depth: 4.0,
            rise: 1.0,
            shore: 12.0,
            rim: 0.5,
            plane: [-8.0, -8.0, 108.0, 108.0],
        };
        let bed = RiverBed { basin: Some(b), ..Default::default() };
        let none = BedCell::default();
        assert_eq!(bed.water_ground(50.0, 50.0, none, -6.0), Some(WaterGround::Deep), "its basin, 4 under");
        assert_eq!(bed.water_ground(50.0, 50.0, none, -3.0), Some(WaterGround::Shallow), "its shallows");
        assert_eq!(bed.water_ground(50.0, 50.0, none, -1.5), None, "an island over its water");
        assert_eq!(bed.water_ground(-5.0, 50.0, none, -3.0), Some(WaterGround::Shallow), "low ground under its plane, past the outline");
        assert_eq!(bed.water_ground(-20.0, 50.0, none, -6.0), None, "low ground past its plane: no water shows there");
    }

    #[test]
    fn a_lakes_basin_digs_inside_and_raises_its_shore() {
        let b = Basin {
            outline: vec![[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]],
            water: -2.0,
            depth: 4.0,
            rise: 1.0,
            shore: 12.0,
            rim: 0.5,
            plane: [-8.0, -8.0, 108.0, 108.0],
        };
        assert_eq!(b.shape(50.0, 50.0, 3.0), Some(-6.0), "dug to its depth");
        assert_eq!(b.shape(50.0, 50.0, -10.0), None, "lower ground stays");
        assert_eq!(b.shape(2.0, 50.0, 3.0), Some(-4.0), "its side climbs to the outline");
        assert_eq!(b.shape(100.0001, 50.0, -5.0).map(|h| (h * 1e3).round() / 1e3), Some(-1.5), "the shore at the rim");
        assert!(b.shape(112.0, 50.0, -5.0).is_some_and(|h| h > -5.0 && h < -1.5), "fading out past the plane");
        assert_eq!(b.shape(125.0, 50.0, -5.0), None, "past the shore");
        assert_eq!(b.shape(107.0, 107.0, -5.0).map(|h| (h * 1e3).round() / 1e3), Some(-1.5), "over the plane's corner, held");
        assert_eq!(b.shape(104.0, 50.0, 0.0), None, "ground over the rim stays");
        assert!(b.reaches(50.0, 50.0) && b.reaches(105.0, 50.0) && !b.reaches(125.0, 50.0));
    }

    #[test]
    fn a_cliff_bank_raises_the_ground_behind_it_on_its_side_only() {
        // A river along +x, 40 m wide, its water at -6; a cliff on its left (+z) from 60 to 140 m along.
        let c = Channel {
            line: vec![[0.0, 0.0], [100.0, 0.0], [200.0, 0.0]],
            water: vec![-6.0; 3],
            width: vec![40.0; 3],
            depth: 5.7,
            bank_slope: 30.0,
            vertical: 1.0,
            cuts: true,
            cliffs: vec![CliffBank { from: 60.0, to: 140.0, side: 1.0 }],
            ..Default::default()
        };
        let bed = RiverBed { channel: Some(c.clone()), ..Default::default() };
        let cells = c.fill_cells([0.0, -80.0], [1.0, 1.0], (201, 161));
        let at = |x: f64, z: f64| {
            let cell = cells[((z + 80.0) as usize) * 201 + x as usize];
            bed.height_of(x, z, -5.5, cell).unwrap_or(-5.5)
        };
        let top = -6.0 + CLIFF_TOP + CLIFF_GROUND_OVER;
        assert!((at(100.0, 20.0 + CLIFF_FOOT + CLIFF_CLIMB + 4.0) - top).abs() < 1e-6, "its top: {}", at(100.0, 31.0));
        assert!(at(100.0, 10.0) < -6.0, "the river's bed in front of it: {}", at(100.0, 10.0));
        let front = at(100.0, 20.0 + CLIFF_FOOT - 1.0);
        assert!(front <= -6.0 - CLIFF_FRONT_DEPTH + 1e-6, "under the water in front of its face: {front}");
        assert!(at(100.0, -31.0) < -5.0, "the other bank as it was: {}", at(100.0, -31.0));
        let fall = at(100.0, 20.0 + CLIFF_FOOT + CLIFF_CLIMB + CLIFF_BACK + CLIFF_FALL * 0.5);
        assert!(fall > -5.5 && fall < top, "easing down behind: {fall}");
        assert!(at(100.0, 20.0 + CLIFF_REACH + 2.0) <= -5.5 + 1e-9, "the land past it");
        // Its ends: its face falls along its ramps inside its stretch, nothing raised past them at its face; behind it, a shoulder.
        let face = 20.0 + CLIFF_FOOT + CLIFF_CLIMB;
        let ramp = at(140.0 - CLIFF_RAMP * 0.5, face + 0.5);
        assert!(ramp < top - 1.0 && ramp > -5.5 + 1.0, "its ramp: {ramp}");
        assert!(at(140.0 + 2.0, face + 0.5) <= -5.5 + 1e-9, "none past its end at its face");
        let shoulder = at(140.0 - 2.0, face + 15.0);
        assert!(shoulder > -5.5 + 0.2 && shoulder < top, "its shoulder by its end behind it: {shoulder}");
        assert!(at(140.0 + 15.0 * CLIFF_SHOULDER + 1.0, face + 15.0) <= -5.5 + 1e-9, "none further along behind it");
        // A short stretch: lower, never its whole height.
        let short = Channel { cliffs: vec![CliffBank { from: 90.0, to: 117.0, side: 1.0 }], ..c.clone() };
        assert!(short.cliffs[0].face(103.5) < 0.75 && short.cliffs[0].face(103.5) > 0.4, "{}", short.cliffs[0].face(103.5));
        // Higher land stays.
        let cell = cells[(31 + 80) * 201 + 100];
        assert_eq!(bed.height_of(100.0, 31.0, 20.0, cell), None, "never lowered");
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
            cliffs: vec![],
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
            cliffs: vec![],
        };
        assert!((c.across_at(0.0, -6.2, 45.0, 150.0) - (-6.2 + KIT_BED[0] * FORD_DEPTH / KIT_DEPTH)).abs() < 1e-9, "the ford's bed");
        assert!((c.across_at(0.0, -6.2, 45.0, 10.0) - (-6.2 + KIT_BED[0])).abs() < 1e-9, "the river's own away from it");
        let mid = c.across_at(0.0, -6.2, 45.0, 180.0);
        assert!(mid > -6.2 + KIT_BED[0] && mid < -6.2 + KIT_BED[0] * FORD_DEPTH / KIT_DEPTH, "fading: {mid}");
        // Out of the water the ford only ever lowers the river's profile: no flat valley out to the profile's end.
        for r in [30.0, 45.0, 64.0, 80.0] {
            let (ford, river) = (c.across_at(r, -6.2, 45.0, 150.0), c.across(r, -6.2, 45.0));
            assert!(ford <= river + 1e-9, "at {r}: {ford} over the river's {river}");
            assert!(ford >= river - 25.0, "at {r}: {ford}, the river's {river}");
        }

        // Narrower than the river: flat to 4 m short of the water's edge, then climbing steeply, over the water at the edge.
        assert!((c.across_at(17.0, -6.2, 45.0, 150.0) - (-6.2 - FORD_DEPTH)).abs() < 1e-9, "its floor");
        let climb = c.across_at(22.5, -6.2, 45.0, 150.0) - c.across_at(18.5, -6.2, 45.0, 150.0);
        assert!(climb > 1.5, "its side climbs steeply to the land: {climb} over 4 m");
        assert!(c.across_at(22.5, -6.2, 45.0, 150.0) >= -6.2 + EDGE_LIFT - 1e-9, "over the water at its edge");
        let g = c.fill_cells([140.0, 0.0], [10.0, 1.0], (3, 1));
        assert!(g[1].bed > -7.0 && g[1].bed < -6.2 && g[1].ford == 1.0 && g[1].wet, "on the grid: {g:?}");

        // A river that cuts no bed: a kit bed at -12 filled to the ford's, nothing away from it.
        let bed = RiverBed { channel: Some(c.clone()), ..Default::default() };
        assert_eq!(bed.height_of(150.0, 0.0, -12.0, g[1]), Some(g[1].bed));
        let far = c.fill_cells([20.0, 0.0], [1.0, 1.0], (1, 1))[0];
        assert_eq!(bed.height_of(20.0, 0.0, -12.0, far), None);
    }
}
