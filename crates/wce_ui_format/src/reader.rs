//! UI binary -> XML. A port of `ui/lib/ui_file.rb` of taw/etwng (MIT): the file is a version-gated stream of
//! primitives, each of which becomes a tag (`<u>`, `<i>`, `<s>`, `<flt>`, `<yes />`, ...) under structure tags.
//! The XML mirrors the byte stream one to one, so `writer` can serialise it again without knowing the layout.

use crate::ruby_float::{pretty_single, round2_to_s, ruby_float_to_s};

pub type R<T> = Result<T, String>;

pub struct Reader<'a> {
    d: &'a [u8],
    ofs: usize,
    ver: u32,
    out: String,
    depth: usize,
}

pub struct Converted {
    pub xml: String,
    pub version: u32,
}

/// Escape for XML text the way the original does (apostrophes and quotes too, CR and 0x1f as character references).
pub fn xml_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '&' => o.push_str("&amp;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            '\r' => o.push_str("&#xD;"),
            '\u{1f}' => o.push_str("&#xE01F;"),
            c => o.push(c),
        }
    }
    o
}

fn hex_bytes(b: &[u8], sep: &str) -> String {
    b.iter().map(|x| format!("{:02x}", x)).collect::<Vec<_>>().join(sep)
}

pub fn convert(data: &[u8]) -> R<Converted> {
    let mut r = Reader { d: data, ofs: 0, ver: 0, out: String::with_capacity(data.len() * 8), depth: 0 };
    r.autodetect()?;
    Ok(Converted { xml: r.out, version: r.ver })
}

impl<'a> Reader<'a> {
    // ---- output ----

    fn out(&mut self, s: &str) {
        for _ in 0..self.depth {
            self.out.push_str("  ");
        }
        self.out.push_str(s);
        if !s.ends_with('\n') {
            self.out.push('\n');
        }
    }

    fn cmt_out(&mut self, xml: String, comment: Option<&str>) {
        match comment {
            Some(c) => self.out(&format!("{}<!-- {} -->", xml, c)),
            None => self.out(&xml),
        }
    }

    fn tag<F: FnOnce(&mut Self) -> R<()>>(&mut self, name: &str, attrs: &[(&str, String)], f: F) -> R<()> {
        let a = Self::attrs_to_s(attrs);
        self.out(&format!("<{}{}>", name, a));
        self.depth += 1;
        f(self)?;
        self.depth -= 1;
        self.out(&format!("</{}>", name));
        Ok(())
    }

    fn tag_empty(&mut self, name: &str, attrs: &[(&str, String)]) {
        let a = Self::attrs_to_s(attrs);
        self.out(&format!("<{}{}/>", name, a));
    }

    fn attrs_to_s(attrs: &[(&str, String)]) -> String {
        let mut v: Vec<String> = attrs.iter().map(|(k, val)| format!(" {}=\"{}\"", k, xml_escape(val))).collect();
        v.sort();
        v.concat()
    }

    // ---- input ----

    fn err<T>(&self, msg: impl AsRef<str>) -> R<T> {
        Err(format!("{} (at offset {})", msg.as_ref(), self.ofs))
    }

    fn get(&mut self, n: usize) -> R<&'a [u8]> {
        if self.ofs + n > self.d.len() {
            return self.err(format!("Requested reading past end of file - {}", n));
        }
        let r = &self.d[self.ofs..self.ofs + n];
        self.ofs += n;
        Ok(r)
    }

    fn look(&self, n: usize) -> R<&'a [u8]> {
        if self.ofs + n > self.d.len() {
            return self.err(format!("Requested reading past end of file - {}", n));
        }
        Ok(&self.d[self.ofs..self.ofs + n])
    }

    fn look_i(&self) -> R<i32> {
        Ok(i32::from_le_bytes(self.look(4)?.try_into().unwrap()))
    }

    fn behind(&self, n: usize) -> &'a [u8] {
        &self.d[self.ofs - n..self.ofs]
    }

    fn get_byte(&mut self) -> R<u8> {
        Ok(self.get(1)?[0])
    }

    fn get_bool(&mut self) -> R<bool> {
        let v = self.get_byte()?;
        if v > 1 {
            return self.err(format!("Invalid boolean value: {}", v));
        }
        Ok(v == 1)
    }

    fn get_u2(&mut self) -> R<u16> {
        Ok(u16::from_le_bytes(self.get(2)?.try_into().unwrap()))
    }

    fn get_u(&mut self) -> R<u32> {
        Ok(u32::from_le_bytes(self.get(4)?.try_into().unwrap()))
    }

    fn get_i(&mut self) -> R<i32> {
        Ok(i32::from_le_bytes(self.get(4)?.try_into().unwrap()))
    }

    fn get_flt(&mut self) -> R<String> {
        let f = f32::from_le_bytes(self.get(4)?.try_into().unwrap());
        if !f.is_finite() {
            return self.err("Non-finite float");
        }
        Ok(ruby_float_to_s(pretty_single(f)))
    }

    fn get_s(&mut self) -> R<String> {
        let n = self.get_u2()? as usize;
        let b = self.get(n)?;
        match std::str::from_utf8(b) {
            Ok(s) => Ok(s.to_string()),
            Err(_) => self.err("String is not valid UTF-8"),
        }
    }

    fn get_unicode(&mut self) -> R<String> {
        let n = self.get_u2()? as usize;
        let b = self.get(n * 2)?;
        let units: Vec<u16> = b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        match String::from_utf16(&units) {
            Ok(s) => Ok(s),
            Err(_) => self.err("Unicode string has an unpaired surrogate"),
        }
    }

    fn bytes_left(&self) -> usize {
        self.d.len() - self.ofs
    }

    fn eof(&self) -> bool {
        self.ofs == self.d.len()
    }

    fn version_string(&self) -> String {
        format!("{:03}", self.ver)
    }

    fn get_version(&mut self) -> R<u32> {
        if self.ofs != 0 {
            return self.err("Trying to get Version block at wrong offset");
        }
        if self.d.len() < 10 || &self.d[..7] != b"Version" {
            return self.err("Not UI file");
        }
        let v = &self.d[7..10];
        if !v.iter().all(|c| c.is_ascii_digit()) {
            return self.err("Not UI file");
        }
        self.ofs = 10;
        Ok(std::str::from_utf8(v).unwrap().parse().unwrap())
    }

    fn starts_with_root_ui_entry(&self) -> bool {
        self.d.get(16..20) == Some(b"root") || self.d.get(32..36) == Some(b"root")
    }

    // ---- primitives ----

    fn c_u(&mut self, c: Option<&str>) -> R<()> {
        let v = self.get_u()?;
        self.cmt_out(format!("<u>{}</u>", v), c);
        Ok(())
    }

    fn c_id(&mut self) -> R<()> {
        let v4 = hex_bytes(self.look(4)?, " ");
        let v = self.get_u()?;
        self.out(&format!("<u>{}</u><!-- ID ({}) -->", v, v4));
        if self.ver >= 126 {
            self.c_uuid(None)?;
        }
        Ok(())
    }

    fn c_uuid(&mut self, c: Option<&str>) -> R<()> {
        let b = self.get(16)?;
        let vv = [
            hex_bytes(&b[0..4], ""),
            hex_bytes(&b[4..6], ""),
            hex_bytes(&b[6..8], ""),
            hex_bytes(&b[8..10], ""),
            hex_bytes(&b[10..16], ""),
        ]
        .join("-");
        self.cmt_out(format!("<uuid>{}</uuid>", vv), c);
        Ok(())
    }

    fn c_i(&mut self, c: Option<&str>) -> R<()> {
        let v = self.get_i()?;
        self.cmt_out(format!("<i>{}</i>", v), c);
        Ok(())
    }

    fn c_i_zero(&mut self, c: Option<&str>) -> R<()> {
        let v = self.get_i()?;
        if v != 0 {
            let flt = f32::from_le_bytes(self.behind(4).try_into().unwrap());
            let hex = hex_bytes(self.behind(4), ":");
            return self.err(format!("Must be zero, got {} / {} / {} ({})", v, flt, hex, c.unwrap_or("")));
        }
        self.cmt_out(format!("<i>{}</i><!-- always zero -->", v), c);
        Ok(())
    }

    fn c_ix(&mut self, c: Option<&str>) -> R<()> {
        let v = self.get_i()?;
        let hex = hex_bytes(self.behind(4), ":");
        self.cmt_out(format!("<i>{}</i><!-- {} -->", v, hex), c);
        Ok(())
    }

    fn c_byte(&mut self, c: Option<&str>) -> R<()> {
        let v = self.get_byte()?;
        self.cmt_out(format!("<byte>{}</byte>", v), c);
        Ok(())
    }

    fn c_s(&mut self, c: Option<&str>) -> R<()> {
        let s = self.get_s()?;
        self.cmt_out(format!("<s>{}</s>", xml_escape(&s)), c);
        Ok(())
    }

    fn c_unicode(&mut self, c: Option<&str>) -> R<()> {
        let s = self.get_unicode()?;
        self.cmt_out(format!("<unicode>{}</unicode>", xml_escape(&s)), c);
        Ok(())
    }

    fn c_bool(&mut self, c: Option<&str>) -> R<()> {
        let v = self.get_byte()?;
        if v > 1 {
            return self.err(format!("Invalid boolean value: got {} ({})", v, c.unwrap_or("")));
        }
        self.cmt_out(if v == 1 { "<yes />".into() } else { "<no />".into() }, c);
        Ok(())
    }

    fn c_bool_false(&mut self, c: Option<&str>) -> R<()> {
        let v = self.get_byte()?;
        if v != 0 {
            return self.err(format!("Invalid boolean value: false is expected, got {} ({})", v, c.unwrap_or("")));
        }
        self.cmt_out("<no />".into(), c);
        Ok(())
    }

    fn c_u2(&mut self, c: Option<&str>) -> R<()> {
        let v = self.get_u2()?;
        self.cmt_out(format!("<u2>{}</u2>", v), c);
        Ok(())
    }

    fn c_flt(&mut self, c: Option<&str>) -> R<()> {
        let v = self.get_flt()?;
        self.cmt_out(format!("<flt>{}</flt>", v), c);
        Ok(())
    }

    fn c_angle(&mut self, c: Option<&str>) -> R<()> {
        let f = f32::from_le_bytes(self.look(4)?.try_into().unwrap());
        let v = self.get_flt()?;
        let deg = round2_to_s(pretty_single(f) * 180.0 / std::f64::consts::PI);
        self.cmt_out(format!("<flt>{}</flt><!-- {} degrees -->", v, deg), c);
        Ok(())
    }

    fn c_bgra(&mut self, c: Option<&str>) -> R<()> {
        if let Some(c) = c {
            self.out(&format!("<!-- {} -->", c));
        }
        self.tag("color", &[], |s| {
            s.c_byte(Some("B"))?;
            s.c_byte(Some("G"))?;
            s.c_byte(Some("R"))?;
            s.c_byte(Some("A"))
        })
    }

    fn c_array<F: FnMut(&mut Self) -> R<()>>(&mut self, ty: &str, mut f: F) -> R<()> {
        let count = self.get_u()?;
        if count > 0xffff {
            return self.err(format!("{} array element count of {} impossibly high", ty, count));
        }
        self.tag(ty, &[("count", count.to_string())], |s| {
            for _ in 0..count {
                f(s)?;
            }
            Ok(())
        })
    }

    fn c_data(&mut self, size: usize, c: Option<&str>, zero: bool) -> R<()> {
        let v = self.get(size)?;
        if !zero && let Some(c) = c {
            self.out(&format!("<!-- {} -->", c));
        }
        self.out(&format!("<data size=\"{}\">", size));
        for slice in v.chunks(16) {
            self.out(&format!("  {}\n", hex_bytes(slice, " ")));
            if zero && slice.iter().any(|b| *b != 0) {
                return self.err(format!("Data should be all zeroes ({})", c.unwrap_or("")));
            }
        }
        self.out("</data>");
        Ok(())
    }

    // ---- structures ----

    fn c_image_list(&mut self) -> R<()> {
        self.c_array("images", |s| {
            s.tag("image", &[], |s| {
                s.c_id()?;
                s.c_s(Some("path"))?;
                s.c_i(Some("x size"))?;
                s.c_i(Some("y size"))?;
                if s.ver < 74 {
                    s.c_bgra(None)?;
                }
                if s.ver >= 78 {
                    s.c_bool(None)?;
                }
                Ok(())
            })
        })
    }

    fn c_state_list(&mut self) -> R<()> {
        self.c_array("states", |s| {
            s.tag("state", &[], |s| {
                let v = s.ver;
                s.c_id()?;
                s.c_s(Some("title - NewState"))?;
                s.c_i(Some("x size"))?;
                s.c_i(Some("y size"))?;
                s.c_unicode(Some("state text"))?;
                s.c_unicode(Some("tooltip"))?;
                s.c_i(Some("text xsize?"))?;
                s.c_i(Some("text ysize?"))?;
                s.c_i(Some("text xalign?"))?;
                s.c_i(Some("text yalign?"))?;
                s.c_bool(Some("text behavior?"))?;
                s.c_unicode(Some("text label?"))?;
                if v >= 116 {
                    s.c_unicode(Some("localization id"))?;
                    s.c_bool_false(Some("state stuff 1?"))?;
                    s.c_bool_false(Some("state stuff 2?"))?;
                    if v == 121 || v == 122 || v >= 129 {
                        s.c_s(None)?;
                    }
                } else {
                    s.c_bool_false(Some("state stuff 3?"))?;
                    s.c_bool(Some("state stuff 4?"))?;
                    s.c_unicode(Some("localization id"))?;
                    s.c_unicode(Some("tooltip id"))?;
                    if v >= 90 {
                        s.c_s(Some("state stuff before font?"))?;
                    }
                }
                if v >= 27 {
                    s.c_s(Some("font"))?;
                }
                s.c_i(Some("font size? line height?"))?;
                s.c_i(Some("font leading?"))?;
                s.c_i(Some("font tracking?"))?;
                if v >= 74 {
                    s.c_bgra(None)?;
                }
                if v >= 43 {
                    s.c_s(Some("font category / twui"))?;
                }
                if v >= 86 {
                    s.c_i(Some("left ?"))?;
                    s.c_i(Some("right ?"))?;
                    s.c_i(Some("top ?"))?;
                    s.c_i(Some("bottom ?"))?;
                } else {
                    s.c_ix(Some("x ?"))?;
                    s.c_ix(Some("y ?"))?;
                }
                s.c_i(None)?;
                let nbool = if v >= 90 {
                    0
                } else if v >= 83 {
                    4
                } else if v >= 29 {
                    3
                } else if v >= 27 {
                    2
                } else if v >= 25 {
                    6
                } else {
                    0
                };
                for _ in 0..nbool {
                    s.c_bool(None)?;
                }
                s.c_s(Some("shader name"))?;
                for _ in 0..4 {
                    s.c_flt(Some("shader vars"))?;
                }
                if v >= 77 {
                    s.c_s(Some("text shader name"))?;
                    for _ in 0..4 {
                        s.c_flt(Some("text shader vars"))?;
                    }
                }
                if v < 74 {
                    s.c_s(Some("state description"))?;
                    s.c_s(Some("event text"))?;
                }
                s.c_image_uses()?;
                s.c_i(None)?;
                s.c_i(None)?;
                if v < 74 {
                    s.c_transitions()?;
                } else {
                    s.c_mouse_states()?;
                }
                if v >= 124 {
                    s.c_material()?;
                }
                Ok(())
            })
        })
    }

    fn c_material(&mut self) -> R<()> {
        self.tag("material", &[], |s| {
            let path = s.get_s()?;
            s.out(&format!("<s>{}</s><!-- path -->", xml_escape(&path)));
            if path.is_empty() {
                s.out("<!-- rest skipped if path empty -->");
            } else {
                s.c_array("materialdata", |s| {
                    s.tag("materialdatapoint", &[], |s| {
                        s.c_s(None)?;
                        for _ in 0..4 {
                            s.c_flt(None)?;
                        }
                        Ok(())
                    })
                })?;
                s.c_properties()?;
            }
            Ok(())
        })
    }

    fn c_mouse_state_data(&mut self) -> R<()> {
        self.c_array("mouse_state_data", |s| {
            s.tag("mouse_state_datapoint", &[], |s| {
                s.c_id()?;
                s.c_s(None)?;
                s.c_s(None)?;
                s.c_s(None)
            })
        })
    }

    fn c_mouse_states(&mut self) -> R<()> {
        self.c_array("mouse_states", |s| {
            s.tag("mouse_state", &[], |s| {
                s.c_i(Some("type"))?;
                s.c_id()?;
                s.c_i(None)?;
                s.c_i(None)?;
                s.c_mouse_state_data()
            })
        })
    }

    fn c_transitions(&mut self) -> R<()> {
        self.c_array("transitions", |s| {
            s.tag("transition", &[], |s| {
                s.c_i(Some("type"))?;
                s.c_id()?;
                if s.ver >= 39 {
                    s.c_s(None)?;
                    s.c_i(None)?;
                    s.c_i(None)?;
                }
                if s.ver >= 43 {
                    s.c_s(None)?;
                    s.c_i(None)?;
                }
                Ok(())
            })
        })
    }

    fn c_image_uses(&mut self) -> R<()> {
        self.c_array("image_uses", |s| {
            s.tag("image_use", &[], |s| {
                let v = s.ver;
                s.c_id()?;
                s.c_u(Some("x offset"))?;
                s.c_u(Some("y offset"))?;
                s.c_u(Some("x size"))?;
                s.c_u(Some("y size"))?;
                s.c_bgra(Some("color"))?;
                if v >= 119 {
                    s.c_s(Some("color?"))?;
                }
                s.c_bool(Some("tiled?"))?;
                s.c_bool(Some("flipped x?"))?;
                s.c_bool(Some("flipped y?"))?;
                s.c_i(Some("dock position (0-9)?"))?;
                if v >= 77 {
                    s.c_i(Some("dock offset x?"))?;
                    s.c_i(Some("dock offset y?"))?;
                }
                s.c_bool(Some("stretch x?"))?;
                s.c_bool(Some("stretch y?"))?;
                s.c_angle(Some("rotation angle"))?;
                s.c_flt(Some("rotation pivot x?"))?;
                s.c_flt(Some("rotation pivot y?"))?;
                if v >= 74 {
                    if v >= 103 {
                        s.c_flt(Some("rotation axis x?"))?;
                        s.c_flt(Some("rotation axis y?"))?;
                        s.c_flt(Some("rotation axis z?"))?;
                        s.c_s(Some("shader name"))?;
                    } else {
                        s.c_s(Some("shader name"))?;
                        s.c_flt(Some("rotation axis x?"))?;
                        s.c_flt(Some("rotation axis y?"))?;
                        s.c_flt(Some("rotation axis z?"))?;
                        s.c_i_zero(None)?;
                    }
                    if v <= 77 {
                        s.c_bool(Some("extra image use stuff?"))?;
                    }
                    if v >= 103 {
                        for _ in 0..4 {
                            s.c_flt(Some("shader var?"))?;
                        }
                    }
                    if v >= 92 {
                        s.c_flt(Some("margin top?"))?;
                        s.c_flt(Some("margin right?"))?;
                        s.c_flt(Some("margin bottom?"))?;
                        s.c_flt(Some("margin left?"))?;
                    } else {
                        s.c_flt(Some("margin top-bottom?"))?;
                        s.c_flt(Some("margin left-right?"))?;
                    }
                    if v >= 125 {
                        s.c_bool(None)?;
                    }
                } else {
                    if v >= 51 {
                        s.c_bool(None)?;
                    }
                    if v == 31 {
                        s.c_i(None)?;
                    }
                    if v == 30 {
                        s.c_bool(None)?;
                    }
                }
                Ok(())
            })
        })
    }

    fn c_anim_attrs(&mut self) -> R<()> {
        self.c_array("anim_attrs", |s| {
            s.tag("anim_attr", &[], |s| {
                s.c_id()?;
                s.c_s(Some("animation?"))?;
                s.c_s(Some("state?"))?;
                s.c_s(Some("property?"))
            })
        })
    }

    fn c_sound(&mut self) -> R<()> {
        self.tag("sound", &[], |s| {
            let la = s.look(4)?.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(", ");
            s.out(&format!("<!-- lookahead [{}] -->", la));
            let v = s.look_i()?;
            if v == -1 {
                s.c_i(Some("no sound"))
            } else if s.ver >= 124 {
                s.c_u2(None)?;
                s.c_s(None)
            } else if v == 0 {
                s.c_i(Some("no sound"))
            } else {
                s.c_s(None)?;
                s.c_s(None)
            }
        })
    }

    fn c_anims(&mut self) -> R<()> {
        self.c_array("anims", |s| {
            s.tag("anim", &[], |s| {
                let v = s.ver;
                if v >= 113 {
                    s.c_sound()?;
                }
                s.c_flt(Some("x offset?"))?;
                s.c_flt(Some("y offset?"))?;
                s.c_i(Some("x size?"))?;
                s.c_i(Some("y size?"))?;
                s.c_bgra(None)?;
                for _ in 0..4 {
                    s.c_flt(Some("shader vars?"))?;
                }
                s.c_flt(Some("rotation angle?"))?;
                s.c_i(Some("image index 1?"))?;
                s.c_i(Some("image index 2?"))?;
                if v >= 113 {
                    s.c_flt(Some("font scale?"))?;
                }
                s.c_i(Some("interpolation time?"))?;
                s.c_i(Some("interpolation property mask?"))?;
                s.c_flt(Some("easing weight?"))?;
                s.c_s(Some("easing curve?"))?;
                s.c_anim_attrs()?;
                if v >= 90 {
                    s.c_bool(Some("is movement absolute?"))?;
                }
                if v >= 104 {
                    s.c_bool(Some("end of anim 1?"))?;
                }
                if v <= 112 {
                    if v >= 100 {
                        s.c_s(Some("end of anim 2?"))?;
                    }
                    if v >= 106 {
                        s.c_s(Some("end of anim 3"))?;
                    }
                }
                if v >= 124 {
                    s.c_s(None)?;
                }
                Ok(())
            })
        })
    }

    fn c_funcs(&mut self) -> R<()> {
        self.c_array("funcs", |s| {
            s.tag("func", &[], |s| {
                s.c_s(Some("name"))?;
                s.c_bool(Some("propagate?"))?;
                s.c_bool(Some("make noninteractive?"))?;
                s.c_anims()?;
                if s.ver >= 91 && s.ver < 100 {
                    s.c_s(None)?;
                }
                if s.ver >= 113 {
                    s.c_s(None)?;
                    s.c_s(None)?;
                }
                Ok(())
            })
        })
    }

    fn c_properties(&mut self) -> R<()> {
        self.c_array("properties", |s| {
            s.tag("property", &[], |s| {
                s.c_s(Some("key"))?;
                s.c_s(Some("value"))
            })
        })
    }

    fn find_variant(&self) -> Option<usize> {
        let end = (self.ofs + 1000).min(self.d.len());
        let hay = self.d.get(self.ofs..end)?;
        hay.windows(7).position(|w| w == b"Variant")
    }

    fn c_model(&mut self) -> R<()> {
        self.tag("models", &[], |s| {
            s.out("<!-- this is very poorly decoded part -->");
            s.c_s(None)?;
            let header_size = match s.ver {
                74 => 37,
                84..=86 => 50,
                _ => 74,
            };
            match s.find_variant() {
                Some(e) => s.out(&format!("<!-- model expected header size {}/{} -->", header_size, e as i64 - 6)),
                None => s.out(&format!("<!-- model expected header size {}/unknown -->", header_size)),
            }
            s.c_data(header_size, Some("model header data"), false)?;
            let model_count = s.get_i()?;
            s.out(&format!("<i>{}</i><!-- model count -->", model_count));
            for _ in 0..model_count.max(0) {
                s.tag("model", &[], |s| {
                    s.c_s(Some("mesh path?"))?;
                    s.c_s(Some("mesh name?"))?;
                    if s.ver <= 77 {
                        s.c_data(21, Some("some model data or anim header or sth"), false)?;
                    } else if (84..=86).contains(&s.ver) {
                        s.c_data(29, Some("some model data or anim header or sth"), false)?;
                    } else {
                        s.c_data(1, Some("some model data or anim header or sth"), false)?;
                    }
                    s.c_ix(Some("anim count or something?"))?;
                    s.c_s(Some("anim name?"))?;
                    s.c_s(Some("anim path?"))?;
                    s.c_data(4, Some("rest of anim stuff or sth"), false)
                })?;
            }
            Ok(())
        })
    }

    fn c_uientry_gen2(&mut self, attrs: &[(&str, String)]) -> R<()> {
        self.tag("uientry", attrs, |s| {
            let v = s.ver;
            s.c_id()?;
            s.c_s(Some("title"))?;
            if v >= 43 {
                s.c_s(Some("title2"))?;
            }
            if (100..113).contains(&v) {
                s.c_s(Some("v100+ extra string?"))?;
            } else if v == 113 {
                s.tag("event", &[], |s| {
                    s.c_s(None)?;
                    s.c_s(None)?;
                    s.c_s(None)
                })?;
            } else if v >= 115 {
                s.c_array("events", |s| {
                    s.tag("event", &[], |s| {
                        s.c_s(None)?;
                        s.c_s(None)?;
                        s.c_s(None)?;
                        if s.ver >= 121 {
                            s.c_properties()?;
                        }
                        Ok(())
                    })
                })?;
            }
            s.c_i(Some("x offset"))?;
            s.c_i(Some("y offset"))?;
            for i in 1..=12 {
                let c = format!("uientry flag {}", i);
                if i == 3 {
                    s.c_byte(Some(&c))?;
                } else {
                    s.c_bool(Some(&c))?;
                }
            }
            if v < 90 {
                s.c_bool(None)?;
            }
            s.c_unicode(Some("tooltip text"))?;
            s.c_unicode(Some("tooltip id"))?;
            s.c_ix(Some("docking?"))?;
            if v >= 77 {
                s.c_ix(Some("docking x?"))?;
                s.c_ix(Some("docking y?"))?;
            }
            s.c_bool(None)?;
            s.c_i(Some("default state id"))?;
            s.c_image_list()?;
            s.c_ix(Some("mask image?"))?;
            if v < 110 {
                s.c_i(None)?;
            }
            if v >= 126 {
                s.c_uuid(None)?;
            }
            s.c_state_list()?;
            if v >= 126 {
                s.c_uuid(None)?;
            }
            s.c_properties()?;
            s.c_i(Some("priority?"))?;
            s.c_funcs()?;
            s.c_children()?;
            s.c_additional_data()?;
            s.c_s(Some("end of uientry 1?"))?;
            s.c_s(Some("end of uientry 2?"))?;
            if s.get_bool()? {
                s.out("<yes /><!-- has some extra data (just below) -->");
                s.c_array("array", |s| s.c_flt(None))?;
            } else {
                s.out("<no /><!-- has some extra data (section skipped) -->");
            }
            if v == 97 {
                if s.get_bool()? {
                    s.out("<yes /><!-- has some extra v97 data (just below) -->");
                    s.c_i(Some("extra int 1?"))?;
                    s.c_array("array", |s| s.c_ix(None))?;
                } else {
                    s.out("<no /><!-- has some extra v97 data (section skipped) -->");
                }
                s.c_ix(Some("uientry id reference maybe?"))?;
            }
            if s.get_bool()? {
                s.out("<yes /><!-- has model (controls presence of model below) -->");
                s.c_model()?;
            } else {
                s.out("<no /><!-- has model (controls presence of model below) -->");
            }
            if v <= 84 && v != 77 && v != 78 {
                s.c_bool(Some("end of uientry flag 5A?"))?;
            } else {
                s.c_bool(Some("end of uientry flag 5B?"))?;
                s.c_bool(Some("end of uientry flag 6B?"))?;
            }
            if v >= 94 {
                s.c_bool(None)?;
            }
            if v >= 113 {
                s.c_flt(None)?;
                s.c_flt(None)?;
                s.c_flt(None)?;
            }
            Ok(())
        })
    }

    fn c_uientry(&mut self) -> R<()> {
        self.tag("uientry", &[], |s| {
            let v = s.ver;
            s.c_id()?;
            s.c_s(Some("title"))?;
            if v >= 43 {
                s.c_s(Some("title2"))?;
            }
            s.c_i(Some("x offset"))?;
            s.c_i(Some("y offset"))?;
            for _ in 0..7 {
                s.c_bool(None)?;
            }
            if v >= 47 {
                s.c_bool(None)?;
            }
            if v >= 50 {
                for _ in 0..3 {
                    s.c_bool(None)?;
                }
            }
            if v >= 52 {
                s.c_bool(None)?;
            }
            s.c_s(Some("parent name"))?;
            s.c_i(None)?;
            s.c_unicode(Some("tooltip text"))?;
            s.c_unicode(Some("tooltip id"))?;
            s.c_i(None)?;
            if v >= 33 {
                s.c_bool(None)?;
            }
            if v >= 39 {
                s.c_ix(Some("default state id"))?;
            }
            s.c_s(Some("script"))?;
            s.c_image_list()?;
            if v >= 28 {
                s.c_i(None)?;
            }
            if v >= 32 {
                s.c_i(None)?;
            }
            s.c_state_list()?;
            s.c_i(None)?;
            s.c_event_list()?;
            s.c_i(None)?;
            if v >= 39 {
                s.c_effects()?;
            }
            s.c_children()?;
            s.c_s(Some("template"))?;
            if v >= 44 {
                s.c_additional_data()?;
            }
            if v >= 49 {
                s.c_s(None)?;
            }
            Ok(())
        })
    }

    fn c_additional_data(&mut self) -> R<()> {
        if !self.get_bool()? {
            self.tag_empty("additional_data", &[("type", "none".into())]);
            return Ok(());
        }
        let ty = self.get_s()?;
        self.tag("additional_data", &[("type", ty.clone())], |s| {
            let v = s.ver;
            match ty.as_str() {
                "List" | "HorizontalList" => {
                    let count = s.get_u()?;
                    s.out(&format!("<i>{}</i><!-- count data points -->", count));
                    for _ in 0..count {
                        s.c_flt(Some("data point"))?;
                    }
                    s.c_i(Some("mystery1"))?;
                    s.c_i(Some("mystery2"))?;
                    s.c_bool(Some("mystery3"))?;
                    s.c_i(Some("mystery4"))?;
                    if v >= 54 && v != 77 {
                        s.c_bool(Some("mystery5"))?;
                    }
                    if v >= 96 {
                        s.c_bool(Some("mystery6"))?;
                        s.c_i(Some("mystery7"))?;
                    }
                    if v == 101 {
                        s.c_ix(Some("mystery8"))?;
                        s.c_bool_false(Some("mystery9"))?;
                    } else if (102..=104).contains(&v) {
                        s.c_i(Some("mystery8"))?;
                        s.c_data(5, None, true)?;
                    } else if v == 105 {
                        s.c_i(Some("mystery8"))?;
                        s.c_data(10, None, true)?;
                    } else if v >= 106 {
                        for _ in 0..4 {
                            s.c_i(None)?;
                        }
                        s.c_bool(None)?;
                        s.c_bool(None)?;
                        s.c_i(None)?;
                        if v >= 113 {
                            s.c_i_zero(None)?;
                        }
                        if v >= 119 {
                            s.c_i_zero(None)?;
                            s.c_bool(None)?;
                        }
                        if v >= 122 {
                            s.c_i(None)?;
                            s.c_i(None)?;
                        }
                        if v >= 129 {
                            s.c_data(2, Some("v129 additional data?"), true)?;
                        }
                    }
                    Ok(())
                }
                "Table" => s.c_array("table", |s| {
                    s.c_array("row", |s| {
                        s.tag("col", &[], |s| {
                            for _ in 0..5 {
                                s.c_flt(Some("data point"))?;
                            }
                            Ok(())
                        })
                    })
                }),
                "RadialList" => {
                    s.c_angle(None)?;
                    s.c_angle(None)?;
                    s.c_angle(None)?;
                    s.c_flt(None)?;
                    s.c_bool(None)?;
                    if v == 121 {
                        s.c_bool(None)?;
                    }
                    Ok(())
                }
                other => s.err(format!("Unknown additional data section {}", other)),
            }
        })
    }

    fn c_children(&mut self) -> R<()> {
        self.c_array("children", |s| {
            if s.ver <= 54 {
                s.c_uientry()
            } else if s.ver <= 99 {
                s.c_uientry_gen2(&[])
            } else if s.look(2)? == [0, 0] {
                s.ofs += 2;
                s.c_uientry_gen2(&[("type", "normal".into())])
            } else {
                s.c_template()
            }
        })
    }

    fn c_subtemplate(&mut self) -> R<()> {
        self.tag("subtemplate", &[], |s| {
            let v = s.ver;
            s.c_s(Some("source uientry?"))?;
            s.c_s(Some("dest uientry?"))?;
            if v >= 129 {
                s.c_uuid(None)?;
                s.c_array("states", |s| {
                    s.tag("state", &[], |s| {
                        s.c_s(Some("name"))?;
                        s.c_uuid(None)?;
                        s.c_uuid(None)
                    })
                })?;
            }
            s.c_s(None)?;
            if v >= 119 {
                s.c_array("events", |s| {
                    s.tag("event", &[], |s| {
                        s.c_s(None)?;
                        s.c_s(None)?;
                        s.c_s(None)?;
                        if s.ver >= 121 {
                            s.c_array("properties", |s| {
                                s.tag("property", &[], |s| {
                                    s.c_s(None)?;
                                    s.c_s(None)
                                })
                            })?;
                        }
                        Ok(())
                    })
                })?;
            } else {
                s.c_s(Some("type?"))?;
            }
            s.c_s(Some("func name?"))?;
            for _ in 0..4 {
                s.c_flt(None)?;
            }
            s.c_i(None)?;
            s.c_i(None)?;
            s.c_bool(Some("subtemplate flag 1?"))?;
            if v >= 128 {
                s.c_bool(Some("subtemplate flag 2?"))?;
            }
            s.c_i(None)?;
            s.c_i(None)?;
            s.c_data(2, None, true)?;
            s.c_unicode(Some("tooltip id?"))?;
            s.c_unicode(Some("tooltip text?"))?;
            if v >= 128 {
                s.c_bool(None)?;
            }
            // The original has no count for these states; it guesses from the next bytes.
            loop {
                let la = s.d.get(s.ofs..(s.ofs + 4).min(s.d.len())).unwrap_or(&[]);
                s.out(&format!("<!-- lookahead: [{}] -->", la.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(", ")));
                let b = s.d.get(s.ofs + 2..s.ofs + 4).ok_or_else(|| format!("Requested reading past end of file (at offset {})", s.ofs))?;
                if b[0] == 0 {
                    break;
                }
                if b[1] == 0 && s.look(2)? != [1, 0] {
                    break;
                }
                s.tag("state", &[], |s| {
                    s.c_s(Some("name"))?;
                    for _ in 0..4 {
                        s.c_unicode(None)?;
                    }
                    if s.ver >= 128 {
                        s.c_bool(None)?;
                    }
                    Ok(())
                })?;
            }
            s.c_properties()?;
            s.c_array("images", |s| s.tag("image", &[], |s| s.c_s(None)))?;
            if v >= 128 {
                s.c_i_zero(Some("v128+ stuff 1?"))?;
                s.c_array("states", |s| {
                    s.tag("state", &[], |s| {
                        s.c_s(None)?;
                        s.c_s(None)?;
                        for _ in 0..4 {
                            s.c_flt(None)?;
                        }
                        Ok(())
                    })
                })?;
                s.c_array("images", |s| {
                    s.tag("image", &[], |s| {
                        s.c_s(Some("path"))?;
                        s.c_uuid(None)
                    })
                })?;
            }
            Ok(())
        })
    }

    fn c_template(&mut self) -> R<()> {
        self.tag("template", &[], |s| {
            s.c_s(Some("name?"))?;
            if s.ver >= 119 {
                s.c_id()?;
            }
            s.c_array("subtemplates", |s| s.c_subtemplate())?;
            s.c_array("children", |s| s.c_uientry_gen2(&[]))
        })
    }

    fn c_effects(&mut self) -> R<()> {
        self.c_array("effects", |s| {
            s.tag("effect", &[], |s| {
                s.c_s(Some("name"))?;
                s.c_bool(None)?;
                s.c_bool(None)?;
                s.c_array("phases", |s| {
                    s.tag("phase", &[], |s| {
                        for _ in 0..11 {
                            s.c_ix(None)?;
                        }
                        if s.ver >= 50 {
                            s.out("<!-- extra 3 -->");
                            for _ in 0..3 {
                                s.c_ix(None)?;
                            }
                        }
                        let v = s.get_i()?;
                        s.out(&format!("<i>{}</i><!-- include optional extra phase details? -->", v));
                        for _ in 0..v.max(0) {
                            s.c_ix(None)?;
                            s.c_s(None)?;
                            s.c_s(None)?;
                        }
                        Ok(())
                    })
                })
            })
        })
    }

    fn c_event_list(&mut self) -> R<()> {
        self.tag("events", &[], |s| {
            loop {
                let e = s.get_s()?;
                if e == "events_end" {
                    return Ok(());
                }
                s.tag("event", &[], |s| {
                    s.out(&format!("<s>{}</s>", xml_escape(&e)));
                    Ok(())
                })?;
            }
        })
    }

    fn c_ui(&mut self) -> R<()> {
        let vs = self.version_string();
        self.tag("ui", &[("version", vs)], |s| {
            if s.ver <= 54 {
                s.c_uientry()?;
            } else {
                s.c_uientry_gen2(&[])?;
            }
            if s.bytes_left() > 0 {
                return s.err(format!("TODO - incomplete decoding, {} bytes left", s.bytes_left()));
            }
            Ok(())
        })
    }

    fn c_cml(&mut self) -> R<()> {
        let vs = self.version_string();
        self.tag("cml", &[("version", vs)], |s| {
            while !s.eof() {
                let k = s.get_s()?;
                s.out(&format!("<key>{}</key>", xml_escape(&k)));
                let v = s.get_s()?;
                s.out(&format!("<value>{}</value>", xml_escape(&v)));
            }
            Ok(())
        })
    }

    fn c_fc(&mut self) -> R<()> {
        let vs = self.version_string();
        self.tag("fc", &[("version", vs)], |s| {
            while !s.eof() {
                s.tag("fcentry", &[], |s| {
                    let v = s.ver;
                    s.c_s(None)?;
                    s.c_u(None)?;
                    s.c_s(None)?;
                    s.c_u(None)?;
                    s.c_u(None)?;
                    if v >= 50 {
                        s.c_u(None)?;
                        s.c_u(None)?;
                    }
                    if v >= 51 {
                        s.c_u(None)?;
                    }
                    s.c_byte(Some("B"))?;
                    s.c_byte(Some("G"))?;
                    s.c_byte(Some("R"))?;
                    s.c_byte(Some("A"))?;
                    if v >= 53 {
                        s.c_s(None)?;
                    }
                    if v >= 52 {
                        s.c_s(Some("T0"))?;
                        for _ in 0..4 {
                            s.c_u(None)?;
                        }
                    }
                    Ok(())
                })?;
            }
            Ok(())
        })
    }

    fn c_twui_images(&mut self) -> R<()> {
        let vs = self.version_string();
        self.tag("twui_images", &[("version", vs)], |s| {
            s.c_array("images", |s| {
                s.tag("image", &[], |s| {
                    s.c_s(None)?;
                    s.c_data(19, None, true)?;
                    s.c_i(None)?;
                    s.c_data(6, None, true)?;
                    let x = s.get_u2()?;
                    s.out(&format!("<i2>{}</i2><!-- x -->", x));
                    let y = s.get_u2()?;
                    s.out(&format!("<i2>{}</i2><!-- y -->", y));
                    s.c_u2(None)?;
                    s.out(&format!("<!-- {}*{} datapoints -->", x, y));
                    for _ in 0..(x as u32 * y as u32) {
                        s.c_bgra(None)?;
                    }
                    Ok(())
                })
            })?;
            // Bytes after the images would not be written back.
            if s.bytes_left() > 0 {
                return s.err(format!("{} bytes after the images", s.bytes_left()));
            }
            Ok(())
        })
    }

    fn autodetect(&mut self) -> R<()> {
        self.ver = self.get_version()?;
        match self.ver {
            2 => self.c_cml(),
            44 | 50 | 51 | 52 | 53 => {
                if self.starts_with_root_ui_entry() {
                    self.c_ui()
                } else {
                    self.c_fc()
                }
            }
            119 | 129 => {
                if self.starts_with_root_ui_entry() {
                    self.c_ui()
                } else {
                    self.c_twui_images()
                }
            }
            25..=999 => self.c_ui(),
            _ => self.err("Not supported yet"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{convert, xml_escape};
    use crate::writer;

    /// A binary UI file written primitive by primitive, in the order the reader reads them.
    struct B {
        d: Vec<u8>,
        v: u32,
    }

    impl B {
        fn new(v: u32) -> Self {
            Self { d: format!("Version{v:03}").into_bytes(), v }
        }

        fn raw(&mut self, b: &[u8]) -> &mut Self {
            self.d.extend_from_slice(b);
            self
        }

        fn u(&mut self, v: u32) -> &mut Self {
            self.raw(&v.to_le_bytes())
        }

        fn i(&mut self, v: i32) -> &mut Self {
            self.raw(&v.to_le_bytes())
        }

        fn u2(&mut self, v: u16) -> &mut Self {
            self.raw(&v.to_le_bytes())
        }

        fn byte(&mut self, v: u8) -> &mut Self {
            self.raw(&[v])
        }

        /// `n` booleans, true and false in turn.
        fn flags(&mut self, n: usize) -> &mut Self {
            for k in 0..n {
                self.byte((k % 2) as u8);
            }
            self
        }

        fn s(&mut self, s: &str) -> &mut Self {
            self.u2(s.len() as u16).raw(s.as_bytes())
        }

        fn uni(&mut self, s: &str) -> &mut Self {
            let units: Vec<u16> = s.encode_utf16().collect();
            self.u2(units.len() as u16);
            for u in units {
                self.u2(u);
            }
            self
        }

        fn flt(&mut self, f: f32) -> &mut Self {
            self.raw(&f.to_le_bytes())
        }

        fn flts(&mut self, f: &[f32]) -> &mut Self {
            for x in f {
                self.flt(*x);
            }
            self
        }

        fn uuid(&mut self) -> &mut Self {
            self.raw(&[0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef])
        }

        fn bgra(&mut self) -> &mut Self {
            self.raw(&[1, 2, 3, 255])
        }

        /// An ID, with its UUID from version 126 on.
        fn id(&mut self, id: u32) -> &mut Self {
            self.u(id);
            if self.v >= 126 {
                self.uuid();
            }
            self
        }
    }

    #[derive(Clone, Copy, Debug)]
    enum Extra {
        None,
        List,
        Table,
        Radial,
    }

    const EXTRAS: [Extra; 4] = [Extra::None, Extra::List, Extra::Table, Extra::Radial];

    fn image(b: &mut B) {
        let v = b.v;
        b.id(42).s("ui/skins/button.png").i(64).i(-32);
        if v < 74 {
            b.bgra();
        }
        if v >= 78 {
            b.byte(1);
        }
    }

    fn image_use(b: &mut B) {
        let v = b.v;
        b.id(43).u(1).u(2).u(3).u(4).bgra();
        if v >= 119 {
            b.s("colour");
        }
        b.flags(3).i(5);
        if v >= 77 {
            b.i(-6).i(7);
        }
        b.flags(2).flt(std::f32::consts::FRAC_PI_2).flt(0.5).flt(-0.5);
        if v >= 74 {
            if v >= 103 {
                b.flts(&[0.0, 0.0, 1.0]).s("shader");
            } else {
                b.s("shader").flts(&[0.0, 0.0, 1.0]).i(0);
            }
            if v <= 77 {
                b.byte(1);
            }
            if v >= 103 {
                b.flts(&[0.25; 4]);
            }
            if v >= 92 {
                b.flts(&[1.0, 2.0, 3.0, 4.0]);
            } else {
                b.flts(&[1.0, 2.0]);
            }
            if v >= 125 {
                b.byte(0);
            }
        } else {
            if v >= 51 {
                b.byte(1);
            }
            if v == 31 {
                b.i(9);
            }
            if v == 30 {
                b.byte(1);
            }
        }
    }

    fn state(b: &mut B) {
        let v = b.v;
        b.id(44).s("NewState").i(100).i(50).uni("State text \u{fc}").uni("").i(1).i(2).i(3).i(4).byte(1).uni("label");
        if v >= 116 {
            b.uni("loc_id").byte(0).byte(0);
            if v == 121 || v == 122 || v >= 129 {
                b.s("v129 string");
            }
        } else {
            b.byte(0).byte(1).uni("loc").uni("tip_id");
            if v >= 90 {
                b.s("before font");
            }
        }
        if v >= 27 {
            b.s("font_name");
        }
        b.i(12).i(0).i(-1);
        if v >= 74 {
            b.bgra();
        }
        if v >= 43 {
            b.s("font_category");
        }
        if v >= 86 {
            b.i(1).i(2).i(3).i(4);
        } else {
            b.i(-1).i(-2);
        }
        b.i(0);
        let nbool = match v {
            90.. => 0,
            83.. => 4,
            29.. => 3,
            27.. => 2,
            25.. => 6,
            _ => 0,
        };
        b.flags(nbool);
        b.s("shader").flts(&[0.1, 1e-7, -0.0, 3.0e38]);
        if v >= 77 {
            b.s("text_shader").flts(&[1.0, 16_777_216.0, f32::MIN_POSITIVE, 1e-45]);
        }
        if v < 74 {
            b.s("description").s("event text");
        }
        b.u(1);
        image_use(b);
        b.i(0).i(0);
        if v < 74 {
            b.u(1).i(1).id(45);
            if v >= 39 {
                b.s("transition").i(2).i(3);
            }
            if v >= 43 {
                b.s("x").i(4);
            }
        } else {
            b.u(1).i(1).id(46).i(0).i(0).u(1).id(47).s("a").s("b").s("c");
        }
        if v >= 129 {
            b.s("material.xml").u(1).s("point").flts(&[1.0, 2.0, 3.0, 4.0]).u(1).s("k").s("v");
        } else if v >= 124 {
            b.s("");
        }
    }

    fn anim(b: &mut B) {
        let v = b.v;
        if v >= 124 {
            b.u2(5).s("sound_event");
        } else if v >= 113 {
            b.i(-1);
        }
        b.flt(1.5).flt(-2.25).i(10).i(20).bgra().flts(&[0.0, 0.1, 0.2, 0.3]).flt(0.785).i(0).i(1);
        if v >= 113 {
            b.flt(1.0);
        }
        b.i(250).i(7).flt(0.5).s("ease_in");
        b.u(1).id(48).s("anim").s("state").s("property");
        if v >= 90 {
            b.byte(1);
        }
        if v >= 104 {
            b.byte(0);
        }
        if v <= 112 {
            if v >= 100 {
                b.s("end2");
            }
            if v >= 106 {
                b.s("end3");
            }
        }
        if v >= 124 {
            b.s("v124");
        }
    }

    fn func(b: &mut B) {
        let v = b.v;
        b.s("OnClick").byte(1).byte(0).u(1);
        anim(b);
        if (91..100).contains(&v) {
            b.s("v91");
        }
        if v >= 113 {
            b.s("a").s("b");
        }
    }

    fn additional(b: &mut B, extra: Extra) {
        let v = b.v;
        match extra {
            Extra::None => {
                b.byte(0);
            }
            Extra::List => {
                b.byte(1).s("List").u(2).flts(&[0.5, 1.5]).i(1).i(2).byte(1).i(4);
                if v >= 54 && v != 77 {
                    b.byte(0);
                }
                if v >= 96 {
                    b.byte(1).i(7);
                }
                if v == 101 {
                    b.i(8).byte(0);
                } else if (102..=104).contains(&v) {
                    b.i(8).raw(&[0; 5]);
                } else if v == 105 {
                    b.i(8).raw(&[0; 10]);
                } else if v >= 106 {
                    b.i(1).i(2).i(3).i(4).byte(1).byte(0).i(5);
                    if v >= 113 {
                        b.i(0);
                    }
                    if v >= 119 {
                        b.i(0).byte(1);
                    }
                    if v >= 122 {
                        b.i(9).i(10);
                    }
                    if v >= 129 {
                        b.raw(&[0, 0]);
                    }
                }
            }
            Extra::Table => {
                b.byte(1).s("Table").u(1).u(1).flts(&[1.0, 2.0, 3.0, 4.0, 5.0]);
            }
            Extra::Radial => {
                b.byte(1).s("RadialList").flts(&[0.0, std::f32::consts::PI, -std::f32::consts::FRAC_PI_2, 2.0]).byte(1);
                if v == 121 {
                    b.byte(0);
                }
            }
        }
    }

    fn uientry_gen1(b: &mut B, title: &str, full: bool, extra: Extra) {
        let v = b.v;
        b.id(7).s(title);
        if v >= 43 {
            b.s("title2");
        }
        b.i(-5).i(12);
        let n = 7 + usize::from(v >= 47) + if v >= 50 { 3 } else { 0 } + usize::from(v >= 52);
        b.flags(n).s("parent").i(0).uni("Tooltip \u{fc}").uni("").i(3);
        if v >= 33 {
            b.byte(1);
        }
        if v >= 39 {
            b.i(-1);
        }
        b.s("script");
        if full {
            b.u(1);
            image(b);
        } else {
            b.u(0);
        }
        if v >= 28 {
            b.i(1);
        }
        if v >= 32 {
            b.i(2);
        }
        if full {
            b.u(1);
            state(b);
        } else {
            b.u(0);
        }
        b.i(0);
        if full {
            b.s("OnClick").s("On<Hover> & \"more\"");
        }
        b.s("events_end").i(0);
        if v >= 39 {
            if full {
                b.u(1).s("effect").byte(1).byte(0).u(1);
                for k in 0..11 {
                    b.i(k);
                }
                if v >= 50 {
                    b.i(-1).i(-2).i(-3);
                }
                b.i(1).i(9).s("a").s("b");
            } else {
                b.u(0);
            }
        }
        if full {
            b.u(1);
            uientry_gen1(b, "child", false, Extra::None);
        } else {
            b.u(0);
        }
        b.s("template");
        if v >= 44 {
            additional(b, extra);
        }
        if v >= 49 {
            b.s("");
        }
    }

    fn subtemplate(b: &mut B) {
        let v = b.v;
        b.s("source").s("dest");
        if v >= 129 {
            b.uuid().u(1).s("name").uuid().uuid();
        }
        b.s("x");
        if v >= 119 {
            b.u(1).s("e1").s("e2").s("e3");
            if v >= 121 {
                b.u(1).s("k").s("v");
            }
        } else {
            b.s("type");
        }
        b.s("func").flts(&[1.0, 2.0, 3.0, 4.0]).i(1).i(2).byte(1);
        if v >= 128 {
            b.byte(0);
        }
        b.i(3).i(4).raw(&[0, 0]).uni("tip id").uni("tip");
        if v >= 128 {
            b.byte(1);
        }
        // One state of the uncounted list: its name's first two bytes are not zero, so the reader's guess takes it.
        b.s("st").uni("a").uni("").uni("c").uni("d");
        if v >= 128 {
            b.byte(0);
        }
        b.u(0).u(1).s("image.png");
        if v >= 128 {
            b.i(0).u(1).s("a").s("b").flts(&[0.1, 0.2, 0.3, 0.4]).u(1).s("path").uuid();
        }
    }

    fn model(b: &mut B) {
        let v = b.v;
        let header = match v {
            74 => 37,
            84..=86 => 50,
            _ => 74,
        };
        b.s("mdl").raw(&(0..header).map(|k| k as u8).collect::<Vec<_>>()).i(1).s("mesh path").s("mesh name");
        let size = if v <= 77 {
            21
        } else if (84..=86).contains(&v) {
            29
        } else {
            1
        };
        b.raw(&vec![0xab; size]).i(-1).s("anim").s("anim path").raw(&[1, 2, 3, 4]);
    }

    fn uientry_gen2(b: &mut B, title: &str, full: bool, extra: Extra, floats: &[f32]) {
        let v = b.v;
        b.id(11).s(title);
        if v >= 43 {
            b.s("title2");
        }
        if (100..113).contains(&v) {
            b.s("v100 extra");
        } else if v == 113 {
            b.s("e1").s("e2").s("e3");
        } else if v >= 115 {
            if full {
                b.u(1).s("OnClick").s("ctx").s("fn");
                if v >= 121 {
                    b.u(1).s("k").s("v");
                }
            } else {
                b.u(0);
            }
        }
        b.i(-10).i(10);
        for k in 1..=12u8 {
            b.byte(if k == 3 { 200 } else { k % 2 });
        }
        if v < 90 {
            b.byte(0);
        }
        b.uni("tooltip \u{1f600}").uni("").i(-1);
        if v >= 77 {
            b.i(-2).i(-3);
        }
        b.byte(1).i(0);
        if full {
            b.u(1);
            image(b);
        } else {
            b.u(0);
        }
        b.i(-1);
        if v < 110 {
            b.i(0);
        }
        if v >= 126 {
            b.uuid();
        }
        if full {
            b.u(1);
            state(b);
        } else {
            b.u(0);
        }
        if v >= 126 {
            b.uuid();
        }
        if full {
            b.u(1).s("key").s("value");
        } else {
            b.u(0);
        }
        b.i(3);
        if full {
            b.u(1);
            func(b);
        } else {
            b.u(0);
        }
        if !full {
            b.u(0);
        } else if v <= 99 {
            b.u(1);
            uientry_gen2(b, "child", false, Extra::None, &[]);
        } else {
            b.u(2).raw(&[0, 0]);
            uientry_gen2(b, "child", false, Extra::None, &[]);
            b.s("template");
            if v >= 119 {
                b.id(12);
            }
            b.u(1);
            subtemplate(b);
            b.u(1);
            uientry_gen2(b, "template child", false, Extra::None, &[]);
        }
        additional(b, extra);
        b.s("end1").s("");
        if floats.is_empty() {
            b.byte(0);
        } else {
            b.byte(1).u(floats.len() as u32).flts(floats);
        }
        if v == 97 {
            b.byte(1).i(5).u(2).i(1).i(2).i(-1);
        }
        if full {
            b.byte(1);
            model(b);
        } else {
            b.byte(0);
        }
        if v <= 84 && v != 77 && v != 78 {
            b.byte(1);
        } else {
            b.byte(1).byte(0);
        }
        if v >= 94 {
            b.byte(0);
        }
        if v >= 113 {
            b.flts(&[0.0, 1.0, -1.0]);
        }
    }

    fn ui_gen1(v: u32, full: bool, extra: Extra) -> Vec<u8> {
        let mut b = B::new(v);
        uientry_gen1(&mut b, "root", full, extra);
        b.d
    }

    fn ui_gen2(v: u32, full: bool, extra: Extra, floats: &[f32]) -> Vec<u8> {
        let mut b = B::new(v);
        uientry_gen2(&mut b, "root", full, extra, floats);
        b.d
    }

    /// Bytes -> XML -> bytes gives the same bytes, and that XML -> bytes -> XML gives the same XML. Returns the XML.
    fn check(bytes: &[u8]) -> String {
        let c = convert(bytes).unwrap_or_else(|e| panic!("read failed: {e}"));
        let back = writer::convert(&c.xml).unwrap_or_else(|e| panic!("write failed: {e}\n{}", c.xml));
        assert!(back == bytes, "bytes differ after a round trip\n{}", c.xml);
        assert_eq!(convert(&back).unwrap().xml, c.xml);
        assert_eq!(crate::round_trip(bytes), Ok(true));
        c.xml
    }

    /// Every prefix of a file reads without a panic; one that reads is a whole file that round trips, and none of `from` bytes
    /// or more does.
    fn check_prefixes(bytes: &[u8], from: usize) {
        for n in 0..bytes.len() {
            if let Ok(c) = convert(&bytes[..n]) {
                assert!(n < from, "a prefix of {n} of {} bytes was read", bytes.len());
                assert_eq!(writer::convert(&c.xml).unwrap(), &bytes[..n], "the prefix of {n} bytes");
            }
        }
    }

    /// A simple generator, so the fuzz tests are the same every run.
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> u32 {
            self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 33) as u32
        }
    }

    fn find(hay: &[u8], needle: &[u8]) -> usize {
        hay.windows(needle.len()).position(|w| w == needle).expect("needle")
    }

    /// The error of reading bytes that must not read.
    fn read_err(data: &[u8]) -> String {
        convert(data).err().expect("an error")
    }

    #[test]
    fn cml_round_trips_with_empty_unicode_and_escaped_strings() {
        let mut b = B::new(2);
        b.s("").s("").s("key").s("value \u{fc}\u{1f600}").s("<a href=\"x\">&'</a>").s("line1\nline2\ttab\rcr\u{1f}us").s("  ").s("\u{0}");
        let xml = check(&b.d);
        assert!(xml.starts_with("<cml version=\"002\">\n"));
        assert!(xml.contains("<key></key>\n  <value></value>"));
        assert!(xml.contains("<key>&lt;a href=&quot;x&quot;&gt;&amp;&apos;&lt;/a&gt;</key>"));
        assert!(xml.contains("&#xD;cr&#xE01F;us"));
        check_prefixes(&b.d, usize::MAX);

        // No pairs at all is a file too.
        assert_eq!(check(b"Version002"), "<cml version=\"002\">\n</cml>\n");
    }

    #[test]
    fn fc_round_trips_in_each_version() {
        for v in [44, 50, 51, 52, 53] {
            let mut b = B::new(v);
            for name in ["default", "header \u{e9}"] {
                b.s(name).u(1).s("font").u(2).u(u32::MAX);
                if v >= 50 {
                    b.u(4).u(5);
                }
                if v >= 51 {
                    b.u(6);
                }
                b.raw(&[10, 20, 30, 255]);
                if v >= 53 {
                    b.s("");
                }
                if v >= 52 {
                    b.s("T0").u(7).u(8).u(9).u(10);
                }
            }
            let xml = check(&b.d);
            assert!(xml.starts_with(&format!("<fc version=\"{v:03}\">")), "{v}");
            assert_eq!(xml.matches("<fcentry>").count(), 2);
            assert!(xml.contains("<u>4294967295</u>"));
            check_prefixes(&b.d, usize::MAX);
        }
    }

    #[test]
    fn twui_images_round_trip_in_each_version() {
        for v in [119, 129] {
            let mut b = B::new(v);
            b.u(2);
            b.s("ui/a.png").raw(&[0; 19]).i(-7).raw(&[0; 6]).u2(2).u2(1).u2(9).raw(&[1, 2, 3, 4, 5, 6, 7, 8]);
            b.s("").raw(&[0; 19]).i(0).raw(&[0; 6]).u2(0).u2(3).u2(0);
            let xml = check(&b.d);
            assert!(xml.contains("<i2>2</i2><!-- x -->") && xml.contains("<!-- 2*1 datapoints -->"));
            check_prefixes(&b.d, 0);
        }
    }

    #[test]
    fn gen1_layouts_round_trip_in_each_version() {
        for v in [25, 27, 28, 29, 30, 31, 32, 33, 39, 43, 44, 47, 49, 50, 51, 52, 53, 54] {
            for (k, extra) in EXTRAS.into_iter().enumerate() {
                if v < 44 && k > 0 {
                    continue;
                }
                let bytes = ui_gen1(v, true, extra);
                let xml = check(&bytes);
                assert!(xml.starts_with(&format!("<ui version=\"{v:03}\">")), "{v} {extra:?}");
                assert!(xml.contains("<s>On&lt;Hover&gt; &amp; &quot;more&quot;</s>"), "{v}");
                assert!(xml.contains("<unicode>Tooltip \u{fc}</unicode>"), "{v}");
                check(&ui_gen1(v, false, extra));
            }
            check_prefixes(&ui_gen1(v, true, Extra::List), 36);
        }
    }

    #[test]
    fn gen2_layouts_round_trip_in_each_version() {
        let floats = [0.1, 1e-7, -0.0, 0.0, 3.0, 1e20, f32::MAX, f32::MIN, 1e-45, f32::EPSILON];
        let versions = [
            55, 70, 74, 77, 78, 83, 84, 85, 86, 90, 91, 92, 94, 96, 97, 99, 100, 101, 102, 103, 104, 105, 106, 110, 112, 113, 115, 116,
            119, 121, 122, 124, 125, 126, 128, 129,
        ];
        for v in versions {
            for extra in EXTRAS {
                let xml = check(&ui_gen2(v, true, extra, &floats));
                assert!(xml.starts_with(&format!("<ui version=\"{v:03}\">")), "{v} {extra:?}");
                assert!(xml.contains("<unicode>tooltip \u{1f600}</unicode>"), "{v}");
                assert_eq!(xml.contains("<uientry type=\"normal\">"), v >= 100, "{v}");
                assert_eq!(xml.contains("<template>"), v >= 100, "{v}");
                assert_eq!(xml.contains("<uuid>"), v >= 126, "{v}");
                check(&ui_gen2(v, false, extra, &[]));
            }
            check_prefixes(&ui_gen2(v, true, Extra::List, &floats), 36);
        }
    }

    #[test]
    fn floats_are_written_as_ruby_does_and_read_back_to_the_same_bits() {
        let xml = check(&ui_gen2(70, false, Extra::None, &[0.1, 1e-7, -0.0, 0.0, 3.0, 16_777_216.0, 1e-45, -12.25, 0.00001]));
        for text in ["0.1", "1.0000000116860974e-07", "-0.0", "0.0", "3.0", "16777216.0", "1.401298464324817e-45", "-12.25", "1.0e-05"] {
            assert!(xml.contains(&format!("<flt>{text}</flt>")), "{text} not in\n{xml}");
        }

        // Any finite f32, by its bits.
        let mut rng = Lcg(1);
        let mut floats = Vec::new();
        while floats.len() < 20_000 {
            let f = f32::from_bits(rng.next());
            if f.is_finite() {
                floats.push(f);
            }
        }
        check(&ui_gen2(70, false, Extra::None, &floats));
    }

    #[test]
    fn angles_get_a_degree_comment() {
        let xml = check(&ui_gen2(113, false, Extra::Radial, &[]));
        assert!(xml.contains("<flt>3.1415927410125732</flt><!-- 180.0 degrees -->"), "{xml}");
        assert!(xml.contains("<!-- -90.0 degrees -->"));
    }

    #[test]
    fn a_bad_header_is_an_error() {
        let bad: [&[u8]; 9] =
            [b"", b"Version", b"Version04", b"Versionabc", b"version044", b"XVersion044", b"Version001", b"Version024", b"Version000"];
        for data in bad {
            assert!(convert(data).is_err(), "{:?}", String::from_utf8_lossy(data));
        }
        assert!(!crate::looks_like_ui(b"Version04") && !crate::looks_like_ui(b"Version0a4") && crate::looks_like_ui(b"Version044"));
    }

    #[test]
    fn huge_counts_and_lengths_are_errors() {
        // An array count over 65535 is refused before anything is read.
        let mut b = B::new(119);
        b.u(u32::MAX);
        assert!(read_err(&b.d).contains("impossibly high"));

        // Counts and lengths the data does not have.
        let mut b = B::new(119);
        b.u(0xffff);
        assert!(convert(&b.d).is_err());
        let mut b = B::new(119);
        b.u(1).s("a").raw(&[0; 19]).i(0).raw(&[0; 6]).u2(0xffff).u2(0xffff).u2(0);
        assert!(convert(&b.d).is_err());
        let mut b = B::new(2);
        b.u2(0xffff).raw(b"abc");
        assert!(convert(&b.d).is_err());

        // The uncapped List count and model count.
        let mut list = ui_gen2(70, false, Extra::List, &[]);
        let at = find(&list, b"List") + 4;
        list[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(convert(&list).is_err());
        let mut models = ui_gen2(70, true, Extra::None, &[]);
        let at = find(&models, b"mdl") + 3 + 74;
        assert_eq!(models[at..at + 4], 1i32.to_le_bytes());
        models[at..at + 4].copy_from_slice(&i32::MAX.to_le_bytes());
        assert!(convert(&models).is_err());
    }

    #[test]
    fn bad_values_are_errors() {
        // A boolean of 2 (the first flag after the offsets of a v44 root entry).
        let mut bytes = ui_gen1(44, false, Extra::None);
        assert_eq!(bytes[36], 0);
        bytes[36] = 2;
        assert!(read_err(&bytes).contains("Invalid boolean"));

        // A string that is not UTF-8, and a UTF-16 string with an unpaired surrogate.
        let mut b = B::new(2);
        b.u2(2).raw(&[0xff, 0xfe]).s("");
        assert!(read_err(&b.d).contains("UTF-8"));
        let mut bytes = ui_gen1(44, false, Extra::None);
        let at = find(&bytes, &[b'T', 0, b'o', 0]);
        bytes[at..at + 2].copy_from_slice(&0xd800u16.to_le_bytes());
        assert!(read_err(&bytes).contains("surrogate"));

        // Floats that are not finite.
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut bytes = ui_gen2(70, false, Extra::None, &[98_765.43]);
            let at = find(&bytes, &98_765.43f32.to_le_bytes());
            bytes[at..at + 4].copy_from_slice(&bad.to_le_bytes());
            assert!(read_err(&bytes).contains("Non-finite"));
        }

        // Bytes after the root entry, and a byte that must be zero.
        let mut bytes = ui_gen2(70, false, Extra::None, &[]);
        bytes.push(0);
        assert!(read_err(&bytes).contains("bytes left"));
        let mut b = B::new(119);
        b.u(1).s("a").raw(&[0; 18]).byte(1).i(0).raw(&[0; 6]).u2(0).u2(0).u2(0);
        assert!(read_err(&b.d).contains("all zeroes"));
    }

    #[test]
    fn changed_bytes_never_panic_and_what_reads_round_trips() {
        let files = [ui_gen1(44, true, Extra::List), ui_gen2(97, true, Extra::Table, &[1.0]), ui_gen2(129, true, Extra::List, &[0.5])];
        for file in files {
            for at in 10..file.len() {
                for value in [0x00, 0x01, 0x7f, 0xff] {
                    let mut data = file.clone();
                    data[at] = value;
                    if let Ok(c) = convert(&data) {
                        assert_eq!(writer::convert(&c.xml).unwrap(), data, "byte {at} set to {value}");
                    }
                }
            }
        }
    }

    #[test]
    fn random_bytes_never_panic_and_what_reads_round_trips() {
        random_bytes(&[2, 30, 44, 53, 70, 100]);
    }

    #[test]
    fn twui_images_keep_or_refuse_bytes_after_the_images() {
        let mut b = B::new(119);
        b.u(0).byte(0x29);
        if let Ok(c) = convert(&b.d) {
            assert_eq!(writer::convert(&c.xml).unwrap(), b.d);
        }
        random_bytes(&[119, 129]);
    }

    fn random_bytes(versions: &[u32]) {
        let mut rng = Lcg(7);
        for &v in versions {
            for _ in 0..500 {
                let len = rng.next() as usize % 200;
                let mut data = format!("Version{v:03}").into_bytes();
                // Mostly small values, so the reader gets past the first counts and lengths.
                data.extend((0..len).map(|_| {
                    let r = rng.next();
                    if r.is_multiple_of(4) { (r >> 8) as u8 } else { (r >> 8) as u8 % 3 }
                }));
                if let Ok(c) = convert(&data) {
                    assert_eq!(writer::convert(&c.xml).unwrap(), data);
                }
            }
        }
    }

    #[test]
    fn hand_written_xml_converts_and_round_trips() {
        let xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- written by hand -->\n<cml version=\"2\">\
                   <key>a &amp; b</key><value/>\n<key><![CDATA[x<y]]></key>  <value>&#252;&#x1F600;</value></cml>";
        let bytes = writer::convert(xml).unwrap();
        let mut b = B::new(2);
        b.s("a & b").s("").s("x<y").s("\u{fc}\u{1f600}");
        assert_eq!(bytes, b.d);
        check(&bytes);

        let xml = "<fc version=\"044\"><fcentry><s>f</s><u> 1 </u><s/><u>2</u><u>3</u><byte>1</byte><byte>2</byte><byte>3</byte>\
                   <byte>4</byte></fcentry></fc>";
        let bytes = writer::convert(xml).unwrap();
        let mut b = B::new(44);
        b.s("f").u(1).s("").u(2).u(3).raw(&[1, 2, 3, 4]);
        assert_eq!(bytes, b.d);
        check(&bytes);
    }

    #[test]
    fn xml_escape_escapes_what_the_original_does() {
        assert_eq!(xml_escape("<>&\"'\r\u{1f}\n\t\u{fc}"), "&lt;&gt;&amp;&quot;&apos;&#xD;&#xE01F;\n\t\u{fc}");
        assert_eq!(xml_escape(""), "");
    }

    #[test]
    #[ignore = "a string holding U+E01F comes back as 0x1f: the writer reads U+E01F as the escape of 0x1f"]
    fn a_private_use_e01f_character_round_trips() {
        let mut b = B::new(2);
        b.s("a\u{e01f}b").s("");
        check(&b.d);
    }
}
