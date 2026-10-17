//! XML -> UI binary. A port of `ui/lib/xml2ui.rb` of taw/etwng (MIT): the XML is a primitive stream, so this walks
//! it and appends each primitive; only a handful of tags carry structure (array counts, `events`, `uientry`,
//! `additional_data`).

use crate::xml_lite::{Ev, line_col, parse};

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Text(Text),
    Yes,
    No,
    Array,
    Pass,
    Events,
    Uientry,
    AdditionalData,
    TopLevel,
}

#[derive(Clone, Copy, PartialEq)]
enum Text {
    U,
    I,
    U2,
    I2,
    Byte,
    Flt,
    S,
    Unicode,
    Data,
    Uuid,
}

fn kind_of(name: &str) -> Option<Kind> {
    Some(match name {
        "u" => Kind::Text(Text::U),
        "i" => Kind::Text(Text::I),
        "u2" => Kind::Text(Text::U2),
        "i2" => Kind::Text(Text::I2),
        "byte" => Kind::Text(Text::Byte),
        "flt" => Kind::Text(Text::Flt),
        "s" | "key" | "value" => Kind::Text(Text::S),
        "unicode" => Kind::Text(Text::Unicode),
        "data" => Kind::Text(Text::Data),
        "uuid" => Kind::Text(Text::Uuid),
        "yes" => Kind::Yes,
        "no" => Kind::No,
        "images" | "states" | "image_uses" | "transitions" | "effects" | "children" | "phases" | "table" | "row" | "dynamics" | "funcs" | "anims"
        | "anim_attrs" | "mouse_states" | "mouse_state_data" | "subtemplates" | "properties" | "materialdata" | "array" => Kind::Array,
        "fcentry" | "image" | "state" | "image_use" | "transition" | "effect" | "phase" | "col" | "color" | "dynamic" | "func" | "anim" | "anim_attr"
        | "mouse_state" | "mouse_state_datapoint" | "model" | "models" | "template" | "subtemplate" | "event" | "property" | "material"
        | "materialdatapoint" | "sound" => Kind::Pass,
        "events" => Kind::Events,
        "uientry" => Kind::Uientry,
        "additional_data" => Kind::AdditionalData,
        "ui" | "cml" | "fc" | "twui_images" => Kind::TopLevel,
        _ => return None,
    })
}

/// True for tags whose elements are counted: the array tags of the format.
pub fn is_array_tag(name: &str) -> bool {
    kind_of(name) == Some(Kind::Array)
}

struct Frame {
    name: String,
    kind: Kind,
    can_nest: bool,
    text: String,
    start: usize,
}

fn int(s: &str) -> Result<i64, String> {
    let t = s.trim();
    t.parse::<i64>().map_err(|_| format!("not an integer: {:?}", t))
}

pub fn convert(xml: &str) -> Result<Vec<u8>, String> {
    let evs = parse(xml).map_err(|e| {
        let (l, c) = line_col(xml, e.offset);
        format!("XML error at {}:{}: {}", l, c, e.msg)
    })?;
    let mut out: Vec<u8> = Vec::with_capacity(xml.len() / 4);
    let mut stack: Vec<Frame> = Vec::new();
    let mut ver: u32 = 0;
    let mut saw_root = false;

    for (at, ev) in evs {
        let fail = |msg: String| {
            let (l, c) = line_col(xml, at);
            format!("{}:{}: {}", l, c, msg)
        };
        match ev {
            Ev::Start(name, attrs) => {
                if let Some(top) = stack.last() {
                    if !top.can_nest {
                        return Err(fail(format!("<{}> cannot be nested in <{}>", name, top.name)));
                    }
                } else if saw_root {
                    return Err(fail("more than one root element".into()));
                }
                let kind = kind_of(&name).ok_or_else(|| fail(format!("unknown tag <{}>", name)))?;
                let attr = |k: &str| attrs.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str());
                let mut can_nest = true;
                match kind {
                    Kind::TopLevel => {
                        saw_root = true;
                        let v = attr("version").ok_or_else(|| fail(format!("<{}> needs a version attribute", name)))?;
                        let v = int(v).map_err(&fail)?;
                        if !(0..=999).contains(&v) {
                            return Err(fail(format!("version {} out of range", v)));
                        }
                        ver = v as u32;
                        out.extend_from_slice(format!("Version{:03}", v).as_bytes());
                    }
                    Kind::Array => {
                        let c = attr("count").ok_or_else(|| fail(format!("<{}> needs a count attribute", name)))?;
                        out.extend_from_slice(&(int(c).map_err(&fail)? as u32).to_le_bytes());
                    }
                    Kind::Events => {
                        if ver > 54 {
                            let c = attr("count").ok_or_else(|| fail("<events> needs a count attribute".into()))?;
                            out.extend_from_slice(&(int(c).map_err(&fail)? as u32).to_le_bytes());
                        }
                    }
                    Kind::Uientry => {
                        if attr("type") == Some("normal") {
                            out.extend_from_slice(&[0, 0]);
                        }
                    }
                    Kind::AdditionalData => {
                        let t = attr("type").ok_or_else(|| fail("<additional_data> needs a type attribute".into()))?;
                        if t == "none" {
                            out.push(0);
                            can_nest = false;
                        } else {
                            out.push(1);
                            put_str(&mut out, t).map_err(&fail)?;
                        }
                    }
                    Kind::Text(_) | Kind::Yes | Kind::No => can_nest = false,
                    Kind::Pass => {}
                }
                if kind == Kind::Yes {
                    out.push(1);
                }
                if kind == Kind::No {
                    out.push(0);
                }
                stack.push(Frame { name, kind, can_nest, text: String::new(), start: at });
            }
            Ev::Comment(_) => {}
            Ev::Text(t) => match stack.last_mut() {
                Some(f) if matches!(f.kind, Kind::Text(_)) => f.text.push_str(&t.replace('\u{e01f}', "\u{1f}")),
                _ => {
                    if t.chars().any(|c| !c.is_whitespace()) {
                        return Err(fail(format!("text is not allowed here: {:?}", t.trim())));
                    }
                }
            },
            Ev::End(name) => {
                let f = stack.pop().ok_or_else(|| fail("unbalanced end tag".into()))?;
                if f.name != name {
                    return Err(fail(format!("</{}> closes <{}>", name, f.name)));
                }
                let ferr = |m: String| {
                    let (l, c) = line_col(xml, f.start);
                    format!("{}:{}: <{}>: {}", l, c, f.name, m)
                };
                match f.kind {
                    Kind::Text(t) => put_text(&mut out, t, &f.text).map_err(ferr)?,
                    Kind::Events => {
                        if ver <= 54 {
                            put_str(&mut out, "events_end").map_err(ferr)?;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    if !stack.is_empty() {
        return Err("unclosed element".into());
    }
    if !saw_root {
        return Err("no root element".into());
    }
    Ok(out)
}

fn put_str(out: &mut Vec<u8>, s: &str) -> Result<(), String> {
    if s.len() > 0xffff {
        return Err("string longer than 65535 bytes".into());
    }
    out.extend_from_slice(&(s.len() as u16).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
    Ok(())
}

fn put_text(out: &mut Vec<u8>, t: Text, s: &str) -> Result<(), String> {
    match t {
        Text::U | Text::I => out.extend_from_slice(&(int(s)? as u32).to_le_bytes()),
        Text::U2 | Text::I2 => out.extend_from_slice(&(int(s)? as u16).to_le_bytes()),
        Text::Byte => {
            let v = int(s)?;
            if !(0..=255).contains(&v) {
                return Err(format!("byte out of range: {}", v));
            }
            out.push(v as u8);
        }
        Text::Flt => {
            let v: f64 = s.trim().parse().map_err(|_| format!("not a number: {:?}", s.trim()))?;
            out.extend_from_slice(&(v as f32).to_le_bytes());
        }
        Text::S => put_str(out, s)?,
        Text::Unicode => {
            let units: Vec<u16> = s.encode_utf16().collect();
            if units.len() > 0xffff {
                return Err("string longer than 65535 characters".into());
            }
            out.extend_from_slice(&(units.len() as u16).to_le_bytes());
            for u in units {
                out.extend_from_slice(&u.to_le_bytes());
            }
        }
        Text::Data => {
            for w in s.split_whitespace() {
                out.push(u8::from_str_radix(w, 16).map_err(|_| format!("bad hex byte {:?}", w))?);
            }
        }
        Text::Uuid => {
            let h: String = s.trim().chars().filter(|c| *c != '-').collect();
            if h.len() != 32 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(format!("bad uuid {:?}", s.trim()));
            }
            for i in 0..16 {
                out.push(u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).unwrap());
            }
        }
    }
    Ok(())
}
