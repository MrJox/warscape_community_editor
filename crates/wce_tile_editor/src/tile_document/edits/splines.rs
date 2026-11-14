//! Rivers and roads (`wce_tile::splines`): their points, values and sharp corners.

use super::*;

impl TileDocument {
    // ---- splines (wce_tile::splines): rivers and roads

    pub fn is_spline(&self, elem: usize) -> bool {
        let sc = self.tile().tscene();
        elem < sc.doc().elements().len() && wce_tile::splines::is_spline(sc.type_of(elem))
    }

    /// Whether its points have handles: TEd's splines and the editor's own (procedural roads and walls, `splines::OWN_KINDS`).
    pub fn has_handles(&self, elem: usize) -> bool {
        let sc = self.tile().tscene();
        elem < sc.doc().elements().len() && wce_tile::splines::has_handles(sc.type_of(elem))
    }

    /// A spline point edit, `world` the cursor:
    /// - `move`: the point, its handles with it;
    /// - `in` / `out`: its `controlIn` / `controlOut`, at the point's height, the other handle mirrored;
    /// - `insert`: a new point after `index` (after the last one: the spline grows), its handles made from its neighbours;
    /// - `prepend`: a new first point;
    /// - `remove`: the point (a spline keeps 2).
    ///
    /// The handles no segment uses (the first point's `controlIn`, the last one's `controlOut`) stay `0,0,0`, as TEd writes them.
    ///
    /// # Errors
    ///
    /// Not a spline, no such point, or an unknown operation.
    pub fn spline_point(&mut self, elem: usize, op: &str, index: usize, world: Float3, gesture: u64) -> Result<(), String> {
        use wce_tile::new_instances::point_text;
        use wce_tile::splines::{auto_handles, mirror, read};

        if !self.has_handles(elem) {
            return Err("not a spline".to_owned());
        }

        let t = self.tile();
        let sc = t.tscene();
        let pts = read(sc, elem);
        // The editor's own splines: their handles not written are made from the points (and go on being): a point moved takes along only
        // those written.
        let own = !wce_tile::splines::is_spline(sc.type_of(elem));
        let written = wce_tile::splines::written(sc, elem);
        let keeps = |i: usize, side: usize| !own || written.get(i).is_some_and(|h| h[side].is_some());
        let n = pts.len();
        let elems = sc.polygon_points(elem);
        if index >= n.max(1) {
            return Err("no such point".to_owned());
        }

        let local = t.to_local(elem, world);
        // An attribute not written (a procedural spline's handle made from the points) is as it is drawn now.
        let text = |i: usize, attr: &str| {
            sc.doc().get(elems[i], attr).filter(|v| !v.is_empty()).unwrap_or_else(|| match attr {
                "controlIn" => point_text(pts[i].control_in),
                "controlOut" => point_text(pts[i].control_out),
                _ => String::new(),
            })
        };
        let step = |i: usize, attr: &str, v: Double3| (i, attr.to_owned(), text(i, attr), point_text(v));
        let (first, last) = (index == 0, index + 1 == n);
        let attrs = match op {
            "move" => {
                let p = pts[index];
                let d = local - p.position;
                let mut steps = vec![step(index, "position", local)];
                if !first && keeps(index, 0) {
                    steps.push(step(index, "controlIn", p.control_in + d));
                }

                if !last && keeps(index, 1) {
                    steps.push(step(index, "controlOut", p.control_out + d));
                }

                Some(("Move spline point", steps))
            }

            "in" | "out" => {
                let p = pts[index];
                let h = Double3::new(local.x, p.position.y, local.z);
                let (this, other, used) = if op == "in" { ("controlIn", "controlOut", !last) } else { ("controlOut", "controlIn", !first) };
                if (op == "in" && first) || (op == "out" && last) {
                    return Err("an end's outer handle is not used".to_owned());
                }

                let mut steps = vec![step(index, this, h)];
                if used {
                    steps.push(step(index, other, mirror(p.position, h)));
                }

                Some(("Move spline handle", steps))
            }

            _ => None,
        };

        if let Some((name, steps)) = attrs {
            let steps = steps.into_iter().filter(|s| s.2 != s.3).collect::<Vec<_>>();
            if steps.is_empty() {
                return Ok(());
            }

            // A handle the point has not written yet: written first as it is, in a text splice (its undo takes the attribute away
            // again, which setting it back cannot), then edited as the others.
            let missing = steps
                .iter()
                .filter(|s| self.tile().tscene().doc().get(elems[s.0], &s.1).is_none_or(|v| v.is_empty()))
                .cloned()
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                let mark = self.begin()?;
                let before = self.tile().tscene().polygon_points(elem);
                let sc = self.tscene_mut();
                for (i, attr, was, _) in &missing {
                    sc.set_attr(elems[*i], attr, was);
                }

                self.push_splice_point("Write spline handles".to_owned(), mark, elem, &before);
            }

            self.history.push(Box::new(TedPointAttrsCommand { name: name.to_owned(), elem, steps, gesture }));
            return Ok(());
        }

        let set = |sc: &mut TedScene, i: usize, attr: &str, v: Double3| {
            let p = sc.polygon_points(elem)[i];
            sc.set_attr(p, attr, &point_text(v));
        };

        let mark = self.begin()?;
        let before = self.tile().tscene().polygon_points(elem);
        let result = match op {
            "insert" => {
                let mut positions = pts.iter().map(|p| p.position).collect::<Vec<_>>();
                positions.insert(index + 1, local);
                let (cin, cout) = auto_handles(&positions, index + 1);
                let sc = self.tscene_mut();
                if let Err(e) = sc.insert_polygon_point(elem, index, local) {
                    return self.rolled_back(mark, e);
                }
                set(sc, index + 1, "controlIn", cin);
                set(sc, index + 1, "controlOut", cout);
                if last {
                    // The old end is inside now: its outer handle mirrors its inner one (or is made, for a spline of one point).
                    let p = pts[index];
                    let out = if n > 1 { mirror(p.position, p.control_in) } else { auto_handles(&positions, index).1 };
                    set(sc, index, "controlOut", out);
                }

                Ok("Insert spline point")
            }

            "prepend" => {
                let mut positions = pts.iter().map(|p| p.position).collect::<Vec<_>>();
                positions.insert(0, local);
                let (_, cout) = auto_handles(&positions, 0);
                let p = pts[0];
                let sc = self.tscene_mut();

                // The copy after the first point keeps its values; the first one becomes the new point.
                if let Err(e) = sc.insert_polygon_point(elem, 0, p.position) {
                    return self.rolled_back(mark, e);
                }
                set(sc, 0, "position", local);
                set(sc, 0, "controlIn", Double3::ZERO);
                set(sc, 0, "controlOut", cout);
                let cin = if n > 1 { mirror(p.position, p.control_out) } else { auto_handles(&positions, 1).0 };
                set(sc, 1, "controlIn", cin);
                Ok("Insert spline point")
            }

            "remove" => {
                let sc = self.tscene_mut();
                if sc.remove_polygon_point(elem, index)? {
                    if first {
                        set(sc, 0, "controlIn", Double3::ZERO);
                    } else if last {
                        set(sc, index - 1, "controlOut", Double3::ZERO);
                    }

                    Ok("Remove spline point")
                } else {
                    Err("a spline needs at least 2 points".to_owned())
                }
            }

            _ => Err(format!("unknown point operation '{op}'")),
        };

        match result {
            Ok(name) => {
                self.push_splice_point(name.to_owned(), mark, elem, &before);
                Ok(())
            }

            Err(err) => self.rolled_back(mark, err),
        }
    }

    /// Reverses a spline's direction (a river's flow, a road's way): the points in the other order, each with its own values (`width`,
    /// `foam_amount`, ...) and its `controlIn` and `controlOut` swapped. Only the points' attribute texts are permuted (no number is
    /// computed again), so the spline stays exactly where it is and flipping twice gives the file's bytes back. The instance does not move:
    /// the first point is no longer at its origin, as TEd puts it (a habit, not a rule: 10 of the kit's 132 rivers have it off zero; the
    /// river files carry world positions). One undo entry. TEd has no such command (wce's).
    ///
    /// # Errors
    ///
    /// Not a spline, or fewer than 2 points.
    pub fn flip_spline(&mut self, elem: usize) -> Result<(), String> {
        if !self.is_spline(elem) {
            return Err("not a spline".to_owned());
        }

        let sc = self.tile().tscene();
        let elems = sc.polygon_points(elem);
        let n = elems.len();
        if n < 2 {
            return Err("a spline needs at least 2 points".to_owned());
        }

        // Every point's attributes as written, the handles under each other's name.
        let doc = sc.doc();
        let texts = elems
            .iter()
            .map(|&p| {
                doc.elements()[p]
                    .attrs
                    .iter()
                    .map(|a| {
                        let name = match a.name.as_str() {
                            "controlIn" => "controlOut",
                            "controlOut" => "controlIn",
                            other => other,
                        };

                        (name.to_owned(), doc.get(p, &a.name).unwrap_or_default())
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        let mark = self.begin()?;
        let sc = self.tscene_mut();
        for (i, &dst) in elems.iter().enumerate() {
            for (k, v) in &texts[n - 1 - i] {
                if sc.doc().get(dst, k).as_deref() != Some(v.as_str()) {
                    sc.set_attr(dst, k, v);
                }
            }
        }

        self.push_splice_touching("Flip spline".to_owned(), mark, vec![item_of(elem)], vec![item_of(elem)], vec![elem]);
        Ok(())
    }

    /// A value of spline points (`width`, `foam_amount`, `alpha_fade`, `flow_speed`, `terrain_offset`): one undo entry, the steps of one
    /// gesture merge.
    pub fn set_spline_value(&mut self, elem: usize, points: &[usize], attr: &str, value: &str, gesture: u64) -> bool {
        if !self.is_spline(elem) {
            return false;
        }

        let sc = self.tile().tscene();
        let elems = sc.polygon_points(elem);
        let steps = points
            .iter()
            .filter_map(|&i| {
                let before = sc.doc().get(*elems.get(i)?, attr).unwrap_or_default();
                Some((i, attr.to_owned(), before, value.to_owned()))
            })
            .collect::<Vec<_>>();
        if steps.is_empty() {
            return false;
        }

        self.history.push(Box::new(TedPointAttrsCommand { name: format!("Set {attr}"), elem, steps, gesture }));
        true
    }

    /// # Errors
    ///
    /// No parent layer (a tile without SCENE_GRAPH).
    pub fn add_layer(&mut self, parent: Option<usize>, name: &str) -> Result<usize, String> {
        let parent = parent.ok_or("this tile has no SCENE_GRAPH, so layers cannot be added")?;
        let mark = self.begin()?;
        let layer = match self.tscene_mut().add_layer(Some(parent), if name.is_empty() { "New layer" } else { name }) {
            Ok(l) => l,
            Err(e) => return self.rolled_back(mark, e),
        };
        self.push_splice_added("Add layer".to_owned(), mark, vec![], vec![item_of(layer)], &[layer]);
        Ok(layer)
    }

    /// Keeps the world position and orientation.
    ///
    /// # Errors
    ///
    /// A layer moved into itself, or a move the text does not allow.
    pub fn move_to_layer(&mut self, elem: usize, layer: usize) -> Result<bool, String> {
        if elem == layer {
            return Ok(false);
        }

        self.check_structure(&[elem], Some(layer))?;
        self.check_fits([self.tile().tscene().type_of(elem)], layer)?;

        if self.tile().tscene().is_descendant(layer, elem) {
            return Err("cannot move a layer into itself".to_owned());
        }

        let mark = self.begin()?;
        let sc = self.tile().tscene();
        let w = world_transform(sc, elem);
        let has_pos = sc.position(elem).is_some();
        let moved = match self.tscene_mut().move_to_layer(elem, layer) {
            Ok(m) => m,
            Err(e) => return self.rolled_back(mark, e),
        };
        let sc = self.tscene_mut();
        if has_pos {
            set_world_position(sc, moved, w.position);
        }

        if sc.euler(moved).is_some() {
            set_world_rotation(sc, moved, &w.rotation);
        }

        self.push_splice("Move to layer".to_owned(), mark, vec![item_of(elem)], vec![item_of(moved)]);
        Ok(true)
    }

    /// Sets an attribute of a polygon's point (a wall's `sharp`), the polygon staying selected.
    pub fn set_point_attribute(&mut self, elem: usize, index: usize, attr: &str, value: &str) -> bool {
        let Some(&p) = self.tile().tscene().polygon_points(elem).get(index) else { return false };
        if self.tile().tscene().doc().get(p, attr).as_deref() == Some(value) {
            return true;
        }

        let Ok(mark) = self.begin() else { return false };
        self.tscene_mut().set_attr(p, attr, value);
        self.push_splice_touching(format!("Set point {attr}"), mark, vec![item_of(elem)], vec![item_of(elem)], vec![elem]);
        true
    }

    pub fn rename(&mut self, elem: usize, name: &str) -> bool {
        self.set_attribute(elem, "name", name)
    }

    pub fn set_attribute(&mut self, elem: usize, attr: &str, value: &str) -> bool {
        if elem >= self.tile().tscene().doc().elements().len() {
            return false;
        }

        if self.tile().tscene().doc().get(elem, attr).as_deref() == Some(value) {
            return true;
        }

        let Ok(mark) = self.begin() else { return false };
        self.tscene_mut().set_attr(elem, attr, value);
        self.push_splice_attrs(format!("Set {attr}"), mark, vec![item_of(elem)], vec![item_of(elem)], vec![elem], &[attr]);
        true
    }

    pub fn remove_layer(&mut self, layer: usize) -> bool {
        if let Err(err) = self.check_structure(&[layer], None) {
            wce_foundation::log::warn(err);
            return false;
        }

        let sc = self.tile().tscene();
        if layer >= sc.doc().elements().len() || !sc.is_layer(layer) || sc.type_of(layer) == "SCENE_GRAPH" {
            return false;
        }

        self.remove(layer)
    }
}
