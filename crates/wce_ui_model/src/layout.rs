//! Where components and their pictures sit: docking and resizing as the game does it.
//!
//! Ported from the game's own code (`TWUI::Component::Dock`, `Component::Resize`, `ComponentState::Resize` and
//! `ComponentState::DockImages*`, read from the disassembly):
//!
//! * A component with a dock point (the `docking?` value after the tooltip) does not keep its stored offset: when it loads, and whenever
//!   its parent changes size, the game puts it at the dock point of the parent and adds the dock offset. Points 1..9 are the nine places
//!   of a 3x3 grid (top left, top, top right, left, centre, right, bottom left, bottom, bottom right). They place the component's own
//!   matching point on the parent's point (inside). Points 10..18 are the same grid but place the component outside the parent.
//! * `SizeToScreen` resizes the root to the screen: `Resize(w, h, resize_children = false)`. Only a component that allows it (flags 1 and 2)
//!   changes size on that axis; its children keep their sizes, but every docked descendant is docked again.
//! * A state's pictures follow the state's size: a picture that stretches (`stretch x/y`) scales with the state, a docked picture is placed
//!   at its dock point of the state, a picture without a dock point keeps its offset.

use crate::dom::Element;
use crate::draw::DrawOptions;
use crate::model::{Entry, EntryKind, UiModel, leaves, nth_int};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Round half to even, like the `rint` the game calls.
fn rint(v: f32) -> f32 {
    v.round_ties_even()
}

/// Where the dock point puts a component of size `own` inside (or beside) a parent of size `parent`, relative to the parent's corner.
/// `None` when the component is not docked.
pub fn dock_position(dock: i32, parent: (f32, f32), own: (f32, f32)) -> Option<(f32, f32)> {
    if dock <= 0 {
        return None;
    }

    let idx = if (1..=9).contains(&dock) { dock - 1 } else { (dock - 10).clamp(0, 8) };
    let (row, col) = ((idx / 3) as f32, (idx % 3) as f32);
    let outside = dock >= 10 && dock != 14;
    let (pw, ph) = (parent.0 * 0.5, parent.1 * 0.5);
    let (ow, oh) = (own.0 * 0.5, own.1 * 0.5);
    Some(if outside {
        (rint(pw * col + (col - 2.0) * ow), rint(ph * row + (row - 2.0) * oh))
    } else {
        (rint(pw * col - col * ow), rint(ph * row - row * oh))
    })
}

/// The dock point and dock offset of a component element.
pub fn dock_of(m: &UiModel, el: &Element) -> (i32, (i32, i32)) {
    let dock = nth_int(el, "i", 2).unwrap_or(0) as i32;
    if m.version >= 77 {
        (dock, (nth_int(el, "i", 3).unwrap_or(0) as i32, nth_int(el, "i", 4).unwrap_or(0) as i32))
    } else {
        (dock, (0, 0))
    }
}

/// The twelve flags after the offsets: (allow horizontal resize, allow vertical resize).
pub fn resize_flags(el: &Element) -> (bool, bool) {
    let flags: Vec<&Element> = el.elems().filter(|c| matches!(c.name.as_str(), "yes" | "no" | "byte")).take(2).collect();
    (flags.first().is_none_or(|f| f.name != "no"), flags.get(1).is_none_or(|f| f.name != "no"))
}

/// Size of a state: its first two integers.
pub fn state_size(state: &Element) -> (f32, f32) {
    (nth_int(state, "i", 0).unwrap_or(0) as f32, nth_int(state, "i", 1).unwrap_or(0) as f32)
}

/// A picture of a state, placed inside the component (relative to its top left corner).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

struct Use {
    off: (f32, f32),
    size: (f32, f32),
    dock: i32,
    dock_off: (f32, f32),
    stretch: (bool, bool),
}

fn bools(u: &Element) -> Vec<bool> {
    u.elems().filter(|c| c.name == "yes" || c.name == "no").map(|c| c.name == "yes").collect()
}

fn read_use(u: &Element, version: u32) -> Use {
    let ints: Vec<i64> = leaves(u, "i").iter().filter_map(|(_, c)| c.text().trim().parse().ok()).collect();
    let us: Vec<u32> = leaves(u, "u").iter().filter_map(|(_, c)| c.text().trim().parse::<i64>().ok().map(|v| v as u32)).collect();
    let b = bools(u);
    let signed = |i: usize| us.get(i).map(|v| *v as i32 as f32).unwrap_or(0.0);
    Use {
        off: (signed(1), signed(2)),
        size: (signed(3), signed(4)),
        dock: ints.first().copied().unwrap_or(0) as i32,
        dock_off: if version >= 77 { (ints.get(1).copied().unwrap_or(0) as f32, ints.get(2).copied().unwrap_or(0) as f32) } else { (0.0, 0.0) },
        stretch: (b.get(3).copied().unwrap_or(false), b.get(4).copied().unwrap_or(false)),
    }
}

/// The 3x3 cell of a dock point 1..9: (row, column), `None` otherwise.
fn cell(dock: i32) -> Option<(i32, i32)> {
    (1..=9).contains(&dock).then(|| ((dock - 1) / 3, (dock - 1) % 3))
}

/// Places every picture of `state` when the state is `now` big and was designed `design` big.
/// `nine_patch` is the state's flag (the fourth byte of the interactive word): the pictures then make up a 3x3 frame whose edges stretch.
pub fn place_images(uses: &[&Element], version: u32, design: (f32, f32), now: (f32, f32), nine_patch: bool) -> Vec<Placed> {
    let xs = if design.0 != 0.0 { now.0 / design.0 } else { 1.0 };
    let ys = if design.1 != 0.0 { now.1 / design.1 } else { 1.0 };
    let all: Vec<Use> = uses.iter().map(|u| read_use(u, version)).collect();
    let mut out = Vec::with_capacity(all.len());
    for u in all.iter() {
        let w = if u.stretch.0 { u.size.0 * xs } else { u.size.0 };
        let h = if u.stretch.1 { u.size.1 * ys } else { u.size.1 };
        let Some(cl) = cell(u.dock) else {
            // not docked: keeps its offset (a nine patch state also scales the offset)
            out.push(if nine_patch { Placed { x: u.off.0 * xs, y: u.off.1 * ys, w, h } } else { Placed { x: u.off.0, y: u.off.1, w, h } });
            continue;
        };

        let (mut w, mut h) = (w, h);
        let (mut left, mut top) = (0.0f32, 0.0f32);
        if nine_patch && now != design {
            // Corners keep their picture size; an edge picture takes what is left between the corners on its axis.
            let at = |r: i32, c: i32| all.iter().enumerate().find(|(_, v)| cell(v.dock) == Some((r, c)));
            let size_of = |v: &Use| (v.size.0, v.size.1);
            let corner_w = |r: i32| -> f32 {
                let l = at(r, 0).map(|(_, v)| size_of(v).0).unwrap_or(0.0);
                let rt = at(r, 2).map(|(_, v)| size_of(v).0).unwrap_or(0.0);
                l + rt
            };

            let corner_h = |c: i32| -> f32 {
                let t = at(0, c).map(|(_, v)| size_of(v).1).unwrap_or(0.0);
                let b = at(2, c).map(|(_, v)| size_of(v).1).unwrap_or(0.0);
                t + b
            };

            if cl.1 == 1 && u.stretch.0 {
                w = (now.0 - corner_w(cl.0)).max(0.0);
                left = at(cl.0, 0).map(|(_, v)| size_of(v).0).unwrap_or(0.0);
            }

            if cl.0 == 1 && u.stretch.1 {
                h = (now.1 - corner_h(cl.1)).max(0.0);
                top = at(0, cl.1).map(|(_, v)| size_of(v).1).unwrap_or(0.0);
            }
        }

        let x = match cl.1 {
            0 => u.dock_off.0 + left,
            1 => (now.0 - w) * 0.5 + u.dock_off.0,
            _ => now.0 - w + u.dock_off.0,
        };

        let y = match cl.0 {
            0 => u.dock_off.1 + top,
            1 => (now.1 - h) * 0.5 + u.dock_off.1,
            _ => now.1 - h + u.dock_off.1,
        };

        out.push(Placed { x, y, w, h });
    }

    out
}

/// The last two flags of a component: (aspect ratio locked, relative resize). Versions before 85 have neither, before 94 no relative resize.
pub fn resize_modes(version: u32, el: &Element) -> (bool, bool) {
    let b: Vec<bool> = el.elems().filter(|c| c.name == "yes" || c.name == "no").map(|c| c.name == "yes").collect();
    let n = b.len();
    if version >= 94 && n >= 2 {
        (b[n - 2], b[n - 1])
    } else if version >= 85 && n >= 1 {
        (b[n - 1], false)
    } else {
        (false, false)
    }
}

/// The game's `Component::Resize` rule: the size a component takes when it is asked for `want`, from its size `cur`.
/// A component only changes on the axes it allows; a component with a locked aspect ratio needs both and keeps its proportions.
pub fn resized(allow: (bool, bool), aspect_locked: bool, cur: (f32, f32), want: (f32, f32)) -> (f32, f32) {
    if (cur.0 == want.0 || !allow.0) && (cur.1 == want.1 || !allow.1) {
        return cur;
    }

    let mut w = if allow.0 { want.0 } else { cur.0 };
    let mut h = if allow.1 { want.1 } else { cur.1 };
    if aspect_locked {
        if !(allow.0 && allow.1) {
            return cur;
        }

        if cur.1 == want.1 {
            if cur.0 != 0.0 {
                h = cur.1 / cur.0 * w;
            }
        } else if cur.1 != 0.0 {
            w = cur.0 / cur.1 * h;
        }
    }

    (w, h)
}

impl UiModel {
    /// The rectangle of every entry after docking and resizing, for the screen size and animation pose in `o`
    /// (the layout's own size and no animation when `None`). Templates take their parent's rectangle.
    ///
    /// A component that is resized with `resize_children` (an animation with propagate) takes its descendants along as the game does:
    /// they scale with it (or grow by the same amount when they resize relatively) and keep their places in proportion.
    pub fn geometry(&self, entries: &[Entry], o: &DrawOptions) -> Vec<Rect> {
        let mut out: Vec<Rect> = Vec::with_capacity(entries.len());
        // per entry: the factor its size changed by, the size it was designed at, and whether its children follow it
        let mut factor: Vec<(f32, f32)> = Vec::with_capacity(entries.len());
        let mut design: Vec<(f32, f32)> = Vec::with_capacity(entries.len());
        let mut follow: Vec<bool> = Vec::with_capacity(entries.len());
        for en in entries {
            let parent_ix = en.parent;
            let parent = parent_ix.map(|p| out[p]);
            if en.kind == EntryKind::Template {
                out.push(parent.unwrap_or_default());
                factor.push(parent_ix.map(|p| factor[p]).unwrap_or((1.0, 1.0)));
                design.push(parent_ix.map(|p| design[p]).unwrap_or((0.0, 0.0)));
                follow.push(parent_ix.is_some_and(|p| follow[p]));
                continue;
            }

            let Some(el) = self.dom.at(&en.path) else {
                out.push(Rect { x: en.ax as f32, y: en.ay as f32, w: en.w as f32, h: en.h as f32 });
                factor.push((1.0, 1.0));
                design.push((en.w as f32, en.h as f32));
                follow.push(false);
                continue;
            };

            let pose = o.pose.as_ref().and_then(|p| p.by_entry.get(&en.path));
            let dsize = self.chosen_state(el, en, o).map(state_size).unwrap_or((en.w as f32, en.h as f32));
            let allow = resize_flags(el);
            let (locked, relative) = resize_modes(self.version, el);
            let mut size = dsize;
            let mut my_follow = false;

            // asked to follow a parent that was resized with its children
            if let (Some(p), true) = (parent_ix, parent_ix.is_some_and(|p| follow[p])) {
                let (sx, sy) = factor[p];
                let want = if relative { (dsize.0 + (out[p].w - design[p].0), dsize.1 + (out[p].h - design[p].1)) } else { (dsize.0 * sx, dsize.1 * sy) };
                size = resized(allow, locked, dsize, want);
                my_follow = true;
            }

            // an animation sets the size
            if let Some(ps) = pose.and_then(|p| p.size) {
                size = resized(allow, locked, size, ps);
                my_follow = pose.is_some_and(|p| p.propagate);
            }

            // the root sized to the screen (children keep their sizes)
            if parent.is_none() {
                if let Some((sw, sh)) = o.screen {
                    size = resized(allow, locked, size, (sw as f32, sh as f32));
                    my_follow = false;
                }
            }

            let (dock, mut doff) = dock_of(self, el);
            let mut stored = (en.x as f32, en.y as f32);
            if let Some(off) = pose.and_then(|p| p.offset) {
                if dock > 0 {
                    doff = (off.0.round() as i32, off.1.round() as i32);
                } else {
                    stored = off;
                }
            }

            // a parent that was resized with its children also moves what is not docked, in proportion
            if let (Some(p), true) = (parent_ix, parent_ix.is_some_and(|p| follow[p])) {
                if dock == 0 {
                    stored = (stored.0 * factor[p].0, stored.1 * factor[p].1);
                }
            }

            let screen = o.screen.map(|(a, b)| (a as f32, b as f32)).unwrap_or(size);
            let (px, py, psize) = match parent {
                Some(p) => (p.x, p.y, (p.w, p.h)),
                None => (0.0, 0.0, screen),
            };

            let (lx, ly) = match dock_position(dock, psize, size) {
                Some((dx, dy)) => (dx + doff.0 as f32, dy + doff.1 as f32),
                None => stored,
            };

            out.push(Rect { x: px + lx, y: py + ly, w: size.0, h: size.1 });
            factor.push((if dsize.0 != 0.0 { size.0 / dsize.0 } else { 1.0 }, if dsize.1 != 0.0 { size.1 / dsize.1 } else { 1.0 }));
            design.push(dsize);
            follow.push(my_follow);
        }

        out
    }
}
