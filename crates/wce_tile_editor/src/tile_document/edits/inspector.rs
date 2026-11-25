//! The Inspector's rows and the gizmo's edits.

use super::*;

impl TileDocument {
    // ---- inspector and gizmo

    pub(crate) fn inspect_item(&mut self, item: SourceRef, ins: &mut dyn Inspector) {
        if let Some((e, k)) = piece_of(item.item) {
            self.inspect_piece(e, k, ins);
            return;
        }

        let Some(e) = elem_of(item.item) else {
            self.inspect_terrain(ins);
            return;
        };

        if e >= self.tile().tscene().doc().elements().len() {
            return;
        }

        let t = self.tile();
        let sc = t.tscene();
        ins.section(sc.type_of(e));
        self.override_rows(e, ins);
        let t = self.tile();
        let sc = t.tscene();
        let before = sc.name(e);
        if sc.get(e, "name").is_some() {
            // TEd renames in the Scene tab; the name is not one of the properties.
            let drafts_key = (e, "name".to_owned());
            let mut shown = self.tile().drafts.get(&drafts_key).cloned().unwrap_or_else(|| before.clone());
            ins.hint("The element's name, as the Scene panel lists it. Applied when the typing ends.");
            if ins.edit_string("name", "name", &mut shown) {
                self.tile_mut().drafts.insert(drafts_key.clone(), shown);
            }

            if ins.gesture_ended()
                && let Some(v) = self.tile_mut().drafts.remove(&drafts_key)
                && v != before
            {
                self.rename(e, &v);
            }
        } else {
            ins.text("name", "name", &before);
        }

        self.variation_rows(e, ins);
        self.road_role_row(e, ins);
        let t = self.tile();
        let sc = t.tscene();
        if let Some(k) = sc.get(e, "key").filter(|_| sc.type_of(e) == "BUILDING_INSTANCE") {
            let model = match t.building_model(&k) {
                Some(m) if m.ok => format!("{} pieces, {} destruction levels", m.pieces.len(), m.levels.max(1)),
                Some(_) => "no model: box".to_owned(),
                None => String::new(),
            };

            if let Some(c) = t.building_category(&k) {
                ins.text("category", "category", &c);
            }

            ins.text("model", "model", &model);
        }

        let sc = self.tile().tscene();
        ins.text("layer", "layer", &sc.layer_path(e));

        let tag = sc.type_of(e).to_owned();
        let Some(schema) = wce_tile::properties::schema(&tag) else {
            self.inspect_unknown(e, ins);
            return;
        };

        // The rows of TEd's Property Editor, in its order.
        ins.section("Properties");
        let mut shown = self.shown_attrs(e);
        for prop in schema {
            shown.push(prop.attr);
            if let Some(twin) = prop.twin {
                shown.push(twin);
            }

            // A capture location's boundary: its preset shape, size and turn in place of TEd's shape and radius rows.
            if tag == "CaptureLocationPolygonInstance" && matches!(prop.attr, "polygon_shape" | "radius") {
                if prop.attr == "polygon_shape" {
                    self.boundary_shape_rows(e, ins);
                }

                continue;
            }

            self.property_row(e, prop, ins);
            if prop.attr == "burn" && self.tile().tscene().type_of(e) == "BUILDING_INSTANCE" {
                self.fire_controls(e, ins);
            }
        }

        // A kit wall: made into the buildings of its pieces (kit_walls.rs).
        if tag == super::super::WALL_SPLINE && !wce_walls::scene::is_procedural(self.tile().tscene(), e) {
            let can = self.can_convert_wall(e);
            ins.hint(&match &can {
                Ok(n) => format!("Makes the wall's {n} pieces buildings of their own where they are (one undo). The wall is kept, hidden and not exported, \
                    in a layer of its own inside its layer, and its ground becomes a height layer of yours"),
                Err(why) => format!("Not now: {why}"),
            });
            if ins.button("convert_to_buildings", "Convert to buildings", can.is_ok()) {
                match self.convert_wall_to_buildings(e) {
                    Ok(msg) => {
                        wce_foundation::log::info(&msg);
                        self.tile_mut().last_message = msg;
                    }

                    Err(why) => wce_foundation::log::warn(format!("Convert to buildings: {why}")),
                }

                return;
            }
        }

        // A river: the tool's River profile next, then its bed, its shores and the rest (`inspect_river_tail`). A lake: its rows here.
        if tag == "SplineRiverInstance" {
            return;
        }

        if self.tile().tscene().get(e, super::super::lakes::OUTLINE_ATTR).is_some() {
            self.river_bed_rows(e, ins);
        }

        self.other_attribute_rows(e, &shown, ins);
    }

    /// A river's sections after its River profile (the tool's, below Properties): its bed, its shores, and the other attributes at the
    /// bottom.
    pub fn inspect_river_tail(&mut self, e: usize, ins: &mut dyn Inspector) {
        if self.tile().tscene().type_of(e) != "SplineRiverInstance" {
            return;
        }

        self.river_bed_rows(e, ins);
        let mut shown = self.shown_attrs(e);
        if let Some(schema) = wce_tile::properties::schema("SplineRiverInstance") {
            for prop in schema {
                shown.push(prop.attr);
                shown.extend(prop.twin);
            }
        }

        self.other_attribute_rows(e, &shown, ins);
    }

    /// The attributes the rows above show (or keep out of sight), but the schema's own.
    fn shown_attrs(&self, e: usize) -> Vec<&'static str> {
        let tag = self.tile().tscene().type_of(e).to_owned();
        // The reference's key is the prefab row above.
        let mut shown = vec![
            "name",
            wce_tile::project_files::PREFAB_ATTR,
            wce_tile::road_pieces::PART_ROLE_ATTR,
            wce_tile::road_pieces::PIECE_WEIGHT_ATTR,
        ];

        // A reference's ground has its own rows, above.
        shown.extend([
            super::super::prefab_ground::TERRAIN_ATTR,
            super::super::prefab_ground::TERRAIN_OPACITY_ATTR,
            super::super::prefab_ground::TERRAIN_FEATHER_ATTR,
        ]);

        // A stage's group: the Prefab tool's rows (its ground's settings), below.
        shown.extend([wce_tile::prefab_stage::STAGE_ATTR, wce_tile::prefab_stage::NORMALIZE_ATTR, wce_tile::prefab_stage::FEATHER_ATTR]);

        // A river's bed and shores have their own rows; its id is the editor's.
        shown.extend(super::super::river_beds::RIVER_ROW_ATTRS);
        shown.extend(super::super::shores::SHORE_ROW_ATTRS);
        // Its width variation's settings: the Inspector's Width variation (`river_widths.rs`).
        shown.extend(super::super::river_widths::WIDTH_ROW_ATTRS);

        // A reference's own are the tool's rows, below.
        if matches!(tag.as_str(), super::super::REF_HUMAN | super::super::REF_UNIT | super::super::REF_SHIP) {
            shown.extend(wce_tile::new_instances::REF_TOOL_ATTRS);
        }

        if wce_tile::new_instances::MEASURE_KINDS.contains(&tag.as_str()) {
            shown.extend(wce_tile::new_instances::MEASURE_HIDDEN_ATTRS);
        }

        shown
    }

    /// What the file has and the grid does not show, at the bottom; a polygon's point count.
    fn other_attribute_rows(&self, e: usize, shown: &[&str], ins: &mut dyn Inspector) {
        let sc = self.tile().tscene();
        // A combined project's variations are the checkboxes above (`variation_rows`), not text here.
        let rest = sc.doc().elements()[e]
            .attrs
            .iter()
            .enumerate()
            .filter(|(_, a)| {
                !shown.contains(&a.name.as_str())
                    && a.name != wce_tile::variations::VARIATIONS_ATTR
                    && a.name != wce_tile::variations::GROUP_ATTR
            })
            .collect::<Vec<_>>();
        if !rest.is_empty() {
            ins.section("Other attributes");
            for (i, a) in rest {
                ins.text(&format!("attr_{}", a.name), &a.name, &shown_raw(sc, e, i));
            }
        }

        // Polygons: the point count (their points are edited in the viewport).
        if self.tile().is_polygon(e) {
            ins.text("points", "points", &sc.polygon_points(e).len().to_string());
        }
    }

    /// A group inside a road piece on the Prefab tool's stage (its prefab's key `road_...`): its role in the piece (`road_pieces::PartRole`),
    /// what decides how its parts follow the road, whatever the group's name.
    fn road_role_row(&mut self, e: usize, ins: &mut dyn Inspector) {
        use wce_tile::road_pieces::{PART_ROLE_ATTR, PartRole};
        let t = self.tile();
        let sc = t.tscene();
        let Some(stage) = self.stage_group().filter(|_| sc.type_of(e) == "UserInstanceGroup") else { return };

        // The piece itself: how often it is picked against the other variants of its piece.
        if stage == e {
            use wce_tile::road_pieces::PIECE_WEIGHT_ATTR;
            if !sc.name(e).starts_with("road_") {
                return;
            }

            let before = sc.get(e, PIECE_WEIGHT_ATTR).and_then(|w| w.trim().parse::<f32>().ok()).unwrap_or(1.0);
            let mut w = before;
            ins.hint(
                "How often this piece is laid against its other variants (straight, straight_2, ...): 2 is twice as often as 1, 0 never.",
            );
            if ins.edit_float("road_weight", "road weight", &mut w, 0.05, 0.0, 20.0) && (w - before).abs() > 1e-6 {
                let text = if (w - 1.0).abs() < 1e-6 { String::new() } else { wce_tile::ted_scene::format_number(f64::from(w)) };
                self.set_attribute(e, PIECE_WEIGHT_ATTR, &text);
            }

            return;
        }

        let els = sc.doc().elements();
        if !sc.name(stage).starts_with("road_") || !std::iter::successors(els[e].parent, |&p| els[p].parent).any(|p| p == stage) {
            return;
        }

        let current = sc.get(e, PART_ROLE_ATTR).and_then(|r| PartRole::parse(&r));
        let names = std::iter::once("none").chain(PartRole::ALL.iter().map(|r| r.name())).map(str::to_owned).collect::<Vec<_>>();
        let tips = [
            "Its parts are anchored by where they stand: on the road, by an edge or on the verge.",
            "The road's surface (ruts, mud): widened with the road.",
            "The road's edges (borders): each keeps its distance from its edge.",
            "The verges (banks, erosion): each keeps its distance from the verge.",
            "A fence: its parts take the road's fence settings.",
            "Optional detail (rocks, puddles, patches): each shows by the road's detail density and seed.",
            "Stays where it is in the piece (a junction's middle).",
        ]
        .map(str::to_owned);
        let mut idx = current.map_or(0, |r| PartRole::ALL.iter().position(|&x| x == r).map_or(0, |i| i as i32 + 1));
        let before = idx;
        ins.hint("What this group's parts are in the road piece; a part's own road_anchor still wins.");
        if ins.edit_choice_tips("road_role", "road role", &mut idx, &names, &tips) && idx != before {
            let value = usize::try_from(idx - 1).ok().and_then(|i| PartRole::ALL.get(i)).map_or("", |r| r.name());
            self.set_attribute(e, PART_ROLE_ATTR, value);
        }
    }

    /// A building's fire preview (fire.rs): where the sequence is, and playing or stopping it. Not an edit: the file keeps its values.
    /// The burning sequence's preview, on the `burn` row: play, stop and where it is.
    pub(super) fn fire_controls(&mut self, e: usize, ins: &mut dyn Inspector) {
        let time = self.tile().fire_time(e);
        let state = match time {
            Some(t) => {
                let (amount, stage) = super::super::fire::fire_at(t);
                let what = if stage == 1 {
                    "flames"
                } else if t < super::super::fire::STAGE_ONE_SECONDS + super::super::fire::STAGE_TWO_SECONDS {
                    "collapse"
                } else {
                    "burnt"
                };
                format!("{t:.0} s: {what} {amount:.2}")
            }

            None => "not burning".to_owned(),
        };

        // Lucide's play and square (`wce_editor::icons`).
        match ins.row_buttons(
            &[("fire_play", "\u{e13c}", "Play the fire", true), ("fire_stop", "\u{e167}", "Stop the fire", time.is_some())],
            &state,
        ) {
            Some(0) => {
                let now = self.tile().fire_now();
                self.tile_mut().fire_start(e, now);
            }

            Some(_) => self.tile_mut().fire_stop(Some(e)),
            None => {}
        }
    }

    /// The Property Editor of a building piece (TEd: `BUILDING_PIECE_INSTANCE`). TEd does not write the pieces to the tile, so these values live
    /// for the session; the position, orientation, scale, visibility and destruction level are shown in the viewport.
    pub(super) fn inspect_piece(&mut self, e: usize, k: usize, ins: &mut dyn Inspector) {
        let t = self.tile();
        let Some(model) = t.model_of(e).filter(|m| k < m.pieces.len()) else { return };
        let levels = i64::from(model.levels.max(1));
        let file = model.pieces[k].rsplit('/').next().unwrap_or_default().to_owned();
        let owner = t.tscene().name(e);
        ins.section("building piece");
        ins.text("owner", "building", &owner);
        ins.text("mesh", "mesh", &file);

        let mut p = self.tile().piece_props_or_building(e, k);
        let before = p.clone();

        let mut flags = [p.visible, p.active, p.draggable];
        ins.hint("Visible: the piece is drawn. Active and draggable are only kept, as in TEd.");
        if ins.edit_checks("flags", "flags", &["visible", "active", "draggable"], &mut flags) {
            (p.visible, p.active, p.draggable) = (flags[0], flags[1], flags[2]);
        }

        let modes = ["Landscape: Above", "Freeform"].map(str::to_owned);
        let mut idx = i32::from(p.freeform);
        ins.hint("Only kept: it changes nothing in the viewport, as in TEd.");
        if ins.edit_choice("surface_mode", "surface_mode", &mut idx, &modes) {
            p.freeform = idx == 1;
        }

        ins.hint("The piece's position in its building.");
        ins.edit_float3("position", "position", &mut p.position, 0.05);
        let mut deg = p.euler.map(f32::to_degrees);
        ins.hint("The piece's turn in its building, in degrees.");
        if ins.edit_float3("orientation", "orientation", &mut deg, 0.5) {
            p.euler = deg.map(f32::to_radians);
        }

        ins.hint("The piece's scale along x, y and z.");
        ins.edit_float3("scale", "scale", &mut p.scale, 0.01);
        ins.hint("How opaque the piece is drawn (0 to 1).");
        ins.edit_slider("opacity", "opacity", &mut p.opacity, 0.0, 1.0);
        ins.edit_bool("burn", "burn", &mut p.burn);
        ins.edit_slider("burn_amount", "burn_amount", &mut p.burn_amount, 0.0, 1.0);
        ins.edit_color4("bounds_colour", "bounds_colour", &mut p.bounds_colour);
        ins.hint("Draws the building's collision, even when the collision view is off.");
        ins.edit_bool("show_collision", "show_collision", &mut p.show_collision);
        ins.hint("Only kept: it changes nothing in the viewport, as in TEd.");
        ins.edit_checks("seasons", "seasons", &["Winter", "Spring", "Summer", "Autumn"], &mut p.seasons);
        let mut level = p.destruction_level.clamp(0, levels - 1);
        ins.hint("Which destruction level's model the piece shows.");
        if ins.edit_int_slider("destruction_level", "destruction_level", &mut level, 0, levels - 1) {
            p.destruction_level = level;
        }

        if p != before {
            let t = self.tile_mut();
            t.piece_props.insert((e, k), p);
            t.piece_pending.push((e, k));
        }
    }

    /// An element the property schema does not know: its attributes, read-only.
    pub(super) fn inspect_unknown(&mut self, e: usize, ins: &mut dyn Inspector) {
        let base = (u64::from(self.id()) << 40) ^ ((e as u64) << 8) ^ 0x31;
        if self.tile().is_instance(e) {
            self.position_rows(e, ins, base);
        }

        ins.section("Attributes");
        let sc = self.tile().tscene();
        for (i, a) in sc.doc().elements()[e].attrs.iter().enumerate() {
            ins.text(&format!("attr_{}", a.name), &a.name, &shown_raw(sc, e, i));
        }
    }

    /// The position, in the Inspector's coordinate system (`Inspector::local_coordinates`, the toolbar's Global / Local): relative to the
    /// parent as the file has it (scene units), or in the world in metres (TEd's X / Y / Z fields).
    pub(super) fn position_rows(&mut self, e: usize, ins: &mut dyn Inspector, base: u64) {
        let pos = self.tile().tscene().position(e);
        if ins.local_coordinates() {
            let Some(p) = pos else { return };
            let mut v = p.to_array().map(|c| c as f32);
            ins.hint("Relative to its parent, in the file's scene units. The toolbar's Global / Local (L) switches.");
            if ins.edit_float3("position", "position (local)", &mut v, 0.5) {
                self.set_local(e, Some(v.map(f64::from).into()), None, base);
            }
        } else {
            let t = self.tile();
            if pos.is_none() && !t.is_instance(e) {
                return;
            }

            let w = if t.is_instance(e) { t.world_position(e) } else { scene_metres(world_transform(t.tscene(), e).position, t.unit()) };
            let mut v = [w.x, w.y, w.z];
            ins.hint("In the world, in metres. The toolbar's Global / Local (L) switches.");
            if ins.edit_float3("position_world", "position (world, m)", &mut v, 0.05) {
                let p = Float3::new(v[0], v[1], v[2]);
                if self.tile().is_instance(e) {
                    self.set_world_metres(e, p, base + 2);
                } else {
                    // A layer or a group: its local position for that world one.
                    let pw = parent_transform(self.tile().tscene(), e);
                    let lp = pw.rotation.transpose() * (scene_point(p, self.tile().unit()) - pw.position);
                    self.set_local(e, Some(lp), None, base + 2);
                }
            }
        }

        if ins.gesture_ended() {
            self.history.break_merge();
        }
    }

    /// The orientation (degrees), in the Inspector's coordinate system: relative to the parent as the file has it, or in the world.
    pub(super) fn orientation_row(&mut self, e: usize, ins: &mut dyn Inspector, base: u64, editable: bool) {
        let Some(local) = self.tile().tscene().euler(e) else { return };
        let world = !ins.local_coordinates();
        let pw = parent_transform(self.tile().tscene(), e);
        let shown = if world { euler_from_matrix(&(pw.rotation * euler_matrix(local))) } else { local };
        let (field, label) = if world { ("euler_world", "orientation (world)") } else { ("euler", "orientation (local)") };
        let mut v = shown.to_array().map(degrees);
        if !editable {
            ins.text(field, label, &format!("{:.1}°  {:.1}°  {:.1}°", v[0], v[1], v[2]));
            return;
        }

        ins.hint(if world {
            "Its turn in the world, in degrees. The toolbar's Global / Local (L) switches."
        } else {
            "Its turn relative to its parent, in degrees. The toolbar's Global / Local (L) switches."
        });
        if ins.edit_float3(field, label, &mut v, 0.5) {
            let typed: Double3 = v.map(radians).into();
            let euler = if world { euler_from_matrix(&(pw.rotation.transpose() * euler_matrix(typed))) } else { typed };
            self.set_local(e, None, Some(euler), base + 1);
        }

        if ins.gesture_ended() {
            self.history.break_merge();
        }
    }

    /// One row of the Property Editor.
    pub(super) fn property_row(&mut self, e: usize, prop: &Prop, ins: &mut dyn Inspector) {
        let base = (u64::from(self.id()) << 40) ^ ((e as u64) << 8) ^ 0x31;
        let t = self.tile();
        let sc = t.tscene();
        let tag = sc.type_of(e).to_owned();
        let text = sc.get(e, prop.attr);

        // The rules that need to know more than the property: a building's category, a polygon's shape.
        let key = sc.get(e, "key").unwrap_or_default();
        let category = if tag == "BUILDING_INSTANCE" { t.building_category(&key) } else { None };
        let shape = sc.get(e, "polygon_shape");
        let capture_location = wce_tile::export_as_prop::capture_location_set(sc.get(e, "capture_location").as_deref());
        let ctx = RuleContext {
            category: category.as_deref(),
            shape: shape.as_deref(),
            present: text.is_some(),
            capture_location,
            export_as_prop: t.exports_as_prop(e),
        };
        let access = wce_tile::properties::access(prop, &ctx);
        if access == Access::Hidden {
            return;
        }

        // What a wall's row does (its tooltip).
        if tag == super::super::WALL_SPLINE
            && let Some(tip) = wce_tile::properties::wall_tip(prop.attr)
        {
            ins.hint(tip);
        }

        if tag == super::super::BUILDING_SPLINE
            && let Some(tip) = wce_tile::properties::building_spline_tip(prop.attr)
        {
            ins.hint(tip);
        }

        // A property the instance lacks is not in the file (TEd writes only some of them when they have their default), but every instance
        // has all of them in the grid.
        let cur = text.clone().unwrap_or_default();
        let editable = access == Access::Edit;
        let label = prop.label();
        let field = prop.attr;

        // The ones that have their own handling.
        match prop.kind {
            Kind::Flags => {
                let (mut vis, mut act, mut drag) = (
                    self.tile().own_flag(e, "OIF_VISIBLE"),
                    self.tile().own_flag(e, "OIF_ACTIVE"),
                    self.tile().own_flag(e, "OIF_DRAGGABLE"),
                );
                if self.tile().tscene().get(e, "flags").is_none() {
                    return;
                }

                let (shown, selectable) = (self.tile().shown(e), self.tile().selectable(e));
                let mut on = [vis, act, drag];
                if editable {
                    ins.hint(FLAGS_TIP);
                }

                if editable && ins.edit_checks("flags", "flags", &["visible", "active", "draggable"], &mut on) {
                    if on[0] != vis {
                        vis = on[0];
                        self.set_flag(e, "OIF_VISIBLE", vis);
                    }

                    if on[1] != act {
                        act = on[1];
                        self.set_flag(e, "OIF_ACTIVE", act);
                    }

                    if on[2] != drag {
                        drag = on[2];
                        self.set_flag(e, "OIF_DRAGGABLE", drag);
                    }
                }

                let _ = (vis, act, drag);
                if !shown {
                    ins.text("hidden", "", "hidden by a layer above");
                } else if !selectable {
                    ins.text("locked", "", "locked by a layer above");
                }

                return;
            }

            Kind::Placement => {
                if self.tile().is_instance(e) && self.tile().tscene().get(e, "surface_mode").is_some() {
                    // TEd's two modes and the editor's Entity: Above (freeform, marked: `project_files::SURFACE_ATTR`).
                    use wce_tile::project_files::{ROTATION_ATTR, SURFACE_ATTR};
                    let doc = self.tile().tscene().doc();
                    let on_entity = doc.get(e, SURFACE_ATTR).is_some_and(|v| v == "entity");
                    let modes = ["Landscape: Above", "Freeform", "Entity: Above"].map(str::to_owned);
                    let mut idx = if on_entity { 2 } else { i32::from(self.tile().placement(e) == Placement::Freeform) };
                    let before = idx;
                    let tips = SURFACE_TIPS.map(str::to_owned);
                    if ins.edit_choice_tips("surface_mode", "surface_mode", &mut idx, &modes, &tips) && editable && idx != before {
                        self.set_placement(&[e], if idx == 0 { Placement::Landscape } else { Placement::Freeform });
                        if idx == 2 {
                            self.set_attribute(e, SURFACE_ATTR, "entity");
                        } else if on_entity {
                            self.set_attribute(e, SURFACE_ATTR, "");
                        }
                    }

                    if idx == 2 {
                        ins.text("surface_note", "", "kept at its height; G puts it on what is under the mouse");
                    }

                    // The rotation: free (as given), following the surface under it, or upright on its slope (tilted to it now; a grab
                    // tilts it again).
                    let before = rotation_index(self.tile().tscene().get(e, ROTATION_ATTR).as_deref());
                    let mut r = before;
                    let (rotations, tips) = (ROTATION_MODES.map(str::to_owned), ROTATION_TIPS.map(str::to_owned));
                    if ins.edit_choice_tips("rotation_mode", "rotation", &mut r, &rotations, &tips) && editable && r != before {
                        self.set_attribute(e, ROTATION_ATTR, ROTATION_VALUES[r.clamp(0, 2) as usize]);
                        let p = self.tile().world_position(e);
                        let n = self.tile().terrain_normal(p.x, p.z);
                        self.history.break_merge();
                        self.follow_ground(e, n, base + 5);
                        self.history.break_merge();
                    }

                    let t = self.tile();
                    if t.on_the_terrain(e) {
                        // Shown on the terrain; its file's height may be another (a terrain edited after it was placed).
                        let w = t.world_position(e);
                        let dy = t.file_world_y(e) - t.landscape_y(e, w.x, w.z);
                        if dy.abs() > 0.005 {
                            ins.text(
                                "off_terrain",
                                "off the terrain",
                                &format!("file: {dy:+.3} m from the terrain (shown on it; Snap to terrain writes it)"),
                            );
                        }
                    }
                }

                return;
            }

            Kind::Position => {
                self.position_rows(e, ins, base);
                return;
            }

            Kind::Orientation => {
                self.orientation_row(e, ins, base, editable);
                return;
            }

            _ => {}
        }

        if prop.rule == wce_tile::properties::Rule::NoCaptureLocation && !editable {
            greyed_export_as_prop(prop, ins);
            return;
        }

        if prop.rule == wce_tile::properties::Rule::WhenExportedAsProp && !editable {
            ins.hint(wce_tile::export_as_prop::SCALE_TIP);
            ins.disabled_float3(field, label, parse_vec3_text(&cur).unwrap_or([1.0; 3]));
            return;
        }

        if !editable {
            ins.text(field, label, &cur);
            return;
        }

        let gesture = base ^ (attr_hash(prop.attr) << 4) ^ 0x7;
        let prop = &self.tile().game_choices(prop);
        // A road's fence type: `fence` or its road type's named fences.
        let prop =
            &if prop.attr == "road_fence_type" { Prop { kind: Kind::Enum(self.tile().road_fence_types(Some(e))), ..*prop } } else { *prop };
        let mut new = edit_value(prop, &cur, text.is_some(), i64::from(self.tile().destruction_levels(&key)), ins);
        if prop.kind == Kind::Text {
            // Typed text is applied when the typing ends (a key edit loads a model).
            let drafts_key = (e, prop.attr.to_owned());
            let mut shown = self.tile().drafts.get(&drafts_key).cloned().unwrap_or_else(|| cur.clone());
            if ins.edit_string(field, label, &mut shown) {
                self.tile_mut().drafts.insert(drafts_key.clone(), shown);
            }

            if ins.gesture_ended()
                && let Some(v) = self.tile_mut().drafts.remove(&drafts_key)
            {
                new = Some(v);
            }
        }

        if let Some(v) = new {
            self.set_property(e, prop, &v, gesture);
            // A road type chosen: the road at its width (a track's 3 m, dirt's 6), its pieces then drawn as they are, not stretched.
            if prop.attr == "road_type"
                && let Some(w) = self.tile().road_type_width(&v)
                && let Some(width) = wce_tile::properties::schema(self.tile().tscene().type_of(e))
                    .and_then(|s| s.iter().find(|p| p.attr == "road_width").copied())
            {
                self.set_property(e, &width, &wce_tile::ted_scene::format_number(w), gesture ^ 0x51);
            }
        }

        if ins.gesture_ended() {
            self.history.break_merge();
        }
    }

    /// Whether the Inspector shows a property of an element, and edits it (the rules that need its building's category or polygon's shape).
    pub(super) fn prop_access(&self, e: usize, prop: &Prop) -> Access {
        let t = self.tile();
        let doc = t.tscene().doc();
        let key = doc.get(e, "key").unwrap_or_default();
        let category = if t.tscene().type_of(e) == "BUILDING_INSTANCE" { t.building_category(&key) } else { None };
        let shape = doc.get(e, "polygon_shape");
        let capture_location = wce_tile::export_as_prop::capture_location_set(doc.get(e, "capture_location").as_deref());
        let ctx = RuleContext {
            category: category.as_deref(),
            shape: shape.as_deref(),
            present: doc.get(e, prop.attr).is_some(),
            capture_location,
            export_as_prop: t.exports_as_prop(e),
        };
        wce_tile::properties::access(prop, &ctx)
    }

    /// The Inspector with several elements selected: the properties all of them have (the same attribute of the same kind, shown for each),
    /// but their place (position, orientation, scale) and name; a value they do not share is marked, and shows the first one's. An edit sets
    /// it on all of them, one undo entry (a drag merges). False when fewer than two of the items are elements.
    pub(crate) fn inspect_elements(&mut self, items: &[SourceRef], ins: &mut dyn Inspector) -> bool {
        let n = self.tile().tscene().doc().elements().len();
        let mut elems = items
            .iter()
            .filter(|r| r.doc == self.id() && piece_of(r.item).is_none())
            .filter_map(|r| elem_of(r.item))
            .filter(|&e| e < n)
            .collect::<Vec<_>>();
        elems.dedup();
        if elems.len() < 2 {
            return false;
        }

        // What is selected, by type.
        let mut types: BTreeMap<String, usize> = BTreeMap::new();
        for &e in &elems {
            *types.entry(self.tile().tscene().type_of(e).to_owned()).or_default() += 1;
        }

        ins.section(&format!("{} items", elems.len()));
        ins.text("types", "types", &types.iter().map(|(t, n)| format!("{t} x{n}")).collect::<Vec<_>>().join(", "));

        // The properties shown for every one of them, in the first one's order.
        let excluded = |p: &Prop| matches!(p.kind, Kind::Position | Kind::Orientation) || p.attr == "scale" || p.attr == "name";
        let mut common: Option<Vec<(Prop, bool)>> = None;
        for &e in &elems {
            let Some(schema) = wce_tile::properties::schema(self.tile().tscene().type_of(e)) else {
                common = Some(vec![]);
                break;
            };

            let mine = schema
                .iter()
                .filter(|p| !excluded(p))
                .filter_map(|p| {
                    let access = self.prop_access(e, p);
                    (access != Access::Hidden).then_some((*p, access == Access::Edit))
                })
                .collect::<Vec<_>>();

            common = Some(match common {
                None => mine,
                Some(c) => c
                    .into_iter()
                    .filter_map(|(p, edit)| mine.iter().find(|(m, _)| m.attr == p.attr && m.kind == p.kind).map(|(_, e2)| (p, edit && *e2)))
                    .collect(),
            });
        }

        // Flags and placement need the attribute in every element (an added one is text the attribute edits cannot take back).
        let doc = self.tile().tscene().doc();
        let common = common
            .unwrap_or_default()
            .into_iter()
            .filter(|(p, _)| match p.kind {
                Kind::Flags => elems.iter().all(|&e| doc.get(e, "flags").is_some()),
                Kind::Placement => elems.iter().all(|&e| self.tile().is_instance(e) && doc.get(e, "surface_mode").is_some()),
                _ => true,
            })
            .collect::<Vec<_>>();

        if common.is_empty() {
            ins.text("none", "", "no property in common");
            return true;
        }

        // The values they do not share.
        const DIFFERS: &str = "differs between the selected items: the first one's is shown";
        let mut marks = vec![];
        for (p, _) in &common {
            let first = doc.get(elems[0], p.attr);
            if elems[1..].iter().any(|&e| doc.get(e, p.attr) != first) {
                marks.push((p.attr.to_owned(), DIFFERS.to_owned()));
            }

            // The rotation mode is a row of the placement's.
            let rotation = |e: usize| rotation_index(doc.get(e, wce_tile::project_files::ROTATION_ATTR).as_deref());
            if p.kind == Kind::Placement && elems[1..].iter().any(|&e| rotation(e) != rotation(elems[0])) {
                marks.push(("rotation_mode".to_owned(), DIFFERS.to_owned()));
            }
        }

        self.common_variation_rows(&elems, ins);
        ins.section("Common properties");
        ins.mark_fields(marks);
        let buildings = elems.iter().all(|&e| self.tile().tscene().type_of(e) == "BUILDING_INSTANCE");
        for (prop, editable) in common {
            self.common_row(&elems, &prop, editable, ins);
            if prop.attr == "burn" && buildings {
                self.common_fire_controls(&elems, ins);
            }
        }

        true
    }

    /// A row of [`Self::inspect_elements`].
    pub(super) fn common_row(&mut self, elems: &[usize], prop: &Prop, editable: bool, ins: &mut dyn Inspector) {
        let first = elems[0];
        let base = (u64::from(self.id()) << 40) ^ 0x5151_0000 ^ elems.len() as u64;
        let text = self.tile().tscene().get(first, prop.attr);
        let cur = text.clone().unwrap_or_default();
        match prop.kind {
            Kind::Flags => {
                let t = self.tile();
                let names = ["OIF_VISIBLE", "OIF_ACTIVE", "OIF_DRAGGABLE"];
                let before = names.map(|f| t.own_flag(first, f));
                let mut on = before;
                if editable {
                    ins.hint(FLAGS_TIP);
                }

                if editable && ins.edit_checks("flags", "flags", &["visible", "active", "draggable"], &mut on) {
                    for (i, flag) in names.iter().enumerate() {
                        if on[i] != before[i] {
                            self.set_flags(elems, flag, on[i]);
                        }
                    }
                }

                return;
            }

            Kind::Placement => {
                use wce_tile::project_files::SURFACE_ATTR;
                let doc = self.tile().tscene().doc();
                let on_entity = doc.get(first, SURFACE_ATTR).is_some_and(|v| v == "entity");
                let modes = ["Landscape: Above", "Freeform", "Entity: Above"].map(str::to_owned);
                let mut idx = if on_entity { 2 } else { i32::from(self.tile().placement(first) == Placement::Freeform) };
                let tips = SURFACE_TIPS.map(str::to_owned);
                if ins.edit_choice_tips("surface_mode", "surface_mode", &mut idx, &modes, &tips) && editable {
                    self.set_placement(elems, if idx == 0 { Placement::Landscape } else { Placement::Freeform });
                    self.history.break_merge();
                    self.set_attribute_all(elems, SURFACE_ATTR, if idx == 2 { "entity" } else { "" });
                }

                self.common_rotation_row(elems, editable, base, ins);
                return;
            }

            // Each season on its own: the others keep each element's.
            Kind::Seasons => {
                let present = text.is_some();
                let before = if present { parse_seasons(&cur) } else { [true; 4] };
                let mut on = before;
                if editable {
                    ins.hint("A tick changes only that season, on each selected item; the others keep their own.");
                }

                if editable && ins.edit_checks(prop.attr, prop.attr, &["Winter", "Spring", "Summer", "Autumn"], &mut on) {
                    let doc = self.tile().tscene().doc();
                    let values = elems
                        .iter()
                        .map(|&e| {
                            let mut s = doc.get(e, prop.attr).map_or([true; 4], |v| parse_seasons(&v));
                            for i in 0..4 {
                                if on[i] != before[i] {
                                    s[i] = on[i];
                                }
                            }

                            format_seasons(s)
                        })
                        .collect::<Vec<_>>();
                    self.set_property_each(elems, prop, &values, 0);
                } else if !editable {
                    ins.text(prop.attr, prop.attr, &cur);
                }

                return;
            }

            _ => {}
        }

        if prop.rule == wce_tile::properties::Rule::NoCaptureLocation && !editable {
            greyed_export_as_prop(prop, ins);
            return;
        }

        if !editable {
            ins.text(prop.attr, prop.attr, &cur);
            return;
        }

        let gesture = base ^ (attr_hash(prop.attr) << 4) ^ 0x9;
        let prop = &self.tile().game_choices(prop);
        let levels = elems
            .iter()
            .map(|&e| {
                let key = self.tile().tscene().get(e, "key").unwrap_or_default();
                i64::from(self.tile().destruction_levels(&key))
            })
            .min()
            .unwrap_or(1);
        let mut new = edit_value(prop, &cur, text.is_some(), levels, ins);
        if prop.kind == Kind::Text {
            // Typed text is applied when the typing ends (a key edit loads models).
            let drafts_key = (usize::MAX, prop.attr.to_owned());
            let mut shown = self.tile().drafts.get(&drafts_key).cloned().unwrap_or_else(|| cur.clone());
            if ins.edit_string(prop.attr, prop.attr, &mut shown) {
                self.tile_mut().drafts.insert(drafts_key.clone(), shown);
            }

            if ins.gesture_ended()
                && let Some(v) = self.tile_mut().drafts.remove(&drafts_key)
            {
                new = Some(v);
            }
        }

        if let Some(v) = new {
            self.set_property_each(elems, prop, &vec![v; elems.len()], gesture);
        }

        if ins.gesture_ended() {
            self.history.break_merge();
        }
    }

    /// The rotation mode of several instances (as [`Self::property_row`]'s): following the surface tilts each of them to the terrain under it,
    /// one undo entry, then marks them.
    fn common_rotation_row(&mut self, elems: &[usize], editable: bool, base: u64, ins: &mut dyn Inspector) {
        use wce_tile::project_files::ROTATION_ATTR;
        let mut r = rotation_index(self.tile().tscene().get(elems[0], ROTATION_ATTR).as_deref());
        let (rotations, tips) = (ROTATION_MODES.map(str::to_owned), ROTATION_TIPS.map(str::to_owned));
        if !(ins.edit_choice_tips("rotation_mode", "rotation", &mut r, &rotations, &tips) && editable) {
            return;
        }

        self.set_attribute_all(elems, ROTATION_ATTR, ROTATION_VALUES[r.clamp(0, 2) as usize]);
        self.history.break_merge();
        if r != 0 {
            // Every tilt in one entry, as the gizmo's moves of a selection.
            self.move_batch = Some(vec![]);
            for &e in elems {
                let p = self.tile().world_position(e);
                let n = self.tile().terrain_normal(p.x, p.z);
                self.follow_ground(e, n, base + 5);
            }

            let moves = self.move_batch.take().unwrap_or_default();
            if !moves.is_empty() {
                self.history.push(Box::new(super::super::commands::TedMovesCommand { gesture: 0, moves }));
            }

            self.history.break_merge();
        }
    }

    /// The burning preview of several buildings (as [`Self::fire_controls`]): play starts all of them together, stop stops them.
    fn common_fire_controls(&mut self, elems: &[usize], ins: &mut dyn Inspector) {
        let burning = elems.iter().filter(|&&e| self.tile().fire_time(e).is_some()).count();
        let state = if burning == 0 { "not burning".to_owned() } else { format!("{burning} of {} burning", elems.len()) };
        match ins.row_buttons(
            &[
                ("fire_play", "\u{e13c}", "Play the fire of every selected building", true),
                ("fire_stop", "\u{e167}", "Stop their fire", burning > 0),
            ],
            &state,
        ) {
            Some(0) => {
                let now = self.tile().fire_now();
                for &e in elems {
                    self.tile_mut().fire_start(e, now);
                }
            }

            Some(_) => {
                for &e in elems {
                    self.tile_mut().fire_stop(Some(e));
                }
            }

            None => {}
        }
    }

    /// Sets a property every one of the elements has and can edit, as the Inspector does with them selected (a script); how many changed.
    ///
    /// # Errors
    ///
    /// One of them does not have the property, or it is read-only for it.
    pub fn set_common(&mut self, elems: &[usize], attr: &str, value: &str) -> Result<usize, String> {
        let first = *elems.first().ok_or("nothing selected")?;
        let prop = wce_tile::properties::schema(self.tile().tscene().type_of(first))
            .and_then(|s| s.iter().find(|p| p.attr == attr))
            .copied()
            .ok_or_else(|| format!("{} has no property {attr}", self.tile().tscene().type_of(first)))?;
        for &e in elems {
            let has = wce_tile::properties::schema(self.tile().tscene().type_of(e))
                .is_some_and(|s| s.iter().any(|p| p.attr == attr && p.kind == prop.kind));
            if !has || self.prop_access(e, &prop) != Access::Edit {
                return Err(format!("element {e} ({}) cannot edit {attr}", self.tile().tscene().type_of(e)));
            }
        }

        let doc = self.tile().tscene().doc();
        let changed = elems.iter().filter(|&&e| doc.get(e, attr).as_deref() != Some(value)).count();
        self.set_property_each(elems, &prop, &vec![value.to_owned(); elems.len()], 0);
        Ok(changed)
    }

    /// Sets an attribute of several elements, one undo entry (a text splice; empty: as [`Self::set_attribute`] does with it).
    pub fn set_attribute_all(&mut self, elems: &[usize], attr: &str, value: &str) -> bool {
        let n = self.tile().tscene().doc().elements().len();
        let todo =
            elems.iter().copied().filter(|&e| e < n && self.tile().tscene().get(e, attr).as_deref() != Some(value)).collect::<Vec<_>>();
        if todo.is_empty() {
            return true;
        }

        let Ok(mark) = self.begin() else { return false };
        for &e in &todo {
            self.tscene_mut().set_attr(e, attr, value);
        }

        let items = todo.iter().map(|&e| item_of(e)).collect::<Vec<_>>();
        self.push_splice_attrs(format!("Set {attr} of {} items", todo.len()), mark, items.clone(), items, todo, &[attr]);
        true
    }

    /// Sets a property (and its twin) of several elements, each to its value: one undo entry, the steps of a drag (`gesture`) merge. Elements
    /// that lack the attribute get it first, by one text splice.
    pub fn set_property_each(&mut self, elems: &[usize], prop: &Prop, values: &[String], gesture: u64) -> bool {
        let attrs = std::iter::once(prop.attr).chain(prop.twin).collect::<Vec<_>>();
        let n = self.tile().tscene().doc().elements().len();
        let pairs = elems.iter().copied().zip(values.iter().cloned()).filter(|(e, _)| *e < n).collect::<Vec<_>>();

        // The attributes missing (an empty optional text stays out of the file, as `set_property` keeps it).
        let doc = self.tile().tscene().doc();
        let missing = pairs
            .iter()
            .flat_map(|(e, v)| {
                attrs
                    .iter()
                    .filter(move |a| doc.get(*e, a).is_none() && !(v.is_empty() && prop.kind == Kind::Text))
                    .map(move |a| (*e, *a, v.clone()))
            })
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            let Ok(mark) = self.begin() else { return false };
            for (e, a, v) in &missing {
                self.tscene_mut().set_attr(*e, a, v);
            }

            let items = missing.iter().map(|(e, _, _)| item_of(*e)).collect::<Vec<_>>();
            let mut touched = missing.iter().map(|(e, _, _)| *e).collect::<Vec<_>>();
            touched.dedup();
            self.push_splice_attrs(
                format!("Set {} of {} items", prop.attr, pairs.len()),
                mark,
                items.clone(),
                items,
                touched,
                &[prop.attr],
            );
            self.history.break_merge();
        }

        let doc = self.tile().tscene().doc();
        let edits = pairs
            .iter()
            .filter_map(|(e, v)| {
                let tag = self.tile().tscene().type_of(*e);
                let steps = attrs
                    .iter()
                    .filter_map(|a| doc.get(*e, a).filter(|old| old != v).map(|old| ((*a).to_owned(), old, v.clone())))
                    .collect::<Vec<_>>();
                (!steps.is_empty()).then(|| TedAttrCommand {
                    name: String::new(),
                    elem: *e,
                    steps,
                    gesture,
                    rebuild: tag == "BUILDING_INSTANCE" && prop.attr == "key",
                })
            })
            .collect::<Vec<_>>();

        if edits.is_empty() {
            return true;
        }

        let name = format!("Set {} of {} items", prop.attr, edits.len());
        self.history.push(Box::new(TedAttrsCommand { name, edits, gesture }));
        true
    }

    /// Moves a civilian shelter's target to a world point (TEd drags it over the ground), as the Inspector's `target` row writes it; the
    /// steps of a drag (one `gesture`) merge into one undo entry.
    pub fn set_shelter_target(&mut self, elem: usize, at: Float3, gesture: u64) -> bool {
        if self.tile().tscene().type_of(elem) != "CivilianShelter" {
            return false;
        }

        let Some(prop) = wce_tile::properties::schema("CivilianShelter").and_then(|s| s.iter().find(|p| p.attr == "target")) else {
            return false;
        };
        let pw = parent_transform(self.tile().tscene(), elem);
        let v = pw.rotation.transpose() * (scene_point(at, self.tile().unit()) - pw.position);
        self.set_property(elem, prop, &wce_tile::properties::format_vec3([v.x as f32, v.y as f32, v.z as f32]), gesture)
    }

    /// Sets a point light's radius as the Inspector's slider does (clamped to its range, `[0, 1000]`); the steps of a drag (one `gesture`)
    /// merge into one undo entry. False for anything but a point light.
    pub fn set_light_radius(&mut self, elem: usize, radius: f32, gesture: u64) -> bool {
        if self.tile().tscene().type_of(elem) != "POINT_LIGHT_INSTANCE" {
            return false;
        }

        let Some(prop) = wce_tile::properties::schema("POINT_LIGHT_INSTANCE").and_then(|s| s.iter().find(|p| p.attr == "radius")) else {
            return false;
        };
        let (lo, hi) = match prop.kind {
            Kind::Slider(lo, hi) => (lo, hi),
            _ => (0.0, f32::MAX),
        };

        // A hundredth of a metre is as fine as a drag goes.
        let r = ((radius.clamp(lo, hi) * 100.0).round() / 100.0).clamp(lo, hi);
        self.set_property(elem, prop, &wce_tile::properties::format_float(r), gesture)
    }

    /// Sets a property's attribute (and its twin): one undo entry, the steps of a drag merge. An attribute the element lacks is added by a text
    /// splice (that entry does not merge).
    pub fn set_property(&mut self, elem: usize, prop: &Prop, value: &str, gesture: u64) -> bool {
        let t = self.tile();
        if elem >= t.tscene().doc().elements().len() {
            return false;
        }

        let sc = t.tscene();
        let tag = sc.type_of(elem).to_owned();
        let mut steps = vec![];
        let mut missing = vec![];
        for attr in std::iter::once(prop.attr).chain(prop.twin) {
            match sc.get(elem, attr) {
                Some(old) if old == value => {}
                Some(old) => steps.push((attr.to_owned(), old, value.to_owned())),
                None => missing.push(attr),
            }
        }

        // An empty optional text that is not in the file stays out of it (TEd writes `capture_location` only when it has one).
        missing.retain(|_| !(value.is_empty() && matches!(prop.kind, Kind::Text)));
        let n = self.tile().tscene().name(elem);
        let name = format!("Set {} of {}", prop.attr, if n.is_empty() { tag.clone() } else { n });

        // One the file lacks (a twin): added, with the others, in one edit, so one undo takes the whole value back (two entries left the
        // pair half set). Once it is there, the next steps of the gesture are attribute edits that merge.
        if !missing.is_empty() {
            let Ok(mark) = self.begin() else { return false };
            for attr in missing.iter().copied().chain(steps.iter().map(|(a, ..)| a.as_str())) {
                self.tscene_mut().set_attr(elem, attr, value);
            }

            let attrs = missing.iter().copied().chain(steps.iter().map(|(a, ..)| a.as_str())).collect::<Vec<_>>();
            self.push_splice_attrs(name, mark, vec![item_of(elem)], vec![item_of(elem)], vec![elem], &attrs);
            return true;
        }

        if steps.is_empty() {
            return true;
        }

        let rebuild = tag == "BUILDING_INSTANCE" && prop.attr == "key";
        self.history.push(Box::new(TedAttrCommand { name, elem, steps, gesture, rebuild }));
        true
    }

    pub(super) fn inspect_terrain(&self, ins: &mut dyn Inspector) {
        let t = self.tile();
        let p = &t.project;
        ins.section("Terrain");
        match &p.height {
            Some(h) => {
                ins.text("height_map", "height map", &format!("{} x {} px", h.width(), h.height()));
                let found = if p.info.found { "" } else { "  [tile database entry not found]" };
                let range = format!(
                    "{:.2} .. {:.2} m (min {:.3} scale {:.3}){found}",
                    h.min_height(),
                    h.min_height() + h.scale(),
                    p.info.min_height,
                    p.info.scale
                );
                ins.text("range", "heights", &range);
                ins.text("scale", "resolution", &format!("{:.3} m/px, border {:.0} m", p.mpp_x(), p.border_m()));
            }

            None => ins.text("height_map", "height map", "none (no height_map_0.png)"),
        }

        let tile = format!("{} x {} cells, {} x {} m", p.info.cells_x, p.info.cells_y, t.extent_x() as i32, t.extent_z() as i32);
        ins.text("tile", "tile", &tile);
        ins.text("ted", "tile.ted", &format!("version {}, {} instances", t.tscene().project_version(), t.instances.len()));
    }

    /// The gizmo: moves (and turns) the items by a world-space delta.
    /// The `scale` property of an element the Scale gizmo works on: every type that has one, while the Inspector can edit it (a building only
    /// as a prop).
    /// An item the Scale gizmo (and Ctrl + Alt + drag) changes: an element, not a building's piece, whose type has a scale it may edit.
    pub fn item_scalable(&self, item: SourceRef) -> bool {
        item.doc == self.id() && piece_of(item.item).is_none() && elem_of(item.item).is_some_and(|e| self.scale_prop(e).is_some())
    }

    pub(super) fn scale_prop(&self, e: usize) -> Option<Prop> {
        let schema = wce_tile::properties::schema(self.tile().tscene().type_of(e))?;
        let prop = *schema.iter().find(|p| p.attr == "scale")?;
        (self.prop_access(e, &prop) == Access::Edit).then_some(prop)
    }

    /// The Scale gizmo on the selected items that have a scale, one undo entry: each from the scale it had when the drag began (`factor` is
    /// the scale since then). A vector takes the axes (a decal's negative z stays negative); a single number takes the axis that moved most,
    /// within its range.
    pub(crate) fn scale_selected(&mut self, items: &[SourceRef], factor: [f32; 3], gesture: u64) {
        if self.tile().scale_drag.as_ref().is_none_or(|(g, _)| *g != gesture) {
            let t = self.tile();
            let base = items
                .iter()
                .filter(|r| r.doc == self.id() && piece_of(r.item).is_none())
                .filter_map(|r| elem_of(r.item))
                .filter(|&e| self.scale_prop(e).is_some())
                .map(|e| {
                    let s = t.tscene().scale(e).map_or([1.0; 3], |s| [s.x as f32, s.y as f32, s.z as f32]);
                    (e, s)
                })
                .collect();
            self.tile_mut().scale_drag = Some((gesture, base));
        }

        let Some((_, base)) = self.tile().scale_drag.clone() else { return };
        let uniform = factor.into_iter().max_by(|a, b| (a - 1.0).abs().total_cmp(&(b - 1.0).abs())).unwrap_or(1.0);
        let scaled = base
            .iter()
            .filter_map(|(&e, s)| {
                let prop = self.scale_prop(e)?;
                let value = match prop.kind {
                    Kind::Vec3 => format_vec3([s[0] * factor[0], s[1] * factor[1], s[2] * factor[2]]),
                    Kind::Slider(lo, hi) => format_float((s[0] * uniform).clamp(lo, hi)),
                    _ => format_float(s[0] * uniform),
                };

                Some((e, prop, value))
            })
            .collect::<Vec<_>>();

        // One attribute: the types' props differ only in how the value is written, done above.
        let Some(&(_, prop, _)) = scaled.first() else { return };
        let elems = scaled.iter().map(|s| s.0).collect::<Vec<_>>();
        let values = scaled.into_iter().map(|s| s.2).collect::<Vec<_>>();
        self.set_property_each(&elems, &prop, &values, gesture);
    }

    pub(crate) fn transform_selected(&mut self, items: &[SourceRef], delta: &Float4x4, gesture: u64) {
        // An item inside a selected group moves with the group, not twice.
        let mine = items.iter().filter(|r| r.doc == self.id()).filter_map(|r| elem_of(r.item)).collect::<std::collections::HashSet<_>>();
        let els = self.tile().tscene().doc().elements();
        let inside = |e: usize| std::iter::successors(els[e].parent, |&p| els[p].parent).any(|p| mine.contains(&p));
        let items = items.iter().copied().filter(|r| elem_of(r.item).is_none_or(|e| e >= els.len() || !inside(e))).collect::<Vec<_>>();

        // Every item's move in one entry, merging with the drag's next step: one entry per item and step, the keys taking turns, never
        // merged (a two-second drag of three items left about 360 entries).
        self.move_batch = Some(vec![]);
        for r in items {
            let t = self.tile();
            let Some(before) = t.item_transform(r).filter(|_| r.doc == self.id()) else { continue };
            let Some(e) = elem_of(r.item) else { continue };
            let now = *delta * before;
            let pw = parent_transform(t.tscene(), e);
            let u = t.unit();

            // The new world position in scene units, local to the parent.
            let mut wp = Double3::new(f64::from(now.m[0][3]) * u, f64::from(now.m[1][3]) * u, f64::from(now.m[2][3]) * u);

            // As in TEd: x / z move a landscape instance over the terrain, y is the terrain's (a y handle does nothing). The float32 noise of a
            // y-axis drag at 1000 m is 6e-5.
            if t.placement(e) == Placement::Landscape {
                let same_xz = (now.m[0][3] - before.m[0][3]).abs() < 2e-4 && (now.m[2][3] - before.m[2][3]).abs() < 2e-4;
                wp.y = u * if same_xz { f64::from(before.m[1][3]) } else { f64::from(t.landscape_y(e, now.m[0][3], now.m[2][3])) };
            }

            let lp = pw.rotation.transpose() * (wp - pw.position);
            let (wr, br) = (rotation_of(&now), rotation_of(&before));
            let dr = wr.data().iter().zip(br.data()).fold(0.0f64, |m, (a, b)| m.max((a - b).abs()));
            let le = (dr > 1e-5).then(|| euler_from_matrix(&(pw.rotation.transpose() * wr)));

            // A y drag on a landscape instance: nothing changed.
            if le.is_none() && t.tscene().position(e).is_some_and(|cur| close(cur, lp)) {
                continue;
            }

            self.set_local(e, Some(lp), le, gesture);
        }

        let moves = self.move_batch.take().unwrap_or_default();
        if !moves.is_empty() {
            self.history.push(Box::new(super::super::commands::TedMovesCommand { gesture, moves }));
        }
    }
}

/// An attribute's text as the Inspector shows it: the file's (still entity-encoded), or its display variation's own value of a combined
/// scene.
fn shown_raw(sc: &wce_tile::ted_scene::TedScene, e: usize, i: usize) -> String {
    let name = &sc.doc().elements()[e].attrs[i].name;
    sc.display_variation(e)
        .and_then(|v| wce_tile::variations::own_value(sc.doc(), e, v, name))
        .unwrap_or_else(|| String::from_utf8_lossy(sc.doc().attr_raw(e, i)).into_owned())
}

/// The Inspector's rotation modes: their names, tooltips and `wce_rotation` values.
const ROTATION_MODES: [&str; 3] = ["Free", "Follow surface", "Follow slope"];
const ROTATION_TIPS: [&str; 3] = [
    "Turned as given.",
    "Tilted to the surface under it, now and again after each grab.",
    "Upright: pitched to the slope along its length only, never leaning to a side, as the building spline lays a building without Tilt \
     across slope (a fence on a hillside). Now and again after each grab.",
];
const ROTATION_VALUES: [&str; 3] = ["", "surface", "slope"];

/// The rotation mode of a `wce_rotation` value (none or another: Free).
fn rotation_index(value: Option<&str>) -> i32 {
    match value {
        Some("surface") => 1,
        Some("slope") => 2,
        _ => 0,
    }
}
