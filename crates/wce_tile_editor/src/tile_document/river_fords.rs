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
        self.tile().river_line(river)
    }
}

/// The layer an export puts the rivers' AI hints in.
pub const HINTS_LAYER: &str = "WCE rivers";

/// How far apart a river edge hint's points are (metres), and how far a ford's crossing hint reaches past its ends along the river and
/// past the water's edges across it (metres: vanilla's ford tiles have theirs about 65 to 80 along the river, 100 to 115 across).
const EDGE_STEP: f64 = 20.0;
const CROSSING_ALONG: f64 = 10.0;
const CROSSING_ACROSS: f64 = 30.0;

impl super::Tile {
    fn river_line(&self, river: usize) -> Option<Line> {
        let (_, samples) = self.river_ribbons_by_elem().remove(&river)?;
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

    /// The AI hints of the rivers that shape the terrain (their terrain patch on; the kit's have their own), as vanilla's river tiles have
    /// them: an `AIH_RIVER_EDGE` line along each bank at the water's edge, and an `AIH_BRIDGE_CROSSING` polygon over each ford (from bank
    /// to bank and past them, along it and past its ends). Their texts, in the file's units.
    pub(super) fn river_hints(&self) -> Vec<Vec<u8>> {
        use wce_tile::new_instances::{NewInstance, NewPoint, element_text};
        let sc = self.tscene();
        let unit = self.unit();
        let mut out = vec![];
        for e in sc.find_all("SplineRiverInstance") {
            let on = super::river_beds::river_patch_on(
                sc.get(e, super::river_beds::PATCH_ATTR).as_deref(),
                sc.get(e, super::river_beds::BED_ATTR).as_deref(),
            );
            if !on || !self.shown(e) || !self.of_tile_variation(e) {
                continue;
            }

            let Some(line) = self.river_line(e) else { continue };
            let length = line.pts.last().map_or(0.0, |p| p.4);
            let file = |p: Float3| [f64::from(p.x) * unit, f64::from(p.z) * unit];

            // Its banks: an open line each.
            for side in [-1.0, 1.0] {
                let n = (length / EDGE_STEP).ceil().max(1.0) as usize;
                let pts = (0..=n)
                    .filter_map(|k| {
                        let along = length * k as f64 / n as f64;
                        line.at(along, side * line.half(along)).map(file)
                    })
                    .collect::<Vec<_>>();
                if pts.len() < 2 {
                    continue;
                }

                let c = pts.iter().fold([0.0, 0.0], |a, p| [a[0] + p[0] / pts.len() as f64, a[1] + p[1] / pts.len() as f64]);
                let mut h = NewInstance::new("AI_HINT_POLYGON_INSTANCE")
                    .position(wce_foundation::math::Double3::new(c[0], 0.0, c[1]))
                    .attr("name", "River edge")
                    .attr("hint_type", "AIH_RIVER_EDGE")
                    .attr("polygon_closed", "0")
                    .attr("surface_mode", "MSM_LANDSCAPE");
                h.points = pts.iter().map(|p| NewPoint::at(wce_foundation::math::Double3::new(p[0] - c[0], 0.0, p[1] - c[1]))).collect();
                if let Ok(text) = element_text(&h) {
                    out.push(text);
                }
            }

            // Its fords: a crossing each, its sides following the river every 4 m.
            for (at, flen) in river_fords(sc.get(e, FORDS_ATTR).as_deref().unwrap_or_default()) {
                let f = Ford { at, length: flen, depth: FORD_DEPTH };
                let (s0, e0) = (f.ends(-1.0).0.min(f.ends(1.0).0), f.ends(-1.0).1.max(f.ends(1.0).1));
                let (from, to) = ((s0 - CROSSING_ALONG).max(0.0), (e0 + CROSSING_ALONG).min(length));
                let n = ((to - from) / 4.0).ceil().max(1.0) as usize;
                let side = |sign: f64| {
                    (0..=n)
                        .filter_map(|k| {
                            let along = from + (to - from) * k as f64 / n as f64;
                            line.at(along, sign * (line.half(along) + CROSSING_ACROSS)).map(file)
                        })
                        .collect::<Vec<_>>()
                };
                let mut outline = side(1.0);
                outline.extend(side(-1.0).into_iter().rev());
                if let Ok(text) = wce_tile::road_layout::crossing_hint(&outline) {
                    out.push(text);
                }
            }
        }

        out
    }

    /// The layer of the rivers' AI hints an export adds ([`Self::river_hints`]).
    pub(super) fn with_river_hints(&self, text: Vec<u8>) -> Result<Vec<u8>, String> {
        let hints = self.river_hints();
        if hints.is_empty() {
            return Ok(text);
        }

        let mut scene = wce_tile::ted_scene::TedScene::parse_any(text)?;
        let layer = scene.add_layer(scene.scene_graph(), HINTS_LAYER)?;
        scene.insert_elements_text(layer, &hints)?;

        Ok(scene.save())
    }
}

impl TileDocument {
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
