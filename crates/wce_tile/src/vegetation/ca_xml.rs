//! CA's own XML reader (`CA::XMLParser`, CAUtils.AssemblyKit.dll), which BOB reads the vegetation parameters with. Several of the shipped
//! parameter files are not well-formed, and what BOB generates depends on how this reader takes them:
//!
//! - an end tag closes the innermost open element whatever its name (`</group>` after a stray `</tree>` closes the `<trees>` root);
//! - an end tag at the document level ends the parse;
//! - a tag name runs up to whitespace, `>` or `/>` (so `<a\u{a0}/>` is a tag named `a\u{a0}`);
//! - a tag that can't be read stops the parse, and what was read before it is kept;
//! - a file starting with a byte >= 0x80 (a UTF-8 BOM) gives an empty document.
//!
//! The text is bytes (Latin-1); element text and attributes are kept as they are, entities decoded.

/// An element.
#[derive(Clone, Debug, Default)]
pub struct Element {
    pub name: String,
    pub attributes: Vec<(String, String)>,
    pub children: Vec<Element>,
    pub text: String,
}

impl Element {
    /// The first child with this name.
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.name == name)
    }

    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    /// The text of the first child with this name.
    pub fn child_text(&self, name: &str) -> Option<&str> {
        self.child(name).map(|c| c.text.as_str())
    }
}

fn is_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r')
}

struct Parser<'a> {
    s: &'a [u8],
    p: usize,
}

impl Parser<'_> {
    fn at(&self, i: usize) -> u8 {
        self.s.get(i).copied().unwrap_or(0)
    }

    fn cur(&self) -> u8 {
        self.at(self.p)
    }

    fn find(&self, from: usize, pat: &[u8]) -> Option<usize> {
        self.s.get(from..)?.windows(pat.len()).position(|w| w == pat).map(|i| i + from)
    }

    /// `FUN_10009310`: text, entities, comments and child elements up to an end tag (true) or the end of the text (true); false on a broken
    /// child.
    fn content(&mut self, el: &mut Element) -> bool {
        while self.cur() != 0 {
            match self.cur() {
                b'<' => {
                    let next = self.at(self.p + 1);
                    if next == b'/' {
                        return true;
                    }

                    if next == b'!' {
                        if self.at(self.p + 2) == b'-' && self.at(self.p + 3) == b'-' {
                            let Some(end) = self.find(self.p + 4, b"-->") else { return false };
                            self.p = end + 3;
                        } else {
                            let Some(end) = self.find(self.p + 2, b">") else { return false };
                            self.p = end + 1;
                        }

                        continue;
                    }

                    el.children.push(Element::default());
                    let ok = {
                        let child = el.children.last_mut().expect("just pushed");
                        self.tag(child)
                    };

                    if !ok {
                        return false;
                    }

                    self.p += 1;
                }
                b'&' => {
                    let Some(end) = self.find(self.p, b";") else { return false };
                    let entity = &self.s[self.p + 1..end];
                    el.text.push_str(match entity {
                        b"lt" => "<",
                        b"gt" => ">",
                        b"amp" => "&",
                        b"quot" => "\"",
                        b"apos" => "'",
                        _ => "",
                    });

                    self.p = end + 1;
                }
                c => {
                    el.text.push(char::from(c));
                    self.p += 1;
                }
            }
        }

        true
    }

    /// `FUN_100089a0`: one element from its `<`; on success `p` is on the last character of the element (`>`), as the caller skips it.
    fn tag(&mut self, el: &mut Element) -> bool {
        self.p += 1;
        let start = self.p;
        loop {
            let c = self.cur();
            if is_ws(c) || c == b'>' || (c == b'/' && self.at(self.p + 1) == b'>') {
                break;
            }

            if c == 0 {
                return false;
            }

            self.p += 1;
        }

        el.name = latin1(&self.s[start..self.p]);
        while is_ws(self.cur()) {
            self.p += 1;
        }

        if self.cur() == 0 {
            return false;
        }

        if self.cur() != b'/' && self.cur() != b'>' {
            loop {
                if !self.attribute(el) {
                    return false;
                }

                while is_ws(self.cur()) {
                    self.p += 1;
                }

                if self.cur() == 0 {
                    return false;
                }

                if self.cur() == b'/' || self.cur() == b'>' {
                    break;
                }
            }
        }

        if self.cur() == b'/' {
            self.p += 1;
            return true;
        }

        self.p += 1;
        if !self.content(el) {
            return false;
        }

        // The end tag's name is not looked at.
        self.p += 2;
        if self.cur() == b'>' {
            return true;
        }

        while self.cur() != 0 {
            self.p += 1;
            if self.cur() == b'>' {
                return true;
            }
        }

        false
    }

    /// `FUN_10007500`: `name = 'value'` with any quote character.
    fn attribute(&mut self, el: &mut Element) -> bool {
        let start = self.p;
        loop {
            if self.cur() == 0 {
                return false;
            }

            self.p += 1;
            if self.cur() == b'=' || is_ws(self.cur()) {
                break;
            }
        }

        let name = latin1(&self.s[start..self.p]);
        while is_ws(self.cur()) {
            self.p += 1;
        }

        if self.cur() == 0 {
            return false;
        }

        self.p += 1;
        while is_ws(self.cur()) {
            self.p += 1;
        }

        let quote = self.cur();
        if quote == 0 {
            return false;
        }

        self.p += 1;
        let Some(end) = self.s[self.p..].iter().position(|&c| c == quote).map(|i| i + self.p) else { return false };
        el.attributes.push((name, latin1(&self.s[self.p..end])));
        self.p = end + 1;
        true
    }
}

fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| char::from(c)).collect()
}

/// The document: an element whose children are the top-level elements.
pub fn parse(bytes: &[u8]) -> Element {
    let mut doc = Element::default();
    if bytes.first().is_some_and(|&b| b >= 0x80) {
        return doc;
    }

    let mut p = Parser { s: bytes, p: 0 };
    if bytes.starts_with(b"<?xml") && bytes.get(5).is_some_and(|&c| is_ws(c)) {
        p.p = 6;
        loop {
            while is_ws(p.cur()) {
                p.p += 1;
            }

            if p.cur() == 0 {
                return doc;
            }

            if p.cur() == b'?' && p.at(p.p + 1) == b'>' {
                p.p += 2;
                break;
            }

            if !p.attribute(&mut Element::default()) {
                return doc;
            }
        }
    }

    p.content(&mut doc);
    doc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_tags_close_the_innermost_element() {
        let doc = parse(b"<?xml version=\"1.0\"?>\n<trees><group group_name='a'><tree><model>m</model></tree></tree></group><group group_name='b'/></trees>");
        let trees = doc.child("trees").expect("trees");
        assert_eq!(trees.children.len(), 1);
        assert_eq!(trees.children[0].attribute("group_name"), Some("a"));
        // The stray </tree> closed group a, </group> closed trees, and group b is a sibling of trees.
        assert_eq!(doc.children.len(), 2);
    }

    #[test]
    fn names_run_to_whitespace_or_the_end_of_the_tag() {
        let doc = parse("<a><b\u{a0}/><c>1</c></a>".as_bytes());
        let a = doc.child("a").expect("a");
        assert_eq!(a.children.len(), 2);
        assert_eq!(a.child_text("c"), Some("1"));
    }

    #[test]
    fn a_bom_gives_an_empty_document() {
        assert!(parse(b"\xef\xbb\xbf<a/>").children.is_empty());
    }
}
