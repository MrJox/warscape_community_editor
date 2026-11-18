//! The texture rules in a tile (docs/plans/auto_detail.md, A): the tile's rules (`TileProject::texture_rules`, their own file beside
//! tile.wce) and the blend layer they make, owned by them (`texture_rules::OWNER`), under the user's layers so that hand paint goes over
//! it.
//!
//! The layer is made again **live**, as the roads' layers are (`roads.rs`): when what it is made from (the rules, the tile's texture
//! groups, the heights as they show) is not what its owner's signature says, and that stayed so for a look (a stroke or a drag waits for
//! its end), a job off the editor's thread makes it over a copy of the blend stack; the result goes in when the stack is still as it was
//! taken, as no undo entry of its own: an undo puts the heights or the rules back, and the layer is made again from them. An edit of the
//! rules is an undo entry ([`TextureRulesCommand`]).

use std::any::Any;

use wce_editor_core::command::Command;
use wce_tile::terrain_layers::{Blend, Stack, TerrainLayers};
use wce_tile::texture_rules::{self, Fields, OWNER, RuleSet};

use super::{TERRAIN, Tile, TileDocument};

/// A job making the rules' layer, and what it was started from.
pub(crate) struct TextureRulesJob {
    job: super::jobs::Job<Stack<Blend>>,
    signature: u64,
    before: Stack<Blend>,
}

/// The owner of the layer made from what `signature` says.
fn owner_of(signature: u64) -> String {
    format!("{OWNER}#{signature:016x}")
}

fn signature_of(owner: &str) -> Option<u64> {
    u64::from_str_radix(owner.strip_prefix(OWNER)?.strip_prefix('#')?, 16).ok()
}

impl Tile {
    /// The tile's texture group slot names (red, green, blue, alpha), empty without an entry.
    fn texture_group_slots(&self) -> Vec<String> {
        self.project.entry.as_ref().map(|e| e.texture_group()).map(|g| vec![g.red, g.green, g.blue, g.alpha]).unwrap_or_default()
    }

    /// What the rules' layer is made from, hashed: the rules' text, the texture groups, the heights as they show.
    fn texture_rules_signature(&mut self) -> Option<u64> {
        let rules = self.project.texture_rules.as_ref()?;
        let hf = self.project.height.as_ref()?;
        let version = hf.version();
        if self.texture_rules_heights.0 != version {
            self.texture_rules_heights = (version, hf.region_hash(0.0, 0.0, f64::from(hf.width()), f64::from(hf.height())));
        }

        let mut h = 0xcbf2_9ce4_8422_2325u64;
        let mut eat = |bytes: &[u8]| {
            for &b in bytes {
                h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
            }
        };
        eat(&texture_rules::text(&RuleSet { preset: String::new(), rules: rules.rules.clone() }));
        for g in self.texture_group_slots() {
            eat(g.as_bytes());
            eat(b"/");
        }

        eat(&self.texture_rules_heights.1.to_le_bytes());
        eat(&self.project.unit_scale.to_le_bytes());
        Some(h)
    }

    /// Makes the rules' layer again when it is stale, or takes it away with the rules. `sync_terrain_layers` has run.
    pub(super) fn refresh_texture_rules(&mut self) {
        self.refresh_texture_rules_with(false);
    }

    /// [`Self::refresh_texture_rules`]; `wait`: what is being made is waited for, and what is to be made is made now (an export).
    pub(super) fn refresh_texture_rules_with(&mut self, wait: bool) {
        if self.stroke_open {
            return;
        }

        self.take_texture_rules(wait);
        let made = self
            .project
            .layers
            .as_ref()
            .and_then(|l| l.blend.as_ref())
            .and_then(|s| s.layers.iter().find(|l| l.owner.starts_with(OWNER)))
            .map(|l| signature_of(&l.owner));
        if self.project.texture_rules.is_none() {
            self.texture_rules_job = None;
            if made.is_some() {
                self.remove_texture_rules_layer();
            }

            return;
        }

        let Some(signature) = self.texture_rules_signature() else { return };
        if made == Some(Some(signature)) {
            self.texture_rules_seen = signature;
            return;
        }

        // Still changing (a drag, an undo after another): the next look.
        if signature != self.texture_rules_seen && !wait {
            self.texture_rules_seen = signature;
            return;
        }

        if self.texture_rules_job.as_ref().is_some_and(|j| j.signature == signature) {
            if wait {
                self.take_texture_rules(true);
            }

            return;
        }

        self.texture_rules_seen = signature;
        if let Err(why) = self.start_texture_rules(signature) {
            wce_foundation::log::warn(format!("texture rules: {why}"));
            return;
        }

        if wait {
            self.take_texture_rules(true);
        }
    }

    /// Starts making the rules' layer off the editor's thread, on a copy of the blend stack (its samples shared, cheap) and of the heights.
    fn start_texture_rules(&mut self, signature: u64) -> Result<(), String> {
        let groups = self.texture_group_slots();
        let p = &mut self.project;
        p.load_blend()?;
        let rules = p.texture_rules.as_ref().map(|r| r.rules.clone()).unwrap_or_default();
        let (border, ex, ez, mx, mz, unit) = (p.border_m(), p.extent_x(), p.extent_z(), p.mpp_x(), p.mpp_z(), p.unit_scale);
        let (Some(hf), Some(bf)) = (p.height.as_ref(), p.blend.as_ref()) else { return Err("the tile has no height map".to_owned()) };
        let fields = Fields::from_height_field(hf, (mx, mz), unit);
        let (bw, bh) = (bf.width(), bf.height());
        let ppm = (f64::from(bw) / (ex + 2.0 * border), f64::from(bh) / (ez + 2.0 * border));
        let layers = p.layers.get_or_insert_with(TerrainLayers::default);
        let id = layers.new_id();
        let mut blend = layers.blend.get_or_insert_with(|| Stack::new(bf)).clone();
        let before = blend.clone();
        let job = super::jobs::Job::spawn("texture rules", move || {
            wce_foundation::wce_prof!("tile.texture_rules");
            blend.layers.retain(|l| !l.owner.starts_with(OWNER));
            let id = blend.add(id, "Texture rules", &owner_of(signature));
            texture_rules::generate(&rules, &groups, &fields, ppm, [0, 0, bw, bh], &mut |x, y, px| blend.put(id, x, y, &px));
            blend
        });
        self.texture_rules_job = Some(TextureRulesJob { job, signature, before });
        Ok(())
    }

    /// The rules' layer made off the editor's thread, put in place of the stack it was made from, and the map composed again over where
    /// it was and is: when the stack is still as it was taken. Edited meanwhile, the job is let go and the layer is made again.
    fn take_texture_rules(&mut self, wait: bool) {
        let Some(j) = &self.texture_rules_job else { return };
        let made = match if wait { j.job.wait() } else { j.job.poll() } {
            super::jobs::Poll::Running => return,
            super::jobs::Poll::Done(m) => Some(m),
            super::jobs::Poll::Failed => None,
        };

        let j = self.texture_rules_job.take().expect("a job");
        let Some(blend) = made else {
            wce_foundation::log::warn("texture rules: their layer could not be made");
            return;
        };

        let p = &mut self.project;
        let (Some(bf), Some(layers)) = (p.blend.as_mut(), p.layers.as_mut()) else { return };
        if !layers.blend.as_ref().is_some_and(|b| b.unchanged_since(&j.before)) {
            return;
        }

        let rect = rules_rect(&j.before, &blend);
        layers.blend = Some(blend);
        layers.dirty = true;
        if let Some(s) = layers.blend.as_mut() {
            if let Some(r) = rect {
                s.apply(bf, r);
            }

            s.remember();
        }

        // The game and the compile read the map, not the layers.
        p.blend_dirty |= rect.is_some();
        self.layers_shown = true;
        self.reload_grass();
    }

    /// Takes the rules' layer away (the rules went).
    fn remove_texture_rules_layer(&mut self) {
        let p = &mut self.project;
        let (Some(bf), Some(layers)) = (p.blend.as_mut(), p.layers.as_mut()) else { return };
        let Some(s) = layers.blend.as_mut() else { return };
        let before = s.clone();
        s.layers.retain(|l| !l.owner.starts_with(OWNER));
        if let Some(r) = rules_rect(&before, s) {
            s.apply(bf, r);
            p.blend_dirty = true;
        }

        s.remember();
        layers.dirty = true;
        self.layers_shown = true;
        self.reload_grass();
    }
}

/// Where the rules' layers of two stacks hold samples.
fn rules_rect(old: &Stack<Blend>, new: &Stack<Blend>) -> Option<[u32; 4]> {
    old.layers
        .iter()
        .chain(&new.layers)
        .filter(|l| l.owner.starts_with(OWNER))
        .filter_map(|l| l.extent())
        .reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])
}

impl TileDocument {
    /// The tile's texture rules, if it has any.
    #[must_use]
    pub fn texture_rules(&self) -> Option<&RuleSet> {
        self.project().texture_rules.as_ref()
    }

    /// Sets the tile's texture rules (`None`: takes them and their layer away). One undo entry; the layer follows live.
    pub fn set_texture_rules(&mut self, rules: Option<RuleSet>) {
        if self.texture_rules() == rules.as_ref() {
            return;
        }

        self.end_stroke();
        let before = self.texture_rules().cloned();
        let name = match &rules {
            None => "Remove texture rules".to_owned(),
            Some(_) if before.is_none() => "Add texture rules".to_owned(),
            Some(_) => "Edit texture rules".to_owned(),
        };
        self.history.push(Box::new(TextureRulesCommand { name, before, after: rules }));
    }

    /// The rules' layer made now, waiting for one being made (an export; scripts). The edits not yet in the layers go in first: the map
    /// is composed again from the stack.
    pub fn finish_texture_rules(&mut self) {
        let t = self.tile_mut();
        t.sync_terrain_layers();
        t.refresh_texture_rules_with(true);
    }

    /// The rules whose texture group the tile does not have (they lay nothing), by name.
    #[must_use]
    pub fn texture_rules_missing_groups(&self) -> Vec<String> {
        let Some(set) = self.texture_rules() else { return vec![] };
        let groups = self.tile().texture_group_slots();
        texture_rules::missing_groups(&set.rules, &groups).into_iter().map(|r| format!("{} ({})", r.name, r.group)).collect()
    }
}

/// An edit of the tile's texture rules.
pub(crate) struct TextureRulesCommand {
    name: String,
    before: Option<RuleSet>,
    after: Option<RuleSet>,
}

impl Command<Tile> for TextureRulesCommand {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn apply(&mut self, t: &mut Tile) {
        t.project.texture_rules.clone_from(&self.after);
        t.project.texture_rules_dirty = true;
    }

    fn revert(&mut self, t: &mut Tile) {
        t.project.texture_rules.clone_from(&self.before);
        t.project.texture_rules_dirty = true;
    }

    fn dirty_items(&self) -> Vec<u64> {
        vec![TERRAIN]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
