//! Combining variations into one tile.wce (docs/plans/tile_variations.md, "The scene merge").
//!
//! Each variation's text is a tree of lines (TEd's family of files: one element per line, a fixed indent). The merge goes down from the
//! root. A container's children are a sequence of keys: an instance's key is its whole text, a container's (a layer, a group, the
//! wrappers) its start tag's line. The sequences are matched pairwise (common ends, then keys unique on both sides as anchors, a full
//! LCS for small gaps and a greedy match for big ones) and merged so that each variation's sequence is in the result in its own order.
//! Matched containers are merged in turn. Containers left over are matched by their contents ([`Merger::alike`]: a group moved
//! between variations), and a group that one variation has and another has only the contents of holds them for both
//! ([`GROUP_ATTR`], [`Merger::wraps`]: a level regrouped). A run of children that only some variations have is tagged: layers and
//! groups get `wce_variations` themselves, anything else goes in a [`FOLDER`] at the run's place, one indent deeper.
//!
//! [`super::to_variation`] of the result gives every variation back byte for byte; [`check`] makes sure of it. A poorer match only shares
//! less, it is never wrong.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use wce_formats::xml_doc::XmlDoc;

use super::{
    FOLDER, GROUP_ATTR, START, TAGGABLE, VALUES, VARIATIONS_ATTR, VIEW_STATE, WRAPPABLE, close_tag, indent_len, line_start, list_text,
    whole_lines,
};

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

/// A child in a merged sequence.
struct Item {
    key: u64,
    container: bool,

    /// A container's start tag with every value blanked: what a container matched by its contents ([`Merger::alike`]) must have the
    /// same.
    shape: u64,

    /// The key but for the height of its `position` (`vector(x,_,z)`): TEd puts an element on the landscape on save, so the same
    /// element of variations whose terrain differs differs in that. Matched on it, each variation keeps its value ([`START`] for a
    /// container, [`VALUES`] for an instance).
    near: u64,

    /// Matched by its contents or by `near`: the members' start tags may differ in any value, not only the view state.
    loose: bool,

    /// The sources that have it (a group with `inner`: as a group), by (source, element).
    members: Vec<(usize, usize)>,

    /// A group's contents (inside its `children`), merged before the emission: a group that some variations have and others have
    /// only the contents of ([`GROUP_ATTR`]), or one matched to such a group. `None`: merged when emitted, from the members.
    inner: Option<Vec<Item>>,

    /// The sources it is in: its members, and for one with `inner` the sources of what is in it too.
    all: u64,
}

impl Item {
    fn set(&self) -> u64 {
        self.all
    }

    /// The sources that have it as a group.
    fn group_set(&self) -> u64 {
        self.members.iter().fold(0, |m, &(s, _)| m | 1 << s)
    }
}

fn sets(items: &[Item]) -> u64 {
    items.iter().fold(0, |m, i| m | i.all)
}

/// How items of a merged sequence (`a`) and of a new source's (`b`) go together, by their indices.
enum Join {
    /// One with one; `true`: matched by its contents ([`Merger::alike`]).
    Pair(usize, usize, bool),

    /// The merged sequence's group holds the new source's items `.1 ..= .1` (the new source has no such group).
    WrapsNew(usize, (usize, usize)),

    /// The new source's group holds the merged sequence's items `.0 ..= .0`.
    WrapsOld((usize, usize), usize),
}

impl Join {
    fn a(&self) -> (usize, usize) {
        match *self {
            Self::Pair(a, _, _) | Self::WrapsNew(a, _) => (a, a),
            Self::WrapsOld(a, _) => a,
        }
    }

    fn b(&self) -> (usize, usize) {
        match *self {
            Self::Pair(_, b, _) | Self::WrapsOld(_, b) => (b, b),
            Self::WrapsNew(_, b) => b,
        }
    }

    /// Whether two joins can both be: one wholly before the other in both sequences.
    fn fits(&self, other: &Self) -> bool {
        let (a, b, oa, ob) = (self.a(), self.b(), other.a(), other.b());
        (a.1 < oa.0 && b.1 < ob.0) || (a.0 > oa.1 && b.0 > ob.1)
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

/// An instance's key: its lines, each without as much indent as the first has (the same instance one level deeper or higher is the
/// same key).
fn hash_shifted(tag: u8, lines: &[u8]) -> u64 {
    let indent = indent_len(lines, 0);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    tag.hash(&mut h);
    for line in lines.split_inclusive(|&c| c == b'\n') {
        let skip = line.iter().take(indent).take_while(|&&c| c == b' ' || c == b'\t').count();
        h.write(&line[skip..]);
    }

    h.finish()
}

/// The span of the height in an element's `position="vector(x,y,z)"`, by its offset in the text and length.
fn height_span(doc: &XmlDoc, e: usize) -> Option<(usize, usize)> {
    let i = doc.find_attr(e, "position")?;
    let a = &doc.elements()[e].attrs[i];
    if a.appended {
        return None;
    }

    let raw = doc.attr_raw(e, i);
    let first = raw.iter().position(|&c| c == b',')?;
    let second = first + 1 + raw[first + 1..].iter().position(|&c| c == b',')?;
    raw.starts_with(b"vector(").then_some((a.value_off + first + 1, second - first - 1))
}

/// `bytes` (starting at `from` in the text) without the spans given by text offset, from the last.
fn without(mut bytes: Vec<u8>, from: usize, mut spans: Vec<(usize, usize)>) -> Vec<u8> {
    spans.sort_unstable();
    for (at, len) in spans.into_iter().rev() {
        bytes.drain(at - from..at - from + len);
    }

    bytes
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
/// `w`: the weight of each of `a`'s items (its size): where the order differs, the heaviest anchors are kept.
fn matches(a: &[u64], b: &[u64], w: &[u64], ao: usize, bo: usize, out: &mut Vec<(usize, usize)>) {
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
        middle(ma, mb, &w[p..a.len() - s], mao, mbo, out);
    }

    for k in 0..s {
        out.push((ao + a.len() - s + k, bo + b.len() - s + k));
    }
}

/// [`matches`] of two sequences whose ends differ.
fn middle(a: &[u64], b: &[u64], w: &[u64], ao: usize, bo: usize, out: &mut Vec<(usize, usize)>) {
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
        let anchors = heaviest_increasing(&pairs, |i| w[i]);
        let (mut i, mut j) = (0, 0);
        for (ai, bj) in anchors {
            if ai > i && bj > j {
                matches(&a[i..ai], &b[j..bj], &w[i..ai], ao + i, bo + j, out);
            }

            out.push((ao + ai, bo + bj));
            (i, j) = (ai + 1, bj + 1);
        }

        if i < a.len() && j < b.len() {
            matches(&a[i..], &b[j..], &w[i..], ao + i, bo + j, out);
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

/// The subsequence of pairs (sorted by the first, each first and second once) increasing in the second whose firsts weigh the most.
fn heaviest_increasing(pairs: &[(usize, usize)], weight: impl Fn(usize) -> u64) -> Vec<(usize, usize)> {
    // A Fenwick tree over the seconds' ranks of the best chain ending below each.
    let mut ranks = pairs.iter().map(|p| p.1).collect::<Vec<_>>();
    ranks.sort_unstable();
    let n = ranks.len();
    let mut tree = vec![(0u64, usize::MAX); n + 1];
    let mut best = vec![(0u64, usize::MAX); pairs.len()];
    for (k, &(a, b)) in pairs.iter().enumerate() {
        let r = ranks.partition_point(|&x| x < b);
        let (mut top, mut i) = ((0u64, usize::MAX), r);
        while i > 0 {
            if tree[i].0 > top.0 {
                top = tree[i];
            }

            i &= i - 1;
        }

        best[k] = (top.0 + weight(a).max(1), top.1);
        let mut i = r + 1;
        while i <= n {
            if best[k].0 > tree[i].0 {
                tree[i] = (best[k].0, k);
            }

            i += i.isolate_lowest_one();
        }
    }

    let mut out = vec![];
    let mut k = (0..best.len()).max_by_key(|&k| best[k].0).unwrap_or(usize::MAX);
    while k != usize::MAX {
        out.push(pairs[k]);
        k = best[k].1;
    }

    out.reverse();
    out
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
            let line = text[start.0..start.1].to_vec();
            let view = el
                .attrs
                .iter()
                .filter(|a| VIEW_STATE.contains(&a.name.as_str()) && !a.appended)
                .map(|a| (a.value_off, a.value_len))
                .collect::<Vec<_>>();
            let mut near = view.clone();
            near.extend(height_span(doc, e));
            let values = el.attrs.iter().filter(|a| !a.appended).map(|a| (a.value_off, a.value_len)).collect::<Vec<_>>();

            // Without the indent: the same group one level deeper or higher is the same key.
            let indent = indent_len(text, start.0);
            let key = without(line.clone(), start.0, view);
            let near = without(line.clone(), start.0, near);
            let shape = without(line, start.0, values);
            return Ok(Item {
                key: hash_bytes(1, &key[indent..]),
                container: true,
                shape: hash_bytes(2, &shape[indent..]),
                near: hash_bytes(3, &near[indent..]),
                loose: false,
                members: vec![(s, e)],
                inner: None,
                all: 1 << s,
            });
        }

        let (a, b) = whole_lines(text, el.start_off, el.end_off)
            .ok_or_else(|| format!("{}: <{}> at offset {} is not on lines of its own", self.names[s], el.name, el.start_off))?;
        let near = without(text[a..b].to_vec(), a, height_span(doc, e).into_iter().collect());
        Ok(Item {
            key: hash_shifted(0, &text[a..b]),
            container: false,
            shape: 0,
            near: hash_shifted(4, &near),
            loose: false,
            members: vec![(s, e)],
            inner: None,
            all: 1 << s,
        })
    }

    /// An item's weight in a match: its first member's bytes.
    fn size(&self, it: &Item) -> u64 {
        let (s, e) = it.members[0];
        let el = &self.srcs[s].doc.elements()[e];
        (el.end_off - el.start_off) as u64
    }

    /// The items of an element's children.
    fn items_of(&self, s: usize, e: usize) -> Result<Vec<Item>, String> {
        self.srcs[s].doc.elements()[e].children.iter().map(|&c| self.item(s, c)).collect()
    }

    /// The `children` element of a group that can hold what variations without the group have ([`WRAPPABLE`]): its only child, a
    /// `<children>` on lines of its own.
    fn wrapper(&self, s: usize, e: usize) -> Option<usize> {
        let (doc, text) = (&self.srcs[s].doc, self.srcs[s].text());
        let el = &doc.elements()[e];
        if !WRAPPABLE.contains(&el.name.as_str()) || el.self_closing {
            return None;
        }

        let &[c] = el.children.as_slice() else { return None };
        let ch = &doc.elements()[c];
        (ch.name == "children"
            && !ch.self_closing
            && ch.attrs.is_empty()
            && container_lines(doc, text, e).is_some()
            && container_lines(doc, text, c).is_some())
        .then_some(c)
    }

    /// Whether every member of an item is a group that can hold others' contents.
    fn wrappable(&self, it: &Item) -> bool {
        it.inner.is_some() || (it.container && it.members.iter().all(|&(s, e)| self.wrapper(s, e).is_some()))
    }

    /// Merges a group's contents ahead ([`Item::inner`]).
    fn make_inner(&self, it: &mut Item) -> Result<(), String> {
        if it.inner.is_some() {
            return Ok(());
        }

        let wrappers = it.members.iter().map(|&(s, e)| (s, self.wrapper(s, e).expect("checked by wrappable()"))).collect::<Vec<_>>();
        let inner = self.merge_children(&wrappers)?;
        it.all |= sets(&inner);
        it.inner = Some(inner);
        Ok(())
    }

    /// The keys of what a container holds: inside its `children` when that is its only child.
    fn inner_keys(&self, it: &Item) -> Result<Vec<u64>, String> {
        self.inner_items(it, |i| i.key)
    }

    /// [`Self::inner_keys`], of any of the items' keys.
    fn inner_items(&self, it: &Item, key: fn(&Item) -> u64) -> Result<Vec<u64>, String> {
        if let Some(inner) = &it.inner {
            return Ok(inner.iter().map(key).collect());
        }

        let (s, e) = it.members[0];
        let doc = &self.srcs[s].doc;
        let e = match doc.elements()[e].children.as_slice() {
            &[c] if doc.elements()[c].name == "children" => c,
            _ => e,
        };
        doc.elements()[e].children.iter().map(|&c| self.item(s, c).map(|i| key(&i))).collect()
    }

    /// The merged children of matched containers.
    fn merge_children(&self, members: &[(usize, usize)]) -> Result<Vec<Item>, String> {
        let mut merged = vec![];
        for (n, &(s, e)) in members.iter().enumerate() {
            let list = self.items_of(s, e)?;
            merged = if n == 0 { list } else { self.merge_lists(merged, list)? };
        }

        Ok(merged)
    }

    /// A new source's items (`b`, each of that source alone) merged into a merged sequence, each sequence's order kept.
    fn merge_lists(&self, a: Vec<Item>, b: Vec<Item>) -> Result<Vec<Item>, String> {
        let ka = a.iter().map(|i| i.key).collect::<Vec<_>>();
        let kb = b.iter().map(|i| i.key).collect::<Vec<_>>();
        let mut pairs = vec![];
        let wa = a.iter().map(|i| self.size(i)).collect::<Vec<_>>();
        matches(&ka, &kb, &wa, 0, 0, &mut pairs);

        // A group whose contents were merged ahead goes only with one that can hold them too.
        let fits = |i: usize, j: usize| a[i].inner.is_none() || self.wrappable(&b[j]);
        pairs.retain(|&(i, j)| fits(i, j));

        // Between those, the same but for a height.
        let na = a.iter().map(|i| i.near).collect::<Vec<_>>();
        let nb = b.iter().map(|i| i.near).collect::<Vec<_>>();
        let mut near = vec![];
        let mut prev = (0, 0);
        for &(pa, pb) in pairs.iter().chain(std::iter::once(&(a.len(), b.len()))) {
            if pa > prev.0 && pb > prev.1 {
                matches(&na[prev.0..pa], &nb[prev.1..pb], &wa[prev.0..pa], prev.0, prev.1, &mut near);
            }

            prev = (pa + 1, pb + 1);
        }

        near.retain(|&(i, j)| a[i].container == b[j].container && fits(i, j));
        let mut loose = near;
        let mut sorted = pairs.clone();
        sorted.extend(&loose);
        sorted.sort_unstable();
        loose.extend(self.alike(&a, &b, &sorted)?);
        let mut joins = pairs.iter().map(|&(i, j)| Join::Pair(i, j, false)).collect::<Vec<_>>();
        joins.extend(loose.into_iter().filter(|&(i, j)| fits(i, j)).map(|(i, j)| Join::Pair(i, j, true)));
        joins.sort_unstable_by_key(Join::a);
        let wraps = self.wraps(&a, &b, &joins)?;
        joins.extend(wraps);
        joins.sort_unstable_by_key(Join::a);

        let mut a = a.into_iter().map(Some).collect::<Vec<_>>();
        let mut b = b.into_iter().map(Some).collect::<Vec<_>>();
        let mut out = Vec::with_capacity(a.len() + b.len());
        let (mut i, mut j) = (0, 0);
        for join in joins {
            let ((alo, ahi), (blo, bhi)) = (join.a(), join.b());
            out.extend(a[i..alo].iter_mut().filter_map(Option::take));
            out.extend(b[j..blo].iter_mut().filter_map(Option::take));
            let item = match join {
                Join::Pair(x, y, loose) => {
                    let mut it = a[x].take().expect("a join's item");
                    let other = b[y].take().expect("a join's item");
                    if let Some(inner) = it.inner.take() {
                        let (s, e) = other.members[0];
                        let theirs = self.items_of(s, self.wrapper(s, e).expect("checked by fits"))?;
                        let inner = self.merge_lists(inner, theirs)?;
                        it.all |= sets(&inner);
                        it.inner = Some(inner);
                    }

                    it.all |= other.all;
                    it.members.extend(other.members);
                    it.loose |= loose;
                    it
                }
                Join::WrapsNew(x, _) => {
                    let mut it = a[x].take().expect("a join's item");
                    self.make_inner(&mut it)?;
                    let held = b[blo..=bhi].iter_mut().filter_map(Option::take).collect::<Vec<_>>();
                    let inner = self.merge_lists(it.inner.take().expect("made"), held)?;
                    it.all |= sets(&inner);
                    it.inner = Some(inner);
                    it
                }
                Join::WrapsOld(_, y) => {
                    let mut it = b[y].take().expect("a join's item");
                    let (s, e) = it.members[0];
                    let theirs = self.items_of(s, self.wrapper(s, e).expect("checked by wraps()"))?;
                    let held = a[alo..=ahi].iter_mut().filter_map(Option::take).collect::<Vec<_>>();
                    let inner = self.merge_lists(held, theirs)?;
                    it.all |= sets(&inner);
                    it.inner = Some(inner);
                    it
                }
            };
            out.push(item);
            (i, j) = (ahi + 1, bhi + 1);
        }

        out.extend(a[i..].iter_mut().filter_map(Option::take));
        out.extend(b[j..].iter_mut().filter_map(Option::take));
        Ok(out)
    }

    /// Containers left unmatched between two matches, paired by their contents: the same tag and attributes (their values aside) and
    /// at least half of the smaller one's children the same (but for their heights). A group whose pivot moved between variations (its `position`,
    /// `orientation`, `surface_mode`) but holds the same buildings is then one group, its values kept per variation ([`START`]).
    /// Returns pairs increasing in both, none of them in `pairs`.
    fn alike(&self, a: &[Item], b: &[Item], pairs: &[(usize, usize)]) -> Result<Vec<(usize, usize)>, String> {
        let mut out = vec![];
        let mut prev = (0, 0);
        for &(pa, pb) in pairs.iter().chain(std::iter::once(&(a.len(), b.len()))) {
            let ga = (prev.0..pa).filter(|&i| a[i].container).collect::<Vec<_>>();
            let gb = (prev.1..pb).filter(|&j| b[j].container).collect::<Vec<_>>();
            prev = (pa + 1, pb + 1);
            if ga.is_empty() || gb.is_empty() {
                continue;
            }

            let keys = |it: &Item| -> Result<Vec<u64>, String> {
                let mut k = self.inner_items(it, |i| i.near)?;
                k.sort_unstable();
                k.dedup();
                Ok(k)
            };

            let mut owners = HashMap::<u64, Vec<usize>>::new();
            let mut sizes = HashMap::<usize, usize>::new();
            for &j in &gb {
                let k = keys(&b[j])?;
                sizes.insert(j, k.len());
                for key in k {
                    owners.entry(key).or_default().push(j);
                }
            }

            let mut found = vec![];
            for &i in &ga {
                let k = keys(&a[i])?;
                let mut counts = HashMap::<usize, usize>::new();
                for key in &k {
                    if let Some(list) = owners.get(key).filter(|l| l.len() <= 8) {
                        for &j in list {
                            *counts.entry(j).or_default() += 1;
                        }
                    }
                }

                let best = counts
                    .into_iter()
                    .filter(|&(j, n)| b[j].shape == a[i].shape && 2 * n >= k.len().min(sizes[&j]))
                    .max_by_key(|&(j, n)| (n, usize::MAX - j));
                if let Some((j, _)) = best {
                    found.push((i, j));
                }
            }

            found.sort_unstable();
            out.extend(longest_increasing(&found));
        }

        Ok(out)
    }

    /// Groups that one side has and the other has only the contents of, between the joins so far: a group of either side whose
    /// contents are among the other side's items there holds them ([`Join::WrapsNew`], [`Join::WrapsOld`]). The same buildings in
    /// other groups of each variation (a level regrouped) are then kept once. The best first, as long as they fit with each other.
    fn wraps(&self, a: &[Item], b: &[Item], joins: &[Join]) -> Result<Vec<Join>, String> {
        // The run of positions the most of `pos` are in, no more than this apart.
        const SPREAD: usize = 16;
        let cluster = |mut pos: Vec<usize>| -> Option<(usize, usize, usize)> {
            pos.sort_unstable();
            pos.dedup();
            let mut best: Option<(usize, usize, usize)> = None;
            let mut lo = 0;
            for k in 0..pos.len() {
                if k > 0 && pos[k] - pos[k - 1] > SPREAD {
                    lo = k;
                }

                let n = k - lo + 1;
                if best.is_none_or(|(_, _, m)| n > m) {
                    best = Some((pos[lo], pos[k], n));
                }
            }

            best
        };

        let mut out = vec![];
        let mut prev = (0, 0);
        let ends = joins.iter().map(|j| (j.a().0, j.b().0)).chain(std::iter::once((a.len(), b.len())));
        let after = joins.iter().map(|j| (j.a().1 + 1, j.b().1 + 1));
        for ((na, nb), next_prev) in ends.zip(after.map(Some).chain(std::iter::once(None))) {
            let (ga, gb) = (prev.0..na, prev.1..nb);
            if let Some(p) = next_prev {
                prev = p;
            }

            if ga.is_empty() || gb.is_empty() {
                continue;
            }

            let index = |items: &[Item], range: std::ops::Range<usize>| {
                let mut at = HashMap::<u64, Vec<usize>>::new();
                for k in range {
                    at.entry(items[k].key).or_default().push(k);
                }

                at
            };
            let held = |keys: Vec<u64>, at: &HashMap<u64, Vec<usize>>| {
                cluster(keys.iter().filter_map(|k| at.get(k).filter(|l| l.len() <= 8)).flatten().copied().collect())
            };

            let mut found = vec![];
            let in_b = index(b, gb.clone());
            for i in ga.clone() {
                if self.wrappable(&a[i])
                    && let Some((lo, hi, n)) = held(self.inner_keys(&a[i])?, &in_b)
                {
                    found.push((n, Join::WrapsNew(i, (lo, hi))));
                }
            }

            let in_a = index(a, ga);
            for j in gb {
                if self.wrappable(&b[j])
                    && let Some((lo, hi, n)) = held(self.inner_keys(&b[j])?, &in_a)
                {
                    found.push((n, Join::WrapsOld((lo, hi), j)));
                }
            }

            found.sort_by_key(|(n, j)| (usize::MAX - n, j.a(), j.b()));
            let mut taken: Vec<Join> = vec![];
            for (_, join) in found {
                if taken.iter().all(|t| t.fits(&join)) {
                    taken.push(join);
                }
            }

            out.extend(taken);
        }

        Ok(out)
    }

    fn names_of(&self, set: u64) -> String {
        list_text(&self.names.iter().enumerate().filter(|(s, _)| set & 1 << s != 0).map(|(_, n)| n.clone()).collect::<Vec<_>>())
    }

    /// The spaces to put before an element's lines so that they start at `indent`.
    fn pad(&self, s: usize, e: usize, indent: usize) -> Result<usize, String> {
        let text = self.srcs[s].text();
        let own = line_start(text, self.srcs[s].doc.elements()[e].start_off).map_or(0, |at| indent_len(text, at));
        indent.checked_sub(own).ok_or_else(|| format!("{}: an element at offset {own} would go less deep than it is", self.names[s]))
    }

    /// Lines of a source, `pad` spaces more at the start of each.
    fn lines(&mut self, s: usize, from: usize, to: usize, pad: usize) {
        let text = self.srcs[s].text();
        if pad == 0 {
            self.out.extend_from_slice(&text[from..to]);
            return;
        }

        let mut at = from;
        while at < to {
            let end = text[at..to].iter().position(|&c| c == b'\n').map_or(to, |i| at + i + 1);
            self.out.extend(std::iter::repeat_n(b' ', pad));
            self.out.extend_from_slice(&text[at..end]);
            at = end;
        }
    }

    /// Items of a sequence whose parent is in the sources `parent`, at `indent`.
    fn emit_items(&mut self, items: &[Item], parent: u64, indent: usize) -> Result<(), String> {
        let mut i = 0;
        while i < items.len() {
            let set = items[i].set();
            if set == parent {
                self.emit_item(&items[i], set, indent, false)?;
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
                    self.emit_item(it, set, indent, true)?;
                }
            } else {
                let names = self.names_of(set);
                let eol = self.eol;
                self.out.extend(std::iter::repeat_n(b' ', indent));
                self.out.extend_from_slice(format!("<{FOLDER} {VARIATIONS_ATTR}=\"{names}\">").as_bytes());
                self.out.extend_from_slice(eol);
                for it in run {
                    self.emit_item(it, set, indent + self.unit, false)?;
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
    /// value at `indent`, before the children.
    /// `every`: every attribute may differ (the root's: an older file's `version`; a group matched by its contents), not only the view
    /// state.
    fn emit_starts(&mut self, item: &Item, indent: usize, every: bool, name: &str) {
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

        for (differs, set) in groups {
            let names = self.names_of(set);
            self.out.extend(std::iter::repeat_n(b' ', indent));
            self.out.extend_from_slice(format!("<{name} {VARIATIONS_ATTR}=\"{names}\"").as_bytes());
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

    fn emit_item(&mut self, item: &Item, set: u64, indent: usize, tag: bool) -> Result<(), String> {
        let (s, e) = item.members[0];
        let el = self.srcs[s].doc.elements()[e].clone();
        let pad = self.pad(s, e, indent)?;
        if !item.container {
            if item.loose {
                self.emit_starts(item, indent, true, VALUES);
            }

            let (a, b) = whole_lines(self.srcs[s].text(), el.start_off, el.end_off).expect("checked by item()");
            self.lines(s, a, b, pad);
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
        let group = item.group_set();
        let wraps = item.inner.is_some() && group != item.all;
        if tag || wraps {
            self.lines(s, start.0, el.attrs_end, pad);
            if tag {
                let names = self.names_of(set);
                self.out.extend_from_slice(format!(" {VARIATIONS_ATTR}=\"{names}\"").as_bytes());
            }

            if wraps {
                let names = self.names_of(group);
                self.out.extend_from_slice(format!(" {GROUP_ATTR}=\"{names}\"").as_bytes());
            }

            self.lines(s, el.attrs_end, start.1, 0);
        } else {
            self.lines(s, start.0, start.1, pad);
        }

        self.emit_starts(item, indent + self.unit, item.loose, START);
        if let Some(inner) = &item.inner {
            let c = self.wrapper(s, e).expect("checked by wrappable()");
            let (cs, ce) = container_lines(&self.srcs[s].doc, self.srcs[s].text(), c).expect("checked by wrapper()");
            self.lines(s, cs.0, cs.1, pad);
            self.emit_items(inner, item.all, indent + 2 * self.unit)?;
            self.lines(s, ce.0, ce.1, pad);
        } else {
            let children = self.merge_children(&item.members)?;
            self.emit_items(&children, set, indent + self.unit)?;
        }

        self.lines(s, end.0, end.1, pad);
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
    let all = (0..srcs.len()).fold(0, |m, s| m | 1 << s);
    let root = Item {
        key: 0,
        container: true,
        shape: 0,
        near: 0,
        loose: false,
        members: (0..srcs.len()).map(|s| (s, srcs[s].doc.roots()[0])).collect(),
        inner: None,
        all,
    };
    m.out.extend_from_slice(&head);
    let el = src0.doc.elements()[root0].clone();
    m.lines(0, start0.0, el.attrs_end, 0);
    m.out.extend_from_slice(format!(" {VARIATIONS_ATTR}=\"{}\"", list_text(&names)).as_bytes());
    m.lines(0, el.attrs_end, start0.1, 0);
    let inside = indent_len(src0.text(), start0.0) + unit;
    m.emit_starts(&root, inside, true, START);
    let children = m.merge_children(&root.members)?;
    m.emit_items(&children, all, inside)?;
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

    fn group(pos: &str, children: &str) -> String {
        // One level deeper than a layer's children: what `b` gives is re-indented.
        let inner = children.lines().map(|l| format!("        {l}\r\n")).collect::<String>();
        format!(
            "                        <UserInstanceGroup position=\"{pos}\">\r\n                            <children>\r\n{inner}                            </children>\r\n                        </UserInstanceGroup>\r\n"
        )
    }

    #[test]
    fn regrouped_contents_are_kept_once() {
        // minor groups the houses and the barns; small has the houses loose and the barns in a group with a moved pivot; medium groups
        // the houses elsewhere and has the barns loose.
        let minor =
            tile(&layer("buildings", &(group("1", &(b("house") + &b("hut"))) + &b("well") + &group("5", &(b("barn") + &b("shed"))))));
        let small = tile(&layer("buildings", &(b("house") + &b("hut") + &b("well") + &group("7", &(b("barn") + &b("shed"))))));
        let medium = tile(&layer("buildings", &(group("2", &(b("house") + &b("hut"))) + &b("well") + &b("barn") + &b("shed"))));
        let sources = [
            Source { variation: "minor".into(), text: minor },
            Source { variation: "small".into(), text: small },
            Source { variation: "medium".into(), text: medium },
        ];
        let c = merge(&sources).unwrap();
        let text = String::from_utf8(c.text.clone()).unwrap();
        check(&c.text, &sources).unwrap_or_else(|e| panic!("{e}\n{text}"));
        for key in ["house", "hut", "well", "barn", "shed"] {
            assert_eq!(text.matches(&format!("key=\"{key}\"")).count(), 1, "{key} once: {text}");
        }

        assert!(text.contains(&format!("position=\"1\" {GROUP_ATTR}=\"minor,medium\"")), "{text}");
        assert!(text.contains(&format!("position=\"5\" {GROUP_ATTR}=\"minor,small\"")), "{text}");
        assert_eq!(c.stats.in_all, 5, "{text}");
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
