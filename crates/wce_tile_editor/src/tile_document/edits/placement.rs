//! Placement (landscape / freeform), and align and distribute.

use super::*;

impl TileDocument {
    // ---- placement

    /// Sets the mode of the instances among `elems` (layers: everything below); Landscape snaps y. One undo entry; returns how many instances
    /// changed.
    pub fn set_placement(&mut self, elems: &[usize], mode: Placement) -> usize {
        if mode == Placement::None {
            return 0;
        }

        let t = self.tile();
        let text = if mode == Placement::Landscape { "MSM_LANDSCAPE" } else { "MSM_FREEFORM" };
        let mut items = vec![];
        for e in t.instances_below(elems) {
            if t.tscene().doc().get(e, "surface_mode").is_none() {
                continue;
            }

            let cur = t.placement(e);
            let code = |p: Placement| match p {
                Placement::Landscape => 1,
                Placement::Freeform => 2,
                Placement::None => 0,
            };

            let mut it = PlacementItem {
                elem: e,
                mode_old: None,
                mode_new: None,
                pos_old: None,
                pos_new: None,
                idx_old: code(cur),
                idx_new: code(mode),
                pos_old_text: None,
            };
            if cur != mode {
                it.mode_old = t.tscene().doc().get(e, "surface_mode");
                it.mode_new = Some(text.to_owned());
            }

            let file_y = (world_transform(t.tscene(), e).position.y / t.unit()) as f32;
            if mode == Placement::Landscape {
                // Freeform -> Landscape snaps (TEd writes the terrain height); a landscape item is shown on the terrain even when its file is not.
                let p = t.world_position(e);
                let y = t.landscape_y(e, p.x, p.z);
                if cur != mode || (y - file_y).abs() > 1e-5 {
                    it.pos_old = t.tscene().position(e);
                    it.pos_old_text = t.tscene().doc().get(e, "position");
                    it.pos_new = Some(t.local_for_world_y(e, f64::from(y)));
                }
            } else if cur == Placement::Landscape {
                // Landscape -> Freeform keeps the height it is shown at (the file's y may be a stale one: see snap_landscape_world).
                let y = t.world_position(e).y;
                if (y - file_y).abs() > 1e-5 {
                    it.pos_old = t.tscene().position(e);
                    it.pos_old_text = t.tscene().doc().get(e, "position");
                    it.pos_new = Some(t.local_for_world_y(e, f64::from(y)));
                }
            }

            if it.mode_new.is_some() || it.pos_new.is_some() {
                items.push(it);
            }
        }

        if items.is_empty() {
            return 0;
        }

        let n = items.len();
        let name = format!(
            "{}{}",
            if mode == Placement::Landscape { "Landscape placement" } else { "Freeform placement" },
            if n == 1 { String::new() } else { format!(" ({n} items)") }
        );
        self.history.push(Box::new(TedPlacementCommand { name, items }));
        n
    }

    /// y = the terrain height; returns how many moved.
    pub fn snap_to_terrain(&mut self, elems: &[usize], include_freeform: bool) -> usize {
        let t = self.tile();
        let mut items = vec![];
        for e in t.instances_below(elems) {
            let cur = t.placement(e);
            if cur != Placement::Landscape && !(include_freeform && cur != Placement::None) {
                continue;
            }

            let p = t.world_position(e);
            let y = t.landscape_y(e, p.x, p.z);

            // The file's own y (see snap_landscape_world): this writes the shown height into it.
            if (y - (world_transform(t.tscene(), e).position.y / t.unit()) as f32).abs() < 1e-5 {
                continue;
            }

            let code = if cur == Placement::Landscape { 1 } else { 2 };
            items.push(PlacementItem {
                elem: e,
                mode_old: None,
                mode_new: None,
                pos_old: t.tscene().position(e),
                pos_new: Some(t.local_for_world_y(e, f64::from(y))),
                idx_old: code,
                idx_new: code,
                pos_old_text: t.tscene().doc().get(e, "position"),
            });
        }

        if items.is_empty() {
            return 0;
        }

        let n = items.len();
        let name = format!("Snap to terrain{}", if n == 1 { String::new() } else { format!(" ({n} items)") });
        self.history.push(Box::new(TedPlacementCommand { name, items }));
        n
    }

    // ---- align and distribute

    /// Moves instances to world points (metres) in one undo entry named `name`: a landscape instance takes the terrain's height there.
    /// Returns how many moved.
    pub fn move_instances(&mut self, name: &str, moves: &[(usize, Float3)]) -> usize {
        let t = self.tile();
        let mut items = vec![];
        for &(e, p) in moves {
            if !t.is_instance(e) {
                continue;
            }

            let cur = t.placement(e);
            let y = if cur == Placement::Landscape { t.landscape_y(e, p.x, p.z) } else { p.y };
            let pw = parent_transform(t.tscene(), e);
            let lp = pw.rotation.transpose() * (scene_point(Float3::new(p.x, y, p.z), t.unit()) - pw.position);
            if t.tscene().position(e).is_some_and(|old| close(old, lp)) {
                continue;
            }

            let code = match cur {
                Placement::Landscape => 1,
                Placement::Freeform => 2,
                Placement::None => 0,
            };
            items.push(PlacementItem {
                elem: e,
                mode_old: None,
                mode_new: None,
                pos_old: t.tscene().position(e),
                pos_new: Some(lp),
                idx_old: code,
                idx_new: code,
                pos_old_text: t.tscene().doc().get(e, "position"),
            });
        }

        if items.is_empty() {
            return 0;
        }

        let n = items.len();
        let name = if n == 1 { name.to_owned() } else { format!("{name} ({n} items)") };
        self.history.push(Box::new(TedPlacementCommand { name, items }));
        n
    }

    /// Lines the instances up on an axis (0 x, 1 height, 2 z) at `to`: their lowest, middle or highest value there, or the first one's. The
    /// height lines up the freeform ones only (a landscape instance stands on the terrain). One undo entry; how many moved.
    pub fn align(&mut self, elems: &[usize], axis: usize, to: AlignTo) -> usize {
        let t = self.tile();
        let items = elems
            .iter()
            .copied()
            .filter(|&e| t.is_instance(e) && !(axis == 1 && t.placement(e) == Placement::Landscape))
            .collect::<Vec<_>>();
        if items.len() < 2 {
            return 0;
        }

        let at = |e: usize| t.world_position(e).to_arr()[axis];
        let values = items.iter().map(|&e| at(e)).collect::<Vec<_>>();
        let (lo, hi) = values.iter().fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
        let v = match to {
            AlignTo::Min => lo,
            AlignTo::Centre => (lo + hi) * 0.5,
            AlignTo::Max => hi,
            AlignTo::First => values[0],
        };

        let moves = items
            .iter()
            .map(|&e| {
                let mut p = t.world_position(e).to_arr();
                p[axis] = v;
                (e, Float3::new(p[0], p[1], p[2]))
            })
            .collect::<Vec<_>>();
        self.move_instances(&format!("Align {}", ["x", "height", "z"][axis]), &moves)
    }

    /// Spaces the instances evenly between the two furthest apart on an axis (0 x, 2 z), in their order along it; with `axis` None, along
    /// the line from the first to the last of `elems` (the others in their order along it, put on the line). One undo entry.
    pub fn distribute(&mut self, elems: &[usize], axis: Option<usize>) -> usize {
        let t = self.tile();
        let items = elems.iter().copied().filter(|&e| t.is_instance(e)).collect::<Vec<_>>();
        if items.len() < 3 {
            return 0;
        }

        let pos = |e: usize| t.world_position(e);
        let moves = match axis {
            Some(axis) => {
                let mut by = items.iter().map(|&e| (pos(e).to_arr()[axis], e)).collect::<Vec<_>>();
                by.sort_by(|a, b| a.0.total_cmp(&b.0));
                let (lo, hi) = (by[0].0, by[by.len() - 1].0);
                let n = (by.len() - 1) as f32;
                by.iter()
                    .enumerate()
                    .map(|(i, &(_, e))| {
                        let mut p = pos(e).to_arr();
                        p[axis] = lo + (hi - lo) * i as f32 / n;
                        (e, Float3::new(p[0], p[1], p[2]))
                    })
                    .collect::<Vec<_>>()
            }

            None => {
                let (a, b) = (pos(items[0]), pos(items[items.len() - 1]));
                let d = b - a;
                let len2 = (d.x * d.x + d.z * d.z).max(1e-9);
                let mut by = items.iter().map(|&e| (((pos(e).x - a.x) * d.x + (pos(e).z - a.z) * d.z) / len2, e)).collect::<Vec<_>>();
                by.sort_by(|x, y| x.0.total_cmp(&y.0));
                let n = (by.len() - 1) as f32;
                by.iter().enumerate().map(|(i, &(_, e))| (e, a + d * (i as f32 / n))).collect::<Vec<_>>()
            }
        };

        self.move_instances(if axis.is_some() { "Distribute" } else { "Distribute along a line" }, &moves)
    }
}
