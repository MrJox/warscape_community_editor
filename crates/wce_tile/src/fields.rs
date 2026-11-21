//! Fields and farmland (docs/plans/auto_detail_2.md, G): an area (a vegetation exclusion polygon, as a biome's: the game's own trees keep
//! off it) cut into farms by lanes (a dirt track, or a belt of trees and shrubs), each farm cut into parcels by verges, and each parcel
//! into strips: a crop (the kit's field decals laid end to end along it, as its farm tiles lay them) or an orchard (rows of trees), with
//! haystacks and carts here and there. A share of the farms is fenced (a gate left in it); the ground under the area is worn to a texture.
//! The same area, settings and seed lay the same farmland; [`presets`] are ready-made settings.
//!
//! Everything here is in metres on the ground's plane (x, z); the editor turns it into decals and instances.

use std::fmt::Write as _;

/// The area's settings ([`FieldSettings::to_text`]) and its seed, on the polygon.
pub const FIELDS_ATTR: &str = "wce_fields";
pub const SEED_ATTR: &str = "wce_fields_seed";

/// What the farmland is made of.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldSettings {
    /// The largest side of a farm (metres; the area is cut by lanes until no farm is larger), and the lanes' width. 0: one farm.
    pub farm_size: f32,
    pub lane_width: f32,

    /// The share of the lanes that are tracks (the rest are belts of trees and shrubs), the tracks' decals (their long side along the
    /// track), the belts' trees and shrubs and how far apart the trees are.
    pub tracks: f32,
    pub track_decals: Vec<String>,
    pub belt_trees: Vec<String>,
    pub belt_shrubs: Vec<String>,
    pub belt_spacing: f32,

    /// The largest side of a parcel (a farm is cut by verges until no parcel is larger; 0: one parcel a farm), and the share of the
    /// parcels whose strips run across the others (a patchwork).
    pub parcel_size: f32,
    pub patchwork: f32,

    /// How wide a strip is, and the verge between two strips or two parcels (metres).
    pub strip_width: f32,
    pub verge: f32,

    /// Which way the strips run (degrees from +x towards +z); none: along the area's longest side.
    pub angle: Option<f32>,

    /// The crops' decals (a name under `rigidmodels\decals\battle`, as the decal rules'), one picked a parcel, and how long a decal is.
    pub crops: Vec<String>,
    pub crop_length: f32,

    /// The share of the parcels that are orchards (0 .. 1), their trees, how many rows a strip and how far apart along a row.
    pub orchard: f32,
    pub orchard_trees: Vec<String>,
    pub rows: u32,
    pub tree_spacing: f32,

    /// The fence round a farm (building keys, laid end to end as a building spline's; none: no fences), the share of the farms that have
    /// one, and the gap left for a gate (metres; 0: none).
    pub fence: Vec<String>,
    pub fenced: f32,
    pub gate: f32,

    /// Props here and there on the crops (haystacks, carts), how many a hectare.
    pub props: Vec<String>,
    pub props_per_ha: f32,

    /// The ground under the area: a texture group (several separated by `|`, the first the tile has), its variation and strength; an
    /// empty group: none.
    pub ground: String,
    pub ground_variation: f32,
    pub ground_strength: f32,
}

fn keys(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_owned).collect()
}

impl Default for FieldSettings {
    /// The "Patchwork farms" preset.
    fn default() -> Self {
        Self {
            farm_size: 130.0,
            lane_width: 6.0,
            tracks: 0.5,
            track_decals: keys("path_1 global_terrain_path_01"),
            belt_trees: keys("oak.oak_g oak.oak_f beech.beech_e birch.birch_d"),
            belt_shrubs: keys("shrubs.shrub_rosec shrubs.shrub_roseb shrubs.shrub_burdockb shrubs.shrub_tansyb"),
            belt_spacing: 5.0,
            parcel_size: 60.0,
            patchwork: 0.4,
            strip_width: 12.0,
            verge: 1.5,
            angle: None,
            crops: keys("global_fields_1 global_fields_2 global_fields_3 global_fields_4 global_fields_5 global_fields_6"),
            crop_length: 25.0,
            orchard: 0.15,
            orchard_trees: keys("olive.olive_a olive.olive_b olive.olive_c olive.olive_d"),
            rows: 2,
            tree_spacing: 6.0,
            fence: keys("lowfence_1 lowfence_2 lowfence_3"),
            fenced: 0.6,
            gate: 5.0,
            props: keys("hay01 cart_1_hay"),
            props_per_ha: 4.0,
            ground: "mud|mud_a|mud_b|settlement_2".to_owned(),
            ground_variation: 1.0,
            ground_strength: 0.5,
        }
    }
}

/// A ready-made kind of farmland: its name, what it is, and its settings.
pub struct Preset {
    pub name: &'static str,
    pub tip: &'static str,
    pub settings: FieldSettings,
}

/// The built-in kinds of farmland, the default first.
#[must_use]
pub fn presets() -> Vec<Preset> {
    let d = FieldSettings::default;
    vec![
        Preset {
            name: "Patchwork farms",
            tip: "Farms of a few parcels each, some fenced, with tracks and belts of trees between them",
            settings: d(),
        },
        Preset {
            name: "Hedged fields",
            tip: "Fields walled in by belts of trees and shrubs (a bocage), a track now and then; no fences",
            settings: FieldSettings {
                farm_size: 80.0,
                lane_width: 10.0,
                tracks: 0.2,
                belt_spacing: 4.0,
                parcel_size: 0.0,
                patchwork: 0.5,
                orchard: 0.1,
                fenced: 0.0,
                ..d()
            },
        },
        Preset {
            name: "Fenced smallholdings",
            tip: "Small plots, each fenced with a gate, along tracks; more orchards, haystacks and carts",
            settings: FieldSettings {
                farm_size: 55.0,
                lane_width: 5.0,
                tracks: 0.85,
                parcel_size: 0.0,
                patchwork: 0.5,
                strip_width: 9.0,
                orchard: 0.25,
                fenced: 1.0,
                props_per_ha: 8.0,
                ..d()
            },
        },
        Preset {
            name: "Open strip fields",
            tip: "One great field in long narrow strips, as a village's open field; no lanes, no fences",
            settings: FieldSettings {
                farm_size: 0.0,
                parcel_size: 0.0,
                patchwork: 0.0,
                strip_width: 7.0,
                verge: 1.0,
                orchard: 0.0,
                fenced: 0.0,
                props_per_ha: 2.0,
                ..d()
            },
        },
        Preset {
            name: "Orchards",
            tip: "Mostly rows of trees, a few crops between, fenced farms along tracks",
            settings: FieldSettings {
                farm_size: 110.0,
                tracks: 0.7,
                parcel_size: 55.0,
                patchwork: 0.3,
                orchard: 0.8,
                rows: 3,
                fenced: 0.4,
                props_per_ha: 2.0,
                ..d()
            },
        },
    ]
}

impl FieldSettings {
    /// As one attribute's text: `key=value` pairs separated by `;`, lists by `,`.
    #[must_use]
    pub fn to_text(&self) -> String {
        let num = |v: f32| crate::ted_scene::format_number(f64::from(v));
        let mut t = String::new();
        let _ = write!(t, "strip={};verge={}", num(self.strip_width), num(self.verge));
        if let Some(a) = self.angle {
            let _ = write!(t, ";angle={}", num(a));
        }

        let _ = write!(
            t,
            ";farm={};lane={};tracks={};track_decals={};belt_trees={};belt_shrubs={};belt_spacing={};parcel={};patchwork={}",
            num(self.farm_size),
            num(self.lane_width),
            num(self.tracks),
            self.track_decals.join(","),
            self.belt_trees.join(","),
            self.belt_shrubs.join(","),
            num(self.belt_spacing),
            num(self.parcel_size),
            num(self.patchwork),
        );
        let _ = write!(
            t,
            ";crops={};crop_length={};orchard={};orchard_trees={};rows={};tree_spacing={};fence={};fenced={};gate={};props={};props_ha={};ground={};ground_variation={};ground_strength={}",
            self.crops.join(","),
            num(self.crop_length),
            num(self.orchard),
            self.orchard_trees.join(","),
            self.rows,
            num(self.tree_spacing),
            self.fence.join(","),
            num(self.fenced),
            num(self.gate),
            self.props.join(","),
            num(self.props_per_ha),
            self.ground,
            num(self.ground_variation),
            num(self.ground_strength),
        );
        t
    }

    /// From its text ([`Self::to_text`]): what is missing or wrong is the default's.
    #[must_use]
    pub fn from_text(text: &str) -> Self {
        let mut s = Self::default();
        let list = |v: &str| v.split(',').map(str::trim).filter(|k| !k.is_empty()).map(str::to_owned).collect::<Vec<_>>();
        for pair in text.split(';') {
            let Some((k, v)) = pair.split_once('=') else { continue };
            let v = v.trim();
            let f = v.parse::<f32>().ok().filter(|f| f.is_finite());
            match (k.trim(), f) {
                ("farm", Some(f)) => s.farm_size = f.clamp(0.0, 2000.0),
                ("lane", Some(f)) => s.lane_width = f.clamp(0.0, 50.0),
                ("tracks", Some(f)) => s.tracks = f.clamp(0.0, 1.0),
                ("track_decals", _) => s.track_decals = list(v),
                ("belt_trees", _) => s.belt_trees = list(v),
                ("belt_shrubs", _) => s.belt_shrubs = list(v),
                ("belt_spacing", Some(f)) => s.belt_spacing = f.clamp(1.0, 100.0),
                ("parcel", Some(f)) => s.parcel_size = f.clamp(0.0, 2000.0),
                ("patchwork", Some(f)) => s.patchwork = f.clamp(0.0, 1.0),
                ("strip", Some(f)) => s.strip_width = f.clamp(1.0, 200.0),
                ("verge", Some(f)) => s.verge = f.clamp(0.0, 50.0),
                ("angle", Some(f)) => s.angle = Some(f),
                ("crops", _) => s.crops = list(v),
                ("crop_length", Some(f)) => s.crop_length = f.clamp(1.0, 200.0),
                ("orchard", Some(f)) => s.orchard = f.clamp(0.0, 1.0),
                ("orchard_trees", _) => s.orchard_trees = list(v),
                ("rows", Some(f)) => s.rows = (f as u32).clamp(1, 20),
                ("tree_spacing", Some(f)) => s.tree_spacing = f.clamp(1.0, 100.0),
                ("fence", _) => s.fence = list(v),
                ("fenced", Some(f)) => s.fenced = f.clamp(0.0, 1.0),
                ("gate", Some(f)) => s.gate = f.clamp(0.0, 50.0),
                ("props", _) => s.props = list(v),
                ("props_ha", Some(f)) => s.props_per_ha = f.clamp(0.0, 1000.0),
                ("ground", _) => s.ground = v.to_owned(),
                ("ground_variation", Some(f)) => s.ground_variation = f.clamp(0.0, 3.0),
                ("ground_strength", Some(f)) => s.ground_strength = f.clamp(0.0, 1.0),
                _ => {}
            }
        }

        s
    }

    /// The preset these settings are, if any.
    #[must_use]
    pub fn preset_name(&self) -> Option<&'static str> {
        presets().into_iter().find(|p| p.settings == *self).map(|p| p.name)
    }
}

/// A decal: its name (as the settings name it), where its middle is, which way its length runs (radians: its length along (sin, cos)
/// in x and z, as a decal rule's) and its size (`across`: its x, `length`: its z).
#[derive(Clone, Debug, PartialEq)]
pub struct CropDecal {
    pub key: String,
    pub x: f32,
    pub z: f32,
    pub yaw: f32,
    pub across: f32,
    pub length: f32,
}

/// A tree, a shrub or a prop: its key, where, its turn (degrees) and its scale.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub key: String,
    pub x: f32,
    pub z: f32,
    pub yaw: f32,
    pub scale: f32,
}

/// A strip: its middle line's ends (its pieces inside the area) and what it holds.
#[derive(Clone, Debug, PartialEq)]
pub struct Strip {
    pub pieces: Vec<[(f32, f32); 2]>,
    pub orchard: bool,
    pub crop: Option<String>,
}

/// A lane between farms: its middle line's pieces inside the area, its width and whether it is a track (else a belt of trees).
#[derive(Clone, Debug, PartialEq)]
pub struct Lane {
    pub pieces: Vec<[(f32, f32); 2]>,
    pub width: f32,
    pub track: bool,
}

/// A farm's fence: its line round the farm (x, z), open where its gate is; `closed`: no gate, the last point joins the first.
#[derive(Clone, Debug, PartialEq)]
pub struct Fence {
    pub points: Vec<(f32, f32)>,
    pub closed: bool,
}

/// What an area lays.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Farmland {
    pub strips: Vec<Strip>,
    pub lanes: Vec<Lane>,
    pub fences: Vec<Fence>,
    pub decals: Vec<CropDecal>,
    pub trees: Vec<Item>,
    pub props: Vec<Item>,
}

/// The way the strips run for an area: the settings' angle, else its longest side's (radians from +x towards +z).
#[must_use]
pub fn strip_angle(poly: &[(f32, f32)], s: &FieldSettings) -> f32 {
    if let Some(a) = s.angle {
        return a.to_radians();
    }

    let n = poly.len();
    (0..n)
        .map(|i| (poly[i], poly[(i + 1) % n]))
        .max_by(|a, b| {
            let len = |(p, q): &((f32, f32), (f32, f32))| (q.0 - p.0).hypot(q.1 - p.1);
            len(a).total_cmp(&len(b))
        })
        .map_or(0.0, |(p, q)| (q.1 - p.1).atan2(q.0 - p.0))
}

/// A rectangle in the area's frame: u from `.0` to `.2`, v from `.1` to `.3`.
type Rect = (f32, f32, f32, f32);

/// The most farms, parcels or lanes an area is cut into.
const MOST_PIECES: usize = 4000;

/// Cuts `r` across its longer side (somewhere about its middle) until no piece is longer than `most` on a side (0: not cut), leaving
/// `gap` between two pieces; the pieces go to `out`, and each gap (whole across the piece it cut: lanes meet) to `gaps`, with whether it
/// runs along u.
fn cut(r: Rect, most: f32, gap: f32, rnd: &mut Random, out: &mut Vec<Rect>, gaps: &mut Vec<(Rect, bool)>) {
    let (w, h) = (r.2 - r.0, r.3 - r.1);
    if most <= 0.0 || (w <= most && h <= most) || out.len() + gaps.len() >= MOST_PIECES {
        out.push(r);
        return;
    }

    let t = 0.38 + 0.24 * rnd.unit();
    if w >= h {
        let at = r.0 + w * t;
        if (at - gap * 0.5 - r.0).min(r.2 - at - gap * 0.5) < 1.0 {
            out.push(r);
            return;
        }

        gaps.push(((at - gap * 0.5, r.1, at + gap * 0.5, r.3), false));
        cut((r.0, r.1, at - gap * 0.5, r.3), most, gap, rnd, out, gaps);
        cut((at + gap * 0.5, r.1, r.2, r.3), most, gap, rnd, out, gaps);
    } else {
        let at = r.1 + h * t;
        if (at - gap * 0.5 - r.1).min(r.3 - at - gap * 0.5) < 1.0 {
            out.push(r);
            return;
        }

        gaps.push(((r.0, at - gap * 0.5, r.2, at + gap * 0.5), true));
        cut((r.0, r.1, r.2, at - gap * 0.5), most, gap, rnd, out, gaps);
        cut((r.0, at + gap * 0.5, r.2, r.3), most, gap, rnd, out, gaps);
    }
}

/// One of a list, at random.
fn pick<'a>(list: &'a [String], rnd: &mut Random) -> &'a str {
    &list[((rnd.unit() * list.len() as f32) as usize).min(list.len() - 1)]
}

/// The area in a frame (a, b) with the strips (or a lane) along a: u and v as they are, or swapped.
struct Frame {
    poly: Vec<(f32, f32)>,
    along_u: bool,
}

impl Frame {
    fn new(uv: &[(f32, f32)], along_u: bool) -> Self {
        Self { poly: uv.iter().map(|&(u, v)| if along_u { (u, v) } else { (v, u) }).collect(), along_u }
    }

    /// A rectangle of the area's frame in this one: a from, b from, a to, b to.
    fn rect(&self, r: Rect) -> Rect {
        if self.along_u { r } else { (r.1, r.0, r.3, r.2) }
    }

    /// A point of this frame in the area's (u, v).
    fn uv(&self, (a, b): (f32, f32)) -> (f32, f32) {
        if self.along_u { (a, b) } else { (b, a) }
    }

    /// Where the line across at `b`, and lines `half` to each side of it, are all inside the area, within a0 .. a1; pieces of at least
    /// `least`.
    fn pieces(&self, b: f32, half: f32, (a0, a1): (f32, f32), least: f32) -> Vec<(f32, f32)> {
        let inside = intersect(&intersect(&spans(&self.poly, b), &spans(&self.poly, b - half + 0.01)), &spans(&self.poly, b + half - 0.01));
        intersect(&inside, &[(a0, a1)]).into_iter().filter(|(lo, hi)| hi - lo >= least).collect()
    }
}

/// The farmland of an area (`poly`: x and z, metres; either winding), from its settings and seed.
#[must_use]
pub fn lay(poly: &[(f32, f32)], s: &FieldSettings, seed: u32) -> Farmland {
    let mut out = Farmland::default();
    if poly.len() < 3 {
        return out;
    }

    // The area turned so that the strips run along +u, across +v.
    let a = strip_angle(poly, s);
    let (sin, cos) = a.sin_cos();
    let to_uv = |(x, z): (f32, f32)| (x * cos + z * sin, -x * sin + z * cos);
    let to_xz = |(u, v): (f32, f32)| (u * cos - v * sin, u * sin + v * cos);
    let uv = poly.iter().map(|&p| to_uv(p)).collect::<Vec<_>>();
    let bounds = uv.iter().fold((f32::MAX, f32::MAX, f32::MIN, f32::MIN), |b, p| (b.0.min(p.0), b.1.min(p.1), b.2.max(p.0), b.3.max(p.1)));
    // Which way a frame's +a runs, as a decal's yaw (its length along (sin, cos) in x and z).
    let yaw_of = |along_u: bool| {
        let (dx, dz) = if along_u { (cos, sin) } else { (-sin, cos) };
        dx.atan2(dz)
    };

    let mut rnd = Random::new(u64::from(seed));
    let lane = if s.farm_size > 0.0 { s.lane_width.max(0.0) } else { 0.0 };
    let (mut farms, mut lanes) = (vec![], vec![]);
    cut(bounds, s.farm_size, lane, &mut rnd, &mut farms, &mut lanes);

    // The lanes: tracks of decals, or belts of trees and shrubs.
    for (r, along_u) in lanes {
        let f = Frame::new(&uv, along_u);
        let (a0, b0, a1, b1) = f.rect(r);
        let (mid, width) = ((b0 + b1) * 0.5, b1 - b0);
        let pieces = f.pieces(mid, (width * 0.5).min(1.0), (a0, a1), 4.0);
        let track = !s.track_decals.is_empty() && (s.belt_trees.is_empty() || rnd.unit() < s.tracks);
        let yaw = yaw_of(along_u);
        for &(lo, hi) in &pieces {
            if track {
                lay_track(&mut out, s, &mut rnd, &|p| to_xz(f.uv(p)), yaw, (lo, hi), mid, width);
            } else {
                lay_belt(&mut out, s, &mut rnd, &|p| to_xz(f.uv(p)), (lo, hi), mid, width);
            }
        }

        if !pieces.is_empty() {
            let line = pieces.iter().map(|&(lo, hi)| [to_xz(f.uv((lo, mid))), to_xz(f.uv((hi, mid)))]).collect();
            out.lanes.push(Lane { pieces: line, width, track });
        }
    }

    for farm in farms {
        // Its fence: round the part of it inside the area, a gate left in its longest side.
        if !s.fence.is_empty() && rnd.unit() < s.fenced {
            let ring = clip(&uv, farm);
            if area(&ring).abs() > 50.0 {
                out.fences.push(fence(&ring.iter().map(|&p| to_xz(p)).collect::<Vec<_>>(), s.gate));
            }
        }

        // Its parcels, inside its edge.
        let inner = (farm.0 + s.verge, farm.1 + s.verge, farm.2 - s.verge, farm.3 - s.verge);
        if inner.2 - inner.0 < 2.0 || inner.3 - inner.1 < 2.0 {
            continue;
        }

        let (mut parcels, mut verges) = (vec![], vec![]);
        cut(inner, s.parcel_size, s.verge.max(0.0), &mut rnd, &mut parcels, &mut verges);
        for parcel in parcels {
            let along_u = rnd.unit() >= s.patchwork;
            lay_parcel(&mut out, s, &mut rnd, &Frame::new(&uv, along_u), parcel, &|p| to_xz(p), yaw_of(along_u));
        }
    }

    out
}

/// A parcel's strips: one crop (its decals end to end along each strip, props on them) or orchard rows, the strips along the frame's a.
fn lay_parcel(
    out: &mut Farmland,
    s: &FieldSettings,
    rnd: &mut Random,
    f: &Frame,
    parcel: Rect,
    to_xz: &dyn Fn((f32, f32)) -> (f32, f32),
    yaw: f32,
) {
    let (a0, b0, a1, b1) = f.rect(parcel);
    let width = s.strip_width.max(1.0).min(b1 - b0);
    let orchard = !s.orchard_trees.is_empty() && rnd.unit() < s.orchard;
    let crop = (!orchard && !s.crops.is_empty()).then(|| pick(&s.crops, rnd).to_owned());
    let at = |a: f32, b: f32| to_xz(f.uv((a, b)));
    let mut b = b0 + width * 0.5;
    while b + width * 0.5 <= b1 + 1e-3 && out.strips.len() < MOST_PIECES {
        let pieces = f.pieces(b, width * 0.5, (a0, a1), width.min(4.0));
        for &(lo, hi) in &pieces {
            if let Some(key) = &crop {
                let n = ((hi - lo) / s.crop_length.max(1.0)).ceil().max(1.0) as usize;
                let len = (hi - lo) / n as f32;
                for i in 0..n {
                    let (x, z) = at(lo + (i as f32 + 0.5) * len, b);
                    out.decals.push(CropDecal { key: key.clone(), x, z, yaw, across: width, length: len });
                }

                // Props on the crops: here and there, by the area.
                let count = (hi - lo) * width / 10_000.0 * s.props_per_ha;
                let count = count.floor() as usize + usize::from(rnd.unit() < count.fract());
                for _ in 0..count {
                    if s.props.is_empty() {
                        break;
                    }

                    let (x, z) = at(lo + rnd.unit() * (hi - lo), b + (rnd.unit() - 0.5) * width * 0.8);
                    let key = pick(&s.props, rnd).to_owned();
                    out.props.push(Item { key, x, z, yaw: rnd.unit() * 360.0, scale: 1.0 });
                }
            } else if orchard {
                let rows = s.rows.max(1);
                let spacing = s.tree_spacing.max(1.0);
                for r in 0..rows {
                    let rb = b - width * 0.5 + width * (r as f32 + 0.5) / rows as f32;
                    let mut a = lo + spacing * 0.5;
                    while a <= hi - spacing * 0.25 {
                        let jitter = (rnd.unit() - 0.5) * spacing * 0.15;
                        let (x, z) = at(a + jitter, rb);
                        let key = pick(&s.orchard_trees, rnd).to_owned();
                        out.trees.push(Item { key, x, z, yaw: rnd.unit() * 360.0, scale: 0.85 + rnd.unit() * 0.3 });
                        a += spacing;
                    }
                }
            }
        }

        out.strips.push(Strip { pieces: pieces.iter().map(|&(lo, hi)| [at(lo, b), at(hi, b)]).collect(), orchard, crop: crop.clone() });
        b += width + s.verge.max(0.0);
    }
}

/// A track along a lane's piece: its decals end to end, each its own shape (the kit's size of it) as wide as the lane, its long side
/// along the lane.
#[allow(clippy::too_many_arguments)]
fn lay_track(
    out: &mut Farmland,
    s: &FieldSettings,
    rnd: &mut Random,
    at: &dyn Fn((f32, f32)) -> (f32, f32),
    yaw: f32,
    (lo, hi): (f32, f32),
    mid: f32,
    width: f32,
) {
    let key = pick(&s.track_decals, rnd).to_owned();
    let [sx, _, sz] = crate::decal_sizes::decal_size(&crate::decal_rules::key_of(&key)).unwrap_or([2.0, 10.0, 1.0]);
    let (sx, sz) = (sx.max(0.1), sz.max(0.1));
    // Its long side along the lane: its x when that is longer (the decal turned a quarter), else its z.
    let x_along = sx >= sz;
    let step = width * if x_along { sx / sz } else { sz / sx };
    let n = ((hi - lo) / step.max(1.0)).round().max(1.0) as usize;
    let len = (hi - lo) / n as f32;
    for i in 0..n {
        let (x, z) = at((lo + (i as f32 + 0.5) * len, mid));
        let decal = if x_along {
            CropDecal { key: key.clone(), x, z, yaw: yaw + std::f32::consts::FRAC_PI_2, across: len, length: width }
        } else {
            CropDecal { key: key.clone(), x, z, yaw, across: width, length: len }
        };
        out.decals.push(decal);
    }
}

/// A belt of trees along a lane's piece: a row of trees (two, staggered, on a lane 6 m wide or more), shrubs between and under them.
fn lay_belt(
    out: &mut Farmland,
    s: &FieldSettings,
    rnd: &mut Random,
    at: &dyn Fn((f32, f32)) -> (f32, f32),
    (lo, hi): (f32, f32),
    mid: f32,
    width: f32,
) {
    let spacing = s.belt_spacing.max(1.0);
    let rows: &[f32] = if width >= 6.0 { &[-0.22, 0.22] } else { &[0.0] };
    for (r, &off) in rows.iter().enumerate() {
        // Rows staggered by half a step.
        let mut a = lo + spacing * (0.5 + 0.5 * r as f32);
        while a <= hi - spacing * 0.25 && !s.belt_trees.is_empty() {
            let (x, z) = at((a + (rnd.unit() - 0.5) * spacing * 0.4, mid + off * width + (rnd.unit() - 0.5) * width * 0.2));
            let key = pick(&s.belt_trees, rnd).to_owned();
            out.trees.push(Item { key, x, z, yaw: rnd.unit() * 360.0, scale: 0.8 + rnd.unit() * 0.4 });
            a += spacing;
        }
    }

    // Shrubs: about three a step along it, across the lane.
    if !s.belt_shrubs.is_empty() {
        let count = ((hi - lo) / spacing * 3.0).round() as usize;
        for _ in 0..count {
            let (x, z) = at((lo + rnd.unit() * (hi - lo), mid + (rnd.unit() - 0.5) * width * 0.8));
            let key = pick(&s.belt_shrubs, rnd).to_owned();
            out.trees.push(Item { key, x, z, yaw: rnd.unit() * 360.0, scale: 0.8 + rnd.unit() * 0.5 });
        }
    }
}

/// A fence round `ring` (x, z): open at the middle of its longest side for a `gate` metres wide, else closed.
fn fence(ring: &[(f32, f32)], gate: f32) -> Fence {
    let n = ring.len();
    let len = |i: usize| {
        let (p, q) = (ring[i], ring[(i + 1) % n]);
        (q.0 - p.0).hypot(q.1 - p.1)
    };
    let Some(i) = (0..n).max_by(|&a, &b| len(a).total_cmp(&len(b))).filter(|&i| gate > 0.0 && len(i) > gate + 4.0) else {
        return Fence { points: ring.to_vec(), closed: true };
    };

    // From the gate's far post round to its near one.
    let (p, q) = (ring[i], ring[(i + 1) % n]);
    let l = len(i);
    let lerp = |t: f32| (p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t);
    let (near, far) = (lerp(0.5 - gate * 0.5 / l), lerp(0.5 + gate * 0.5 / l));
    let mut points = vec![far];
    points.extend((1..=n).map(|k| ring[(i + k) % n]));
    points.push(near);
    Fence { points, closed: false }
}

/// The part of a polygon inside a rectangle (Sutherland and Hodgman: a concave polygon may keep a seam along the rectangle's edge).
fn clip(poly: &[(f32, f32)], r: Rect) -> Vec<(f32, f32)> {
    let mut out = poly.to_vec();
    // Each edge: inside when `side(p) >= 0`, and where a segment crosses it.
    let edges: [(usize, f32, f32); 4] = [(0, r.0, 1.0), (0, r.2, -1.0), (1, r.1, 1.0), (1, r.3, -1.0)];
    for (axis, at, sign) in edges {
        let coord = |p: (f32, f32)| if axis == 0 { p.0 } else { p.1 };
        let side = |p: (f32, f32)| (coord(p) - at) * sign;
        let input = std::mem::take(&mut out);
        for k in 0..input.len() {
            let (p, q) = (input[k], input[(k + 1) % input.len()]);
            let (sp, sq) = (side(p), side(q));
            if sp >= 0.0 {
                out.push(p);
            }

            if (sp >= 0.0) != (sq >= 0.0) {
                let t = sp / (sp - sq);
                out.push((p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t));
            }
        }

        if out.is_empty() {
            break;
        }
    }

    out
}

/// A polygon's signed area.
fn area(poly: &[(f32, f32)]) -> f32 {
    let n = poly.len();
    (0..n).map(|i| poly[i].0 * poly[(i + 1) % n].1 - poly[(i + 1) % n].0 * poly[i].1).sum::<f32>() * 0.5
}

/// Where a line across the area at `v` is inside it: its spans along u, sorted.
fn spans(uv: &[(f32, f32)], v: f32) -> Vec<(f32, f32)> {
    let n = uv.len();
    let mut cuts = vec![];
    for i in 0..n {
        let (p, q) = (uv[i], uv[(i + 1) % n]);
        // Half-open, so that a vertex on the line counts once.
        if (p.1 <= v) != (q.1 <= v) {
            cuts.push(p.0 + (v - p.1) / (q.1 - p.1) * (q.0 - p.0));
        }
    }

    cuts.sort_by(f32::total_cmp);
    cuts.chunks_exact(2).map(|c| (c[0], c[1])).collect()
}

/// Where two lists of spans both are.
fn intersect(a: &[(f32, f32)], b: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let mut out = vec![];
    for &(a0, a1) in a {
        for &(b0, b1) in b {
            let (lo, hi) = (a0.max(b0), a1.min(b1));
            if hi > lo {
                out.push((lo, hi));
            }
        }
    }

    out
}

/// A small random sequence (xorshift, seeded through splitmix), as the ground coverage's.
struct Random(u64);

impl Random {
    fn new(seed: u64) -> Self {
        let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        Self((z ^ (z >> 31)) | 1)
    }

    fn unit(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 100 m along x, 60 m along z.
    fn rect() -> Vec<(f32, f32)> {
        vec![(0.0, 0.0), (100.0, 0.0), (100.0, 60.0), (0.0, 60.0)]
    }

    /// One farm of one parcel, every strip along the area: the open strip fields.
    fn open_field() -> FieldSettings {
        FieldSettings { farm_size: 0.0, parcel_size: 0.0, patchwork: 0.0, orchard: 0.0, fenced: 0.0, ..FieldSettings::default() }
    }

    #[test]
    fn strips_run_along_the_longest_side_and_stay_inside() {
        let s = open_field();
        let f = lay(&rect(), &s, 1);
        // 60 m across, 1.5 m in from each side: strips of 12 m with 1.5 m verges, 4 of them.
        assert_eq!(f.strips.len(), 4);
        assert!(f.trees.is_empty() && f.lanes.is_empty() && f.fences.is_empty());
        for d in &f.decals {
            assert!((d.yaw.sin().abs() - 1.0).abs() < 1e-4, "along x (either way): {}", d.yaw.to_degrees());
            assert!((d.across - 12.0).abs() < 1e-4 && d.length <= 25.0 + 1e-3);
            // The decal's box inside the area.
            assert!(d.x - d.length * 0.5 >= -1e-3 && d.x + d.length * 0.5 <= 100.0 + 1e-3);
            assert!(d.z - 6.0 >= -1e-3 && d.z + 6.0 <= 60.0 + 1e-3);
        }

        // Each strip is covered end to end: 97 m in 4 decals.
        assert_eq!(f.decals.len(), 16);
        assert_eq!(lay(&rect(), &s, 1), f, "the same seed, the same farmland");
    }

    #[test]
    fn an_orchard_strip_has_rows_of_trees_and_a_concave_area_is_cut_around() {
        // An L: the strips stop at its inner corner.
        let l = vec![(0.0, 0.0), (100.0, 0.0), (100.0, 30.0), (40.0, 30.0), (40.0, 80.0), (0.0, 80.0)];
        let s = FieldSettings { orchard: 1.0, angle: Some(0.0), ..open_field() };
        let f = lay(&l, &s, 3);
        assert!(f.decals.is_empty() && !f.trees.is_empty());
        for t in &f.trees {
            let inside_l = (t.x >= 0.0 && t.x <= 100.0 && t.z >= 0.0 && t.z <= 30.0) || (t.x >= 0.0 && t.x <= 40.0 && t.z <= 80.0);
            assert!(inside_l, "{t:?}");
        }

        // Two rows a strip: the trees of a strip at two distances across it.
        let first = &f.strips[0];
        let v = first.pieces[0][0].1;
        let rows = f
            .trees
            .iter()
            .filter(|t| (t.z - v).abs() < 6.0)
            .map(|t| (t.z * 10.0).round() as i32)
            .collect::<std::collections::BTreeSet<_>>();
        assert!(rows.len() >= 2, "{rows:?}");
    }

    #[test]
    fn farms_are_cut_by_lanes_and_each_fenced_with_a_gate() {
        let big = vec![(0.0, 0.0), (300.0, 0.0), (300.0, 200.0), (0.0, 200.0)];
        let s = FieldSettings { farm_size: 100.0, lane_width: 6.0, fenced: 1.0, gate: 5.0, orchard: 0.0, ..FieldSettings::default() };
        let f = lay(&big, &s, 7);
        // 300 x 200 in farms of at most 100: at least 3 x 2 farms, so at least 5 lanes.
        assert!(f.lanes.len() >= 5, "{}", f.lanes.len());
        assert!(f.fences.len() >= 6, "{}", f.fences.len());
        for fence in &f.fences {
            assert!(!fence.closed, "a gate in each");
            let (p, q) = (fence.points[0], fence.points[fence.points.len() - 1]);
            assert!(((q.0 - p.0).hypot(q.1 - p.1) - 5.0).abs() < 0.1, "the gate's gap: {p:?} {q:?}");
            // Each fence round a farm, not the whole area.
            let span = fence.points.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.0), hi.max(p.0)));
            assert!(span.1 - span.0 <= 100.0 + 1e-3, "{span:?}");
        }

        // No crop on a lane: each crop decal's middle off every lane's line by more than half the lane.
        for lane in &f.lanes {
            for [p, q] in &lane.pieces {
                for d in &f.decals {
                    let (dx, dz) = (q.0 - p.0, q.1 - p.1);
                    let t = (((d.x - p.0) * dx + (d.z - p.1) * dz) / (dx * dx + dz * dz)).clamp(0.0, 1.0);
                    let off = (d.x - p.0 - dx * t).hypot(d.z - p.1 - dz * t);
                    let crop = d.key.starts_with("global_fields");
                    assert!(!crop || off > 3.0 - 1e-3, "a crop on a lane: {d:?}");
                }
            }
        }
    }

    #[test]
    fn belts_are_trees_and_tracks_decals_their_long_side_along() {
        let big = vec![(0.0, 0.0), (300.0, 0.0), (300.0, 100.0), (0.0, 100.0)];
        let belts = FieldSettings { farm_size: 100.0, tracks: 0.0, orchard: 0.0, fenced: 0.0, ..FieldSettings::default() };
        let f = lay(&big, &belts, 2);
        assert!(f.lanes.iter().all(|l| !l.track) && !f.lanes.is_empty());
        assert!(f.trees.iter().any(|t| t.key.starts_with("oak.")) && f.trees.iter().any(|t| t.key.starts_with("shrubs.")));

        // path_1 is 20 x 5 in the kit: its x along the track (the decal turned a quarter), 6 m across it.
        let tracks = FieldSettings { track_decals: vec!["path_1".to_owned()], tracks: 1.0, ..belts };
        let f = lay(&big, &tracks, 2);
        let paths = f.decals.iter().filter(|d| d.key == "path_1").collect::<Vec<_>>();
        assert!(!paths.is_empty() && f.lanes.iter().all(|l| l.track));
        for d in paths {
            assert!((d.length - 6.0).abs() < 1e-3 && d.across > 15.0, "{d:?}");
        }
    }

    #[test]
    fn presets_are_distinct_and_named_by_their_settings() {
        let all = presets();
        assert_eq!(all[0].settings, FieldSettings::default());
        for (i, p) in all.iter().enumerate() {
            assert_eq!(p.settings.preset_name(), Some(p.name));
            assert!(all[..i].iter().all(|q| q.settings != p.settings && q.name != p.name));
            assert!(!lay(&rect(), &p.settings, 1).decals.is_empty() || p.settings.orchard > 0.5, "{}", p.name);
        }
    }

    #[test]
    fn settings_through_their_text() {
        let s = FieldSettings {
            angle: Some(30.0),
            rows: 3,
            fence: vec![],
            props_per_ha: 2.5,
            farm_size: 70.0,
            tracks: 0.25,
            ..FieldSettings::default()
        };
        assert_eq!(FieldSettings::from_text(&s.to_text()), s);
        assert_eq!(FieldSettings::from_text("strip=banana;rows=4"), FieldSettings { rows: 4, ..FieldSettings::default() });
    }
}
