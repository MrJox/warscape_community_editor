//! A river's fords edited where they are (docs/plans/rivers.md, step 6): where a place is along a river and across it, each ford's outline
//! on the water (its curved ends, `wce_tile::river_bed::Ford::ends`, and its handles: its start, its middle and its end), and a ford's part
//! dragged along the river (one undo entry a drag).

use wce_foundation::math::Float3;
use wce_tile::river_bed::{FORD_DEPTH, Ford};

use super::TileDocument;
use super::commands::TedAttrCommand;
use super::river_beds::{FORDS_ATTR, river_fords};

/// A ford's part a drag moves: its start (the end nearer the river's first point), its middle (the whole ford), its end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FordPart {
    Start,
    Middle,
    End,
}

/// A ford as drawn: its two curved ends across the water (world points, on the water) and its handles (start, middle, end).
#[derive(Clone, Debug, PartialEq)]
pub struct FordShape {
    pub ends: [Vec<Float3>; 2],
    pub handles: [Float3; 3],
}

/// How short a ford may be (metres).
pub const MIN_FORD_LENGTH: f64 = 5.0;

/// A river's middle line as its ribbon has it: each sample's place, its water, its half width and how far along it is.
struct Line {
    pts: Vec<(f64, f64, f64, f64, f64)>,
}

impl Line {
    /// The place `along` metres along, offset `across` metres to the left of the way the river runs, on its water.
    fn at(&self, along: f64, across: f64) -> Option<Float3> {
        let p = &self.pts;
        let i = p.windows(2).position(|w| along <= w[1].4).unwrap_or(p.len().checked_sub(2)?);
        let (a, b) = (p[i], p[i + 1]);
        let l = (b.4 - a.4).max(1e-9);
        let t = ((along - a.4) / l).clamp(0.0, 1.0);
        let (dx, dz) = ((b.0 - a.0) / l, (b.1 - a.1) / l);
        let (x, z) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
        // The left of the direction (dx, dz) is (-dz, dx): positive across, as `Channel::fill_cells`'s side.
        Some(Float3::new((x - dz * across) as f32, (a.2 + (b.2 - a.2) * t) as f32, (z + dx * across) as f32))
    }

    /// The half width `along` metres along.
    fn half(&self, along: f64) -> f64 {
        let p = &self.pts;
        let i = p.windows(2).position(|w| along <= w[1].4).unwrap_or(p.len().saturating_sub(2));
        let (a, b) = (p[i], p[(i + 1).min(p.len() - 1)]);
        let t = ((along - a.4) / (b.4 - a.4).max(1e-9)).clamp(0.0, 1.0);
        a.3 + (b.3 - a.3) * t
    }
}

impl TileDocument {
    fn river_line(&self, river: usize) -> Option<Line> {
        let (_, samples) = self.tile().river_ribbons_by_elem().remove(&river)?;
        let mut along = 0.0;
        let mut pts = vec![];
        for (i, s) in samples.iter().enumerate() {
            if i > 0 {
                along += (s.x - samples[i - 1].x).hypot(s.z - samples[i - 1].z);
            }

            pts.push((s.x, s.z, s.y, s.width * 0.5, along));
        }

        (pts.len() >= 2).then_some(Line { pts })
    }

    /// Where a place (x, z) is along a river (metres) and how far off its middle line (metres, to the left of its way positive), by the
    /// nearest place of its ribbon; none when it is not drawn.
    pub fn river_along_at(&self, river: usize, x: f32, z: f32) -> Option<(f64, f64)> {
        let line = self.river_line(river)?;
        let (x, z) = (f64::from(x), f64::from(z));
        let mut best: Option<(f64, f64, f64)> = None;
        for w in line.pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let (dx, dz) = (b.0 - a.0, b.1 - a.1);
            let l2 = dx * dx + dz * dz;
            let t = if l2 > 1e-12 { (((x - a.0) * dx + (z - a.1) * dz) / l2).clamp(0.0, 1.0) } else { 0.0 };
            let d = (x - a.0 - dx * t).hypot(z - a.1 - dz * t);
            if best.is_none_or(|b| d < b.0) {
                let side = if dx * (z - a.1) - dz * (x - a.0) < 0.0 { -1.0 } else { 1.0 };
                best = Some((d, a.4 + (b.4 - a.4) * t, side * d));
            }
        }

        best.map(|b| (b.1, b.2))
    }

    /// A river's fords as drawn: their curved ends on the water and their handles; none when it is not drawn.
    pub fn river_ford_shapes(&self, river: usize) -> Vec<FordShape> {
        let Some(line) = self.river_line(river) else { return vec![] };
        let fords = river_fords(self.tile().tscene().get(river, FORDS_ATTR).as_deref().unwrap_or_default());
        let mut out = vec![];
        for (at, length) in fords {
            let f = Ford { at, length, depth: FORD_DEPTH };
            let h = line.half(at);
            let curve = |end: usize| {
                (0..=16)
                    .filter_map(|k| {
                        let u = -1.0 + 2.0 * f64::from(k) / 16.0;
                        let (s, e) = f.ends(u);
                        line.at(if end == 0 { s } else { e }, u * h)
                    })
                    .collect::<Vec<_>>()
            };
            let (s, e) = f.ends(0.0);
            let (Some(a), Some(m), Some(b)) = (line.at(s, 0.0), line.at(at, 0.0), line.at(e, 0.0)) else { continue };
            out.push(FordShape { ends: [curve(0), curve(1)], handles: [a, m, b] });
        }

        out
    }

    /// Sets a river's fords (each one's middle along it and its length, metres): one undo entry per `gesture` (a drag's steps merge).
    pub fn set_river_fords(&mut self, river: usize, fords: &[(f64, f64)], name: &str, gesture: u64) {
        use wce_tile::ted_scene::format_number;
        // To a tenth of a metre: a drag's numbers carry the cursor's every digit.
        let tenth = |v: f64| format_number((v * 10.0).round() / 10.0);
        let text = fords.iter().map(|(a, l)| format!("{}:{}", tenth(*a), tenth(*l))).collect::<Vec<_>>().join(";");
        let old = self.tile().tscene().get(river, FORDS_ATTR);
        match old {
            Some(old) if old != text => {
                self.end_stroke();
                let steps = vec![(FORDS_ATTR.to_owned(), old, text)];
                self.history.push(Box::new(TedAttrCommand { name: name.to_owned(), elem: river, steps, gesture, rebuild: false }));
            }

            Some(_) => {}
            None => {
                self.set_attribute(river, FORDS_ATTR, &text);
            }
        }
    }

    /// How many fords a river has.
    pub fn river_ford_count(&self, river: usize) -> usize {
        river_fords(self.tile().tscene().get(river, FORDS_ATTR).as_deref().unwrap_or_default()).len()
    }

    /// A river's half width (metres) `along` metres along it; none when it is not drawn.
    pub fn river_half_width_at(&self, river: usize, along: f64) -> Option<f64> {
        Some(self.river_line(river)?.half(along))
    }

    /// Sets a ford to run between two places along the river (either order; at least [`MIN_FORD_LENGTH`]): a ford being drawn. One
    /// undo entry per `gesture`.
    pub fn place_ford(&mut self, river: usize, index: usize, a: f64, b: f64, gesture: u64) {
        let mut fords = river_fords(self.tile().tscene().get(river, FORDS_ATTR).as_deref().unwrap_or_default());
        if index >= fords.len() {
            return;
        }

        let (s, e) = (a.min(b), a.max(b));
        let (s, e) = if e - s < MIN_FORD_LENGTH { (s, s + MIN_FORD_LENGTH) } else { (s, e) };
        fords[index] = ((s + e) * 0.5, e - s);
        self.set_river_fords(river, &fords, "Draw ford", gesture);
    }

    /// Moves a ford's part to a place along the river (metres): its start or its end (the other stays), or the whole ford by its middle.
    /// One undo entry per `gesture`.
    pub fn move_ford_part(&mut self, river: usize, index: usize, part: FordPart, along: f64, gesture: u64) {
        let length_of_river = self.river_line(river).and_then(|l| l.pts.last().map(|p| p.4)).unwrap_or(f64::INFINITY);
        let mut fords = river_fords(self.tile().tscene().get(river, FORDS_ATTR).as_deref().unwrap_or_default());
        let Some(&(at, length)) = fords.get(index) else { return };
        let along = along.clamp(0.0, length_of_river);
        let (s, e) = (at - length * 0.5, at + length * 0.5);
        let (s, e) = match part {
            FordPart::Middle => (along - length * 0.5, along + length * 0.5),
            FordPart::Start => (along.min(e - MIN_FORD_LENGTH), e),
            FordPart::End => (s, along.max(s + MIN_FORD_LENGTH)),
        };
        fords[index] = ((s + e) * 0.5, e - s);
        self.set_river_fords(river, &fords, "Move ford", gesture);
    }
}
