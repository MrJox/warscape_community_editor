//! Layers and groups: flags, structure, export type, and the Scene tree.

use super::*;

pub(super) fn register(c: &mut CommandRegistry<Editor>) {
    // ---- layers: flags, structure, export type. A layer is named by its path from the scene graph ("Walls/Standard_Walls"), "#n" is an
    // element index.
    c.add(
        "tile_flag",
        "tile_flag <layer path | #elem | selected> visible|active on|off",
        "the eye (OIF_VISIBLE) or the cursor arrow (OIF_ACTIVE, selectable) of a layer or instance",
        |e, a| {
            let sel = selected(e);
            let Some(d) = document(e).filter(|_| a.len() >= 3) else {
                return fail("usage: tile_flag <layer path | #elem | selected> visible|active on|off");
            };
            let el = element_arg(d, &a[0], primary_of(&sel, d));
            let Some(el) = el.filter(|&el| el < d.tile().tscene().doc().elements().len()) else {
                return fail(format!("no such layer or element '{}'", a[0]));
            };
            let flag = match a[1].as_str() {
                "visible" => "OIF_VISIBLE",
                "active" => "OIF_ACTIVE",
                _ => return fail("the flag is visible or active"),
            };

            let targets = if a[0] == "selected" { sel } else { vec![el] };
            if d.set_flags(&targets, flag, a[2] == "on") { ok("") } else { ok(format!("already {}", a[2])) }
        },
    );

    c.add(
        "tile_new_layer",
        "tile_new_layer <name> [parent layer path]",
        "a new layer in a layer (the scene graph by default), selected",
        |e, a| {
            let Some(d) = document(e).filter(|_| !a.is_empty()) else { return fail("usage: tile_new_layer <name> [parent layer path]") };
            let parent = match a.get(1) {
                Some(p) => d.tile().find_layer(p),
                None => d.tile().tscene().scene_graph(),
            };

            d.add_layer(parent, &a[0])
                .map_or_else(|err| fail(if err.is_empty() { "no such parent layer".to_owned() } else { err }), |_| ok(""))
        },
    );

    c.add("tile_rename", "tile_rename <layer path | #elem | selected> <name>", "rename a layer or an instance", |e, a| {
        let sel = selected(e);
        let Some(d) = document(e).filter(|_| a.len() >= 2) else {
            return fail("usage: tile_rename <layer path | #elem | selected> <name>");
        };
        let el = element_arg(d, &a[0], primary_of(&sel, d));
        if el.is_some_and(|el| d.rename(el, &a[1])) { ok("") } else { fail(format!("no such layer or element '{}'", a[0])) }
    });

    c.add(
        "tile_move_to",
        "tile_move_to <what: path | #elem | selected> <target layer path> [before <path | #elem>]",
        "move layers / instances into a layer (in front of a sibling): the selection when `selected`",
        |e, a| {
            let sel = selected(e);
            let Some(d) = document(e).filter(|_| a.len() >= 2) else {
                return fail("usage: tile_move_to <path | #elem | selected> <target layer path> [before <path | #elem>]");
            };

            let p = primary_of(&sel, d);
            let what = if a[0] == "selected" { sel.iter().copied().map(Some).collect() } else { vec![element_arg(d, &a[0], p)] };
            let layer = element_arg(d, &a[1], p);
            let before = if a.len() > 3 && a[2] == "before" { element_arg(d, &a[3], p) } else { None };
            let (Some(Some(_)), Some(layer)) = (what.first(), layer) else { return fail("nothing to move, or no such target layer") };
            let what = what.into_iter().flatten().collect::<Vec<_>>();
            d.move_items(&what, layer, before).map_or_else(fail, |()| ok(""))
        },
    );

    c.add("tile_group", "tile_group <new layer name>", "put the selected items into a new layer next to the first one", |e, a| {
        let sel = selected(e);
        let Some(d) = document(e).filter(|_| !a.is_empty()) else { return fail("usage: tile_group <new layer name>") };
        d.group_items(&sel, &a[0]).map_or_else(fail, |_| ok(""))
    });

    // ---- a combined project's variations (docs/plans/tile_variations.md): lists are comma or space separated.
    let list =
        |a: &[String]| a.iter().flat_map(|s| s.split(',')).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect::<Vec<_>>();
    c.add(
        "tile_variations",
        "tile_variations <variation,...> | all",
        "the variations the selected layers, groups and variation folders belong to (all: as their parent's); an instance has its folder's",
        move |e, a| {
            let sel = selected(e);
            let Some(d) = document(e).filter(|_| !a.is_empty()) else { return fail("usage: tile_variations <variation,...> | all") };
            let l = list(a);
            let l = (l != ["all"]).then_some(l);
            d.set_variations(&sel, l.as_deref()).map_or_else(fail, |()| ok(""))
        },
    );

    c.add(
        "tile_to_variation_folder",
        "tile_to_variation_folder <variation,...>",
        "Move to a variation folder: the selection (side by side in one layer, group or folder) into a new folder for those variations",
        move |e, a| {
            let sel = selected(e);
            let Some(d) = document(e).filter(|_| !a.is_empty()) else { return fail("usage: tile_to_variation_folder <variation,...>") };
            d.to_variation_folder(&sel, &list(a)).map_or_else(fail, |f| ok(format!("#{f}")))
        },
    );

    c.add(
        "tile_combine",
        "tile_combine [--ref <variation>] [--align] <variation> <variation>...",
        "Tile > Variations > Combine: the open tile's variations named made one combined project in the projects (each from its project, else the kit, else the packs; checked before anything is written; the projects that went in moved to the backup), opened",
        move |e, a| {
            let align = a.iter().any(|x| x == "--align");
            let a = a.iter().filter(|x| *x != "--align").cloned().collect::<Vec<_>>();
            let at = a.iter().position(|x| x == "--ref");
            let reference = at.and_then(|i| a.get(i + 1)).cloned();
            let names = a.iter().enumerate().filter(|(i, _)| at.is_none_or(|r| *i != r && *i != r + 1)).map(|(_, x)| x.clone()).collect::<Vec<_>>();
            let names = list(&names);
            with_tile(e, |t, e| t.combine_now(e, &names, reference.as_deref(), align).map_or_else(fail, ok))
        },
    );

    c.add(
        "tile_separate",
        "tile_separate <variation>",
        "Tile > Variations > Separate: a variation of the open combined project made a project of its own again, the others left combined (the tile saved first, opened again)",
        |e, a| {
            let Some(v) = a.first().cloned() else { return fail("usage: tile_separate <variation>") };
            with_tile(e, |t, e| t.separate_variation(e, &v).map_or_else(fail, ok))
        },
    );

    c.add(
        "tile_split_variation",
        "tile_split_variation <variation>",
        "Split for a variation: the selection copied into a folder for it, the originals into one for the others; the copies selected",
        |e, a| {
            let sel = selected(e);
            let Some(v) = a.first() else { return fail("usage: tile_split_variation <variation>") };
            let Some(d) = document(e) else { return no_tile() };
            d.split_for_variation(&sel, v).map_or_else(fail, |c| ok(format!("{} copies", c.len())))
        },
    );

    c.add(
        "tile_variation_tidy",
        "tile_variation_tidy",
        "Tidy: folders of every or no variation taken away, same ones joined; no variation changes",
        |e, _| {
            let Some(d) = document(e) else { return no_tile() };
            d.tidy_variations().map_or_else(fail, |n| ok(format!("{n} edits")))
        },
    );

    c.add(
        "tile_working_layer",
        "tile_working_layer [layer path | #elem | none]",
        "TEd's working layer, where new items go (none: the root); no argument: log it",
        |e, a| {
            let sel = selected(e);
            let Some(id) = crate::tool::document_id(e) else { return no_tile() };
            with_tile(e, |t, e| {
                let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
                match a.first().map(String::as_str) {
                    None => ok(t.working_layer_of(d).map_or_else(|| "the root".to_owned(), |l| crate::tool::TileTool::layer_label(d, l))),
                    Some("none") => t.set_working_layer(d, None).map_or_else(fail, ok),
                    Some(spec) => {
                        let Some(el) = element_arg(d, spec, primary_of(&sel, d)) else { return fail(format!("no layer '{spec}'")) };
                        t.set_working_layer(d, Some(el)).map_or_else(fail, ok)
                    }
                }
            })
        },
    );

    c.add(
        "tile_instance_group",
        "tile_instance_group",
        "group the selection into an instance group in the working layer (Ctrl+G)",
        |e, _| with_tile(e, |t, e| t.group_selection(e).map_or_else(fail, ok)),
    );

    c.add("tile_ungroup", "tile_ungroup", "take the selected instance groups apart into the working layer (Ctrl+U)", |e, _| {
        with_tile(e, |t, e| t.ungroup_selection(e).map_or_else(fail, ok))
    });

    c.add("tile_delete", "tile_delete <layer path | #elem | selected>", "delete layers / instances (with everything below)", |e, a| {
        let sel = selected(e);
        let Some(d) = document(e).filter(|_| !a.is_empty()) else { return fail("usage: tile_delete <layer path | #elem | selected>") };
        let p = primary_of(&sel, d);
        let what = if a[0] == "selected" { sel.iter().copied().map(Some).collect() } else { vec![element_arg(d, &a[0], p)] };
        let ok_first = what.first().is_some_and(Option::is_some);
        let what = what.into_iter().flatten().collect::<Vec<_>>();
        if ok_first && d.remove_items(&what) { ok("") } else { fail("nothing to delete") }
    });

    c.add(
        "tile_duplicate_layer",
        "tile_duplicate_layer <layer path>",
        "duplicate a layer with everything in it (named '<name> copy')",
        |e, a| {
            let sel = selected(e);
            let Some(d) = document(e).filter(|_| !a.is_empty()) else { return fail("usage: tile_duplicate_layer <layer path>") };
            let Some(el) = element_arg(d, &a[0], primary_of(&sel, d)) else { return fail("no such layer") };
            d.duplicate_layer(el).map_or_else(fail, |_| ok(""))
        },
    );

    c.add(
        "tile_export_type",
        "tile_export_type <layer path> Normal|Off|Separate|Escalation1|Escalation2|Escalation3",
        "the export type of a layer",
        |e, a| {
            let sel = selected(e);
            let Some(d) = document(e).filter(|_| a.len() >= 2) else {
                return fail("usage: tile_export_type <layer path> Normal|Off|Separate|Escalation1|Escalation2|Escalation3");
            };
            let Some(el) = element_arg(d, &a[0], primary_of(&sel, d)).filter(|&el| d.tile().tscene().is_layer(el)) else {
                return fail("no such layer");
            };
            if d.set_attribute(el, "export_type", &format!("LayerExportType_{}", a[1])) { ok("") } else { fail("cannot set it") }
        },
    );

    c.add("tile_select_layer", "tile_select_layer <layer path | #elem>", "select a layer or an element in the scene tree", |e, a| {
        let Some(what) = a.first() else { return fail("usage: tile_select_layer <layer path | #elem>") };
        let p = primary(e);
        let Some(d) = document(e) else { return fail("usage: tile_select_layer <layer path | #elem>") };
        let el = element_arg(d, what, p);
        let Some(el) = el else { return fail("no such layer or element") };
        select_elem(e, Some(el), false);
        ok("")
    });

    c.add(
        "tile_select_in", "tile_select_in <layer path> <type> [frame|click]",
        "select the first instance of a type below a layer; `frame` points the camera at it, `click` clicks its pixel with the scripted mouse (frame it and wait some frames first)",
        |e, a| {
            let sel = selected(e);
            let Some(d) = document(e).filter(|_| a.len() >= 2) else { return fail("usage: tile_select_in <layer path> <type> [click]") };
            let Some(layer) = element_arg(d, &a[0], primary_of(&sel, d)) else { return fail("no such layer") };
            let t = d.tile();
            let Some(found) = t.instances().iter().copied().find(|&el| t.tscene().type_of(el) == a[1] && t.tscene().is_descendant(el, layer)) else {
                return fail(format!("no {} in {}", a[1], a[0]));
            };

            let p = t.world_position(found);
            match a.get(2).map(String::as_str) {
                Some("click") => {
                    let mut q = p;
                    if t.tscene().type_of(found) == "BUILDING_INSTANCE" {
                        q.y += t.marker_radius(found) * 0.5;
                    }

                    click_at(e, q, format!("clicked element {found}")).unwrap_or_else(|| fail("the instance is not in view"))
                }

                // The camera looks at it (wait a few frames before clicking).
                Some("frame") => {
                    e.viewport.controller().set_orbit(0.8, 0.7, 60.0, p);

                    ok(format!("framed element {found}"))
                }

                _ => {
                    select_elem(e, Some(found), false);
                    ok(format!("element {found}"))
                }
            }
        },
    );

    c.add(
        "tile_tree",
        "tile_tree [depth]",
        "log the scene tree from the root down to a depth (default 2) with each layer's flags and child count",
        |e, a| {
            let Some(d) = document(e) else { return no_tile() };
            let depth = a.first().map_or(2, |x| atoi(x));
            let t = d.tile();
            let sc = t.tscene();
            let mut out = String::new();
            let mut stack = sc.scene_graph().map(|g| vec![(g, 0i64)]).unwrap_or_default();
            while let Some((el, level)) = stack.pop() {
                let kids = sc.children(el);
                out += &format!(
                    "{:width$}#{el} {} '{}' {}{}{} {}\n",
                    "",
                    sc.type_of(el),
                    sc.name(el),
                    if t.own_flag(el, "OIF_VISIBLE") { "V" } else { "-" },
                    if t.own_flag(el, "OIF_ACTIVE") { "A" } else { "-" },
                    if sc.is_layer(el) { format!(" export={}", t.export_type(el)) } else { String::new() },
                    kids.len(),
                    width = (level * 2) as usize,
                );
                if level < depth {
                    stack.extend(kids.into_iter().filter(|&k| sc.is_layer(k)).rev().map(|k| (k, level + 1)));
                }
            }

            ok(out)
        },
    );

    c.add("assert_flag", "assert_flag <layer path | #elem> visible|active 0|1", "test: the element's own eye / cursor arrow", |e, a| {
        let sel = selected(e);
        let Some(d) = document(e).filter(|_| a.len() >= 3) else {
            return fail("usage: assert_flag <layer path | #elem> visible|active 0|1");
        };
        let Some(el) = element_arg(d, &a[0], primary_of(&sel, d)) else { return fail("no such layer or element") };
        let v = d.tile().own_flag(el, if a[1] == "visible" { "OIF_VISIBLE" } else { "OIF_ACTIVE" });
        if v == (a[2] == "1") { ok("") } else { fail(format!("{} {} is {}", a[0], a[1], if v { "1" } else { "0" })) }
    });

    c.add(
        "assert_shown",
        "assert_shown <layer path | #elem> 0|1",
        "test: the element is drawn (visible itself and in every layer above)",
        |e, a| {
            let sel = selected(e);
            let Some(d) = document(e).filter(|_| a.len() >= 2) else { return fail("usage: assert_shown <layer path | #elem> 0|1") };
            let Some(el) = element_arg(d, &a[0], primary_of(&sel, d)) else { return fail("no such layer or element") };
            let shown = d.tile().shown(el);
            if shown == (a[1] == "1") { ok("") } else { fail(format!("{}{}", a[0], if shown { " is shown" } else { " is hidden" })) }
        },
    );

    c.add(
        "assert_selectable",
        "assert_selectable <layer path | #elem> 0|1",
        "test: the element can be picked (active itself and in every layer above)",
        |e, a| {
            let sel = selected(e);
            let Some(d) = document(e).filter(|_| a.len() >= 2) else { return fail("usage: assert_selectable <layer path | #elem> 0|1") };
            let Some(el) = element_arg(d, &a[0], primary_of(&sel, d)) else { return fail("no such layer or element") };
            let s = d.tile().selectable(el);
            if s == (a[1] == "1") { ok("") } else { fail(format!("{}{}", a[0], if s { " is selectable" } else { " is locked" })) }
        },
    );

    c.add(
        "assert_layer",
        "assert_layer <layer path> <children n | exists 0|1 | export Type | name N>",
        "test: facts of a layer (children n | exists 0|1 | export Type | child <index> <name>)",
        |e, a| {
            let sel = selected(e);
            let usage = "usage: assert_layer <layer path> <children n | exists 0|1 | export Type>";
            let Some(d) = document(e).filter(|_| a.len() >= 2) else { return fail(usage) };
            let el = element_arg(d, &a[0], primary_of(&sel, d));
            if a[1] == "exists" {
                return if el.is_some() == a.get(2).is_some_and(|x| x == "1") {
                    ok("")
                } else {
                    fail(if el.is_some() { "the layer exists" } else { "no such layer" })
                };
            }

            let Some(el) = el else { return fail(format!("no such layer '{}'", a[0])) };
            let t = d.tile();
            match (a[1].as_str(), a.len()) {
                ("children", 3..) => {
                    let n = t.tscene().children(el).len();
                    if n as i64 == atoi(&a[2]) { ok("") } else { fail(format!("children: {n}")) }
                }

                ("export", 3..) => {
                    let x = t.export_type(el);
                    if x == a[2] { ok("") } else { fail(format!("export type {x}")) }
                }

                // The direct child at an index (counted from 0) has this name.
                ("child", 4..) => {
                    let kids = t.tscene().children(el);
                    let got = usize::try_from(atoi(&a[2]))
                        .ok()
                        .and_then(|i| kids.get(i))
                        .map_or_else(|| "(none)".to_owned(), |&k| t.tscene().name(k));
                    if got == a[3] { ok("") } else { fail(format!("child {} is '{got}'", a[2])) }
                }

                _ => fail(usage),
            }
        },
    );

    c.add(
        "assert_selection_names",
        "assert_selection_names <name>...",
        "test: the selected elements have exactly these names, in selection order",
        |e, a| {
            let sel = selected(e);
            let Some(d) = document(e) else { return no_tile() };
            let got = sel.iter().map(|&el| d.tile().tscene().name(el)).collect::<Vec<_>>().join(",");
            let want = a.join(",");
            if got == want { ok("") } else { fail(format!("selected: {got}")) }
        },
    );
}
