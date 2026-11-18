//! The VFX tool's commands: playback, solo / mute and a state dump, for scripts and the command palette.

use wce_editor::editor::Editor;
use wce_editor_core::command_registry::{CommandRegistry, CommandResult};
use wce_editor_core::document::Document;

use crate::tool::{Part, document, with_vfx};

fn ok(m: impl Into<String>) -> CommandResult {
    CommandResult::success(m)
}

fn fail(m: impl Into<String>) -> CommandResult {
    CommandResult::fail(m)
}

/// An emitter id or a reference's inst_name of the open file.
fn part(t: &crate::VfxTool, name: &str) -> Option<Part> {
    if t.ix.emitter(name).is_some() {
        Some(Part::Emitter(name.to_owned()))
    } else {
        t.ix.reference(name).map(|_| Part::Reference(name.to_owned()))
    }
}

pub fn register(c: &mut CommandRegistry<Editor>) {
    c.add("vfx_play", "vfx_play [on|off]", "VFX preview: run (on, default) or pause the particles' clock", |e, a| {
        let on = a.first().is_none_or(|x| x != "off");
        with_vfx(e, |t, _| {
            t.play.playing = on;
            ok("")
        })
    });

    c.add("vfx_speed", "vfx_speed <rate>", "VFX preview: the particles' clock rate (1 the game's)", |e, a| {
        let Some(rate) = a.first().and_then(|x| x.parse::<f32>().ok()) else { return fail("usage: vfx_speed <rate>") };
        with_vfx(e, |t, _| {
            t.play.speed = rate.clamp(0.0, 10.0);
            ok("")
        })
    });

    c.add("vfx_restart", "vfx_restart", "VFX preview: start the effect again", |e, _| {
        with_vfx(e, |t, _| {
            t.restart();
            ok("")
        })
    });

    c.add("vfx_mute", "vfx_mute <emitter|reference> [off]", "VFX preview: mute a part of the open effect (preview only)", |e, a| {
        let Some(name) = a.first().cloned() else { return fail("usage: vfx_mute <emitter|reference> [off]") };
        let off = a.get(1).is_some_and(|x| x == "off");
        with_vfx(e, |t, e| {
            t.refresh_index(e);
            let Some(p) = part(t, &name) else { return fail(format!("no emitter or reference '{name}'")) };
            if off {
                t.muted.remove(&p);
            } else {
                t.muted.insert(p);
            }

            ok("")
        })
    });

    c.add("vfx_solo", "vfx_solo <emitter|reference>|off", "VFX preview: play only one part of the open effect (preview only)", |e, a| {
        let Some(name) = a.first().cloned() else { return fail("usage: vfx_solo <emitter|reference>|off") };
        with_vfx(e, |t, e| {
            t.refresh_index(e);
            if name == "off" {
                t.solo = None;
                return ok("");
            }

            let Some(p) = part(t, &name) else { return fail(format!("no emitter or reference '{name}'")) };
            t.solo = Some(p);
            ok("")
        })
    });

    c.add("vfx_focus", "vfx_focus <emitter|reference>", "VFX: the part whose handles the viewport shows", |e, a| {
        let Some(name) = a.first().cloned() else { return fail("usage: vfx_focus <emitter|reference>") };
        with_vfx(e, |t, e| {
            t.refresh_index(e);
            let Some(p) = part(t, &name) else { return fail(format!("no emitter or reference '{name}'")) };
            t.focus = Some(p);
            ok("")
        })
    });

    c.add("vfx_only_focused", "vfx_only_focused on|off", "VFX: the Inspector lists only the focused part", |e, a| {
        let on = a.first().is_none_or(|x| x != "off");
        with_vfx(e, |t, _| {
            t.only_focused = on;
            ok("")
        })
    });

    c.add("vfx_filter", "vfx_filter [text]", "VFX: the Inspector lists the properties whose name has the text (none: all)", |e, a| {
        let text = a.join(" ");
        with_vfx(e, |t, _| {
            t.prop_filter = text;
            ok("")
        })
    });

    c.add("vfx_quality", "vfx_quality 0|1|2", "VFX: the graphics quality variant the Inspector edits", |e, a| {
        let Some(q) = a.first().and_then(|x| x.parse::<i32>().ok()) else { return fail("usage: vfx_quality 0|1|2") };
        with_vfx(e, |t, _| {
            t.quality = q.clamp(0, 2);
            ok("")
        })
    });

    c.add(
        "vfx_set",
        "vfx_set <emitter> <property> <attribute> <value>",
        "VFX: set an attribute of an emitter's property (base quality), as an Inspector edit would",
        |e, a| {
            let [em, prop, attr, value] = a else { return fail("usage: vfx_set <emitter> <property> <attribute> <value>") };
            with_vfx(e, |t, e| {
                t.refresh_index(e);
                let Some(p) = t.ix.emitter(em).and_then(|x| crate::model::pick(&x.props, prop, 0)).map(|p| p.elem) else {
                    return fail(format!("no property {prop} in emitter {em}"));
                };

                let Some(d) = crate::tool::document_mut(e) else { return fail("no effect open") };
                // A number in the style of the one it replaces, as the Inspector writes it.
                let text = match (d.xml().get(p, attr), value.parse::<f32>()) {
                    (Some(old), Ok(v)) if old.trim().parse::<f64>().is_ok() => crate::inspect::number_like(&old, v),
                    _ => value.clone(),
                };
                crate::inspect::put(d, p, attr, &text, 0);
                ok("")
            })
        },
    );

    c.add(
        "vfx_curve",
        "vfx_curve <emitter> <property> <curve> <t,v;t,v...>",
        "VFX: set the keys of one curve of an emitter's ranged property (base quality), as the curve editor would",
        |e, a| {
            let [em, prop, curve, keys] = a else { return fail("usage: vfx_curve <emitter> <property> <curve> <t,v;t,v...>") };
            let pts = keys
                .split(';')
                .filter_map(|k| k.split_once(','))
                .filter_map(|(t, v)| Some((t.trim().parse::<f32>().ok()?, v.trim().parse::<f32>().ok()?)))
                .collect::<Vec<_>>();
            with_vfx(e, |t, e| {
                t.refresh_index(e);
                let Some(p) = t.ix.emitter(em).and_then(|x| crate::model::pick(&x.props, prop, 0)).map(|p| p.elem) else {
                    return fail(format!("no property {prop} in emitter {em}"));
                };

                let Some(d) = crate::tool::document_mut(e) else { return fail("no effect open") };
                let raw = d.xml().inner_text(p).map_err(|e| e.to_string());
                let text = match raw {
                    Ok(raw) => String::from_utf8_lossy(&wce_formats::xml_doc::decode_entities(&raw)).into_owned(),
                    Err(m) => return fail(m),
                };
                let mut curves = crate::curves::Curves::parse(&text);
                let Some(c) = curves.find_mut(curve) else { return fail(format!("{prop} has no curve {curve}")) };
                c.set_points(&pts);
                let out = wce_formats::xml_doc::encode_entities(curves.write().as_bytes(), b'"');
                match d.set_inner_text(p, &out, &format!("Edit the curves of {prop}")) {
                    Ok(()) => ok(""),
                    Err(m) => fail(m),
                }
            })
        },
    );

    c.add(
        "vfx_duplicate",
        "vfx_duplicate <emitter|reference|chain>",
        "VFX: duplicate a part of the open effect (an emitter with its render stack)",
        |e, a| {
            let Some(name) = a.first().cloned() else { return fail("usage: vfx_duplicate <emitter|reference|chain>") };
            structural(e, &name, true)
        },
    );

    c.add(
        "vfx_delete",
        "vfx_delete <emitter|reference|chain>",
        "VFX: delete a part of the open effect (an emitter with its place in the stacks)",
        |e, a| {
            let Some(name) = a.first().cloned() else { return fail("usage: vfx_delete <emitter|reference|chain>") };
            structural(e, &name, false)
        },
    );

    c.add(
        "vfx_handles",
        "vfx_handles",
        "VFX: the focused emitter's handles, where they are in the viewport image (pixels) and their values (metres)",
        |e, _| {
            with_vfx(e, |t, e| {
                t.refresh_index(e);
                let list = crate::gizmos::handle_list(t, e);
                if list.is_empty() {
                    return ok("no handles (focus an emitter with a spawn ring or box)");
                }

                let lines = list
                    .iter()
                    .map(|(h, px, v)| match px {
                        Some((x, y)) => format!("{h:?} {x:.0} {y:.0} {v:.3}"),
                        None => format!("{h:?} off-screen {v:.3}"),
                    })
                    .collect::<Vec<_>>();
                ok(lines.join("\n"))
            })
        },
    );

    c.add(
        "vfx_drag_handle",
        "vfx_drag_handle Outer|Inner|BoxX|BoxZ|BoxY <dx> <dy>",
        "VFX test: drag a handle of the focused emitter by (dx, dy) image pixels with the mouse, from where it is",
        |e, a| {
            let [name, dx, dy] = a else { return fail("usage: vfx_drag_handle <handle> <dx> <dy>") };
            let (Ok(dx), Ok(dy)) = (dx.parse::<f32>(), dy.parse::<f32>()) else { return fail("dx and dy are numbers") };
            let at = with_vfx(e, |t, e| {
                t.refresh_index(e);
                match crate::gizmos::handle_list(t, e).into_iter().find(|(h, _, _)| format!("{h:?}") == *name) {
                    Some((_, Some((x, y)), _)) => ok(format!("{x} {y}")),
                    _ => fail(format!("no handle {name} on screen")),
                }
            });
            if !at.ok {
                return at;
            }

            let mut xy = at.message.split(' ').filter_map(|v| v.parse::<f32>().ok());
            let (Some(x), Some(y)) = (xy.next(), xy.next()) else { return fail("no position") };
            let args = ["drag", &x.to_string(), &y.to_string(), &(x + dx).to_string(), &(y + dy).to_string(), "12"].map(str::to_owned);
            CommandRegistry::run_args(e, "mouse", &args)
        },
    );

    c.add("vfx_new", "vfx_new <name> copy | vfx_new <name> plays <library>", "VFX: a new effect in the project, opened", |e, a| {
        let (Some(name), Some(how)) = (a.first().cloned(), a.get(1)) else { return fail("usage: vfx_new <name> copy | plays <library>") };
        let start = match (how.as_str(), a.get(2)) {
            ("copy", _) => crate::new::Start::Copy,
            ("plays", Some(lib)) => crate::new::Start::Plays(lib.clone()),
            _ => return fail("usage: vfx_new <name> copy | plays <library>"),
        };
        with_vfx(e, |t, e| match crate::new::create(e, &name, &start) {
            Ok(path) => {
                t.open_later(path.clone());
                ok(format!("created {path}"))
            }

            Err(m) => fail(m),
        })
    });

    c.add(
        "vfx_add_property",
        "vfx_add_property <emitter> [property [quality]]",
        "VFX: add a property an emitter lacks, as vanilla writes it (no property: list them); a quality 1 or 2 adds that variant of one it has",
        |e, a| {
            let Some(em) = a.first().cloned() else { return fail("usage: vfx_add_property <emitter> [property [quality]]") };
            let name = a.get(1).cloned();
            let q = a.get(2).and_then(|x| x.parse::<i32>().ok()).unwrap_or(0);
            with_vfx(e, |t, e| {
                t.refresh_index(e);
                let Some(table) = t.defaults.get(e.vfs()) else { return fail("still reading the game's effects: try again") };
                let Some(emitter) = t.ix.emitter(&em).cloned() else { return fail(format!("no emitter {em}")) };
                let Some(d) = crate::tool::document_mut(e) else { return fail("no effect open") };
                let has = |n: &str, q: i32| emitter.props.iter().any(|p| p.name == n && p.quality == q);
                let missing = table
                    .get(&crate::defaults::emitter_scope(&emitter.kind))
                    .into_iter()
                    .flatten()
                    .filter(|tpl| !has(&tpl.name, 0))
                    .collect::<Vec<_>>();
                let Some(name) = name else {
                    if missing.is_empty() {
                        return ok(format!("{em} has every property vanilla's {} emitters have", emitter.kind));
                    }

                    return ok(missing.iter().map(|t| format!("{} ({} uses): {}", t.name, t.uses, t.text)).collect::<Vec<_>>().join("\n"));
                };

                let text = if q > 0 {
                    let Some(base) = emitter.props.iter().find(|p| p.name == name && p.quality == 0) else {
                        return fail(format!("{em} has no {name} to make a variant of"));
                    };
                    if has(&name, q) {
                        return fail(format!("{em} has a quality {q} {name} already"));
                    }

                    crate::defaults::with_quality(&crate::defaults::element_text(d.xml(), base.elem), q)
                } else {
                    let Some(tpl) = missing.iter().find(|t| t.name == name) else {
                        return fail(format!("{name}: {em} has it, or no vanilla emitter of its type does"));
                    };
                    tpl.text.clone()
                };

                match d.add_child_text(emitter.elem, text.as_bytes(), &format!("Add {name}")) {
                    Ok(_) => ok(text),
                    Err(m) => fail(m),
                }
            })
        },
    );

    c.add("vfx_frame", "vfx_frame", "VFX preview: the camera on the effect", |e, _| {
        with_vfx(e, |t, e| {
            t.frame_camera(e);
            ok("")
        })
    });

    c.add("vfx_info", "vfx_info", "VFX: the open effect's parts, what plays, the particles alive and the particle errors", |e, _| {
        with_vfx(e, |t, e| {
            t.refresh_index(e);
            let Some(d) = document(e) else { return fail("no effect open") };
            let mut out = format!("{} ({}), modified {}\n", d.title(), crate::tool::preview_path(d), d.modified());
            for em in &t.ix.emitters {
                let p = Part::Emitter(em.id.clone());
                out += &format!(
                    "emitter {} ({}), {} properties, {}\n",
                    em.id,
                    em.kind,
                    em.props.len(),
                    if t.plays(&p) { "plays" } else { "silent" }
                );
                for pr in em.props.iter().filter(|p| p.quality == 0) {
                    let text = d.xml().inner_text(pr.elem).unwrap_or_default();
                    if !text.is_empty() {
                        let c = crate::curves::Curves::parse(&String::from_utf8_lossy(&text));
                        let names = c.curves.iter().map(|c| format!("{} ({} keys)", c.name(), c.keys.len())).collect::<Vec<_>>();
                        out += &format!("  {}: {}\n", pr.name, names.join(", "));
                        let text = c.write();
                        out += &format!("    {}\n", text.chars().take(160).collect::<String>());
                    }
                }
            }

            for c in &t.ix.chains {
                out += &format!("chain {} ({} modifiers)\n", c.id, c.modifiers.len());
            }

            for st in &t.ix.stacks {
                let ems = st.emitters.iter().map(|(_, id)| id.as_str()).collect::<Vec<_>>().join(",");
                let chs = st.chains.iter().map(|(_, id)| id.as_str()).collect::<Vec<_>>().join(",");
                out += &format!("stack {}: {ems} with {chs}\n", st.id);
            }

            for r in &t.ix.refs {
                let p = Part::Reference(r.inst_name.clone());
                out += &format!("reference {} -> {}, {}\n", r.inst_name, r.library, if t.plays(&p) { "plays" } else { "silent" });
            }

            out += &format!("playing {}, speed {}, t {:.2}\n", t.play.playing, t.play.speed, t.play.elapsed);
            if let Some(r) = e.renderer() {
                let p = r.particles();
                out += &format!("particles {} / {}\n", p.alive(), p.capacity());
                for err in p.errors() {
                    out += &format!("error: {err}\n");
                }
            }

            ok(out)
        })
    });
}

/// Duplicates or deletes the emitter, reference or behaviour chain called `name`.
fn structural(e: &mut Editor, name: &str, dup: bool) -> CommandResult {
    with_vfx(e, |t, e| {
        t.refresh_index(e);
        let (is_emitter, is_ref, is_chain) = (t.ix.emitter(name).is_some(), t.ix.reference(name).is_some(), t.ix.chain(name).is_some());
        let Some(d) = crate::tool::document_mut(e) else { return fail("no effect open") };
        let r = match (is_emitter, is_ref, is_chain, dup) {
            (true, _, _, true) => crate::ops::duplicate_emitter(d, name),
            (true, _, _, false) => crate::ops::delete_emitter(d, name),
            (_, true, _, true) => crate::ops::duplicate_reference(d, name),
            (_, true, _, false) => crate::ops::delete_reference(d, name),
            (_, _, true, true) => crate::ops::duplicate_chain(d, name),
            (_, _, true, false) => crate::ops::delete_chain(d, name),
            _ => Err(format!("no emitter, reference or behaviour chain '{name}'")),
        };

        match r {
            Ok(m) => ok(m),
            Err(m) => fail(m),
        }
    })
}
