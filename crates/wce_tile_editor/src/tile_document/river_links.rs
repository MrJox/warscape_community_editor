//! Rivers against the tile's link targets (Phase 4c of docs/plans/tile_ted_parity.md, docs/re/tile_links.md): a river that crosses into a
//! neighbour ends just past the tile's edge, level with the centre of an edge cell the entry offers as a `river` (or `river_shallow`)
//! target, so that the neighbour's river meets it. Measured on the kit's 187 river ends at an edge: every one on a river target cell,
//! 0.1 .. 12.4 m past the edge (median 5.8), 4.8 m at most off the cell's centre along the edge (median 1.4).

use wce_foundation::math::Float3;

use super::{TileDocument, item_of};

/// How far past the edge a snapped end goes when it is not past it already (the kit's median, rounded).
pub const RIVER_END_OVERSHOOT_M: f32 = 6.0;

/// How far from the cell's centre along the edge an end may be before the check reports it (the kit's ends: 4.8 m at most).
pub const RIVER_END_TOLERANCE_M: f32 = 8.0;

/// An end within this of an edge is an end at that edge (half a cell).
const EDGE_BAND_M: f32 = 64.0;

/// A river end at the tile's edge: its spline, which end (the point index), where it is (world metres), the edge cell it is at (the
/// entry's cell numbering: x along +x, y from the +z edge) and that cell's centre on the edge's line with the across-edge axis.
#[derive(Clone, Debug, PartialEq)]
pub struct RiverEnd {
    pub elem: usize,
    pub point: usize,
    pub at: Float3,
    pub cell: (i32, i32),

    /// Where the end belongs: the cell's centre along the edge, `RIVER_END_OVERSHOOT_M` past the edge (or as far past as it is).
    pub snapped: Float3,

    /// How far it is from the cell's centre along the edge.
    pub off_centre: f32,
}

impl TileDocument {
    fn river_targets(&self) -> Vec<(i32, i32)> {
        let Some(en) = self.tile().project.entry.as_ref() else { return vec![] };
        en.link_targets().into_iter().filter(|t| t.tile_set.contains("river")).map(|t| (t.x, t.y)).collect()
    }

    /// The tile's river ends that lie at its edge (within half a cell of it).
    pub fn river_ends(&self) -> Vec<RiverEnd> {
        let t = self.tile();
        let p = &t.project;
        if !p.info.found {
            return vec![];
        }

        let (cw, ch) = (p.info.cells_x as i32, p.info.cells_y as i32);
        let (w, h) = (t.extent_x() as f32, t.extent_z() as f32);
        let mut out = vec![];
        for &e in t.instances.iter().filter(|&&e| t.tscene().type_of(e) == "SplineRiverInstance") {
            let pts = t.polygon_world_points(e);
            if pts.len() < 2 {
                continue;
            }

            for point in [0, pts.len() - 1] {
                let at = pts[point];
                // The nearest edge: west (x 0), east (x w), the +z edge (cell row 0) or the z 0 edge (the last row).
                let edges = [(at.x.abs(), 0), ((at.x - w).abs(), 1), ((at.z - h).abs(), 2), (at.z.abs(), 3)];
                let &(dist, edge) = edges.iter().min_by(|a, b| a.0.total_cmp(&b.0)).expect("four edges");
                if dist > EDGE_BAND_M {
                    continue;
                }

                let col = ((at.x / 128.0).floor() as i32).clamp(0, cw - 1);
                let row = (ch - 1 - (at.z / 128.0).floor() as i32).clamp(0, ch - 1);
                let centre = |i: i32| i as f32 * 128.0 + 64.0;

                // Past the edge: as far as it is, or the kit's usual overshoot when it is not past it.
                let keep = |past: f32| if past >= 0.5 { past } else { RIVER_END_OVERSHOOT_M };
                let (cell, snapped, off_centre) = match edge {
                    0 | 1 => {
                        let zc = centre(ch - 1 - row);
                        let x = if edge == 0 { -keep(-at.x) } else { w + keep(at.x - w) };
                        ((if edge == 0 { 0 } else { cw - 1 }, row), Float3::new(x, at.y, zc), (at.z - zc).abs())
                    }

                    _ => {
                        let xc = centre(col);
                        let z = if edge == 2 { h + keep(at.z - h) } else { -keep(-at.z) };
                        ((col, if edge == 2 { 0 } else { ch - 1 }), Float3::new(xc, at.y, z), (at.x - xc).abs())
                    }
                };

                out.push(RiverEnd { elem: e, point, at, cell, snapped, off_centre });
            }
        }

        out
    }

    /// What does not join (the check of Phase 4c): a river target cell without a river end at it, a river end at an edge cell that is not
    /// a target, an end more than `RIVER_END_TOLERANCE_M` off its cell's centre along the edge. Each with the river's element, if any.
    pub fn river_link_problems(&self) -> Vec<(Option<usize>, String)> {
        let targets = self.river_targets();
        let ends = self.river_ends();
        let mut out = vec![];
        for &(x, y) in &targets {
            if !ends.iter().any(|en| en.cell == (x, y)) {
                out.push((None, format!("river target cell ({x}, {y}) has no river end at its edge: a neighbour's river would stop there")));
            }
        }

        for en in &ends {
            let name = self.tile().tscene().name(en.elem);
            if !targets.contains(&en.cell) {
                out.push((Some(en.elem), format!("'{name}' ends at the edge in cell ({}, {}), which the tile offers no river link at", en.cell.0, en.cell.1)));
            } else if en.off_centre > RIVER_END_TOLERANCE_M {
                out.push((Some(en.elem), format!("'{name}' ends {:.1} m off the centre of its target cell ({}, {}): a neighbour's river would not meet it", en.off_centre, en.cell.0, en.cell.1)));
            }
        }

        out
    }

    /// The problems of the tile's links themselves (`wce_tile::link_checks`), against the kit's tile sets.
    pub fn link_problems(&self) -> Vec<wce_tile::link_checks::LinkProblem> {
        let t = self.tile();
        let Some(entry) = t.project.entry.as_ref() else { return vec![] };
        let sets = t.tile_sets.get_or_init(|| entry.raw_root().map(|r| wce_tile::link_checks::kit_tile_sets(&r)).unwrap_or_default());
        wce_tile::link_checks::link_problems(entry, sets)
    }

    /// Snaps the river ends at the edge onto their target cells (`RiverEnd::snapped`; the handles move with the end, the height stays); ends
    /// at cells that are no target are left alone. One undo entry; returns how many ends moved.
    ///
    /// # Errors
    ///
    /// The scene could not be edited.
    pub fn snap_river_ends(&mut self) -> Result<usize, String> {
        use wce_tile::new_instances::point_text;

        let targets = self.river_targets();
        let moves = self.river_ends().into_iter().filter(|en| targets.contains(&en.cell) && { let d = en.snapped - en.at; d.x.abs() + d.z.abs() > 1e-3 }).collect::<Vec<_>>();
        if moves.is_empty() {
            return Ok(0);
        }

        self.end_stroke();
        let mark = self.begin()?;
        let mut items = vec![];
        for en in &moves {
            let t = self.tile();
            let sc = t.tscene();
            let point = sc.polygon_points(en.elem)[en.point];
            let (old, new) = (t.to_local(en.elem, en.at), t.to_local(en.elem, en.snapped));
            let d = new - old;
            let handles = ["controlIn", "controlOut"].map(|h| (h, sc.doc().get(point, h).and_then(|v| wce_tile::ted_scene::parse_vec3(&v, ""))));
            let sc = self.tscene_mut();
            sc.set_attr(point, "position", &point_text(new));

            // The end's handle that a segment uses goes with it; the unused one stays `0,0,0`.
            for (h, v) in handles {
                if let Some(v) = v.filter(|v| *v != wce_foundation::math::Double3::ZERO) {
                    sc.set_attr(point, h, &point_text(v + d));
                }
            }

            items.push(item_of(en.elem));
        }

        items.dedup();
        self.push_splice("Snap river ends to link targets".to_owned(), mark, items.clone(), items);
        Ok(moves.len())
    }
}
