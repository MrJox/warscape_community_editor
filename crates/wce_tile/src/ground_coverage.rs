//! Automatic ground coverage (experimental, docs/plans/ground_coverage.md): finds the steep height differentials of a tile (slopes, cliffs)
//! and decorates them. Rocks and vegetation go along the foot of each one, rocks only up its face to just under its top, and a vegetation
//! exclusion polygon goes around each cluster of the vegetation (BOB drops the manual trees outside every one, docs/re/vegetation.md).
//!
//! Everything here is in metres on a grid of the terrain's heights; the editor turns the result into instances.
//!
//! There are three kinds of terrain (`Kind`), which differ in steps 1 and 2: the cells they find and the height their bands are measured
//! from (each cell's base).
//!
//! 1. Cliffs: a cell is steep when its slope is above `slope_deg`. Coast: the game's no-go cells within `water_reach` of the sea (of a tile
//!    whose entry links to the sea: the ground under 0 m reached from the tile's edge). Riverbank: the no-go cells within `water_reach` of the water (stage B's water map),
//!    not under it. A closing (dilate, erode) joins a feature broken by a ledge; the 8-connected regions of that are the differentials.
//! 2. Each cell of a region walks up the steepest ascent to the first cell out of the region (its top). Its base: for a cliff, where it
//!    walks down the steepest descent to (its foot); by the sea, 0 m; by a river, the nearest water's surface. Along a long cliff the foot
//!    and the top change, so they are per cell. A cliff's cell whose foot is at the sea (`sea_margin`) or in the water is the coast's or
//!    the banks': it gets no bands.
//! 3. A region is kept when its rise (the 95th percentile of top - base over its cells) and its area are big enough.
//! 4. Foot band: the region's cells up to `foot_up` over their base, and the cells out of it within `foot_out` on its low side (nearer the
//!    base's height than the top's); by the water, down to `water_below` under its surface, the cells under it apart (`wet`: rocks only).
//!    Face band: the region's cells above that, up to `top_margin` under their top. The cells within `building_clearance` of a building's
//!    footprint are taken out of the bands: the coverage goes around the buildings.
//! 5. Rocks are thrown into both bands (bigger at the foot), plants into the foot band, from a random sequence seeded by the seed and the
//!    region, so that a region's result doesn't depend on the others; one whose own radius reaches into a building's clearance is dropped.
//! 6. The plants are clustered (single linkage within `cluster_distance`, no cluster wider than `max_cluster`); each cluster gets the convex
//!    hull of circles of `plant_radius + exclusion_margin` around its plants.

use crate::vegetation_sets::VegetationSet;

/// What the user can change.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    /// Degrees: steeper than this is a differential.
    pub slope_deg: f32,

    /// Metres: a lower differential is left alone (a smooth ramp, a bump).
    pub min_rise: f32,

    /// Square metres: a smaller one is left alone.
    pub min_area: f32,

    /// Metres up the slope from its foot that are foot band.
    pub foot_up: f32,

    /// Metres out on the flat below the slope that are foot band.
    pub foot_out: f32,

    /// Metres under the top that stay bare.
    pub top_margin: f32,

    /// Rocks per square metre at the foot and on the face.
    pub rock_density_foot: f32,
    pub rock_density_face: f32,

    /// Plants per square metre at the foot.
    pub plant_density: f32,

    /// Metres between plants.
    pub plant_spacing: f32,

    /// Whether the rocks are scaled at all: a building's scale is the editor's (`project_files::SCALE_ATTR`), and whether Attila honours
    /// it is not known. Without it every rock is at scale 1.
    pub scale_rocks: bool,

    /// Scale ranges of the rocks at the foot and on the face (a set entry's own range multiplies them).
    pub rock_scale_foot: (f32, f32),
    pub rock_scale_face: (f32, f32),

    /// Scale range of the plants (a set entry's own range replaces it).
    pub plant_scale: (f32, f32),

    /// What ground a rock stands on (before it is sunk).
    pub seat: Seat,

    /// How much of a rock's height goes under the ground it stands on (0..1).
    pub sink: f32,

    /// How much of a rock's height stays over the ground at its centre at least (0..1), whatever the seat and the sink: on a steep face the
    /// ground around a big rock is metres under its centre.
    pub min_visible: f32,

    /// Metres every rock and plant keeps from a building's footprint.
    pub building_clearance: f32,

    /// Metres: plants this near are in the same cluster.
    pub cluster_distance: f32,

    /// Metres: no cluster gets wider (its exclusion polygon would cover the land between).
    pub max_cluster: f32,

    /// Metres: a plant's radius, and the margin around it, in its exclusion polygon.
    pub plant_radius: f32,
    pub exclusion_margin: f32,

    /// Cliffs: metres over the sea a cliff's foot must be for the cliff to be inland (a lower one is the coast's).
    pub sea_margin: f32,

    /// Coast and banks: metres from the water (the sea, a river) the no-go ground may be to count.
    pub water_reach: f32,

    /// Coast and banks: metres under the water the waterline band goes (rocks only: half sunk ones in the shallows).
    pub water_below: f32,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            slope_deg: 35.0,
            min_rise: 3.0,
            min_area: 20.0,
            foot_up: 3.0,
            foot_out: 4.0,
            top_margin: 1.0,
            rock_density_foot: 1.0 / 10.0,
            rock_density_face: 1.0 / 25.0,
            plant_density: 1.0 / 15.0,
            plant_spacing: 2.0,
            scale_rocks: false,
            rock_scale_foot: (0.9, 1.6),
            rock_scale_face: (0.5, 1.0),
            plant_scale: (0.85, 1.15),
            seat: Seat::Average,
            sink: 0.25,
            min_visible: 0.5,
            building_clearance: 3.0,
            cluster_distance: 8.0,
            max_cluster: 30.0,
            plant_radius: 1.0,
            exclusion_margin: 2.0,
            sea_margin: 1.0,
            water_reach: 6.0,
            water_below: 0.6,
        }
    }
}

/// The kind of terrain the coverage is for: what it finds, and what its bands are measured from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    /// Inland slopes and cliffs (steeper than `slope_deg`) whose foot is over the sea and out of the water: bands from the foot each cell
    /// walks down to.
    #[default]
    Cliffs,

    /// The no-go ground by the sea (of a tile that meets it: the ground under 0 m reached from its edge): bands from the sea's surface (0 m).
    Coast,

    /// The no-go ground by a river or a water plane (stage B's water map): bands from the nearest water's surface.
    Riverbank,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Cliffs, Kind::Coast, Kind::Riverbank];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Cliffs => "cliffs",
            Kind::Coast => "coast",
            Kind::Riverbank => "riverbank",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.name() == name)
    }

    /// Whether its bands are measured from water (the waterline band reaches under it).
    pub fn by_water(self) -> bool {
        self != Kind::Cliffs
    }
}

impl Params {
    /// The defaults for a kind: by the water, a narrower foot band (the waterline) and fewer plants.
    pub fn for_kind(kind: Kind) -> Self {
        match kind {
            Kind::Cliffs => Self::default(),
            Kind::Coast => Self { min_rise: 2.0, foot_up: 2.0, foot_out: 3.0, rock_density_foot: 1.0 / 8.0, plant_density: 1.0 / 30.0, ..Self::default() },
            Kind::Riverbank => Self { min_rise: 1.5, min_area: 10.0, foot_up: 1.5, foot_out: 2.0, water_reach: 4.0, ..Self::default() },
        }
    }
}

/// What ground a rock stands on: under its centre, the mean of that and of 8 points around it at its radius (no higher than the centre's),
/// or the lowest of them (its downhill side never floats, but on a steep face it is buried).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Seat {
    Centre,
    #[default]
    Average,
    Lowest,
}

impl Seat {
    pub const ALL: [Seat; 3] = [Seat::Centre, Seat::Average, Seat::Lowest];

    pub fn name(self) -> &'static str {
        match self {
            Seat::Centre => "centre",
            Seat::Average => "average",
            Seat::Lowest => "lowest",
        }
    }
}

impl Params {
    /// As one attribute's text (`slope_deg=35 min_rise=3 ...`), to keep them with what they made.
    pub fn to_text(&self) -> String {
        let mut out = self.numbers().into_iter().map(|(name, v)| format!("{name}={v}")).collect::<Vec<_>>();

        out.push(format!("scale_rocks={}", self.scale_rocks));
        out.push(format!("seat={}", self.seat.name()));
        out.join(" ")
    }

    /// From [`Self::to_text`]; what is missing or does not read keeps its default (the cliffs').
    pub fn from_text(text: &str) -> Self {
        Self::from_text_for(Kind::Cliffs, text)
    }

    /// From [`Self::to_text`]; what is missing or does not read keeps the kind's default.
    pub fn from_text_for(kind: Kind, text: &str) -> Self {
        let mut p = Self::for_kind(kind);
        for (name, value) in text.split_whitespace().filter_map(|kv| kv.split_once('=')) {
            if name == "scale_rocks" {
                p.scale_rocks = value == "true";
            } else if name == "seat" {
                p.seat = Seat::ALL.into_iter().find(|s| s.name() == value).unwrap_or_default();
            } else if let (Ok(v), Some(slot)) = (value.parse::<f32>(), p.number_mut(name)) {
                *slot = v;
            }
        }

        p
    }

    const NAMES: [&'static str; 26] = [
        "slope_deg", "min_rise", "min_area", "foot_up", "foot_out", "top_margin", "rock_density_foot", "rock_density_face", "plant_density",
        "plant_spacing", "rock_scale_foot_min", "rock_scale_foot_max", "rock_scale_face_min", "rock_scale_face_max", "plant_scale_min",
        "plant_scale_max", "sink", "min_visible", "building_clearance", "cluster_distance", "max_cluster", "plant_radius", "exclusion_margin",
        "sea_margin", "water_reach", "water_below",
    ];

    fn numbers(&self) -> Vec<(&'static str, f32)> {
        let mut c = *self;
        Self::NAMES.iter().map(|&n| (n, *c.number_mut(n).expect("a number"))).collect()
    }

    fn number_mut(&mut self, name: &str) -> Option<&mut f32> {
        Some(match name {
            "slope_deg" => &mut self.slope_deg,
            "min_rise" => &mut self.min_rise,
            "min_area" => &mut self.min_area,
            "foot_up" => &mut self.foot_up,
            "foot_out" => &mut self.foot_out,
            "top_margin" => &mut self.top_margin,
            "rock_density_foot" => &mut self.rock_density_foot,
            "rock_density_face" => &mut self.rock_density_face,
            "plant_density" => &mut self.plant_density,
            "plant_spacing" => &mut self.plant_spacing,
            "rock_scale_foot_min" => &mut self.rock_scale_foot.0,
            "rock_scale_foot_max" => &mut self.rock_scale_foot.1,
            "rock_scale_face_min" => &mut self.rock_scale_face.0,
            "rock_scale_face_max" => &mut self.rock_scale_face.1,
            "plant_scale_min" => &mut self.plant_scale.0,
            "plant_scale_max" => &mut self.plant_scale.1,
            "sink" => &mut self.sink,
            "min_visible" => &mut self.min_visible,
            "building_clearance" => &mut self.building_clearance,
            "cluster_distance" => &mut self.cluster_distance,
            "max_cluster" => &mut self.max_cluster,
            "plant_radius" => &mut self.plant_radius,
            "exclusion_margin" => &mut self.exclusion_margin,
            "sea_margin" => &mut self.sea_margin,
            "water_reach" => &mut self.water_reach,
            "water_below" => &mut self.water_below,
            _ => return None,
        })
    }
}

/// The terrain's heights on a grid: `nx * nz` samples (rows along z), `cell` metres apart, sample (0, 0) at (`x0`, `z0`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Grid {
    pub x0: f32,
    pub z0: f32,
    pub cell: f32,
    pub nx: usize,
    pub nz: usize,
    pub heights: Vec<f32>,
}

impl Grid {
    /// Samples `height(x, z)` (metres) over [x0, x1] x [z0, z1], `cell` metres apart.
    pub fn sample(x0: f32, z0: f32, x1: f32, z1: f32, cell: f32, height: impl Fn(f32, f32) -> f32) -> Self {
        let nx = (((x1 - x0) / cell).floor() as usize + 1).max(1);
        let nz = (((z1 - z0) / cell).floor() as usize + 1).max(1);
        let mut heights = Vec::with_capacity(nx * nz);
        for z in 0..nz {
            for x in 0..nx {
                heights.push(height(x0 + x as f32 * cell, z0 + z as f32 * cell));
            }
        }

        Self { x0, z0, cell, nx, nz, heights }
    }

    /// The centre of a sample, in metres.
    pub fn at(&self, i: usize) -> (f32, f32) {
        (self.x0 + (i % self.nx) as f32 * self.cell, self.z0 + (i / self.nx) as f32 * self.cell)
    }

    /// The height at a point, bilinear between the samples (clamped to the grid).
    pub fn height(&self, x: f32, z: f32) -> f32 {
        let fx = ((x - self.x0) / self.cell).clamp(0.0, (self.nx - 1) as f32);
        let fz = ((z - self.z0) / self.cell).clamp(0.0, (self.nz - 1) as f32);
        let (ix, iz) = ((fx as usize).min(self.nx.saturating_sub(2)), (fz as usize).min(self.nz.saturating_sub(2)));
        let (tx, tz) = (fx - ix as f32, fz - iz as f32);
        let h = |x: usize, z: usize| self.heights[z.min(self.nz - 1) * self.nx + x.min(self.nx - 1)];
        let a = h(ix, iz) + (h(ix + 1, iz) - h(ix, iz)) * tx;
        let b = h(ix, iz + 1) + (h(ix + 1, iz + 1) - h(ix, iz + 1)) * tx;
        a + (b - a) * tz
    }

    /// The 8 neighbours of a sample that are on the grid.
    fn neighbours(&self, i: usize) -> impl Iterator<Item = usize> + '_ {
        let (x, z) = ((i % self.nx) as isize, (i / self.nx) as isize);
        (-1isize..=1).flat_map(move |dz| (-1isize..=1).map(move |dx| (dx, dz))).filter(|&d| d != (0, 0)).filter_map(move |(dx, dz)| {
            let (nx, nz) = (x + dx, z + dz);
            (nx >= 0 && nz >= 0 && (nx as usize) < self.nx && (nz as usize) < self.nz).then(|| nz as usize * self.nx + nx as usize)
        })
    }
}

/// The categories of `battlefield_buildings` the coverage goes around: the real buildings. Not `incidental` (rocks, props, barricades) or
/// `ground_platform` (path borders, mounds: part of the ground), nor a key of no known category (the invisible dummies among them).
pub const AVOIDED_CATEGORIES: [&str; 6] = ["generic", "fort_wall", "fort_tower", "unbreachable_fort_wall", "gate", "bridge"];

/// What the kinds need of the tile besides its heights, one value per sample of a [`Grid`]: the water over the ground and the game's no-go
/// field (stage B's: `terrain_mesh::WaterFields`, the final heights the game is built from).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Context {
    /// Metres of water (rivers, water planes) over the ground; 0 where there is none. The sea is not in it: it is the ground under 0 m.
    pub water_depth: Vec<f32>,

    /// Where units can't walk (BOB's automatic no-go areas: steeper than 30 degrees, less the go outlines).
    pub no_go: Vec<bool>,

    /// Whether the tile meets the sea (its entry links to the `sea` tile set). A tile's heights are not the sea's: an inland tile may be
    /// all under 0 m.
    pub has_sea: bool,
}

impl Context {
    /// Stage B's fields, `w` x `h` vertices over the tile (row j along +z) `sx` and `sz` metres apart from (0, 0), at the grid's samples
    /// (the nearest vertex); no sea. `depth`: metres, NaN or under 0 where there is no water.
    pub fn resample(g: &Grid, w: usize, h: usize, sx: f32, sz: f32, depth: &[f32], no_go: &[bool]) -> Self {
        let mut c = Self { water_depth: Vec::with_capacity(g.nx * g.nz), no_go: Vec::with_capacity(g.nx * g.nz), has_sea: false };
        for i in 0..g.nx * g.nz {
            let (x, z) = g.at(i);
            let vx = ((x / sx).round().max(0.0) as usize).min(w.saturating_sub(1));
            let vz = ((z / sz).round().max(0.0) as usize).min(h.saturating_sub(1));
            let v = vz * w + vx;
            c.water_depth.push(depth.get(v).copied().filter(|d| *d > 0.0).unwrap_or(0.0));
            c.no_go.push(no_go.get(v).copied().unwrap_or(false));
        }

        c
    }
}

/// A building the coverage goes around: its centre and radius, metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blocker {
    pub x: f32,
    pub z: f32,
    pub radius: f32,
}

/// A rock model's size at scale 1: its radius across and its height, metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RockSize {
    pub radius: f32,
    pub height: f32,
}

/// What a generation takes.
pub struct Input<'a> {
    pub kind: Kind,
    pub grid: &'a Grid,

    /// The water and the no-go field on the grid's samples; none: no water, all walkable (the coast and the banks find nothing).
    pub context: Option<&'a Context>,
    pub blockers: &'a [Blocker],
    pub rocks: &'a VegetationSet,
    pub plants: &'a VegetationSet,

    /// The size of a rock key's model.
    pub rock_size: &'a dyn Fn(&str) -> RockSize,
    pub params: Params,
    pub seed: u64,
}

/// Why a differential was left alone, or that it was not.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    Kept,

    /// Its rise (metres) is under the minimum.
    TooLow(f32),

    /// Its area (square metres) is under the minimum.
    TooSmall(f32),

    /// A cliff whose foot is at the sea or in a river everywhere (the coast's, the banks').
    ByWater,
}

/// A placed rock or plant: its key, where (metres, `y` the height of its origin), its scale and its turn (degrees).
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub key: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub scale: f32,
    pub yaw: f32,
}

/// A differential: its number (from 1, in the order of the grid), what became of it, its rise and area, its cells (the grid's indices: the
/// region, its foot band and its face band), and what goes on it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Differential {
    pub number: usize,
    pub state: Option<State>,
    pub rise: f32,
    pub area: f32,
    pub cells: Vec<usize>,
    pub foot: Vec<usize>,
    pub face: Vec<usize>,

    /// By the water, the foot band's cells under it (rocks only).
    pub wet: Vec<usize>,
    pub rocks: Vec<Placed>,
    pub plants: Vec<Placed>,

    /// How many buildings it goes around.
    pub avoided: usize,

    /// The exclusion polygons around its plants (x, z), counter-clockwise seen from above (+y).
    pub exclusions: Vec<Vec<(f32, f32)>>,
}

impl Differential {
    pub fn kept(&self) -> bool {
        self.state == Some(State::Kept)
    }
}

/// Finds the differentials of the grid and what goes on them.
pub fn generate(input: &Input) -> Vec<Differential> {
    let g = input.grid;
    let p = &input.params;
    let n = g.nx * g.nz;
    if g.nx < 3 || g.nz < 3 || g.heights.len() != n {
        return vec![];
    }

    // 1. The kind's cells, closed, in regions; the height each cell's bands are measured from (its base); and the cells left to another kind.
    let Some(Features { mask, base, others }) = features(input) else { return vec![] };
    let mut label = vec![NONE; n];
    let mut regions: Vec<Differential> = vec![];
    for start in 0..n {
        if !mask[start] || label[start] != NONE {
            continue;
        }

        let r = regions.len();
        let mut cells = vec![start];
        label[start] = r;
        let mut k = 0;
        while k < cells.len() {
            for nb in g.neighbours(cells[k]) {
                if mask[nb] && label[nb] == NONE {
                    label[nb] = r;
                    cells.push(nb);
                }
            }

            k += 1;
        }

        cells.sort_unstable();
        regions.push(Differential { number: r + 1, cells, ..Default::default() });
    }

    // 2. The top of every cell of a region.
    let top = walk(g, &mask, false);
    let theirs = |i: usize| others[i];

    // 3. Rise and area.
    let area_of = |cells: usize| cells as f32 * g.cell * g.cell;
    for d in &mut regions {
        let mut rises = d.cells.iter().filter(|&&i| !theirs(i)).map(|&i| g.heights[top[i]] - base[i]).collect::<Vec<_>>();
        rises.sort_unstable_by(f32::total_cmp);
        d.rise = rises.get(rises.len().saturating_sub(1) * 95 / 100).copied().unwrap_or(0.0);
        d.area = area_of(d.cells.len());
        d.state = Some(if rises.is_empty() {
            State::ByWater
        } else if d.rise < p.min_rise {
            State::TooLow(d.rise)
        } else if d.area < p.min_area {
            State::TooSmall(d.area)
        } else {
            State::Kept
        });
    }

    // 4. The bands of the kept ones: their own cells, then the low side out of them (nearest region first, breadth first); by the water,
    //    down to `water_below` under it, the cells under it apart (rocks only).
    let mut ring = vec![NONE; n];
    let mut source = vec![NONE; n];
    let mut queue = std::collections::VecDeque::new();
    let wet = |h: f32, f: f32| input.kind.by_water() && h < f;
    for d in regions.iter_mut().filter(|d| d.kept()) {
        for &i in d.cells.iter().filter(|&&i| !theirs(i)) {
            let (h, f, t) = (g.heights[i], base[i], g.heights[top[i]]);
            if h - f <= p.foot_up {
                if wet(h, f) { d.wet.push(i) } else { d.foot.push(i) }
            } else if h <= t - p.top_margin {
                d.face.push(i);
            }

            source[i] = i;
            queue.push_back(i);
        }
    }

    let reach = p.foot_out / g.cell;
    let floor = if input.kind.by_water() { p.water_below } else { f32::INFINITY };
    while let Some(i) = queue.pop_front() {
        let s = source[i];
        let r = label[s];
        let (sx, sz) = ((s % g.nx) as f32, (s / g.nx) as f32);
        let (f, t) = (base[s], g.heights[top[s]]);
        for nb in g.neighbours(i) {
            if mask[nb] || ring[nb] != NONE {
                continue;
            }

            let (x, z) = ((nb % g.nx) as f32, (nb / g.nx) as f32);
            let h = g.heights[nb];
            if (x - sx).hypot(z - sz) <= reach && (h - f).abs() < (h - t).abs() && h >= f - floor {
                ring[nb] = r;
                source[nb] = s;
                if wet(h, f) { regions[r].wet.push(nb) } else { regions[r].foot.push(nb) }
                queue.push_back(nb);
            }
        }
    }

    // The cells near a building come out of the bands (the coverage goes around it).
    let mut near = vec![false; n];
    let mut avoided = vec![std::collections::BTreeSet::new(); regions.len()];
    for (b, bl) in input.blockers.iter().enumerate() {
        let reach = bl.radius + p.building_clearance;
        let x0 = (((bl.x - reach - g.x0) / g.cell).floor().max(0.0) as usize).min(g.nx - 1);
        let x1 = (((bl.x + reach - g.x0) / g.cell).ceil().max(0.0) as usize).min(g.nx - 1);
        let z0 = (((bl.z - reach - g.z0) / g.cell).floor().max(0.0) as usize).min(g.nz - 1);
        let z1 = (((bl.z + reach - g.z0) / g.cell).ceil().max(0.0) as usize).min(g.nz - 1);
        for z in z0..=z1 {
            for x in x0..=x1 {
                let i = z * g.nx + x;
                let (cx, cz) = g.at(i);
                if (cx - bl.x).hypot(cz - bl.z) > reach {
                    continue;
                }

                near[i] = true;
                let r = if label[i] != NONE { label[i] } else { ring[i] };
                if r != NONE {
                    avoided[r].insert(b);
                }
            }
        }
    }

    for (d, a) in regions.iter_mut().zip(avoided) {
        d.avoided = a.len();
        d.foot.retain(|&i| !near[i]);
        d.face.retain(|&i| !near[i]);
        d.wet.retain(|&i| !near[i]);
    }

    let buildings = Buildings::new(input.blockers, p.building_clearance);

    // 5 and 6. What goes on the kept ones.
    for d in regions.iter_mut() {
        if d.kept() {
            d.foot.sort_unstable();
            d.wet.sort_unstable();
            populate(input, &buildings, &base, d);
        } else {
            d.foot.clear();
            d.face.clear();
            d.wet.clear();
        }
    }

    regions
}

const NONE: usize = usize::MAX;

/// What a kind finds on the grid.
struct Features {
    /// Its cells (closed: a feature broken by a ledge stays one).
    mask: Vec<bool>,

    /// The height every cell's bands are measured from: a cliff's walked foot, the sea's surface, the nearest water's surface.
    base: Vec<f32>,

    /// The cells of the mask that are another kind's: a cliff's whose foot is at the sea (`sea_margin`) or in the water. They get no bands.
    others: Vec<bool>,
}

/// What the kind finds; none when it needs the context and there is none.
fn features(input: &Input) -> Option<Features> {
    let g = input.grid;
    let p = &input.params;
    let n = g.nx * g.nz;
    let closed = |m: &[bool]| erode(g, &dilate(g, m));
    match input.kind {
        Kind::Cliffs => {
            let tan = p.slope_deg.to_radians().tan();
            let mask = closed(&(0..n).map(|i| slope(g, i) > tan).collect::<Vec<_>>());
            let foot = walk(g, &mask, true);
            let base = (0..n).map(|i| if foot[i] == NONE { g.heights[i] } else { g.heights[foot[i]] }).collect::<Vec<_>>();
            let tile_sea = input.context.is_some_and(|c| c.has_sea);
            let wet = |i: usize| input.context.and_then(|c| c.water_depth.get(i)).is_some_and(|&d| d > 0.0);
            let others = (0..n).map(|i| mask[i] && ((tile_sea && base[i] <= p.sea_margin) || (foot[i] != NONE && wet(foot[i])))).collect();
            Some(Features { mask, base, others })
        }

        Kind::Coast => {
            let c = input.context.filter(|c| c.no_go.len() == n && c.has_sea)?;
            let sea = sea(g);
            let near = nearest(g, &sea, p.water_reach);
            let mut mask = closed(&(0..n).map(|i| c.no_go[i] && near[i] != NONE && !sea[i]).collect::<Vec<_>>());
            for (m, s) in mask.iter_mut().zip(&sea) {
                *m &= !s;
            }

            Some(Features { mask, base: vec![0.0; n], others: vec![false; n] })
        }

        Kind::Riverbank => {
            let c = input.context.filter(|c| c.no_go.len() == n && c.water_depth.len() == n)?;
            let water = c.water_depth.iter().map(|&d| d > 0.0).collect::<Vec<_>>();
            let near = nearest(g, &water, p.water_reach);
            let mut mask = closed(&(0..n).map(|i| c.no_go[i] && near[i] != NONE && !water[i]).collect::<Vec<_>>());
            for (m, w) in mask.iter_mut().zip(&water) {
                *m &= !w;
            }

            // A cell's water is the nearest wet cell's surface (further than the reach for the cells the closing added).
            let far = nearest(g, &water, p.water_reach + 2.0 * g.cell);
            let surface = |w: usize| g.heights[w] + c.water_depth[w];
            let base = (0..n).map(|i| if far[i] == NONE { g.heights[i] } else { surface(far[i]) }).collect();
            Some(Features { mask, base, others: vec![false; n] })
        }
    }
}

/// The sea of a tile that meets it ([`Context::has_sea`]): the samples under 0 m that are reached from the tile's edge through others
/// under it (a pond under 0 m inland is not the sea).
fn sea(g: &Grid) -> Vec<bool> {
    let n = g.nx * g.nz;
    let mut sea = vec![false; n];
    let mut queue = (0..n)
        .filter(|&i| {
            let (x, z) = (i % g.nx, i / g.nx);
            (x == 0 || z == 0 || x == g.nx - 1 || z == g.nz - 1) && g.heights[i] < 0.0
        })
        .collect::<Vec<_>>();
    for &i in &queue {
        sea[i] = true;
    }

    while let Some(i) = queue.pop() {
        for nb in g.neighbours(i) {
            if !sea[nb] && g.heights[nb] < 0.0 {
                sea[nb] = true;
                queue.push(nb);
            }
        }
    }

    sea
}

/// For every sample within `reach` metres (straight) of a source sample, that source (breadth first from all of them: about the nearest);
/// NONE for the others. A source is its own.
fn nearest(g: &Grid, sources: &[bool], reach: f32) -> Vec<usize> {
    let mut near = vec![NONE; sources.len()];
    let mut queue = std::collections::VecDeque::new();
    for (i, _) in sources.iter().enumerate().filter(|(_, s)| **s) {
        near[i] = i;
        queue.push_back(i);
    }

    let reach = reach / g.cell;
    while let Some(i) = queue.pop_front() {
        let s = near[i];
        let (sx, sz) = ((s % g.nx) as f32, (s / g.nx) as f32);
        for nb in g.neighbours(i) {
            let (x, z) = ((nb % g.nx) as f32, (nb / g.nx) as f32);
            if near[nb] == NONE && (x - sx).hypot(z - sz) <= reach {
                near[nb] = s;
                queue.push_back(nb);
            }
        }
    }

    near
}

/// tan of the slope at a sample (central differences, one-sided at the edges).
fn slope(g: &Grid, i: usize) -> f32 {
    let (x, z) = (i % g.nx, i / g.nx);
    let h = |x: usize, z: usize| g.heights[z * g.nx + x];
    let d = |a: f32, b: f32, steps: usize| (b - a) / (steps as f32 * g.cell);
    let (xl, xr) = (x.saturating_sub(1), (x + 1).min(g.nx - 1));
    let (zl, zr) = (z.saturating_sub(1), (z + 1).min(g.nz - 1));
    let gx = d(h(xl, z), h(xr, z), xr - xl);
    let gz = d(h(x, zl), h(x, zr), zr - zl);
    gx.hypot(gz)
}

fn dilate(g: &Grid, m: &[bool]) -> Vec<bool> {
    (0..m.len()).map(|i| m[i] || g.neighbours(i).any(|nb| m[nb])).collect()
}

fn erode(g: &Grid, m: &[bool]) -> Vec<bool> {
    (0..m.len()).map(|i| m[i] && g.neighbours(i).all(|nb| m[nb])).collect()
}

/// For every cell of the mask, where the steepest descent (`down`) or ascent from it leaves the mask: the first cell out, or the last one
/// in when it stops inside (a pit, a peak). The paths are remembered: a cell that one went through takes the same end.
fn walk(g: &Grid, mask: &[bool], down: bool) -> Vec<usize> {
    let n = mask.len();
    let mut end = vec![NONE; n];
    let mut path = vec![];
    for start in 0..n {
        if !mask[start] || end[start] != NONE {
            continue;
        }

        path.clear();
        path.push(start);
        let found = loop {
            let cur = *path.last().unwrap_or(&start);
            let h = g.heights[cur];
            let better = |a: f32, b: f32| if down { a < b } else { a > b };
            let next = g.neighbours(cur).fold(None::<usize>, |best, nb| match best {
                Some(b) if !better(g.heights[nb], g.heights[b]) => Some(b),
                _ if better(g.heights[nb], h) => Some(nb),
                _ => best,
            });

            match next {
                None => break cur,
                Some(nx) if !mask[nx] => break nx,
                Some(nx) if end[nx] != NONE => break end[nx],
                Some(nx) => path.push(nx),
            }
        };

        for &c in &path {
            end[c] = found;
        }
    }

    end
}

/// A small random sequence (xorshift, seeded through splitmix).
struct Random(u64);

impl Random {
    fn new(seed: u64) -> Self {
        let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        Self((z ^ (z >> 31)) | 1)
    }

    /// In [0, 1).
    fn unit(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }

    fn range(&mut self, (lo, hi): (f32, f32)) -> f32 {
        lo + (hi - lo) * self.unit()
    }

    /// A point in a cell of the list, metres.
    fn point_in(&mut self, g: &Grid, cells: &[usize]) -> (f32, f32) {
        let i = cells[((self.unit() * cells.len() as f32) as usize).min(cells.len() - 1)];
        let (x, z) = g.at(i);
        (x + (self.unit() - 0.5) * g.cell, z + (self.unit() - 0.5) * g.cell)
    }
}

/// The buildings in square buckets, to find the ones near a point quickly (a settlement has thousands).
struct Buildings<'a> {
    list: &'a [Blocker],
    clearance: f32,
    buckets: std::collections::HashMap<(i32, i32), Vec<usize>>,
}

impl<'a> Buildings<'a> {
    /// Metres a bucket is across.
    const BUCKET: f32 = 32.0;

    /// The largest radius of a rock or plant asked about (metres): a building is in every bucket this near its clearance.
    const ITEM: f32 = 16.0;

    fn new(list: &'a [Blocker], clearance: f32) -> Self {
        let mut buckets: std::collections::HashMap<(i32, i32), Vec<usize>> = std::collections::HashMap::new();
        for (b, bl) in list.iter().enumerate() {
            let reach = bl.radius + clearance + Self::ITEM;
            let cell = |v: f32| (v / Self::BUCKET).floor() as i32;
            for bz in cell(bl.z - reach)..=cell(bl.z + reach) {
                for bx in cell(bl.x - reach)..=cell(bl.x + reach) {
                    buckets.entry((bx, bz)).or_default().push(b);
                }
            }
        }

        Self { list, clearance, buckets }
    }

    /// Whether something of radius `r` at (x, z) keeps the clearance from every building.
    fn clear(&self, x: f32, z: f32, r: f32) -> bool {
        let key = ((x / Self::BUCKET).floor() as i32, (z / Self::BUCKET).floor() as i32);
        self.buckets.get(&key).is_none_or(|bs| bs.iter().all(|&b| {
            let bl = &self.list[b];
            (bl.x - x).hypot(bl.z - z) >= bl.radius + self.clearance + r.min(Self::ITEM)
        }))
    }
}

/// Rocks and plants on a kept differential (clear of the buildings), and the exclusion polygons around its plants.
fn populate(input: &Input, buildings: &Buildings, base: &[f32], d: &mut Differential) {
    let g = input.grid;
    let p = &input.params;
    let mut rnd = Random::new(input.seed ^ (d.number as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15));
    let (xmax, zmax) = (g.x0 + (g.nx - 1) as f32 * g.cell, g.z0 + (g.nz - 1) as f32 * g.cell);
    let area = |cells: &[usize]| cells.len() as f32 * g.cell * g.cell;

    // Rocks: at the foot (by the water, in the shallows too) and on the face, standing on the ground under them and sunk into it.
    let shore = d.foot.iter().chain(&d.wet).copied().collect::<Vec<_>>();
    for (cells, density, range) in [(&shore, p.rock_density_foot, p.rock_scale_foot), (&d.face, p.rock_density_face, p.rock_scale_face)] {
        if cells.is_empty() || input.rocks.entries.is_empty() {
            continue;
        }

        let count = (area(cells) * density).round() as usize;
        for _ in 0..count {
            let (x, z) = rnd.point_in(g, cells);
            let Some(entry) = input.rocks.pick(rnd.unit()) else { break };
            let own = entry.scale.map_or(1.0, |r| rnd.range(r));
            let scale = rnd.range(range) * own;
            let scale = if p.scale_rocks { scale } else { 1.0 };
            let size = (input.rock_size)(&entry.key);
            let (r, h) = (size.radius * scale, size.height * scale);
            let centre = g.height(x, z);
            let ring = (0..8).map(|k| {
                let a = k as f32 * std::f32::consts::FRAC_PI_4;
                g.height(x + r * a.cos(), z + r * a.sin())
            });

            // On the tile (a cell at its edge reaches half a cell out), as the plants.
            if !buildings.clear(x, z, r) || x < g.x0 || z < g.z0 || x > xmax || z > zmax {
                continue;
            }

            let ground = match p.seat {
                Seat::Centre => centre,
                // Never over the centre's ground (at the foot of a cliff the ring's uphill side is high): it would float there.
                Seat::Average => ((centre + ring.sum::<f32>()) / 9.0).min(centre),
                Seat::Lowest => ring.fold(centre, f32::min),
            };

            let y = (ground - p.sink * h).max(centre - (1.0 - p.min_visible) * h);
            let yaw = rnd.unit() * 360.0;
            d.rocks.push(Placed { key: entry.key.clone(), x, y, z, scale, yaw });
        }
    }

    // Plants: at the foot, apart from each other and off the rocks.
    if !d.foot.is_empty() && !input.plants.entries.is_empty() {
        let count = (area(&d.foot) * p.plant_density).round() as usize;
        let rocks = d.rocks.iter().map(|r| (r.x, r.z, (input.rock_size)(&r.key).radius * r.scale * 0.8)).collect::<Vec<_>>();
        let mut tries = count * 30;
        while d.plants.len() < count && tries > 0 {
            tries -= 1;
            let (x, z) = rnd.point_in(g, &d.foot);
            let pick = rnd.unit();
            let (scale_r, yaw_r) = (rnd.unit(), rnd.unit());
            // By the water, none under it (a point in a cell at the waterline may be).
            let cell = |v: f32, v0: f32, n: usize| (((v - v0) / g.cell).round().max(0.0) as usize).min(n - 1);
            let dry = !input.kind.by_water() || g.height(x, z) >= base[cell(z, g.z0, g.nz) * g.nx + cell(x, g.x0, g.nx)];
            let clear = dry
                && buildings.clear(x, z, p.plant_radius)
                && d.plants.iter().all(|o| (o.x - x).hypot(o.z - z) >= p.plant_spacing)
                && rocks.iter().all(|&(rx, rz, rr)| (rx - x).hypot(rz - z) >= rr);
            if !clear || x < g.x0 || z < g.z0 || x > xmax || z > zmax {
                continue;
            }

            let Some(entry) = input.plants.pick(pick) else { break };
            let (lo, hi) = entry.scale.unwrap_or(p.plant_scale);
            d.plants.push(Placed { key: entry.key.clone(), x, y: g.height(x, z), z, scale: lo + (hi - lo) * scale_r, yaw: yaw_r * 360.0 });
        }
    }

    // The exclusion polygons: a convex hull of circles around each cluster's plants, kept on the tile.
    let pad = p.plant_radius + p.exclusion_margin;
    for cluster in clusters(&d.plants, p.cluster_distance, p.max_cluster) {
        let mut pts = vec![];
        for &k in &cluster {
            let pl = &d.plants[k];
            for s in 0..12 {
                let a = s as f32 * std::f32::consts::TAU / 12.0;
                pts.push(((pl.x + pad * a.cos()).clamp(g.x0, xmax), (pl.z + pad * a.sin()).clamp(g.z0, zmax)));
            }
        }

        let hull = convex_hull(pts);
        if hull.len() >= 3 {
            d.exclusions.push(hull);
        }
    }
}

/// Groups of plants (their indices): single linkage within `distance`, without letting a group's box get wider than `widest` across.
fn clusters(plants: &[Placed], distance: f32, widest: f32) -> Vec<Vec<usize>> {
    let mut group = (0..plants.len()).collect::<Vec<_>>();
    let mut boxes = plants.iter().map(|p| (p.x, p.z, p.x, p.z)).collect::<Vec<_>>();
    fn root(group: &mut [usize], mut i: usize) -> usize {
        while group[i] != i {
            group[i] = group[group[i]];
            i = group[i];
        }

        i
    }

    for a in 0..plants.len() {
        for b in a + 1..plants.len() {
            if (plants[a].x - plants[b].x).hypot(plants[a].z - plants[b].z) > distance {
                continue;
            }

            let (ra, rb) = (root(&mut group, a), root(&mut group, b));
            if ra == rb {
                continue;
            }

            let (pa, pb) = (boxes[ra], boxes[rb]);
            let merged = (pa.0.min(pb.0), pa.1.min(pb.1), pa.2.max(pb.2), pa.3.max(pb.3));
            if (merged.2 - merged.0).hypot(merged.3 - merged.1) > widest {
                continue;
            }

            group[rb] = ra;
            boxes[ra] = merged;
        }
    }

    let mut out: Vec<Vec<usize>> = vec![];
    let mut index = vec![NONE; plants.len()];
    for i in 0..plants.len() {
        let r = root(&mut group, i);
        if index[r] == NONE {
            index[r] = out.len();
            out.push(vec![]);
        }

        out[index[r]].push(i);
    }

    out
}

/// The convex hull (monotone chain), counter-clockwise in (x, z); no repeated or collinear points.
fn convex_hull(mut pts: Vec<(f32, f32)>) -> Vec<(f32, f32)> {
    pts.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }

    let cross = |o: (f32, f32), a: (f32, f32), b: (f32, f32)| (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0);
    let mut hull: Vec<(f32, f32)> = vec![];
    for pass in 0..2 {
        let start = hull.len();
        let iter: Box<dyn Iterator<Item = &(f32, f32)>> = if pass == 0 { Box::new(pts.iter()) } else { Box::new(pts.iter().rev()) };
        for &pt in iter {
            while hull.len() >= start + 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], pt) <= 0.0 {
                hull.pop();
            }

            hull.push(pt);
        }

        hull.pop();
    }

    hull
}

/// Whether a point is inside a polygon (even-odd).
pub fn inside(poly: &[(f32, f32)], x: f32, z: f32) -> bool {
    let mut is = false;
    let mut j = poly.len().wrapping_sub(1);
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a.1 > z) != (b.1 > z) && x < (b.0 - a.0) * (z - a.1) / (b.1 - a.1) + a.0 {
            is = !is;
        }

        j = i;
    }

    is
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sets() -> (VegetationSet, VegetationSet) {
        (VegetationSet::from_keys("rocks", ["gaul_rock_01", "gaul_rock_02"]), VegetationSet::from_keys("bushes", ["bush.bush_a"]))
    }

    fn size(_: &str) -> RockSize {
        RockSize { radius: 1.0, height: 1.0 }
    }

    /// 100 x 100 m: flat at 0 for x < 40, a slope of `rise` metres over `run` metres, flat on top.
    fn step(rise: f32, run: f32) -> Grid {
        Grid::sample(0.0, 0.0, 100.0, 100.0, 1.0, |x, _| ((x - 40.0) / run).clamp(0.0, 1.0) * rise)
    }

    fn run(g: &Grid, blockers: &[Blocker], seed: u64) -> Vec<Differential> {
        let (rocks, plants) = sets();
        generate(&Input { kind: Kind::Cliffs, grid: g, context: None, blockers, rocks: &rocks, plants: &plants, rock_size: &size, params: Params::default(), seed })
    }

    #[test]
    fn a_cliff_is_one_differential_with_its_bands() {
        let g = step(10.0, 5.0);
        let out = run(&g, &[], 7);
        let kept = out.iter().filter(|d| d.kept()).collect::<Vec<_>>();
        assert_eq!(kept.len(), 1, "{:?}", out.iter().map(|d| d.state).collect::<Vec<_>>());
        let d = kept[0];
        assert!((d.rise - 10.0).abs() < 0.5, "rise {}", d.rise);

        // The foot band is low (up to 3 m up the slope, out to 4 m before it), the face band above it and under 9 m.
        let p = Params::default();
        assert!(d.foot.iter().all(|&i| g.heights[i] <= p.foot_up + 0.01 && g.at(i).0 >= 40.0 - p.foot_out - 1.0));
        assert!(d.foot.iter().any(|&i| g.at(i).0 < 40.0), "the foot band goes out onto the flat");
        assert!(!d.face.is_empty() && d.face.iter().all(|&i| g.heights[i] > p.foot_up && g.heights[i] <= 10.0 - p.top_margin + 0.01));
        assert!(!d.rocks.is_empty() && !d.plants.is_empty());

        // The plants are at the foot, apart, and every one is inside an exclusion polygon.
        for pl in &d.plants {
            assert!(pl.x < 40.0 + 3.0 / 2.0 + 1.0, "plant at {}", pl.x);
            assert!(d.exclusions.iter().any(|poly| inside(poly, pl.x, pl.z)), "plant at ({}, {}) not excluded", pl.x, pl.z);
        }

        for (a, pa) in d.plants.iter().enumerate() {
            for pb in &d.plants[a + 1..] {
                assert!((pa.x - pb.x).hypot(pa.z - pb.z) >= p.plant_spacing);
            }
        }

        // A rock is sunk, but at least half of it stays over the ground at its centre.
        assert!(d.rocks.iter().all(|r| r.y < g.height(r.x, r.z)));
        assert!(d.rocks.iter().all(|r| r.y + 1.0 - g.height(r.x, r.z) >= 0.5 - 1e-4));
    }

    #[test]
    fn a_ramp_is_left_alone() {
        // 10 degrees.
        let g = step(10.0, 10.0 / 10f32.to_radians().tan());
        assert!(run(&g, &[], 1).iter().all(|d| !d.kept()));
    }

    #[test]
    fn a_low_step_is_too_low() {
        let g = step(1.5, 1.0);
        let out = run(&g, &[], 1);
        assert!(!out.is_empty() && out.iter().all(|d| matches!(d.state, Some(State::TooLow(_)))));
    }

    #[test]
    fn the_coverage_goes_around_a_building_on_the_cliff() {
        let g = step(10.0, 5.0);
        let b = Blocker { x: 42.0, z: 50.0, radius: 3.0 };
        let out = run(&g, &[b], 1);
        let d = out.iter().find(|d| d.kept()).expect("still covered");
        assert_eq!(d.avoided, 1);
        assert!(!d.rocks.is_empty() && !d.plants.is_empty());

        // Every rock and plant keeps the clearance (with its own radius) from the building's footprint.
        let p = Params::default();
        for r in &d.rocks {
            assert!((r.x - b.x).hypot(r.z - b.z) >= b.radius + p.building_clearance + 1.0, "rock at {}, {}", r.x, r.z);
        }

        for pl in &d.plants {
            assert!((pl.x - b.x).hypot(pl.z - b.z) >= b.radius + p.building_clearance + p.plant_radius, "plant at {}, {}", pl.x, pl.z);
        }

        // Far from it: nothing avoided.
        let out = run(&g, &[Blocker { x: 10.0, z: 50.0, radius: 3.0 }], 1);
        assert!(out.iter().filter(|d| d.kept()).all(|d| d.avoided == 0));
    }

    #[test]
    fn the_same_seed_gives_the_same_result() {
        let g = step(10.0, 5.0);
        assert_eq!(run(&g, &[], 42), run(&g, &[], 42));
        assert_ne!(run(&g, &[], 42), run(&g, &[], 43));
    }

    #[test]
    fn the_parameters_read_back_from_their_text() {
        let p = Params { slope_deg: 40.5, scale_rocks: true, rock_scale_face: (0.25, 0.75), seat: Seat::Lowest, min_visible: 0.3, ..Default::default() };
        assert_eq!(Params::from_text(&p.to_text()), p);
        assert_eq!(Params::from_text("min_rise=7 nonsense nope=1"), Params { min_rise: 7.0, ..Default::default() });
    }

    #[test]
    fn rocks_are_at_scale_1_unless_scaled() {
        let g = step(10.0, 5.0);
        assert!(run(&g, &[], 5).iter().flat_map(|d| &d.rocks).all(|r| r.scale == 1.0));
        let (rocks, plants) = sets();
        let params = Params { scale_rocks: true, ..Default::default() };
        let out = generate(&Input { kind: Kind::Cliffs, grid: &g, context: None, blockers: &[], rocks: &rocks, plants: &plants, rock_size: &size, params, seed: 5 });
        assert!(out.iter().flat_map(|d| &d.rocks).any(|r| r.scale != 1.0));
    }

    /// A grid of `height(x, z)` over 100 x 60 m, its no-go field the samples steeper than 30 degrees, and `depth(x)` metres of water; a
    /// tile that meets the sea.
    fn field(height: impl Fn(f32, f32) -> f32, depth: impl Fn(f32) -> f32) -> (Grid, Context) {
        let g = Grid::sample(0.0, 0.0, 100.0, 60.0, 1.0, height);
        let tan = 30f32.to_radians().tan();
        let no_go = (0..g.nx * g.nz).map(|i| slope(&g, i) > tan).collect();
        let water_depth = (0..g.nx * g.nz).map(|i| depth(g.at(i).0)).collect();
        (g, Context { water_depth, no_go, has_sea: true })
    }

    fn run_kind(kind: Kind, g: &Grid, c: &Context) -> Vec<Differential> {
        let (rocks, plants) = sets();
        let params = Params::for_kind(kind);
        generate(&Input { kind, grid: g, context: Some(c), blockers: &[], rocks: &rocks, plants: &plants, rock_size: &size, params, seed: 3 })
    }

    fn kept(out: &[Differential]) -> Vec<&Differential> {
        out.iter().filter(|d| d.kept()).collect()
    }

    /// The sea (2 m deep) up to x = 40, a cliff up to 10 m over 5 m.
    fn sea_cliff(x: f32, _: f32) -> f32 {
        -2.0 + ((x - 40.0) / 5.0).clamp(0.0, 1.0) * 12.0
    }

    #[test]
    fn a_cliff_at_the_sea_is_the_coasts() {
        let (g, c) = field(sea_cliff, |_| 0.0);
        let cliffs = run_kind(Kind::Cliffs, &g, &c);
        assert!(kept(&cliffs).is_empty());
        assert!(cliffs.iter().any(|d| d.state == Some(State::ByWater)), "{:?}", cliffs.iter().map(|d| d.state).collect::<Vec<_>>());

        let coast = run_kind(Kind::Coast, &g, &c);
        let shore = kept(&coast);
        assert_eq!(shore.len(), 1, "{:?}", coast.iter().map(|d| d.state).collect::<Vec<_>>());
        assert!((shore[0].rise - 10.0).abs() < 0.5, "measured from the sea: {}", shore[0].rise);
        assert!(!shore[0].rocks.is_empty());
        assert!(shore[0].plants.iter().all(|pl| g.height(pl.x, pl.z) >= 0.0), "no plant under the sea");
    }

    #[test]
    fn a_cliff_over_the_sea_stays_a_cliff() {
        // The sea up to x = 10, a beach up to 5 m by x = 30 (19 degrees: walkable), the cliff 10 m from the beach's top.
        let (g, c) = field(|x, _| -2.0 + ((x - 10.0) / 20.0).clamp(0.0, 1.0) * 7.0 + ((x - 40.0) / 5.0).clamp(0.0, 1.0) * 10.0, |_| 0.0);
        assert_eq!(kept(&run_kind(Kind::Cliffs, &g, &c)).len(), 1);
        assert!(kept(&run_kind(Kind::Coast, &g, &c)).is_empty(), "too far from the sea");
    }

    #[test]
    fn a_tile_away_from_the_sea_has_none_under_0_m() {
        let (g, mut c) = field(|x, z| sea_cliff(x, z) - 20.0, |_| 0.0);
        c.has_sea = false;
        assert_eq!(kept(&run_kind(Kind::Cliffs, &g, &c)).len(), 1, "an inland cliff, all under 0 m");
        assert!(run_kind(Kind::Coast, &g, &c).is_empty());
    }

    #[test]
    fn a_pond_under_the_sea_level_is_not_the_sea() {
        // 5 m high, a pit to -3 m in the middle with steep sides, away from the edges.
        let (g, c) = field(|x, z| 5.0 - 8.0 * (1.0 - ((x - 50.0).hypot(z - 30.0) - 8.0) / 3.0).clamp(0.0, 1.0), |_| 0.0);
        assert!(kept(&run_kind(Kind::Coast, &g, &c)).is_empty());
    }

    /// 3 m high, a river 10 m wide (its bed at 0 m, its surface at 1 m) with banks 4 m wide on each side (37 degrees).
    fn river(x: f32, _: f32) -> f32 {
        let d = (x - 50.0).abs();
        3.0 * ((d - 5.0) / 4.0).clamp(0.0, 1.0)
    }

    fn river_depth(x: f32) -> f32 {
        (1.0 - river(x, 0.0)).max(0.0)
    }

    #[test]
    fn a_steep_bank_is_covered_from_the_waters_surface() {
        let (g, mut c) = field(river, river_depth);
        c.has_sea = false;
        let out = run_kind(Kind::Riverbank, &g, &c);
        let banks = kept(&out);
        assert_eq!(banks.len(), 2, "{:?}", out.iter().map(|d| (d.state, d.rise)).collect::<Vec<_>>());
        for d in banks {
            assert!((d.rise - 2.0).abs() < 0.5, "from the surface (1 m) to the top (3 m): {}", d.rise);
            assert!(!d.wet.is_empty(), "the waterline band reaches into the water");
            assert!(d.wet.iter().all(|&i| g.heights[i] >= 1.0 - 0.6 - 1e-3), "no deeper than water_below");
            assert!(d.plants.iter().all(|pl| g.height(pl.x, pl.z) >= 1.0 - 0.2), "no plant under the water");
        }

        let cliffs = run_kind(Kind::Cliffs, &g, &c);
        assert!(kept(&cliffs).is_empty() && cliffs.iter().any(|d| d.state == Some(State::ByWater)), "the banks', not a cliff");
    }

    #[test]
    fn a_walkable_bank_is_left_alone() {
        let (g, mut c) = field(river, river_depth);
        c.no_go.iter_mut().for_each(|b| *b = false);
        assert!(run_kind(Kind::Riverbank, &g, &c).is_empty());
    }

    #[test]
    fn the_context_takes_the_nearest_vertex() {
        // Stage B's fields every 2 m (3 x 2 vertices, row j along +z); the grid every metre over 4 x 2 m.
        let g = Grid::sample(0.0, 0.0, 4.0, 2.0, 1.0, |_, _| 0.0);
        let depth = [f32::NAN, 0.5, 2.0, -1000.0, 0.0, 3.0];
        let no_go = [false, true, false, false, false, true];
        let c = Context::resample(&g, 3, 2, 2.0, 2.0, &depth, &no_go);
        assert_eq!(c.water_depth[..5], [0.0, 0.5, 0.5, 2.0, 2.0]);
        assert_eq!(c.no_go[..5], [false, true, true, false, false]);
        assert_eq!(c.water_depth[10..], [0.0, 0.0, 0.0, 3.0, 3.0]);
        assert_eq!(c.no_go[10..], [false, false, false, true, true]);
    }

    #[test]
    fn a_hull_is_counter_clockwise_and_convex() {
        let hull = convex_hull(vec![(0.0, 0.0), (2.0, 0.0), (1.0, 1.0), (2.0, 2.0), (0.0, 2.0), (1.0, 0.0)]);
        assert_eq!(hull, vec![(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]);
    }
}
