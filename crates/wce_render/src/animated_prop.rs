//! A prop that moves: a weighted model with its own clip beside it (`<model>.anim`; the campaign's fauna:
//! `rigidmodels/campaign/fauna/campaign_deer_male.rigid_model_v2` and `.anim`, doves, gulls, whales...). The clip names its skeleton
//! (`animations/skeletons/<name>.anim`); the model plays it in a loop, its root's motion kept (a bird circles with it). Its meshes are
//! `Weighted` / `WeightedSkin`: drawn skinned (a static instance draws no `WeightedSkin` mesh).

use std::sync::Arc;

use wce_formats::anim::Animation;
use wce_formats::anim_tables::FragmentEntry;
use wce_scene::render_scene::{SKINNED_NO_OFFSET, SkinnedInstance, SkinnedPart};

use crate::unit_animation::{AnimationSet, Clip, Rig};
use crate::unit_assembly::Reader;

/// The clip of a model, by its path: the model's own, beside it.
pub fn clip_path(model: &str) -> Option<String> {
    model.strip_suffix(".rigid_model_v2").map(|stem| format!("{stem}.anim"))
}

/// A model and its clip on its skeleton.
#[derive(Clone, Debug)]
pub struct AnimatedProp {
    model: String,
    rig: Rig,
    clip: Arc<Clip>,
}

impl AnimatedProp {
    /// A model with a clip beside it whose skeleton loads; none otherwise.
    pub fn load(read: &Reader, model: &str) -> Option<Self> {
        let path = clip_path(model)?;
        let head = Animation::decode(&read(&path)?, 1)?;
        let set = AnimationSet { skeleton: head.skeleton.clone(), slots: Default::default() };
        let rig = Rig::load(read, &set)?;
        let clip = rig.clip(read, &FragmentEntry { filename: path, ..FragmentEntry::default() })?;
        Some(Self { model: model.to_owned(), rig, clip: Arc::new(clip) })
    }

    /// Its length (seconds).
    pub fn duration(&self) -> f32 {
        self.clip.anim.duration().max(1e-3)
    }

    /// The model at `t` (seconds, looping).
    pub fn pose(&self, t: f32, casts_shadow: bool) -> SkinnedInstance {
        let locals = self.rig.locals(&self.clip, t.rem_euclid(self.duration()));
        let (palette, bones) = self.rig.pose(&locals);
        let part = SkinnedPart {
            model: self.model.clone(),
            tint: [0.0; 4],
            decal_diffuse: None,
            decal_normal: None,
            masks: [0, 1, 2],
            bone: None,
            offset: SKINNED_NO_OFFSET,
        };
        SkinnedInstance {
            parts: vec![part],
            palette,
            bones,
            casts_shadow,
            faction_colours: [[1.0; 3], [1.0; 3], [1.0; 3], [0.0; 3]],
            faction_scale: 0.0,
        }
    }
}
