//! A small XML event reader, enough for the converter's own output and for hand-edited files of the same shape:
//! elements, attributes, text, character references, comments, CDATA, `<?...?>` and `<!DOCTYPE>` (skipped).

#[derive(Debug, Clone, PartialEq)]
pub enum Ev {
    Start(String, Vec<(String, String)>),
    End(String),
    Text(String),
    /// The text between `<!--` and `-->`, as written.
    Comment(String),
}

/// 1-based line and column of a byte offset, for error messages.
pub fn line_col(src: &str, ofs: usize) -> (usize, usize) {
    let ofs = ofs.min(src.len());
    let before = &src.as_bytes()[..ofs];
    let line = before.iter().filter(|b| **b == b'\n').count() + 1;
    let col = ofs - before.iter().rposition(|b| *b == b'\n').map(|p| p + 1).unwrap_or(0) + 1;
    (line, col)
}

#[derive(Debug)]
pub struct XmlError {
    pub offset: usize,
    pub msg: String,
}

fn decode_entities(s: &str, at: usize) -> Result<String, XmlError> {
    if !s.contains('&') {
        return Ok(s.to_string());
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        let after = &rest[p + 1..];
        let Some(semi) = after.find(';') else {
            return Err(XmlError { offset: at, msg: "unterminated character reference".into() });
        };
        let name = &after[..semi];
        match name {
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "amp" => out.push('&'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            _ => {
                let cp = if let Some(h) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                    u32::from_str_radix(h, 16).ok()
                } else if let Some(d) = name.strip_prefix('#') {
                    d.parse::<u32>().ok()
                } else {
                    None
                };
                match cp.and_then(char::from_u32) {
                    Some(c) => out.push(c),
                    None => return Err(XmlError { offset: at, msg: format!("unknown entity &{};", name) }),
                }
            }
        }
        rest = &after[semi + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Parse to events. Text events carry decoded text (adjacent text and CDATA are merged); every event is paired
/// with the byte offset where it starts.
pub fn parse(src: &str) -> Result<Vec<(usize, Ev)>, XmlError> {
    let b = src.as_bytes();
    let mut i = 0;
    let mut evs: Vec<(usize, Ev)> = Vec::new();
    let mut depth = 0usize;
    let mut text = String::new();
    let mut text_at = 0usize;

    macro_rules! flush {
        () => {
            if !text.is_empty() {
                evs.push((text_at, Ev::Text(std::mem::take(&mut text))));
            }
        };
    }

    while i < b.len() {
        if b[i] != b'<' {
            let end = src[i..].find('<').map(|p| i + p).unwrap_or(b.len());
            if text.is_empty() {
                text_at = i;
            }
            text.push_str(&decode_entities(&src[i..end], i)?);
            i = end;
            continue;
        }
        let rest = &src[i..];
        if rest.starts_with("<!--") {
            let Some(e) = rest.find("-->") else {
                return Err(XmlError { offset: i, msg: "unterminated comment".into() });
            };
            flush!();
            evs.push((i, Ev::Comment(rest[4..e].to_string())));
            i += e + 3;
        } else if rest.starts_with("<![CDATA[") {
            let Some(e) = rest.find("]]>") else {
                return Err(XmlError { offset: i, msg: "unterminated CDATA".into() });
            };
            if text.is_empty() {
                text_at = i;
            }
            text.push_str(&rest[9..e]);
            i += e + 3;
        } else if rest.starts_with("<?") {
            let Some(e) = rest.find("?>") else {
                return Err(XmlError { offset: i, msg: "unterminated declaration".into() });
            };
            i += e + 2;
        } else if rest.starts_with("<!") {
            let Some(e) = rest.find('>') else {
                return Err(XmlError { offset: i, msg: "unterminated declaration".into() });
            };
            i += e + 1;
        } else if rest.starts_with("</") {
            flush!();
            let Some(e) = rest.find('>') else {
                return Err(XmlError { offset: i, msg: "unterminated end tag".into() });
            };
            let name = rest[2..e].trim().to_string();
            if depth == 0 {
                return Err(XmlError { offset: i, msg: format!("unexpected </{}>", name) });
            }
            depth -= 1;
            evs.push((i, Ev::End(name)));
            i += e + 1;
        } else {
            flush!();
            let start = i;
            i += 1;
            let ns = i;
            while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' && b[i] != b'/' {
                i += 1;
            }
            let name = src[ns..i].to_string();
            if name.is_empty() {
                return Err(XmlError { offset: start, msg: "empty tag name".into() });
            }
            let mut attrs = Vec::new();
            let self_close;
            loop {
                while i < b.len() && b[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i >= b.len() {
                    return Err(XmlError { offset: start, msg: format!("unterminated tag <{}", name) });
                }
                if b[i] == b'>' {
                    i += 1;
                    self_close = false;
                    break;
                }
                if b[i] == b'/' {
                    if b.get(i + 1) != Some(&b'>') {
                        return Err(XmlError { offset: i, msg: "stray / in tag".into() });
                    }
                    i += 2;
                    self_close = true;
                    break;
                }
                let ks = i;
                while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'=' && b[i] != b'>' && b[i] != b'/' {
                    i += 1;
                }
                let key = src[ks..i].to_string();
                while i < b.len() && b[i].is_ascii_whitespace() {
                    i += 1;
                }
                if b.get(i) != Some(&b'=') {
                    return Err(XmlError { offset: ks, msg: format!("attribute {} has no value", key) });
                }
                i += 1;
                while i < b.len() && b[i].is_ascii_whitespace() {
                    i += 1;
                }
                let q = match b.get(i) {
                    Some(q @ (b'"' | b'\'')) => *q,
                    _ => return Err(XmlError { offset: i, msg: "attribute value must be quoted".into() }),
                };
                i += 1;
                let vs = i;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                if i >= b.len() {
                    return Err(XmlError { offset: vs, msg: "unterminated attribute value".into() });
                }
                attrs.push((key, decode_entities(&src[vs..i], vs)?));
                i += 1;
            }
            evs.push((start, Ev::Start(name.clone(), attrs)));
            if self_close {
                evs.push((start, Ev::End(name)));
            } else {
                depth += 1;
            }
        }
    }
    flush!();
    if depth != 0 {
        return Err(XmlError { offset: b.len(), msg: "unclosed element".into() });
    }
    Ok(evs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic() {
        let e = parse("<?xml version=\"1.0\"?><a x=\"1&amp;2\"><!-- c --><s>a &lt; b&#xD;&#xE01F;</s><yes /></a>").unwrap();
        assert_eq!(e[0].1, Ev::Start("a".into(), vec![("x".into(), "1&2".into())]));
        assert_eq!(e[1].1, Ev::Comment(" c ".into()));
        assert_eq!(e[3].1, Ev::Text("a < b\r\u{e01f}".into()));
        assert_eq!(e[5].1, Ev::Start("yes".into(), vec![]));
    }
}
