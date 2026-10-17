//! The UI tool's commands: what the menu and the canvas do, for scripts, plus checks (ui_assert_*) for the scripted gates.

use std::path::Path;

use wce_editor::editor::Editor;
use wce_editor_core::command_registry::{CommandRegistry, CommandResult};
use wce_foundation::log;
use wce_scene::render_scene::DocumentId;

use crate::tool::{UiTool, current, tool_doc, tool_select, with_ui};

fn ok(m: impl Into<String>) -> CommandResult {
    CommandResult::success(m)
}

fn fail(m: impl Into<String>) -> CommandResult {
    CommandResult::fail(m)
}

fn int(s: &str) -> Option<i32> {
    s.trim().parse().ok()
}

/// An entry by number or by name (the first of that name).
fn find_entry(e: &Editor, token: &str) -> Result<(DocumentId, usize), String> {
    let (id, _) = current(e).ok_or("no UI layout is open")?;
    let ents = UiTool::entries_of(e, id);
    if let Some(i) = int(token).filter(|&i| i >= 0 && (i as usize) < ents.len()) {
        return Ok((id, i as usize));
    }

    ents.iter().position(|en| en.name == token).map(|i| (id, i)).ok_or_else(|| format!("no component '{token}'"))
}

pub fn register(c: &mut CommandRegistry<Editor>) {
    c.add("ui_open", "ui_open <path>", "Opens a UI layout from the game packs (or a file on disk)", |e, a| {
        let Some(p) = a.first() else { return fail("usage: ui_open <path>") };
        let from_packs = !Path::new(p).is_file();
        match e.open_path(p, from_packs, true) {
            Ok(m) => ok(m).wait(2),
            Err(m) => fail(m),
        }
    });

    c.add("ui_tree", "ui_tree", "Logs the components of the open layout", |e, _| {
        let Some((id, _)) = current(e) else { return fail("no UI layout is open") };
        for (i, en) in UiTool::entries_of(e, id).iter().enumerate() {
            log::info(format!("{}[{}] {} ({}) @{},{} {}x{} screen {},{}", "  ".repeat(en.depth), i, en.name, en.class, en.x, en.y, en.w, en.h, en.ax, en.ay));
        }

        ok("listed")
    });

    c.add("ui_select", "ui_select <number|name>", "Selects a component", |e, a| {
        let Some(t) = a.first() else { return fail("usage: ui_select <number|name>") };
        match find_entry(e, t) {
            Ok((id, i)) => {
                tool_select(e, id, i);
                ok(format!("selected {i}"))
            }
            Err(m) => fail(m),
        }
    });

    c.add("ui_set", "ui_set x|y|w|h|name|text <value>", "Changes the selected component", |e, a| {
        if a.len() < 2 {
            return fail("usage: ui_set x|y|w|h|name|text <value>");
        }

        let Some((id, Some(i))) = current(e) else { return fail("no component is selected") };
        let Some(en) = UiTool::entries_of(e, id).get(i).cloned() else { return fail("the selection is gone") };
        let value = a[1..].join(" ");
        let what = a[0].as_str();
        let Some(doc) = tool_doc(e, id) else { return fail("no UI layout is open") };
        let r = doc.edit(&format!("Set {what}"), 0, |m| match what {
            "x" | "y" => {
                let v = int(&value).ok_or("not a number")?;
                m.set_offset(&en.path, if what == "x" { v } else { en.x }, if what == "y" { v } else { en.y })
            }
            "w" | "h" => {
                let v = int(&value).ok_or("not a number")?;
                m.set_size(&en.path, if what == "w" { v } else { en.w }, if what == "h" { v } else { en.h })
            }
            "name" => m.set_name(&en.path, &value),
            "text" => m.set_text(&en.path, &value),
            other => Err(format!("unknown property '{other}'")),
        });
        match r {
            Ok(()) => ok("ok"),
            Err(m) => fail(m),
        }
    });

    c.add("ui_add", "ui_add [name]", "Adds a child to the selected component", |e, a| {
        let name = a.first().cloned().unwrap_or_else(|| "new_component".to_owned());
        with_ui(e, |t, e| match t.op_add(e, &name) {
            Ok(()) => ok("added"),
            Err(m) => fail(m),
        })
    });

    c.add("ui_delete", "ui_delete", "Deletes the selected component", |e, _| {
        with_ui(e, |t, e| {
            t.op_delete(e);
            ok("deleted")
        })
    });

    c.add("ui_dup", "ui_dup", "Duplicates the selected component", |e, _| {
        with_ui(e, |t, e| {
            t.op_duplicate(e);
            ok("duplicated")
        })
    });

    c.add("ui_order", "ui_order <delta>", "Moves the selected component in the paint order", |e, a| {
        let d = a.first().and_then(|s| int(s)).unwrap_or(1);
        with_ui(e, |t, e| match t.op_reorder(e, d) {
            Ok(()) => ok("moved"),
            Err(m) => fail(m),
        })
    });

    c.add("ui_into_previous", "ui_into_previous", "Moves the selected component into the sibling before it", |e, _| {
        with_ui(e, |t, e| match t.op_into_previous(e) {
            Ok(()) => ok("moved"),
            Err(m) => fail(m),
        })
    });

    c.add("ui_move_out", "ui_move_out", "Moves the selected component out of its parent", |e, _| {
        with_ui(e, |t, e| match t.op_move_out(e) {
            Ok(()) => ok("moved"),
            Err(m) => fail(m),
        })
    });

    c.add("ui_zoom", "ui_zoom <factor>", "Canvas zoom (1 = 100%)", |e, a| {
        let z = a.first().and_then(|s| s.parse::<f32>().ok()).unwrap_or(1.0);
        with_ui(e, |t, _| {
            t.op_zoom(z);
            ok("zoom")
        })
    });

    c.add("ui_fit", "ui_fit", "Fits the layout in the canvas", |e, _| {
        with_ui(e, |t, _| {
            t.op_fit();
            ok("fit")
        })
    });

    c.add("ui_outlines", "ui_outlines on|off", "Outline every component", |e, a| {
        let on = a.first().map(|s| s == "on").unwrap_or(true);
        with_ui(e, |t, _| {
            t.op_outlines(on);
            ok("outlines")
        })
    });

    c.add("ui_skin", "ui_skin <skin folder|default>", "Draws the layout with a UI skin ", |e, a| {
        let name = a.first().cloned().unwrap_or_default();
        with_ui(e, |t, e| match t.op_skin(e, &name) {
            Ok(s) => ok(format!("skin {s}")),
            Err(m) => fail(m),
        })
    });

    c.add("ui_screen", "ui_screen <WxH|none>", "Sizes the layout's root to a screen, as the game does: docked components move with it", |e, a| {
        let v = a.first().cloned().unwrap_or_else(|| "none".into());
        let size = if v == "none" {
            None
        } else {
            match v.to_ascii_lowercase().split_once('x').and_then(|(w, h)| Some((w.parse::<i32>().ok()?, h.parse::<i32>().ok()?))) {
                Some(s) => Some(s),
                None => return fail("usage: ui_screen <WxH|none>, like 1920x1080"),
            }
        };

        with_ui(e, |t, _| {
            t.op_screen(size);
            ok("screen")
        })
    });

    c.add("ui_related", "ui_related on|off [all]", "Draws the layouts this one is used with, faded, as reference", |e, a| {
        let on = a.first().map(|s| s == "on").unwrap_or(true);
        let generic = a.get(1).is_some_and(|s| s == "all");
        with_ui(e, |t, _| {
            t.op_related(on, generic);
            ok("related")
        })
    });

    c.add("ui_assert_related", "ui_assert_related <min items>", "Fails unless at least that many reference pictures and texts are drawn", |e, a| {
        let Some((id, _)) = current(e) else { return fail("no UI layout is open") };
        let want = a.first().and_then(|s| int(s)).unwrap_or(1) as usize;
        with_ui(e, |t, e| {
            let n = t.related_count(e, id);
            if n >= want { ok(format!("{n} reference items")) } else { fail(format!("{n} reference items drawn, wanted at least {want}")) }
        })
    });

    c.add("ui_animate", "ui_animate <number|name> <animation> <ms> | ui_animate none", "Previews an animation of a component at a moment", |e, a| {
        if a.first().map(String::as_str) == Some("none") {
            let Some((id, _)) = current(e) else { return fail("no UI layout is open") };
            return with_ui(e, |t, e| match t.op_animate(e, id, None) {
                Ok(m) => ok(m),
                Err(m) => fail(m),
            });
        }

        if a.len() < 3 {
            return fail("usage: ui_animate <number|name> <animation> <ms>");
        }

        let (id, i) = match find_entry(e, &a[0]) {
            Ok(v) => v,
            Err(m) => return fail(m),
        };

        let path = UiTool::entries_of(e, id)[i].path.clone();
        let t_ms = a[2].parse::<f32>().unwrap_or(0.0);
        let name = a[1].clone();
        with_ui(e, |t, e| match t.op_animate(e, id, Some((path, name, t_ms))) {
            Ok(m) => ok(m),
            Err(m) => fail(m),
        })
    });

    c.add("ui_state", "ui_state <name|none>", "Draws the state of that name wherever a component has it", |e, a| {
        let n = a.first().cloned().filter(|s| s != "none");
        with_ui(e, |t, _| {
            t.op_state(n);
            ok("state")
        })
    });

    c.add("ui_show_hidden", "ui_show_hidden on|off", "Draw components the layout hides when it loads", |e, a| {
        let on = a.first().map(|s| s == "on").unwrap_or(true);
        with_ui(e, |t, _| {
            t.op_show_hidden(on);
            ok("show_hidden")
        })
    });

    c.add("ui_show_templates", "ui_show_templates on|off", "Draw components the game fills in at runtime", |e, a| {
        let on = a.first().map(|s| s == "on").unwrap_or(true);
        with_ui(e, |t, _| {
            t.op_show_templates(on);
            ok("show_templates")
        })
    });

    c.add("ui_preview", "ui_preview <number|name> show|hide|reset|state <name>", "Preview a component hidden, shown or in another state", |e, a| {
        if a.len() < 2 {
            return fail("usage: ui_preview <number|name> show|hide|reset|state <name>");
        }

        let (id, i) = match find_entry(e, &a[0]) {
            Ok(v) => v,
            Err(m) => return fail(m),
        };

        let path = UiTool::entries_of(e, id)[i].path.clone();
        let state_idx = if a[1] == "state" {
            let Some(want) = a.get(2) else { return fail("usage: ui_preview <entry> state <name>") };
            match tool_doc(e, id).and_then(|d| d.model().entry_state_names(&path).iter().position(|n| n == want)) {
                Some(k) => Some(k),
                None => return fail(format!("no state '{want}'")),
            }
        } else {
            None
        };

        let verb = a[1].clone();
        with_ui(e, |t, _| {
            match verb.as_str() {
                "show" => t.op_visibility(path, Some(true)),
                "hide" => t.op_visibility(path, Some(false)),
                "reset" => {
                    t.op_visibility(path.clone(), None);
                    t.op_entry_state(path, None);
                }
                "state" => t.op_entry_state(path, state_idx),
                other => return fail(format!("unknown preview action '{other}'")),
            }

            ok("preview")
        })
    });

    c.add("ui_assert_shown", "ui_assert_shown <n>", "Fails unless n components are drawn with the current options", |e, a| {
        let Some((id, _)) = current(e) else { return fail("no UI layout is open") };
        let want = a.first().and_then(|s| int(s));
        with_ui(e, |t, e| {
            let n = t.shown_count(e, id);
            match want {
                Some(w) if w as usize == n => ok(format!("{n} shown")),
                _ => fail(format!("{n} components are drawn")),
            }
        })
    });

    c.add("ui_set_xml", "ui_set_xml <file>", "Replaces the layout's XML with a file's text", |e, a| {
        let Some(p) = a.first() else { return fail("usage: ui_set_xml <file>") };
        let Ok(text) = std::fs::read_to_string(p) else { return fail(format!("cannot read {p}")) };
        let Some((id, _)) = current(e) else { return fail("no UI layout is open") };
        match tool_doc(e, id).map(|d| d.set_xml(&text, 0)) {
            Some(Ok(())) => ok("applied"),
            Some(Err(m)) => fail(m),
            None => fail("no UI layout is open"),
        }
    });

    c.add("ui_save_xml", "ui_save_xml <file>", "Writes the layout's XML to a file", |e, a| {
        let Some(p) = a.first() else { return fail("usage: ui_save_xml <file>") };
        let Some((id, _)) = current(e) else { return fail("no UI layout is open") };
        match tool_doc(e, id).map(|d| std::fs::write(p, d.xml().as_bytes())) {
            Some(Ok(())) => ok(format!("wrote {p}")),
            Some(Err(m)) => fail(m.to_string()),
            None => fail("no UI layout is open"),
        }
    });

    c.add("ui_save_pack", "ui_save_pack <pack> [path inside]", "Writes the layout into a mod pack", |e, a| {
        let Some(pack) = a.first() else { return fail("usage: ui_save_pack <pack> [path inside]") };
        let Some((id, _)) = current(e) else { return fail("no UI layout is open") };
        let Some(doc) = tool_doc(e, id) else { return fail("no UI layout is open") };
        let inside = a.get(1).cloned().unwrap_or_else(|| doc.source.vfs_path.clone());
        if inside.is_empty() {
            return fail("say where the file goes inside the pack");
        }

        match doc.save_to_pack(Path::new(pack), &inside) {
            Ok(()) => ok(format!("saved {inside} into {pack}")),
            Err(m) => fail(m),
        }
    });

    c.add("ui_assert_entries", "ui_assert_entries <n>", "Fails unless the layout has n components", |e, a| {
        let Some((id, _)) = current(e) else { return fail("no UI layout is open") };
        let n = UiTool::entries_of(e, id).len();
        match a.first().and_then(|s| int(s)) {
            Some(want) if want as usize == n => ok(format!("{n} components")),
            _ => fail(format!("expected {:?} components, have {n}", a.first())),
        }
    });

    c.add("ui_assert_rect", "ui_assert_rect <number|name> <x> <y> <w> <h>", "Fails unless the component has that rectangle", |e, a| {
        if a.len() < 5 {
            return fail("usage: ui_assert_rect <number|name> <x> <y> <w> <h>");
        }

        let (id, i) = match find_entry(e, &a[0]) {
            Ok(v) => v,
            Err(m) => return fail(m),
        };

        let en = &UiTool::entries_of(e, id)[i];
        let want: Vec<Option<i32>> = a[1..5].iter().map(|s| int(s)).collect();
        if want == [Some(en.x), Some(en.y), Some(en.w), Some(en.h)] {
            ok("rect ok")
        } else {
            fail(format!("{} is at {},{} {}x{}, expected {:?}", en.name, en.x, en.y, en.w, en.h, want))
        }
    });

    c.add("ui_assert_screen_rect", "ui_assert_screen_rect <number|name> <x> <y> <w> <h>", "Fails unless the game would put the component there with the current screen size and animation", |e, a| {
        if a.len() < 5 {
            return fail("usage: ui_assert_screen_rect <number|name> <x> <y> <w> <h>");
        }

        let (id, i) = match find_entry(e, &a[0]) {
            Ok(v) => v,
            Err(m) => return fail(m),
        };

        let want: Vec<Option<i32>> = a[1..5].iter().map(|s| int(s)).collect();
        with_ui(e, |t, e| match t.rect_of(e, id, i) {
            Some(r) => {
                let got = [r.x.round() as i32, r.y.round() as i32, r.w.round() as i32, r.h.round() as i32];
                if want == got.map(Some) { ok("screen rect ok") } else { fail(format!("component {i} is at {got:?} on screen, expected {want:?}")) }
            }
            None => fail("no such component"),
        })
    });

    c.add("ui_assert_xml_roundtrip", "ui_assert_xml_roundtrip", "Fails unless the layout converts to binary and back to the same XML", |e, _| {
        let Some((id, _)) = current(e) else { return fail("no UI layout is open") };
        let Some(doc) = tool_doc(e, id) else { return fail("no UI layout is open") };
        match doc.bytes().and_then(|b| wce_ui_model::UiModel::from_bytes(&b)) {
            Ok(m) if m.to_xml() == *doc.xml() => ok("round trip ok"),
            Ok(_) => fail("the binary reads back as different XML"),
            Err(m) => fail(m),
        }
    });
}
