//! A unit on its feet: one call from a unit and a time to what the scene draws.
//!
//! [`UnitModel`] holds a soldier (and his mount): his parts, skeleton and a schedule of clips. [`UnitModel::instances`] is a pure function of the
//! time, so that a timeline can scrub it. Slots are looked up through the unit's animation table.
//!
//! - The schedule loops, built once from the seed, so soldiers don't move as one: a slot's lines are its variations, picked by `selection_weight`, and a
//!   standing slot mixes in its `<slot>_IDLE_n` slots. Each clip blends in from the one before over its `blend_in_time`. A mount's schedule follows
//!   its rider's slot by slot, at the rider's lengths. **ASSUMPTION**: the engine's choice isn't traced; see [`pick`] for `single_frame_variant`.
//! - A `.anim` moves the body; the fingers come from the clip's `RHAND_POSE` / `LHAND_POSE` entries, whose numbers are the `HAND_POSE_*` slots in the
//!   engine's order (`BATTLE_ENTITY_ANIMATION_TYPE`).
//! - `SPLICE` entries (`META_DATA_SPLICE`) lay another clip over parts of the body by the region weights: as a pose, or added as the difference from its
//!   reference. The regions by bone name are **ASSUMPTION**: the engine's `SPLICE_INFO` works by depths in the skeleton.
//! - A clip's `PROP` and `ANIMATED_PROP` entries hold another model on a bone from their start to the clip's end (their end is their start): the bow
//!   that bends while shooting, with a skeleton and a clip of its own. That clip is in the figure's space, so such a prop is placed at the figure's origin
//!   (checked: the bow comes to the left hand). **ASSUMPTION** (not traced): it takes the place of the static part on its bone, the rigid bow of the
//!   definition, and its clip runs from the entry's start.
//! - `WEAPON_ON` shows what a weapon bone carries, from its start: a clip with such an entry hides that bone's parts until then. `bn_weapon_05` is the
//!   ammunition's (arrows, darts): it jumps within a frame in half its clips even with its flag on, so its parts show only under a `WEAPON_ON`
//!   (**ASSUMPTION**, both: the arrow appears in the archer's hand, no dart hangs in the air).
//!
//! - A clip travels (`bn_hips` from its first to its last frame: the battle moves the entity along) and a schedule stays where it is: that travel, on the
//!   ground plane, is taken out of the clip's root bones as it goes, so a walk loops on the spot. A timeline moving a unit would add it back: [`Clip::travel`].
//!
//! Not used: `weapon_bone_N` flags and `WEAPON_RHAND`, `_LHAND`, `_HIP`. The clips' tracks carry the weapons (the poses are right without them), and over
//! 1500 to 2500 clips neither says where a bone is or whether it jumps: on and off bones are near the body alike, `WEAPON_RHAND` of a sword has it in the
//! hand 503 times and away 128. They look like the battle logic's state, not motion. Not here yet: `stretch`, face poses.

use std::collections::HashMap;
use std::sync::Arc;

use wce_formats::anim::{Animation, BoneLocal};
use wce_formats::anim_meta::Meta;
use wce_formats::anim_tables::{AnimationTable, FragmentEntry, parse_fragment};
use wce_foundation::math::{Float3, Float4x4, translation};
use wce_scene::render_scene::{SKINNED_NO_OFFSET, SkinnedInstance, SkinnedPart};

use crate::anim_pose::{inverse_binds, palette, world_pose};
use crate::unit_assembly::{AssembledUnit, Attach, Reader, Rng, assemble};
use crate::unit_catalog::{UnitCatalog, variant_path};

/// The `HAND_POSE_*` slots, by the number a clip's metadata gives.
const HAND_POSES: [&str; 9] = [
    "HAND_POSE_RELAX", "HAND_POSE_FLAX", "HAND_POSE_FLAT", "HAND_POSE_SHIELD", "HAND_POSE_SPEAR_CLENCH", "HAND_POSE_SWORD_ANGLED", "HAND_POSE_SWORD_STAND",
    "HAND_POSE_SWORD_STRAIGHT", "HAND_POSE_POINT",
];

/// A table's clips, by slot.
#[derive(Clone, Debug, Default)]
pub struct AnimationSet {
    pub skeleton: String,

    /// The later fragments of a table override the earlier ones' slots (**ASSUMPTION**: a weapon's fragment over the unarmed one).
    pub slots: HashMap<String, Vec<FragmentEntry>>,
}

impl AnimationSet {
    pub fn load(read: &Reader, table: &AnimationTable) -> Self {
        let mut slots = HashMap::<String, Vec<FragmentEntry>>::new();
        for name in &table.fragments {
            let Some(bytes) = read(&format!("animations/animation_fragments/{name}.txt")) else { continue };
            let Some(fragment) = parse_fragment(&String::from_utf8_lossy(&bytes)) else { continue };
            let mut own = HashMap::<String, Vec<FragmentEntry>>::new();
            for e in fragment.entries.into_iter().filter(|e| !e.filename.is_empty()) {
                own.entry(e.slot.clone()).or_default().push(e);
            }

            slots.extend(own);
        }

        Self { skeleton: table.skeleton.clone(), slots }
    }
}

/// A model a clip holds on a bone.
#[derive(Clone, Debug, Default)]
pub struct Prop {
    pub model: String,

    /// When it appears in the clip.
    pub start: f32,

    /// The figure's bone that carries it.
    pub bone: u32,

    /// From that bone to the model.
    pub offset: Float4x4,

    /// Its own skeleton and clip; `None` for a model that doesn't move.
    pub anim: Option<(Rig, Animation)>,
}

/// Another clip laid over a clip.
#[derive(Clone, Debug, Default)]
pub struct Splice {
    pub anim: Animation,

    /// What an additive splice is the difference from: its first frame, on the skeleton.
    pub reference: Option<Vec<BoneLocal>>,

    /// Torso, left arm, right arm, legs.
    pub weights: [f32; 4],
    pub blend_in: f32,
    pub blend_out: f32,
    pub additive: bool,
    pub start: f32,

    /// When it ends; its start: at the clip's end.
    pub end: f32,
}

/// A clip on its skeleton, its metadata and what its entries add.
#[derive(Clone, Debug, Default)]
pub struct Clip {
    pub anim: Animation,
    pub meta: Option<Meta>,
    pub props: Vec<Prop>,
    pub splices: Vec<Splice>,
}

/// One clip of a schedule.
#[derive(Clone, Debug, Default)]
pub struct Segment {
    pub clip: Arc<Clip>,
    pub slot: String,

    /// How long it plays: the clip's duration, or the rider's segment's on a mount.
    pub length: f32,
    pub blend_in: f32,
}

/// What the scene draws of a unit at a moment.
#[derive(Clone, Debug)]
pub struct UnitInstance {
    /// The figure's scale.
    pub scale: f32,

    /// Between the unit's origin (scaled) and this instance: the identity but for a prop.
    pub local: Float4x4,
    pub instance: SkinnedInstance,
}

fn qmul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

fn normalised(q: [f32; 4]) -> [f32; 4] {
    let len = q.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-6);
    q.map(|v| v / len)
}

/// Between two local transforms: translations lerped, rotations nlerped along the shorter arc.
fn blend_local(a: BoneLocal, b: BoneLocal, w: f32) -> BoneLocal {
    let sign = if a.1.iter().zip(&b.1).map(|(x, y)| x * y).sum::<f32>() < 0.0 { -1.0 } else { 1.0 };
    (std::array::from_fn(|c| a.0[c] + (b.0[c] - a.0[c]) * w), normalised(std::array::from_fn(|c| a.1[c] + (sign * b.1[c] - a.1[c]) * w)))
}

/// `local` with the difference from `base` to `over` added.
fn add_local(local: BoneLocal, base: BoneLocal, over: BoneLocal) -> BoneLocal {
    let inverse = [-base.1[0], -base.1[1], -base.1[2], base.1[3]];
    (std::array::from_fn(|c| local.0[c] + over.0[c] - base.0[c]), normalised(qmul(local.1, qmul(inverse, over.1))))
}

/// The fingers: which bones a hand pose moves. The clips name them `bn_lefthandindex1`...
fn hand_side(bone: &str) -> Option<bool> {
    let finger = |b: &str| b.starts_with("index") || b.starts_with("ring") || b.starts_with("thumb");
    if bone.strip_prefix("bn_lefthand").is_some_and(finger) {
        Some(true)
    } else if bone.strip_prefix("bn_righthand").is_some_and(finger) {
        Some(false)
    } else {
        None
    }
}

/// The body region of a bone, as `Splice::weights` indexes them: torso, left arm, right arm, legs.
fn region(bone: &str) -> Option<usize> {
    let name = bone.strip_prefix("bn_")?;
    let limb = |side: &str| name.strip_prefix(side).map(|rest| ["arm", "forearm", "shoulder", "hand"].iter().any(|p| rest.starts_with(p)));
    let leg = ["upleg", "leg", "foot", "toe"].iter().any(|p| name.strip_prefix("left").or_else(|| name.strip_prefix("right")).is_some_and(|r| r.starts_with(p)));
    if leg {
        Some(3)
    } else if limb("left") == Some(true) {
        Some(1)
    } else if limb("right") == Some(true) {
        Some(2)
    } else if ["hips", "spine", "neck", "head"].iter().any(|p| name.starts_with(p)) {
        Some(0)
    } else {
        None
    }
}

/// A skeleton with what it needs to pose: its bind and the hand poses of the set.
#[derive(Clone, Debug, Default)]
pub struct Rig {
    pub skeleton: Animation,
    binds: Vec<Float4x4>,

    /// By `HAND_POSES`' number: per bone its local transform, where the bone is a finger.
    hands: Vec<Option<Vec<BoneLocal>>>,
}

impl Rig {
    pub fn load(read: &Reader, set: &AnimationSet) -> Option<Self> {
        Self::of_skeleton(read, set, &set.skeleton)
    }

    fn of_skeleton(read: &Reader, set: &AnimationSet, name: &str) -> Option<Self> {
        let stem = format!("animations/skeletons/{name}");
        let skeleton = Animation::decode(&read(&format!("{stem}.anim"))?, 1)?;
        let binds = inverse_binds(&read(&format!("{stem}.bone_inv_trans_mats"))?)?;
        let hands = HAND_POSES.iter().map(|slot| {
            let path = &set.slots.get(*slot)?.first()?.filename;
            let mut clip = Animation::decode(&read(&wce_vfs::normalize(path))?, 1)?;
            clip.on_skeleton(&skeleton);
            clip.frames.into_iter().next()
        }).collect();
        Some(Self { skeleton, binds, hands })
    }

    pub fn bone(&self, name: &str) -> Option<u32> {
        self.skeleton.bones.iter().position(|b| b == name).map(|i| i as u32)
    }

    pub fn clip(&self, read: &Reader, entry: &FragmentEntry) -> Option<Clip> {
        let mut anim = Animation::decode(&read(&wce_vfs::normalize(&entry.filename))?, usize::MAX)?;
        anim.on_skeleton(&self.skeleton);
        let meta = (!entry.metadata.is_empty()).then(|| read(&wce_vfs::normalize(&entry.metadata)).and_then(|b| Meta::decode(&b))).flatten();
        let entries = || meta.iter().flat_map(|m| &m.entries);
        let props = entries().filter(|e| matches!(e.kind.as_str(), "PROP" | "ANIMATED_PROP")).filter_map(|e| Self::prop(read, e)).collect();
        let splices = entries().filter(|e| e.kind == "SPLICE").filter_map(|e| self.splice(read, e)).collect();
        Some(Clip { anim, meta, props, splices })
    }

    /// A `SPLICE` entry. From version 3: clip, reference, the four weights, blend in and out, additive, weapon pose (and from 4, stretch); before: clip, the
    /// four weights, a flag.
    fn splice(&self, read: &Reader, e: &wce_formats::anim_meta::MetaEntry) -> Option<Splice> {
        let v = e.version.unwrap_or(0);
        let first = if v >= 3 { 2 } else { 1 };
        let weights = [e.f32(first)?, e.f32(first + 1)?, e.f32(first + 2)?, e.f32(first + 3)?];
        let (blend_in, blend_out) = if v >= 3 { (e.f32(6)?, e.f32(7)?) } else { (0.0, 0.0) };
        let additive = v >= 3 && e.str(8) == Some("true");
        let on_skeleton = |path: &str| {
            let mut anim = Animation::decode(&read(&wce_vfs::normalize(path))?, usize::MAX)?;
            anim.on_skeleton(&self.skeleton);
            Some(anim)
        };

        let anim = on_skeleton(e.str(0)?)?;
        let reference = if additive { Some(on_skeleton(e.str(1).filter(|r| !r.is_empty()).unwrap_or_else(|| e.str(0).unwrap_or_default()))?.sample(0.0)) } else { None };
        Some(Splice { anim, reference, weights, blend_in, blend_out, additive, start: e.start, end: e.end })
    }

    /// Lays a clip's splices over a pose at `t` of the clip.
    fn apply_splices(&self, clip: &Clip, t: f32, locals: &mut [BoneLocal]) {
        let duration = clip.anim.duration();
        for s in &clip.splices {
            let end = if s.end > s.start { s.end } else { duration };
            if t < s.start || t > end {
                continue;
            }

            let into = if s.blend_in > 0.0 { ((t - s.start) / s.blend_in).min(1.0) } else { 1.0 };
            let out = if s.blend_out > 0.0 { ((end - t) / s.blend_out).min(1.0) } else { 1.0 };
            let time = (t - s.start).rem_euclid(s.anim.duration().max(1e-3));
            let pose = s.anim.sample(time);
            for (i, name) in clip.anim.bones.iter().enumerate() {
                let Some(w) = region(name).map(|r| s.weights[r] * into * out).filter(|w| *w > 0.0) else { continue };
                let (Some(local), Some(over)) = (locals.get(i).copied(), pose.get(i)) else { continue };
                let target = match s.reference.as_ref().and_then(|r| r.get(i)) {
                    Some(base) => add_local(local, *base, *over),
                    None => *over,
                };

                locals[i] = blend_local(local, target, w);
            }
        }
    }

    /// A `PROP` or `ANIMATED_PROP` entry: model, clip (may be empty), position, rotation, bone.
    fn prop(read: &Reader, e: &wce_formats::anim_meta::MetaEntry) -> Option<Prop> {
        let f: Vec<f32> = (2..9).map(|i| e.f32(i)).collect::<Option<_>>()?;
        let offset = world_pose(&[-1], &[([f[0], f[1], f[2]], [f[3], f[4], f[5], f[6]])])[0];
        let anim = Some(e.str(1)?).filter(|a| !a.is_empty()).and_then(|path| {
            let mut anim = Animation::decode(&read(&wce_vfs::normalize(path))?, usize::MAX)?;
            let rig = Self::of_skeleton(read, &AnimationSet::default(), &anim.skeleton.clone())?;
            anim.on_skeleton(&rig.skeleton);
            Some((rig, anim))
        });
        Some(Prop { model: wce_vfs::normalize(e.str(0)?), start: e.start, bone: e.u32(9)?, offset, anim })
    }

    /// The hand pose number in force at `t`: the last entry of `kind` that has started.
    fn hand_at(clip: &Clip, kind: &str, t: f32) -> Option<usize> {
        let entries = clip.meta.iter().flat_map(|m| &m.entries).filter(|e| e.kind == kind);
        entries.filter(|e| e.start <= t).max_by(|a, b| a.start.total_cmp(&b.start)).and_then(|e| e.u32(0)).map(|n| n as usize)
    }

    /// The bones' local transforms of a clip at `t` (seconds, held at its end).
    pub fn locals(&self, clip: &Clip, t: f32) -> Vec<BoneLocal> {
        let mut locals = clip.anim.sample(t);
        self.apply_splices(clip, t, &mut locals);
        let (left, right) = (Self::hand_at(clip, "LHAND_POSE", t), Self::hand_at(clip, "RHAND_POSE", t));
        for (i, name) in clip.anim.bones.iter().enumerate() {
            let Some(is_left) = hand_side(name) else { continue };
            let Some(pose) = (if is_left { left } else { right }).and_then(|n| self.hands.get(n)).and_then(Option::as_ref) else { continue };
            if let (Some(l), Some(p)) = (locals.get_mut(i), pose.get(i)) {
                *l = *p;
            }
        }

        locals
    }

    /// A pose as the palette and the bones of a [`SkinnedInstance`].
    pub fn pose(&self, locals: &[BoneLocal]) -> (Arc<Vec<[f32; 12]>>, Arc<Vec<[f32; 12]>>) {
        let world = world_pose(&self.skeleton.parents, locals);
        let skin: Vec<Float4x4> = world.iter().zip(&self.binds).map(|(w, i)| *w * *i).collect();
        (Arc::new(palette(&skin)), Arc::new(palette(&world)))
    }
}

/// One figure of a unit: a soldier or his mount.
#[derive(Clone, Debug, Default)]
pub struct Figure {
    pub parts: AssembledUnit,
    pub rig: Rig,
    pub set: AnimationSet,
    pub segments: Vec<Segment>,
}

/// How long a schedule is, in clips, for a slot with idles.
const SCHEDULE_CLIPS: usize = 8;

/// A line of a slot, by `selection_weight`, among the ones without `single_frame_variant`. A slot of nothing but those plays its first: the flag is on
/// plain looping clips too (the horse's `STAND` is two such lines, `horse_stand` first), and what it does in the engine isn't traced.
fn pick<'a>(lines: &'a [FragmentEntry], rng: &mut Rng) -> Option<&'a FragmentEntry> {
    let usable: Vec<&FragmentEntry> = lines.iter().filter(|e| e.flag("single_frame_variant").is_none()).collect();
    if usable.is_empty() {
        return lines.first();
    }

    let weight = |e: &FragmentEntry| e.flag("selection_weight").and_then(|w| w.parse::<f32>().ok()).unwrap_or(1.0).max(0.0);
    let mut left = rng.unit() * usable.iter().map(|e| weight(e)).sum::<f32>();
    for e in &usable {
        left -= weight(e);
        if left <= 0.0 {
            return Some(e);
        }
    }

    usable.last().copied()
}

impl Clip {
    /// How far the clip has travelled on the ground plane at `t`, evenly along it.
    pub fn travel(&self, t: f32) -> [f32; 2] {
        let (Some(first), Some(last)) = (self.anim.frames.first(), self.anim.frames.last()) else { return [0.0; 2] };
        let (Some(a), Some(b)) = (first.first(), last.first()) else { return [0.0; 2] };
        let at = (t / self.anim.duration().max(1e-3)).clamp(0.0, 1.0);
        [(b.0[0] - a.0[0]) * at, (b.0[2] - a.0[2]) * at]
    }
}

impl Figure {
    fn load(read: &Reader, catalog: &UnitCatalog, table: &str, variant: &str, seed: u64) -> Result<Self, String> {
        let table = catalog.tables.get(table).ok_or_else(|| format!("no animation table '{table}'"))?;
        let set = AnimationSet::load(read, table);
        let rig = Rig::load(read, &set).ok_or_else(|| format!("skeleton '{}' does not load", set.skeleton))?;
        let parts = assemble(read, &variant_path(variant), seed).ok_or_else(|| format!("variant '{variant}' does not load"))?;
        Ok(Self { parts, rig, set, segments: vec![] })
    }

    /// A segment of a slot; `length` 0: the clip's. A clip's own skeleton wins over the table's: a mod's rider is on one of its own.
    fn segment(&mut self, read: &Reader, slot: &str, rng: &mut Rng, length: f32) -> Option<Segment> {
        let entry = pick(self.set.slots.get(slot)?, rng)?.clone();
        let mut clip = self.rig.clip(read, &entry)?;
        let own = clip.anim.skeleton.clone();
        if own != self.rig.skeleton.skeleton && let Some(other) = Rig::of_skeleton(read, &self.set, &own) {
            self.rig = other;
            clip = self.rig.clip(read, &entry)?;
        }

        let blend_in = entry.flag("blend_in_time").and_then(|b| b.parse().ok()).unwrap_or(0.0);
        let length = if length > 0.0 { length } else { clip.anim.duration().max(1e-3) };
        Some(Segment { clip: Arc::new(clip), slot: slot.to_owned(), length, blend_in })
    }

    /// The slots of a schedule for `slot`: the slot alone, or mixed with its idles.
    fn schedule(&self, slot: &str, rng: &mut Rng) -> Vec<String> {
        let prefix = format!("{slot}_IDLE_");
        let mut idles: Vec<&String> = self.set.slots.keys().filter(|k| k.starts_with(&prefix)).collect();
        idles.sort();
        if idles.is_empty() {
            return vec![slot.to_owned()];
        }

        (0..SCHEDULE_CLIPS).map(|k| if k == 0 || rng.below(3) != 0 { slot.to_owned() } else { idles[rng.below(idles.len())].clone() }).collect()
    }

    /// Plays `slots` in a loop, at `lengths` where given; false when none of them is there.
    fn play(&mut self, read: &Reader, slots: &[String], lengths: &[f32], rng: &mut Rng) -> bool {
        let segments: Vec<Segment> = slots.iter().enumerate().filter_map(|(k, s)| self.segment(read, s, rng, lengths.get(k).copied().unwrap_or(0.0))).collect();
        let done = !segments.is_empty();
        if done {
            self.segments = segments;
        }

        done
    }

    fn duration(&self) -> f32 {
        self.segments.iter().map(|s| s.length).sum::<f32>().max(1e-3)
    }

    /// The segment at `t`, its index and the time inside it.
    fn at(&self, t: f32) -> Option<(usize, f32)> {
        let mut left = t.rem_euclid(self.duration());
        for (k, s) in self.segments.iter().enumerate() {
            if left < s.length {
                return Some((k, left));
            }

            left -= s.length;
        }

        (!self.segments.is_empty()).then(|| (self.segments.len() - 1, 0.0))
    }

    /// The pose at `t`: the segment's clip, coming in from the one before over its blend.
    fn locals_at(&self, k: usize, inside: f32) -> Vec<BoneLocal> {
        let s = &self.segments[k];
        let in_place = |clip: &Clip, t: f32| {
            let mut locals = self.rig.locals(clip, t);
            let [x, z] = clip.travel(t);
            for (l, _) in locals.iter_mut().zip(&clip.anim.parents).filter(|(_, p)| **p < 0) {
                (l.0[0], l.0[2]) = (l.0[0] - x, l.0[2] - z);
            }

            locals
        };

        let mut locals = in_place(&s.clip, inside);
        if s.blend_in > 0.0 && inside < s.blend_in {
            let before = &self.segments[(k + self.segments.len() - 1) % self.segments.len()];
            let from = in_place(&before.clip, (before.length + inside).min(before.clip.anim.duration()));
            let w = inside / s.blend_in;
            let w = w * w * (3.0 - 2.0 * w);
            for (l, f) in locals.iter_mut().zip(from) {
                *l = blend_local(f, *l, w);
            }
        }

        locals
    }

    /// The figure at `t`, then the props the clip holds by then.
    fn instances(&self, t: f32, scale: f32, faction_colours: [[f32; 3]; 4], casts_shadow: bool) -> Vec<UnitInstance> {
        let Some((k, inside)) = self.at(t) else { return vec![] };
        let clip = &self.segments[k].clip;
        let locals = self.locals_at(k, inside);
        let world = world_pose(&self.rig.skeleton.parents, &locals);
        let (body_palette, body_bones) = self.rig.pose(&locals);
        let held: Vec<&Prop> = clip.props.iter().filter(|p| p.start <= inside).collect();
        let ammunition = self.rig.bone("bn_weapon_05");
        let hidden = |bone: u32| {
            let mut ons = clip.meta.iter().flat_map(|m| &m.entries).filter(|e| e.kind == "WEAPON_ON" && e.u32(0) == Some(bone)).peekable();
            (ons.peek().is_some() || Some(bone) == ammunition) && !ons.any(|e| e.start <= inside)
        };

        let parts = self.parts.parts.iter().filter_map(|p| {
            let (bone, offset) = match &p.attach {
                Attach::None => (None, SKINNED_NO_OFFSET),
                Attach::Bone(name) => (self.rig.bone(name), SKINNED_NO_OFFSET),
                Attach::Point { bone, matrix } => (Some(*bone), *matrix),
            };

            // A prop takes the bone's place.
            (bone.is_none() || !(held.iter().any(|h| Some(h.bone) == bone) || bone.is_some_and(hidden))).then(|| {
                SkinnedPart { model: p.model.clone(), tint: p.tint, decal_diffuse: p.decal_diffuse.clone(), decal_normal: p.decal_normal.clone(), masks: p.masks, bone, offset }
            })
        }).collect();
        let body = SkinnedInstance { parts, palette: body_palette, bones: body_bones, casts_shadow, faction_colours, faction_scale: 0.0 };
        let mut out = vec![UnitInstance { scale, local: Float4x4::IDENTITY, instance: body }];
        for prop in held {
            let (prop_palette, bones) = match &prop.anim {
                Some((rig, anim)) => rig.pose(&anim.sample(inside - prop.start)),
                None => (Arc::new(palette(&[Float4x4::IDENTITY])), Arc::new(vec![])),
            };

            let part = SkinnedPart { model: prop.model.clone(), tint: [0.0; 4], decal_diffuse: None, decal_normal: None, masks: [0, 1, 2], bone: None, offset: SKINNED_NO_OFFSET };

            // A clip of its own is authored in the figure's space, as the figure's are: the bone only says what it replaces.
            let [x, z] = clip.travel(inside);
            let carrier = if prop.anim.is_some() { translation(Float3::new(-x, 0.0, -z)) } else { world.get(prop.bone as usize).copied().unwrap_or(Float4x4::IDENTITY) };
            let instance = SkinnedInstance { parts: vec![part], palette: prop_palette, bones, casts_shadow, faction_colours, faction_scale: 0.0 };
            out.push(UnitInstance { scale, local: carrier * prop.offset, instance });
        }

        out
    }
}

/// A soldier of a unit, and his mount if he rides.
#[derive(Clone, Debug, Default)]
pub struct UnitModel {
    pub man: Figure,
    pub mount: Option<Figure>,

    /// The man's scale over the skeleton's size (`height_scale`, and `height_variation` by the seed).
    pub scale: f32,
    pub faction_colours: [[f32; 3]; 4],

    /// Where in its schedule the unit is at time 0: soldiers aren't in step.
    pub phase: f32,
    seed: u64,
}

impl UnitModel {
    /// A soldier of `unit` for `faction` (a `factions` key, or anything for the unit's general row), the same for the same seed, standing.
    ///
    /// # Errors
    ///
    /// Returns which link of the unit's data is missing.
    pub fn load(read: &Reader, catalog: &UnitCatalog, unit: &str, faction: &str, seed: u64) -> Result<Self, String> {
        let entry = catalog.entry(unit, faction).ok_or_else(|| format!("no unit_variants row for '{unit}'"))?;
        let land = catalog.land_units.get(unit).ok_or_else(|| format!("no land unit for '{unit}'"))?;
        let mount = catalog.mounts.get(&land.mount);
        let man = Figure::load(read, catalog, &land.man_animation, &entry.variant, seed)?;
        let figure = mount.map(|m| Figure::load(read, catalog, &m.animation, &m.variant, seed.wrapping_add(1))).transpose()?;

        // Evenly over the variation, either way. **ASSUMPTION**: the engine's distribution isn't traced.
        let spread = ((seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40) as f32 / (1u64 << 24) as f32).mul_add(2.0, -1.0);
        let scale = entry.height_scale.max(0.01) * (1.0 + entry.height_variation * spread);
        let white = [[1.0; 3], [1.0; 3], [1.0; 3], [0.0; 3]];
        let mut model = Self { man, mount: figure, scale, faction_colours: catalog.colours.get(faction).copied().unwrap_or(white), phase: 0.0, seed };
        let slots: &[&str] = if model.mount.is_some() { &["RIDER_STAND", "STAND", "MISSING_ANIM"] } else { &["STAND", "MISSING_ANIM"] };
        if !slots.iter().any(|s| model.play(read, s)) {
            return Err(format!("none of the slots {slots:?} in table '{}'", land.man_animation));
        }

        Ok(model)
    }

    /// The models, for waiting on them.
    pub fn models(&self) -> Vec<String> {
        let figures = std::iter::once(&self.man).chain(self.mount.as_ref());
        figures.flat_map(|f| f.parts.parts.iter().map(|p| p.model.clone()).chain(f.segments.iter().flat_map(|s| s.clip.props.iter().map(|p| p.model.clone())))).collect()
    }

    /// The slots the man can play, sorted.
    pub fn slots(&self) -> Vec<&str> {
        let mut slots: Vec<&str> = self.man.set.slots.keys().map(String::as_str).collect();
        slots.sort_unstable();
        slots
    }

    /// Plays a slot, with its variations and idles, in a loop; a mount follows clip by clip, with its slots without the `RIDER_`. False when the man
    /// hasn't the slot.
    pub fn play(&mut self, read: &Reader, slot: &str) -> bool {
        let mut rng = Rng(self.seed ^ 0x5851_F42D_4C95_7F2D);
        let slots = self.man.schedule(slot, &mut rng);
        if !self.man.set.slots.contains_key(slot) || !self.man.play(read, &slots, &[], &mut rng) {
            return false;
        }

        let lengths: Vec<f32> = self.man.segments.iter().map(|s| s.length).collect();
        if let Some(mount) = &mut self.mount {
            let of_mount: Vec<String> = self.man.segments.iter().map(|s| {
                let own = s.slot.strip_prefix("RIDER_").unwrap_or(&s.slot);
                [own.to_owned(), format!("{own}_1")].into_iter().find(|o| mount.set.slots.contains_key(o)).unwrap_or_else(|| "STAND".to_owned())
            }).collect();
            mount.play(read, &of_mount, &lengths, &mut rng);
        }

        self.phase = rng.unit() * self.man.duration();
        true
    }

    /// What the scene draws at `t` (seconds). A man and his mount share one frame and one origin: their clips are made so.
    pub fn instances(&self, t: f32, casts_shadow: bool) -> Vec<UnitInstance> {
        let t = t + self.phase;
        let mut out = self.man.instances(t, self.scale, self.faction_colours, casts_shadow);
        if let Some(mount) = &self.mount {
            out.extend(mount.instances(t, 1.0, self.faction_colours, casts_shadow));
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use wce_formats::anim_meta::{Meta, MetaEntry, MetaField};

    use wce_formats::anim_tables::FragmentEntry;

    use super::{Clip, Figure, Rig, Segment, add_local, blend_local, hand_side, pick, region};
    use crate::unit_assembly::Rng;

    #[test]
    fn fingers_are_told_from_the_hand() {
        assert_eq!(hand_side("bn_lefthandring2"), Some(true));
        assert_eq!(hand_side("bn_righthandthumb1"), Some(false));
        assert_eq!((hand_side("bn_lefthand"), hand_side("bn_head")), (None, None));
    }

    #[test]
    fn bones_fall_in_their_regions() {
        let of = |b: &str| region(b);
        assert_eq!((of("bn_spine1"), of("bn_head"), of("bn_leftforearm"), of("bn_righthandring2")), (Some(0), Some(0), Some(1), Some(2)));
        assert_eq!((of("bn_lefttoebase"), of("bn_rightupleg"), of("bn_weapon_03"), of("bn_jaw")), (Some(3), Some(3), None, None));
    }

    #[test]
    fn a_clip_travels_evenly() {
        let bone = |z: f32| ([0.0, 1.0, z], [0.0, 0.0, 0.0, 1.0]);
        let clip = Clip { anim: wce_formats::anim::Animation { fps: 10.0, end: 2.0, frames: vec![vec![bone(1.0)], vec![bone(3.0)]], ..Default::default() }, ..Clip::default() };
        assert_eq!((clip.travel(0.0), clip.travel(1.0), clip.travel(2.0)), ([0.0, 0.0], [0.0, 1.0], [0.0, 2.0]));
    }

    #[test]
    fn a_blend_runs_between_the_two_poses() {
        let (a, b) = (([0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]), ([2.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]));
        assert_eq!(blend_local(a, b, 0.0), a);
        let half = blend_local(a, b, 0.5);
        assert!((half.0[0] - 1.0).abs() < 1e-5);
        let s = std::f32::consts::FRAC_1_SQRT_2;
        assert!((half.1[1] - s).abs() < 1e-3 && (half.1[3] - s).abs() < 1e-3, "{:?}", half.1);
    }

    #[test]
    fn an_additive_pose_adds_the_difference() {
        let q = |y: f32, w: f32| [0.0, y, 0.0, w];
        let s = std::f32::consts::FRAC_1_SQRT_2;
        // The reference at a quarter turn, the splice at a half turn: a quarter turn more.
        let out = add_local(([1.0, 0.0, 0.0], q(0.0, 1.0)), ([0.0, 0.0, 0.0], q(s, s)), ([0.5, 0.0, 0.0], q(1.0, 0.0)));
        assert!((out.0[0] - 1.5).abs() < 1e-5);
        assert!((out.1[1] - s).abs() < 1e-3 && (out.1[3] - s).abs() < 1e-3, "{:?}", out.1);
    }

    #[test]
    fn a_line_is_picked_by_its_weight() {
        let line = |name: &str, flags: &[(&str, &str)]| FragmentEntry { filename: name.to_owned(), flags: flags.iter().map(|(a, b)| ((*a).to_owned(), (*b).to_owned())).collect(), ..FragmentEntry::default() };
        let lines = [line("never", &[("selection_weight", "0.00")]), line("pose", &[("single_frame_variant", "true")]), line("yes", &[])];
        let mut rng = Rng(7);
        assert!((0..50).all(|_| pick(&lines, &mut rng).is_some_and(|e| e.filename == "yes")));

        // A horse's STAND: every line flagged, the first the base clip.
        let flagged = [line("base", &[("single_frame_variant", "true")]), line("variation", &[("single_frame_variant", "true")])];
        assert!((0..50).all(|_| pick(&flagged, &mut rng).is_some_and(|e| e.filename == "base")));
    }

    #[test]
    fn the_schedule_finds_its_segment_and_loops() {
        let segment = |length: f32| Segment { length, ..Segment::default() };
        let f = Figure { segments: vec![segment(1.0), segment(2.0)], ..Figure::default() };
        assert_eq!((f.at(0.5), f.at(1.5), f.at(3.5), f.at(-0.5)), (Some((0, 0.5)), Some((1, 0.5)), Some((0, 0.5)), Some((1, 1.5))));
        assert_eq!(Figure::default().at(1.0), None);
    }

    #[test]
    fn the_hand_pose_is_the_last_one_started() {
        let entry = |start: f32, pose: u32| MetaEntry { kind: "LHAND_POSE".to_owned(), version: Some(2), start, end: start, tag: String::new(), fields: vec![MetaField::U32(pose), MetaField::F32(1.0)] };
        let clip = Clip { meta: Some(Meta { version: 2, entries: vec![entry(0.0, 3), entry(0.5, 6)] }), ..Clip::default() };
        assert_eq!((Rig::hand_at(&clip, "LHAND_POSE", 0.2), Rig::hand_at(&clip, "LHAND_POSE", 0.7)), (Some(3), Some(6)));
        assert_eq!(Rig::hand_at(&clip, "RHAND_POSE", 0.7), None);
    }
}
