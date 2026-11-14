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
        "images" | "states" | "image_uses" | "transitions" | "effects" | "children" | "phases" | "table" | "row" | "dynamics" | "funcs"
        | "anims" | "anim_attrs" | "mouse_states" | "mouse_state_data" | "subtemplates" | "properties" | "materialdata" | "array" => {
            Kind::Array
        }
        "fcentry"
        | "image"
        | "state"
        | "image_use"
        | "transition"
        | "effect"
        | "phase"
        | "col"
        | "color"
        | "dynamic"
        | "func"
        | "anim"
        | "anim_attr"
        | "mouse_state"
        | "mouse_state_datapoint"
        | "model"
        | "models"
        | "template"
        | "subtemplate"
        | "event"
        | "property"
        | "material"
        | "materialdatapoint"
        | "sound" => Kind::Pass,
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
                        out.extend_from_slice(&count(c).map_err(&fail)?.to_le_bytes());
                    }
                    Kind::Events => {
                        if ver > 54 {
                            let c = attr("count").ok_or_else(|| fail("<events> needs a count attribute".into()))?;
                            out.extend_from_slice(&count(c).map_err(&fail)?.to_le_bytes());
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
                Some(f) if matches!(f.kind, Kind::Text(_)) => f.text.push_str(&t),
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
                    Kind::Events if ver <= 54 => {
                        put_str(&mut out, "events_end").map_err(ferr)?;
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

/// An integer of a field `bits` wide, signed or unsigned: from the smallest signed value to the largest unsigned one.
/// A count of an array or events block: what its 32 bits can hold (the reader writes it unsigned), else an error, not wrapped.
fn count(s: &str) -> Result<u32, String> {
    let v = int(s)?;
    u32::try_from(v).map_err(|_| format!("count {v} does not fit 32 bits"))
}

fn in_width(s: &str, bits: u32) -> Result<i64, String> {
    let v = int(s)?;
    let (lo, hi) = (-(1i64 << (bits - 1)), (1i64 << bits) - 1);
    if (lo..=hi).contains(&v) { Ok(v) } else { Err(format!("{v} does not fit a {bits}-bit field")) }
}

fn put_text(out: &mut Vec<u8>, t: Text, s: &str) -> Result<(), String> {
    match t {
        // Signed or unsigned, as the reader writes it: what the field's width can hold, else an error (70000 in a <u2> was 4464).
        Text::U | Text::I => out.extend_from_slice(&(in_width(s, 32)? as u32).to_le_bytes()),
        Text::U2 | Text::I2 => out.extend_from_slice(&(in_width(s, 16)? as u16).to_le_bytes()),
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

#[cfg(test)]
mod tests {
    use super::{convert, in_width};

    #[test]
    fn an_integer_must_fit_its_field_signed_or_unsigned() {
        assert_eq!(in_width("65535", 16), Ok(65535));
        assert_eq!(in_width("-32768", 16), Ok(-32768));
        assert!(in_width("70000", 16).is_err(), "was written as 4464");
        assert!(in_width("-32769", 16).is_err());
        assert_eq!(in_width("4294967295", 32), Ok(4_294_967_295));
        assert_eq!(in_width("-1", 32), Ok(-1));
        assert!(in_width("4294967296", 32).is_err());
    }

    /// The bytes of a `<cml>` document with the given body.
    fn cml(body: &str) -> Result<Vec<u8>, String> {
        convert(&format!("<cml version=\"002\">{body}</cml>"))
    }

    #[test]
    fn each_primitive_is_written_little_endian() {
        let xml = "<ui version=\"070\"><u>4294967295</u><i>-2</i><u2>65535</u2><i2>-1</i2><byte>255</byte><flt>0.1</flt><flt>-0.0</flt>\
                   <s>\u{fc}</s><unicode>\u{1f600}</unicode><data>00 ff\n 7f</data><uuid>10325476-98ba-dcfe-0123-456789abcdef</uuid><yes/><no/>\
                   <array count=\"3\"/></ui>";
        let mut want = b"Version070".to_vec();
        want.extend_from_slice(&[0xff; 4]);
        want.extend_from_slice(&(-2i32).to_le_bytes());
        want.extend_from_slice(&[0xff, 0xff, 0xff, 0xff, 0xff]);
        want.extend_from_slice(&0.1f32.to_le_bytes());
        want.extend_from_slice(&(-0.0f32).to_le_bytes());
        want.extend_from_slice(&[2, 0, 0xc3, 0xbc]);
        want.extend_from_slice(&[2, 0, 0x3d, 0xd8, 0x00, 0xde]);
        want.extend_from_slice(&[0x00, 0xff, 0x7f]);
        want.extend_from_slice(&[0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]);
        want.extend_from_slice(&[1, 0, 3, 0, 0, 0]);
        assert_eq!(convert(xml).unwrap(), want);
    }

    #[test]
    fn structure_tags_write_what_their_version_needs() {
        // Events: a count from version 55 on, an `events_end` string before.
        assert_eq!(
            convert("<ui version=\"054\"><events><event><s>a</s></event></events></ui>").unwrap(),
            b"Version054\x01\x00a\x0a\x00events_end"
        );
        assert_eq!(
            convert("<ui version=\"055\"><events count=\"1\"><event><s>a</s></event></events></ui>").unwrap(),
            b"Version055\x01\x00\x00\x00\x01\x00a"
        );
        assert!(convert("<ui version=\"055\"><events/></ui>").is_err());

        // A normal uientry is marked by two zero bytes; an additional_data is a flag and its type.
        assert_eq!(convert("<ui version=\"100\"><uientry type=\"normal\"/><uientry/></ui>").unwrap(), b"Version100\x00\x00");
        assert_eq!(
            convert("<ui version=\"100\"><additional_data type=\"none\"/><additional_data type=\"List\"/></ui>").unwrap(),
            b"Version100\x00\x01\x04\x00List"
        );
        assert!(convert("<ui version=\"100\"><additional_data type=\"none\"><u>1</u></additional_data></ui>").is_err());
        assert!(convert("<ui version=\"100\"><additional_data/></ui>").is_err());

        // Every array tag needs a count; the pass-through tags write nothing.
        assert!(super::is_array_tag("children") && super::is_array_tag("properties") && !super::is_array_tag("uientry"));
        assert!(convert("<ui version=\"100\"><children/></ui>").is_err());
        assert_eq!(convert("<ui version=\"100\"><state><image/></state></ui>").unwrap(), b"Version100");
        assert_eq!(cml("<!-- comment --><key/><value>&lt;&amp;&gt;</value>").unwrap(), b"Version002\x00\x00\x03\x00<&>");
    }

    #[test]
    fn values_out_of_their_range_are_errors() {
        for body in [
            "<u>4294967296</u>",
            "<i>-2147483649</i>",
            "<u2>70000</u2>",
            "<i2>-32769</i2>",
            "<byte>256</byte>",
            "<byte>-1</byte>",
            "<u>1.5</u>",
            "<u></u>",
            "<flt>one</flt>",
            "<flt></flt>",
            "<data>0g</data>",
            "<data>100</data>",
            "<uuid>1032547698badcfe0123456789abcde</uuid>",
            "<uuid>1032547698badcfe0123456789abcdeg</uuid>",
            "<uuid>1032547698badcfe0123456789abcdef00</uuid>",
        ] {
            assert!(cml(body).is_err(), "{body}");
        }

        // The longest strings fit, one more byte or UTF-16 unit does not.
        assert!(cml(&format!("<s>{}</s>", "a".repeat(0xffff))).is_ok());
        assert!(cml(&format!("<s>{}</s>", "a".repeat(0x10000))).is_err());
        assert!(cml(&format!("<s>{}</s>", "\u{fc}".repeat(0x8000))).is_err());
        assert!(cml(&format!("<unicode>{}</unicode>", "\u{fc}".repeat(0xffff))).is_ok());
        assert!(cml(&format!("<unicode>{}</unicode>", "a".repeat(0x10000))).is_err());
        assert!(convert(&format!("<ui version=\"100\"><additional_data type=\"{}\"/></ui>", "a".repeat(0x10000))).is_err());
    }

    #[test]
    fn malformed_documents_are_errors() {
        for xml in [
            "",
            "   ",
            "<!-- only a comment -->",
            "<ui></ui>",
            "<ui version=\"x\"></ui>",
            "<ui version=\"1000\"></ui>",
            "<ui version=\"-1\"></ui>",
            "<unknown version=\"002\"/>",
            "<cml version=\"002\"><nope/></cml>",
            "<cml version=\"002\">text</cml>",
            "<cml version=\"002\"></cml>trailing",
            "<cml version=\"002\"></cml><cml version=\"002\"></cml>",
            "<cml version=\"002\"><s><s>a</s></s></cml>",
            "<cml version=\"002\"><yes><no/></yes></cml>",
            "<cml version=\"002\"><s>a</u></cml>",
            "<cml version=\"002\"><s>a</s>",
            "<cml version=\"002\"><s>a</s></cml></cml>",
            "<cml version=\"002\"><s a=b>x</s></cml>",
            "<cml version=\"002\"><s>&bogus;</s></cml>",
        ] {
            let e = convert(xml);
            assert!(e.is_err(), "{xml:?} gave {e:?}");
        }

        // Errors say where.
        let e = convert("<cml version=\"002\">\n  <bad/>\n</cml>").unwrap_err();
        assert!(e.starts_with("2:3:"), "{e}");
    }

    #[test]
    fn an_array_count_must_fit_32_bits() {
        assert!(convert("<ui version=\"100\"><children count=\"4294967296\"/></ui>").is_err());
        assert!(convert("<ui version=\"100\"><events count=\"4294967296\"/></ui>").is_err());
    }
}
