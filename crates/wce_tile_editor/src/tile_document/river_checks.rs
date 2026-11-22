//! The rivers' water against the ground (docs/plans/rivers.md, step 7): where a river's water would float, vanish or kink as the game
//! draws it. The game draws a river's ribbon at its file's heights and width over whatever ground is there (docs/re/water.md), so:
//! - **spills**: the ground at the ribbon's edge lower than the water: the ribbon's edge hangs in the air over it;
//! - **buried**: the ground over the water's middle: the river vanishes under it;
//! - **steps**: the water dropping more steeply than [`STEP_GRADE`] between two samples: a kink in the ribbon.
//!
//! Each is reported once a stretch, with its worst place. Outside the tile (a river's ends just past the edge) nothing is checked.

use super::TileDocument;
use super::river_links::{RiverFix, RiverProblem};

/// How far the ground may be over the water at its middle before it is reported (metres).
pub const WATER_TOLERANCE_M: f64 = 0.25;

/// How far the ground may be under the water at the ribbon's edge before it is reported (metres): the kit's shallow rivers stand up to
/// 0.3 m over their banks at a junction.
pub const SPILL_TOLERANCE_M: f64 = 0.5;

/// The steepest the water may drop between two samples (rise over run), and the least drop that counts (metres).
pub const STEP_GRADE: f64 = 0.15;
const STEP_LEAST_M: f64 = 0.5;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Spill,
    Buried,
    Step,
}

impl TileDocument {
    /// The rivers' water problems (spills, buried stretches, steps), one a stretch, with a fix where one does (a buried river that cuts
    /// no bed: cut it).
    pub fn river_water_problems(&self) -> Vec<RiverProblem> {
        let t = self.tile();
        let (ex, ez) = (t.extent_x(), t.extent_z());
        let sc = t.tscene();
        let mut out = vec![];
        let mut ribbons = t.river_ribbons_by_elem().into_iter().collect::<Vec<_>>();
        ribbons.sort_by_key(|(e, _)| *e);
        // Under another river's water (a junction): within half its width of its line, where its water is over the ground.
        let all = ribbons
            .iter()
            .map(|(e, (_, s))| (*e, s.iter().map(|s| (s.x, s.z, s.y, s.width * 0.5)).collect::<Vec<_>>()))
            .collect::<Vec<_>>();
        let under_other = |me: usize, x: f64, z: f64, g: f64| {
            all.iter().filter(|(e, _)| *e != me).any(|(_, s)| {
                s.windows(2).any(|w| {
                    let (a, b) = (w[0], w[1]);
                    let (dx, dz) = (b.0 - a.0, b.1 - a.1);
                    let l2 = dx * dx + dz * dz;
                    let t = if l2 > 1e-12 { (((x - a.0) * dx + (z - a.1) * dz) / l2).clamp(0.0, 1.0) } else { 0.0 };
                    let r = (x - a.0 - dx * t).hypot(z - a.1 - dz * t);
                    r <= a.3 + (b.3 - a.3) * t && a.2 + (b.2 - a.2) * t >= g
                })
            })
        };
        for (e, (_, samples)) in ribbons.iter().map(|(e, r)| (*e, r)) {
            let name = sc.name(e);
            let cuts = sc.get(e, super::river_beds::BED_ATTR).as_deref() == Some("1");
            // Per sample: what is wrong there and by how much, and where.
            let mut flags: Vec<Option<(Kind, f64)>> = vec![None; samples.len()];
            let inside = |x: f64, z: f64| x >= 0.0 && z >= 0.0 && x <= ex && z <= ez;
            let ground = |x: f64, z: f64| f64::from(t.terrain_height(x as f32, z as f32));
            for (i, s) in samples.iter().enumerate() {
                if !inside(s.x, s.z) {
                    continue;
                }

                let (a, b) = (&samples[i.saturating_sub(1)], &samples[(i + 1).min(samples.len() - 1)]);
                let (dx, dz) = (b.x - a.x, b.z - a.z);
                let len = dx.hypot(dz);
                if len < 1e-6 {
                    continue;
                }

                let (nx, nz) = (-dz / len, dx / len);
                let half = s.width * 0.5;
                let edges = [(s.x + nx * half, s.z + nz * half), (s.x - nx * half, s.z - nz * half)];
                let low = edges
                    .iter()
                    .filter(|p| inside(p.0, p.1))
                    .map(|p| (p, ground(p.0, p.1)))
                    .filter(|(p, g)| !under_other(e, p.0, p.1, *g))
                    .map(|(_, g)| g)
                    .fold(f64::INFINITY, f64::min);
                let middle = ground(s.x, s.z);
                if middle - s.y > WATER_TOLERANCE_M {
                    flags[i] = Some((Kind::Buried, middle - s.y));
                } else if s.y - low > SPILL_TOLERANCE_M {
                    flags[i] = Some((Kind::Spill, s.y - low));
                } else if i + 1 < samples.len() {
                    let n = &samples[i + 1];
                    let (drop, run) = ((n.y - s.y).abs(), (n.x - s.x).hypot(n.z - s.z));
                    if drop > STEP_LEAST_M && drop > run * STEP_GRADE {
                        flags[i] = Some((Kind::Step, drop));
                    }
                }
            }

            // Stretches of one kind: the worst sample and the length.
            let mut i = 0;
            while i < flags.len() {
                let Some((kind, _)) = flags[i] else {
                    i += 1;
                    continue;
                };

                let start = i;
                let (mut worst, mut at) = (0.0f64, i);
                while i < flags.len() && flags[i].is_some_and(|f| f.0 == kind) {
                    let v = flags[i].expect("flagged").1;
                    if v > worst {
                        (worst, at) = (v, i);
                    }

                    i += 1;
                }

                let length = samples[start..i].windows(2).map(|w| (w[1].x - w[0].x).hypot(w[1].z - w[0].z)).sum::<f64>();
                let s = &samples[at];
                let place = format!("({:.0}, {:.0})", s.x, s.z);
                let (message, fix) = match kind {
                    Kind::Spill => (
                        format!(
                            "'{name}': its water stands up to {worst:.1} m over the ground at its edge for {length:.0} m around {place}: the \
                             water floats there (lower it, or raise the banks)"
                        ),
                        None,
                    ),
                    Kind::Buried => (
                        format!(
                            "'{name}': the ground stands up to {worst:.1} m over its water for {length:.0} m around {place}: the river vanishes \
                             there"
                        ),
                        (!cuts).then_some(RiverFix::CutBed { river: e }),
                    ),
                    Kind::Step => {
                        (format!("'{name}': its water drops {worst:.1} m in one step around {place}: the ribbon kinks there"), None)
                    }
                };
                out.push(RiverProblem { elem: Some(e), message, fix });
            }
        }

        out
    }
}
