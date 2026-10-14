//! The weather tool's commands: everything the menus and panels do, plus the test commands (assert_*) the scripted checks use, and the old
//! weather_editor command-line flags (--view, --blend, --pick, --picker, --vdb-script, --no-flare), which the app forwards as commands.

use std::path::PathBuf;

use wce_editor::editor::Editor;
use wce_editor_core::command_registry::{CommandRegistry, CommandResult};
use wce_editor_core::document::Document;
use wce_formats::xml_doc::{XmlDoc, decode_entities};
use wce_scene::render_scene::SourceRef;
use wce_vfs::normalize;
use wce_weather::sample_map_document::SampleMapDocument;
use wce_weather::xml_file_document::Op;

use crate::tool::{WeatherTool, active_xml, find_xml, view_of, with_weather};

fn ok(m: impl Into<String>) -> CommandResult {
    CommandResult::success(m)
}

fn fail(m: impl Into<String>) -> CommandResult {
    CommandResult::fail(m)
}

/// C's strtof / strtoul on a whole argument: the leading number, 0 when there is none.
fn num(s: &str) -> f64 {
    wce_formats::c_num::strtod_prefix(s).map_or(0.0, |(v, _)| v)
}

fn int(s: &str) -> i64 {
    wce_formats::c_num::strtol(s).unwrap_or(0)
}

/// An element given as an index, or as the name of its first occurrence.
fn resolve_element(xml: &XmlDoc, token: &str) -> Option<usize> {
    if !token.is_empty() && token.bytes().all(|c| c.is_ascii_digit()) {
        return token.parse::<usize>().ok().filter(|&e| e < xml.elements().len());
    }

    xml.elements().iter().position(|e| e.name == token)
}

fn resolve_attr(xml: &XmlDoc, elem: usize, name: &str) -> Option<usize> {
    xml.elements()[elem].attrs.iter().position(|a| a.name == name)
}

/// `std::to_string` of a float: `%f`.
fn f(v: f32) -> String {
    format!("{v:.6}")
}

/// What the legacy weather editor wrote next to every shot (<shot>.png.txt): renderer, wind and particle state, for comparing runs and with
/// the game's dumps.
pub fn weather_diagnostics(e: &mut Editor, tool: &WeatherTool) -> String {
    let mut d = format!("status: {}\n", e.status_text());
    d += &format!("viewport sizes:{} x{}\n", e.viewport.size_trace(), e.viewport.frames_rendered);
    d += &format!("scene objects: {}\n", e.scene.entities().len());
    let Some(r) = e.renderer() else { return d };
    d += &format!("draws: {}, meshes: {}\n", r.draw_count(), r.mesh_count());
    d += &format!("cloud sprites: {}\n", r.cloud_sprites());
    d += &format!("sun visibility (lens flare): {}\n", f(r.sun_visibility()));
    {
        let p = r.particles();
        let fx = p.effect();
        d += &format!("weather vfx: '{}' -> {}, {} emitters, {} particles\n", fx.definition.name, fx.definition.vfx1, fx.emitters.len(), p.alive());
        for (i, em) in fx.emitters.iter().enumerate() {
            d += &format!("  emitter: {} / {} / {}: {} particles\n", em.library, em.instance, em.name, p.alive_of(i));
        }
    }

    let w = r.wind().clone();
    d += &format!(
        "wind: dir {} deg, strength {}, vfx ({} {}), g_wind ({} {})\n", f(r.wind_direction_degrees(&tool.last_params)), f(w.strength), f(w.vfx_velocity.x),
        f(w.vfx_velocity.z), f(w.smoothed.x), f(w.smoothed.y),
    );
    {
        let p = r.particles();
        for n in &p.effect().notes {
            d += &format!("  vfx note: {n}\n");
        }

        for err in p.errors() {
            d += &format!("  vfx error: {err}\n");
        }
    }

    for err in r.models().errors() {
        d += &format!("scene error: {err}\n");
    }

    for err in r.textures().errors() {
        d += &format!("texture error: {err}\n");
    }

    d += &format!("points readback: {} maps, raw {} {} {} {}\n", r.debug_maps, f(r.debug_points[0]), f(r.debug_points[1]), f(r.debug_points[2]), f(r.debug_points[3]));
    d += &format!("sky cube face averages (+x -x +y -y +z -z): {}\n", r.engine_ambient_faces());
    d += &format!("luma black/white points: {} / {}\n", f(r.luma_black()), f(r.luma_white()));
    for err in r.errors() {
        d += &format!("render error: {err}\n");
    }

    for m in r.cbuffers().missing() {
        d += &format!("missing cb var: {m}\n");
    }

    d
}

pub fn register(c: &mut CommandRegistry<Editor>) {
    c.add("sample", "sample <seed> <count>", "open a new sample map document (the old one goes)", |e, a| {
        with_weather(e, |t, e| {
            t.scene_seed = a.first().map_or(1, |s| int(s) as u32 as i32);
            t.scene_count = a.get(1).map_or(16, |s| (int(s) as i32).clamp(1, 100));
            if e.vfs().is_none() {
                return fail("the game packs are not mounted");
            }

            t.reroll(e);
            ok("sample map")
        })
    });

    c.add("reroll", "reroll", "generate the sample map again with the current seed and count", |e, _| {
        with_weather(e, |t, e| {
            t.reroll(e);
            ok("")
        })
    });

    c.add("assert_offset", "assert_offset <item> dx dy dz [yaw_degrees]", "test: a sample map building has this placement", |e, a| {
        let doc = e.docs.order().iter().rev().copied().find(|&id| e.docs.find_as::<SampleMapDocument>(id).is_some());
        let (Some(id), true) = (doc, a.len() >= 4) else { return fail("usage: assert_offset <item> dx dy dz [yaw_degrees] (needs a sample map)") };
        let p = e.docs.find_as::<SampleMapDocument>(id).expect("found").placement(a[0].parse().unwrap_or(0));
        let want = [num(&a[1]) as f32, num(&a[2]) as f32, num(&a[3]) as f32];
        let degrees = p.yaw * 57.295_78;
        let same = (p.offset.x - want[0]).abs() < 1e-3
            && (p.offset.y - want[1]).abs() < 1e-3
            && (p.offset.z - want[2]).abs() < 1e-3
            && a.get(4).is_none_or(|y| (degrees - num(y) as f32).abs() < 1e-2);
        if same { ok("") } else { fail(format!("placement is ({}, {}, {}) yaw {}", f(p.offset.x), f(p.offset.y), f(p.offset.z), f(degrees))) }
    });

    // ---- the old command-line flags
    c.add(
        "view", "view <n>", "renderer view: 0 final, 1 HDR raw, 2 albedo, 3 normal, 4 specular, 5 depth, 6 sky cube, 7 bloom, 8 quarter, 9 tone mapped, 10 AO",
        |e, a| {
            let (Some(n), Some(r)) = (a.first(), e.renderer()) else { return fail("usage: view <n> (needs the renderer)") };
            r.view = view_of(int(n).clamp(0, 10) as i32);
            ok("")
        },
    );

    c.add("blend", "blend <t>", "cross-fade the active environment with the first other open environment, t from 0 to 1", |e, a| {
        let Some(t) = a.first() else { return fail("usage: blend <t>") };
        with_weather(e, |tool, e| {
            tool.blend_t = (num(t) as f32).clamp(0.0, 1.0);

            // The partner is the first environment document that is not the one that lights the scene.
            let active = active_xml(e).filter(|x| x.is_environment()).map(|x| x.id());
            let lit = active.unwrap_or(tool.env_doc);
            tool.blend_doc = e.docs.order().to_vec().into_iter().find(|&id| id != lit && find_xml(e, id).is_some_and(|x| x.is_environment())).unwrap_or(0);
            if tool.blend_doc != 0 { ok("") } else { fail("no second environment is open") }
        })
    });

    c.add("pick", "pick <category>=<value>", "weather picker condition (opens the picker; repeatable)", |e, a| {
        let Some((category, value)) = a.first().and_then(|x| x.split_once('=')) else { return fail("usage: pick <category>=<value>") };
        with_weather(e, |t, _| {
            t.picker.set_pick(category, value);
            t.picker.open = true;
            ok("")
        })
    });

    c.add("picker", "picker pick|edit", "open the weather picker on the Pick (any other argument) or Edit tab", |e, a| {
        with_weather(e, |t, _| {
            t.picker.open = true;
            if a.first().is_some_and(|x| x == "edit") {
                t.picker.show_edit_tab();
            }

            ok("")
        })
    });

    c.add("vdb-script", "vdb-script \"add-value:climate=X;save:both\"", "edit and save the weather database (for tests; see docs/re/variation_db.md)", |e, a| {
        let Some(script) = a.first() else { return fail("usage: vdb-script \"<commands>\"") };
        with_weather(e, |t, e| {
            t.picker.open = true;
            t.picker.set_script(script);
            if !t.picker.ensure_loaded(e, &e.output_dir()) {
                return fail("the database did not load");
            }

            ok(t.picker.message())
        })
    });

    c.add("assert_candidates", "assert_candidates <n> [file]", "test: the picker finds n environment files for its conditions, optionally including <file>", |e, a| {
        let Some(n) = a.first() else { return fail("usage: assert_candidates <n> [file]") };
        with_weather(e, |t, e| {
            if !t.picker.ensure_loaded(e, &e.output_dir()) {
                return fail("the database did not load");
            }

            let r = t.picker.candidates();
            if r.candidates.len() as u64 != n.parse::<u64>().unwrap_or(0) {
                return fail(format!("{} candidates", r.candidates.len()));
            }

            if let Some(file) = a.get(1)
                && !r.candidates.iter().any(|c| normalize(&c.filename) == normalize(file))
            {
                return fail(format!("{file} is not a candidate"));
            }

            ok("")
        })
    });

    c.add("no-flare", "no-flare", "lens flare off (the old --no-flare flag)", |e, _| {
        if let Some(r) = e.renderer() {
            r.lens_flare = false;
        }

        ok("")
    });

    c.add("output_dir", "output_dir <folder>", "where Save writes loose files (stored in wce.ini unless 'temp' is added)", |e, a| {
        let Some(dir) = a.first() else { return fail("usage: output_dir <folder> [temp]") };
        e.set_output_dir(&PathBuf::from(dir), !a.get(1).is_some_and(|x| x == "temp"));
        ok("")
    });

    // ---- XML documents
    c.add(
        "edit_attr", "edit_attr <element index|name> <attribute> <value>", "set an attribute of the active weather file (one undo entry), the value as it is shown",
        |e, a| {
            let (Some(d), true) = (active_xml(e), a.len() >= 3) else { return fail("usage: edit_attr <element> <attribute> <value> (needs an open weather file)") };
            let Some(el) = resolve_element(d.xml(), &a[0]) else { return fail(format!("no element '{}'", a[0])) };
            let Some(attr) = resolve_attr(d.xml(), el, &a[1]) else { return fail(format!("no attribute '{}' on {}", a[1], a[0])) };
            d.set_attr_text(el, attr, &a[2], 0);
            ok("")
        },
    );

    c.add(
        "xml_op", "xml_op duplicate|remove|up|down|add_child|add_attr|remove_attr <element> [name] [value]", "structural edit of the active weather file (one undo entry)",
        |e, a| {
            let (Some(d), true) = (active_xml(e), a.len() >= 2) else { return fail("usage: xml_op <op> <element> [name] [value]") };
            let Some(el) = resolve_element(d.xml(), &a[1]) else { return fail(format!("no element '{}'", a[1])) };
            let name = a.get(2).map_or("", String::as_str);
            let value = a.get(3).map_or("", String::as_str);
            let mut attr = 0;
            let op = match a[0].as_str() {
                "duplicate" => Op::Duplicate,
                "remove" => Op::Remove,
                "up" => Op::MoveUp,
                "down" => Op::MoveDown,
                "add_child" => Op::AddChild,
                "add_attr" => Op::AddAttr,
                "remove_attr" => {
                    let Some(k) = resolve_attr(d.xml(), el, name) else { return fail(format!("no attribute '{name}'")) };
                    attr = k;
                    Op::RemoveAttr
                }

                other => return fail(format!("unknown operation '{other}'")),
            };

            let id = d.id();
            match d.structural(op, el, attr, name, value) {
                Err(err) => fail(err),
                Ok(sel) => {
                    e.selection.remove_document(id);
                    if let Some(s) = sel {
                        e.selection.set(SourceRef { doc: id, item: s as u64 });
                    }

                    ok("")
                }
            }
        },
    );

    c.add("revert_xml", "revert_xml", "undo every edit of the active weather file (also structural ones)", |e, _| {
        let Some(d) = active_xml(e) else { return fail("no weather file") };
        d.revert_all();
        let id = d.id();
        e.selection.remove_document(id);
        ok("")
    });

    c.add("assert_attr", "assert_attr <element index|name> <attribute> <value>", "test: an attribute of the active weather file shows this value", |e, a| {
        let (Some(d), true) = (active_xml(e), a.len() >= 3) else { return fail("usage: assert_attr <element> <attribute> <value>") };
        let Some(el) = resolve_element(d.xml(), &a[0]) else { return fail(format!("no element '{}'", a[0])) };
        let Some(attr) = resolve_attr(d.xml(), el, &a[1]) else { return fail(format!("no attribute '{}'", a[1])) };
        let now = String::from_utf8_lossy(&decode_entities(d.xml().attr_raw(el, attr))).into_owned();
        if now == a[2] { ok("") } else { fail(format!("value is '{now}'")) }
    });

    c.add("assert_xml_elements", "assert_xml_elements N", "test: the active weather file has N elements", |e, a| {
        let (Some(d), Some(n)) = (active_xml(e), a.first()) else { return fail("usage: assert_xml_elements N") };
        let have = d.xml().elements().len();
        if have as u64 == n.parse::<u64>().unwrap_or(0) { ok("") } else { fail(format!("{have} elements")) }
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

            let want = num(&a[1]) as f32;
            if (now - want).abs() < 1e-3 * want.abs().max(1.0) { ok("") } else { fail(format!("{} is {}", a[0], f(now))) }
        },
    );

    c.add("diagnostics", "diagnostics <file.txt>", "write the renderer, wind and particle state (what weather_editor wrote next to every shot)", |e, a| {
        let Some(path) = a.first() else { return fail("usage: diagnostics <file.txt>") };
        with_weather(e, |t, e| {
            let text = weather_diagnostics(e, t);
            match std::fs::write(path, text) {
                Ok(()) => ok(""),
                Err(_) => fail(format!("cannot write {path}")),
            }
        })
    });

    c.add("assert_saved", "assert_saved <relative path>", "test: a loose file exists under the output folder", |e, a| {
        let Some(rel) = a.first() else { return fail("usage: assert_saved <path under the output folder>") };
        if e.output_dir().join(rel).exists() { ok("") } else { fail("no such file") }
    });
}
