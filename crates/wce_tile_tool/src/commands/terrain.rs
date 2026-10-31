//! The terrain brushes: heights, ground, water and its rivers, roads, vegetation, and their checks.

use super::*;

pub(super) fn register(c: &mut CommandRegistry<Editor>) {
    c.add("brush", "brush <radius m> [strength] [falloff] [flatten height m] [paint strength 0-1]", "set the terrain brush", |e, a| {
        let Some(r) = a.first().and_then(|x| number(x)) else {
            return fail("usage: brush <radius m> [strength] [falloff] [flatten height m] [paint strength 0-1]");
        };
        with_tile(e, |t, _| {
            t.brush.radius_m = r.clamp(1.0, 500.0);
            if let Some(v) = a.get(1).and_then(|x| number(x)) {
                t.brush.strength = v;
            }

            if let Some(v) = a.get(2).and_then(|x| number(x)) {
                t.brush.falloff = v.clamp(0.0, 1.0);
            }

            if let Some(v) = a.get(3).and_then(|x| number(x)) {
                t.brush.flatten_height = v;
            }

            if let Some(v) = a.get(4).and_then(|x| number(x)) {
                t.brush.opacity = v.clamp(0.0, 1.0);
            }

            ok("")
        })
    });

    c.add("paint", "paint raise|lower|smooth|flatten|level|noise|stamp <x m> <z m> <steps> [open]", "a brush stroke at a point of the tile (one undo entry); open: the stroke goes on (as while the mouse is held), the next call without it ends it", |e, a| {
        with_doc(e, |t, e| {
            let parsed = (a.len() >= 4).then(|| Some((number(&a[1])?, number(&a[2])?, number(&a[3])?))).flatten();
            let Some((x, z, n)) = parsed else { return fail("usage: paint raise|lower|smooth|flatten|level|noise|stamp <x m> <z m> <steps>") };
            let kind = match a[0].as_str() {
                "raise" => BrushKind::Raise,
                "lower" => BrushKind::Lower,
                "smooth" => BrushKind::Smooth,
                "flatten" => BrushKind::Flatten,
                "level" => BrushKind::Level,
                "noise" => BrushKind::Noise,
                "stamp" => BrushKind::Stamp,
                other => return fail(format!("unknown brush '{other}'")),
            };

            let d = document(e).expect("a tile");
            if !d.project().has_height() {
                return fail("no height map");
            }

            for _ in 0..n as i32 {
                d.brush_step(kind, x, z, t.brush.clone(), 1.0 / 60.0);
            }

            if a.get(4).is_none_or(|w| w != "open") {
                d.end_stroke();
            }

            ok("")
        })
    });

    c.add("paint_ground", "paint_ground <group 0-3> <variation 0-3> <x m> <z m> <steps> [open]", "a paint stroke of the ground's textures at a point of the tile, with the brush's radius, strength and falloff (one undo entry); open: the stroke goes on, the next call without it ends it", |e, a| {
        with_doc(e, |t, e| {
            let parsed = (a.len() >= 5).then(|| Some((number(&a[0])?, number(&a[1])?, number(&a[2])?, number(&a[3])?, number(&a[4])?))).flatten();
            let Some((g, v, x, z, n)) = parsed.filter(|p| (0.0..4.0).contains(&p.0) && (0.0..4.0).contains(&p.1)) else {
                return fail("usage: paint_ground <group 0-3> <variation 0-3> <x m> <z m> <steps>");
            };

            let d = document(e).expect("a tile");
            let kind = BrushKind::Paint((g as u8) * 4 + v as u8);
            for _ in 0..n as i32 {
                d.brush_step(kind, x, z, t.brush.clone(), 1.0 / 60.0);
            }

            if a.get(5).is_none_or(|w| w != "open") {
                d.end_stroke();
            }

            if d.tile().blend_pixel_at(x, z).is_none() { fail("no blend map there") } else { ok("") }
        })
    });

    c.add("paint_whole_tile", "paint_whole_tile [<group 0-3> <variation 0-3>]", "paint the whole tile at once with the paint brush as it is set (its strength, mode and filters; default its texture), one undo entry", |e, a| {
        with_doc(e, |t, e| {
            let parsed = (a.len() >= 2).then(|| Some((number(&a[0])?, number(&a[1])?))).flatten().filter(|p| (0.0..4.0).contains(&p.0) && (0.0..4.0).contains(&p.1));
            let channel = match parsed {
                Some((g, v)) => g as u8 * 4 + v as u8,
                None if a.is_empty() => t.paint_channel,
                None => return fail("usage: paint_whole_tile [<group 0-3> <variation 0-3>]"),
            };

            let d = document(e).expect("a tile");
            d.paint_whole_tile(BrushKind::Paint(channel), &t.brush).map_or_else(fail, |changed| ok(if changed { "painted" } else { "nothing changed" }))
        })
    });

    c.add(
        "brush_curve",
        "brush_curve off | on | <preset> | <distance,weight>...",
        "the brushes' falloff curve: off (the falloff slider), on, a preset (smooth, linear, plateau, spike, hard) or its points",
        |e, a| {
            let Some(first) = a.first() else { return fail("usage: brush_curve off | on | <preset> | <distance,weight>...") };
            with_tile(e, |t, e| {
                let mut c = t.brush_curve(e);
                match first.as_str() {
                    "off" => c.on = false,
                    "on" => c.on = true,
                    name => {
                        if let Some((_, points)) = wce_tile::falloff::PRESETS.iter().find(|(n, _)| *n == name) {
                            c.points = points.to_vec();
                        } else {
                            let parsed = a
                                .iter()
                                .map(|p| p.split_once(',').and_then(|(x, y)| Some([number(x)?, number(y)?])))
                                .collect::<Option<Vec<_>>>();
                            let Some(points) = parsed else { return fail(format!("not a preset or points: {}", a.join(" "))) };
                            c.points = wce_tile::falloff::FalloffCurve::from_points(&points).points;
                        }

                        c.on = true;
                    }
                }

                t.set_brush_curve(e, c, true);
                ok("")
            })
        },
    );

    c.add(
        "brush_image",
        "brush_image off | <image file> [turn degrees | random]",
        "the height and paint brushes' image brush (a PNG or DDS whose brightness is the weight)",
        |e, a| {
            let Some(first) = a.first().cloned() else { return fail("usage: brush_image off | <image file> [turn degrees | random]") };
            with_tile(e, |t, e| {
                let mut b = t.image_brush(e);
                if first == "off" {
                    b.on = false;
                } else {
                    (b.on, b.path) = (true, first.clone());
                    match a.get(1).map(String::as_str) {
                        Some("random") => b.random_angle = true,
                        Some(deg) => (b.random_angle, b.angle) = (false, number(deg).unwrap_or(0.0)),
                        None => {}
                    }
                }

                match t.set_image_brush(e, b) {
                    Ok(()) => ok(""),
                    Err(why) => fail(why),
                }
            })
        },
    );

    c.add(
        "brush_paint",
        "brush_paint <group 0-3> <variation 0-3> [normal | flange [threshold] | rock | grouped [variation 0-3] | blur]",
        "what the paint brush lays (the entry's texture group and its variation) and how (TEd's Auto-Flange, Rock-painting)",
        |e, a| {
            let usage =
                "usage: brush_paint <group 0-3> <variation 0-3> [normal | flange [threshold] | rock | grouped [variation 0-3] | blur]";
            let parsed = (a.len() >= 2)
                .then(|| Some((number(&a[0])?, number(&a[1])?)))
                .flatten()
                .filter(|p| (0.0..4.0).contains(&p.0) && (0.0..4.0).contains(&p.1));
            let Some((g, v)) = parsed else { return fail(usage) };
            let value = a.get(3).and_then(|x| number(x));
            let threshold = value.map(|t| t.clamp(0.0, 0.95));
            let mode = match a.get(2).map(String::as_str) {
                None | Some("normal") => wce_tile::blend_field::PaintMode::Normal,
                Some("flange") => wce_tile::blend_field::PaintMode::AutoFlange { threshold: threshold.unwrap_or(0.0) },
                Some("rock") => wce_tile::blend_field::PaintMode::Rock,
                Some("grouped") => wce_tile::blend_field::PaintMode::Grouped { variation: value.map_or(1.5, |v| v.clamp(0.0, 3.0)) },
                Some("blur") => wce_tile::blend_field::PaintMode::Blur,
                Some(_) => return fail(usage),
            };

            with_tile(e, |t, _| {
                t.paint_channel = g as u8 * 4 + v as u8;
                t.brush.paint_mode = mode;
                if let wce_tile::blend_field::PaintMode::AutoFlange { threshold } = mode {
                    t.flange_threshold = threshold;
                }

                ok("")
            })
        },
    );

    c.add(
        "brush_ground",
        "brush_ground <ground type | none>",
        "what the ground types brush paints (a name with spaces in quotes)",
        |e, a| {
            let ground = a.first().and_then(|n| {
                if n == "none" { Some(GROUND_ERASE) } else { GROUND_TYPES.iter().position(|(g, _)| g == n).map(|i| i as u8) }
            });
            let Some(ground) = ground else { return fail("usage: brush_ground <ground type | none>") };
            with_tile(e, |t, _| {
                t.ground_type = ground;
                ok("")
            })
        },
    );

    c.add(
        "paint_filter",
        "paint_filter off | height <min m> <max m> | slope <min deg> <max deg> | nogo",
        "the paint brush's Blendmap Filters (TEd's): where the paint lands",
        |e, a| {
            let usage = "usage: paint_filter off | height <min m> <max m> | slope <min deg> <max deg> | nogo";
            let range = (a.len() >= 3).then(|| Some([number(&a[1])?, number(&a[2])?])).flatten().filter(|r| r[0] <= r[1]);
            let what = a.first().cloned().unwrap_or_default();
            with_tile(e, |t, _| {
                match (what.as_str(), range) {
                    ("off", _) => (t.filter_height_on, t.filter_slope_on) = (false, false),
                    ("height", Some(r)) => (t.filter_height_on, t.filter_height) = (true, r),
                    ("slope", Some(r)) => (t.filter_slope_on, t.filter_slope) = (true, r),
                    ("nogo", _) => (t.filter_slope_on, t.filter_slope) = (true, [wce_tile::blend_field::NOGO_SLOPE_DEGREES, 90.0]),
                    _ => return fail(usage),
                }

                t.sync_paint_filter();
                ok("")
            })
        },
    );

    c.add(
        "paint_water_types",
        "paint_water_types [shallow from m] [deep from m]",
        "paint the water ground types from the water depth view's depth (one undo entry; the view must have shown)",
        |e, a| {
            let shallow = a.first().and_then(|x| number(x)).unwrap_or(0.3);
            let deep = a.get(1).and_then(|x| number(x)).unwrap_or(2.5).max(shallow);
            let Some(d) = document(e) else { return no_tile() };
            match d.paint_water_ground_types(shallow, deep) {
                Ok(n) => ok(format!("{n} pixel(s) changed")),
                Err(why) => fail(why),
            }
        },
    );

    c.add(
        "assert_water_depth",
        "assert_water_depth <x m> <z m> <min m> [max m]",
        "test: the water depth view's depth at a point of the tile (none: no water)",
        |e, a| {
            let parsed = (a.len() >= 3).then(|| Some((number(&a[0])?, number(&a[1])?, number(&a[2])?))).flatten();
            let (Some(d), Some((x, z, lo))) = (document(e), parsed) else {
                return fail("usage: assert_water_depth <x m> <z m> <min m> [max m]");
            };
            let hi = a.get(3).and_then(|v| number(v)).unwrap_or(f32::MAX);
            let t = d.tile();
            let Some(w) = t.water_depth() else { return fail("no water depth worked out (show the water view)") };
            let (ex, ez) = (t.extent_x() as f32, t.extent_z() as f32);
            let (i, j) = ((x / ex * (w.w - 1) as f32).round() as usize, (z / ez * (w.h - 1) as f32).round() as usize);
            let Some(&got) = w.depth.get(j.min(w.h - 1) * w.w + i.min(w.w - 1)) else { return fail("outside the tile") };
            if got.is_finite() && (lo..=hi).contains(&got) {
                ok(format!("{got:.2} m"))
            } else {
                fail(format!("depth {got:.2}, expected {lo} .. {hi}"))
            }
        },
    );

    c.add(
        "river_check",
        "river_check",
        "the rivers against the tile's river link targets: what does not join (logged, one line each)",
        |e, _| {
            let Some(d) = document(e) else { return no_tile() };
            let p = d.river_link_problems();
            ok(if p.is_empty() {
                "the rivers meet their link targets".to_owned()
            } else {
                p.into_iter().map(|(_, m)| m).collect::<Vec<_>>().join("\n")
            })
        },
    );

    c.add("assert_river_problems", "assert_river_problems <count>", "test: how many river link problems the tile has", |e, a| {
        let (Some(d), Some(n)) = (document(e), a.first().and_then(|x| x.parse::<usize>().ok())) else {
            return fail("usage: assert_river_problems <count>");
        };
        let p = d.river_link_problems();
        if p.len() == n {
            ok("")
        } else {
            fail(format!("{} problem(s): {}", p.len(), p.into_iter().map(|(_, m)| m).collect::<Vec<_>>().join("; ")))
        }
    });

    c.add("river_snap", "river_snap", "snap the river ends at the edge onto their link target cells (one undo entry)", |e, _| {
        let Some(d) = document(e) else { return no_tile() };
        match d.snap_river_ends() {
            Ok(n) => ok(format!("{n} end(s) moved")),
            Err(why) => fail(why),
        }
    });

    c.add("spline_flip", "spline_flip", "reverse the selected river's or road's direction (one undo entry)", |e, _| {
        let sel = primary(e);
        let (Some(d), Some(sel)) = (document(e), sel) else { return fail("select a river or road") };
        match d.flip_spline(sel) {
            Ok(()) => ok(""),
            Err(why) => fail(why),
        }
    });

    c.add(
        "import_heights",
        "import_heights <picture> <black m> <white m> [add]",
        "a picture's brightness as the tile's heights over the whole height map, replacing them (or added); one undo entry",
        |e, a| {
            let parsed = (a.len() >= 3).then(|| Some((number(&a[1])?, number(&a[2])?))).flatten();
            let (Some(d), Some((lo, hi))) = (document(e), parsed) else {
                return fail("usage: import_heights <picture> <black m> <white m> [add]");
            };
            match d.import_height_map(Path::new(&a[0]), lo, hi, a.get(3).is_some_and(|x| x == "add")) {
                Ok(n) => ok(format!("{n} samples changed")),
                Err(why) => fail(why),
            }
        },
    );

    c.add("stamp_height", "stamp_height <m>", "the stamp's height at its middle (negative: a hollow)", |e, a| {
        let Some(v) = a.first().and_then(|x| number(x)) else { return fail("usage: stamp_height <m>") };
        with_tile(e, |t, _| {
            t.brush.stamp_height_m = v;
            ok("")
        })
    });

    c.add("tint_freeform", "tint_freeform on|off", "the view filter that tints the freeform instances in the viewport", |e, a| {
        let Some(on) = a.first().map(|x| x == "on") else { return fail("usage: tint_freeform on|off") };
        let Some(d) = document(e) else { return no_tile() };
        d.tile_mut().display_mut().tint_freeform = on;
        ok("")
    });

    c.add("terrain_layer", "terrain_layer <height|blend|ground> list | add <name> | work <id|base> | show <id> on|off | opacity <id> <0..1> | up <id> | down <id> | merge <id> | remove <id> | depend <id> <scene layer path | none>",
        "the tile's terrain layers (the Tile panel's Terrain layers section)", |e, a| {
        let usage = "usage: terrain_layer <height|blend|ground> list | add <name> | work <id|base> | show <id> on|off | opacity <id> <0..1> | up <id> | down <id> | merge <id> | remove <id> | depend <id> <scene layer path | none>";
        let Some(kind) = a.first().and_then(|k| LayerType::named(k)) else { return fail(usage) };
        let Some(d) = document(e) else { return no_tile() };
        let id = a.get(2).and_then(|v| v.parse::<u64>().ok());
        let (rows, working) = d.terrain_layers(kind);
        match (a.get(1).map(String::as_str), id) {
            (Some("list"), _) => {}
            (Some("add"), _) => match d.add_terrain_layer(kind, a.get(2).map_or("Layer", String::as_str)) {
                Ok(id) => return ok(format!("layer {id}")),
                Err(why) => return fail(why),
            },

            (Some("work"), id) => d.set_working_layer(kind, id),
            (Some("show"), Some(id)) => {
                d.set_terrain_layer_visible(kind, id, a.get(3).is_some_and(|v| v == "on"));
            }

            (Some("opacity"), Some(id)) => {
                let Some(o) = a.get(3).and_then(|v| v.parse::<f32>().ok()) else { return fail(usage) };
                d.set_terrain_layer_opacity(kind, id, o);
                d.commit_terrain_layer("Layer opacity", kind);
            }

            (Some("up"), Some(id)) => d.move_terrain_layer(kind, id, 1),
            (Some("down"), Some(id)) => d.move_terrain_layer(kind, id, -1),
            (Some("merge"), Some(id)) => d.merge_terrain_layer(kind, id),
            (Some("depend"), Some(id)) => {
                let Some(path) = a.get(3) else { return fail(usage) };
                let layer = if path == "none" { None } else { match d.tile().find_layer(path) { Some(l) => Some(l), None => return fail("no such scene layer") } };
                d.set_terrain_layer_dependency(kind, id, layer);
            }

            (Some("remove"), Some(id)) => d.remove_terrain_layer(kind, id),
            _ => return fail(usage),
        }

        let (rows, working) = if a.get(1).is_some_and(|v| v == "list") { (rows, working) } else { document(e).map_or((vec![], None), |d| d.terrain_layers(kind)) };
        let list = rows.iter().map(|r| {
            let with = if r.depends.is_empty() { String::new() } else { format!(" with layer {}", r.depends) };
            format!("{} {}{} {:.2} {} KB{with}", r.id, r.name, if r.visible { "" } else { " (hidden)" }, r.opacity, r.bytes / 1024)
        }).collect::<Vec<_>>();
        ok(format!("working {}; {}", working.map_or("base".to_owned(), |w| w.to_string()), if list.is_empty() { "no layers".to_owned() } else { list.join("; ") }))
    });

    c.add(
        "road_items",
        "road_items",
        "how many instances stand along the roads (prefab parts and bridge pieces), and the bridges' pieces",
        |e, _| {
            let Some(d) = document(e) else { return no_tile() };
            ok(d.tile().road_items_report())
        },
    );

    c.add(
        "road_set",
        "road_set <attribute> <value>",
        "set an attribute of the selected procedural road (road_width, road_group ...: one undo entry)",
        |e, a| {
            let sel = primary(e);
            let (Some(sel), Some(attr), Some(value)) = (sel, a.first(), a.get(1)) else {
                return fail("select a road: road_set <attribute> <value>");
            };
            let Some(d) = document(e) else { return no_tile() };
            if d.set_attribute(sel, attr, value) { ok(format!("{attr} = {value}")) } else { fail(format!("{attr} did not change")) }
        },
    );

    c.add(
        "level_along",
        "level_along",
        "TEd's Spline Road: level the ground under the selected line to its height (one undo entry)",
        |e, _| {
            let sel = primary(e);
            with_tile(e, |t, e| {
                let Some(sel) = sel else { return fail("select a spline, polygon or line") };
                let Some(d) = document(e) else { return no_tile() };
                match d.level_along(sel, &t.brush) {
                    Ok(n) => ok(format!("{n} dabs")),
                    Err(why) => fail(why),
                }
            })
        },
    );

    c.add(
        "stroke_along",
        "stroke_along",
        "the active brush (tile_mode) stroked along the selected spline, polygon or line (one undo entry)",
        |e, _| {
            let sel = primary(e);
            with_tile(e, |t, e| {
                let (Some(sel), Some(kind)) = (sel, t.brush_kind()) else {
                    return fail("select a spline, polygon or line, with a brush mode active");
                };
                let Some(d) = document(e) else { return no_tile() };
                match d.stroke_along(kind, sel, &t.brush) {
                    Ok(n) => ok(format!("{n} dabs")),
                    Err(why) => fail(why),
                }
            })
        },
    );

    c.add("falloff_window", "falloff_window 0|1", "close or open the Brush Falloff window", |e, a| {
        let on = a.first().is_some_and(|x| x == "1");
        with_tile(e, |t, _| {
            t.falloff_open = on;
            ok("")
        })
    });

    c.add("paint_ground_type", "paint_ground_type <ground type | none> <x m> <z m> <steps> [open]", "a ground types stroke at a point of the tile with the brush's radius (one undo entry; a name with spaces in quotes); open: the stroke goes on, the next call without it ends it", |e, a| {
        with_doc(e, |t, e| {
            let parsed = (a.len() >= 4).then(|| Some((number(&a[1])?, number(&a[2])?, number(&a[3])?))).flatten();
            let ground = a.first().and_then(|n| if n == "none" { Some(GROUND_ERASE) } else { GROUND_TYPES.iter().position(|(g, _)| g == n).map(|i| i as u8) });
            let (Some((x, z, n)), Some(ground)) = (parsed, ground) else { return fail("usage: paint_ground_type <ground type | none> <x m> <z m> <steps>") };
            let d = document(e).expect("a tile");
            for _ in 0..n as i32 {
                d.brush_step(BrushKind::Ground(ground), x, z, t.brush.clone(), 1.0 / 60.0);
            }

            if a.get(4).is_none_or(|w| w != "open") {
                d.end_stroke();
            }

            if d.tile().ground_rgb_at(x, z).is_none() { fail("no ground type map there") } else { ok("") }
        })
    });

    c.add(
        "texture_group",
        "texture_group <0-3> <name>",
        "set one of the tile entry's four texture groups (red, green, blue, alpha; one undo entry)",
        |e, a| {
            let parsed = (a.len() >= 2).then(|| number(&a[0])).flatten();
            let (Some(d), Some(slot)) = (document(e), parsed) else { return fail("usage: texture_group <0-3> <name>") };
            if !d.texture_group_names().contains(&a[1]) {
                return fail(format!("no texture group '{}' in the climates", a[1]));
            }

            match d.set_texture_group(slot as usize, &a[1]) {
                Ok(()) => ok(""),
                Err(m) => fail(m),
            }
        },
    );

    c.add(
        "veg_brush",
        "veg_brush on|off [radius] [spacing] [clearance] [fill]",
        "the vegetation brush: paint trees instead of placing one per click, and its settings",
        |e, a| {
            let on = match a.first().map(String::as_str) {
                Some("on") => true,
                Some("off") => false,
                _ => return fail("usage: veg_brush on|off [radius] [spacing] [clearance] [fill]"),
            };

            with_tile(e, |t, e| {
                let mut b = t.veg_brush(e);
                b.on = on;
                let num = |i: usize| a.get(i).and_then(|v| number(v));
                b.radius = num(1).unwrap_or(b.radius);
                b.spacing = num(2).unwrap_or(b.spacing);
                b.clearance = num(3).unwrap_or(b.clearance);
                b.fill = num(4).unwrap_or(b.fill);
                t.set_veg_brush(e, b);
                ok("")
            })
        },
    );

    c.add(
        "veg_set_new",
        "veg_set_new <name>",
        "a new vegetation set from the selected trees, or buildings (each kind weighted by how many there are)",
        |e, a| {
            let Some(name) = a.first().cloned() else { return fail("usage: veg_set_new <name>") };
            with_tile(e, |t, e| t.new_vegetation_set(e, &name).map_or_else(fail, ok))
        },
    );

    c.add(
        "veg_set_add",
        "veg_set_add <key>...",
        "add trees (or buildings) to the new set (not saved) and place or paint with it, as Ctrl + click on them in the Create panel does",
        |e, a| {
            if a.is_empty() {
                return fail("usage: veg_set_add <key>...");
            }

            let keys =
                a.iter().map(|k| wce_tile::vegetation_sets::SetEntry { key: k.clone(), weight: 1.0, scale: None }).collect::<Vec<_>>();
            with_tile(e, |t, e| {
                let trees = t.catalogue(e, "VEGETATION_INSTANCE");
                let buildings = t.catalogue(e, "BUILDING_INSTANCE");
                let kind = if keys.iter().all(|k| trees.contains(&k.key)) {
                    wce_tile::vegetation_sets::SetKind::Vegetation
                } else if keys.iter().all(|k| buildings.contains(&k.key)) {
                    wce_tile::vegetation_sets::SetKind::Building
                } else {
                    return fail("the keys are not all trees or all buildings of the game's files");
                };

                t.add_to_new_set(e, kind, &keys);
                ok(t.status.clone())
            })
        },
    );

    c.add("ground_coverage", "ground_coverage <preview|generate> <cliffs|coast|riverbank> <preset|-> [seed]", "the ground coverage's window from a script: a kind's preset (Default: the built-in one; -: the tile's last run, else the kind's last preset), then Preview (prints the summary, draws it) or Generate (replaces the kind's group in \"(AUTOGEN) Ground Coverage\" without asking)", |e, a| {
        let (Some(what), Some(kind), Some(preset)) = (a.first().cloned(), a.get(1).cloned(), a.get(2).cloned()) else {
            return fail("usage: ground_coverage <preview|generate> <cliffs|coast|riverbank> <preset|-> [seed]");
        };

        let Some(kind) = wce_tile::ground_coverage::Kind::from_name(&kind) else { return fail("the kind is cliffs, coast or riverbank") };
        let seed = a.get(3).and_then(|s| s.parse::<u64>().ok());
        with_tile(e, |t, e| t.coverage_for_script(e, &what, kind, &preset, seed).map_or_else(fail, ok))
    });

    c.add("ground_coverage_brush", "ground_coverage_brush <paint|erase> <radius m> <x m> <z m> [<x m> <z m>]...", "one stroke of the ground coverage's brush through ground points (a dab at each), with the window's kind and values (ground_coverage picks them): painting puts the kind's rocks and plants into its Brushed group, erasing takes its coverage away", |e, a| {
        let usage = "usage: ground_coverage_brush <paint|erase> <radius m> <x m> <z m> [<x m> <z m>]...";
        let (Some(what), Some(radius)) = (a.first().cloned(), a.get(1).and_then(|v| number(v))) else { return fail(usage) };
        let pts = a[2..].chunks(2).map(|p| (p.first().and_then(|v| number(v)), p.get(1).and_then(|v| number(v)))).collect::<Vec<_>>();
        if pts.is_empty() || a.len() % 2 != 0 || pts.iter().any(|p| p.0.is_none() || p.1.is_none()) || !matches!(what.as_str(), "paint" | "erase") {
            return fail(usage);
        }

        let pts = pts.iter().map(|p| (p.0.unwrap_or_default(), p.1.unwrap_or_default())).collect::<Vec<_>>();
        with_tile(e, |t, e| t.coverage_stroke_for_script(e, what == "erase", radius, &pts).map_or_else(fail, ok))
    });

    c.add("veg_set_save", "veg_set_save <name>", "save the new vegetation set under a name", |e, a| {
        let Some(name) = a.first().cloned() else { return fail("usage: veg_set_save <name>") };
        with_tile(e, |t, e| {
            t.scratch_name = name;
            t.save_new_set(e).map_or_else(fail, ok)
        })
    });

    c.add("veg_set", "veg_set <name>", "place or paint the trees of a vegetation set (as its Create panel entry)", |e, a| {
        let Some(name) = a.first().cloned() else { return fail("usage: veg_set <name>") };
        with_tile(e, |t, e| crate::create_panel::make_set_for_script(t, e, &name).map_or_else(fail, |()| ok("")))
    });

    c.add(
        "veg_paint",
        "veg_paint <x m> <z m> [<x m> <z m>]...",
        "one stroke of the vegetation brush through ground points (a dab at each): its trees go into the working layer",
        |e, a| {
            let pts = a.chunks(2).map(|p| (p.first().and_then(|v| number(v)), p.get(1).and_then(|v| number(v)))).collect::<Vec<_>>();
            if pts.is_empty() || a.len() % 2 != 0 || pts.iter().any(|p| p.0.is_none() || p.1.is_none()) {
                return fail("usage: veg_paint <x m> <z m> [<x m> <z m>]...");
            }

            with_tile(e, |t, e| {
                if !t.painting_trees(e) {
                    return fail("not painting trees (start placing a tree with the brush on)");
                }

                for (i, (x, z)) in pts.iter().map(|p| (p.0.unwrap_or_default(), p.1.unwrap_or_default())).enumerate() {
                    let Some(d) = document(e) else { return no_tile() };
                    t.cursor = Float3::new(x, d.tile().terrain_height(x, z), z);
                    if i == 0 {
                        t.begin_stroke(e, false);
                    } else {
                        t.dab(e);
                    }
                }

                ok(t.end_stroke_veg(e))
            })
        },
    );

    c.add(
        "veg_erase",
        "veg_erase <x m> <z m>",
        "the vegetation brush's taking away (Ctrl + drag) once at a ground point: the manual trees in its circle go",
        |e, a| {
            let (Some(x), Some(z)) = (a.first().and_then(|v| number(v)), a.get(1).and_then(|v| number(v))) else {
                return fail("usage: veg_erase <x m> <z m>");
            };
            with_tile(e, |t, e| {
                let Some(d) = document(e) else { return no_tile() };
                t.cursor = Float3::new(x, d.tile().terrain_height(x, z), z);
                t.begin_stroke(e, true);
                ok(t.end_stroke_veg(e))
            })
        },
    );

    c.add(
        "mark_height",
        "mark_height <name> <x m> <z m>",
        "remember the terrain height at a point under a name (for assert_height_delta)",
        |e, a| {
            with_doc(e, |t, e| {
                let parsed = (a.len() >= 3).then(|| Some((number(&a[1])?, number(&a[2])?))).flatten();
                let Some((x, z)) = parsed else { return fail("usage: mark_height <name> <x m> <z m>") };
                let h = document(e).expect("a tile").tile().terrain_height(x, z);
                t.marks.insert(a[0].clone(), h);
                ok("")
            })
        },
    );

    c.add(
        "assert_height_delta",
        "assert_height_delta <name> <x m> <z m> <min> <max>",
        "test: the height at a point changed by an amount in [min, max] since mark_height",
        |e, a| {
            with_doc(e, |t, e| {
                let parsed = (a.len() >= 5).then(|| Some((number(&a[1])?, number(&a[2])?, number(&a[3])?, number(&a[4])?))).flatten();
                let Some((x, z, lo, hi)) = parsed else { return fail("usage: assert_height_delta <name> <x m> <z m> <min> <max>") };
                let Some(&mark) = t.marks.get(&a[0]) else { return fail(format!("no mark '{}'", a[0])) };
                let delta = document(e).expect("a tile").tile().terrain_height(x, z) - mark;
                if delta >= lo && delta <= hi { ok("") } else { fail(format!("height changed by {delta:.4}, expected {lo:.4} .. {hi:.4}")) }
            })
        },
    );

    c.add("assert_height", "assert_height <x m> <z m> <height m> [tolerance]", "test: the terrain height at a point", |e, a| {
        let parsed = (a.len() >= 3).then(|| Some((number(&a[0])?, number(&a[1])?, number(&a[2])?))).flatten();
        let (Some(d), Some((x, z, h))) = (document(e), parsed) else {
            return fail("usage: assert_height <x m> <z m> <height m> [tolerance]");
        };
        let tol = a.get(3).and_then(|v| number(v)).unwrap_or(0.01);
        let got = d.tile().terrain_height(x, z);
        if (got - h).abs() <= tol { ok("") } else { fail(format!("height {got:.4}, expected {h:.4}")) }
    });

    c.add(
        "assert_blend",
        "assert_blend <x m> <z m> <group 0-3> <variation 0-3> <min weight 0-65535> [max weight]",
        "test: a blend map weight at a point",
        |e, a| {
            let parsed =
                (a.len() >= 5).then(|| Some((number(&a[0])?, number(&a[1])?, number(&a[2])?, number(&a[3])?, number(&a[4])?))).flatten();
            let (Some(d), Some((x, z, g, v, lo))) = (document(e), parsed) else {
                return fail("usage: assert_blend <x m> <z m> <group> <variation> <min weight> [max weight]");
            };
            let hi = a.get(5).and_then(|v| number(v)).unwrap_or(65535.0);
            if let Err(why) = d.tile_mut().load_blend() {
                return fail(why);
            }

            let Some(p) = d.tile().blend_pixel_at(x, z) else { return fail("no blend map read there") };
            let got = f32::from(p[(g as usize).min(3) * 4 + (v as usize).min(3)]);
            if (lo..=hi).contains(&got) { ok("") } else { fail(format!("weight {got}, expected {lo} .. {hi}")) }
        },
    );

    c.add(
        "assert_ground_type",
        "assert_ground_type <x m> <z m> <ground type | none>",
        "test: the ground type painted at a point",
        |e, a| {
            let parsed = (a.len() >= 3).then(|| Some((number(&a[0])?, number(&a[1])?))).flatten();
            let (Some(d), Some((x, z))) = (document(e), parsed) else {
                return fail("usage: assert_ground_type <x m> <z m> <ground type | none>");
            };
            if let Err(why) = d.tile_mut().load_ground() {
                return fail(why);
            }

            let Some(rgb) = d.tile().ground_rgb_at(x, z) else { return fail("no ground type map there") };
            let got = GROUND_TYPES.iter().find(|(_, c)| *c == rgb).map_or(if rgb == [0, 0, 0] { "none" } else { "?" }, |(n, _)| *n);
            if got == a[2] { ok("") } else { fail(format!("ground type {got} ({rgb:?})")) }
        },
    );
}
