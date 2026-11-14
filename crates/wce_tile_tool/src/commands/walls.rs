//! Walls, their pieces and logic, and the siege layout (its nodes, streets and checks).

use super::*;

pub(super) fn register(c: &mut CommandRegistry<Editor>) {
    c.add(
        "wall_sharp",
        "wall_sharp <point index> on|off",
        "mark a point of the selected wall (WceWallSpline) as a corner (a mitre) or a smooth one",
        |e, a| {
            let el = primary(e);
            let Some(d) = document(e) else { return no_tile() };
            let (Some(i), Some(on)) = (a.first().and_then(|x| x.parse::<usize>().ok()), a.get(1).map(|x| x == "on")) else {
                return fail("usage: wall_sharp <point index> on|off");
            };
            let Some(el) = el.filter(|&el| d.tile().is_wall(el)) else { return fail("no wall selected") };
            if d.set_point_attribute(el, i, "sharp", wce_tile::properties::format_bool(on)) { ok("") } else { fail("no such point") }
        },
    );

    c.add(
        "road_sharp",
        "road_sharp <point index> on|off",
        "mark a point of the selected procedural road (WceRoadSpline) as a corner (a corner piece) or a bend",
        |e, a| {
            let el = primary(e);
            let Some(d) = document(e) else { return no_tile() };
            let (Some(i), Some(on)) = (a.first().and_then(|x| x.parse::<usize>().ok()), a.get(1).map(|x| x == "on")) else {
                return fail("usage: road_sharp <point index> on|off");
            };
            let Some(el) = el.filter(|&el| d.tile().is_road(el)) else { return fail("no road selected") };
            if d.set_point_attribute(el, i, "sharp", wce_tile::properties::format_bool(on)) { ok("") } else { fail("no such point") }
        },
    );

    c.add(
        "ground_profile",
        "ground_profile <x0 m> <z0 m> <x1 m> <z1 m> <n>",
        "log the terrain's height (as it shows) at n + 1 points from (x0, z0) to (x1, z1), metres",
        |e, a| {
            let v = a.iter().filter_map(|t| t.parse::<f32>().ok()).collect::<Vec<_>>();
            let [x0, z0, x1, z1, n] = v.as_slice() else { return fail("usage: ground_profile <x0 m> <z0 m> <x1 m> <z1 m> <n>") };
            let Some(d) = document(e) else { return no_tile() };
            let n = (*n as usize).max(1);
            ok((0..=n)
                .map(|i| {
                    let t = i as f32 / n as f32;
                    let (x, z) = (x0 + (x1 - x0) * t, z0 + (z1 - z0) * t);
                    format!("({x:.1}, {z:.1}) {:.2}", d.tile().terrain_height(x, z))
                })
                .collect::<Vec<_>>()
                .join("\n"))
        },
    );

    c.add(
        "wall_layers",
        "wall_layers",
        "log the kit walls' height layers, in the stack's order: each one's name and owner (wall:<key>#<signature>)",
        |e, _| {
            let Some(d) = document(e) else { return no_tile() };
            let l = d.tile().kit_wall_layers();
            ok(format!("{} layer(s){}", l.len(), l.iter().map(|(n, o)| format!("; {n} ({o})")).collect::<String>()))
        },
    );

    c.add("wall_levels", "wall_levels", "log the selected kit wall's pieces with their levels at their joints, the ground at their middle below the wall's height layer and as it shows (scene y), and their joints (metres)", |e, _| {
        let el = primary(e);
        let Some(d) = document(e) else { return no_tile() };
        let Some(el) = el else { return fail("no wall selected") };
        let rows = d.tile().kit_wall_levels(el);
        if rows.is_empty() {
            return fail("no kit wall selected");
        }

        ok(rows.iter().map(|(k, l, below, shown, j)| format!("{k}: {:.2} -> {:.2}, ground {below:.2} below, {shown:.2} shown; joints ({:.2}, {:.2}) -> ({:.2}, {:.2})", l[0], l[1], j[0][0], j[0][1], j[1][0], j[1][1])).collect::<Vec<_>>().join("\n"))
    });

    c.add("wall_convert", "wall_convert", "the selected kit wall's Convert to buildings (the Inspector's button): its pieces made buildings, the wall kept hidden and not exported", |e, _| {
        let el = primary(e);
        let Some(id) = crate::tool::document_id(e) else { return no_tile() };
        let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
        let Some(el) = el else { return fail("no wall selected") };
        d.convert_wall_to_buildings(el).map_or_else(fail, ok)
    });

    c.add(
        "bspline_info",
        "bspline_info",
        "log the selected building spline's buildings: how many, and each one's key, position (m) and destruction level",
        |e, _| {
            let el = primary(e);
            let Some(d) = document(e) else { return no_tile() };
            let Some(el) = el.filter(|&el| d.tile().is_building_spline(el)) else { return fail("no building spline selected") };
            ok(d.tile().building_spline_report(el))
        },
    );

    c.add("wall_info", "wall_info", "log the selected wall's layout: base, length, corners, runs, step sections, attached ends, warnings (a kit wall: its pieces, how far it strays from its line, how short of its end it stops)", |e, _| {
        let el = primary(e);
        let Some(d) = document(e) else { return no_tile() };
        // A wall of kit pieces (*Procedural* off): what it was laid with.
        if let Some(k) = el.and_then(|el| d.tile().kit_wall_report(el)) {
            let keys = k.placed.iter().map(|p| p.key.as_str()).collect::<Vec<_>>().join(" ");
            let end = |a: Option<bool>| match a {
                None => "free",
                Some(true) => "on its building",
                Some(false) => "on its building by a procedural stretch",
            };
            let u = d.tile().unit();
            let stretches = k.stretches.iter().map(|st| format!("{:.2} m long ({:.2}, {:.2}) -> ({:.2}, {:.2})", st.length() / u, st.from[0] / u, st.from[1] / u, st.to[0] / u, st.to[1] / u)).collect::<Vec<_>>().join(", ");
            return ok(format!(
                "kit wall: {} pieces, strays {:.2} at most, short of its end by {:.2}; first end {}, last end {}; {} stretch(es){}{}: {keys}",
                k.placed.len(),
                k.stray,
                k.short_by,
                end(k.attached[0]),
                end(k.attached[1]),
                k.stretches.len(),
                if stretches.is_empty() { String::new() } else { format!(" [{stretches}]") },
                k.notes.iter().map(|n| format!("; {n}")).collect::<String>()
            ));
        }

        let Some(l) = el.and_then(|el| d.tile().wall_layout(el)) else { return fail("no wall selected") };
        let runs = l.plan.runs.iter().map(|r| format!("{:.0}..{:.0} at {:.2} (drift {:.2})", r.s.0, r.s.1, r.level, r.drift)).collect::<Vec<_>>().join(", ");
        let steps = l.plan.steps.iter().map(|st| format!("{:.0}..{:.0} {:.2} -> {:.2}{}", st.s.0, st.s.1, st.from, st.to, if st.whole { " (bastion)" } else { "" })).collect::<Vec<_>>().join(", ");
        let corners = l.plan.corners.iter().map(|(s, i)| format!("point {i} at {s:.0}")).collect::<Vec<_>>().join(", ");
        let ps = wce_walls::pieces::pieces(&l.plan, l.profile);
        let (doors, skipped) = d.tile().wall_doors(&l, &ps);
        let (defences, defences_skipped) = wce_walls::pieces::defence_slots(&l.plan, l.profile);
        let defences = defences.iter().map(|(s, k)| format!("{k} at {s:.0}")).collect::<Vec<_>>().join(", ");
        let n = l.plan.samples.len();
        let ends = [(0, l.plan.samples.first()), (1, l.plan.samples.get(n.wrapping_sub(1)))].iter().filter_map(|&(k, sm)| {
            let (sm, a) = (sm?, l.ends[k].as_ref()?);
            Some(format!("end {k} at ({:.3}, {:.3}), its joint ({:.3}, {:.3})", sm.centre[0], sm.centre[1], a.at[0], a.at[2]))
        }).collect::<Vec<_>>().join(", ");
        ok(format!("{}; {} pieces, doors at pieces {doors:?}; towers and bastions [{defences}]; corners [{corners}]; runs [{runs}]; steps [{steps}]; joints [{ends}]{}", crate::tool::wall_summary(&l), ps.len(),
            l.plan.warnings.iter().chain(&skipped).chain(&defences_skipped).map(|w| format!("; {w}")).collect::<String>()))
    });

    c.add(
        "assert_wall",
        "assert_wall <corners> <steps> [start end: free|attached]",
        "test: the selected wall's layout has these many corners and step sections (and its ends are free or attached)",
        |e, a| {
            let el = primary(e);
            let Some(d) = document(e) else { return no_tile() };
            let Some(l) = el.and_then(|el| d.tile().wall_layout(el)) else { return fail("no wall selected") };
            let want = |i: usize| a.get(i).and_then(|x| x.parse::<usize>().ok());
            let (corners, steps) = (l.plan.corners.len(), l.plan.steps.len());
            if want(0).is_some_and(|n| n != corners) || want(1).is_some_and(|n| n != steps) {
                return fail(format!("{corners} corner(s), {steps} step section(s)"));
            }

            for (k, end) in l.ends.iter().enumerate() {
                if let Some(w) = a.get(2 + k).filter(|w| (*w == "attached") != end.is_some()) {
                    return fail(format!("end {k} is {}, not {w}", if end.is_some() { "attached" } else { "free" }));
                }
            }

            ok("")
        },
    );

    c.add("assert_wall_doors", "assert_wall_doors <n>", "test: the selected wall has n door sections", |e, a| {
        let el = primary(e);
        let Some(d) = document(e) else { return no_tile() };
        let Some(l) = el.and_then(|el| d.tile().wall_layout(el)) else { return fail("no wall selected") };
        let ps = wce_walls::pieces::pieces(&l.plan, l.profile);
        let (doors, _) = d.tile().wall_doors(&l, &ps);
        if a.first().and_then(|x| x.parse::<usize>().ok()) == Some(doors.len()) {
            ok("")
        } else {
            fail(format!("{} door section(s)", doors.len()))
        }
    });

    c.add(
        "siege_ends", "siege_ends [all | <start> <end>]",
        "the nodes the selected siege edge joins, as the game joins them (start, end: an element or -); with `all`, how many street and area connection edges of the tile have an end in no node (each edge logged); with two elements (or -), test that",
        |e, a| {
            let el = primary(e);
            let Some(d) = document(e) else { return no_tile() };
            let t = d.tile();
            if a.first().is_some_and(|x| x == "all") {
                let nodes = t.siege_nodes();
                let edges = t.tscene().find_all("SiegeAIEdgeInstance");
                let name = |n: Option<usize>| n.map_or_else(|| "-".to_owned(), |n| n.to_string());
                for &ed in &edges {
                    if let Some(en) = t.siege_edge_ends_among(ed, &nodes) {
                        wce_foundation::log::info(format!("siege edge {ed} ({}): start {} end {}", t.tscene().doc().get(ed, "type").unwrap_or_default(), name(en.nodes[0]), name(en.nodes[1])));
                    }
                }

                let open = edges.iter().filter(|&&ed| t.siege_edge_ends_among(ed, &nodes).is_some_and(|en| !en.walls && en.nodes.contains(&None))).count();
                return ok(format!("{open} of {} edges have an end in no node (wall edges not counted)", edges.len()));
            }

            let Some(ends) = el.and_then(|el| t.siege_edge_ends(el)) else { return fail("no siege edge selected") };
            let name = |n: Option<usize>| n.map_or_else(|| "-".to_owned(), |n| n.to_string());
            let got = format!("start {} end {}", name(ends.nodes[0]), name(ends.nodes[1]));
            match (a.first(), a.get(1)) {
                (Some(s0), Some(s1)) if got != format!("start {s0} end {s1}") => fail(format!("{got}, expected start {s0} end {s1}")),
                _ => ok(got),
            }
        },
    );

    c.add("check_platforms", "check_platforms", "check the tile's bridges and wall walks as the game links them: the bridges units cannot get onto or cross, the walls whose walkways meet but do not join (the Problems section's Check platforms)", |e, _| {
        with_tile(e, |t, e| {
            let Some(id) = crate::tool::document_id(e) else { return no_tile() };
            let msg = t.run_bridge_check(e, id);
            let (problems, summary) = t.bridge_check.as_ref().map(|(_, r)| (r.problems.iter().map(|p| p.message.clone()).collect::<Vec<_>>().join("; "), r.summary.join("; "))).unwrap_or_default();
            ok(if problems.is_empty() { format!("{msg} [{summary}]") } else { format!("{msg}: {problems} [{summary}]") })
        })
    });

    c.add(
        "on_walks",
        "on_walks <x m> <z m> <radius m>",
        "the buildings that are not the fort's but stand on its walks near a point (what blocks units on the walls)",
        |e, a| {
            let (Some(x), Some(z), Some(r)) = (
                a.first().and_then(|v| v.parse::<f32>().ok()),
                a.get(1).and_then(|v| v.parse::<f32>().ok()),
                a.get(2).and_then(|v| v.parse::<f32>().ok()),
            ) else {
                return fail("usage: on_walks <x m> <z m> <radius m>");
            };
            let Some(d) = document(e) else { return fail("no tile open") };
            let found = d.tile_mut().things_on_walks(x, z, r);
            ok(format!("{} on the walks\n{}", found.len(), found.join("\n")))
        },
    );

    c.add("siege_check", "siege_check", "check the tile's siege AI layout as the game needs it (wce_siege::check: the perimeter, the nodes' rectangles and boundaries, the edges' ends): its problems and notes", |e, _| {
        let Some(d) = document(e) else { return fail("no tile open") };
        let layout = d.tile().current_siege_layout();
        let r = wce_siege::check::check(&layout);
        ok(format!(
            "{} node(s), {} edge(s); {} problem(s), {} note(s)\n{}",
            layout.nodes.len(),
            layout.edges.len(),
            r.problems.len(),
            r.notes.len(),
            r.problems.iter().map(|p| format!("problem: {p}")).chain(r.notes.iter().map(|n| format!("note: {n}"))).collect::<Vec<_>>().join("\n")
        ))
    });

    c.add("wall_anchors", "wall_anchors <x m> <z m> <radius m>", "the fort buildings' ends near a point that a wall's end attaches to (in metres): each building's element and key, the end, and the way out of the building there", |e, a| {
        let (Some(x), Some(z), Some(r)) = (a.first().and_then(|v| v.parse::<f64>().ok()), a.get(1).and_then(|v| v.parse::<f64>().ok()), a.get(2).and_then(|v| v.parse::<f64>().ok())) else {
            return fail("usage: wall_anchors <x m> <z m> <radius m>");
        };
        let Some(d) = document(e) else { return fail("no tile open") };
        let u = d.tile().unit();
        let out = d.tile().wall_anchors().into_iter().filter(|a| (a.at[0] / u - x).hypot(a.at[2] / u - z) <= r).map(|a| {
            format!("#{} {} at ({:.3}, {:.3}, {:.3}) axis ({:.3}, {:.3})", a.elem, a.key, a.at[0] / u, a.at[1] / u, a.at[2] / u, a.axis[0], a.axis[1])
        }).collect::<Vec<_>>();
        ok(format!("{} end(s)\n{}", out.len(), out.join("\n")))
    });

    c.add("wall_pairs", "wall_pairs <x m> <z m> <radius m> [detail]", "the fort buildings near a point and how their walks meet (Check platforms' wall walks): each building's key, position and turn, and per pair that meets its joints and, when it does not join, how far its facing edges are off", |e, a| {
        let (Some(x), Some(z), Some(r)) = (a.first().and_then(|v| v.parse::<f32>().ok()), a.get(1).and_then(|v| v.parse::<f32>().ok()), a.get(2).and_then(|v| v.parse::<f32>().ok())) else {
            return fail("usage: wall_pairs <x m> <z m> <radius m>");
        };
        let Some(d) = document(e) else { return fail("no tile open") };
        let t = d.tile_mut();
        let report = t.check_walls();
        let near = |t: &wce_tile_editor::tile_document::Tile, el: usize| {
            let p = t.world_position(el);
            (p.x - x).hypot(p.z - z) <= r
        };
        let doc = t.tscene().doc();
        let name = |el: usize| format!("#{el} {}", doc.get(el, "key").unwrap_or_default());
        let mut out = vec![];
        for p in report.wall_pairs.iter().filter(|p| near(t, p.a) || near(t, p.b)) {
            let state = match (p.joints, p.closest) {
                (0, Some((across, rise))) => format!("BREAK: facing {across:.3} m across, {rise:.3} m in height"),
                (n, Some((across, rise))) => format!("{n} joint(s); also facing {across:.3} m / {rise:.3} m"),
                (n, None) => format!("{n} joint(s)"),
            };
            out.push(format!("{} <-> {}: {state}", name(p.a), name(p.b)));
        }

        // The fort buildings there that meet nothing.
        let met = report.wall_pairs.iter().flat_map(|p| [p.a, p.b]).collect::<std::collections::HashSet<_>>();
        for el in (0..doc.elements().len()).filter(|&el| t.tscene().type_of(el) == "BUILDING_INSTANCE" && near(t, el)) {
            let key = doc.get(el, "key").unwrap_or_default();
            let c = t.building_category(&key.to_lowercase()).unwrap_or_default();
            if (c.contains("fort") || c.contains("wall") || c.contains("gate") || c.contains("tower")) && !met.contains(&el) {
                let p = t.world_position(el);
                out.push(format!("{} at ({:.2}, {:.2}, {:.2}): meets no other wall", name(el), p.x, p.y, p.z));
            }
        }

        // With `detail`: each fort building's walks against the terrain.
        if a.get(3).is_some_and(|w| w == "detail") {
            let els = (0..t.tscene().doc().elements().len()).filter(|&el| t.tscene().type_of(el) == "BUILDING_INSTANCE" && near(t, el)).collect::<Vec<_>>();
            for el in els {
                let key = t.tscene().doc().get(el, "key").unwrap_or_default();
                let c = t.building_category(&key.to_lowercase()).unwrap_or_default();
                if c.contains("fort") || c.contains("wall") || c.contains("gate") || c.contains("tower") {
                    let p = t.world_position(el);
                    out.push(format!("#{el} {key} ({c}) at ({:.2}, {:.2}, {:.2}):", p.x, p.y, p.z));
                    out.extend(t.walk_heights(el));
                }
            }
        }

        ok(format!("{} line(s)\n{}", out.len(), out.join("\n")))
    });

    c.add("node_adjust_boundary", "node_adjust_boundary [range m]", "make the selected siege AI node's Boundary again for the ground around it, as its Inspector's button does, within the range from its centre (none or 0: automatic; one undo entry)", |e, a| {
        let reach = a.first().and_then(|r| number(r)).filter(|&r| r > 0.0);
        let Some(el) = primary(e) else { return fail("no node selected") };
        let Some(d) = document(e) else { return no_tile() };
        d.adjust_node_boundary(el, reach).map_or_else(fail, |b| ok(format!("Boundary {b}: {} points", d.tile().tscene().polygon_points(b).len())))
    });

    c.add(
        "street_fit",
        "street_fit",
        "fit the selected street edge's two Street boundaries between the nodes it joins, as its Inspector's button does (one undo entry)",
        |e, _| {
            let Some(el) = primary(e) else { return fail("no edge selected") };
            let Some(d) = document(e) else { return no_tile() };
            d.fit_street_sides(el).map_or_else(fail, |()| ok(""))
        },
    );

    c.add("boundary_preset", "boundary_preset circle|semicircle|square|polygon [size m] [turn deg]", "the selected capture location's (or the selected boundary's) boundary as a preset shape, as its Inspector does: its size and turn kept unless given (one undo entry)", |e, a| {
        use wce_tile_editor::tile_document::BoundaryKind;
        let Some(sel) = primary(e) else { return fail("select a capture location or its boundary") };
        let Some(d) = document(e) else { return no_tile() };
        let kind = match a.first().map(|k| k.to_lowercase()).as_deref() {
            Some("circle") => BoundaryKind::Circle,
            Some("semicircle") => BoundaryKind::Semicircle,
            Some("square") => BoundaryKind::Square,
            Some("polygon") => BoundaryKind::Polygon,
            _ => return fail("usage: boundary_preset circle|semicircle|square|polygon [size m] [turn deg]"),
        };

        let location = d.tile().tscene().type_of(sel) == "CaptureLocationInstance";
        let Some(b) = (if location { d.capture_boundary(sel) } else { Some(sel) }) else { return fail("no boundary") };
        let Some(mut shape) = d.boundary_preset(b, kind) else { return fail("not a capture location's boundary") };
        if let Some(size) = a.get(1).and_then(|v| number(v)) {
            shape.size = size;
        }

        if let Some(turn) = a.get(2).and_then(|v| number(v)) {
            shape.turn = turn;
        }

        d.set_boundary_shape(b, shape, location.then_some(sel)).map_or_else(fail, |n| ok(format!("boundary {n}")))
    });

    c.add(
        "tile_piece_ends",
        "tile_piece_ends [selected | #elem]",
        "log a fort piece's ends as piece-to-piece snapping sees them (piece frame: joint, the way out, the outer side)",
        |e, a| {
            let sel = selected(e);
            let Some(d) = document(e) else { return no_tile() };
            let Some(el) = element_arg(d, a.first().map_or("selected", String::as_str), primary_of(&sel, d)) else {
                return fail("no such element");
            };
            let Some(key) = d.tile().tscene().doc().get(el, "key") else { return fail("not a building") };
            let ends = d.tile().piece_ends(&key);
            ok(format!("{} end(s): ", ends.len())
                + &ends
                    .iter()
                    .map(|x| {
                        format!(
                            "at {:.2} {:.2} {:.2} out {:.2} {:.2} side {:.2} {:.2}",
                            x.at[0], x.at[1], x.at[2], x.out[0], x.out[1], x.side[0], x.side[1]
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" | "))
        },
    );

    c.add(
        "tile_fire",
        "tile_fire play|stop|state [selected | #elem]",
        "the burning sequence's preview of a building (fire.rs): play it from the start, stop it, or log where it is",
        |e, a| {
            let sel = selected(e);
            let now = e.clock.time();
            let Some(d) = document(e).filter(|_| !a.is_empty()) else { return fail("usage: tile_fire play|stop|state [selected | #elem]") };
            let Some(el) = element_arg(d, a.get(1).map_or("selected", String::as_str), primary_of(&sel, d)) else {
                return fail("no such element");
            };
            if d.tile().tscene().type_of(el) != "BUILDING_INSTANCE" {
                return fail("not a building");
            }

            match a[0].as_str() {
                "play" => d.tile_mut().fire_start(el, now),
                "stop" => d.tile_mut().fire_stop(Some(el)),
                "state" => {
                    return ok(d.tile().fire_time(el).map_or("not burning".to_owned(), |t| {
                        let (amount, stage) = wce_tile_editor::tile_document::fire::fire_at(t);
                        format!("{t:.2} s, stage {stage}, char {amount:.3}")
                    }));
                }

                _ => return fail("usage: tile_fire play|stop|state [selected | #elem]"),
            }

            ok("")
        },
    );

    c.add(
        "wall_preview_stats",
        "wall_preview_stats [made] [kept]",
        "log how many times the walls' preview pieces were made and how many sets are kept; with numbers, test them",
        |e, a| {
            let Some(d) = document(e) else { return no_tile() };
            let (made, kept) = d.tile().wall_preview_stats();
            let want = |i: usize| a.get(i).and_then(|v| v.parse::<u64>().ok());
            match (want(0), want(1)) {
                (Some(m), _) if m != made => fail(format!("{made} made, expected {m}")),
                (_, Some(k)) if k != kept as u64 => fail(format!("{kept} kept, expected {k}")),
                _ => ok(format!("{made} made, {kept} kept")),
            }
        },
    );

    c.add(
        "wall_tech_check", "wall_tech_check [gaps stacked loose]",
        "check the selected walls' pieces (or roads' bridges') together (as the technology overlay draws them): the stretches no footprint covers, the platforms over one another, the formation line ends on no platform; with numbers, test them (at most that many)",
        |e, a| {
            let walls = crate::tool::selected(e);
            let Some(id) = crate::tool::document_id(e) else { return no_tile() };
            let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
            let (mut gaps, mut stacked, mut loose, mut text) = (0, 0, 0, String::new());
            let walls = walls.into_iter().filter(|&w| d.tile().is_wall(w) || d.tile().is_road(w)).collect::<Vec<_>>();
            for w in walls {
                let Some(r) = d.tile_mut().wall_tech_check(w) else { continue };
                (gaps, stacked, loose) = (gaps + r.gaps.len(), stacked + r.stacked.len(), loose + r.loose_lines.len());
                text.push_str(&format!("\n  wall {w}: {} gap(s), {} stacked, {} loose line end(s)", r.gaps.len(), r.stacked.len(), r.loose_lines.len()));
                for (s0, s1, at) in r.gaps.iter().take(8) {
                    text.push_str(&format!("\n    gap {s0:.1}..{s1:.1} m at ({:.1}, {:.1})", at[0], at[1]));
                }

                for (i, j, area, at) in r.stacked.iter().take(8) {
                    text.push_str(&format!("\n    stacked pieces {i}/{j}, {area:.2} m2 at ({:.1}, {:.1}, {:.1})", at[0], at[1], at[2]));
                }

                for at in r.loose_lines.iter().take(8) {
                    text.push_str(&format!("\n    loose line end at ({:.1}, {:.1}, {:.1})", at[0], at[1], at[2]));
                }
            }

            let want = |i: usize| a.get(i).and_then(|v| v.parse::<usize>().ok());
            let summary = format!("{gaps} gap(s), {stacked} stacked, {loose} loose line end(s){text}");
            if want(0).is_some_and(|n| gaps > n) || want(1).is_some_and(|n| stacked > n) || want(2).is_some_and(|n| loose > n) {
                return fail(summary);
            }

            ok(summary)
        },
    );

    c.add(
        "tile_generate_siege_ai",
        "tile_generate_siege_ai",
        "Generate SiegeAI (docs/plans/siege_ai.md): the layout into its own layer, the old one not exported",
        |e, _| {
            with_tile(e, |t, e| match t.generate_siege_ai_now(e) {
                Ok(l) => ok(format!("{}{}", l.summary(), l.warnings.iter().map(|w| format!("\n  {w}")).collect::<String>())),
                Err(err) => fail(&err),
            })
        },
    );
}
