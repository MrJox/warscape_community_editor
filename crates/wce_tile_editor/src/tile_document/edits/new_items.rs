//! New instances of any type (`wce_tile::new_instances`, with the kit's defaults).

use super::*;

impl TileDocument {
    // ---- new instances of any type (wce_tile::new_instances, with the kit's defaults)

    /// Makes a new instance of `kind` in the layer `into`: a point type (`Create::Point`, on the ground at a world point) or a drawn type
    /// (`Create::Line`, world points on the ground). `key` is the model, decal, tree or effect where the type has one. The types made of
    /// several elements are made whole: a capture location with its 20 m boundary circle, a siege AI node with its 20 m square Bounding Box
    /// and a 24 m Boundary, a deployment zone group with a 64 m square polygon (a drawn deployment polygon goes into the selected group,
    /// `into`, or into a new group), a siege AI edge with its drawn Street and, a street edge, a Street boundary 8 m to each side. A siege AI
    /// shape or a capture location boundary goes only into its holder (`into`), in a role of it (`shape_role`). One undo entry; the
    /// selection afterwards is it (a holder's shape: the holder).
    ///
    /// # Errors
    ///
    /// A type that can't be made this way, too few points, or no such layer.
    pub fn create(&mut self, kind: &str, into: usize, shape: &Create, key: &str) -> Result<usize, String> {
        self.create_with(kind, into, shape, key, &BTreeMap::new())
    }

    /// [`Self::create`] with these attributes over the type's defaults (a draft's values, `Draft::attrs`).
    ///
    /// # Errors
    ///
    /// As `create`.
    pub fn create_with(
        &mut self,
        kind: &str,
        into: usize,
        shape: &Create,
        key: &str,
        attrs: &BTreeMap<String, String>,
    ) -> Result<usize, String> {
        self.check_structure(&[], Some(into))?;
        use wce_tile::new_instances::{NewInstance, NewPoint, element_text};

        let t = self.tile();
        let unit = t.unit();
        let local = |p: Float3| t.to_local(into, Float3::new(p.x, p.y, p.z));

        // A drawn shape: its centre (the element's position) and the points around it.
        let line = |pts: &[Float3]| -> Result<(Double3, Vec<Double3>), String> {
            if pts.len() < 2 {
                return Err("draw at least 2 points".to_owned());
            }

            let local = pts.iter().map(|&p| local(p)).collect::<Vec<_>>();
            let c = local.iter().fold(Double3::default(), |a, &p| a + p) * (1.0 / local.len() as f64);
            Ok((c, local.into_iter().map(|p| p - c).collect()))
        };

        let into_type = t.tscene().type_of(into).to_owned();
        let element = match (kind, shape) {
            (_, Create::Point(p)) => point_instance(kind, local(*p), key, unit)?,
            (
                "ZONE_POLYGON_INSTANCE"
                | "CivilianDeploymentZone"
                | "LiteBuildingZone"
                | "AI_HINT_POLYGON_INSTANCE"
                | "NOGO_REGION_POLYGON_INSTANCE"
                | "GO_REGION_POLYGON_INSTANCE"
                | "VegetationExclusionPolygonInstance",
                Create::Line(pts),
            ) => {
                let (c, points) = line(pts)?;
                new_polygon(kind, points, true).position(c)
            }

            // A shape of a siege AI node or edge or of a capture location: in the role named (a draft's `name`), else the first one the
            // holder lacks; it replaces the holder's one of that role when it has only one.
            ("SIEGE_AI_POLYGON_INSTANCE" | "CaptureLocationPolygonInstance", Create::Line(pts)) => {
                let role = self.shape_role(into, attrs.get("name").map(String::as_str))?;
                let (c, mut points) = line(pts)?;
                if role.name == "Bounding Box" {
                    points = bounding_rectangle(points);
                }

                let mut n = new_polygon(kind, points, role.closed).position(c).attr("name", role.name);
                if role.closed {
                    n = n.attr("polygon_closed", "1");
                }

                if kind == "CaptureLocationPolygonInstance" {
                    n = n.attr("polygon_shape", "PS_POLYGON");
                }

                n
            }

            (super::super::WALL_SPLINE, Create::Line(pts)) => {
                // A wall's ends go onto the fort buildings' ends near them.
                let mut pts = pts.clone();
                if let [first, .., last] = pts.as_mut_slice() {
                    *first = t.snap_wall_end(*first);
                    *last = t.snap_wall_end(*last);
                }

                let (c, points) = line(&pts)?;
                new_polygon(kind, points, false).position(c).attr(wce_tile::new_instances::WALL_ID, t.next_wall_id().to_string())
            }

            (super::super::ROAD_SPLINE, Create::Line(pts)) => {
                let (c, points) = line(pts)?;
                new_polygon(kind, points, false).position(c).attr("road_id", t.next_road_id().to_string())
            }

            (super::super::BUILDING_SPLINE, Create::Line(pts)) => {
                let (c, points) = line(pts)?;
                new_polygon(kind, points, false).position(c)
            }

            ("AISeparatorInstance", Create::Line(pts)) => {
                let (c, points) = line(pts)?;
                new_polygon(kind, points, false).position(c)
            }

            // A line: no area. An angle finder: an arm's end, the corner, the other arm's end.
            (wce_tile::new_instances::MEASURE_LINE | wce_tile::new_instances::ANGLE_FINDER, Create::Line(pts)) => {
                if kind == wce_tile::new_instances::ANGLE_FINDER && pts.len() != 3 {
                    return Err(ANGLE_POINTS.to_owned());
                }

                let (c, points) = line(pts)?;
                let mut n = new_polygon(kind, points, false).position(c);
                n.attrs.remove("area");
                n
            }

            ("DEPLOYMENT_ZONE_POLYGON_INSTANCE", Create::Line(pts)) => {
                let (c, points) = line(pts)?;
                if into_type == "DeploymentZonePolygonInstanceGroup" {
                    new_polygon(kind, points, true).position(c)
                } else {
                    // A new group around it (scripts: `add_polygon`; the editor draws regions in a zone), the polygon with the values asked
                    // for it (they were left out here before).
                    let mut g =
                        NewInstance::new("DeploymentZonePolygonInstanceGroup").position(c).attr("deployment_category", NEW_ZONE_CATEGORY);
                    let mut p = new_polygon(kind, points, true);
                    p.attrs.extend(attrs.iter().map(|(k, v)| (k.clone(), v.clone())));
                    g.children.push(element_text(&p)?);
                    g
                }
            }

            ("SiegeAIEdgeInstance", Create::Line(pts)) => {
                let (c, points) = line(pts)?;

                // TEd's shapes: the drawn line is the Street; a street edge has a Street boundary to each side of it, 8 m away (CA's
                // corridors are about 16 m wide).
                let mut n = NewInstance::new(kind).position(c);
                let ty = attrs.get("type").map_or("SAET_STREET", String::as_str);
                let sides = if ty == "SAET_STREET" { side_lines(&points, 8.0 * unit) } else { vec![] };
                n.children.push(element_text(&new_polygon("SIEGE_AI_POLYGON_INSTANCE", points, false).attr("name", "Street"))?);
                for side in sides {
                    n.children.push(element_text(&new_polygon("SIEGE_AI_POLYGON_INSTANCE", side, false).attr("name", "Street boundary"))?);
                }

                n
            }

            ("SplineRiverInstance" | "SEGMENTED_SPLINE_INSTANCE", Create::Line(pts)) => {
                // TEd's layout: the instance at the first point, the points from it.
                if pts.len() < 2 {
                    return Err("draw at least 2 points".to_owned());
                }

                // A river the editor draws runs under the ground it was drawn on (its bed cut round it: below).
                let river = kind == "SplineRiverInstance" && self.tile().rivers_keep_heights();
                let pts = pts
                    .iter()
                    .map(|&p| if river { Float3::new(p.x, p.y - super::super::river_beds::WATER_BELOW_BANK, p.z) } else { p })
                    .collect::<Vec<_>>();
                let at = local(pts[0]);
                let positions = pts.iter().map(|&p| local(p) - at).collect::<Vec<_>>();
                let mut n = NewInstance::new(kind).position(at).attr("lf_offset", wce_tile::ted_scene::format_number(at.y));
                // A river the editor draws keeps its real heights, cuts its bed and owns its layer for it (docs/plans/rivers.md, step 2).
                if river {
                    n = n
                        .attr(wce_tile::river_files::RIVER_HEIGHT_ATTR, "game")
                        .attr(super::super::river_beds::BED_ATTR, "1")
                        .attr(super::super::river_links::RIVER_ID_ATTR, &self.tile().next_river_id().to_string());
                }

                n.points = wce_tile::splines::new_points(&positions)
                    .into_iter()
                    .map(|p| {
                        let mut q = NewPoint::at(p.position);
                        q.attrs.insert("controlIn".to_owned(), wce_tile::new_instances::point_text(p.control_in));
                        q.attrs.insert("controlOut".to_owned(), wce_tile::new_instances::point_text(p.control_out));
                        q
                    })
                    .collect();
                n
            }

            (_, Create::Line(_)) => return Err(format!("{kind} is placed, not drawn")),
        };

        let mut element = element;
        if element.kind == kind {
            element.attrs.extend(attrs.iter().map(|(k, v)| (k.clone(), v.clone())));
        }

        self.check_fits([element.kind.as_str()], into)?;

        // A holder's shape: in its role's place among the holder's shapes (TEd's order), in place of the one it replaces.
        let placed = if wce_tile::parents::holders(&element.kind).is_empty() { None } else { Some(self.shape_place(into, &element)) };
        let text = element_text(&element)?;
        let mark = self.begin()?;
        let inserted = self.tscene_mut().insert_element_text(into, &text).and_then(|e| match placed {
            Some((before, replaced)) => self.put_shape(e, into, before, replaced),
            None => Ok(e),
        });

        // A holder's new shape leaves the holder selected (its shapes are picked only to be edited by hand); it is before the shape,
        // so the insert did not move it.
        let holder = placed.is_some() && wce_tile::parents::child_kind(self.tile().tscene().type_of(into)).is_some();
        let selected = |e: usize| vec![item_of(if holder { into } else { e })];
        match inserted {
            Ok(e) if placed.is_some_and(|p| p != (None, None)) => {
                self.push_splice(format!("Add {kind}"), mark, vec![], selected(e));
                Ok(e)
            }

            Ok(e) => {
                self.push_splice_added(format!("Add {kind}"), mark, vec![], selected(e), &[e]);
                Ok(e)
            }

            Err(err) => self.rolled_back(mark, err),
        }
    }

    /// The text of what a click of the Create section places (a type placed by a click, at the origin), for the preview.
    pub fn point_instance_text(&self, kind: &str, key: &str) -> Option<Vec<u8>> {
        let n = point_instance(kind, Double3::ZERO, key, self.tile().unit()).ok()?;
        wce_tile::new_instances::element_text(&n).ok()
    }
}
