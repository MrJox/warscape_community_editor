//! Prefabs (prefab_panel.rs): the list, placing, opening, recognising, unpacking and saving them.

use super::*;

pub(super) fn register(c: &mut CommandRegistry<Editor>) {
    // ---- prefabs (prefab_panel.rs)

    c.add("prefab_list", "prefab_list [words]", "log the prefab library (key, folder), filtered by words", |e, a| {
        let words = a.iter().map(|w| w.to_lowercase()).collect::<Vec<_>>();
        with_tile(e, |t, e| {
            let lib = t.prefab_library(e);
            let rows = lib
                .entries()
                .filter(|p| words.iter().all(|w| format!("{}/{}", p.folder, p.key).to_lowercase().contains(w)))
                .map(|p| format!("{}  {}", p.key, p.folder))
                .collect::<Vec<_>>();
            ok(format!("{} prefabs:\n{}", rows.len(), rows.join("\n")))
        })
    });

    c.add("prefab_rescan", "prefab_rescan", "scan the prefab folders again", |e, _| {
        with_tile(e, |t, e| ok(format!("{} prefabs", t.rescan_prefabs(e))))
    });

    c.add(
        "prefab_place",
        "prefab_place <key> [x m z m]",
        "place a prefab in the working layer, on the ground at x z (default: the view's centre)",
        |e, a| {
            let Some(key) = a.first().cloned() else { return fail("usage: prefab_place <key> [x m z m]") };
            let at = (a.len() >= 3).then(|| Some(Float3::new(number(&a[1])?, 0.0, number(&a[2])?))).flatten();
            with_tile(e, |t, e| {
                let at = at.unwrap_or_else(|| t.placement_point(e));
                t.place_prefab(e, &key, at).map_or_else(fail, ok)
            })
        },
    );

    c.add("prefab_open_in_tool", "prefab_open_in_tool", "open the selected copy's prefab in the Prefab tool", |e, _| {
        let Some(g) = selected_group(e) else { return fail("select a copy of a prefab") };
        with_tile(e, |t, e| t.open_in_prefab_tool(e, g).map_or_else(fail, ok))
    });

    c.add(
        "prefab_recognise",
        "prefab_recognise",
        "make the instance groups that are exact copies of a prefab references to it (as opening a tile from the Assembly Kit does)",
        |e, _| with_tile(e, |t, e| t.recognise_prefabs(e).map_or_else(fail, ok)),
    );

    c.add("prefab_unpack", "prefab_unpack", "make the selected copy of a prefab a plain group", |e, _| {
        let Some(g) = selected_group(e) else { return fail("select a copy of a prefab") };
        with_tile(e, |t, e| t.unpack_prefab(e, g).map_or_else(fail, ok))
    });

    c.add(
        "prefab_save_kit",
        "prefab_save_kit <key>",
        "write a prefab as TEd reads it into the Assembly Kit's art/prefabs (the prefabs placed in it baked)",
        |e, a| {
            let Some(key) = a.first().cloned() else { return fail("usage: prefab_save_kit <key>") };
            with_tile(e, |t, e| t.save_prefab_for_kit(e, &key, None).map_or_else(fail, ok))
        },
    );

    c.add("prefab_save", "prefab_save <name> [folder]", "save the selected instance group as a new prefab (renamed after it)", |e, a| {
        let Some(name) = a.first().cloned() else { return fail("usage: prefab_save <name> [folder]") };
        let folder = a.get(1).cloned().unwrap_or_default();
        let Some(g) = selected_group(e) else { return fail("select an instance group") };
        with_tile(e, |t, e| t.save_as_prefab(e, g, &name, &folder).map_or_else(fail, ok))
    });
}

/// The first selected group (a UserInstanceGroup).
fn selected_group(e: &Editor) -> Option<usize> {
    let d = e.docs.find_as::<TileDocument>(crate::tool::document_id(e)?)?;
    selected(e).into_iter().find(|&el| d.tile().tscene().type_of(el) == "UserInstanceGroup")
}
