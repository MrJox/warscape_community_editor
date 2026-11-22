//! Rivers against the tile's link targets (Phase 4c of docs/plans/tile_ted_parity.md, docs/re/tile_links.md): a river that crosses into a
//! neighbour ends just past the tile's edge, level with the centre of an edge cell the entry offers as a `river` (or `river_shallow`)
//! target, so that the neighbour's river meets it. Measured on the kit's 187 river ends at an edge: every one on a river target cell,
//! 0.1 .. 12.4 m past the edge (median 5.8), 4.8 m at most off the cell's centre along the edge (median 1.4).

use wce_foundation::math::Float3;

use wce_foundation::math::Double3;

use super::{Tile, TileDocument, item_of};

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

    /// The cell just past the edge it leaves by (where a link's rule names the neighbour).
    pub past: (i32, i32),
}

/// What fixes a river link problem.
#[derive(Clone, Debug, PartialEq)]
pub enum RiverFix {
    /// Add a river link at a cell (a target, and the rule on the cell past the edge), of a set (`river`, `river_shallow`).
    AddLink { cell: (i32, i32), past: (i32, i32), set: &'static str },

    /// Draw a river from a link's place (where its end goes, `RiverLinkPoint::snapped`).
    DrawFrom { at: Float3 },

    /// Move a river's end (its point) to meet the neighbour's river: along the edge to it, at its height and width.
    MatchNeighbour { river: usize, point: usize, at: Float3, width: f32 },
}

/// A river link problem: the river it is about (if any), what is wrong, what fixes it (if anything does in one go).
#[derive(Clone, Debug, PartialEq)]
pub struct RiverProblem {
    pub elem: Option<usize>,
    pub message: String,
    pub fix: Option<RiverFix>,
}

/// A river's end in a neighbour tile of the map context, in the open tile's frame (metres; its height its file's).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NeighbourEnd {
    pub at: Float3,
    pub width: f32,
}

/// How far two rivers' ends meeting at an edge may be apart along it, step in their water and differ in width before the seam check
/// reports them (docs/re/tile_links.md, R4: the kit lives with a 0.8 m step).
pub const SEAM_ALONG_M: f32 = 8.0;
pub const SEAM_STEP_M: f32 = 1.0;
pub const SEAM_WIDTH_M: f32 = 10.0;

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
        for e in t.tscene().find_all(RIVER) {
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

                let past = match edge {
                    0 => (-1, cell.1),
                    1 => (cw, cell.1),
                    2 => (cell.0, -1),
                    _ => (cell.0, ch),
                };
                out.push(RiverEnd { elem: e, point, at, cell, snapped, off_centre, past });
            }
        }

        out
    }

    /// What does not join (the check of Phase 4c): a river target cell without a river end at it (fix: draw a river from there), a river
    /// end at an edge cell that is not a target (fix: add a river link there), an end more than `RIVER_END_TOLERANCE_M` off its cell's
    /// centre along the edge (*Snap river ends* fixes those). Each with the river's element, if any.
    pub fn river_link_problems(&self) -> Vec<RiverProblem> {
        let targets = self.river_targets();
        let ends = self.river_ends();
        let links = self.tile().river_link_points();
        let mut out = vec![];
        for &(x, y) in &targets {
            if !ends.iter().any(|en| en.cell == (x, y)) {
                out.push(RiverProblem {
                    elem: None,
                    message: format!("river target cell ({x}, {y}) has no river end at its edge: a neighbour's river would stop there"),
                    fix: links.iter().find(|l| l.cell == (x, y)).map(|l| RiverFix::DrawFrom { at: l.snapped }),
                });
            }
        }

        for en in &ends {
            let name = self.tile().tscene().name(en.elem);
            if !targets.contains(&en.cell) {
                // Deep unless the river is as shallow as the shallow connection's water.
                let set = if en.at.y as f64 > (wce_tile::river_bed::SHALLOW.water + wce_tile::river_bed::DEEP.water) * 0.5 {
                    wce_tile::river_bed::SHALLOW.name
                } else {
                    wce_tile::river_bed::DEEP.name
                };
                out.push(RiverProblem {
                    elem: Some(en.elem),
                    message: format!(
                        "'{name}' ends at the edge in cell ({}, {}), which the tile offers no river link at",
                        en.cell.0, en.cell.1
                    ),
                    fix: Some(RiverFix::AddLink { cell: en.cell, past: en.past, set }),
                });
            } else if en.off_centre > RIVER_END_TOLERANCE_M {
                out.push(RiverProblem {
                    elem: Some(en.elem),
                    message: format!(
                        "'{name}' ends {:.1} m off the centre of its target cell ({}, {}): a neighbour's river would not meet it",
                        en.off_centre, en.cell.0, en.cell.1
                    ),
                    fix: None,
                });
            }
        }

        out
    }

    /// The rivers' ends at the tile's links against the neighbours' rivers in a map context (`neighbours`: their ends in this tile's
    /// frame): a link no neighbour's river comes to, and an end whose neighbour's is further along the edge than [`SEAM_ALONG_M`], a
    /// water step over [`SEAM_STEP_M`] or a width off by over [`SEAM_WIDTH_M`] (fix: match the neighbour's).
    pub fn river_seam_problems(&self, neighbours: &[NeighbourEnd]) -> Vec<RiverProblem> {
        let t = self.tile();
        let links = t.river_link_points();
        let mut out = vec![];
        for en in self.river_ends() {
            let Some(l) = links.iter().filter(|l| l.cell == en.cell).min_by(|a, b| {
                let d = |q: Float3| (q.x - en.at.x).hypot(q.z - en.at.z);
                d(a.on_edge).total_cmp(&d(b.on_edge))
            }) else {
                continue;
            };

            let name = t.tscene().name(en.elem);
            let along = |p: Float3| (p.x - l.on_edge.x) * -l.inward[1] + (p.z - l.on_edge.z) * l.inward[0];
            let across = |p: Float3| (p.x - l.on_edge.x) * l.inward[0] + (p.z - l.on_edge.z) * l.inward[1];
            // A neighbour's end at this edge: just past it on our side, within the cell's side.
            let theirs = neighbours
                .iter()
                .filter(|n| across(n.at).abs() <= 32.0 && along(n.at).abs() <= 64.0 + SEAM_ALONG_M)
                .min_by(|a, b| (along(a.at) - along(en.at)).abs().total_cmp(&(along(b.at) - along(en.at)).abs()));
            let Some(n) = theirs else {
                out.push(RiverProblem {
                    elem: Some(en.elem),
                    message: format!(
                        "'{name}' leaves by its link at cell ({}, {}), but the neighbour there has no river at that edge",
                        en.cell.0, en.cell.1
                    ),
                    fix: None,
                });
                continue;
            };

            let width = t
                .tscene()
                .polygon_points(en.elem)
                .get(en.point)
                .and_then(|&p| t.tscene().get(p, "width"))
                .and_then(|v| v.parse::<f32>().ok());
            let my_y = en.at.y / t.river_height_factor(en.elem) as f32;
            let (d_along, step) = ((along(n.at) - along(en.at)).abs(), (n.at.y - my_y).abs());
            let d_width = width.map_or(0.0, |w| (w - n.width).abs());
            if d_along > SEAM_ALONG_M || step > SEAM_STEP_M || d_width > SEAM_WIDTH_M {
                // Where its end goes: level with the neighbour's along the edge, as far past it as a snapped end.
                let a = along(n.at);
                let base = Float3::new(l.on_edge.x + -l.inward[1] * a, 0.0, l.on_edge.z + l.inward[0] * a);
                let at = Float3::new(base.x - l.inward[0] * RIVER_END_OVERSHOOT_M, n.at.y, base.z - l.inward[1] * RIVER_END_OVERSHOOT_M);
                out.push(RiverProblem {
                    elem: Some(en.elem),
                    message: format!(
                        "'{name}' meets the neighbour's river at cell ({}, {}) {d_along:.1} m apart along the edge, its water {step:.1} m off and \
                         its width {d_width:.0} m off",
                        en.cell.0, en.cell.1
                    ),
                    fix: Some(RiverFix::MatchNeighbour { river: en.elem, point: en.point, at, width: n.width }),
                });
            }
        }

        out
    }

    /// Adds a river link: a target of `set` at `cell` and a rule that the cell past the edge (`past`) is one too (as the kit's: base
    /// (1, 0), 64 px blend, a blend quad). One undo entry.
    ///
    /// # Errors
    ///
    /// The tile has no database entry.
    pub fn add_river_link(&mut self, cell: (i32, i32), past: (i32, i32), set: &str) -> Result<bool, String> {
        use wce_formats::tile_entry::{TileLink, TileLinkTarget};

        let Some(entry) = self.tile().project.entry.as_ref() else { return Err("the tile has no database entry".to_owned()) };
        let mut targets = entry.link_targets();
        let mut links = entry.links();
        if !targets.iter().any(|t| (t.x, t.y) == cell && t.tile_set == set) {
            targets.push(TileLinkTarget { elem: 0, serialise_version: 1, tile_set: set.to_owned(), x: cell.0, y: cell.1 });
        }

        if !links.iter().any(|l| (l.x, l.y) == past && l.link_set == set) {
            links.push(TileLink {
                elem: 0,
                serialise_version: 1,
                link_set: set.to_owned(),
                x: past.0,
                y: past.1,
                base_x: 1,
                base_y: 0,
                is_entry: false,
                blend_size: 64,
                no_offline_blend: false,
                test: "TLT_EQUALS".to_owned(),
                has_blend_quad: true,
            });
        }

        let g = self.next_gesture();
        self.set_links("Add river link", targets, links, g)
    }

    /// Moves a river's end to `at` (world metres, its height the file's) with a width: the neighbour's, as `RiverFix::MatchNeighbour`
    /// says. Its handle keeps its offset. One undo entry.
    ///
    /// # Errors
    ///
    /// The scene could not be edited.
    pub fn match_river_end(&mut self, river: usize, point: usize, at: Float3, width: f32) -> Result<(), String> {
        use wce_tile::new_instances::point_text;

        let t = self.tile();
        let sc = t.tscene();
        let Some(&p) = sc.polygon_points(river).get(point) else { return Err("no such point".to_owned()) };
        let at = Float3::new(at.x, at.y * t.river_height_factor(river) as f32, at.z);
        let (old, new) = (sc.point_position(p).unwrap_or_default(), t.to_local(river, at));
        let d = new - old;
        let handles = ["controlIn", "controlOut"].map(|h| (h, sc.get(p, h).and_then(|v| wce_tile::ted_scene::parse_vec3(&v, ""))));
        self.end_stroke();
        let mark = self.begin()?;
        let sc = self.tscene_mut();
        sc.set_attr(p, "position", &point_text(new));
        for (h, v) in handles {
            if let Some(v) = v.filter(|v| *v != Double3::ZERO) {
                sc.set_attr(p, h, &point_text(v + d));
            }
        }

        sc.set_attr(p, "width", &wce_tile::ted_scene::format_number(f64::from(width)));
        let items = vec![item_of(river)];
        self.push_splice_touching("Match the neighbour's river".to_owned(), mark, items.clone(), items, vec![river]);
        Ok(())
    }

    /// The problems of the tile's links themselves (`wce_tile::link_checks`), against the kit's tile sets.
    pub fn link_problems(&self) -> Vec<wce_tile::link_checks::LinkProblem> {
        let t = self.tile();
        let Some(entry) = t.project.entry.as_ref() else { return vec![] };
        let sets = t.tile_sets.get_or_init(|| entry.raw_root().map(|r| wce_tile::link_checks::kit_tile_sets(&r)).unwrap_or_default());
        wce_tile::link_checks::link_problems(entry, sets)
    }

    /// Snaps the rivers' ends at the edge onto their river links, whole: each end within half a cell of a link's place on the edge goes
    /// [`RIVER_END_OVERSHOOT_M`] past it through the cell's middle, at the link's water height and width, its handle turned square to the
    /// edge, marked as joined to it (`wce_link`), so that the river's layer shapes the ground there as the export does
    /// (docs/plans/rivers.md, step 1). Ends at cells that are no target are left alone. One undo entry; returns how many ends were snapped.
    ///
    /// # Errors
    ///
    /// The scene could not be edited.
    pub fn snap_river_ends(&mut self) -> Result<usize, String> {
        let rivers = self.tile().tscene().find_all(RIVER);
        self.snap_rivers_to_links(&rivers, LINK_REACH_M, "Snap river ends to link targets")
    }

    /// After a river was drawn or one of its ends dragged: its ends left on a link's snapped place ([`Tile::river_link_points`], where
    /// the live snap puts them) are snapped whole ([`Self::snap_river_ends`]); an end marked as joined that is no longer there loses the
    /// mark. One undo entry, none when nothing changes; returns how many ends were snapped.
    ///
    /// # Errors
    ///
    /// The scene could not be edited.
    pub fn snap_river_to_links(&mut self, river: usize) -> Result<usize, String> {
        self.snap_rivers_to_links(&[river], 0.5, "Snap river to links")
    }

    fn snap_rivers_to_links(&mut self, rivers: &[usize], reach: f32, label: &str) -> Result<usize, String> {
        let links = self.tile().river_link_points();
        // What each end becomes: its river, its point, the link, the end's old place and its neighbour's.
        let mut snaps = vec![];
        let mut unmark = vec![];
        for &e in rivers {
            let t = self.tile();
            let sc = t.tscene();
            let pts = t.polygon_world_points(e);
            let points = sc.polygon_points(e);
            if pts.len() < 2 || points.len() != pts.len() {
                continue;
            }

            for i in [0, pts.len() - 1] {
                let at = pts[i];
                let near = links
                    .iter()
                    .filter(|l| {
                        if reach < 1.0 {
                            (l.snapped.x - at.x).hypot(l.snapped.z - at.z) <= reach
                        } else {
                            // Within half a cell of the link's place on the edge, along it and across it.
                            let (dx, dz) = (at.x - l.on_edge.x, at.z - l.on_edge.z);
                            (dx * l.inward[0] + dz * l.inward[1]).abs() <= reach && (dx * -l.inward[1] + dz * l.inward[0]).abs() <= reach
                        }
                    })
                    .min_by(|a, b| {
                        let d = |l: &RiverLinkPoint| (l.on_edge.x - at.x).hypot(l.on_edge.z - at.z);
                        d(a).total_cmp(&d(b))
                    });
                match near {
                    Some(l) => snaps.push((e, i, l.clone(), pts[if i == 0 { 1 } else { i - 1 }])),
                    None if sc.get(points[i], LINK_ATTR).is_some_and(|v| !v.is_empty()) => unmark.push((e, points[i])),
                    None => {}
                }
            }
        }

        // Nothing to do: every snapped end already as it would be made.
        let wanted = snaps.iter().map(|(e, i, l, prev)| self.snapped_end(*e, *i, l, *prev)).collect::<Vec<_>>();
        let same = snaps.iter().zip(&wanted).all(|((e, i, ..), w)| {
            let sc = self.tile().tscene();
            let point = sc.polygon_points(*e)[*i];
            w.iter().all(|(name, v)| sc.get(point, name).as_deref() == Some(v.as_str()))
                && sc.get(*e, RIVER_ID_ATTR).is_some_and(|v| !v.is_empty())
        });
        if unmark.is_empty() && same {
            return Ok(0);
        }

        self.end_stroke();
        let mark = self.begin()?;
        let mut items = vec![];
        let mut next_id = self.tile().next_river_id();
        for ((e, i, ..), w) in snaps.iter().zip(&wanted) {
            let point = self.tile().tscene().polygon_points(*e)[*i];
            let has_id = self.tile().tscene().get(*e, RIVER_ID_ATTR).is_some_and(|v| !v.is_empty());
            let sc = self.tscene_mut();
            for (name, v) in w {
                sc.set_attr(point, name, v);
            }

            if !has_id {
                sc.set_attr(*e, RIVER_ID_ATTR, &next_id.to_string());
                next_id += 1;
            }

            items.push(item_of(*e));
        }

        for &(e, p) in &unmark {
            self.tscene_mut().set_attr(p, LINK_ATTR, "");
            items.push(item_of(e));
        }

        items.sort_unstable();
        items.dedup();
        let elems = items.iter().filter_map(|&i| super::elem_of(i)).collect();
        self.push_splice_touching(label.to_owned(), mark, items.clone(), items, elems);
        Ok(snaps.len())
    }

    /// The attributes an end of a river (point `i`) gets when snapped to a link: its place, its handle square to the edge (the length
    /// it had, else a third of the way to its neighbour `prev`), its width and the link's mark.
    fn snapped_end(&self, e: usize, i: usize, l: &RiverLinkPoint, prev: Float3) -> Vec<(&'static str, String)> {
        use wce_tile::new_instances::point_text;

        let t = self.tile();
        let sc = t.tscene();
        let point = sc.polygon_points(e)[i];
        // The water at the link's height, in the river's frame (its file's height factor).
        let y = (l.connection.water * t.river_height_factor(e)) as f32;
        let at = Float3::new(l.snapped.x, y, l.snapped.z);
        let first = i == 0;
        let handle = if first { "controlOut" } else { "controlIn" };
        let old = sc.get(point, handle).and_then(|v| wce_tile::ted_scene::parse_vec3(&v, "")).filter(|v| *v != Double3::ZERO);
        let here = sc.point_position(point).unwrap_or_default();
        let unit = t.unit();
        let len = old.map_or(0.0, |h| ((h.x - here.x).hypot(h.z - here.z) / unit) as f32);
        let len = if len > 0.5 { len } else { (prev.x - at.x).hypot(prev.z - at.z) / 3.0 };
        let h = Float3::new(at.x + l.inward[0] * len, y, at.z + l.inward[1] * len);
        vec![
            ("position", point_text(t.to_local(e, at))),
            (handle, point_text(t.to_local(e, h))),
            ("width", wce_tile::ted_scene::format_number(l.connection.width)),
            (LINK_ATTR, format!("{},{}", l.cell.0, l.cell.1)),
        ]
    }
}

/// The type of a river.
const RIVER: &str = "SplineRiverInstance";

/// The mark on a river's end point that it is joined to a link: the link's cell (`x,y`, the entry's numbering), empty when not.
pub const LINK_ATTR: &str = "wce_link";

/// A river's id (on the instance), which its terrain layer's owner names (`river_beds.rs`).
pub const RIVER_ID_ATTR: &str = "wce_river_id";

/// How far an end may be from a link's place on the edge, along it and across it, for *Snap river ends* (half a cell).
const LINK_REACH_M: f32 = 64.0;

/// A river link of the tile: where a river leaves it to a neighbour's (docs/re/tile_links.md, "Rivers at their links").
#[derive(Clone, Debug, PartialEq)]
pub struct RiverLinkPoint {
    /// The target cell (the entry's numbering: x along +x, y from the +z edge).
    pub cell: (i32, i32),
    pub connection: &'static wce_tile::river_bed::Connection,

    /// The middle of the cell's side on the tile's edge, on the ground.
    pub on_edge: Float3,

    /// Where a river's end goes: [`RIVER_END_OVERSHOOT_M`] past it, on the ground.
    pub snapped: Float3,

    /// Into the tile, across the edge (x, z).
    pub inward: [f32; 2],
}

impl Tile {
    /// The tile's river links: for each river target cell (`river`, `river_shallow`...), the sides of it on the tile's edge that a river
    /// leaves by: those with a rule of a river set just past them, else (no such rule for the cell) every side of it on the edge. Its
    /// connection is the rule's set's, else the target's.
    pub fn river_link_points(&self) -> Vec<RiverLinkPoint> {
        let Some(entry) = &self.project.entry else { return vec![] };
        let info = &self.project.info;
        let (w, h) = (info.cells_x.max(1) as i32, info.cells_y.max(1) as i32);
        let rules = entry
            .links()
            .into_iter()
            .filter(|l| l.test != "TLT_NOT_EQUALS" && wce_tile::river_bed::connection_of(&l.link_set).is_some())
            .collect::<Vec<_>>();
        let mut out = vec![];
        for t in entry.link_targets() {
            let Some(target_conn) = wce_tile::river_bed::connection_of(&t.tile_set) else { continue };
            // The sides on the edge: (the cell just past, the inward normal in x, z).
            let sides = [
                ((t.x - 1, t.y), [1.0f32, 0.0]),
                ((t.x + 1, t.y), [-1.0, 0.0]),
                ((t.x, t.y - 1), [0.0, -1.0]),
                ((t.x, t.y + 1), [0.0, 1.0]),
            ];
            let outside = |c: (i32, i32)| c.0 < 0 || c.0 >= w || c.1 < 0 || c.1 >= h;
            let edge_sides = sides.iter().filter(|(c, _)| outside(*c)).collect::<Vec<_>>();
            let ruled = edge_sides.iter().filter(|(c, _)| rules.iter().any(|r| (r.x, r.y) == *c)).copied().collect::<Vec<_>>();
            let cell_ruled = rules.iter().any(|r| edge_sides.iter().any(|(c, _)| (r.x, r.y) == *c));
            for (past, inward) in if cell_ruled { ruled } else { edge_sides } {
                let connection = rules
                    .iter()
                    .find(|r| (r.x, r.y) == *past)
                    .and_then(|r| wce_tile::river_bed::connection_of(&r.link_set))
                    .unwrap_or(target_conn);
                let (x0, z0, x1, z1) = self.cell_rect(t.x, t.y);
                let (cx, cz) = ((x0 + x1) * 0.5, (z0 + z1) * 0.5);
                // The side's middle: half a cell from the centre, against the inward normal.
                let (ex, ez) = (cx - inward[0] * 64.0, cz - inward[1] * 64.0);
                let (sx, sz) = (ex - inward[0] * RIVER_END_OVERSHOOT_M, ez - inward[1] * RIVER_END_OVERSHOOT_M);
                out.push(RiverLinkPoint {
                    cell: (t.x, t.y),
                    connection,
                    on_edge: Float3::new(ex, self.terrain_height(ex, ez), ez),
                    snapped: Float3::new(sx, self.terrain_height(sx, sz), sz),
                    inward: *inward,
                });
            }
        }

        out
    }

    /// An id for a river's terrain layer (`RIVER_ID_ATTR`): one more than the highest.
    pub(super) fn next_river_id(&self) -> u32 {
        let sc = self.tscene();
        sc.find_all(RIVER).into_iter().filter_map(|e| sc.get(e, RIVER_ID_ATTR)?.parse::<u32>().ok()).max().unwrap_or(0) + 1
    }
}

/// A `river_spline<n>.xml`'s points: (x, y, z, width) of every `POINT0` .. `POINT3`, in order (each segment's four; the river's ends are
/// the first and the last).
pub fn file_points(text: &str) -> Vec<[f64; 4]> {
    let attr = |tag: &str, name: &str| -> f64 {
        let key = format!(" {name}=\"");
        tag.find(&key).and_then(|i| tag[i + key.len()..].split('"').next()).and_then(|v| v.parse().ok()).unwrap_or(f64::NAN)
    };
    text.split("<POINT")
        .skip(1)
        .filter_map(|s| s.split("/>").next())
        .map(|tag| [attr(tag, "x"), attr(tag, "y"), attr(tag, "z"), attr(tag, "width")])
        .collect()
}
