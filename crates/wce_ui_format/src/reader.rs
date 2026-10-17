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
        let units: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
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
        let vv = [hex_bytes(&b[0..4], ""), hex_bytes(&b[4..6], ""), hex_bytes(&b[6..8], ""), hex_bytes(&b[8..10], ""), hex_bytes(&b[10..16], "")].join("-");
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
        if !zero {
            if let Some(c) = c {
                self.out(&format!("<!-- {} -->", c));
            }
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
        self.tag("events", &[], |s| loop {
            let e = s.get_s()?;
            if e == "events_end" {
                return Ok(());
            }
            s.tag("event", &[], |s| {
                s.out(&format!("<s>{}</s>", xml_escape(&e)));
                Ok(())
            })?;
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
            })
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
