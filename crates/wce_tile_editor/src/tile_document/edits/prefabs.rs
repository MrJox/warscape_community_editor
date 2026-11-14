//! Prefabs (`wce_tile::prefabs`): a copy is an instance group named after the prefab's key.

use super::*;

impl TileDocument {
    // ---- prefabs (wce_tile::prefabs): a copy is an instance group named after the prefab's key

    /// The copies of a prefab in the tile: the references to it (case aside), in document order.
    pub fn prefab_copies(&self, key: &str) -> Vec<usize> {
        let sc = self.tile().tscene();
        sc.find_all("UserInstanceGroup").into_iter().filter(|&g| self.prefab_key(g).is_some_and(|k| k.eq_ignore_ascii_case(key))).collect()
    }

    /// The prefab an instance group is a reference to (`wce_prefab`). A group that is only named after a prefab is a plain group: the
    /// ones TEd copied become references when the tile opens from the Assembly Kit, or with `prefab_refs::recognise` (whether there is such
    /// a prefab is the caller's to check).
    pub fn prefab_key(&self, group: usize) -> Option<String> {
        let sc = self.tile().tscene();
        if sc.type_of(group) != "UserInstanceGroup" {
            return None;
        }

        sc.doc().get(group, wce_tile::project_files::PREFAB_ATTR).filter(|k| !k.is_empty())
    }

    /// Whether an instance group is a reference to a prefab (placed by the editor), not a copy TEd made.
    pub fn is_prefab_reference(&self, group: usize) -> bool {
        self.tile().tscene().doc().get(group, wce_tile::project_files::PREFAB_ATTR).is_some_and(|k| !k.is_empty())
    }

    /// Makes a copy a plain group: its reference goes (an empty `wce_prefab`, which tile.wce and TEd's file leave out), and its name.
    pub fn unpack_prefab_copy(&mut self, group: usize, name: &str) -> bool {
        let key = self.prefab_key(group);
        let place = self.prefab_ground_place(group);
        if self.tile().tscene().doc().get(group, wce_tile::project_files::PREFAB_ATTR).is_some() {
            self.set_attribute(group, wce_tile::project_files::PREFAB_ATTR, "");
        }

        let renamed = self.rename(group, name);

        // Its ground stays: the layer it owned (still there until the ground is looked at again) becomes the user's.
        if let (Some(key), Some(n)) = (key, place) {
            self.keep_prefab_ground(n, &key);
        }

        renamed
    }

    /// A structural edit (adding, taking away, moving or grouping items) of items inside a prefab reference, or into one, is refused: what
    /// changes in a reference's items is kept as overrides, values only.
    ///
    /// # Errors
    ///
    /// An item (or `into`) is inside a reference: says which.
    /// Whether elements of these types may go into `into`: a deployment region only into a deployment zone, a siege AI shape only into a
    /// node or an edge, a capture location's boundary only into one, and those holders hold nothing else (`wce_tile::parents`).
    ///
    /// # Errors
    ///
    /// One may not: says where it goes.
    pub fn check_fits<'a>(&self, kinds: impl IntoIterator<Item = &'a str>, into: usize) -> Result<(), String> {
        let sc = self.tile().tscene();
        if into >= sc.doc().elements().len() {
            return Ok(());
        }

        let into_kind = sc.type_of(into);
        kinds.into_iter().try_for_each(|k| wce_tile::parents::fits(k, into_kind))
    }

    /// The nearest of `elem` and the elements above it that can hold a `kind` (`wce_tile::parents::holders`).
    pub fn holder_for(&self, kind: &str, elem: usize) -> Option<usize> {
        let sc = self.tile().tscene();
        let els = sc.doc().elements();
        let holders = wce_tile::parents::holders(kind);
        std::iter::successors(Some(elem).filter(|&e| e < els.len()), |&x| els[x].parent).find(|&x| holders.contains(&sc.type_of(x)))
    }

    /// The shapes a holder has (`wce_tile::parents::roles`), by its type.
    pub fn holder_roles(&self, holder: usize) -> &'static [wce_tile::parents::Role] {
        let sc = self.tile().tscene();
        if holder >= sc.doc().elements().len() {
            return &[];
        }

        wce_tile::parents::roles(sc.type_of(holder), &sc.doc().get(holder, "type").unwrap_or_default())
    }

    /// A holder's shapes of each of its roles.
    pub(super) fn role_shapes(&self, holder: usize) -> Vec<(wce_tile::parents::Role, Vec<usize>)> {
        let sc = self.tile().tscene();
        let kids = sc.children(holder);
        self.holder_roles(holder).iter().map(|&r| (r, kids.iter().copied().filter(|&c| sc.name(c) == r.name).collect())).collect()
    }

    /// The role a new shape takes in a holder: `named` (one of its roles), else the first one it has fewer of than it may have, else the
    /// first (which it replaces).
    ///
    /// # Errors
    ///
    /// Not a holder, a name that is not one of its roles, or a role it has all of that it may have more than one of (a street edge's two
    /// boundaries).
    pub fn shape_role(&self, holder: usize, named: Option<&str>) -> Result<wce_tile::parents::Role, String> {
        let shapes = self.role_shapes(holder);
        let Some(first) = shapes.first().map(|c| c.0) else {
            return Err("draw it inside a siege AI node or edge, a deployment zone or a capture location".to_owned());
        };

        let role =
            match named.filter(|n| !n.is_empty()) {
                Some(n) => shapes.iter().find(|c| c.0.name == n).map(|c| c.0).ok_or_else(|| {
                    format!("a {} has no shape named {n}", wce_tile::parents::label(self.tile().tscene().type_of(holder)))
                })?,

                None => shapes.iter().find(|c| c.0.most == 0 || c.1.len() < c.0.most).map_or(first, |c| c.0),
            };

        let have = shapes.iter().find(|c| c.0 == role).map_or(0, |c| c.1.len());
        if role.most > 1 && have >= role.most {
            return Err(format!("it has its {} {}s already: delete one first", role.most, role.name.to_lowercase()));
        }

        Ok(role)
    }

    /// Where a new shape (its role from its `name`) goes among its holder's: in place of the one it replaces (its role's, when the holder
    /// has only one), else in front of the first shape of a later role; and the one it replaces.
    pub(super) fn shape_place(&self, holder: usize, shape: &wce_tile::new_instances::NewInstance) -> (Option<usize>, Option<usize>) {
        let name = shape.attrs.get("name").map(String::as_str).unwrap_or_default();
        let shapes = self.role_shapes(holder);
        let Some(at) = shapes.iter().position(|c| c.0.name == name) else { return (None, None) };
        let replaced = if shapes[at].0.most == 1 { shapes[at].1.first().copied() } else { None };
        let later = shapes[at + 1..].iter().flat_map(|c| c.1.iter().copied()).min();
        (replaced.or(later), replaced)
    }

    /// A shape just put at the end of its holder moved in front of `before`, and `replaced` (which is before it) taken away; returns where
    /// the shape is then. Inside an edit.
    pub(super) fn put_shape(
        &mut self,
        shape: usize,
        holder: usize,
        before: Option<usize>,
        replaced: Option<usize>,
    ) -> Result<usize, String> {
        let Some(before) = before else { return Ok(shape) };
        let moved = self.tscene_mut().move_elements(vec![shape], holder, Some(before))?;
        let at = moved.first().copied().unwrap_or(shape);
        let Some(r) = replaced else { return Ok(at) };

        // In front of the one it replaces: that one comes after it now.
        let r = if r >= at { r + self.tile().tscene().doc().subtree_size(at) } else { r };
        self.tscene_mut().remove(r)?;
        Ok(at)
    }

    /// A capture location's boundary as a preset shape (`BoundaryShape`): its kind, its size (metres: a circle's or a semicircle's
    /// radius, a square's half side; a polygon's: its farthest point from its centre) and its turn (degrees: which way a semicircle's
    /// round side, a square's sides face; 0: +z), read back from what it is (TEd keeps no more: a semicircle's turn is in its points).
    pub fn boundary_shape(&self, boundary: usize) -> Option<BoundaryShape> {
        let t = self.tile();
        let sc = t.tscene();
        if boundary >= sc.doc().elements().len() || sc.type_of(boundary) != "CaptureLocationPolygonInstance" {
            return None;
        }

        let u = t.unit() as f32;
        let pts = t.polygon_world_points(boundary);
        let at = world_transform(sc, boundary).position;
        let position = Float3::new((at.x / f64::from(u)) as f32, (at.y / f64::from(u)) as f32, (at.z / f64::from(u)) as f32);
        let radius = || sc.doc().get(boundary, "radius").and_then(|r| r.trim().parse::<f32>().ok()).map(|r| r / u);
        let mean = || {
            let n = pts.len().max(1) as f32;
            pts.iter().fold(Float3::default(), |a, &p| a + p) * (1.0 / n)
        };

        // The angle of a direction on the ground (x = sin, z = cos: 0 is +z, 90 is +x).
        let angle = |dx: f32, dz: f32| dx.atan2(dz).to_degrees();
        Some(match sc.doc().get(boundary, "polygon_shape").as_deref() {
            Some("PS_CIRCLE") => BoundaryShape { kind: BoundaryKind::Circle, centre: position, size: radius().unwrap_or(20.0), turn: 0.0 },
            Some("PS_SEMICIRCLE") => {
                let mid = pts.get(pts.len() / 2).copied().unwrap_or(position);
                BoundaryShape {
                    kind: BoundaryKind::Semicircle,
                    centre: position,
                    size: radius().unwrap_or(20.0),
                    turn: angle(mid.x - position.x, mid.z - position.z),
                }
            }

            Some("PS_RECTANGLE") if pts.len() >= 2 => {
                let (a, b) = (pts[0], pts[1]);
                let turn = (-(b.z - a.z)).atan2(b.x - a.x).to_degrees();
                BoundaryShape { kind: BoundaryKind::Square, centre: mean(), size: (b.x - a.x).hypot(b.z - a.z) * 0.5, turn }
            }

            _ => {
                let c = mean();
                let size = pts.iter().map(|p| (p.x - c.x).hypot(p.z - c.z)).fold(0.0, f32::max);
                BoundaryShape { kind: BoundaryKind::Polygon, centre: c, size, turn: 0.0 }
            }
        })
    }

    /// A capture location's boundary made a preset shape (TEd's own: `PS_CIRCLE` with its 20 points, `PS_SEMICIRCLE` with 21 over half a
    /// turn, `PS_RECTANGLE` with 4 corners), at `shape.centre`, `shape.size` and `shape.turn`; it keeps its own attributes (name, flags,
    /// colours...) and its place among its capture location's children. A polygon: its points as they are, its shape `PS_POLYGON`
    /// (freely edited). One undo entry; the selection afterwards is `select` (the capture location, or the new boundary: `None`).
    ///
    /// # Errors
    ///
    /// Not a capture location's boundary.
    pub fn set_boundary_shape(&mut self, boundary: usize, shape: BoundaryShape, select: Option<usize>) -> Result<usize, String> {
        let t = self.tile();
        let sc = t.tscene();
        if boundary >= sc.doc().elements().len() || sc.type_of(boundary) != "CaptureLocationPolygonInstance" {
            return Err("not a capture location's boundary".to_owned());
        }

        let holder = self
            .holder_for("CaptureLocationPolygonInstance", sc.doc().elements()[boundary].parent.unwrap_or(boundary))
            .ok_or("the boundary is not in a capture location")?;
        let (r, turn) = (shape.size.max(0.5), shape.turn.to_radians());

        // On the ground, around the centre (metres): the canonical shape (a semicircle's round side and a square's top on +z) turned.
        let c = shape.centre;
        let around = |x: f32, z: f32| {
            let (sn, cs) = turn.sin_cos();
            Float3::new(c.x + x * cs + z * sn, c.y, c.z - x * sn + z * cs)
        };
        let ring = |a_deg: f32| {
            let a = a_deg.to_radians();
            around(r * a.sin(), r * a.cos())
        };
        let (kind, world): (&str, Vec<Float3>) = match shape.kind {
            BoundaryKind::Circle => ("PS_CIRCLE", (0..20).map(|i| ring(i as f32 * 18.0)).collect()),
            BoundaryKind::Semicircle => ("PS_SEMICIRCLE", (0..21).map(|i| ring(-90.0 + i as f32 * 9.0)).collect()),
            BoundaryKind::Square => ("PS_RECTANGLE", [(-r, r), (r, r), (r, -r), (-r, -r)].into_iter().map(|(x, z)| around(x, z)).collect()),
            BoundaryKind::Polygon => ("PS_POLYGON", t.polygon_world_points(boundary)),
        };

        // Local to the capture location; the boundary at the centre (a circle's and a semicircle's points are around it, as TEd's).
        let centre = t.to_local(holder, c);
        let local = world.iter().map(|&p| t.to_local(holder, p) - centre).collect::<Vec<_>>();
        let mut n = new_polygon("CaptureLocationPolygonInstance", local, true)
            .position(centre)
            .attr("polygon_shape", kind)
            .attr("polygon_closed", "1");
        if matches!(shape.kind, BoundaryKind::Circle | BoundaryKind::Semicircle) {
            n = n.attr("radius", wce_tile::ted_scene::format_number(f64::from(r) * t.unit()));
        }

        // Its own attributes kept (not what the shape decides).
        const SHAPED: [&str; 7] = ["position", "orientation", "polygon_shape", "radius", "area", "polygon_closed", "points"];
        for a in &sc.doc().elements()[boundary].attrs {
            if !SHAPED.contains(&a.name.as_str())
                && !n.attrs.contains_key(&a.name)
                && let Some(v) = sc.doc().get(boundary, &a.name)
            {
                n.attrs.insert(a.name.clone(), v);
            }
        }

        let text = wce_tile::new_instances::element_text(&n)?;
        let mark = self.begin()?;
        let result =
            self.tscene_mut().insert_element_text(holder, &text).and_then(|e| self.put_shape(e, holder, Some(boundary), Some(boundary)));
        match result {
            Ok(e) => {
                let after = select.unwrap_or(e);
                self.push_splice(format!("Boundary: {}", shape.kind.label()), mark, vec![item_of(after)], vec![item_of(after)]);
                Ok(e)
            }

            Err(err) => self.rolled_back(mark, err),
        }
    }

    /// A boundary as another preset (`kind`), keeping its centre and size; a semicircle or a square made from what had no turn (a circle, a
    /// polygon) faces the way its capture location's flag does (`flag_facing`).
    pub fn boundary_preset(&self, boundary: usize, kind: BoundaryKind) -> Option<BoundaryShape> {
        let mut shape = self.boundary_shape(boundary)?;
        if kind.turns() && !shape.kind.turns() {
            let sc = self.tile().tscene();
            let location = self.holder_for("CaptureLocationPolygonInstance", boundary);
            let facing = location.and_then(|l| sc.doc().get(l, "flag_facing")).and_then(|f| f.trim().parse::<f32>().ok());
            shape.turn = facing.map_or(0.0, f32::to_degrees);
        }

        shape.kind = kind;
        Some(shape)
    }

    /// The boundary of a capture location (its first `CaptureLocationPolygonInstance`).
    pub fn capture_boundary(&self, location: usize) -> Option<usize> {
        let sc = self.tile().tscene();
        (location < sc.doc().elements().len())
            .then(|| sc.children(location).into_iter().find(|&c| sc.type_of(c) == "CaptureLocationPolygonInstance"))
            .flatten()
    }

    /// The Inspector's rows of a capture location's boundary in place of TEd's shape and radius: its shape (Circle, Semicircle, Square,
    /// Polygon) and, for the presets, its size and turn, any change making its points again (`set_boundary_shape`; a drag at its end).
    pub(super) fn boundary_shape_rows(&mut self, e: usize, ins: &mut dyn Inspector) {
        let Some(shape) = self.boundary_shape(e) else { return };
        let kinds = BoundaryKind::ALL.map(|k| k.label().to_owned());
        let mut idx = BoundaryKind::ALL.iter().position(|&k| k == shape.kind).unwrap_or(0) as i32;
        let mut apply = None;
        ins.hint("The boundary's shape; choosing Circle, Semicircle or Square makes its points again.");
        if ins.edit_choice("wce_boundary_shape", "shape", &mut idx, &kinds) {
            apply = self.boundary_preset(e, BoundaryKind::ALL[idx.clamp(0, 3) as usize]);
        }

        if shape.kind != BoundaryKind::Polygon {
            for (field, label, value, speed) in [
                ("wce_boundary_size", "size (m: radius or half side)", shape.size, 0.1),
                ("wce_boundary_turn", "turn (deg; 0: +z)", shape.turn, 1.0),
            ] {
                if field == "wce_boundary_turn" && !shape.kind.turns() {
                    continue;
                }

                let key = (e, field.to_owned());
                let mut v = self.tile().drafts.get(&key).and_then(|d| d.parse::<f32>().ok()).unwrap_or(value);
                ins.hint(if field == "wce_boundary_size" {
                    "The boundary's size in metres: a circle's radius, half a square's side. Applied when the drag ends."
                } else {
                    "Which way the boundary faces, in degrees (0: +z). Applied when the drag ends."
                });
                if ins.edit_float(
                    field,
                    label,
                    &mut v,
                    speed,
                    if field == "wce_boundary_size" { 0.5 } else { -360.0 },
                    if field == "wce_boundary_size" { 500.0 } else { 360.0 },
                ) {
                    self.tile_mut().drafts.insert(key.clone(), v.to_string());
                }

                if ins.gesture_ended()
                    && let Some(d) = self.tile_mut().drafts.remove(&key).and_then(|d| d.parse::<f32>().ok())
                    && (d - value).abs() > 1e-4
                {
                    let mut s = shape;
                    if field == "wce_boundary_size" {
                        s.size = d
                    } else {
                        s.turn = d
                    }
                    apply = Some(s);
                }
            }
        }

        if let Some(s) = apply
            && let Err(err) = self.set_boundary_shape(e, s, None)
        {
            wce_foundation::log::warn(err);
        }
    }

    pub fn check_structure(&self, elems: &[usize], into: Option<usize>) -> Result<(), String> {
        let doc = self.tile().tscene().doc();
        let els = doc.elements();
        let locked = |e: usize, itself: bool| -> Option<usize> {
            let mut chain =
                std::iter::successors(if itself { Some(e) } else { els.get(e)?.parent }, |&p| els.get(p).and_then(|x| x.parent));
            chain.find(|&p| self.is_prefab_reference(p))
        };

        let hit = elems.iter().find_map(|&e| locked(e, false)).or_else(|| into.and_then(|i| locked(i, true)));
        match hit {
            Some(r) => Err(format!(
                "that is part of the prefab {}: a reference's items change only in their values (overrides); Open in Prefab tool changes the prefab, Unpack prefab makes it a plain group",
                self.prefab_key(r).unwrap_or_default()
            )),

            None => Ok(()),
        }
    }

    /// The outermost prefab reference an element is inside (not the element itself), and the element's path in its contents.
    pub fn reference_above(&self, elem: usize) -> Option<(usize, Vec<usize>)> {
        let doc = self.tile().tscene().doc();
        let els = doc.elements();
        let r = std::iter::successors(els.get(elem)?.parent, |&p| els[p].parent).filter(|&p| self.is_prefab_reference(p)).last()?;
        Some((r, wce_tile::prefab_refs::path_in(doc, r, elem)?))
    }

    /// A prefab's contents to compare its references with, from the project's prefab source (`TileProject::prefab_base`), read again when
    /// they change.
    pub(super) fn base_contents(&mut self, key: &str) -> Option<(Vec<u8>, std::sync::Arc<wce_tile::prefab_refs::BaseContents>)> {
        let src = self.project().prefab_base.clone()?;
        let bytes = (src.0)(key)?;
        if let Some((b, c)) = self.tile().prefab_cache.get(key)
            && *b == bytes
        {
            return Some((bytes, c.clone()));
        }

        let c = std::sync::Arc::new(wce_tile::prefab_refs::BaseContents::parse(&bytes).ok()?);
        self.tile_mut().prefab_cache.insert(key.to_owned(), (bytes.clone(), c.clone()));
        Some((bytes, c))
    }

    /// An item of a prefab reference's contents: the reference's key and the attributes it overrides, each with the prefab's value (`None`:
    /// the prefab's item has no such attribute). `None` when it is not in a reference, or the prefab has no such item there.
    pub fn overrides_of(&mut self, elem: usize) -> Option<(String, wce_tile::prefab_refs::Differences)> {
        let (r, path) = self.reference_above(elem)?;
        let key = self.prefab_key(r)?;
        let (_, base) = self.base_contents(&key)?;
        let diffs = base.differences(self.tile().tscene().doc(), elem, &path)?;
        Some((key, diffs))
    }

    /// The items of a prefab reference that override values of the prefab's (once each, in document order); empty when none do, or its
    /// contents differ by more than values.
    pub fn overridden_items(&self, reference: usize) -> Vec<usize> {
        let Some(key) = self.prefab_key(reference) else { return vec![] };
        let Some(base) = self.project().prefab_base.as_ref().and_then(|src| (src.0)(&key)) else { return vec![] };
        let sc = self.tile().tscene();
        let Ok(ovs) = wce_tile::prefab_refs::overrides(&sc.children_text(reference), &base) else { return vec![] };
        let mut out = ovs.iter().filter_map(|o| wce_tile::prefab_refs::element_at(sc.doc(), reference, &o.path)).collect::<Vec<_>>();
        out.dedup();
        out
    }

    /// How many values a prefab reference's contents override; `None` when they differ by more than values (or there is no prefab).
    pub fn override_count(&mut self, reference: usize) -> Option<usize> {
        let key = self.prefab_key(reference).filter(|_| self.is_prefab_reference(reference))?;
        let (bytes, _) = self.base_contents(&key)?;
        wce_tile::prefab_refs::overrides(&self.tile().tscene().children_text(reference), &bytes).ok().map(|o| o.len())
    }

    /// Gives an item of a prefab reference its prefab's values back (or, for the reference, all its contents): one undo entry. Returns how
    /// many values went back (a reference: 1 when its contents did).
    pub fn revert_overrides(&mut self, elem: usize) -> usize {
        if self.is_prefab_reference(elem) {
            let Some(key) = self.prefab_key(elem) else { return 0 };
            let Some((bytes, _)) = self.base_contents(&key) else { return 0 };
            return self.set_groups_children(&[elem], &bytes, &format!("Revert the overrides of {key}")).unwrap_or(0);
        }

        let Some((key, diffs)) = self.overrides_of(elem) else { return 0 };
        let back = diffs.into_iter().filter_map(|(a, v)| v.map(|v| (a, v))).collect::<Vec<_>>();
        if back.is_empty() {
            return 0;
        }

        let Ok(mark) = self.begin() else { return 0 };
        for (a, v) in &back {
            self.tscene_mut().set_attr(elem, a, v);
        }

        let attrs = back.iter().map(|(a, _)| a.as_str()).collect::<Vec<_>>();
        self.push_splice_attrs(
            format!("Revert the overrides of an item of {key}"),
            mark,
            vec![item_of(elem)],
            vec![item_of(elem)],
            vec![elem],
            &attrs,
        );
        back.len()
    }

    /// The Inspector's rows of a prefab reference's overrides: on one of its items, the values it overrides (marked) and reverting them; on
    /// the reference, how many and reverting them all. The revert buttons are always there, greyed out when there is nothing to revert.
    pub(super) fn override_rows(&mut self, e: usize, ins: &mut dyn Inspector) {
        if self.is_prefab_reference(e) {
            let key = self.prefab_key(e).unwrap_or_default();
            ins.text("prefab", "prefab", &key);
            let count = self.override_count(e);
            match count {
                Some(0) => ins.text("overrides", "overrides", "none"),
                Some(n) => {
                    ins.mark_fields(vec![("overrides".to_owned(), "Values of its items that are not the prefab's".to_owned())]);
                    ins.text("overrides", "overrides", &format!("{n} value(s) in its items"));
                }

                None => ins.text("overrides", "overrides", "its items differ by more than values (kept as they are)"),
            }

            ins.hint("Gives all the reference's items the prefab's values back (one undo step).");
            if ins.button("revert_all", "Revert all overrides", count.is_some_and(|n| n > 0)) {
                self.revert_overrides(e);
            }

            if self.tile().prefab_tool_ready {
                ins.hint("Opens the prefab itself in the Prefab tool, to change it for every reference.");
            }

            if self.tile().prefab_tool_ready && ins.button("open_in_prefab_tool", "Open in Prefab tool", true) {
                self.tile_mut().prefab_open_request = Some(key.clone());
            }

            self.prefab_ground_rows(e, ins);
            return;
        }

        let Some((key, diffs)) = self.overrides_of(e) else { return };
        ins.text("prefab", "in the prefab", &key);
        ins.text("overrides", "overrides", &if diffs.is_empty() { "none".to_owned() } else { format!("{} (marked)", diffs.len()) });
        let revertible = diffs.iter().any(|(_, v)| v.is_some());
        ins.hint("Gives this item the prefab's values back (one undo step).");
        if ins.button("revert", "Revert to the prefab's values", revertible) {
            self.revert_overrides(e);
        }

        let marks = diffs
            .into_iter()
            .map(|(a, v)| {
                let note = match v {
                    Some(v) => format!("Override: the prefab has {v}"),
                    None => "Override: the prefab's item has no such value".to_owned(),
                };

                (a, note)
            })
            .collect::<Vec<_>>();
        if !marks.is_empty() {
            ins.mark_fields(marks);
        }
    }

    /// Makes instance groups references to prefabs (each with its key; the copies `prefab_refs::recognise` found), one undo entry. Returns
    /// how many.
    pub fn make_references(&mut self, groups: &[(usize, String)], what: &str) -> usize {
        let n = self.tile().tscene().doc().elements().len();
        let groups = groups.iter().filter(|(g, _)| *g < n).collect::<Vec<_>>();
        if groups.is_empty() {
            return 0;
        }

        let Ok(mark) = self.begin() else { return 0 };
        for (g, key) in &groups {
            self.tscene_mut().set_attr(*g, wce_tile::project_files::PREFAB_ATTR, key);
        }

        let elems = groups.iter().map(|(g, _)| *g).collect();
        self.push_splice_attrs(what.to_owned(), mark, vec![], vec![], elems, &[wce_tile::project_files::PREFAB_ATTR]);
        groups.len()
    }

    /// Makes an instance group a reference to the prefab `key` (after "Save as prefab").
    pub fn make_prefab_reference(&mut self, group: usize, key: &str) -> bool {
        self.rename(group, key) && self.set_attribute(group, wce_tile::project_files::PREFAB_ATTR, key)
    }

    /// The text of an element (at column 0) and of a container's contents, as a prefab file holds them.
    pub fn element_text(&self, elem: usize) -> Vec<u8> {
        self.tile().tscene().element_text(elem)
    }

    pub fn children_text(&self, elem: usize) -> Vec<u8> {
        self.tile().tscene().children_text(elem)
    }

    /// Places a copy of a prefab (its group's text) in the layer `into`, named after the key, standing on the ground at `at` (world metres).
    /// One undo entry; the selection afterwards is the copy.
    ///
    /// # Errors
    ///
    /// No such layer, or the text is not one element.
    pub fn place_prefab(&mut self, key: &str, group_text: &[u8], into: usize, at: Float3) -> Result<usize, String> {
        let text = prefab_reference_text(group_text, key);
        self.place_text(&format!("Place {key}"), &text, into, at, Some(key), true)
    }

    /// Gives every group of `groups` these contents (text at column 0; their positions are relative to the group, so each copy keeps its
    /// place). One undo entry named `what`; the selection afterwards is empty. Returns how many groups changed.
    ///
    /// # Errors
    ///
    /// A group without a children element.
    pub fn set_groups_children(&mut self, groups: &[usize], children: &[u8], what: &str) -> Result<usize, String> {
        let mut groups = groups.to_vec();
        groups.sort_unstable();
        groups.dedup();

        // From the last one: an edit moves only the indices after it.
        let changed = groups.iter().filter(|&&g| self.tile().tscene().children_text(g) != children).copied().collect::<Vec<_>>();
        if changed.is_empty() {
            return Ok(0);
        }

        let mark = self.begin()?;
        for &g in changed.iter().rev() {
            if let Err(e) = self.tscene_mut().set_children_text(g, children) {
                let record = self.tscene_mut().end_edit(mark).unwrap_or_default();
                let _ = self.tscene_mut().unapply(&record);
                return Err(e);
            }
        }

        self.push_splice(what.to_owned(), mark, vec![], vec![]);
        Ok(changed.len())
    }

    /// Gives each instance group its own contents (`(group, contents)`), one undo entry. Returns how many changed.
    ///
    /// # Errors
    ///
    /// A group's contents do not read (nothing changes).
    pub fn set_each_group_children(&mut self, groups: &[(usize, Vec<u8>)], what: &str) -> Result<usize, String> {
        let mut groups = groups.iter().filter(|(g, c)| self.tile().tscene().children_text(*g) != *c).collect::<Vec<_>>();
        groups.sort_unstable_by_key(|(g, _)| *g);
        groups.dedup_by_key(|(g, _)| *g);
        if groups.is_empty() {
            return Ok(0);
        }

        // From the last one: an edit moves only the indices after it.
        let mark = self.begin()?;
        for (g, c) in groups.iter().rev() {
            if let Err(e) = self.tscene_mut().set_children_text(*g, c) {
                let record = self.tscene_mut().end_edit(mark).unwrap_or_default();
                let _ = self.tscene_mut().unapply(&record);
                return Err(e);
            }
        }

        self.push_splice(what.to_owned(), mark, vec![], vec![]);
        Ok(groups.len())
    }

    /// One undo entry; the selection afterwards is empty.
    pub fn remove_items(&mut self, elems: &[usize]) -> bool {
        if let Err(err) = self.check_structure(elems, None) {
            wce_foundation::log::warn(err);
            return false;
        }

        let sc = self.tile().tscene();
        let valid = elems.iter().copied().filter(|&e| e < sc.doc().elements().len() && sc.type_of(e) != "SCENE_GRAPH").collect();
        let top = top_level(sc, valid);
        if top.is_empty() {
            return false;
        }

        let name = if top.len() == 1 { format!("Delete {}", sc.type_of(top[0])) } else { format!("Delete {} items", top.len()) };
        let sel_before = top.iter().map(|&e| item_of(e)).collect();
        let spans = self.subtree_spans(&top);
        let Ok(mark) = self.begin() else { return false };

        // From the last one: the indices of the others stay.
        for &e in top.iter().rev() {
            let _ = self.tscene_mut().remove(e);
        }

        self.push_splice_removed(name, mark, sel_before, spans);
        true
    }

    /// # Errors
    ///
    /// Not a layer, or a layer that can't be copied.
    pub fn duplicate_layer(&mut self, layer: usize) -> Result<usize, String> {
        self.check_structure(&[layer], None)?;
        let sc = self.tile().tscene();
        if layer >= sc.doc().elements().len() || !sc.is_layer(layer) || sc.type_of(layer) == "SCENE_GRAPH" {
            return Err("not a layer".to_owned());
        }

        let mark = self.begin()?;
        let copy = match self.tscene_mut().duplicate(layer) {
            Ok(c) => c,
            Err(_) => return self.rolled_back(mark, "cannot duplicate this layer"),
        };
        let name = format!("{} copy", self.tile().tscene().name(layer));
        self.tscene_mut().set_attr(copy, "name", &name);
        self.push_splice_added("Duplicate layer".to_owned(), mark, vec![item_of(layer)], vec![item_of(copy)], &[copy]);
        Ok(copy)
    }
}
