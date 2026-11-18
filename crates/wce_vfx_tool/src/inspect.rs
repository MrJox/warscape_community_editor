//! The Inspector of an open vfx file: the effect's own settings, then one collapsible section per emitter, behaviour chain and
//! reference, and the render stacks. Every row edits the file in place (span edits: the rest of the file stays byte-identical); a curve
//! is written when its drag ends.

use wce_editor::editor::Editor;
use wce_editor_core::document::Document;
use wce_editor_core::inspector::Inspector;
use wce_formats::xml_doc::{decode_entities, encode_entities};
use wce_weather::xml_file_document::XmlFileDocument;

use crate::curves::{Curves, format_number};
use crate::model::{Prop, PropTag, pick};
use crate::props::{GROUPS, group, hint, label};
use crate::tool::{Part, PendingText, VfxTool, document_mut};

/// A number written in the style of the text it replaces: as many decimals as it had, more when the value needs them (up to six).
pub(crate) fn number_like(orig: &str, v: f32) -> String {
    let t = orig.trim();
    if !t.contains(['.', 'e', 'E']) && t.parse::<i64>().is_ok() && v.fract() == 0.0 && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }

    let decimals = t.split_once('.').map_or(1, |(_, d)| d.chars().take_while(char::is_ascii_digit).count()).max(1);
    for d in decimals..=6 {
        let s = format!("{v:.d$}");
        if (s.parse::<f32>().unwrap_or(f32::NAN) - v).abs() <= 5e-7 * v.abs().max(1.0) {
            return s;
        }
    }

    format_number(v)
}

/// `float4(1.0,0.9,0.8,1.0)` -> its numbers and their texts.
fn float_n(text: &str) -> Option<(String, Vec<(String, f32)>)> {
    let (head, body) = text.trim().strip_suffix(')')?.split_once('(')?;
    let parts = body.split(',').map(|p| (p.to_owned(), wce_formats::c_num::strtod(p).unwrap_or(0.0) as f32)).collect::<Vec<_>>();
    head.starts_with("float").then(|| (head.to_owned(), parts))
}

fn write_float_n(head: &str, old: &[(String, f32)], values: &[f32]) -> String {
    let parts =
        old.iter().zip(values).map(|((text, was), v)| if was == v { text.clone() } else { number_like(text, *v) }).collect::<Vec<_>>();
    format!("{head}({})", parts.join(","))
}

/// The gesture of a row: one per document, element and attribute (a drag's edits merge into one undo entry).
fn gesture(d: &XmlFileDocument, elem: usize, attr: &str) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in attr.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }

    (u64::from(d.id()) << 40) ^ ((elem as u64) << 8) ^ h ^ 0x5f
}

/// An attribute's value, decoded.
pub(crate) fn get(d: &XmlFileDocument, elem: usize, attr: &str) -> Option<String> {
    d.xml().get(elem, attr)
}

/// Writes an attribute's value (entity-encoded with its quote). Missing: added.
pub(crate) fn put(d: &mut XmlFileDocument, elem: usize, attr: &str, value: &str, gesture: u64) {
    put_many(d, elem, &[(attr, value.to_owned())], gesture);
}

/// Writes several attributes of one element as one undo entry (merged over a drag by `gesture`). Missing ones are added.
pub(crate) fn put_many(d: &mut XmlFileDocument, elem: usize, values: &[(&str, String)], gesture: u64) {
    let mut set = vec![];
    for (attr, value) in values {
        match d.xml().find_attr(elem, attr) {
            Some(a) => {
                let quote = d.xml().elements()[elem].attrs[a].quote;
                set.push((a, encode_entities(value.as_bytes(), quote)));
            }

            None => {
                if let Err(m) = d.set_named(elem, attr, value, 0) {
                    wce_foundation::log::warn(format!("vfx: {m}"));
                }
            }
        }
    }

    d.set_attrs(elem, set, gesture);
}

/// An attribute as a typed row: a check box, a number, a `floatN(...)` vector, or text.
fn attr_row(ins: &mut dyn Inspector, d: &mut XmlFileDocument, elem: usize, attr: &str, field: &str, label: &str) {
    let Some(text) = get(d, elem, attr) else { return };
    let g = gesture(d, elem, attr);
    let t = text.trim();
    if t == "true" || t == "false" {
        let mut b = t == "true";
        if ins.edit_bool(field, label, &mut b) {
            put(d, elem, attr, if b { "true" } else { "false" }, 0);
        }
    } else if let Some((head, parts)) = float_n(t) {
        let mut v = parts.iter().map(|p| p.1).collect::<Vec<_>>();
        let changed = match v.len() {
            2 => ins.edit_float2(field, label, (&mut v[..2]).try_into().expect("two"), 0.01),
            3 => ins.edit_float3(field, label, (&mut v[..3]).try_into().expect("three"), 0.01),
            4 if label.to_ascii_lowercase().contains("colo") => ins.edit_color4(field, label, (&mut v[..4]).try_into().expect("four")),
            4 => {
                let (a, b) = v.split_at_mut(2);
                let x = ins.edit_float2(&format!("{field}.xy"), &format!("{label} (1, 2)"), a.try_into().expect("two"), 0.01);
                let y = ins.edit_float2(&format!("{field}.zw"), &format!("{label} (3, 4)"), b.try_into().expect("two"), 0.01);
                x || y
            }
            _ => {
                ins.text(field, label, t);
                false
            }
        };

        if changed {
            put(d, elem, attr, &write_float_n(&head, &parts, &v), g);
        }
    } else if !t.contains(['.', 'e', 'E'])
        && let Ok(n) = t.parse::<i64>()
    {
        let mut i = n;
        if ins.edit_int(field, label, &mut i, 0.2) {
            put(d, elem, attr, &i.to_string(), g);
        }
    } else if let Some(x) = wce_formats::c_num::strtod(t).filter(|_| t.parse::<f64>().is_ok()) {
        let mut f = x as f32;
        let speed = (f.abs() * 0.005).max(0.001);
        if ins.edit_float(field, label, &mut f, speed, -1e9, 1e9) {
            put(d, elem, attr, &number_like(t, f), g);
        }
    } else {
        let mut s = text.clone();
        if ins.edit_string(field, label, &mut s) {
            put(d, elem, attr, &s, 0);
        }
    }

    if ins.gesture_ended() {
        d.history_mut().break_merge();
    }
}

/// The x / y / z / w attributes of a property as one row.
fn components_row(ins: &mut dyn Inspector, d: &mut XmlFileDocument, p: &Prop, field: &str, label: &str) -> bool {
    let names = ["x", "y", "z", "w"];
    let present = names.iter().take_while(|n| d.xml().find_attr(p.elem, n).is_some()).count();
    if present == 0 {
        return false;
    }

    let texts = names[..present].iter().map(|n| get(d, p.elem, n).unwrap_or_default()).collect::<Vec<_>>();
    let mut v = texts.iter().map(|t| wce_formats::c_num::strtod(t).unwrap_or(0.0) as f32).collect::<Vec<_>>();
    let old = v.clone();
    let changed = match present {
        1 => ins.edit_float(field, label, &mut v[0], 0.01, -1e9, 1e9),
        2 => ins.edit_float2(field, label, (&mut v[..2]).try_into().expect("two"), 0.01),
        3 => ins.edit_float3(field, label, (&mut v[..3]).try_into().expect("three"), 0.01),
        _ => {
            let (a, b) = v.split_at_mut(2);
            let x = ins.edit_float2(&format!("{field}.xy"), &format!("{label} (x, y)"), a.try_into().expect("two"), 0.01);
            let y = ins.edit_float2(&format!("{field}.zw"), &format!("{label} (z, w)"), (&mut b[..2]).try_into().expect("two"), 0.01);
            x || y
        }
    };

    if changed {
        let values = names[..present]
            .iter()
            .enumerate()
            .filter(|&(k, _)| v[k] != old[k])
            .map(|(k, n)| (*n, number_like(&texts[k], v[k])))
            .collect::<Vec<_>>();
        let g = gesture(d, p.elem, "xyzw");
        put_many(d, p.elem, &values, g);
    }

    if ins.gesture_ended() {
        d.history_mut().break_merge();
    }

    true
}

/// A curve's value range for drawing: from 0 (or below, for negative keys) to a round number above the keys.
fn curve_range(points: &[(f32, f32)]) -> (f32, f32) {
    let lo = points.iter().map(|p| p.1).fold(0.0f32, f32::min);
    let top = points.iter().map(|p| p.1).fold(0.0f32, f32::max).max(1e-3) * 1.15;
    let mag = 10f32.powf(top.log10().floor());
    let hi = [1.0, 2.0, 5.0, 10.0].iter().map(|m| m * mag).find(|&s| s >= top).unwrap_or(top);
    let lo = if lo < 0.0 { -([1.0, 2.0, 5.0, 10.0].iter().map(|m| m * mag).find(|&s| s >= -lo).unwrap_or(-lo)) } else { 0.0 };
    (lo, hi)
}

const CURVE_COLOURS: [[f32; 4]; 6] = [
    [0.95, 0.45, 0.35, 1.0],
    [0.4, 0.8, 0.45, 1.0],
    [0.4, 0.6, 1.0, 1.0],
    [0.95, 0.8, 0.3, 1.0],
    [0.8, 0.5, 0.95, 1.0],
    [0.4, 0.9, 0.9, 1.0],
];

impl VfxTool {
    /// A property of an emitter or modifier as its rows.
    fn prop_rows(&mut self, ins: &mut dyn Inspector, d: &mut XmlFileDocument, p: &Prop, dim: bool) {
        let field = format!("{}.{}", p.elem, p.name);
        let lbl = label(&p.name);
        let variant =
            if p.quality > 0 { format!("Quality variant {} (a lower graphics setting's value).", p.quality) } else { String::new() };
        let tip = hint(&p.name, &variant);
        let next = |ins: &mut dyn Inspector| {
            ins.hint(&tip);
            if dim {
                ins.dim_next();
            }
        };

        match p.tag {
            PropTag::Texture => {
                next(ins);
                attr_row(ins, d, p.elem, "path", &field, &lbl);
            }

            // A curve property (SpawnAreaCurve, EmissionRateCurve...) has its curves as text and no value.
            PropTag::Plain => {
                next(ins);
                if !components_row(ins, d, p, &field, &lbl) {
                    attr_row(ins, d, p.elem, "value", &field, &lbl);
                }

                self.curve_rows(ins, d, p, &field, &tip);
            }

            PropTag::Ranged => {
                let (min, max) = (get(d, p.elem, "min"), get(d, p.elem, "max"));
                let colour = min.as_deref().and_then(float_n).filter(|(_, v)| v.len() == 4);
                match (min, max, colour) {
                    (Some(_), Some(max_t), Some((head, lo))) => {
                        let hi = float_n(&max_t).map(|(_, v)| v).unwrap_or_else(|| lo.clone());
                        let mut a = [lo[0].1, lo[1].1, lo[2].1, lo[3].1];
                        let mut b =
                            [hi[0].1, hi.get(1).map_or(0.0, |x| x.1), hi.get(2).map_or(0.0, |x| x.1), hi.get(3).map_or(1.0, |x| x.1)];
                        next(ins);
                        if ins.edit_color4(&format!("{field}.min"), &format!("{lbl} (min)"), &mut a) {
                            let g = gesture(d, p.elem, "min");
                            put(d, p.elem, "min", &write_float_n(&head, &lo, &a), g);
                        }

                        if ins.gesture_ended() {
                            d.history_mut().break_merge();
                        }

                        next(ins);
                        if ins.edit_color4(&format!("{field}.max"), &format!("{lbl} (max)"), &mut b) {
                            let g = gesture(d, p.elem, "max");
                            put(d, p.elem, "max", &write_float_n(&head, &hi, &b), g);
                        }

                        if ins.gesture_ended() {
                            d.history_mut().break_merge();
                        }
                    }

                    (Some(lo_t), Some(hi_t), None) => {
                        let parse = |t: &str| wce_formats::c_num::strtod(t).unwrap_or(0.0) as f32;
                        let mut v = [parse(&lo_t), parse(&hi_t)];
                        let old = v;
                        next(ins);
                        let speed = (v[1].abs() * 0.005).max(0.001);
                        if ins.edit_float2(&field, &format!("{lbl} (min, max)"), &mut v, speed) {
                            let mut values = vec![];
                            if v[0] != old[0] {
                                values.push(("min", number_like(&lo_t, v[0])));
                            }

                            if v[1] != old[1] {
                                values.push(("max", number_like(&hi_t, v[1])));
                            }

                            let g = gesture(d, p.elem, "minmax");
                            put_many(d, p.elem, &values, g);
                        }

                        if ins.gesture_ended() {
                            d.history_mut().break_merge();
                        }
                    }

                    _ => {}
                }

                self.curve_rows(ins, d, p, &field, &tip);
            }
        }
    }

    /// The curves of a ranged property (its text), each a curve editor; a drag plays live and is written when it ends.
    fn curve_rows(&mut self, ins: &mut dyn Inspector, d: &mut XmlFileDocument, p: &Prop, field: &str, tip: &str) {
        let text = match &self.pending {
            Some(pt) if pt.elem == p.elem => pt.text.clone(),
            _ => match d.xml().inner_text(p.elem) {
                Ok(raw) => String::from_utf8_lossy(&decode_entities(&raw)).into_owned(),
                Err(_) => return,
            },
        };

        if text.trim().is_empty() {
            return;
        }

        let mut curves = Curves::parse(&text);
        let mut edited = false;
        let mut ended = false;
        let names = curves.curves.iter().map(|c| c.name().to_owned()).collect::<Vec<_>>();
        for (k, name) in names.iter().enumerate() {
            let Some(c) = curves.find_mut(name) else { continue };
            let mut pts = c.points();
            let (lo, hi) = curve_range(&pts);
            let colour = c.colour().filter(|c| c[0] + c[1] + c[2] > 0.3).unwrap_or(CURVE_COLOURS[k % CURVE_COLOURS.len()]);
            ins.hint(&format!("{tip}\n\nCurve {name}: x is the emitter's (or particle's) life from 0 to 1."));
            let title = if k == 0 { format!("{}: {}", label(&p.name), label(name)) } else { label(name) };
            if ins.edit_curve(&format!("{field}.curve.{name}"), &title, &mut pts, lo, hi, colour) {
                c.set_points(&pts);
                edited = true;
            }

            ended |= ins.gesture_ended();
        }

        if edited {
            self.pending = Some(PendingText { elem: p.elem, text: curves.write() });
        }

        if ended && let Some(pt) = self.pending.take_if(|pt| pt.elem == p.elem) {
            let raw = encode_entities(pt.text.as_bytes(), b'"');
            if let Err(m) = d.set_inner_text(pt.elem, &raw, &format!("Edit the curves of {}", p.name)) {
                wce_foundation::log::warn(format!("vfx: {m}"));
            }
        }
    }

    /// An element's attributes (the `<vfx>` root, render params, a reference) as typed rows, but `skip`.
    fn attrs_rows(&mut self, ins: &mut dyn Inspector, d: &mut XmlFileDocument, elem: usize, skip: &[&str], tips: &dyn Fn(&str) -> String) {
        let names = d.xml().elements()[elem].attrs.iter().map(|a| a.name.clone()).collect::<Vec<_>>();
        for name in names.iter().filter(|n| !skip.contains(&n.as_str())) {
            ins.hint(&tips(name));
            attr_row(ins, d, elem, name, &format!("{elem}.@{name}"), &label(name));
        }
    }
}

/// The Inspector of the open vfx file.
pub fn inspect(t: &mut VfxTool, e: &mut Editor, ins: &mut dyn Inspector) {
    let table = t.defaults.get(e.vfs());
    let Some(d) = document_mut(e) else { return };
    let ix = t.ix.clone();
    let mut action: Option<Action> = None;
    let src = d.source().clone();
    ins.text("file", "File", &if src.vfs_path.is_empty() { src.disk.display().to_string() } else { src.vfs_path.clone() });
    ins.text("from", "From", &src.label);

    // The effect.
    if ins.open_section("Effect") {
        ins.hint(
            "Emitter properties repeat per graphics quality: 0 is the base value, 1 and 2 the values of lower effects settings (fewer \
             particles, shorter culling). A property without a variant of its own uses the base (shown dim).",
        );
        let mut q = t.quality;
        let choices = ["0: high (the base)".to_owned(), "1: medium".to_owned(), "2: low".to_owned()];
        if ins.edit_choice("quality", "Quality variant", &mut q, &choices) {
            t.quality = q.clamp(0, 2);
        }

        ins.hint("Lists only the emitter and modifier properties whose name has this text (the XML name or the label, any case).");
        let mut f = t.prop_filter.clone();
        if ins.edit_string("prop_filter", "Filter properties", &mut f) {
            t.prop_filter = f;
        }

        ins.hint("The Inspector lists only the focused part (the crosshair buttons, here and in the VFX preview panel's Parts).");
        let mut only = t.only_focused;
        if ins.edit_bool("only_focused", "Only the focused part", &mut only) {
            t.only_focused = only;
        }

        if let Some(v) = ix.vfx {
            let tips = |n: &str| -> String {
                let m = match n {
                    "camera_following" => "The effect follows the camera (weather: rain and snow fall around it).",
                    "affected_by_wind" => "The battle's wind moves the effect's particles.",
                    "construction_type" => "How the game builds the effect (1 the usual).",
                    "enable_in_ted" => "TEd plays the effect in its viewport.",
                    _ => "",
                };
                format!("{m}{}XML: <vfx {n}>", if m.is_empty() { "" } else { "\n\n" })
            };
            t.attrs_rows(ins, d, v, &[], &tips);
        }

        if ix.vfx.is_none() && ix.refs.is_empty() {
            ins.text("empty", "Contents", "no <vfx> or references");
        }
    }

    let (only, focus) = (t.only_focused, t.focus.clone());
    let needle = t.prop_filter.trim().to_ascii_lowercase();
    let wanted = |p: &Prop| {
        needle.is_empty() || p.name.to_ascii_lowercase().contains(&needle) || label(&p.name).to_ascii_lowercase().contains(&needle)
    };
    let shown = |p: &Part| !only || focus.as_ref() == Some(p);

    // Emitters.
    for em in &ix.emitters {
        let part = Part::Emitter(em.id.clone());
        if !shown(&part) {
            continue;
        }

        let title = format!("Emitter  {}  ({}){}##em{}", em.id, em.kind, if t.plays(&part) { "" } else { "  (silent)" }, em.elem);
        if !ins.open_section(&title) {
            continue;
        }

        ins.text(
            &format!("{}.chain", em.elem),
            "Drawn with",
            &match ix.chain_of(&em.id) {
                Some(c) => format!("chain {}", c.id),
                None => "no render stack: it draws nothing".to_owned(),
            },
        );
        if let Some(a) = part_buttons(t, ins, &part, em.elem) {
            action = Some(a);
        }

        // Each name once: the chosen variant's, else the base (dim).
        let mut seen = std::collections::BTreeSet::new();
        for g in GROUPS {
            let props = em
                .props
                .iter()
                .filter(|p| group(&p.name) == g && wanted(p) && seen.insert(p.name.clone()))
                .filter_map(|p| pick(&em.props, &p.name, t.quality))
                .cloned()
                .collect::<Vec<_>>();
            if props.is_empty() {
                continue;
            }

            ins.section(g);
            for p in &props {
                t.prop_rows(ins, d, p, p.quality != t.quality);
            }
        }

        if let Some(a) = add_row(ins, d, table.as_ref(), &crate::defaults::emitter_scope(&em.kind), em.elem, &em.props, t.quality) {
            action = Some(a);
        }
    }

    // Behaviour chains (with the focused part only: the focused emitter's).
    for c in &ix.chains {
        let users = ix.users_of_chain(&c.id);
        if only && !users.iter().any(|u| focus.as_ref() == Some(&Part::Emitter(u.clone()))) {
            continue;
        }

        if !ins.open_section(&format!("Behaviour chain  {}##ch{}", c.id, c.elem)) {
            continue;
        }

        ins.text(
            &format!("{}.users", c.elem),
            "Draws",
            &if users.is_empty() { "nothing (no render stack uses it)".to_owned() } else { users.join(", ") },
        );
        let buttons = [
            ("dup", wce_editor::icons::COPY, "Duplicate the chain (the copy is used by nothing until a render stack selects it).", true),
            ("del", wce_editor::icons::TRASH_2, "Delete the chain (only one no render stack uses).", users.is_empty()),
        ];
        ins.text(&format!("{}.edit", c.elem), "Edit", "");
        match ins.row_buttons(&buttons, "") {
            Some(0) => action = Some(Action::DuplicateChain(c.id.clone())),
            Some(1) => action = Some(Action::DeleteChain(c.id.clone())),
            _ => {}
        }

        for m in &c.modifiers {
            let props = m.props.iter().filter(|p| wanted(p)).collect::<Vec<_>>();
            if !needle.is_empty() && props.is_empty() {
                continue;
            }

            ins.section(&m.fragment);
            if needle.is_empty() {
                modifier_preview(ins, d, m);
            }

            for p in props {
                t.prop_rows(ins, d, p, false);
            }

            if needle.is_empty()
                && let Some(a) = add_row(ins, d, table.as_ref(), &crate::defaults::modifier_scope(&m.fragment), m.elem, &m.props, 0)
            {
                action = Some(a);
            }
        }
    }

    // Render stacks.
    if !ix.stacks.is_empty() && !only && ins.open_section("Render stacks") {
        for s in &ix.stacks {
            let emitters = s.emitters.iter().map(|(_, id)| id.as_str()).collect::<Vec<_>>().join(", ");
            let chains = s.chains.iter().map(|(_, id)| id.as_str()).collect::<Vec<_>>().join(", ");
            ins.section(&s.id);
            ins.hint("A render stack draws its emitters with its behaviour chain.");
            ins.text(&format!("{}.what", s.elem), "Draws", &format!("{emitters}  with  {chains}"));
            if let Some(rp) = s.render_params {
                let tips = |n: &str| -> String {
                    let m = match n {
                        "scene_view_type" => "0 the particles pass; 1 the distortion pass (heat haze).",
                        "cast_shadows" => "The particles cast shadows.",
                        _ => "",
                    };
                    format!("{m}{}XML: <render_params {n}>", if m.is_empty() { "" } else { "\n\n" })
                };
                t.attrs_rows(ins, d, rp, &[], &tips);
            }
        }
    }

    // References.
    for r in &ix.refs {
        let part = Part::Reference(r.inst_name.clone());
        if !shown(&part) {
            continue;
        }

        let title =
            format!("Reference  {}  ->  {}{}##ref{}", r.inst_name, r.library, if t.plays(&part) { "" } else { "  (silent)" }, r.elem);
        if !ins.open_section(&title) {
            continue;
        }

        if let Some(a) = part_buttons(t, ins, &part, r.elem) {
            action = Some(a);
        }

        ins.text(&format!("{}.lib", r.elem), "Library", "");
        if ins.row_buttons(
            &[("open", wce_editor::icons::FOLDER_OPEN, "Open the library effect this reference plays.", true)],
            &format!("vfx/{}.xml", r.library),
        ) == Some(0)
        {
            action = Some(Action::OpenLibrary(r.library.clone()));
        }

        let tips = |n: &str| -> String {
            let m = match n {
                "pos_x" | "pos_y" | "pos_z" => "Where the library effect plays, in metres from this effect's origin.",
                "scale_min" | "scale_max" => "The library effect's scale: a random one between min and max per placed effect.",
                "start_time" => "Seconds before it starts.",
                "infinite_life" => "Its emitters keep emitting past their life time.",
                "vfx_ref" => "The library effect (vfx/<name>.xml).",
                _ => "",
            };
            format!("{m}{}XML: <vfx {n}>", if m.is_empty() { "" } else { "\n\n" })
        };
        t.attrs_rows(ins, d, r.elem, &[], &tips);
        for re in &r.emitters {
            ins.section(&format!("emitter {}", re.name));
            reference_emitter_rows(ins, d, re.elem);
        }
    }

    match action {
        Some(Action::Solo(p)) => t.solo = if t.solo.as_ref() == Some(&p) { None } else { Some(p) },
        Some(Action::Mute(p)) => {
            if !t.muted.remove(&p) {
                t.muted.insert(p);
            }
        }
        Some(Action::Focus(p)) => t.focus = if t.focus.as_ref() == Some(&p) { None } else { Some(p) },
        Some(Action::OpenLibrary(lib)) => t.open_library(&lib),
        Some(Action::AddProperty { parent, text, label }) => {
            let Some(d) = document_mut(e) else { return };
            match d.add_child_text(parent, text.as_bytes(), &label) {
                Ok(_) => e.status(&label, false),
                Err(m) => e.status(&format!("{label}: {m}"), true),
            }
        }
        Some(a) => {
            let Some(d) = document_mut(e) else { return };
            let result = match &a {
                Action::Duplicate(Part::Emitter(id)) => crate::ops::duplicate_emitter(d, id).map(|n| format!("duplicated as {n}")),
                Action::Delete(Part::Emitter(id)) => crate::ops::delete_emitter(d, id),
                Action::Duplicate(Part::Reference(n)) => crate::ops::duplicate_reference(d, n).map(|n| format!("duplicated as {n}")),
                Action::Delete(Part::Reference(n)) => crate::ops::delete_reference(d, n),
                Action::DuplicateChain(id) => crate::ops::duplicate_chain(d, id).map(|n| format!("duplicated as {n}")),
                Action::DeleteChain(id) => crate::ops::delete_chain(d, id),
                _ => Ok(String::new()),
            };

            match result {
                Ok(m) => e.status(&m, false),
                Err(m) => e.status(&m, true),
            }
        }
        None => {}
    }
}

/// What a button row asked for (applied after the Inspector is drawn: structural edits renumber the elements).
enum Action {
    Solo(Part),
    Mute(Part),
    Focus(Part),
    Duplicate(Part),
    Delete(Part),
    DuplicateChain(String),

    /// A property's element added to an emitter or modifier.
    AddProperty {
        parent: usize,
        text: String,
        label: String,
    },
    DeleteChain(String),
    OpenLibrary(String),
}

fn part_buttons(t: &VfxTool, ins: &mut dyn Inspector, part: &Part, elem: usize) -> Option<Action> {
    let soloed = t.solo.as_ref() == Some(part);
    let muted = t.muted.contains(part);
    let focused = t.focus.as_ref() == Some(part);
    let state = match (soloed, muted, focused) {
        (true, _, _) => "soloed",
        (_, true, _) => "muted",
        _ if !t.plays(part) => "silent (another part is soloed)",
        _ => "plays",
    };
    let note = if focused { format!("{state}, focused") } else { state.to_owned() };
    ins.text(&format!("{elem}.preview"), "Preview", "");
    let buttons = [
        ("solo", "S", "Solo: only this part plays (preview only, not saved).", true),
        ("mute", "M", "Mute: this part does not play (preview only, not saved).", true),
        (
            "focus",
            wce_editor::icons::CROSSHAIR,
            "Focus: its handles in the viewport, and the gizmo's target when it moves the focused part.",
            true,
        ),
        ("dup", wce_editor::icons::COPY, "Duplicate it (an emitter with its render stack, so the copy draws).", true),
        ("del", wce_editor::icons::TRASH_2, "Delete it (an emitter with its place in the render stacks).", true),
    ];
    match ins.row_buttons(&buttons, &note)? {
        0 => Some(Action::Solo(part.clone())),
        1 => Some(Action::Mute(part.clone())),
        2 => Some(Action::Focus(part.clone())),
        3 => Some(Action::Duplicate(part.clone())),
        _ => Some(Action::Delete(part.clone())),
    }
}

/// The "Add a property" row of an emitter or modifier: the properties vanilla's of its kind have and it lacks, written as vanilla's most
/// common text; with a lower quality variant edited, also a variant of a property it has only in the base.
fn add_row(
    ins: &mut dyn Inspector,
    d: &XmlFileDocument,
    table: Option<&crate::defaults::Table>,
    scope: &str,
    parent: usize,
    props: &[Prop],
    quality: i32,
) -> Option<Action> {
    let field = format!("{parent}.add");
    let Some(table) = table else {
        ins.text(&field, "Add a property", "reading the game's effects...");
        return None;
    };

    let has = |name: &str, q: i32| props.iter().any(|p| p.name == name && p.quality == q);
    let mut offers = vec![];
    if quality > 0 {
        for p in props.iter().filter(|p| p.quality == 0 && !has(&p.name, quality)) {
            let text = crate::defaults::with_quality(&crate::defaults::element_text(d.xml(), p.elem), quality);
            offers.push((
                format!("{} (quality {quality} variant)", p.name),
                text,
                format!("Add the quality {quality} variant of {}", p.name),
            ));
        }
    }

    for tpl in table.get(scope).into_iter().flatten().filter(|tpl| !has(&tpl.name, 0)) {
        offers.push((
            format!("{}  ({} of vanilla's {} have it)", tpl.name, tpl.uses, scope.split(':').nth(1).unwrap_or_default()),
            tpl.text.clone(),
            format!("Add {}", tpl.name),
        ));
    }

    if offers.is_empty() {
        return None;
    }

    let mut choices = vec![format!("add one of {}...", offers.len())];
    choices.extend(offers.iter().map(|o| o.0.clone()));
    let tips = std::iter::once(String::new()).chain(offers.iter().map(|o| format!("Added as:\n{}", o.1))).collect::<Vec<_>>();
    ins.hint(
        "A property this one lacks, as the game's own effects of its kind write it (their most common value: the engine's own defaults          are not decoded). With a lower quality variant edited, a variant of a property it has only in the base.",
    );
    let mut i = 0;
    if ins.edit_choice_tips(&field, "Add a property", &mut i, &choices, &tips) && i > 0 {
        let (_, text, label) = offers.swap_remove(i as usize - 1);
        return Some(Action::AddProperty { parent, text, label });
    }

    None
}

/// A reference's override of one library emitter: on or off, its tint range, its lifetime biases.
fn reference_emitter_rows(ins: &mut dyn Inspector, d: &mut XmlFileDocument, elem: usize) {
    ins.hint("The library emitter plays in this effect.\n\nXML: <emitter enable>");
    attr_row(ins, d, elem, "enable", &format!("{elem}.enable"), "Enable");
    for (which, tip) in [("min", "The tint's one end: each placed effect draws a blend of min and max."), ("max", "The tint's other end.")]
    {
        let names = ["r", "g", "b", "a"].map(|c| format!("tint_{which}_{c}"));
        if names.iter().all(|n| d.xml().find_attr(elem, n).is_none()) {
            continue;
        }

        let texts = names.clone().map(|n| get(d, elem, &n).unwrap_or_else(|| "1.00".to_owned()));
        let mut c = texts.clone().map(|t| wce_formats::c_num::strtod(&t).unwrap_or(1.0) as f32);
        let old = c;
        ins.hint(&format!("{tip}\n\nXML: tint_{which}_r, _g, _b, _a"));
        if ins.edit_color4(&format!("{elem}.tint_{which}"), &format!("Tint ({which})"), &mut c) {
            let values = (0..4).filter(|&k| c[k] != old[k]).map(|k| (names[k].as_str(), number_like(&texts[k], c[k]))).collect::<Vec<_>>();
            let g = gesture(d, elem, &format!("tint_{which}"));
            put_many(d, elem, &values, g);
        }

        if ins.gesture_ended() {
            d.history_mut().break_merge();
        }
    }

    for (base, lbl, tip) in [
        ("particle_lifetime_bias", "Particle life bias", "Seconds added to each particle's life (min to max)."),
        ("emitter_lifetime_bias", "Emitter life bias", "Seconds added to the emitter's life (min to max)."),
    ] {
        let (lo_n, hi_n) = (format!("{base}_min"), format!("{base}_max"));
        let (Some(lo_t), Some(hi_t)) = (get(d, elem, &lo_n), get(d, elem, &hi_n)) else { continue };
        let parse = |t: &str| wce_formats::c_num::strtod(t).unwrap_or(0.0) as f32;
        let mut v = [parse(&lo_t), parse(&hi_t)];
        let old = v;
        ins.hint(&format!("{tip}\n\nXML: {lo_n}, {hi_n}"));
        if ins.edit_float2(&format!("{elem}.{base}"), &format!("{lbl} (min, max)"), &mut v, 0.01) {
            let mut values = vec![];
            if v[0] != old[0] {
                values.push((lo_n.as_str(), number_like(&lo_t, v[0])));
            }

            if v[1] != old[1] {
                values.push((hi_n.as_str(), number_like(&hi_t, v[1])));
            }

            let g = gesture(d, elem, base);
            put_many(d, elem, &values, g);
        }

        if ins.gesture_ended() {
            d.history_mut().break_merge();
        }
    }
}

/// A picture of what a modifier does: the colour ramp of `vs_colour`, the opacity over life of `vs_vertex_alpha`.
fn modifier_preview(ins: &mut dyn Inspector, d: &XmlFileDocument, m: &crate::model::Modifier) {
    let value = |name: &str, attr: &str| m.props.iter().find(|p| p.name == name).and_then(|p| d.xml().get(p.elem, attr));
    let num =
        |name: &str, attr: &str, default: f32| value(name, attr).and_then(|t| wce_formats::c_num::strtod(&t)).map_or(default, |v| v as f32);
    let lerp = (num("lerp_time", "min", 0.5) + num("lerp_time", "max", 0.5)) * 0.5;
    match m.fragment.as_str() {
        "vs_colour" => {
            let colour = |name: &str| {
                value(name, "min")
                    .and_then(|t| float_n(&t))
                    .map_or([1.0; 4], |(_, v)| std::array::from_fn(|k| v.get(k).map_or(1.0, |x| x.1)))
            };
            let stops = [(0.0, colour("start_colour")), (lerp.clamp(0.0, 1.0), colour("mid_colour")), (1.0, colour("end_colour"))];
            ins.hint("The colour over the particle's life (the min colours), the mid one at the middle of lerp time.");
            ins.gradient(&format!("{}.ramp", m.elem), "Over its life", &stops);
        }

        "vs_vertex_alpha" => {
            let (a, b) = (num("lerp_time", "min", 0.25).clamp(0.0, 1.0), num("lerp_time", "max", 0.75).clamp(0.0, 1.0));
            let keys = [
                (0.0, num("initial_alpha", "value", 1.0)),
                (a, num("mid_alpha_a", "value", 1.0)),
                (b.max(a), num("mid_alpha_b", "value", 1.0)),
                (1.0, num("end_alpha", "value", 1.0)),
            ];
            let curve = crate::curves::sample(&keys, 64);
            ins.hint("The opacity over the particle's life: initial, the two mid values at lerp time's min and max, end.");
            ins.plot(&format!("{}.alpha", m.elem), "Opacity over its life", &[([0.9, 0.9, 0.9, 1.0], curve)], "death");
        }

        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_keep_their_style() {
        assert_eq!(number_like("0.00", 1.5), "1.50");
        assert_eq!(number_like("0.00", 0.125), "0.125");
        assert_eq!(number_like("3", 4.0), "4");
        assert_eq!(number_like("3", 4.5), "4.5");
        assert_eq!(number_like("1.0", 2.0), "2.0");
        assert_eq!(
            write_float_n("float4", &float_n("float4(1.0,0.5,0.25,1.0)").unwrap().1, &[1.0, 0.75, 0.25, 1.0]),
            "float4(1.0,0.75,0.25,1.0)"
        );
    }

    #[test]
    fn curve_ranges_are_round() {
        assert_eq!(curve_range(&[(0.0, 0.3), (1.0, 0.8)]), (0.0, 1.0));
        assert_eq!(curve_range(&[(0.0, 12.0)]), (0.0, 20.0));
        assert_eq!(curve_range(&[(0.0, -0.3), (1.0, 0.4)]), (-0.5, 0.5));
    }
}
