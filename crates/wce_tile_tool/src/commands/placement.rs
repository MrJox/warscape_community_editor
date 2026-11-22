//! Placement (landscape / freeform, snapping, the off-terrain report), the Inspector's values, and the problems.

use super::*;

pub(super) fn register(c: &mut CommandRegistry<Editor>) {
    c.add(
        "tile_problems",
        "tile_problems",
        "log what the game would leave out of the open tile: the manual trees outside every vegetation exclusion polygon",
        |e, _| {
            let Some(d) = document(e) else { return no_tile() };
            let trees = d.trees_outside_exclusions();
            let shown = trees
                .iter()
                .take(10)
                .map(|&t| {
                    let p = d.tile().world_position(t);
                    format!("{t} ({:.1}, {:.1})", p.x, p.z)
                })
                .collect::<Vec<_>>();
            ok(format!(
                "{} manual tree(s) outside every vegetation exclusion polygon{}{}",
                trees.len(),
                if shown.is_empty() { "" } else { ": " },
                shown.join(", ")
            ))
        },
    );

    // ---- placement: landscape / freeform, snapping, the off-terrain report
    c.add(
        "tile_placement",
        "tile_placement <path | #elem | selected> landscape|freeform",
        "set the surface mode of the instances (layers: everything below); landscape snaps them to the terrain",
        |e, a| {
            let sel = selected(e);
            let Some(d) = document(e).filter(|_| a.len() >= 2) else {
                return fail("usage: tile_placement <path | #elem | selected> landscape|freeform");
            };
            let p = primary_of(&sel, d);
            let what = if a[0] == "selected" { sel.clone() } else { element_arg(d, &a[0], p).into_iter().collect() };
            if what.is_empty() {
                return fail(format!("no such layer or element '{}'", a[0]));
            }

            let mode = match a[1].as_str() {
                "landscape" => Placement::Landscape,
                "freeform" => Placement::Freeform,
                _ => return fail("the mode is landscape or freeform"),
            };

            ok(format!("{} changed", d.set_placement(&what, mode)))
        },
    );

    c.add(
        "tile_snap",
        "tile_snap <path | #elem | selected | all> [freeform]",
        "put landscape instances (with `freeform`: every instance) on the terrain; `all` is every instance of the tile",
        |e, a| {
            let sel = selected(e);
            let Some(d) = document(e).filter(|_| !a.is_empty()) else {
                return fail("usage: tile_snap <path | #elem | selected | all> [freeform]");
            };
            let p = primary_of(&sel, d);
            let what: Vec<usize> = match a[0].as_str() {
                "all" => d.tile().tscene().scene_graph().into_iter().collect(),
                "selected" => sel.clone(),
                other => element_arg(d, other, p).into_iter().collect(),
            };

            if what.is_empty() {
                return fail(format!("no such layer or element '{}'", a[0]));
            }

            ok(format!("{} moved", d.snap_to_terrain(&what, a.get(1).is_some_and(|x| x == "freeform"))))
        },
    );

    c.add(
        "tile_set_world",
        "tile_set_world <x m> <y m> <z m>",
        "type a world position for the selected instance (the Inspector's X / Y / Z fields)",
        |e, a| {
            let el = primary(e);
            let parsed = (a.len() >= 3).then(|| Some((number(&a[0])?, number(&a[1])?, number(&a[2])?))).flatten();
            let (Some(d), Some((x, y, z))) = (document(e), parsed) else { return fail("usage: tile_set_world <x m> <y m> <z m>") };
            let Some(el) = el.filter(|&el| d.tile().is_instance(el)) else { return fail("no instance selected") };
            d.set_world_metres(el, Float3::new(x, y, z), 0);
            d.history_mut().break_merge();
            ok("")
        },
    );

    c.add(
        "assert_attr",
        "assert_attr <element | selected> <attribute> <value | ->",
        "test: the element's attribute in the tile has this text (`-`: it is not written)",
        |e, a| {
            let selected = primary(e);
            let el = a.first().and_then(|x| if x == "selected" { selected } else { x.parse::<usize>().ok() });
            let (Some(d), Some(el), Some(attr), Some(_)) = (document(e), el, a.get(1), a.get(2)) else {
                return fail("usage: assert_attr <element | selected> <attribute> <value | ->");
            };
            let want = a[2..].join(" ");
            let got = d.tile().tscene().get(el, attr);
            match got {
                Some(v) if v == want => ok(""),
                None if want == "-" => ok(""),
                Some(v) => fail(format!("{attr} is '{v}', expected '{want}'")),
                None => fail(format!("{attr} is not written, expected '{want}'")),
            }
        },
    );

    c.add(
        "tile_set_common",
        "tile_set_common <attribute> <value>",
        "set a property on every selected element, as the Inspector does with several selected (one undo entry)",
        |e, a| {
            let (Some(id), [attr, value]) = (crate::tool::document_id(e), a) else {
                return fail("usage: tile_set_common <attribute> <value>");
            };
            let elems = selected(e);
            let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
            d.set_common(&elems, attr, value).map(|n| format!("{n} of {} changed", elems.len())).map_or_else(fail, ok)
        },
    );

    c.add(
        "assert_attr_near",
        "assert_attr_near <element | selected> <attribute> <number> <tolerance>",
        "test: the element's attribute is a number this close",
        |e, a| {
            let selected = primary(e);
            let el = a.first().and_then(|x| if x == "selected" { selected } else { x.parse::<usize>().ok() });
            let parsed = (a.len() >= 4).then(|| Some((number(&a[2])?, number(&a[3])?))).flatten();
            let (Some(d), Some(el), Some((want, tol))) = (document(e), el, parsed) else {
                return fail("usage: assert_attr_near <element | selected> <attribute> <number> <tolerance>");
            };
            match d.tile().tscene().get(el, &a[1]).and_then(|v| v.trim().parse::<f32>().ok()) {
                Some(v) if (v - want).abs() <= tol => ok(""),
                Some(v) => fail(format!("{} is {v}, expected {want}", a[1])),
                None => fail(format!("{} is not a number", a[1])),
            }
        },
    );

    c.add(
        "assert_piece", "assert_piece <piece> visible|burning <0|1> | model <text> | offset <x> <y> <z> [tolerance]",
        "test: a model piece of the selected instance on the scene: drawn or hidden, burning or not, its file holds the text, or where it is from the building's own matrix (scene units)",
        |e, a| {
            let el = primary(e);
            let (Some(d), Some(el), Some(k), Some(what)) = (document(e), el, a.first().and_then(|x| x.parse::<u64>().ok()), a.get(1)) else {
                return fail("usage: assert_piece <piece> visible|burning <0|1> | model <text> | offset <x> <y> <z> [tolerance]");
            };

            let (ids, w) = (d.tile().piece_entities(el), d.tile().drawn_matrix(el));
            let id = ids.start + k as wce_scene::render_scene::EntityId;
            if !ids.contains(&id) {
                return fail(format!("no piece {k} on the scene"));
            }

            let Some(en) = e.scene.find(id) else { return fail("no such entity") };
            let Some(wce_scene::render_scene::Payload::MeshInstance(mi)) = Some(&en.payload) else { return fail("not a mesh") };
            match (what.as_str(), a.get(2)) {
                ("visible", Some(v)) => if en.visible == (v == "1") { ok("") } else { fail(format!("visible is {}", en.visible)) },
                ("burning", Some(v)) => if mi.burning == (v == "1") { ok("") } else { fail(format!("burning is {}", mi.burning)) },
                ("model", Some(t)) => if mi.model.contains(t.as_str()) { ok("") } else { fail(format!("model is {}", mi.model)) },
                ("offset", Some(_)) => {
                    let parsed = (a.len() >= 5).then(|| Some((number(&a[2])?, number(&a[3])?, number(&a[4])?))).flatten();
                    let Some((x, y, z)) = parsed else { return fail("usage: assert_piece <piece> offset <x> <y> <z> [tolerance]") };
                    let tol = a.get(5).and_then(|v| number(v)).unwrap_or(0.01);
                    let got = [en.world.m[0][3] - w.m[0][3], en.world.m[1][3] - w.m[1][3], en.world.m[2][3] - w.m[2][3]];
                    if (got[0] - x).abs() <= tol && (got[1] - y).abs() <= tol && (got[2] - z).abs() <= tol {
                        ok("")
                    } else {
                        fail(format!("offset {:.3} {:.3} {:.3}", got[0], got[1], got[2]))
                    }
                }

                _ => fail("usage: assert_piece <piece> visible|burning <0|1> | model <text> | offset <x> <y> <z> [tolerance]"),
            }
        },
    );

    c.add(
        "assert_instance_param",
        "assert_instance_param <index 0..15> <value> [tolerance]",
        "test: every model piece of the selected instance draws with this POSITION4..7 value (index 3 is POSITION4.w)",
        |e, a| {
            let el = primary(e);
            let parsed = (a.len() >= 2).then(|| Some((a[0].parse::<usize>().ok().filter(|&i| i < 16)?, number(&a[1])?))).flatten();
            let (Some(d), Some((i, want))) = (document(e), parsed) else {
                return fail("usage: assert_instance_param <index 0..15> <value> [tolerance]");
            };
            let tol = a.get(2).and_then(|v| number(v)).unwrap_or(1e-3);
            let Some(el) = el else { return fail("nothing selected") };
            let ids = d.tile().piece_entities(el);
            if ids.is_empty() {
                return fail("no model pieces on the scene");
            }

            for id in ids {
                let Some(wce_scene::render_scene::Payload::MeshInstance(mi)) = e.scene.find(id).map(|en| &en.payload) else {
                    return fail(format!("entity {id} is no mesh"));
                };
                let got = [mi.p4, mi.p5, mi.p6, mi.p7][i / 4][i % 4];
                if (got - want).abs() > tol {
                    return fail(format!("entity {id}: {got}, expected {want}"));
                }
            }

            ok("")
        },
    );

    c.add(
        "tile_report_off_terrain",
        "tile_report_off_terrain [tolerance m] [layer path]",
        "log the landscape instances that are more than the tolerance (default 0.5) off the terrain, worst first",
        |e, a| {
            let Some(d) = document(e) else { return no_tile() };
            let tol = match a.first() {
                Some(x) => match number(x) {
                    Some(v) => v,
                    None => return fail("usage: tile_report_off_terrain [tolerance m] [layer path]"),
                },

                None => 0.5,
            };

            let t = d.tile();
            let among = match a.get(1) {
                Some(path) => match t.find_layer(path) {
                    Some(l) => Some(t.instances_below(&[l])),
                    None => return fail(format!("no such layer '{path}'")),
                },

                None => None,
            };

            let list = t.off_terrain(tol, among.as_deref());
            let mut s = format!("{} landscape instance(s) more than {} m off the terrain", list.len(), f6(tol));

            // By type: how many, and the signed range of the offsets.
            let mut by_type: BTreeMap<String, (usize, f32, f32)> = BTreeMap::new();
            for o in &list {
                let acc = by_type.entry(t.tscene().type_of(o.elem).to_owned()).or_insert((0, 1e30, -1e30));
                acc.0 += 1;
                acc.1 = acc.1.min(o.dy);
                acc.2 = acc.2.max(o.dy);
            }

            for (ty, (n, lo, hi)) in &by_type {
                s += &format!("\n  {ty}: {n} ({} .. {} m)", f6(*lo), f6(*hi));
            }

            for o in list.iter().take(8) {
                let w = t.world_position(o.elem);
                s += &format!(
                    "\n  #{} {} '{}' {} m (world {} {} {}, terrain {})",
                    o.elem,
                    t.tscene().type_of(o.elem),
                    t.tscene().name(o.elem),
                    f6(o.dy),
                    f6(w.x),
                    f6(w.y),
                    f6(w.z),
                    f6(t.terrain_height(w.x, w.z)),
                );
            }

            ok(s)
        },
    );

    c.add(
        "assert_placement",
        "assert_placement <path | #elem | selected> landscape|freeform|none",
        "test: the element's surface mode",
        |e, a| {
            let sel = selected(e);
            let Some(d) = document(e).filter(|_| a.len() >= 2) else {
                return fail("usage: assert_placement <path | #elem | selected> landscape|freeform|none");
            };
            let Some(el) = element_arg(d, &a[0], primary_of(&sel, d)) else { return fail(format!("no such element '{}'", a[0])) };
            let got = match d.tile().placement(el) {
                Placement::Landscape => "landscape",
                Placement::Freeform => "freeform",
                Placement::None => "none",
            };

            if got == a[1] { ok("") } else { fail(format!("placement is {got}, expected {}", a[1])) }
        },
    );

    c.add(
        "assert_on_terrain", "assert_on_terrain <path | #elem | selected> 0|1 [tolerance m]",
        "test: the instance (any placement mode) stands on the terrain (y = the terrain height) or not; for a layer, all instances below it", |e, a| {
            let sel = selected(e);
            let Some(d) = document(e).filter(|_| a.len() >= 2) else { return fail("usage: assert_on_terrain <path | #elem | selected> 0|1 [tolerance m]") };
            let tol = match a.get(2) {
                Some(x) => match number(x) {
                    Some(v) => v,
                    None => return fail("bad tolerance"),
                },

                None => 0.001,
            };

            let p = primary_of(&sel, d);
            let what = if a[0] == "selected" { sel.clone() } else { element_arg(d, &a[0], p).into_iter().collect() };
            if what.is_empty() {
                return fail(format!("no such element '{}'", a[0]));
            }

            // Any mode: the instance's y against the terrain at its x / z.
            let t = d.tile();
            let (mut off, mut worst) = (0usize, 0f32);
            for el in t.instances_below(&what) {
                let p = t.world_position(el);
                let dy = p.y - t.landscape_y(el, p.x, p.z);
                if dy.abs() > tol {
                    off += 1;
                    if dy.abs() > worst.abs() {
                        worst = dy;
                    }
                }
            }

            let on = off == 0;
            if on != (a[1] == "1") {
                let now = if on { "all stand on the terrain".to_owned() } else { format!("{off} off the terrain, worst {} m", f6(worst)) };
                return fail(format!("{now}, expected {}", if a[1] == "1" { "on" } else { "off" }));
            }

            ok("")
        },
    );

    c.add(
        "tile_inspect",
        "tile_inspect <element | #elem | selected> [piece]",
        "log the Property Editor rows of an element (or of one of its building pieces) with their values",
        |e, a| {
            let sel = selected(e);
            let Some(d) = document(e) else { return fail("usage: tile_inspect <element | #elem | selected> [piece]") };
            let arg = a.first().map_or("selected", String::as_str);
            let Some(el) = arg.parse::<usize>().ok().or_else(|| element_arg(d, arg, primary_of(&sel, d))) else {
                return fail("usage: tile_inspect <element | #elem | selected> [piece]");
            };
            let item = match a.get(1).and_then(|x| x.parse::<usize>().ok()) {
                Some(k) => wce_tile_editor::tile_document::piece_item(el, k),
                None => wce_tile_editor::tile_document::item_of(el),
            };

            let mut rows = DumpInspector::default();
            rows.lines.push(format!("#{el} {}", d.tile().tscene().type_of(el)));
            let src = wce_scene::render_scene::SourceRef { doc: d.id(), item };
            d.inspect(src, &mut rows);
            for l in &rows.lines {
                wce_foundation::log::info(l.clone());
            }

            ok(format!("{} rows", rows.lines.len()))
        },
    );

    c.add(
        "tile_edit", "tile_edit <element | selected> [piece <n>] <row> <value...>",
        "edit one row of the Property Editor of an element (or of a building piece) as the panel would; the value is a number, true / false, text, `a,b,c`, a choice's number or name, or `Name=true` for check boxes", |e, a| {
            let selected = primary(e);
            let el = a.first().and_then(|x| if x == "selected" { selected } else { x.parse::<usize>().ok() });
            let Some(el) = el else { return fail("usage: tile_edit <element | selected> [piece <n>] <row> <value...>") };
            let (item, rest) = match (a.get(1).map(String::as_str), a.get(2).and_then(|x| x.parse::<usize>().ok())) {
                (Some("piece"), Some(k)) => (wce_tile_editor::tile_document::piece_item(el, k), &a[3..]),
                _ => (wce_tile_editor::tile_document::item_of(el), &a[1..]),
            };

            let (Some(row), true) = (rest.first(), rest.len() >= 2) else { return fail("usage: tile_edit <element> [piece <n>] <row> <value...>") };
            let Some(d) = document(e) else { return fail("no tile is open") };
            let mut rows = DumpInspector { edit: Some((row.clone(), rest[1..].join(" "))), ..Default::default() };
            let src = wce_scene::render_scene::SourceRef { doc: d.id(), item };
            d.inspect(src, &mut rows);
            // A river's bed and shores are drawn below its profile (`inspect_river_tail`), not in the document's own rows.
            if !rows.applied {
                d.inspect_river_tail(el, &mut rows);
            }

            if rows.applied { ok(format!("{row} set")) } else { fail(format!("no row '{row}' to edit")) }
        },
    );

    c.add(
        "tile_model_bounds",
        "tile_model_bounds <building key>",
        "log the bounds of a building key's model pieces (after a few frames, when the models have loaded)",
        |e, a| {
            let (Some(d), Some(key)) = (document(e), a.first()) else { return fail("usage: tile_model_bounds <building key>") };
            match d.tile().building_model(key) {
                Some(m) => ok(format!(
                    "{key}: ok {} resolved {} pieces {} bmin {:.3} {:.3} {:.3} bmax {:.3} {:.3} {:.3}",
                    m.ok,
                    m.resolved,
                    m.pieces.len(),
                    m.bmin.x,
                    m.bmin.y,
                    m.bmin.z,
                    m.bmax.x,
                    m.bmax.y,
                    m.bmax.z
                )),
                None => fail(format!("the tile has no building key '{key}'")),
            }
        },
    );

    c.add(
        "tile_dump_placement", "tile_dump_placement <file.tsv>",
        "write every instance with its surface mode, world position, the y of the file's transforms (before the landscape snap) and the terrain height under it (TSV), to study placement", |e, a| {
            let (Some(d), Some(path)) = (document(e), a.first()) else { return fail("usage: tile_dump_placement <file.tsv>") };
            let t = d.tile();
            let mut out = "elem\ttype\tname\tmode\twx\twy\twz\tfile_y\tterrain\n".to_owned();
            for &el in t.instances() {
                let w = t.world_position(el);
                let mode = match t.placement(el) {
                    Placement::Landscape => "landscape",
                    Placement::Freeform => "freeform",
                    Placement::None => "none",
                };

                // The C++ stream wrote floats with 6 significant digits.
                let g = |v: f32| wce_formats::c_num::printf_g(f64::from(v));
                out += &format!(
                    "{el}\t{}\t{}\t{mode}\t{}\t{}\t{}\t{}\t{}\n", t.tscene().type_of(el), t.tscene().name(el), g(w.x), g(w.y), g(w.z),
                    g((world_transform(t.tscene(), el).position.y / d.project().unit_scale) as f32), g(t.terrain_height(w.x, w.z)),
                );
            }

            match wce_foundation::files::write_replacing(path, out) {
                Ok(()) => ok(format!("{} instances written", t.instances().len())),
                Err(_) => fail(format!("cannot write {path}")),
            }
        },
    );

    c.add(
        "assert_off_terrain_count",
        "assert_off_terrain_count <tolerance m> <n>",
        "test: n landscape instances of the tile are more than the tolerance off the terrain",
        |e, a| {
            let parsed = (a.len() >= 2).then(|| Some((number(&a[0])?, number(&a[1])?))).flatten();
            let (Some(d), Some((tol, n))) = (document(e), parsed) else { return fail("usage: assert_off_terrain_count <tolerance m> <n>") };
            let got = d.tile().off_terrain(tol, None).len();
            if got == n as usize { ok("") } else { fail(format!("{got} instances off the terrain, expected {}", a[1])) }
        },
    );

    c.add(
        "assert_placement_counts",
        "assert_placement_counts <landscape n> <freeform n>",
        "test: how many instances are landscape / freeform",
        |e, a| {
            let parsed = (a.len() >= 2).then(|| Some((number(&a[0])?, number(&a[1])?))).flatten();
            let (Some(d), Some((l, f))) = (document(e), parsed) else {
                return fail("usage: assert_placement_counts <landscape n> <freeform n>");
            };
            let c = d.tile().placement_counts();
            if c.landscape == l as usize && c.freeform == f as usize {
                ok("")
            } else {
                fail(format!("landscape {}, freeform {}", c.landscape, c.freeform))
            }
        },
    );
}
