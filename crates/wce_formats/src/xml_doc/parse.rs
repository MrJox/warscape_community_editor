//! Reading the element table of a text.
//!
//! The reader is tolerant: an end tag closes every element opened after the one of its name (unbalanced CA files), and
//! comments, CDATA, processing instructions and declarations are skipped.

use memchr::{memchr, memmem};

use super::{Result, XmlAttr, XmlElement, XmlError};

/// Whitespace between XML tokens.
fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

/// A byte that ends a name.
fn is_name_end(byte: u8) -> bool {
    is_space(byte) || matches!(byte, b'/' | b'>' | b'=' | 0)
}

/// A syntax error.
pub(super) fn fail<T>(offset: usize, what: impl Into<String>) -> Result<T> {
    Err(XmlError::Syntax { offset, what: what.into() })
}

/// The byte at `i`, or 0 past the end of the range being read.
fn at(t: &[u8], n: usize, i: usize) -> u8 {
    if i < n { t[i] } else { 0 }
}

/// Reads the start tag at `t[at_off]` (a `<`): name, attributes, where the attribute list ends and where the tag does.
/// Returns the element (without parent) and the offset after the tag.
pub(super) fn parse_start_tag(t: &[u8], n: usize, at_off: usize, scratch: &mut Vec<XmlAttr>) -> Result<(XmlElement, usize)> {
    let mut el = XmlElement { start_off: at_off, ..Default::default() };
    let s = at_off + 1;
    let mut e = s;
    while !is_name_end(at(t, n, e)) {
        e += 1;
    }

    if e == s {
        return fail(at_off, "empty element name");
    }

    el.name = String::from_utf8_lossy(&t[s..e]).into_owned();
    let mut p = e;
    loop {
        while is_space(at(t, n, p)) {
            p += 1;
        }

        let c = at(t, n, p);
        if c == 0 {
            return fail(at_off, "unterminated start tag");
        }

        if c == b'/' && at(t, n, p + 1) == b'>' {
            el.attrs_end = p;
            el.self_closing = true;
            p += 2;
            break;
        }

        if c == b'>' {
            el.attrs_end = p;
            p += 1;
            break;
        }

        let ns = p;
        while !is_name_end(at(t, n, p)) {
            p += 1;
        }

        if p == ns {
            return fail(p, "bad attribute");
        }

        let name = String::from_utf8_lossy(&t[ns..p]).into_owned();
        while is_space(at(t, n, p)) {
            p += 1;
        }

        if at(t, n, p) != b'=' {
            return fail(p, format!("expected '=' after {name}"));
        }

        p += 1;
        while is_space(at(t, n, p)) {
            p += 1;
        }

        let quote = at(t, n, p);
        if quote != b'\'' && quote != b'"' {
            return fail(p, format!("expected quote for {name}"));
        }

        // The value may run past the range: the caller of a range checks where reading stopped.
        let Some(value_end) = memchr(quote, &t[p + 1..]).map(|end| p + 1 + end) else {
            return fail(p, "unterminated value");
        };

        scratch.push(XmlAttr { name, value_off: p + 1, value_len: value_end - (p + 1), quote, appended: false });
        p = value_end + 1;
    }

    el.attrs = std::mem::take(scratch);
    el.start_end = p;
    el.end_off = p;
    Ok((el, p))
}

/// What a piece of a bigger document leaves open for its neighbours: end tags that close elements started before the piece
/// (`ext`, with the number of top-level elements read before them) and the elements still open at its end (`open`, outermost first).
#[derive(Default)]
pub(super) struct ChunkTail {
    ext: Vec<Close>,
    open: Vec<usize>,
}

/// An end tag that closes an element of an earlier piece.
struct Close {
    after_tops: usize,
    name: String,
    end: usize,
}

/// Finds `needle` in `t` from `from`, as an absolute offset.
fn find(t: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    memmem::find(t.get(from..)?, needle).map(|found| from + found)
}

/// Reads the elements of `t[begin..n]` into `elems` (indices continue from its length; top-level ones get no parent and are listed
/// in `roots`). Returns the offset after the last construct read. With a `tail` the range may start and stop in the middle of the tree.
pub(super) fn parse_range(t: &[u8], begin: usize, n: usize, elems: &mut Vec<XmlElement>, roots: &mut Vec<usize>, mut tail: Option<&mut ChunkTail>) -> Result<usize> {
    let skip_to = |i: usize, end: &str| match find(t, i, end.as_bytes()) {
        Some(p) => Ok(p + end.len()),
        None => fail(i, format!("unterminated '{end}'")),
    };

    let mut stack: Vec<usize> = vec![];
    let mut scratch = vec![];
    let mut i = begin;
    while i < n {
        let Some(lt) = memchr(b'<', &t[i..]).map(|lt| i + lt) else { break };
        if lt >= n {
            break;
        }

        i = lt;
        let rest = &t[i..];
        if rest.starts_with(b"<!--") {
            i = skip_to(i + 4, "-->")?;
            continue;
        }

        if rest.starts_with(b"<![CDATA[") {
            i = skip_to(i + 9, "]]>")?;
            continue;
        }

        if at(t, n, i + 1) == b'?' {
            i = skip_to(i + 2, "?>")?;
            continue;
        }

        if at(t, n, i + 1) == b'!' {
            i = skip_to(i + 2, ">")?;
            continue;
        }

        if at(t, n, i + 1) == b'/' {
            let s = i + 2;
            let mut e = s;
            while !is_name_end(at(t, n, e)) {
                e += 1;
            }

            let name = String::from_utf8_lossy(&t[s..e]);
            let Some(close) = memchr(b'>', &t[e.min(t.len())..]).map(|close| e + close) else {
                return fail(i, "unterminated end tag");
            };

            // Unbalanced files: the end tag closes everything opened after the element of its name.
            let Some(matched) = stack.iter().rposition(|&open| elems[open].name == name) else {
                if let Some(tail) = tail.as_deref_mut()
                    && stack.is_empty()
                {
                    tail.ext.push(Close { after_tops: roots.len(), name: name.into_owned(), end: close + 1 });
                    i = close + 1;
                    continue;
                }

                return fail(i, format!("end tag </{name}> without start"));
            };

            for open in stack.drain(matched..) {
                elems[open].end_off = close + 1;
            }

            i = close + 1;
            continue;
        }

        let (mut el, p) = parse_start_tag(t, n, i, &mut scratch)?;
        el.parent = stack.last().copied();
        let index = elems.len();
        let self_closing = el.self_closing;
        match el.parent {
            Some(parent) => elems[parent].children.push(index),
            None => roots.push(index),
        }

        elems.push(el);
        if !self_closing {
            stack.push(index);
        }

        i = p;
    }

    if let Some(&last) = stack.last() {
        let Some(tail) = tail else { return fail(n, format!("unclosed <{}>", elems[last].name)) };
        tail.open = stack;
    }

    Ok(i)
}

/// Reads the element table of a whole text, in pieces side by side if it has `parallel_min` bytes or more.
pub(super) fn parse_text(t: &[u8], parallel_min: usize) -> Result<(Vec<XmlElement>, Vec<usize>)> {
    if t.len() >= parallel_min {
        let pieces = (t.len() / (parallel_min / 4).max(1024)).clamp(2, 32);
        if let Some(table) = parse_in_pieces(t, pieces) {
            return Ok(table);
        }
    }

    let mut elems = Vec::with_capacity(t.len() / 256 + 16);
    let mut roots = vec![];
    parse_range(t, 0, t.len(), &mut elems, &mut roots, None)?;
    Ok((elems, roots))
}

/// A piece read on its own: its elements (indices local to it), its top-level elements and what it leaves open.
#[derive(Default)]
struct Piece {
    elems: Vec<XmlElement>,
    tops: Vec<usize>,
    tail: ChunkTail,
}

/// Reads the whole text in pieces side by side. The pieces start at a `<` after a line break; the merge checks that no construct
/// (comment, CDATA, attribute value) crosses a piece boundary and that every end tag that leaves its piece closes the innermost
/// open element, which is true for every well formed file. Anything else returns `None` and the caller reads the text in one go
/// (which also gives the proper error message).
fn parse_in_pieces(t: &[u8], pieces: usize) -> Option<(Vec<XmlElement>, Vec<usize>)> {
    let size = t.len();
    let mut cut = vec![0];
    'cuts: for k in 1..pieces {
        let mut p = (cut[cut.len() - 1] + 1).max(size / pieces * k);
        loop {
            let Some(line_end) = memchr(b'\n', t.get(p..).unwrap_or_default()).map(|found| p + found) else { break 'cuts };
            let mut q = line_end + 1;
            while q < size && (t[q] == b' ' || t[q] == b'\t') {
                q += 1;
            }

            p = q;
            if q < size && t[q] == b'<' {
                break;
            }
        }

        cut.push(p);
    }

    cut.push(size);
    let count = cut.len() - 1;
    if count < 2 {
        return None;
    }

    let read = std::thread::scope(|scope| {
        let handles = (0..count).map(|k| {
            let (begin, end) = (cut[k], cut[k + 1]);
            scope.spawn(move || {
                let mut piece = Piece { elems: Vec::with_capacity((end - begin) / 256 + 16), ..Default::default() };
                let stop = parse_range(t, begin, end, &mut piece.elems, &mut piece.tops, Some(&mut piece.tail)).ok()?;
                (stop <= end).then_some(piece)
            })
        }).collect::<Vec<_>>();

        handles.into_iter().map(|handle| handle.join().ok().flatten()).collect::<Option<Vec<_>>>()
    })?;

    let total = read.iter().map(|piece| piece.elems.len()).sum();
    let mut out = Vec::with_capacity(total);
    let mut roots = vec![];

    // The elements at a boundary: parents from the open elements of the pieces before, and the end tags that leave a piece.
    let mut open: Vec<usize> = vec![];
    for piece in read {
        let base = out.len();
        for mut el in piece.elems {
            el.parent = el.parent.map(|parent| parent + base);
            el.children.iter_mut().for_each(|child| *child += base);
            out.push(el);
        }

        let mut open_top = |out: &mut Vec<XmlElement>, open: &[usize], local: usize| {
            let index = base + local;
            let parent = open.last().copied();
            out[index].parent = parent;
            match parent {
                Some(parent) => out[parent].children.push(index),
                None => roots.push(index),
            }
        };

        let mut top = 0;
        for close in &piece.tail.ext {
            while top < close.after_tops {
                open_top(&mut out, &open, piece.tops[top]);
                top += 1;
            }

            let innermost = open.pop()?;
            if out[innermost].name != close.name {
                return None;
            }

            out[innermost].end_off = close.end;
        }

        while top < piece.tops.len() {
            open_top(&mut out, &open, piece.tops[top]);
            top += 1;
        }

        open.extend(piece.tail.open.iter().map(|&local| base + local));
    }

    open.is_empty().then_some((out, roots))
}

