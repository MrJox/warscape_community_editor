//! Component animations: what a layout's `funcs` do over time, and the pose they give the components at any moment.
//!
//! Ported from the game's `TWUI::ComponentAnimation`, `ComponentAnimationFrame` and the easing curves (read from the disassembly):
//!
//! * A component has named animations (`funcs`). An animation is a list of frames that play one after the other.
//! * Each frame goes from where the last one ended (the first from the component as it is) to its own target in `time` milliseconds,
//!   along an easing curve. Only the properties in the frame's mask move: 1 colour, 2 position, 4 size, 8 shader values, 0x10 rotation,
//!   0x20 picture cross-fade, 0x40 opacity, 0x80 text colour.
//! * A relative position frame moves the component by its offset from where it was; an absolute one moves it to the offset.
//! * When a frame ends, its triggers fire: they set a state of a component, start an animation of a component, or set a property.
//! * With `propagate` the colour, opacity and size reach the component's descendants.
//!
//! Not drawn: rotation, picture cross-fade and shader values (the canvas has no rotated or shaded pictures), and properties.

use std::collections::HashMap;

use crate::dom::Element;
use crate::model::{Entry, EntryKind, UiModel, by_label, color_of};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Curve {
    Linear,
    QuadIn,
    QuadOut,
    QuadInOut,
    CubicIn,
    CubicOut,
    CubicInOut,
    ExpIn,
    ExpOut,
    ExpInOut,
    CircIn,
    CircOut,
    CircInOut,
}

impl Curve {
    /// The label the layout stores.
    pub fn from_label(s: &str) -> Curve {
        match s {
            "Quadratic In" => Curve::QuadIn,
            "Quadratic Out" => Curve::QuadOut,
            "Quadratic In & Out" => Curve::QuadInOut,
            "Cubic In" => Curve::CubicIn,
            "Cubic Out" => Curve::CubicOut,
            "Cubic In & Out" => Curve::CubicInOut,
            "Exponential In" => Curve::ExpIn,
            "Exponential Out" => Curve::ExpOut,
            "Exponential In & Out" => Curve::ExpInOut,
            "Circular In" => Curve::CircIn,
            "Circular Out" => Curve::CircOut,
            "Circular In & Out" => Curve::CircInOut,
            _ => Curve::Linear,
        }
    }

    /// The game's `GetProgress(time, duration, change)`: how far along, 0..1 times `w` (the frame's easing weight; linear ignores it).
    pub fn progress(self, time: f32, duration: f32, w: f32) -> f32 {
        let t = time / duration;
        match self {
            Curve::Linear => t,
            Curve::QuadIn => t * t * w,
            Curve::QuadOut => -((t - 2.0) * t * w),
            Curve::QuadInOut => {
                let h = time / (duration * 0.5);
                if h >= 1.0 { (w * -0.5) * (((h - 1.0) - 2.0) * (h - 1.0) - 1.0) } else { (w * 0.5) * h * h }
            }
            Curve::CubicIn => t * t * t * w,
            Curve::CubicOut => ((t - 1.0) * (t - 1.0) * (t - 1.0) + 1.0) * w,
            Curve::CubicInOut => {
                let h = time / (duration * 0.5);
                if h >= 1.0 { (w * 0.5) * ((h - 2.0) * (h - 2.0) * (h - 2.0) + 2.0) } else { (w * 0.5) * h * h * h }
            }
            Curve::ExpIn => 2f32.powf((t - 1.0) * 10.0) * w,
            Curve::ExpOut => 1.0 - 2f32.powf(t * -10.0) * w,
            Curve::ExpInOut => {
                let x = time / (duration * 0.5) - 1.0;
                let v = if x >= 0.0 { 2.0 - 2f32.powf(x * -10.0) } else { 2f32.powf(x * 10.0) };
                (w * 0.5) * v
            }
            Curve::CircIn => -w * ((1.0 - t * t).max(0.0).sqrt() - 1.0),
            Curve::CircOut => (1.0 - (t - 1.0) * (t - 1.0)).max(0.0).sqrt() * w,
            Curve::CircInOut => {
                let h = time / (duration * 0.5);
                if h >= 1.0 {
                    (w * 0.5) * ((1.0 - (h - 2.0) * (h - 2.0)).max(0.0).sqrt() + 1.0)
                } else {
                    (w * -0.5) * ((1.0 - h * h).max(0.0).sqrt() - 1.0)
                }
            }
        }
    }
}

/// Something that happens when a frame ends.
#[derive(Debug, Clone, PartialEq)]
pub struct Trigger {
    /// The component it happens to (its id in the layout).
    pub component: u32,
    pub animation: String,
    pub state: String,
    pub property: String,
}

pub const MASK_COLOUR: u32 = 1;
pub const MASK_POSITION: u32 = 2;
pub const MASK_SIZE: u32 = 4;
pub const MASK_SHADER: u32 = 8;
pub const MASK_ROTATION: u32 = 0x10;
pub const MASK_CROSSFADE: u32 = 0x20;
pub const MASK_OPACITY: u32 = 0x40;
pub const MASK_TEXT_COLOUR: u32 = 0x80;

#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub offset: (f32, f32),
    pub size: (i32, i32),
    /// Target colour, RGBA.
    pub color: [u8; 4],
    pub rotation: f32,
    /// Milliseconds.
    pub time: u32,
    pub mask: u32,
    pub weight: f32,
    pub curve: Curve,
    /// The position is where to go, not how far to move.
    pub absolute: bool,
    pub triggers: Vec<Trigger>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Anim {
    pub name: String,
    pub propagate: bool,
    pub frames: Vec<Frame>,
}

impl Anim {
    /// Milliseconds the animation runs for.
    pub fn duration(&self) -> u32 {
        self.frames.iter().map(|f| f.time).sum()
    }
}

fn leaf_after(el: &Element, label: &str) -> Option<String> {
    by_label(el, label).and_then(|i| el.elem_at(i)).map(|c| c.text())
}

fn num(el: &Element, label: &str) -> f32 {
    leaf_after(el, label).and_then(|t| t.trim().parse().ok()).unwrap_or(0.0)
}

fn parse_frame(a: &Element) -> Frame {
    let triggers = a
        .find("anim_attrs")
        .map(|attrs| {
            attrs
                .elems()
                .filter(|x| x.name == "anim_attr")
                .map(|x| {
                    let strs: Vec<String> = x.elems().filter(|c| c.name == "s").map(|c| c.text()).collect();
                    Trigger {
                        component: x.elems().find(|c| c.name == "u").and_then(|c| c.text().trim().parse().ok()).unwrap_or(0),
                        animation: strs.first().cloned().unwrap_or_default(),
                        state: strs.get(1).cloned().unwrap_or_default(),
                        property: strs.get(2).cloned().unwrap_or_default(),
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let absolute = by_label(a, "is movement absolute").and_then(|i| a.elem_at(i)).is_some_and(|c| c.name == "yes");
    Frame {
        offset: (num(a, "x offset"), num(a, "y offset")),
        size: (num(a, "x size") as i32, num(a, "y size") as i32),
        color: a.find("color").map(color_of).unwrap_or([255; 4]),
        rotation: num(a, "rotation angle"),
        time: num(a, "interpolation time").max(0.0) as u32,
        mask: leaf_after(a, "interpolation property mask").and_then(|t| t.trim().parse::<i64>().ok()).unwrap_or(0) as u32,
        weight: {
            let w = num(a, "easing weight");
            if w == 0.0 { 1.0 } else { w }
        },
        curve: Curve::from_label(&leaf_after(a, "easing curve").unwrap_or_default()),
        absolute,
        triggers,
    }
}

/// How a component is posed at some moment of an animation.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EntryPose {
    /// Replaces the component's offset in its parent (its dock offset when it is docked).
    pub offset: Option<(f32, f32)>,
    pub size: Option<(f32, f32)>,
    /// 0..255: the most opaque any of its pictures and its text can be.
    pub opacity: Option<f32>,
    pub rgb: Option<[u8; 3]>,
    pub state: Option<String>,
    /// Colour, opacity and size reach the descendants.
    pub propagate: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pose {
    pub by_entry: HashMap<Vec<usize>, EntryPose>,
}

/// An animation that has been started: where, which, and when (milliseconds from the start of the preview).
struct Instance {
    entry: usize,
    anim: Anim,
    start: u32,
}

fn lerp(a: f32, b: f32, p: f32) -> f32 {
    a + (b - a) * p
}

impl UiModel {
    /// The animations of one component.
    pub fn animations(&self, path: &[usize]) -> Vec<Anim> {
        let Some(el) = self.dom.at(path) else { return vec![] };
        let Some(funcs) = el.find("funcs") else { return vec![] };
        funcs
            .elems()
            .filter(|f| f.name == "func")
            .map(|f| {
                let name = f.elems().find(|c| c.name == "s").map(|c| c.text()).unwrap_or_default();
                let propagate = f.elems().find(|c| c.name == "yes" || c.name == "no").is_some_and(|c| c.name == "yes");
                let frames = f.find("anims").map(|a| a.elems().filter(|x| x.name == "anim").map(parse_frame).collect()).unwrap_or_default();
                Anim { name, propagate, frames }
            })
            .collect()
    }

    /// Every (component number, animation) in the layout, for listing.
    pub fn all_animations(&self, entries: &[Entry]) -> Vec<(usize, Anim)> {
        let mut out = Vec::new();
        for (i, en) in entries.iter().enumerate() {
            if en.kind == EntryKind::Entry {
                for a in self.animations(&en.path) {
                    out.push((i, a));
                }
            }
        }

        out
    }

    /// Every animation that runs when `animation` of component `entry` starts (it, and what its frames trigger), and the states set on the way
    /// with the time they are set.
    fn instances(&self, entries: &[Entry], entry: usize, animation: &str) -> (Vec<Instance>, Vec<(u32, usize, String)>) {
        let mut started: Vec<Instance> = Vec::new();
        let mut queue: Vec<(usize, String, u32, u32)> = vec![(entry, animation.to_string(), 0, 0)];
        let by_id: HashMap<u32, usize> = entries.iter().enumerate().filter(|(_, e)| e.kind == EntryKind::Entry && e.id != 0).map(|(i, e)| (e.id, i)).collect();
        let mut states: Vec<(u32, usize, String)> = Vec::new();
        while let Some((en, name, start, depth)) = queue.pop() {
            if started.len() >= 64 || depth > 8 || en >= entries.len() {
                continue;
            }

            let Some(anim) = self.animations(&entries[en].path).into_iter().find(|a| a.name == name) else { continue };
            let mut end = start;
            for f in &anim.frames {
                end += f.time;
                for tr in &f.triggers {
                    let Some(&target) = by_id.get(&tr.component) else { continue };
                    if !tr.state.is_empty() {
                        states.push((end, target, tr.state.clone()));
                    }

                    if !tr.animation.is_empty() {
                        queue.push((target, tr.animation.clone(), end, depth + 1));
                    }
                }
            }

            started.push(Instance { entry: en, anim, start });
        }

        started.sort_by_key(|i| i.start);
        states.sort_by_key(|s| s.0);
        (started, states)
    }

    /// Milliseconds until the animation and everything it triggers has finished.
    pub fn animation_length(&self, entries: &[Entry], entry: usize, animation: &str) -> u32 {
        let (started, states) = self.instances(entries, entry, animation);
        let a = started.iter().map(|i| i.start + i.anim.duration()).max().unwrap_or(0);
        a.max(states.iter().map(|s| s.0).max().unwrap_or(0))
    }

    /// The pose of the layout `t` milliseconds after `animation` of component `entry` starts, with the animations its frames trigger.
    pub fn pose_at(&self, entries: &[Entry], entry: usize, animation: &str, t: u32) -> Pose {
        let mut pose = Pose::default();
        let (started, states) = self.instances(entries, entry, animation);
        for inst in &started {
            if t < inst.start {
                continue;
            }

            self.apply_instance(entries, inst, t - inst.start, &mut pose);
        }

        for (when, en, state) in states {
            if t >= when {
                let p = pose.by_entry.entry(entries[en].path.clone()).or_default();
                p.state = Some(state);
            }
        }

        pose
    }

    fn apply_instance(&self, entries: &[Entry], inst: &Instance, lt: u32, pose: &mut Pose) {
        let en = &entries[inst.entry];
        let Some(el) = self.dom.at(&en.path) else { return };
        let (dock, doff) = crate::layout::dock_of(self, el);
        let base = if dock > 0 { (doff.0 as f32, doff.1 as f32) } else { (en.x as f32, en.y as f32) };
        let cur_state = self.default_state(el);
        let size0 = cur_state.map(crate::layout::state_size).unwrap_or((en.w as f32, en.h as f32));
        let colour0 = cur_state.and_then(|s| s.find("image_uses")).and_then(|u| u.elems().find(|x| x.name == "image_use")).and_then(|u| u.find("color")).map(color_of).unwrap_or([255; 4]);

        // source metrics of the first frame: the component as it is
        let (mut cur_off, mut src_size, mut src_col) = (base, (size0.0 as i32, size0.1 as i32), colour0);
        let mut remaining = lt;
        let mut ep = EntryPose { propagate: inst.anim.propagate, ..Default::default() };
        for f in &inst.anim.frames {
            let local = remaining.min(f.time);
            let finished = remaining >= f.time;
            let p = if finished || f.time == 0 { 1.0 } else { f.curve.progress(local as f32, f.time as f32, f.weight) };
            if f.mask & MASK_POSITION != 0 {
                let off = if f.absolute { (lerp(cur_off.0, f.offset.0, p), lerp(cur_off.1, f.offset.1, p)) } else { (cur_off.0 + lerp(0.0, f.offset.0, p), cur_off.1 + lerp(0.0, f.offset.1, p)) };
                ep.offset = Some(off);
                if finished {
                    cur_off = if f.absolute { f.offset } else { (cur_off.0 + f.offset.0, cur_off.1 + f.offset.1) };
                }
            }

            if f.mask & MASK_SIZE != 0 {
                let w = src_size.0 + ((f.size.0 - src_size.0) as f32 * p) as i32;
                let h = src_size.1 + ((f.size.1 - src_size.1) as f32 * p) as i32;
                ep.size = Some((w as f32, h as f32));
            }

            if f.mask & (MASK_COLOUR | MASK_OPACITY) != 0 {
                let mix = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * p).round().clamp(0.0, 255.0);
                if f.mask & MASK_COLOUR != 0 {
                    ep.rgb = Some([mix(src_col[0], f.color[0]) as u8, mix(src_col[1], f.color[1]) as u8, mix(src_col[2], f.color[2]) as u8]);
                }

                if f.mask & MASK_OPACITY != 0 {
                    ep.opacity = Some(mix(src_col[3], f.color[3]));
                }
            }

            // the next frame starts from this one's target
            src_size = f.size;
            src_col = f.color;
            if !finished {
                break;
            }

            remaining -= f.time;
        }

        let slot = pose.by_entry.entry(en.path.clone()).or_default();
        if ep.offset.is_some() {
            slot.offset = ep.offset;
        }

        if ep.size.is_some() {
            slot.size = ep.size;
        }

        if ep.opacity.is_some() {
            slot.opacity = ep.opacity;
        }

        if ep.rgb.is_some() {
            slot.rgb = ep.rgb;
        }

        slot.propagate |= ep.propagate;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_start_at_zero_and_end_at_one() {
        for c in [Curve::Linear, Curve::QuadIn, Curve::QuadOut, Curve::QuadInOut, Curve::CubicIn, Curve::CubicOut, Curve::CubicInOut, Curve::CircIn, Curve::CircOut, Curve::CircInOut] {
            let a = c.progress(0.0, 100.0, 1.0);
            let b = c.progress(100.0, 100.0, 1.0);
            assert!(a.abs() < 1e-4, "{c:?} starts at {a}");
            assert!((b - 1.0).abs() < 1e-4, "{c:?} ends at {b}");
        }
    }

    #[test]
    fn exponential_curves_stop_short_of_one_as_the_game_does() {
        // 1 - 2^-10: the frame itself ends with the full value when its time is up
        assert!((Curve::ExpOut.progress(100.0, 100.0, 1.0) - 0.99902).abs() < 1e-4);
        assert!(Curve::ExpIn.progress(0.0, 100.0, 1.0) < 0.001);
    }

    #[test]
    fn labels() {
        assert_eq!(Curve::from_label("Quadratic In & Out"), Curve::QuadInOut);
        assert_eq!(Curve::from_label("Linear"), Curve::Linear);
        assert_eq!(Curve::from_label("what"), Curve::Linear);
    }
}
