//! The scripted mouse (the same events a real click or drag sends), the camera views, and what is under the cursor.

use super::*;

pub(super) fn register(c: &mut CommandRegistry<Editor>) {
    // ---- scripted mouse on instances (the same events a real click or drag sends)
    c.add(
        "tile_click",
        "tile_click <element> [ctrl|shift]",
        "click the pixel where an instance is (selects it through the viewport, like the mouse)",
        |e, a| {
            let (Some(d), Some(x)) = (document(e), a.first()) else { return fail("usage: tile_click <element> [ctrl|shift]") };
            let Some(el) = usize::try_from(atoi(x)).ok().filter(|&el| d.tile().is_instance(el)) else { return fail("not an instance") };
            let mut p = d.tile().world_position(el);
            if d.tile().tscene().type_of(el) == "BUILDING_INSTANCE" {
                p.y += d.tile().marker_radius(el) * 0.5;
            }

            click_at(e, p, "").unwrap_or_else(|| fail("the instance is not in view"))
        },
    );

    c.add("tile_click_ground", "tile_click_ground <x m> <z m>", "click the pixel where a ground point is", |e, a| {
        let parsed = (a.len() >= 2).then(|| Some((number(&a[0])?, number(&a[1])?))).flatten();
        let (Some(d), Some((x, z))) = (document(e), parsed) else { return fail("usage: tile_click_ground <x m> <z m>") };
        let p = Float3::new(x, d.tile().terrain_height(x, z), z);
        click_at(e, p, "").unwrap_or_else(|| fail("the point is not in view"))
    });

    c.add(
        "tile_drag_ground",
        "tile_drag_ground <x0 m> <z0 m> <x1 m> <z1 m> [steps]",
        "drag the mouse from one ground point to another (a selected line's point there is dragged), each step a frame",
        |e, a| {
            let n = |i: usize| a.get(i).and_then(|v| number(v));
            let (Some(d), Some(x0), Some(z0), Some(x1), Some(z1)) = (document(e), n(0), n(1), n(2), n(3)) else {
                return fail("usage: tile_drag_ground <x0 m> <z0 m> <x1 m> <z1 m> [steps]");
            };
            let steps = a.get(4).cloned().unwrap_or_else(|| "20".to_owned());
            let (p0, p1) = (Float3::new(x0, d.tile().terrain_height(x0, z0), z0), Float3::new(x1, d.tile().terrain_height(x1, z1), z1));
            let (Some(a0), Some(a1)) = (e.viewport.project(p0), e.viewport.project(p1)) else {
                return fail("the points are not in view");
            };
            let args = [a0.0, a0.1, a1.0, a1.1].map(|v| format!("{v:.1}"));
            let message = format!("pixels {} {} -> {} {}", args[0], args[1], args[2], args[3]);
            // Over the place first, as a mouse is before it presses.
            let _ = CommandRegistry::run_args(e, "mouse", &["move".to_owned(), args[0].clone(), args[1].clone()]);
            mouse(e, &["drag".to_owned(), args[0].clone(), args[1].clone(), args[2].clone(), args[3].clone(), steps], message)
        },
    );

    c.add(
        "tile_drag_point",
        "tile_drag_point <index> <x m> <z m>",
        "drag a point handle of the selected polygon to a ground point with the mouse",
        |e, a| {
            let el = primary(e);
            let parsed = (a.len() >= 3).then(|| Some((number(&a[0])?, number(&a[1])?, number(&a[2])?))).flatten();
            let (Some(d), Some((idx, x, z))) = (document(e), parsed) else { return fail("usage: tile_drag_point <index> <x m> <z m>") };
            let Some(el) = el.filter(|&el| d.tile().is_polygon(el)) else { return fail("no polygon selected") };
            let pts = d.tile().polygon_display_points(el);
            let Some(&from) = usize::try_from(idx as i64).ok().and_then(|i| pts.get(i)) else { return fail("no such point") };
            let to = Float3::new(x, d.tile().terrain_height(x, z), z);
            let (Some((x0, y0)), Some((x1, y1))) = (e.viewport.project(from), e.viewport.project(to)) else { return fail("not in view") };
            // On the point first, as a mouse is before it presses (the hover tells the gizmo's centre the point is under it).
            let _ = CommandRegistry::run_args(e, "mouse", &["move".to_owned(), format!("{x0:.1}"), format!("{y0:.1}")]);
            let args = ["drag".to_owned(), format!("{x0:.1}"), format!("{y0:.1}"), format!("{x1:.1}"), format!("{y1:.1}"), "6".to_owned()];
            mouse(e, &args, "")
        },
    );

    c.add(
        "light_radius_drag",
        "light_radius_drag <radius m>",
        "drag the radius handle of the selected point light with the mouse, to this radius",
        |e, a| {
            let el = primary(e);
            let (Some(d), Some(r)) = (document(e), a.first().and_then(|x| number(x))) else {
                return fail("usage: light_radius_drag <radius m>");
            };
            let Some((from, _)) = el.and_then(|el| d.tile().light_radius_handle(el)) else { return fail("no point light selected") };
            let c = d.tile().world_position(el.expect("selected"));
            let to = Float3::new(c.x + r, c.y, c.z);
            let (Some((x0, y0)), Some((x1, y1))) = (e.viewport.project(from), e.viewport.project(to)) else { return fail("not in view") };
            // On the point first, as a mouse is before it presses (the hover tells the gizmo's centre the point is under it).
            let _ = CommandRegistry::run_args(e, "mouse", &["move".to_owned(), format!("{x0:.1}"), format!("{y0:.1}")]);
            let args = ["drag".to_owned(), format!("{x0:.1}"), format!("{y0:.1}"), format!("{x1:.1}"), format!("{y1:.1}"), "6".to_owned()];
            mouse(e, &args, "")
        },
    );

    c.add(
        "shelter_target_drag",
        "shelter_target_drag <x m> <z m>",
        "drag the target handle of the selected civilian shelter with the mouse to a ground point",
        |e, a| {
            let el = primary(e);
            let parsed = (a.len() >= 2).then(|| Some((number(&a[0])?, number(&a[1])?))).flatten();
            let (Some(d), Some((x, z))) = (document(e), parsed) else { return fail("usage: shelter_target_drag <x m> <z m>") };
            let Some(from) = el.and_then(|el| d.tile().shelter_target(el)) else { return fail("no civilian shelter selected") };
            let from = Float3::new(from.x, from.y + 0.3, from.z);
            let to = Float3::new(x, d.tile().terrain_height(x, z), z);
            let (Some((x0, y0)), Some((x1, y1))) = (e.viewport.project(from), e.viewport.project(to)) else { return fail("not in view") };
            // On the point first, as a mouse is before it presses (the hover tells the gizmo's centre the point is under it).
            let _ = CommandRegistry::run_args(e, "mouse", &["move".to_owned(), format!("{x0:.1}"), format!("{y0:.1}")]);
            let args = ["drag".to_owned(), format!("{x0:.1}"), format!("{y0:.1}"), format!("{x1:.1}"), format!("{y1:.1}"), "6".to_owned()];
            mouse(e, &args, "")
        },
    );

    c.add(
        "assert_shelter_target",
        "assert_shelter_target <x m> <z m> [tolerance]",
        "test: the selected civilian shelter's target is at this ground point",
        |e, a| {
            let el = primary(e);
            let parsed = (a.len() >= 2).then(|| Some((number(&a[0])?, number(&a[1])?))).flatten();
            let (Some(d), Some((x, z))) = (document(e), parsed) else {
                return fail("usage: assert_shelter_target <x m> <z m> [tolerance]");
            };
            let tol = a.get(2).and_then(|v| number(v)).unwrap_or(0.5);
            match el.and_then(|el| d.tile().shelter_target(el)) {
                Some(t) if (t.x - x).abs() <= tol && (t.z - z).abs() <= tol => ok(""),
                Some(t) => fail(format!("target at {:.2} {:.2}", t.x, t.z)),
                None => fail("no civilian shelter selected"),
            }
        },
    );

    c.add("draw_polygon", "draw_polygon <type> [hint | base wall]", "start drawing a polygon (or a WceWallSpline, with its base wall): click the ground for points; a closed shape finishes on its first point, Enter finishes any; Esc cancels", |e, a| {
        let Some(kind) = a.first() else { return fail("usage: draw_polygon <type> [hint | base wall]") };
        with_tile(e, |t, e| {
            if let Err(err) = crate::create_panel::make_for_script(t, e, kind, "") {
                return fail(err);
            }

            if !t.drawing {
                t.stop_making();
                return fail(format!("{kind} is placed, not drawn"));
            }

            if let (Some(h), Some(d)) = (a.get(1).filter(|h| HINTS.contains(&h.as_str())), t.draft.as_mut()) {
                d.attrs.insert("hint_type".to_owned(), h.clone());
            }

            if let (Some(b), Some(d)) = (a.get(1).filter(|b| wce_walls::profile::profile(b).is_some()), t.draft.as_mut()) {
                d.attrs.insert("wall_base".to_owned(), b.clone());
            }

            ok("")
        })
    });

    c.add(
        "assert_at_cursor",
        "assert_at_cursor [tolerance]",
        "test: the selected instance is on the ground point the mouse last hovered",
        |e, a| {
            with_doc(e, |t, e| {
                let tol = a.first().and_then(|x| number(x)).unwrap_or(0.5);
                let el = primary(e);
                let (Some(el), true) = (el, t.terrain_hit) else { return fail("nothing selected or the cursor never touched the ground") };
                let p = document(e).expect("a tile").tile().world_position(el);
                let c = t.cursor;
                if (p.x - c.x).abs() <= tol && (p.z - c.z).abs() <= tol {
                    ok("")
                } else {
                    fail(format!("at {:.2} {:.2}, cursor {:.2} {:.2}", p.x, p.z, c.x, c.z))
                }
            })
        },
    );

    c.add("assert_mode", "assert_mode select|raise|...", "test: the tile tool's left-button mode", |e, a| {
        with_tile(e, |t, _| {
            let now = MODE_NAMES[t.mode().index()];
            if a.first().is_some_and(|x| x == now) { ok("") } else { fail(format!("mode is {now}")) }
        })
    });

    c.add("tile_selected_px", "tile_selected_px", "the viewport pixel of the selected instance (its gizmo's centre): x y", |e, _| {
        let Some(el) = crate::tool::primary(e) else { return fail("nothing selected") };
        let Some(p) = document(e).map(|d| d.tile().world_position(el)) else { return no_tile() };
        match e.viewport.project(p) {
            Some((x, y)) => ok(format!("{x:.0} {y:.0}")),
            None => fail("not in view"),
        }
    });

    c.add("assert_grabbing", "assert_grabbing 0|1", "test: whether a G grab is in progress", |e, a| {
        with_tile(e, |t, _| {
            if a.first().is_some_and(|x| (x == "1") == t.grabbing()) {
                ok("")
            } else {
                fail(if t.grabbing() { "grabbing" } else { "not grabbing" })
            }
        })
    });

    c.add("view_save", "view_save <name>", "save the camera as a view of the open tile (the viewport's camera chip)", |e, a| {
        let Some(name) = a.first().cloned() else { return fail("usage: view_save <name>") };
        with_tile(e, |t, e| t.save_camera_view(e, &name).map_or_else(no_tile, |i| ok(format!("view {i}"))))
    });

    c.add("view_go", "view_go <name>", "put the camera on a saved view of the open tile", |e, a| {
        let Some(name) = a.first().cloned() else { return fail("usage: view_go <name>") };
        with_tile(e, |t, e| if t.go_to_camera_view(e, &name) { ok("") } else { fail(format!("no view '{name}'")) })
    });
}
