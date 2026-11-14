//! Combining variations into one tile.wce (docs/plans/tile_variations.md, "The scene merge").
//!
//! Each variation's text is a tree of lines (TEd's family of files: one element per line, a fixed indent). The merge goes down from the
//! root. A container's children are a sequence of keys: an instance's key is its whole text, a container's (a layer, a group, the
//! wrappers) its start tag's line. The sequences are matched pairwise (common ends, then keys unique on both sides as anchors, a full
//! LCS for small gaps and a greedy match for big ones) and merged so that each variation's sequence is in the result in its own order.
//! Matched containers are merged in turn. A run of children that only some variations have is tagged: layers and groups get
//! `wce_variations` themselves, anything else goes in a [`FOLDER`] at the run's place, one indent deeper.
//!
//! [`super::to_variation`] of the result gives every variation back byte for byte; [`check`] makes sure of it. A poorer match only shares
//! less, it is never wrong.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use wce_formats::xml_doc::XmlDoc;

use super::{FOLDER, START, TAGGABLE, VARIATIONS_ATTR, VIEW_STATE, close_tag, indent_len, line_start, list_text, whole_lines};

/// The elements whose children are merged one by one; anything else is matched whole.
pub const CONTAINERS: [&str; 9] = [
    "project",
    "scene",
    "SCENE_GRAPH",
    "children",
    "modules",
    "LayerInstance",
    "UserInstanceGroup",
    "INSTANCE_GROUP",
    "LAYER_INSTANCE_GROUP",
];

/// Gaps of sequences up to this many pairs (`len a x len b`) get a full LCS.
const FULL_LCS: usize = 4_000_000;

/// One variation's tile.wce.
pub struct Source {
    pub variation: String,
    pub text: Vec<u8>,
}

/// What a merge shared.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Each variation's instances (what is matched whole), in the sources' order.
    pub instances: Vec<usize>,

    /// The instances of the combined file.
    pub combined: usize,

    /// Of those, the ones every variation has.
    pub in_all: usize,

    /// The instances two variations share, by their indices (`shared[a][b]`, `shared[a][a]` is `instances[a]`).
    pub shared: Vec<Vec<usize>>,

    /// The sources' bytes, all together, and the combined file's.
    pub bytes_in: usize,
    pub bytes_out: usize,
}

impl Stats {
    /// The share of a variation's instances that another has too, 0 to 1.
    #[must_use]
    pub fn share(&self, of: usize, with: usize) -> f64 {
        self.shared[of][with] as f64 / self.instances[of].max(1) as f64
    }

    /// The variation sharing the most with the others: the reference a combination's terrain base comes from.
    #[must_use]
    pub fn reference(&self) -> usize {
        (0..self.instances.len())
            .max_by_key(|&a| (0..self.instances.len()).filter(|&b| b != a).map(|b| self.shared[a][b]).sum::<usize>())
            .unwrap_or(0)
    }

    /// Each pair's shares, `a/b 63%` (of the first's instances).
    #[must_use]
    pub fn pairs(&self, names: &[String]) -> String {
        let mut out = vec![];
        for a in 0..self.instances.len() {
            for b in a + 1..self.instances.len() {
                out.push(format!("{}/{} {:.0}%/{:.0}%", names[a], names[b], 100.0 * self.share(a, b), 100.0 * self.share(b, a)));
            }
        }

        out.join(", ")
    }

    /// A line for people.
    #[must_use]
    pub fn summary(&self) -> String {
        let total = self.instances.iter().sum::<usize>();
        format!(
            "{} instances in {} variations ({}) combined into {} ({} in every variation); {:.1} MB instead of {:.1} MB",
            total,
            self.instances.len(),
            self.instances.iter().map(ToString::to_string).collect::<Vec<_>>().join(" + "),
            self.combined,
            self.in_all,
            self.bytes_out as f64 / 1e6,
            self.bytes_in as f64 / 1e6,
        )
    }
}

/// The combined file.
pub struct Combined {
    pub text: Vec<u8>,
    pub stats: Stats,
}

struct Src {
    doc: XmlDoc,
}

impl Src {
    fn text(&self) -> &[u8] {
        self.doc.original_text()
    }
}

/// A child in a merged sequence: the sources that have it, by (source, element).
struct Item {
    key: u64,
    container: bool,
    members: Vec<(usize, usize)>,
}

impl Item {
    fn set(&self) -> u64 {
        self.members.iter().fold(0, |m, &(s, _)| m | 1 << s)
    }
}

struct Merger<'a> {
    srcs: &'a [Src],
    names: Vec<String>,
    unit: usize,
    eol: &'static [u8],
    out: Vec<u8>,
    stats: Stats,
}

fn hash_bytes(tag: u8, bytes: &[u8]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    tag.hash(&mut h);
    bytes.hash(&mut h);
    h.finish()
}

/// The lines of a container's start and end tags, when it and its children each sit on lines of their own with nothing between them.
fn container_lines(doc: &XmlDoc, text: &[u8], e: usize) -> Option<((usize, usize), (usize, usize))> {
    let el = &doc.elements()[e];
    let start = whole_lines(text, el.start_off, el.start_end)?;
    let close = close_tag(doc, text, e)?;
    let end = whole_lines(text, close, el.end_off)?;
    let mut at = start.1;
    for &c in &el.children {
        let (s, t) = whole_lines(text, doc.elements()[c].start_off, doc.elements()[c].end_off)?;
        if s != at {
            return None;
        }

        at = t;
    }

    (at == end.0).then_some((start, end))
}

/// Pairs of equal keys of `a` and `b`, increasing in both, appended to `out` with the offsets added.
fn matches(a: &[u64], b: &[u64], ao: usize, bo: usize, out: &mut Vec<(usize, usize)>) {
    let mut p = 0;
    while p < a.len() && p < b.len() && a[p] == b[p] {
        out.push((ao + p, bo + p));
        p += 1;
    }

    let mut s = 0;
    while s < a.len() - p && s < b.len() - p && a[a.len() - 1 - s] == b[b.len() - 1 - s] {
        s += 1;
    }

    let (ma, mb) = (&a[p..a.len() - s], &b[p..b.len() - s]);
    let (mao, mbo) = (ao + p, bo + p);
    if !ma.is_empty() && !mb.is_empty() {
        middle(ma, mb, mao, mbo, out);
    }

    for k in 0..s {
        out.push((ao + a.len() - s + k, bo + b.len() - s + k));
    }
}

/// [`matches`] of two sequences whose ends differ.
fn middle(a: &[u64], b: &[u64], ao: usize, bo: usize, out: &mut Vec<(usize, usize)>) {
    // Anchors: keys once in each, the longest run of them increasing in both (patience).
    let mut seen = HashMap::<u64, (u32, u32, usize, usize)>::with_capacity(a.len() + b.len());
    for (i, &k) in a.iter().enumerate() {
        let e = seen.entry(k).or_insert((0, 0, i, 0));
        e.0 += 1;
    }

    for (j, &k) in b.iter().enumerate() {
        if let Some(e) = seen.get_mut(&k) {
            e.1 += 1;
            e.3 = j;
        }
    }

    let mut pairs = seen.values().filter(|e| e.0 == 1 && e.1 == 1).map(|e| (e.2, e.3)).collect::<Vec<_>>();
    if !pairs.is_empty() {
        pairs.sort_unstable();
        let anchors = longest_increasing(&pairs);
        let (mut i, mut j) = (0, 0);
        for (ai, bj) in anchors {
            if ai > i && bj > j {
                matches(&a[i..ai], &b[j..bj], ao + i, bo + j, out);
            }

            out.push((ao + ai, bo + bj));
            (i, j) = (ai + 1, bj + 1);
        }

        if i < a.len() && j < b.len() {
            matches(&a[i..], &b[j..], ao + i, bo + j, out);
        }

        return;
    }

    if a.len().saturating_mul(b.len()) <= FULL_LCS {
        lcs(a, b, ao, bo, out);
        return;
    }

    // Big and without anchors: each key of `a` takes the first equal key of `b` after the last match.
    let mut at = HashMap::<u64, Vec<usize>>::new();
    for (j, &k) in b.iter().enumerate() {
        at.entry(k).or_default().push(j);
    }

    let mut next = 0;
    for (i, k) in a.iter().enumerate() {
        let Some(list) = at.get(k) else { continue };
        let p = list.partition_point(|&j| j < next);
        if let Some(&j) = list.get(p) {
            out.push((ao + i, bo + j));
            next = j + 1;
        }
    }
}

/// The longest subsequence of pairs (sorted by the first) increasing in the second.
fn longest_increasing(pairs: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut tails: Vec<usize> = vec![];
    let mut prev = vec![usize::MAX; pairs.len()];
    for (k, &(_, b)) in pairs.iter().enumerate() {
        let pos = tails.partition_point(|&t| pairs[t].1 < b);
        if pos > 0 {
            prev[k] = tails[pos - 1];
        }

        if pos == tails.len() {
            tails.push(k);
        } else {
            tails[pos] = k;
        }
    }

    let mut out = vec![];
    let mut k = tails.last().copied().unwrap_or(usize::MAX);
    while k != usize::MAX {
        out.push(pairs[k]);
        k = prev[k];
    }

    out.reverse();
    out
}

/// A full LCS of two short sequences.
fn lcs(a: &[u64], b: &[u64], ao: usize, bo: usize, out: &mut Vec<(usize, usize)>) {
    let (n, m) = (a.len(), b.len());
    let w = m + 1;
    let mut t = vec![0u32; (n + 1) * w];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            t[i * w + j] = if a[i] == b[j] { t[(i + 1) * w + j + 1] + 1 } else { t[(i + 1) * w + j].max(t[i * w + j + 1]) };
        }
    }

    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] {
            out.push((ao + i, bo + j));
            i += 1;
            j += 1;
        } else if t[(i + 1) * w + j] >= t[i * w + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
}

impl Merger<'_> {
    /// A child's item of one source.
    fn item(&self, s: usize, e: usize) -> Result<Item, String> {
        let src = &self.srcs[s];
        let (doc, text) = (&src.doc, src.text());
        let el = &doc.elements()[e];
        if CONTAINERS.contains(&el.name.as_str())
            && !el.self_closing
            && let Some((start, _)) = container_lines(doc, text, e)
        {
            // The start tag without its view state, which each variation keeps as it is (`WceVariationStart`).
            let mut key = text[start.0..start.1].to_vec();
            let mut blanks = el
                .attrs
                .iter()
                .filter(|a| VIEW_STATE.contains(&a.name.as_str()) && !a.appended)
                .map(|a| (a.value_off - start.0, a.value_len))
                .collect::<Vec<_>>();
            blanks.sort_unstable();
            for (at, len) in blanks.into_iter().rev() {
                key.drain(at..at + len);
            }

            return Ok(Item { key: hash_bytes(1, &key), container: true, members: vec![(s, e)] });
        }

        let (a, b) = whole_lines(text, el.start_off, el.end_off)
            .ok_or_else(|| format!("{}: <{}> at offset {} is not on lines of its own", self.names[s], el.name, el.start_off))?;
        Ok(Item { key: hash_bytes(0, &text[a..b]), container: false, members: vec![(s, e)] })
    }

    /// The merged children of matched containers.
    fn merge_children(&self, members: &[(usize, usize)]) -> Result<Vec<Item>, String> {
        let mut merged: Vec<Item> = vec![];
        for (n, &(s, e)) in members.iter().enumerate() {
            let list = self.srcs[s].doc.elements()[e].children.iter().map(|&c| self.item(s, c)).collect::<Result<Vec<_>, _>>()?;
            if n == 0 {
                merged = list;
                continue;
            }

            let a = merged.iter().map(|i| i.key).collect::<Vec<_>>();
            let b = list.iter().map(|i| i.key).collect::<Vec<_>>();
            let mut pairs = vec![];
            matches(&a, &b, 0, 0, &mut pairs);
            let mut next = Vec::with_capacity(merged.len() + list.len() - pairs.len());
            let mut old = merged.into_iter().enumerate().peekable();
            let mut new = list.into_iter().enumerate().peekable();
            for (pa, pb) in pairs {
                while old.peek().is_some_and(|(i, _)| *i < pa) {
                    next.push(old.next().expect("peeked").1);
                }

                while new.peek().is_some_and(|(j, _)| *j < pb) {
                    next.push(new.next().expect("peeked").1);
                }

                let mut item = old.next().expect("a pair's item").1;
                item.members.extend(new.next().expect("a pair's item").1.members);
                next.push(item);
            }

            next.extend(old.map(|(_, i)| i));
            next.extend(new.map(|(_, i)| i));
            merged = next;
        }

        Ok(merged)
    }

    fn names_of(&self, set: u64) -> String {
        list_text(&self.names.iter().enumerate().filter(|(s, _)| set & 1 << s != 0).map(|(_, n)| n.clone()).collect::<Vec<_>>())
    }

    /// Lines of a source, `extra` indents deeper.
    fn lines(&mut self, s: usize, from: usize, to: usize, extra: usize) {
        let text = self.srcs[s].text();
        if extra == 0 {
            self.out.extend_from_slice(&text[from..to]);
            return;
        }

        let pad = extra * self.unit;
        let mut at = from;
        while at < to {
            let end = text[at..to].iter().position(|&c| c == b'\n').map_or(to, |i| at + i + 1);
            self.out.extend(std::iter::repeat_n(b' ', pad));
            self.out.extend_from_slice(&text[at..end]);
            at = end;
        }
    }

    fn emit_items(&mut self, items: &[Item], parent: u64, extra: usize) -> Result<(), String> {
        let mut i = 0;
        while i < items.len() {
            let set = items[i].set();
            if set == parent {
                self.emit_item(&items[i], set, extra, false)?;
                i += 1;
                continue;
            }

            let mut j = i + 1;
            while j < items.len() && items[j].set() == set {
                j += 1;
            }

            let run = &items[i..j];
            let taggable = |it: &Item| {
                let (s, e) = it.members[0];
                it.container && TAGGABLE.contains(&self.srcs[s].doc.elements()[e].name.as_str())
            };
            if run.iter().all(taggable) {
                for it in run {
                    self.emit_item(it, set, extra, true)?;
                }
            } else {
                let (s, e) = run[0].members[0];
                let text = self.srcs[s].text();
                let at = line_start(text, self.srcs[s].doc.elements()[e].start_off).expect("checked by item()");
                let indent = indent_len(text, at) + extra * self.unit;
                let names = self.names_of(set);
                let eol = self.eol;
                self.out.extend(std::iter::repeat_n(b' ', indent));
                self.out.extend_from_slice(format!("<{FOLDER} {VARIATIONS_ATTR}=\"{names}\">").as_bytes());
                self.out.extend_from_slice(eol);
                for it in run {
                    self.emit_item(it, set, extra + 1, false)?;
                }

                self.out.extend(std::iter::repeat_n(b' ', indent));
                self.out.extend_from_slice(format!("</{FOLDER}>").as_bytes());
                self.out.extend_from_slice(eol);
            }

            i = j;
        }

        Ok(())
    }

    /// The view state of a matched container's start tag where a variation's differs from the first's: a [`START`] line per distinct
    /// value, one indent in, before the children.
    /// `every`: every attribute may differ (the root's: an older file's `version`), not only the view state.
    fn emit_starts(&mut self, item: &Item, start_line: usize, extra: usize, every: bool) {
        let (s0, e0) = item.members[0];
        let values = |s: usize, e: usize| {
            let doc = &self.srcs[s].doc;
            doc.elements()[e]
                .attrs
                .iter()
                .enumerate()
                .filter(|(_, a)| every || VIEW_STATE.contains(&a.name.as_str()))
                .map(|(i, a)| (a.name.clone(), a.quote, doc.attr_raw(e, i).to_vec()))
                .collect::<Vec<_>>()
        };
        let first = values(s0, e0);
        let mut groups: Vec<(Vec<(String, u8, Vec<u8>)>, u64)> = vec![];
        for &(s, e) in &item.members[1..] {
            let own = values(s, e);
            let differs = own.iter().zip(&first).filter(|(a, b)| a.2 != b.2).map(|(a, _)| a.clone()).collect::<Vec<_>>();
            if differs.is_empty() {
                continue;
            }

            match groups.iter_mut().find(|(d, _)| *d == differs) {
                Some((_, set)) => *set |= 1 << s,
                None => groups.push((differs, 1 << s)),
            }
        }

        let indent = indent_len(self.srcs[s0].text(), start_line) + (extra + 1) * self.unit;
        for (differs, set) in groups {
            let names = self.names_of(set);
            self.out.extend(std::iter::repeat_n(b' ', indent));
            self.out.extend_from_slice(format!("<{START} {VARIATIONS_ATTR}=\"{names}\"").as_bytes());
            for (name, quote, raw) in differs {
                self.out.extend_from_slice(format!(" {name}=").as_bytes());
                self.out.push(quote);
                self.out.extend_from_slice(&raw);
                self.out.push(quote);
            }

            self.out.extend_from_slice(b"/>");
            self.out.extend_from_slice(self.eol);
        }
    }

    fn emit_item(&mut self, item: &Item, set: u64, extra: usize, tag: bool) -> Result<(), String> {
        let (s, e) = item.members[0];
        let el = self.srcs[s].doc.elements()[e].clone();
        if !item.container {
            let (a, b) = whole_lines(self.srcs[s].text(), el.start_off, el.end_off).expect("checked by item()");
            self.lines(s, a, b, extra);
            self.stats.combined += 1;
            if set.count_ones() as usize == self.names.len() {
                self.stats.in_all += 1;
            }

            for &(a, _) in &item.members {
                for &(b, _) in &item.members {
                    self.stats.shared[a][b] += 1;
                }
            }

            for &(m, _) in &item.members {
                self.stats.instances[m] += 1;
            }

            return Ok(());
        }

        let (start, end) = container_lines(&self.srcs[s].doc, self.srcs[s].text(), e).expect("checked by item()");
        if tag {
            self.lines(s, start.0, el.attrs_end, extra);
            let names = self.names_of(set);
            self.out.extend_from_slice(format!(" {VARIATIONS_ATTR}=\"{names}\"").as_bytes());
            self.lines(s, el.attrs_end, start.1, 0);
        } else {
            self.lines(s, start.0, start.1, extra);
        }

        self.emit_starts(item, start.0, extra, false);
        let children = self.merge_children(&item.members)?;
        self.emit_items(&children, set, extra)?;
        self.lines(s, end.0, end.1, extra);
        Ok(())
    }
}

/// Combines the variations' tile.wce texts (each of one variation: no `wce_variations` on its root) into one, in the sources' order.
///
/// # Errors
///
/// No sources, more than 64, a text that does not read, roots or the text around them that differ, an element not on lines of its own.
pub fn merge(sources: &[Source]) -> Result<Combined, String> {
    use rayon::prelude::*;
    if sources.is_empty() || sources.len() > 64 {
        return Err(format!("{} variations: 1 to 64 can be combined", sources.len()));
    }

    let srcs = sources
        .par_iter()
        .map(|s| XmlDoc::parse(s.text.clone()).map(|doc| Src { doc }).map_err(|e| format!("{}: {e}", s.variation)))
        .collect::<Result<Vec<_>, _>>()?;
    let names = sources.iter().map(|s| s.variation.clone()).collect::<Vec<_>>();

    // The roots: the same text around them in every variation, and the same start tag but for the attributes' values (kept per variation
    // as the view state is: an older file's `version`).
    let mut frame = None;
    for (s, src) in srcs.iter().enumerate() {
        let text = src.text();
        let &[root] = src.doc.roots() else { return Err(format!("{}: not one root element", names[s])) };
        if src.doc.get(root, VARIATIONS_ATTR).is_some() {
            return Err(format!("{}: already lists variations; give each variation's own text", names[s]));
        }

        let (start, end) = container_lines(&src.doc, text, root)
            .ok_or_else(|| format!("{}: the root and its children are not on lines of their own", names[s]))?;
        let mut tag = text[start.0..start.1].to_vec();
        for a in src.doc.elements()[root].attrs.iter().rev() {
            tag.drain(a.value_off - start.0..a.value_off - start.0 + a.value_len);
        }

        let parts = (text[..start.0].to_vec(), tag, text[end.0..].to_vec());
        match &frame {
            None => frame = Some(parts),
            Some(f) if *f == parts => {}
            Some(_) => return Err(format!("{}: the root's start tag or the text around it differs from {}'s", names[s], names[0])),
        }
    }

    let src0 = &srcs[0];
    let root0 = src0.doc.roots()[0];
    let (start0, _) = container_lines(&src0.doc, src0.text(), root0).expect("checked above");
    let unit = src0.doc.elements()[root0]
        .children
        .first()
        .and_then(|&c| line_start(src0.text(), src0.doc.elements()[c].start_off))
        .map_or(4, |at| indent_len(src0.text(), at).saturating_sub(indent_len(src0.text(), start0.0)).max(1));
    let eol: &'static [u8] = if src0.text()[..start0.1].ends_with(b"\r\n") { b"\r\n" } else { b"\n" };
    let mut m = Merger {
        srcs: &srcs,
        names: names.clone(),
        unit,
        eol,
        out: Vec::with_capacity(sources.iter().map(|s| s.text.len()).max().unwrap_or(0) * 3 / 2),
        stats: Stats {
            instances: vec![0; srcs.len()],
            shared: vec![vec![0; srcs.len()]; srcs.len()],
            bytes_in: sources.iter().map(|s| s.text.len()).sum(),
            ..Stats::default()
        },
    };

    let (head, _, tail) = frame.expect("one source at least");
    let root = Item { key: 0, container: true, members: (0..srcs.len()).map(|s| (s, srcs[s].doc.roots()[0])).collect() };
    let all = root.set();
    m.out.extend_from_slice(&head);
    let el = src0.doc.elements()[root0].clone();
    m.lines(0, start0.0, el.attrs_end, 0);
    m.out.extend_from_slice(format!(" {VARIATIONS_ATTR}=\"{}\"", list_text(&names)).as_bytes());
    m.lines(0, el.attrs_end, start0.1, 0);
    m.emit_starts(&root, start0.0, 0, true);
    let children = m.merge_children(&root.members)?;
    m.emit_items(&children, all, 0)?;
    m.out.extend_from_slice(&tail);
    m.stats.bytes_out = m.out.len();
    Ok(Combined { text: m.out, stats: m.stats })
}

/// Whether every source comes back byte for byte from the combined text; the first difference of each that does not.
///
/// # Errors
///
/// A line per variation that does not come back: where it first differs, and the two lines there.
pub fn check(combined: &[u8], sources: &[Source]) -> Result<(), String> {
    let mut bad = vec![];
    for s in sources {
        let back = super::to_variation(combined, &s.variation)?;
        if back == s.text {
            continue;
        }

        let at = back.iter().zip(&s.text).position(|(a, b)| a != b).unwrap_or(back.len().min(s.text.len()));
        let line = |t: &[u8]| {
            let a = t[..at.min(t.len())].iter().rposition(|&c| c == b'\n').map_or(0, |i| i + 1);
            let b = t[at.min(t.len())..].iter().position(|&c| c == b'\n').map_or(t.len(), |i| at + i);
            String::from_utf8_lossy(&t[a..b.min(a + 300)]).into_owned()
        };
        bad.push(format!(
            "{}: differs at byte {at} ({} bytes back, {} expected)\n  back:     {}\n  expected: {}",
            s.variation,
            back.len(),
            s.text.len(),
            line(&back),
            line(&s.text)
        ));
    }

    if bad.is_empty() { Ok(()) } else { Err(bad.join("\n")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(children: &str) -> Vec<u8> {
        format!(
            "<?xml version=\"1.0\"?>\r\n<project version=\"16\" wce_version=\"1\">\r\n    <scene>\r\n        <SCENE_GRAPH name=\"root\">\r\n            <children>\r\n{children}            </children>\r\n        </SCENE_GRAPH>\r\n    </scene>\r\n</project>\r\n"
        )
        .into_bytes()
    }

    fn layer(name: &str, children: &str) -> String {
        format!(
            "                <LayerInstance name=\"{name}\">\r\n                    <children>\r\n{children}                    </children>\r\n                </LayerInstance>\r\n"
        )
    }

    fn b(key: &str) -> String {
        format!("                        <BUILDING_INSTANCE key=\"{key}\"/>\r\n")
    }

    #[test]
    fn variations_combine_and_come_back_byte_for_byte() {
        let minor = tile(&(layer("buildings", &(b("house") + &b("hut") + &b("well"))) + &layer("fields", &b("field"))));
        let small = tile(&(layer("buildings", &(b("house") + &b("barn") + &b("well"))) + &layer("walls", &b("wall"))));
        let medium = tile(&(layer("buildings", &(b("house") + &b("barn") + &b("well") + &b("temple"))) + &layer("walls", &b("wall"))));
        let sources = [
            Source { variation: "minor".into(), text: minor },
            Source { variation: "small".into(), text: small },
            Source { variation: "medium".into(), text: medium },
        ];
        let c = merge(&sources).unwrap();
        check(&c.text, &sources).unwrap();
        let text = String::from_utf8(c.text.clone()).unwrap();
        assert!(text.contains("<project version=\"16\" wce_version=\"1\" wce_variations=\"minor,small,medium\">"), "{text}");
        assert!(text.contains("<LayerInstance name=\"walls\" wce_variations=\"small,medium\">"), "a layer is tagged itself: {text}");
        assert!(
            text.contains("                        <WceVariationFolder wce_variations=\"minor\">\r\n                            <BUILDING_INSTANCE key=\"hut\"/>\r\n                        </WceVariationFolder>\r\n"),
            "an instance goes in a folder, one indent deeper: {text}"
        );
        assert_eq!(text.matches("key=\"house\"").count(), 1, "shared once: {text}");
        assert_eq!(text.matches("key=\"barn\"").count(), 1, "small and medium share theirs: {text}");
        assert_eq!(c.stats.instances, [4, 4, 5]);
        assert_eq!((c.stats.combined, c.stats.in_all), (7, 2));
    }

    #[test]
    fn a_layer_shown_in_one_variation_and_hidden_in_another_is_still_shared() {
        let shown = layer("buildings", &(b("house") + &b("hut")))
            .replace("<LayerInstance name", "<LayerInstance flags=\" OIF_VISIBLE|OIF_ACTIVE\" name");
        let hidden = shown.replace(" OIF_VISIBLE|OIF_ACTIVE", " OIF_ACTIVE");
        let sources = [
            Source { variation: "a".into(), text: tile(&shown) },
            Source { variation: "b".into(), text: tile(&hidden) },
            Source { variation: "c".into(), text: tile(&shown.replace("hut", "barn")) },
        ];
        let c = merge(&sources).unwrap();
        check(&c.text, &sources).unwrap();
        let text = String::from_utf8(c.text).unwrap();
        assert_eq!(text.matches("<LayerInstance").count(), 1, "{text}");
        assert!(
            text.contains(
                "                    <WceVariationStart wce_variations=\"b\" flags=\" OIF_ACTIVE\"/>\r\n                    <children>"
            ),
            "{text}"
        );
        assert_eq!((c.stats.combined, c.stats.in_all), (3, 1));
    }

    #[test]
    fn roots_of_other_versions_combine_and_other_prologs_do_not() {
        let a = tile(&layer("x", &b("a")));
        let other = String::from_utf8(a.clone()).unwrap().replace("version=\"16\"", "version=\"15\"").into_bytes();
        let sources = [Source { variation: "a".into(), text: a.clone() }, Source { variation: "b".into(), text: other }];
        let c = merge(&sources).unwrap();
        check(&c.text, &sources).unwrap();
        assert!(String::from_utf8(c.text).unwrap().contains("\r\n    <WceVariationStart wce_variations=\"b\" version=\"15\"/>\r\n"));

        let prolog = String::from_utf8(a.clone()).unwrap().replace("<?xml version=\"1.0\"?>", "<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
        let err = merge(&[Source { variation: "a".into(), text: a }, Source { variation: "b".into(), text: prolog.into_bytes() }]);
        assert!(err.is_err());
    }

    #[test]
    fn long_sequences_without_anchors_still_come_back() {
        // Every key twice: no anchors, past the full LCS's size.
        let many = |n: usize, skip: usize| (0..n).filter(|i| i % skip != 0).map(|i| b(&format!("k{}", i % (n / 2)))).collect::<String>();
        let x = tile(&layer("veg", &many(5000, 7)));
        let y = tile(&layer("veg", &many(5000, 11)));
        let sources = [Source { variation: "x".into(), text: x }, Source { variation: "y".into(), text: y }];
        let c = merge(&sources).unwrap();
        check(&c.text, &sources).unwrap();
        assert!(c.stats.in_all > 3500, "{:?}", c.stats);
    }
}
