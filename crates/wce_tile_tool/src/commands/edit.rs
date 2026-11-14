//! Selecting, moving, creating, copying and measuring the tile's items.

use super::*;

pub(super) fn register(c: &mut CommandRegistry<Editor>) {
    c.add("scene_filter", "scene_filter [text]", "filter the Scene panel's tree by name or type (no text: clear it)", |e, a| {
        let text = a.join(" ");
        with_tile(e, |t, _| {
            t.tree.filter = text;
            ok("")
        })
    });

    c.add(
        "tile_mode",
        "tile_mode select|raise|lower|smooth|flatten|level|paint|ground",
        "what the left mouse button does in the viewport",
        |e, a| {
            let Some(name) = a.first() else { return fail("usage: tile_mode select|raise|lower|smooth|flatten|level|paint|ground") };
            let Some(i) = MODE_NAMES.iter().position(|m| m == name) else { return fail(format!("unknown mode '{name}'")) };
            with_tile(e, |t, _| {
                t.set_mode(TileMode::from_index(i));
                ok("")
            })
        },
    );

    c.add(
        "tile_select",
        "tile_select <type | element index | none> [add]",
        "select the first instance of a type, or an element by index; `none` clears the selection",
        |e, a| {
            let Some(d) = document(e) else { return no_tile() };
            let Some(what) = a.first() else { return fail("usage: tile_select <type | element index | none> [add]") };
            if what == "none" {
                select_elem(e, None, false);
                return ok("nothing selected");
            }

            let add = a.get(1).is_some_and(|x| x == "add");
            let found = match what.parse::<i64>() {
                Ok(i) => usize::try_from(i).ok(),
                Err(_) => d.tile().instances().iter().copied().find(|&el| d.tile().tscene().type_of(el) == what),
            };

            let Some(found) = found.filter(|&f| f < d.tile().tscene().doc().elements().len()) else {
                return fail(format!("nothing found for '{what}'"));
            };
            select_elem(e, Some(found), add);
            ok(format!("element {found}"))
        },
    );

    c.add(
        "tile_select_all",
        "tile_select_all <type> [key words]",
        "select every instance of a type (BUILDING_INSTANCE, ...) whose key holds the words (and-ed; none: all of them)",
        |e, a| {
            let Some(id) = crate::tool::document_id(e) else { return no_tile() };
            let Some(d) = document(e) else { return no_tile() };
            let Some(ty) = a.first() else { return fail("usage: tile_select_all <type> [key words]") };
            let words = a[1..].iter().map(|w| w.to_lowercase()).collect::<Vec<_>>();
            let sc = d.tile().tscene();
            let key = |el: usize| {
                sc.doc()
                    .find_attr(el, "key")
                    .map(|at| String::from_utf8_lossy(sc.doc().attr_raw(el, at)).to_lowercase())
                    .unwrap_or_default()
            };
            let found = d
                .tile()
                .instances()
                .iter()
                .copied()
                .filter(|&el| sc.type_of(el) == ty && words.iter().all(|w| key(el).contains(w)))
                .collect::<Vec<_>>();
            e.selection.clear();
            for &el in &found {
                e.selection.add(wce_scene::render_scene::SourceRef { doc: id, item: wce_tile_editor::tile_document::item_of(el) });
            }

            ok(format!("{} selected", found.len()))
        },
    );

    c.add(
        "tile_move",
        "tile_move <dx m> <dz m>",
        "move the selected instance over the ground (keeps its height above the terrain), or the selected group",
        |e, a| {
            if primary(e).is_none()
                && let Some(g) = selected_group_or_layer(e)
            {
                let parsed = (a.len() >= 2).then(|| Some((number(&a[0])?, number(&a[1])?))).flatten();
                let Some((dx, dz)) = parsed else { return fail("usage: tile_move <dx m> <dz m>") };
                let Some(d) = document(e) else { return no_tile() };
                let mut w = world_transform(d.tile().tscene(), g);
                let u = d.tile().unit();
                w.position.x += f64::from(dx) * u;
                w.position.z += f64::from(dz) * u;
                return if d.set_world_transform(g, &w, 0) { ok("") } else { fail("cannot move it") };
            }

            let el = primary(e);
            let Some(d) = document(e) else { return no_tile() };
            let parsed = (a.len() >= 2).then(|| Some((number(&a[0])?, number(&a[1])?))).flatten();
            let Some((dx, dz)) = parsed else { return fail("usage: tile_move <dx m> <dz m>") };
            let Some(el) = el else { return fail("nothing selected") };
            let t = d.tile();
            let mut p = t.world_position(el);
            let above = p.y - t.terrain_height(p.x, p.z);
            p.x += dx;
            p.z += dz;
            p.y = t.terrain_height(p.x, p.z) + above;
            if !t.is_instance(el) {
                return fail("cannot move this element");
            }

            d.set_world_metres(el, p, 0);
            ok("")
        },
    );

    c.add(
        "snap",
        "snap [grid on|off] [size <m>] [tolerance <m>] [vertices on|off] [edges on|off] [heights on|off] [angle <deg>]",
        "the snapping of the grab and the placing (the viewport's snap chip); no argument: log it",
        |e, a| {
            with_tile(e, |t, _| {
                let mut i = 0;
                while i + 1 < a.len() {
                    let on = a[i + 1] == "on";
                    match a[i].as_str() {
                        "grid" => t.snap_grid = on,
                        "vertices" => t.snap_vertices = on,
                        "edges" => t.snap_edges = on,
                        "heights" => t.snap_heights = on,
                        "angle" => t.angle_tolerance = number(&a[i + 1]).unwrap_or(t.angle_tolerance).clamp(0.0, 90.0),
                        "size" => t.grid_m = number(&a[i + 1]).unwrap_or(t.grid_m),
                        "tolerance" => t.snap_tolerance = number(&a[i + 1]).unwrap_or(t.snap_tolerance).max(0.0),
                        other => return fail(format!("unknown snap setting {other}")),
                    }

                    i += 2;
                }

                ok(format!(
                    "grid {} size {} tolerance {} vertices {} edges {} heights {} angle {}",
                    t.snap_grid, t.grid_m, t.snap_tolerance, t.snap_vertices, t.snap_edges, t.snap_heights, t.angle_tolerance,
                ))
            })
        },
    );

    c.add(
        "grab_to",
        "grab_to <x m> <z m>",
        "the G grab: put the selected instance on the ground at a point (the snapping applies)",
        |e, a| {
            with_doc(e, |t, e| {
                let parsed = (a.len() >= 2).then(|| Some((number(&a[0])?, number(&a[1])?))).flatten();
                let Some((x, z)) = parsed else { return fail("usage: grab_to <x m> <z m>") };
                if !t.begin_grab(e) {
                    return fail("nothing selected");
                }

                let Some((g, y_offset, gesture)) = t.grab().map(|g| (g.elem, g.y_offset, g.gesture)) else { return fail("no grab") };
                let d = document(e).expect("a tile");
                let (raw_x, raw_z) = (x, z);
                let p = t.snap_point(d, Float3::new(x, 0.0, z), Some(g));
                let (x, z) = (p.x, p.z);
                let y = d.tile().terrain_height(x, z) + y_offset as f32;
                let piece = if t.snap_vertices { d.tile().snap_piece(g, Float3::new(raw_x, y, raw_z), t.piece_reach()) } else { None };
                match piece {
                    Some(w) => d.set_world_transform(g, &w, gesture),
                    None => d.set_world_metres(g, Float3::new(x, y, z), gesture),
                };

                t.end_grab(e, true);
                ok("")
            })
        },
    );

    c.add("duplicate", "duplicate", "duplicate the selected instance (4 m to the side) and select the copy", |e, _| {
        let el = primary(e);
        let Some(d) = document(e) else { return no_tile() };
        let Some(el) = el else { return fail("nothing selected") };
        d.duplicate(el).map_or_else(fail, |_| ok("duplicated"))
    });

    c.add("delete_item", "delete_item", "delete the selected instance", |e, _| {
        let el = primary(e);
        let Some(d) = document(e) else { return no_tile() };
        if el.is_some_and(|el| d.remove(el)) { ok("deleted") } else { fail("nothing selected") }
    });

    c.add("rotate", "rotate <degrees>", "rotate the selected instance about the world Y axis", |e, a| {
        let el = primary(e);
        let Some(d) = document(e) else { return no_tile() };
        let Some(deg) = a.first().and_then(|x| number(x)) else { return fail("usage: rotate <degrees>") };
        if el.is_some_and(|el| d.rotate(el, deg)) {
            return ok("");
        }

        // A group or layer: about its own origin.
        let Some(g) = selected_group_or_layer(e) else { return fail("nothing selected") };
        let Some(d) = document(e) else { return no_tile() };
        let mut w = world_transform(d.tile().tscene(), g);
        w.rotation = wce_foundation::math::Double3x3::rotation_y(f64::from(deg).to_radians()) * w.rotation;
        if d.set_world_transform(g, &w, 0) { ok("") } else { fail("cannot turn it") }
    });

    c.add(
        "add_building", "add_building <key> [x m z m]",
        "add a building (a copy of the selected or the first one, with this key) in the working layer, on the ground at x z when given; selects it",
        |e, a| {
            let el = primary(e);
            let Some(key) = a.first().cloned() else { return fail("usage: add_building <key> [x m z m]") };
            with_tile(e, |t, e| {
                let Some(d) = document(e) else { return no_tile() };
                let at = (a.len() >= 3).then(|| Some((number(&a[1])?, number(&a[2])?))).flatten().map(|(x, z)| Float3::new(x, d.tile().terrain_height(x, z), z));
                let into = t.new_items_layer(d);
                match d.add_building_in(&key, el, at, Placement::Landscape, into) {
                    Ok(added) => {
                        select_elem(e, Some(added), false);
                        ok(format!("added {key} as element {added}"))
                    }
                    Err(err) => fail(err),
                }
            })
        },
    );

    c.add(
        "add_polygon",
        "add_polygon \"TYPE|HINT|x,z;x,z;...\"",
        "add a polygon (AI hint, zone, ...) from points in metres in the working layer; selects it",
        |e, a| {
            let el = primary(e);
            let Some(d) = document(e) else { return no_tile() };
            let Some(spec) = a.first() else { return fail("usage: add_polygon \"TYPE|HINT|x,z;x,z;...\"") };
            let parts = spec.split('|').collect::<Vec<_>>();
            if parts.len() < 3 {
                return fail("expected TYPE|HINT|x,z;x,z");
            }

            let pts = parts[2]
                .split(';')
                .filter_map(|item| {
                    let xz = item.split(',').collect::<Vec<_>>();
                    if xz.len() != 2 {
                        return None;
                    }

                    let (x, z) = (number(xz[0])?, number(xz[1])?);
                    Some(Float3::new(x, d.tile().terrain_height(x, z), z))
                })
                .collect::<Vec<_>>();
            let _ = el;
            let (kind, hint) = (parts[0].to_owned(), parts[1].to_owned());
            with_tile(e, |t, e| {
                let Some(d) = document(e) else { return no_tile() };
                let layer = t.new_items_layer(d);
                if crate::tool::POLYGON_TYPES.contains(&kind.as_str()) {
                    d.add_polygon(layer, &kind, &hint, &pts).map_or_else(fail, |_| ok(format!("added {kind}")))
                } else {
                    let Some(layer) = layer else { return fail("this tile has no scene graph") };
                    d.create(&kind, layer, &wce_tile_editor::tile_document::Create::Line(pts), "")
                        .map_or_else(fail, |_| ok(format!("added {kind}")))
                }
            })
        },
    );

    c.add(
        "create", "create <type> <x m> <z m> [key]",
        "place a new instance of a type (a light, an effect, a decal, a capture location, ...; see the Create section) on the ground at x z, in the working layer",
        |e, a| {
            let (Some(kind), Some(x), Some(z)) = (a.first().cloned(), a.get(1).and_then(|v| number(v)), a.get(2).and_then(|v| number(v))) else {
                return fail("usage: create <type> <x m> <z m> [key]");
            };
            let key = a.get(3).cloned().unwrap_or_default();
            with_tile(e, |t, e| {
                let Some(d) = document(e) else { return no_tile() };
                let Some(into) = t.new_items_layer(d) else { return fail("this tile has no scene graph") };
                let at = Float3::new(x, d.tile().terrain_height(x, z), z);
                d.create(&kind, into, &wce_tile_editor::tile_document::Create::Point(at), &key).map_or_else(fail, |_| ok(format!("added {kind}")))
            })
        },
    );

    c.add(
        "polygon_point", "polygon_point move|insert|remove|in|out|prepend <index> <x m> <z m>",
        "edit a point of the selected polygon or spline (insert: after the index; a spline's in / out: its handles, prepend: a new first point)", |e, a| {
        let el = primary(e);
        let Some(d) = document(e) else { return no_tile() };
        let Some(idx) = a.get(1).and_then(|x| number(x)).filter(|_| a.len() >= 2) else {
            return fail("usage: polygon_point move|insert|remove|in|out|prepend <index> <x m> <z m>");
        };

        let (x, z) = if a.len() >= 4 { (number(&a[2]).unwrap_or(0.0), number(&a[3]).unwrap_or(0.0)) } else { (0.0, 0.0) };
        let Some(el) = el else { return fail("nothing selected") };
        let Ok(idx) = usize::try_from(idx as i64) else { return fail("no such point") };
        let p = Float3::new(x, d.tile().terrain_height(x, z), z);
        d.polygon_point(el, &a[0], idx, p, 0).map_or_else(fail, |()| ok(""))
    });

    c.add(
        "spline_value",
        "spline_value <attribute> <value> [point index|all]",
        "a value of the selected river's or road's points (width, foam_amount, alpha_fade, flow_speed, terrain_offset)",
        |e, a| {
            let el = primary(e);
            let Some(d) = document(e) else { return no_tile() };
            let (Some(attr), Some(value)) = (a.first(), a.get(1).filter(|v| number(v).is_some())) else {
                return fail("usage: spline_value <attribute> <value> [point index|all]");
            };

            let Some(el) = el.filter(|&s| d.is_spline(s)) else { return fail("no river or road selected") };
            let n = d.tile().tscene().polygon_points(el).len();
            let points = match a.get(2).map(String::as_str) {
                None | Some("all") => (0..n).collect::<Vec<_>>(),
                Some(i) => match i.parse::<usize>() {
                    Ok(i) if i < n => vec![i],
                    _ => return fail("no such point"),
                },
            };

            if d.set_spline_value(el, &points, attr, value, 0) {
                ok(format!("{attr} = {value} on {} points", points.len()))
            } else {
                fail("nothing changed")
            }
        },
    );

    c.add(
        "tile_measure",
        "tile_measure",
        "log the selected tape measure's (WceMeasureLine) lengths: the whole line's and each stretch's",
        |e, _| {
            let el = primary(e);
            let Some(d) = document(e) else { return no_tile() };
            let Some(m) = el.and_then(|el| d.tile().measure_line(el)) else { return fail("no tape measure selected") };
            let mut out = crate::measure_tool::summary(&m).into_iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>();
            for (i, s) in m.stretches.iter().enumerate() {
                out.push(format!(
                    "{i}-{}: ground {:.3} straight {:.3} map {:.3} rise {:+.3} grade {:+.2} %",
                    i + 1,
                    s.ground,
                    s.straight,
                    s.horizontal,
                    s.rise,
                    s.grade_percent()
                ));
            }

            ok(out.join("\n"))
        },
    );

    c.add(
        "assert_measure",
        "assert_measure <ground m> <straight m> <map m> [tolerance m]",
        "test: the selected tape measure's total lengths along the ground, straight and on the map (tolerance 0.01 m)",
        |e, a| {
            let el = primary(e);
            let Some(d) = document(e) else { return no_tile() };
            let Some(m) = el.and_then(|el| d.tile().measure_line(el)) else { return fail("no tape measure selected") };
            let want = |i: usize| a.get(i).and_then(|v| number(v));
            let (Some(ground), Some(straight), Some(map)) = (want(0), want(1), want(2)) else {
                return fail("usage: assert_measure <ground> <straight> <map> [tolerance]");
            };
            let tol = f64::from(want(3).unwrap_or(0.01));
            let t = &m.total;
            let got = format!("ground {:.3}, straight {:.3}, map {:.3}", t.ground, t.straight, t.horizontal);
            let off = |a: f64, b: f32| (a - f64::from(b)).abs() > tol;
            if off(t.ground, ground) || off(t.straight, straight) || off(t.horizontal, map) { fail(got) } else { ok(got) }
        },
    );

    c.add("tile_angle", "tile_angle", "log the selected angle finder's (WceAngleFinder) angle and arms", |e, _| {
        let el = primary(e);
        let Some(d) = document(e) else { return no_tile() };
        let Some(a) = el.and_then(|el| d.tile().angle_finder(el)) else { return fail("no angle finder selected") };
        ok(crate::measure_tool::angle_summary(&a).into_iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join("\n"))
    });

    c.add(
        "assert_angle",
        "assert_angle <on the map deg> <in space deg> [tolerance deg]",
        "test: the selected angle finder's angles (tolerance 0.1 deg)",
        |e, a| {
            let el = primary(e);
            let Some(d) = document(e) else { return no_tile() };
            let Some(got) = el.and_then(|el| d.tile().angle_finder(el)) else { return fail("no angle finder selected") };
            let (Some(map), Some(space)) = (a.first().and_then(|v| number(v)), a.get(1).and_then(|v| number(v))) else {
                return fail("usage: assert_angle <map> <space> [tolerance]");
            };
            let tol = f64::from(a.get(2).and_then(|v| number(v)).unwrap_or(0.1));
            let text = format!("on the map {:.3}, in space {:.3}", got.on_map, got.in_space);
            if (got.on_map - f64::from(map)).abs() > tol || (got.in_space - f64::from(space)).abs() > tol { fail(text) } else { ok(text) }
        },
    );

    c.add("new_layer", "new_layer <name>", "a new layer under the scene graph, selected", |e, a| {
        let Some(d) = document(e) else { return no_tile() };
        let Some(name) = a.first() else { return fail("usage: new_layer <name>") };
        let graph = d.tile().tscene().scene_graph();
        d.add_layer(graph, name).map_or_else(fail, |_| ok(""))
    });

    c.add("move_to_layer", "move_to_layer <layer name>", "move the selected instance into the layer with this name", |e, a| {
        let el = primary(e);
        let Some(d) = document(e) else { return no_tile() };
        let Some(name) = a.first() else { return fail("usage: move_to_layer <layer name>") };
        let Some(el) = el else { return fail("nothing selected") };
        let sc = d.tile().tscene();
        let Some(layer) = (0..sc.doc().elements().len()).find(|&i| sc.is_layer(i) && sc.name(i) == *name) else {
            return fail(format!("no layer named '{name}'"));
        };

        d.move_to_layer(el, layer).map_or_else(fail, |_| ok(""))
    });

    c.add("tile_flip_mode", "tile_flip_mode", "enter Flip mode for the selection, the plane through its centre", |e, _| {
        with_tile(e, |t, e| t.begin_flip(e).map_or_else(fail, ok))
    });

    c.add("tile_flip_plane", "tile_flip_plane [<x m> <z m> <angle>]", "where Flip mode's plane is", |e, a| {
        if a.is_empty() {
            return with_tile(e, |t, _| {
                t.flip.map_or_else(
                    || fail("not in Flip mode"),
                    |f| ok(format!("plane {:.3} {:.3} {:.2}", f.plane.x, f.plane.z, f.plane.angle)),
                )
            });
        }

        let [x, z, angle] = [0, 1, 2].map(|i| a.get(i).and_then(|v| number(v)));
        let (Some(x), Some(z), Some(angle)) = (x, z, angle) else { return fail("usage: tile_flip_plane <x m> <z m> <angle>") };
        with_tile(e, |t, _| match &mut t.flip {
            Some(f) => {
                f.plane = wce_tile_editor::tile_document::FlipPlane { x, z, angle };
                ok("")
            }

            None => fail("not in Flip mode"),
        })
    });

    c.add("tile_flip", "tile_flip [copy]", "mirror the selection across Flip mode's plane, moved or as a copy", |e, a| {
        let copy = a.first().is_some_and(|w| w == "copy");
        with_tile(e, |t, e| {
            if let Some(f) = &mut t.flip {
                f.copy = copy;
            }

            t.do_flip(e).map_or_else(fail, ok)
        })
    });

    c.add(
        "tile_flip_tile",
        "tile_flip_tile x|z",
        "flip the whole tile about its middle along x or z: items, terrain, terrain layers and links (one undo entry)",
        |e, a| {
            let Some(axis) = a.first().and_then(|w| wce_tile_editor::tile_document::TileFlipAxis::named(w)) else {
                return fail("usage: tile_flip_tile x|z");
            };
            with_tile(e, |t, e| t.do_flip_tile(e, axis).map_or_else(fail, ok))
        },
    );

    c.add("place_turn", "place_turn <degrees>", "turn what is being placed, as R does by the rotate step", |e, a| {
        let Some(deg) = a.first().and_then(|v| number(v)) else { return fail("usage: place_turn <degrees>") };
        with_tile(e, |t, _| {
            t.turn_placing(deg);
            ok("")
        })
    });

    c.add(
        "create_type",
        "create_type [type]",
        "the Create panel's list: the items of a type (Buildings, Vegetation, Decals ...; none: the favourites and the recent ones)",
        |e, a| {
            let name = a.join(" ");
            with_tile(e, |t, _| {
                t.set_create_category((!name.is_empty()).then_some(name.as_str())).map_or_else(fail, |()| ok(String::new()))
            })
        },
    );

    c.add("create_view", "create_view list|grid", "the Create panel's view: a list, or a grid of pictures", |e, a| {
        let grid = match a.first().map(String::as_str) {
            Some("grid") => true,
            Some("list") => false,
            _ => return fail("usage: create_view list|grid"),
        };

        with_tile(e, |t, e| {
            t.set_create_grid(e, grid);
            t.create_focus_frames = 3;
            ok("")
        })
    });

    c.add(
        "create_filter",
        "create_filter [words]",
        "the Create panel's search (no words: clear it); its folders open while it has words",
        |e, a| {
            let words = a.join(" ");
            with_tile(e, |t, _| {
                t.set_create_filter(&words);
                ok(String::new())
            })
        },
    );

    c.add(
        "keys",
        "keys <type> [filter]",
        "the valid keys of a type in the open tile's files (the Create and Placement pickers' lists): the count and the first 20 matching",
        |e, a| {
            let Some(kind) = a.first().cloned() else { return fail("usage: keys <type> [filter]") };
            let filter = a.get(1).map(|f| f.to_lowercase()).unwrap_or_default();
            with_tile(e, |t, e| {
                let keys = t.catalogue(e, &kind);
                let hits = keys.iter().filter(|k| k.to_lowercase().contains(&filter)).collect::<Vec<_>>();
                ok(format!(
                    "{} keys, {} matching: {}",
                    keys.len(),
                    hits.len(),
                    hits.iter().take(20).map(|k| k.as_str()).collect::<Vec<_>>().join(", ")
                ))
            })
        },
    );

    c.add(
        "place_modes",
        "place_modes <freeform|landscape|entity> [none|surface]",
        "the placement chips: what new things stand on and how they are turned",
        |e, a| {
            use crate::settings::{RotationMode, SurfaceMode};
            let surface = match a.first().map(String::as_str) {
                Some("freeform") => SurfaceMode::Freeform,
                Some("landscape") => SurfaceMode::Landscape,
                Some("entity") => SurfaceMode::Entity,
                _ => return fail("usage: place_modes <freeform|landscape|entity> [none|surface]"),
            };

            let rotation = if a.get(1).map(String::as_str) == Some("surface") { RotationMode::Surface } else { RotationMode::None };
            with_tile(e, |t, e| {
                t.modes = Some((surface, rotation));
                e.update_tool_settings(crate::settings::SETTINGS_ID, |s: &mut crate::settings::TileSettings| {
                    s.place_surface = surface;
                    s.place_rotation = rotation;
                });
                ok("")
            })
        },
    );

    c.add(
        "draw_point",
        "draw_point <x m> <z m>",
        "add a point to the line being drawn, on the ground at x z, as a click there does",
        |e, a| {
            let (Some(x), Some(z)) = (a.first().and_then(|v| number(v)), a.get(1).and_then(|v| number(v))) else {
                return fail("usage: draw_point <x m> <z m>");
            };
            with_tile(e, |t, e| {
                if !t.drawing {
                    return fail("nothing is being drawn");
                }

                let Some(d) = document(e) else { return fail("no tile open") };
                t.draw_pts.push(Float3::new(x, d.tile().terrain_height(x, z), z));
                ok(format!("{} points", t.draw_pts.len()))
            })
        },
    );

    c.add(
        "draw_finish",
        "draw_finish",
        "finish the line being drawn, as Enter does (it goes where the drawing goes: its holder, the working layer)",
        |e, _| {
            with_tile(e, |t, e| {
                if !t.drawing {
                    return fail("nothing is being drawn");
                }

                t.finish_drawing(e);
                ok(t.status.clone())
            })
        },
    );

    c.add("draft_set", "draft_set <attribute> <value>", "set a value of what is being placed or drawn, as its Inspector does", |e, a| {
        with_tile(e, |t, _| {
            let ([attr, value], Some(d)) = (a, &mut t.draft) else {
                return fail("usage: draft_set <attribute> <value>, while placing or drawing");
            };
            d.attrs.insert(attr.clone(), value.clone());
            t.create.drafts.insert(d.kind.clone(), d.clone());
            ok("")
        })
    });

    c.add("holder_draw", "holder_draw [shape name]", "start drawing a shape of the selected holder (a deployment zone's region, a siege AI node's or edge's shape, a capture location's boundary), as its Inspector's buttons do", |e, a| {
        with_tile(e, |t, e| {
            let sel = selected(e);
            let ([holder], Some(d)) = (sel.as_slice(), document(e)) else { return fail("select one holder") };
            let holder = *holder;
            let items = crate::create_panel::shape_items(d, holder).into_iter().filter(|it| it.holder == holder).collect::<Vec<_>>();
            let name = a.join(" ");
            let Some(it) = items.iter().find(|it| name.is_empty() || it.role == Some(name.as_str())).or_else(|| items.first().filter(|_| name.is_empty())) else {
                return fail("not a holder, or no such shape");
            };

            t.start_shape(e, it.holder, it.kind, it.role);
            if t.drawing { ok("") } else { fail(t.status.clone()) }
        })
    });

    c.add("tree_pick", "tree_pick <element>", "select an element as a click on its Scene panel row does: what was placed or painted stops, a line being drawn is finished, the Inspector shows it", |e, a| {
        let Some(el) = a.first().and_then(|x| x.parse::<u64>().ok()) else { return fail("usage: tree_pick <element>") };
        with_tile(e, |t, e| {
            let Some(id) = crate::tool::document_id(e) else { return fail("no tile open") };
            let points = t.draw_pts.len();
            e.selection.set(wce_scene::render_scene::SourceRef { doc: id, item: wce_tile_editor::tile_document::item_of(el as usize) });
            t.leave_for_selection(e);
            let picked = e.selection.items().first().and_then(|r| wce_tile_editor::tile_document::elem_of(r.item)).unwrap_or(0);
            let status = t.status.clone();
            ok(format!("picked #{picked} ({status}; {points} points drawn); making {}, mode {:?}, focus {}", t.making.as_deref().unwrap_or("nothing"), t.mode(), e.pending_focus.as_deref().unwrap_or("-")))
        })
    });

    c.add(
        "start_making",
        "start_making <type | building | prefab> [key]",
        "start placing or drawing something, as a click in the Create panel does (the mouse places it)",
        |e, a| {
            let Some(what) = a.first().cloned() else { return fail("usage: start_making <type | building | prefab> [key]") };
            let key = a.get(1).cloned().unwrap_or_default();
            with_tile(e, |t, e| crate::create_panel::make_for_script(t, e, &what, &key).map_or_else(fail, |()| ok("")))
        },
    );

    c.add(
        "align",
        "align <x | z | height> <first | min | centre | max>",
        "line the selected instances up, as the selection menu's Align does",
        |e, a| {
            let all = selected(e);
            let axis = match a.first().map(String::as_str) {
                Some("x") => 0,
                Some("height") => 1,
                Some("z") => 2,
                _ => return fail("usage: align <x | z | height> <first | min | centre | max>"),
            };
            use wce_tile_editor::tile_document::AlignTo;
            let to = match a.get(1).map(String::as_str) {
                Some("first") => AlignTo::First,
                Some("min") => AlignTo::Min,
                Some("centre") => AlignTo::Centre,
                Some("max") => AlignTo::Max,
                _ => return fail("usage: align <x | z | height> <first | min | centre | max>"),
            };
            let Some(d) = document(e) else { return no_tile() };
            ok(format!("{} moved", d.align(&all, axis, to)))
        },
    );

    c.add(
        "distribute",
        "distribute <x | z | line>",
        "space the selected instances evenly, as the selection menu's Distribute does",
        |e, a| {
            let all = selected(e);
            let axis = match a.first().map(String::as_str) {
                Some("x") => Some(0),
                Some("z") => Some(2),
                Some("line") => None,
                _ => return fail("usage: distribute <x | z | line>"),
            };
            let Some(d) = document(e) else { return no_tile() };
            ok(format!("{} moved", d.distribute(&all, axis)))
        },
    );

    c.add("copy", "copy [cut]", "copy (or cut) the selection, as Ctrl+C / Ctrl+X", |e, a| {
        let cut = a.first().is_some_and(|x| x == "cut");
        with_tile(e, |t, e| {
            t.copy_selection(e, cut);
            ok(t.status.clone())
        })
    });

    c.add(
        "paste",
        "paste [<x m> <z m> | inplace]",
        "paste the copied items: the first at a ground point (the rest around it), or where they were",
        |e, a| {
            let at = (a.len() >= 2).then(|| Some((number(&a[0])?, number(&a[1])?))).flatten();
            with_tile(e, |t, e| {
                if let Some((x, z)) = at {
                    let d = document(e).expect("a tile");
                    t.terrain_hit = true;
                    t.snapped_cursor = Float3::new(x, d.tile().terrain_height(x, z), z);
                }

                if t.paste(e, at.is_some()) { ok(t.status.clone()) } else { fail(t.status.clone()) }
            })
        },
    );

    c.add(
        "select_by",
        "select_by <type | key | everything | invert>",
        "select as the selection menu's Select does: all of the primary's type or key, everything shown, or the inverse",
        |e, a| {
            let what = a.first().cloned().unwrap_or_default();
            with_tile(e, |t, e| {
                match what.as_str() {
                    "type" => t.select_like_primary(e, false),
                    "key" => t.select_like_primary(e, true),
                    "everything" => t.select_everything(e),
                    "invert" => t.invert_selection(e),
                    _ => return fail("usage: select_by <type | key | everything | invert>"),
                }

                ok(t.status.clone())
            })
        },
    );

    c.add("selection_save", "selection_save <name>", "save the selection under a name for the session", |e, a| {
        let Some(name) = a.first().cloned() else { return fail("usage: selection_save <name>") };
        with_tile(e, |t, e| if t.save_selection(e, &name) { ok("") } else { fail(t.status.clone()) })
    });

    c.add("selection_restore", "selection_restore <name>", "select a saved selection again", |e, a| {
        let Some(name) = a.first().cloned() else { return fail("usage: selection_restore <name>") };
        with_tile(e, |t, e| if t.restore_selection(e, &name) { ok(t.status.clone()) } else { fail(t.status.clone()) })
    });

    c.add(
        "box_select",
        "box_select <x0 m> <z0 m> <x1 m> <z1 m>",
        "B and a drag with the mouse from one ground point's pixel to another's (the box select)",
        |e, a| {
            let parsed = (a.len() >= 4).then(|| Some((number(&a[0])?, number(&a[1])?, number(&a[2])?, number(&a[3])?))).flatten();
            let Some((x0, z0, x1, z1)) = parsed else { return fail("usage: box_select <x0 m> <z0 m> <x1 m> <z1 m>") };
            let Some(d) = document(e) else { return no_tile() };
            let (p0, p1) = (Float3::new(x0, d.tile().terrain_height(x0, z0), z0), Float3::new(x1, d.tile().terrain_height(x1, z1), z1));
            let (Some((sx0, sy0)), Some((sx1, sy1))) = (e.viewport.project(p0), e.viewport.project(p1)) else { return fail("not in view") };
            with_tile(e, |t, _| {
                t.box_select = crate::selection_tools::BoxSelect { armed: true, ..Default::default() };
                ok("")
            });
            let args =
                ["drag".to_owned(), format!("{sx0:.1}"), format!("{sy0:.1}"), format!("{sx1:.1}"), format!("{sy1:.1}"), "6".to_owned()];
            mouse(e, &args, "")
        },
    );

    c.add("quick_add", "quick_add", "open the Create panel's quick-add popup (Space over the viewport)", |e, _| {
        with_tile(e, |t, _| {
            t.request_quick_add();
            ok("")
        })
    });

    c.add(
        "place_preview",
        "place_preview <type | building | prefab> <x m> <z m> [key] | place_preview off",
        "start placing (a Create type, a building or a prefab) and show its preview at x z as if the mouse were there",
        |e, a| {
            if a.first().map(String::as_str) == Some("off") {
                return with_tile(e, |t, _| {
                    t.placing = None;
                    t.preview_forced = false;
                    ok("")
                });
            }

            let (Some(what), Some(x), Some(z)) = (a.first().cloned(), a.get(1).and_then(|v| number(v)), a.get(2).and_then(|v| number(v)))
            else {
                return fail("usage: place_preview <type | building | prefab> <x m> <z m> [key] | place_preview off");
            };

            let key = a.get(3).cloned().unwrap_or_default();
            with_tile(e, |t, e| {
                let Some(d) = crate::tool::document(e) else { return no_tile() };
                t.cursor = Float3::new(x, d.tile().terrain_height(x, z), z);
                t.terrain_hit = true;
                t.preview_forced = true;
                match crate::create_panel::make_for_script(t, e, &what, &key) {
                    Ok(()) => ok(""),
                    Err(err) => fail(err),
                }
            })
        },
    );
}
