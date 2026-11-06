//! A unit on its feet: one call from a unit and a time to what the scene draws.
//!
//! [`UnitModel`] holds a soldier (and his mount): his parts, skeleton and clips. [`UnitModel::instances`] is a pure function of the time, so that a
//! timeline can scrub it. A `.anim` moves the body; the fingers come from the clip's `RHAND_POSE` / `LHAND_POSE` entries, whose numbers are the
//! `HAND_POSE_*` slots in the engine's order (`BATTLE_ENTITY_ANIMATION_TYPE`). Slots are looked up through the unit's animation table.
//!
//! Not here yet: blending between clips, a slot's variations other than the first, `weapon_bone_N` flags and `SPLICE`s, `ANIMATED_PROP`s (bows),
//! face poses.

use std::collections::HashMap;
use std::sync::Arc;

use wce_formats::anim::{Animation, BoneLocal};
use wce_formats::anim_meta::Meta;
use wce_formats::anim_tables::{AnimationTable, FragmentEntry, parse_fragment};
use wce_foundation::math::Float4x4;
use wce_scene::render_scene::{SKINNED_NO_OFFSET, SkinnedInstance, SkinnedPart};

use crate::anim_pose::{inverse_binds, palette, world_pose};
use crate::unit_assembly::{AssembledUnit, Attach, Reader, assemble};
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

/// A clip on its skeleton, and its metadata.
#[derive(Clone, Debug, Default)]
pub struct Clip {
    pub anim: Animation,
    pub meta: Option<Meta>,
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
        Some(Clip { anim, meta })
    }

    /// The hand pose number in force at `t`: the last entry of `kind` that has started.
    fn hand_at(clip: &Clip, kind: &str, t: f32) -> Option<usize> {
        let entries = clip.meta.iter().flat_map(|m| &m.entries).filter(|e| e.kind == kind);
        entries.filter(|e| e.start <= t).max_by(|a, b| a.start.total_cmp(&b.start)).and_then(|e| e.u32(0)).map(|n| n as usize)
    }

    /// The bones' local transforms of a clip at `t` (seconds; it loops).
    pub fn locals(&self, clip: &Clip, t: f32) -> Vec<BoneLocal> {
        let duration = clip.anim.duration().max(1e-3);
        let t = t.rem_euclid(duration);
        let mut locals = clip.anim.sample(t);
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
    pub clip: Clip,
}

impl Figure {
    fn load(read: &Reader, catalog: &UnitCatalog, table: &str, variant: &str, seed: u64, slots: &[&str]) -> Result<Self, String> {
        let table = catalog.tables.get(table).ok_or_else(|| format!("no animation table '{table}'"))?;
        let set = AnimationSet::load(read, table);
        let mut rig = Rig::load(read, &set).ok_or_else(|| format!("skeleton '{}' does not load", set.skeleton))?;
        let parts = assemble(read, &variant_path(variant), seed).ok_or_else(|| format!("variant '{variant}' does not load"))?;
        let entry = slots.iter().find_map(|s| set.slots.get(*s).and_then(|e| e.first())).cloned().ok_or_else(|| format!("none of the slots {slots:?} in table '{}'", table.name))?;
        let mut clip = rig.clip(read, &entry).ok_or_else(|| format!("clip '{}' does not load", entry.filename))?;

        // A table's skeleton isn't always its clips': a mod's rider is on one of its own. The clip knows.
        let own = clip.anim.skeleton.clone();
        if own != rig.skeleton.skeleton && let Some(other) = Rig::of_skeleton(read, &set, &own) {
            rig = other;
            clip = rig.clip(read, &entry).ok_or_else(|| format!("clip '{}' does not load", entry.filename))?;
        }

        Ok(Self { parts, rig, set, clip })
    }

    fn instance(&self, t: f32, faction_colours: [[f32; 3]; 4], casts_shadow: bool) -> SkinnedInstance {
        let (palette, bones) = self.rig.pose(&self.rig.locals(&self.clip, t));
        let parts = self.parts.parts.iter().map(|p| {
            let (bone, offset) = match &p.attach {
                Attach::None => (None, SKINNED_NO_OFFSET),
                Attach::Bone(name) => (self.rig.bone(name), SKINNED_NO_OFFSET),
                Attach::Point { bone, matrix } => (Some(*bone), *matrix),
            };

            SkinnedPart { model: p.model.clone(), tint: p.tint, decal_diffuse: p.decal_diffuse.clone(), decal_normal: p.decal_normal.clone(), masks: p.masks, bone, offset }
        }).collect();
        SkinnedInstance { parts, palette, bones, casts_shadow, faction_colours, faction_scale: 0.0 }
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
        let man = Figure::load(read, catalog, &land.man_animation, &entry.variant, seed, if mount.is_some() { &["RIDER_STAND", "STAND"] } else { &["STAND", "MISSING_ANIM"] })?;
        let mount = mount.map(|m| Figure::load(read, catalog, &m.animation, &m.variant, seed.wrapping_add(1), &["STAND", "MISSING_ANIM"])).transpose()?;

        // Evenly over the variation, either way. **ASSUMPTION**: the engine's distribution isn't traced.
        let spread = ((seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40) as f32 / (1u64 << 24) as f32).mul_add(2.0, -1.0);
        let scale = entry.height_scale.max(0.01) * (1.0 + entry.height_variation * spread);
        let white = [[1.0; 3], [1.0; 3], [1.0; 3], [0.0; 3]];
        Ok(Self { man, mount, scale, faction_colours: catalog.colours.get(faction).copied().unwrap_or(white) })
    }

    /// The models, for waiting on them.
    pub fn models(&self) -> Vec<String> {
        self.man.parts.parts.iter().chain(self.mount.iter().flat_map(|m| &m.parts.parts)).map(|p| p.model.clone()).collect()
    }

    /// What the scene draws at `t` (seconds): each figure with its scale. A man and his mount share one frame and one origin: their clips are
    /// made so.
    pub fn instances(&self, t: f32, casts_shadow: bool) -> Vec<(f32, SkinnedInstance)> {
        let man = (self.scale, self.man.instance(t, self.faction_colours, casts_shadow));
        std::iter::once(man).chain(self.mount.iter().map(|m| (1.0, m.instance(t, self.faction_colours, casts_shadow)))).collect()
    }
}

#[cfg(test)]
mod tests {
    use wce_formats::anim_meta::{Meta, MetaEntry, MetaField};

    use super::{Clip, Rig, hand_side};

    #[test]
    fn fingers_are_told_from_the_hand() {
        assert_eq!(hand_side("bn_lefthandring2"), Some(true));
        assert_eq!(hand_side("bn_righthandthumb1"), Some(false));
        assert_eq!((hand_side("bn_lefthand"), hand_side("bn_head")), (None, None));
    }

    #[test]
    fn the_hand_pose_is_the_last_one_started() {
        let entry = |start: f32, pose: u32| MetaEntry { kind: "LHAND_POSE".to_owned(), version: Some(2), start, end: start, tag: String::new(), fields: vec![MetaField::U32(pose), MetaField::F32(1.0)] };
        let clip = Clip { meta: Some(Meta { version: 2, entries: vec![entry(0.0, 3), entry(0.5, 6)] }), ..Clip::default() };
        assert_eq!((Rig::hand_at(&clip, "LHAND_POSE", 0.2), Rig::hand_at(&clip, "LHAND_POSE", 0.7)), (Some(3), Some(6)));
        assert_eq!(Rig::hand_at(&clip, "RHAND_POSE", 0.7), None);
    }
}
