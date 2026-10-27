//! The editor's built-in commands: the menu actions, the script controls and the test assertions.

use std::path::PathBuf;
use std::time::Instant;

use dear_imgui_sys as sys;
use wce_editor_core::camera_controller::CameraController;
use wce_editor_core::command_registry::{CommandRegistry, CommandResult, read_script};
use wce_editor_core::document::Document;
use wce_foundation::log::{self, LogLevel};
use wce_foundation::math::{Float3, cross, length, normalize, translation};
use wce_foundation::profile;
use wce_render::renderer::Renderer;
use wce_scene::camera::Camera;
use wce_scene::render_scene::{DocumentId, SourceRef};

use crate::editor::{Answer, Editor, PromptKind};
use crate::gui;
use crate::viewport::{self, GizmoMode};

fn ok(m: impl Into<String>) -> CommandResult {
    CommandResult::success(m)
}

fn fail(m: impl Into<String>) -> CommandResult {
    CommandResult::fail(m)
}

/// The whole string as a float (strtof with nothing left over).
fn parse_float(s: &str) -> Option<f32> {
    s.parse().ok()
}

/// The whole string as an unsigned number.
fn parse_u64(s: &str) -> Option<u64> {
    s.parse().ok()
}

/// C's atoi: the leading integer, 0 when there is none.
fn atoi(s: &str) -> i32 {
    let s = s.trim_start();
    let digits = s.char_indices().take_while(|&(i, c)| c.is_ascii_digit() || (i == 0 && (c == '-' || c == '+'))).last().map_or(0, |(i, c)| i + c.len_utf8());
    s[..digits].parse::<i64>().map_or(0, |v| v.clamp(i32::MIN as i64, i32::MAX as i64) as i32)
}

/// C's atof: the longest leading float, 0 when there is none.
fn atof(s: &str) -> f64 {
    let s = s.trim_start();
    (1..=s.len()).rev().filter(|&n| s.is_char_boundary(n)).find_map(|n| s[..n].parse::<f64>().ok()).unwrap_or(0.0)
}

/// A document is named by its id, "active", its kind (the first one: tile, environment, ...) or part of its title.
fn resolve_doc(e: &Editor, spec: &str) -> Option<DocumentId> {
    if spec.is_empty() || spec == "active" {
        return e.docs.active().map(|d| d.id());
    }

    if let Some(id) = parse_u64(spec) {
        return e.docs.find(id as DocumentId).map(|d| d.id());
    }

    let order = e.docs.order();
    let find = |f: &dyn Fn(&dyn Document) -> bool| order.iter().copied().find(|&id| e.docs.find(id).is_some_and(f));
    find(&|d| d.kind() == spec).or_else(|| {
        let want = spec.to_lowercase();
        find(&|d| d.title().to_lowercase().contains(&want))
    })
}

const PANELS: [&str; 7] = ["viewport", "outliner", "inspector", "assets", "log", "history", "commands"];
const PANEL_TITLES: [&str; 7] = ["Viewport", "Outliner", "Inspector", "Assets", "Log", "History", "Commands"];

fn panel_flag<'a>(e: &'a mut Editor, name: &str) -> Option<&'a mut bool> {
    let p = &mut e.panels;
    Some(match name {
        "viewport" => &mut p.viewport,
        "outliner" => &mut p.outliner,
        "inspector" => &mut p.inspector,
        "assets" => &mut p.assets,
        "log" => &mut p.log,
        "history" => &mut p.history,
        "commands" => &mut p.commands,
        _ => return None,
    })
}

fn first_selected_transform(e: &Editor) -> Option<(SourceRef, wce_foundation::math::Float4x4)> {
    let first = *e.selection.items().first()?;
    e.docs.find(first.doc)?.item_transform(first).map(|w| (first, w))
}

pub fn register_builtin(c: &mut CommandRegistry<Editor>) {
    c.add("help", "help [name]", "list the commands, or show one", |e, a| {
        if let Some(name) = a.first() {
            return e.commands().find(name).map_or_else(|| fail("unknown command"), |x| ok(format!("{} - {}", x.usage, x.help)));
        }

        ok(e.commands().entries().map(|x| format!("{}\n", x.usage)).collect::<String>())
    });

    c.add("echo", "echo text...", "write to the log", |_, a| {
        log::info(a.join(" "));
        ok("")
    });

    c.add("perf", "perf [reset | full]", "frame times (mean / p95 / worst) of the last frames, the GPU time per pass and, with `full`, the CPU scope timers", |e, a| {
        if a.first().is_some_and(|x| x == "reset") {
            e.perf.reset();
            profile::reset();
            if let Some(r) = e.renderer() {
                r.gpu_timer().reset();
            }

            return ok("perf counters reset");
        }

        let mut r = e.perf.report();
        if let Some(rd) = e.renderer() {
            let s = *rd.draw_stats();
            r += &format!(
                "  draw set: {} items, {} models, {} batches, {} models still loading | last frame: {} views, {} instances, {} draw calls, {:.2} M triangles (camera view: {} instances incl. {} terrain patches, {:.2} M triangles; shadow maps: {} instances, {:.2} M triangles)\n",
                s.items, s.models, s.batches, s.waiting_models, s.views, s.instances, s.draw_calls, s.triangles as f64 * 1e-6, s.camera_instances, s.terrain_patches,
                s.camera_triangles as f64 * 1e-6, s.shadow_instances, s.shadow_triangles as f64 * 1e-6,
            );
            r += &rd.gpu_timer().report();
        }

        if a.first().is_some_and(|x| x == "full") {
            r += "  CPU scopes:\n";
            r += &profile::report(false, 0.05);
        }

        log::info(format!("perf:\n{r}"));
        ok(r)
    });

    c.add("profile", "profile [reset]", "the CPU scope timers (total, calls, worst) sorted by time", |_, a| {
        let r = profile::report(a.first().is_some_and(|x| x == "reset"), 0.01);
        log::info(format!("profile:\n{r}"));
        ok(r)
    });

    c.add("wait_loaded", "wait_loaded [timeout seconds]", "script: wait until every model and texture the scene needs has loaded (reports how long that took)", |e, a| {
        let now = Instant::now();
        let start = *e.wait_loaded_start.get_or_insert(now);
        let timeout = a.first().map_or(60.0, |t| atof(t));
        let elapsed = now.duration_since(start).as_secs_f64();
        let busy = e.renderer().is_some_and(|r| r.models().pending() > 0 || r.textures().upgrades_pending() > 0);
        if busy && elapsed < timeout {
            return ok("").again();
        }

        e.wait_loaded_start = None;
        ok(format!("{} after {elapsed:.2} s", if busy { "still loading" } else { "loaded" }))
    });

    c.add(
        "gfx", "gfx <shadows|ssao|fog|clouds|lens_flare|vignette|depth_of_field|distortion> on|off | gfx lod_scale <x> | gfx min_pixels <x> | gfx view <final|hdr|albedo|normal|specular|depth|ao>",
        "renderer features and the detail settings of the tile views (speed against quality)", |e, a| {
            let Some(r) = e.renderer() else { return fail("no renderer") };
            if a.len() != 2 {
                return fail("usage: gfx <name> on|off");
            }

            match a[0].as_str() {
                "lod_scale" => r.settings.lod_scale = atof(&a[1]) as f32,
                "min_pixels" => r.settings.min_pixels = atof(&a[1]) as f32,
                "shadow_min_texels" => r.settings.shadow_min_texels = atof(&a[1]) as f32,
                "view" => {
                    use wce_render::renderer::View;
                    r.settings.view = match a[1].as_str() {
                        "final" => View::Final,
                        "hdr" => View::HdrRaw,
                        "albedo" => View::Albedo,
                        "normal" => View::Normal,
                        "specular" => View::Specular,
                        "depth" => View::Depth,
                        "ao" => View::Ao,
                        _ => return fail(format!("unknown view '{}' (final, hdr, albedo, normal, specular, depth, ao)", a[1])),
                    };
                }

                _ => {
                    let on = a[1] != "off" && a[1] != "0";
                    let flag = match a[0].as_str() {
                        "shadows" => &mut r.settings.shadows,
                        "ssao" => &mut r.settings.ssao,
                        "fog" => &mut r.settings.fog,
                        "clouds" => &mut r.settings.clouds,
                        "lens_flare" => &mut r.settings.lens_flare,
                        "vignette" => &mut r.settings.vignette,
                        "depth_of_field" => &mut r.settings.depth_of_field,
                        "distortion" => &mut r.settings.distortion,
                        _ => return fail(format!("unknown feature '{}'", a[0])),
                    };

                    *flag = on;
                    return ok(format!("{}{}", a[0], if on { " on" } else { " off" }));
                }
            }

            ok("")
        },
    );

    c.add("frame_cap", "frame_cap [fps | 0]", "the frame rate cap of interactive sessions (0: none), kept in the settings; no argument: log it", |e, a| {
        match a.first() {
            None => ok(format!("frame cap {}", e.frame_cap())),
            Some(v) => match v.parse::<u32>() {
                Ok(fps) => {
                    e.set_frame_cap(fps);
                    ok(format!("frame cap {fps}"))
                }

                Err(_) => fail("usage: frame_cap [fps | 0]"),
            },
        }
    });

    c.add("vsync", "vsync on|off", "present with or without waiting for the monitor (off: frame rate is the real cost of a frame)", |e, a| {
        if a.len() != 1 {
            return fail("usage: vsync on|off");
        }

        e.vsync = a[0] != "off" && a[0] != "0";
        ok(if e.vsync { "vsync on" } else { "vsync off" })
    });

    c.add("frames", "frames N", "script: wait N rendered frames", |_, a| match a.first().and_then(|x| parse_u64(x)) {
        Some(n) => ok("").wait(n.min(100_000) as i32),
        None => fail("usage: frames N"),
    });

    c.add("quit", "quit [exit code]", "leave the editor", |e, a| {
        let code = a.first().and_then(|x| parse_u64(x)).unwrap_or(0);
        e.request_quit(code as i32);
        ok("")
    });

    c.add("tool", "tool <id>", "switch the active tool (weather, tile, ...)", |e, a| {
        let Some(id) = a.first() else { return fail("usage: tool <id>") };
        if e.set_tool(id) { ok("") } else { fail(format!("no tool '{id}'")) }
    });

    c.add("assert_tool", "assert_tool <id>", "test: the active tool is <id> (after a frame)", |e, a| {
        let Some(id) = a.first() else { return fail("usage: assert_tool <id>") };
        let active = e.active_tool_id();
        if !active.is_empty() && active == id { ok("") } else { fail(format!("active tool is {}", if active.is_empty() { "none" } else { active })) }
    });

    c.add("open", "open <path> [disk]", "open a file from the game packs (or from disk)", |e, a| {
        let Some(path) = a.first() else { return fail("usage: open <path> [disk]") };
        let packs = !(a.len() > 1 && a[1] == "disk");
        e.open_path(path, packs, true).map_or_else(fail, ok)
    });

    // Closing asks about unsaved changes (the menu opens a prompt; a script says what to do with them).
    c.add("close", "close [document] [save|discard]", "close a document (default the active one); with unsaved changes it needs `save` or `discard`", |e, a| {
        let mut how = "";
        let mut spec = "";
        for w in a {
            if w == "save" || w == "discard" {
                how = w;
            } else {
                spec = w;
            }
        }

        let Some(id) = resolve_doc(e, spec) else { return fail("no document") };
        let d = e.docs.find(id).expect("resolved");
        if d.modified() {
            if how.is_empty() {
                return fail(format!("{} has unsaved changes: close save | close discard", d.title()));
            }

            if how == "save"
                && let Err(msg) = e.docs.save_one(id)
            {
                return fail(msg);
            }
        }

        e.close_document(id);
        ok("")
    });

    c.add("close_all", "close_all [save|discard]", "close every document; with unsaved changes it needs `save` or `discard`", |e, a| {
        let unsaved = e.docs.unsaved();
        if !unsaved.is_empty() && a.is_empty() {
            return fail(format!("{} document(s) have unsaved changes: close_all save | close_all discard", unsaved.len()));
        }

        if !unsaved.is_empty() && a[0] == "save" {
            let mut msgs = vec![];
            if !e.docs.save_all(&mut msgs) {
                return fail(msgs.pop().unwrap_or_else(|| "cannot save".to_owned()));
            }
        }

        for id in e.docs.order().to_vec() {
            e.close_document(id);
        }

        ok("")
    });

    c.add("close_prompt", "close_prompt [document]", "what File > Close document does: closes at once, or opens the unsaved-changes prompt", |e, a| {
        let Some(id) = resolve_doc(e, a.first().map_or("", String::as_str)) else { return fail("no document") };
        e.ask_close(id);
        ok("")
    });

    c.add("quit_prompt", "quit_prompt", "what File > Quit and the window's close button do: quits at once, or opens the unsaved-changes prompt", |e, _| {
        e.ask_quit();
        ok("")
    });

    c.add("answer", "answer save|discard|cancel", "answer the unsaved-changes prompt", |e, a| {
        let answer = match a.first().map(String::as_str) {
            Some("save") => Answer::Save,
            Some("discard") => Answer::Discard,
            Some("cancel") => Answer::Cancel,
            _ => return fail("usage: answer save|discard|cancel"),
        };

        e.answer_prompt(answer).map_or_else(fail, ok)
    });

    c.add(
        "assert_prompt", "assert_prompt none|close|quit [N]",
        "test: the unsaved-changes prompt is closed / asking about closing / asking about quitting (about N documents)", |e, a| {
            let Some(want) = a.first() else { return fail("usage: assert_prompt none|close|quit [N]") };
            let want = match want.as_str() {
                "close" => PromptKind::Close,
                "quit" => PromptKind::Quit,
                _ => PromptKind::None,
            };

            let kind = e.prompt_kind();
            if kind != want {
                let now = match kind {
                    PromptKind::Close => "close",
                    PromptKind::Quit => "quit",
                    PromptKind::None => "closed",
                };

                return fail(format!("the prompt is {now}"));
            }

            let listed = e.prompt_documents().len();
            if a.len() > 1 && listed as u64 != a[1].parse::<u64>().unwrap_or(0) {
                return fail(format!("the prompt lists {listed} documents"));
            }

            ok("")
        },
    );

    c.add("expect_fail", "expect_fail <command...>", "test: the command fails (the script goes on)", |e, a| {
        let Some(name) = a.first() else { return fail("usage: expect_fail <command...>") };
        let r = CommandRegistry::run_args(e, name, &a[1..]);
        if r.ok { fail(format!("'{name}' succeeded: {}", r.message)) } else { ok(r.message) }
    });

    c.add("activate", "activate <document>", "make a document the active one (id, kind or part of the title); `tool` switches the tool", |e, a| {
        let Some(spec) = a.first() else { return fail("usage: activate <id|kind|title>") };
        let Some(id) = resolve_doc(e, spec) else { return fail(format!("no document '{spec}'")) };
        e.docs.set_active(id);
        ok(e.docs.find(id).map(|d| d.title()).unwrap_or_default())
    });

    c.add("documents", "documents", "list the open documents (id, tool, kind, title, unsaved, visible)", |e, _| {
        let mut s = String::new();
        for &id in e.docs.order() {
            let Some(d) = e.docs.find(id) else { continue };
            s += &format!(
                "{id}  {}  {}  {}{}{}{}{}\n", e.docs.tool_of(id), d.kind(), d.title(), if d.modified() { "  *" } else { "" },
                if e.docs.visible(&e.scene, id) { "" } else { "  (hidden)" }, if id == e.docs.active_id() { "  <active>" } else { "" },
                if Some(id) == e.weather_preview_document() { "  <weather preview>" } else { "" },
            );
        }

        log::info(format!("documents:\n{s}"));
        ok(s)
    });

    c.add("layer_visible", "layer_visible <document> 0|1", "show or hide the layer of a document (the outliner's checkbox)", |e, a| {
        if a.len() < 2 {
            return fail("usage: layer_visible <document> 0|1");
        }

        let Some(id) = resolve_doc(e, &a[0]) else { return fail(format!("no document '{}'", a[0])) };
        e.docs.set_visible(&mut e.scene, id, a[1] != "0");
        ok("")
    });

    c.add("assert_layer_visible", "assert_layer_visible <document> 0|1", "test: the layer of a document is (not) shown", |e, a| {
        if a.len() < 2 {
            return fail("usage: assert_layer_visible <document> 0|1");
        }

        let Some(id) = resolve_doc(e, &a[0]) else { return fail(format!("no document '{}'", a[0])) };
        let shown = e.docs.visible(&e.scene, id);
        if shown == (a[1] != "0") { ok("") } else { fail(format!("the layer is {}", if shown { "shown" } else { "hidden" })) }
    });

    c.add(
        "assert_env", "assert_env <field> <value>",
        "test: the environment the viewport shows has this value (sun_elevation, sun_azimuth, fog_distance_start, has_sky, blend_t, sun_scale)", |e, a| {
            if a.len() < 2 {
                return fail("usage: assert_env <field> <value>");
            }

            let env = e.scene.environment.resolved();
            let now = match a[0].as_str() {
                "sun_elevation" => env.sun_elevation,
                "sun_azimuth" => env.sun_azimuth,
                "fog_distance_start" => env.fog_distance_start,
                "has_sky" => f32::from(u8::from(env.has_sky)),
                "sun_scale" => env.sun_scale,
                "blend_t" => e.scene.environment.blend_t,
                other => return fail(format!("unknown field '{other}'")),
            };

            let want = atof(&a[1]) as f32;
            if (now - want).abs() < 1e-3 * want.abs().max(1.0) { ok("") } else { fail(format!("{} is {now:.6}", a[0])) }
        },
    );

    c.add("assert_preview", "assert_preview <file | none>", "test: the weather preview is this file of the packs (or off)", |e, a| {
        let Some(spec) = a.first() else { return fail("usage: assert_preview <file|none>") };
        let now = e.weather_preview().unwrap_or("none").to_owned();
        let want = if spec == "none" { "none".to_owned() } else { wce_vfs::normalize(spec) };
        if now == want { ok("") } else { fail(format!("the preview is {now}")) }
    });

    c.add("save", "save [document]", "save a document (default the active one)", |e, a| {
        let Some(id) = resolve_doc(e, a.first().map_or("", String::as_str)) else { return fail("no document") };
        e.docs.save_one(id).map_or_else(fail, ok)
    });

    c.add("save_all", "save_all", "save every modified document", |e, _| {
        let mut msgs = vec![];
        let all = e.docs.save_all(&mut msgs);
        let s = msgs.join("; ");
        if all { ok(s) } else { fail(s) }
    });

    c.add("undo", "undo [n]", "undo the last edit (or n edits) of the active document", |e, a| {
        let n = a.first().map_or(1, |x| atoi(x).max(1));
        let mut any = false;
        if let Some(d) = e.docs.active_mut() {
            for _ in 0..n {
                any = d.history_mut().undo() || any;
            }
        }

        if any { ok("") } else { fail("nothing to undo") }
    });

    c.add("redo", "redo [n]", "redo (n edits)", |e, a| {
        let n = a.first().map_or(1, |x| atoi(x).max(1));
        let mut any = false;
        if let Some(d) = e.docs.active_mut() {
            for _ in 0..n {
                any = d.history_mut().redo() || any;
            }
        }

        if any { ok("") } else { fail("nothing to redo") }
    });

    c.add("select", "select none | select [add] <item>...", "select items of the active document", |e, a| {
        let Some(doc) = e.docs.active().map(|d| d.id()) else { return fail("no document") };
        if a.first().is_some_and(|x| x == "none") {
            e.selection.clear();
            return ok("");
        }

        let add = a.first().is_some_and(|x| x == "add");
        if !add {
            e.selection.clear();
        }

        let items = &a[usize::from(add)..];
        if items.is_empty() {
            return fail("usage: select none | select [add] <item>...");
        }

        for x in items {
            let Some(item) = parse_u64(x) else { return fail(format!("bad item '{x}'")) };
            e.selection.add(SourceRef { doc, item });
        }

        ok("")
    });

    c.add("move", "move <item> dx dy dz", "move an item of the active document by a world-space offset (one undo entry)", |e, a| {
        let parsed = (a.len() >= 4).then(|| Some((parse_u64(&a[0])?, parse_float(&a[1])?, parse_float(&a[2])?, parse_float(&a[3])?))).flatten();
        let (Some(d), Some((item, x, y, z))) = (e.docs.active_mut(), parsed) else { return fail("usage: move <item> dx dy dz") };
        thread_local!(static GESTURE: std::cell::Cell<u64> = const { std::cell::Cell::new(1 << 40) });
        let gesture = GESTURE.with(|g| {
            g.set(g.get() + 1);
            g.get()
        });

        let r = SourceRef { doc: d.id(), item };
        d.transform_items(&[r], &translation(Float3::new(x, y, z)), gesture);
        d.history_mut().break_merge();
        ok("")
    });

    c.add("assert_selected", "assert_selected N", "test: N items are selected", |e, a| {
        let Some(n) = a.first().and_then(|x| parse_u64(x)) else { return fail("usage: assert_selected N") };
        let have = e.selection.items().len();
        if have as u64 == n { ok("") } else { fail(format!("selected {have}, expected {}", a[0])) }
    });

    c.add("assert_modified", "assert_modified 0|1", "test: the active document is (not) modified", |e, a| {
        let (Some(d), Some(want)) = (e.docs.active(), a.first()) else { return fail("usage: assert_modified 0|1 (needs a document)") };
        if d.modified() == (want == "1") { ok("") } else { fail(format!("modified is {}", if d.modified() { "1" } else { "0" })) }
    });

    c.add("assert_history", "assert_history size position", "test: the active document's undo stack has `size` entries, `position` applied", |e, a| {
        let want = (a.len() >= 2).then(|| Some((parse_u64(&a[0])?, parse_u64(&a[1])?))).flatten();
        let (Some(d), Some((s, p))) = (e.docs.active(), want) else { return fail("usage: assert_history size position") };
        let h = d.history();
        if h.size() as u64 == s && h.position() as u64 == p { ok("") } else { fail(format!("history is {}/{}", h.size(), h.position())) }
    });

    c.add("assert_documents", "assert_documents N", "test: N documents are open", |e, a| {
        let Some(n) = a.first().and_then(|x| parse_u64(x)) else { return fail("usage: assert_documents N") };
        let have = e.docs.order().len();
        if have as u64 == n { ok("") } else { fail(format!("documents: {have}")) }
    });

    c.add("cam", "cam yaw pitch dist [target_y]", "orbit camera, radians and metres (like weather_editor --cam)", |e, a| {
        let parsed = (a.len() >= 3).then(|| Some((parse_float(&a[0])?, parse_float(&a[1])?, parse_float(&a[2])?))).flatten();
        let Some((yaw, pitch, dist)) = parsed else { return fail("usage: cam yaw pitch dist [target_y]") };
        let ty = a.get(3).and_then(|x| parse_float(x)).unwrap_or(5.0);
        e.viewport.use_orbit();
        let o = e.viewport.orbit_mut();
        let t = o.target;
        o.set_orbit(yaw, pitch, dist, Float3::new(t.x, ty, t.z));
        ok("")
    });

    c.add("cam_look", "cam_look <x> <y> <z> | cam_look eye_x eye_y eye_z target_x target_y target_z", "orbit camera: move its target (metres), keeping its angles and distance; or from an eye to a target", |e, a| {
        let v: Vec<f32> = a.iter().filter_map(|x| parse_float(x)).collect();
        if v.len() != a.len() || !(v.len() == 3 || v.len() == 6) {
            return fail("usage: cam_look x y z | cam_look eye_x eye_y eye_z target_x target_y target_z");
        }

        e.viewport.use_orbit();
        if v.len() == 3 {
            e.viewport.orbit_mut().target = Float3::new(v[0], v[1], v[2]);
        } else {
            let cam = Camera { eye: Float3::new(v[0], v[1], v[2]), target: Float3::new(v[3], v[4], v[5]), ..e.viewport.camera() };
            e.viewport.orbit_mut().set_camera(&cam);
        }

        ok("")
    });

    c.add("cam_info", "cam_info", "test: log the camera, the first selected item's position and the active document's history (for repro scripts)", |e, _| {
        let c = e.viewport.camera();
        let mut s = format!("camera eye {:.2} {:.2} {:.2} target {:.2} {:.2} {:.2}", c.eye.x, c.eye.y, c.eye.z, c.target.x, c.target.y, c.target.z);
        if let Some((first, w)) = first_selected_transform(e) {
            s += &format!("; item {} at {:.3} {:.3} {:.3}", first.item, w.m[0][3], w.m[1][3], w.m[2][3]);
        }

        if let Some(d) = e.docs.active() {
            s += &format!("; history {}/{}", d.history().size(), d.history().position());
        }

        ok(s)
    });

    c.add("cam_mark", "cam_mark", "test: remember the camera (see assert_cam_same)", |e, _| {
        e.cam_mark = Some(e.viewport.camera());
        ok("")
    });

    c.add("assert_cam_same", "assert_cam_same", "test: the camera is where cam_mark left it (a gizmo drag must not orbit the view)", |e, _| {
        let c = e.viewport.camera();
        match e.cam_mark {
            Some(m) => {
                let d = length(c.eye - m.eye) + length(c.target - m.target);
                if d < 1e-4 { ok("") } else { fail(format!("the camera moved by {d:.6}")) }
            }

            None => fail("no cam_mark"),
        }
    });

    c.add("assert_cam_moved", "assert_cam_moved", "test: the camera is not where cam_mark left it", |e, _| {
        let c = e.viewport.camera();
        match e.cam_mark {
            Some(m) if length(c.eye - m.eye) + length(c.target - m.target) > 1e-3 => ok(""),
            _ => fail("the camera did not move"),
        }
    });

    c.add("camera", "camera orbit|fly", "camera controller", |e, a| {
        let Some(which) = a.first() else { return fail("usage: camera orbit|fly") };
        if which == "fly" {
            e.viewport.use_fly();
        } else {
            e.viewport.use_orbit();
        }

        ok("")
    });

    c.add("focus", "focus", "frame the selection in the viewport", |e, _| {
        viewport::focus_selection(e);
        ok("")
    });

    c.add("gizmo", "gizmo off|translate|rotate|scale", "gizmo mode", |e, a| {
        let Some(mode) = a.first() else { return fail("usage: gizmo off|translate|rotate|scale") };
        e.viewport.gizmo = match mode.as_str() {
            "translate" => GizmoMode::Translate,
            "rotate" => GizmoMode::Rotate,
            "scale" => GizmoMode::Scale,
            _ => GizmoMode::Off,
        };

        ok("")
    });

    c.add("gizmo_size", "gizmo_size [points]", "the gizmo's handles' length on screen, in points (kept in the settings); no argument: log it", |e, a| {
        match a.first() {
            None => ok(format!("{} points", e.settings_ref().appearance.gizmo_size)),
            Some(x) => match x.parse::<f32>() {
                Ok(v) if v > 0.0 => {
                    e.set_gizmo_size(v);
                    ok("")
                }

                _ => fail("usage: gizmo_size [points]"),
            },
        }
    });

    c.add("grid", "grid on|off", "viewport grid", |e, a| {
        e.viewport.show_grid = a.first().is_some_and(|x| x == "on");
        ok("")
    });

    c.add("compass", "compass [on|off]", "the compass at the top of the viewport; no argument: log the camera's heading (degrees from north, east 90)", |e, a| {
        match a.first().map(String::as_str) {
            Some("on") | Some("off") => {
                e.viewport.show_compass = a[0] == "on";
                ok("")
            }

            Some(_) => fail("usage: compass [on|off]"),
            None => ok(crate::viewport::heading(&e.viewport.camera(), e.viewport.north).map_or("looking straight down".to_owned(), |h| format!("heading {h:.1}"))),
        }
    });

    c.add("where", "where [x y | on | off]", "the camera's and the cursor's coordinates (scene and game units; map cell and height above the sea in a map context); x y: the ground under that pixel of the viewport image (or 50%); on / off: the readout in the viewport", |e, a| {
        match a.first().map(String::as_str) {
            Some("on") | Some("off") => {
                e.viewport.show_coords = a[0] == "on";
                return ok("");
            }

            Some(_) if a.len() == 2 => {
                let (w, h) = (e.viewport.width(), e.viewport.height());
                let px = |v: &str, n: u32| match v.strip_suffix('%') {
                    Some(p) => p.parse::<f32>().ok().map(|p| (p / 100.0 * n as f32) as i32),
                    None => v.parse::<i32>().ok(),
                };

                let (Some(x), Some(y)) = (px(&a[0], w), px(&a[1], h)) else { return fail("usage: where [x y | on | off]") };
                e.viewport.cursor_ground = e.viewport.ground_under(&e.scene, x, y);
            }

            Some(_) => return fail("usage: where [x y | on | off]"),
            None => {}
        }

        ok(e.viewport.coords_lines(&e.scene).join("
"))
    });

    c.add("window_focus", "window_focus <window>", "bring a window to the front (a docked one: its tab is selected), by its title", |e, a| {
        let Some(name) = a.first() else { return fail("usage: window_focus <window>") };
        e.pending_focus = Some(name.clone());
        ok("")
    });

    c.add("panel", "panel <name> on|off", "show or hide a panel (viewport outliner inspector assets log history commands)", |e, a| {
        if a.len() < 2 {
            return fail("usage: panel <name> on|off");
        }

        match panel_flag(e, &a[0]) {
            Some(f) => {
                *f = a[1] == "on";
                ok("")
            }

            None => fail(format!("no panel '{}'", a[0])),
        }
    });

    c.add("layout", "layout save|reset", "save or reset the active tool's dock layout", |e, a| {
        match a.first().map(String::as_str) {
            Some("save") => e.save_layout(),
            Some("reset") => e.reset_layout(),
            _ => return fail("usage: layout save|reset"),
        }

        ok("")
    });

    c.add("shot", "shot <file.png>", "save the whole window after this frame (the app loop does it)", |e, a| {
        let Some(p) = a.first() else { return fail("usage: shot <file.png>") };
        e.pending_shot = Some(PathBuf::from(p));
        ok("")
    });

    c.add("capture", "capture WxH <file.png> [frames]", "render the scene offscreen (no window) and save a PNG", |e, a| {
        if a.len() < 2 {
            return fail("usage: capture WxH <file.png> [frames]");
        }

        let w = atoi(&a[0]);
        let h = a[0].find('x').map_or(0, |x| atoi(&a[0][x + 1..]));
        let frames = a.get(2).map_or(20, |f| atoi(f).max(1));
        if w < 0 || h < 0 {
            return fail("capture failed");
        }

        if viewport::capture(e, w as u32, h as u32, frames as u32, std::path::Path::new(&a[1])) { ok("") } else { fail("capture failed") }
    });

    c.add("pick_pixel", "pick_pixel x y", "log the entity under a viewport pixel (of the last frame)", |e, a| {
        if a.len() < 2 {
            return fail("usage: pick_pixel x y");
        }

        let id = viewport::pick(e, atoi(&a[0]), atoi(&a[1]));
        ok(e.scene.find(id).map_or_else(|| "nothing".to_owned(), |ent| format!("entity {id} doc {} item {}", ent.source.doc, ent.source.item)))
    });

    c.add("click", "click x y [ctrl]", "select what is under a viewport pixel, as a click would", |e, a| {
        if a.len() < 2 {
            return fail("usage: click x y [ctrl]");
        }

        let id = viewport::pick(e, atoi(&a[0]), atoi(&a[1]));
        let source = e.scene.find(id).map(|ent| ent.source);
        let ctrl = a.get(2).is_some_and(|x| x == "ctrl");
        match source {
            Some(s) if s.doc != 0 => {
                if ctrl {
                    e.selection.toggle(s);
                } else {
                    e.selection.set(s);
                }
            }

            _ => {
                if !ctrl {
                    e.selection.clear();
                }
            }
        }

        ok("")
    });

    c.add("click_item", "click_item <item> [ctrl]", "click the viewport pixel where an item of the active document is (its pivot), as the mouse would", |e, a| {
        let world = e.docs.active().zip(a.first().and_then(|x| parse_u64(x))).and_then(|(d, item)| d.item_transform(SourceRef { doc: d.id(), item }));
        let Some(world) = world else { return fail("usage: click_item <item> [ctrl] (the item needs a transform)") };
        let vp = &e.viewport;
        let p = Float3::new(world.m[0][3], world.m[1][3], world.m[2][3]);
        let Some((px, py)) = vp.target().valid().then(|| Renderer::project(&vp.camera(), vp.width(), vp.height(), p)).flatten() else {
            return fail("the item is not in view");
        };

        let args = [(px as i32).to_string(), (py as i32).to_string(), a.get(1).cloned().unwrap_or_default()];
        CommandRegistry::run_args(e, "click", &args)
    });

    c.add(
        "gizmo_drag", "gizmo_drag <x|y|z> <pixels> [steps]",
        "test: drag the translate handle of the first selected item along a world axis with the scripted mouse (aimed from the projected axis tip, not guessed)",
        |e, a| {
            let world = first_selected_transform(e).map(|t| t.1);
            let (Some(world), true, true) = (world, a.len() >= 2, e.viewport.target().valid()) else {
                return fail("usage: gizmo_drag <x|y|z> <pixels> [steps] (something with a transform is selected)");
            };

            let axis = match a[0].as_str() {
                "x" => Float3::new(1.0, 0.0, 0.0),
                "y" => Float3::new(0.0, 1.0, 0.0),
                "z" => Float3::new(0.0, 0.0, 1.0),
                _ => return fail("axis must be x, y or z"),
            };

            let vp = &e.viewport;
            let cam = vp.camera();
            let o = Float3::new(world.m[0][3], world.m[1][3], world.m[2][3]);
            let fwd = normalize(cam.target - cam.eye);
            let right = normalize(cross(Float3::new(0.0, 1.0, 0.0), fwd));
            let (Some((ox, oy)), Some((rx, ry))) = (vp.project(o), vp.project(o + right)) else { return fail("the item is not in view") };

            // ImGuizmo: the handle is `gizmo_clip` of the clip space (in units of half the viewport height) long, along the axis, from the
            // item's origin.
            let unit_px = (rx - ox).hypot(ry - oy).max(1e-3);
            let len = 0.5 * vp.gizmo_clip * vp.height() as f32 / unit_px;
            let (Some((tx, ty)), Some((hx, hy))) = (vp.project(o + axis * (len * 0.7)), vp.project(o + axis * len)) else {
                return fail("the handle is not in view");
            };

            let (dx, dy) = (hx - ox, hy - oy);
            let dl = dx.hypot(dy).max(1e-3);
            let px = atof(&a[1]) as f32;
            let (x0, y0) = (format!("{tx:.1}"), format!("{ty:.1}"));
            let (x1, y1) = (format!("{:.1}", tx + dx / dl * px), format!("{:.1}", ty + dy / dl * px));
            let steps = a.get(2).cloned().unwrap_or_else(|| "10".to_owned());
            CommandRegistry::run_args(e, "mouse", &["move".to_owned(), x0.clone(), y0.clone()]);
            CommandRegistry::run_args(e, "mouse", &["drag".to_owned(), x0, y0, x1, y1, steps])
        },
    );

    c.add("assert_selected_item", "assert_selected_item <item>", "test: the first selected item of the active document is <item>", |e, a| {
        let Some(item) = a.first().and_then(|x| parse_u64(x)) else { return fail("usage: assert_selected_item <item>") };
        match e.selection.items().first() {
            None => fail("nothing is selected"),
            Some(s) if s.item == item => ok(""),
            Some(s) => fail(format!("selected item is {}", s.item)),
        }
    });

    c.add(
        "mouse", "mouse move x y | click x y [left|right|middle] | drag x0 y0 x1 y1 [steps] [button] | wheel n | off",
        "drive the viewport with a scripted mouse (pixels of the viewport image), as the real one would; every step takes a rendered frame", mouse,
    );

    c.add("type", "type <text>", "type text into the focused text field, as the keyboard would", |_, a| {
        let Some(text) = a.first() else { return fail("usage: type <text>") };
        gui::add_input_text(text);
        ok("").wait(2)
    });

    c.add(
        "key", "key [Ctrl+][Shift+][Alt+]<A-Z|0-9|F1-F12|Delete|Escape|Enter|Space|Tab|Up|Down|PageUp|PageDown>", "press a key combination (held for two frames), as the keyboard would", |e, a| {
            let Some(combo) = a.first() else { return fail("usage: key Ctrl+1") };
            let mut rest = combo.as_str();
            let mut mods = vec![];
            loop {
                let l = rest.to_lowercase();
                if l.starts_with("ctrl+") {
                    mods.push(gui::MOD_CTRL);
                    rest = &rest[5..];
                } else if l.starts_with("shift+") {
                    mods.push(gui::MOD_SHIFT);
                    rest = &rest[6..];
                } else if l.starts_with("alt+") {
                    mods.push(gui::MOD_ALT);
                    rest = &rest[4..];
                } else {
                    break;
                }
            }

            let l = rest.to_lowercase();
            let b = rest.as_bytes();
            let key = if b.len() == 1 && b[0].is_ascii_digit() {
                Some(sys::ImGuiKey_0 + i32::from(b[0] - b'0'))
            } else if b.len() == 1 && b[0].is_ascii_alphabetic() {
                Some(sys::ImGuiKey_A + i32::from(b[0].to_ascii_uppercase() - b'A'))
            } else if l == "delete" {
                Some(sys::ImGuiKey_Delete)
            } else if l == "escape" {
                Some(sys::ImGuiKey_Escape)
            } else if l == "enter" || l == "return" {
                Some(sys::ImGuiKey_Enter)
            } else if l == "space" {
                Some(sys::ImGuiKey_Space)
            } else if l == "tab" {
                Some(sys::ImGuiKey_Tab)
            } else if l == "up" {
                Some(sys::ImGuiKey_UpArrow)
            } else if l == "down" {
                Some(sys::ImGuiKey_DownArrow)
            } else if l == "pageup" {
                Some(sys::ImGuiKey_PageUp)
            } else if l == "pagedown" {
                Some(sys::ImGuiKey_PageDown)
            } else if b.len() >= 2 && (b[0] == b'F' || b[0] == b'f') {
                let n = atoi(&rest[1..]);
                (1..=12).contains(&n).then(|| sys::ImGuiKey_F1 + n - 1)
            } else {
                None
            };

            let Some(key) = key else { return fail(format!("unknown key '{rest}'")) };
            for m in mods {
                gui::add_key_event(m, true);
                e.test_keys.push((m, 3));
            }

            gui::add_key_event(key, true);
            e.test_keys.push((key, 3));
            ok("")
        },
    );

    c.add("show_panel", "show_panel <name>", "show a panel and bring it to the front of its tab group", |e, a| {
        let Some(name) = a.first() else { return fail("usage: show_panel <viewport|outliner|inspector|assets|log|history|commands>") };
        let n = name.to_lowercase();
        if let Some(i) = PANELS.iter().position(|p| *p == n) {
            *panel_flag(e, PANELS[i]).expect("a panel") = true;
            gui::set_window_focus(PANEL_TITLES[i]);
            return ok("");
        }

        // A panel of the active tool, by its title (Tile, Layers, Tiles, ...).
        if gui::window_exists(name) {
            gui::set_window_focus(name);
            return ok("");
        }

        fail(format!("no panel '{name}'"))
    });

    c.add(
        "settings", "settings open [page] | close | apply",
        "the Settings window: open it (on a copy of the settings) at a page (general, game, appearance, folders, a tool's id), close it without applying, or apply it",
        |e, a| {
        match a.first().map(String::as_str) {
            Some("open") => {
                if !e.settings_window_open() {
                    e.show_settings_window(true);
                }

                if let Some(page) = a.get(1)
                    && !e.show_settings_page(page)
                {
                    return fail(format!("no settings page '{page}'"));
                }
            }

            Some("close") => e.show_settings_window(false),
            Some("apply") if e.settings_window_open() => e.apply_settings_window(),
            Some("apply") => return fail("the Settings window is not open"),
            _ => return fail("usage: settings open [page] | close | apply"),
        }

        ok("")
    },
    );

    c.add("setup", "setup <game folder>", "what the first-run dialog's button does: store the game folder in the settings and mount", |e, a| {
        let Some(dir) = a.first() else { return fail("usage: setup <game folder>") };
        e.setup(dir).map_or_else(fail, ok)
    });

    c.add("assert_mounted", "assert_mounted 0|1", "test: the game packs are (not) mounted", |e, a| {
        let want = a.first().is_none_or(|x| x != "0");
        let mounted = e.is_mounted();
        if mounted == want { ok("") } else { fail(if mounted { "mounted" } else { "not mounted" }) }
    });

    c.add("remount", "remount", "read the packs again (after the team edited one) and rebuild the renderer", |e, _| {
        if !e.start() {
            return fail("mount failed");
        }

        e.activate_all_tools();
        ok(e.status_text().to_owned())
    });

    c.add("script", "script <file>", "run a script file", |e, a| {
        let Some(path) = a.first() else { return fail("usage: script <file>") };
        match read_script(path) {
            Ok(lines) => {
                e.script_lines.extend(lines);
                ok("")
            }

            Err(err) => fail(err),
        }
    });

    c.add("assert_log_errors", "assert_log_errors N", "test: the log holds N error entries", |_, a| {
        let n = log::count(LogLevel::Error);
        if a.first().is_none_or(|x| n as u64 == x.parse::<u64>().unwrap_or(0)) { ok("") } else { fail(format!("log has {n} errors")) }
    });
}

/// The `mouse` command. Positions are pixels, or "50%" of the viewport image's width (x) / height (y): the tests do not know the window size.
fn mouse(e: &mut Editor, a: &[String]) -> CommandResult {
    let (vw, vh) = (e.viewport.width() as f32, e.viewport.height() as f32);
    let num = |t: &str, axis: usize| -> Option<f32> {
        match t.strip_suffix('%') {
            Some(p) => p.parse::<f32>().ok().map(|v| v * 0.01 * if axis == 0 { vw } else { vh }),
            None => t.parse::<f32>().ok(),
        }
    };

    let xy = |at: usize| -> Option<(f32, f32)> { Some((num(a.get(at)?, 0)?, num(a.get(at + 1)?, 1)?)) };
    let button = |at: usize| match a.get(at).map(String::as_str) {
        Some("right") => 1,
        Some("middle") => 2,
        _ => 0,
    };

    let Some(verb) = a.first() else { return fail("usage: mouse move|click|drag|wheel|off ...") };
    let mut m = e.mouse_cur;
    match verb.as_str() {
        "off" => {
            e.park_mouse();
            return ok("");
        }

        "wheel" => {
            if let Some(w) = a.get(1).and_then(|t| num(t, 0)) {
                m.wheel = w;
                e.queue_mouse(m);
                return ok("").wait(2);
            }
        }

        "move" => {
            if let Some((x, y)) = xy(1) {
                (m.x, m.y) = (x, y);
                e.queue_mouse(m);
                return ok("").wait(2);
            }
        }

        "click" => {
            if let Some((x, y)) = xy(1) {
                let b = button(3);
                (m.x, m.y) = (x, y);
                for i in 0..4 {
                    m.down[b] = i == 1 || i == 2;
                    e.queue_mouse(m);
                }

                return ok("").wait(4 + 2);
            }
        }

        "drag" => {
            if let (Some((x0, y0)), Some((x1, y1))) = (xy(1), xy(3)) {
                let n = a.get(5).and_then(|t| num(t, 0)).unwrap_or(8.0).max(1.0);
                let b = button(6);
                (m.x, m.y) = (x0, y0);
                m.down[b] = false;
                e.queue_mouse(m);
                m.down[b] = true;
                e.queue_mouse(m);
                let mut frames = 2;
                for i in 1..=n as i32 {
                    m.x = x0 + (x1 - x0) * i as f32 / n;
                    m.y = y0 + (y1 - y0) * i as f32 / n;
                    e.queue_mouse(m);
                    frames += 1;
                }

                e.queue_mouse(m);
                m.down[b] = false;
                e.queue_mouse(m);
                frames += 2;
                return ok("").wait(frames + 2);
            }
        }

        _ => {}
    }

    fail("usage: mouse move x y | click x y [button] | drag x0 y0 x1 y1 [steps] [button] | wheel n | off")
}
