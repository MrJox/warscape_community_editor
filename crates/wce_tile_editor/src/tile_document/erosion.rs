//! The erosion in a tile (docs/plans/auto_detail_2.md, D): its settings (`TileProject::erosion`, their own file beside tile.wce) and the
//! height layer they make, owned by them (`erosion::OWNER`), over the user's layers and under the procedural features' (roads, walls,
//! prefabs' ground lay their own ground over it).
//!
//! The layer is made again **live**, as the texture rules' is: when what it is made from (the settings, the ground under it, the buildings
//! it keeps off) is not what its owner's signature says, and that stayed so for a look (a stroke or a drag waits for its end), a job off
//! the editor's thread makes it over a copy of the height stack; the result goes in when the stack is still as it was taken, as no undo
//! entry of its own. An edit of the settings is an undo entry ([`ErosionCommand`]). Where the water ran and where it laid its load stay
//! with the tile for the texture rules ([`ErosionMade`]).

use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use wce_editor_core::command::Command;
use wce_tile::erosion::{self, OWNER, Settings};
use wce_tile::terrain_layers::{Height, Stack, TerrainLayers};

use super::{TERRAIN, Tile, TileDocument};

/// The rank of the erosion's layer in its stack (`terrain_layers::order_of`): what is under it is what it works from.
const RANK: u8 = 2;

/// How far from a building's model the ground is kept as it is (metres), and over how much more the erosion fades in.
const BUILDING_KEEP: f32 = 2.0;
const BUILDING_FADE: f32 = 4.0;

/// A job making the erosion's layer, what it was started from, and its stop flag (set when it is let go).
pub(crate) struct ErosionJob {
    job: super::jobs::Job<Option<(Stack<Height>, ErosionMade)>>,
    signature: u64,
    before: Stack<Height>,
    stop: Arc<AtomicBool>,
}

impl Drop for ErosionJob {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// What the erosion made besides its layer: where the water ran (0 .. 1) and what it laid (metres), on the height map's grid, and the
/// owner of the layer it goes with.
#[derive(Clone)]
pub(crate) struct ErosionMade {
    pub owner: String,
    pub w: u32,
    pub h: u32,
    pub flow: Arc<Vec<f32>>,
    pub deposit: Arc<Vec<f32>>,
}

fn fnv(h: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *h = (*h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
}

/// The owner of the layer made from what `signature` says.
fn owner_of(signature: u64) -> String {
    format!("{OWNER}#{signature:016x}")
}

fn signature_of(owner: &str) -> Option<u64> {
    u64::from_str_radix(owner.strip_prefix(OWNER)?.strip_prefix('#')?, 16).ok()
}

impl Tile {
    /// The buildings the erosion keeps off (x, z, radius; world metres), the texture rules', cached by the scene's version.
    fn erosion_buildings(&mut self) -> Arc<Vec<[f64; 3]>> {
        let mut key = 0xcbf2_9ce4_8422_2325u64;
        for v in [self.history_version, self.structure_version, self.models.len() as u64, self.unresolved.len() as u64] {
            fnv(&mut key, &v.to_le_bytes());
        }

        if let Some((k, b)) = &self.erosion_buildings
            && *k == key
        {
            return b.clone();
        }

        let b = Arc::new(self.texture_rules_buildings());
        self.erosion_buildings = Some((key, b.clone()));
        b
    }

    /// What the erosion's layer is made from, hashed: the settings, the ground under its layer, the buildings it keeps off, the frame.
    fn erosion_signature(&mut self) -> Option<u64> {
        let settings = self.project.erosion.clone()?;
        let buildings = self.erosion_buildings();
        let stack = self.project.layers.as_ref().and_then(|l| l.height.as_ref());
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        fnv(&mut h, erosion::settings_text(true, &settings).as_bytes());
        for v in buildings.iter().flatten() {
            fnv(&mut h, &v.to_le_bytes());
        }

        fnv(&mut h, &self.project.unit_scale.to_le_bytes());
        match stack {
            Some(s) => fnv(&mut h, &s.identity_under_rank(RANK).to_le_bytes()),
            // No stack yet: the field is all there is (its version says when it changes).
            None => {
                let hf = self.project.height.as_ref()?;
                let (a, b) = hf.version();
                fnv(&mut h, &(a as u64).to_le_bytes());
                fnv(&mut h, &b.to_le_bytes());
            }
        }

        Some(h)
    }

    /// Makes the erosion's layer again when it is stale, or takes it away with the settings. `sync_terrain_layers` has run; the prefabs'
    /// ground and the roads come after (they lay their ground over it).
    pub(super) fn refresh_erosion(&mut self) {
        self.refresh_erosion_with(false);
    }

    /// [`Self::refresh_erosion`]; `wait`: what is being made is waited for, and what is to be made is made now (an export).
    pub(super) fn refresh_erosion_with(&mut self, wait: bool) {
        if self.stroke_open {
            return;
        }

        self.take_erosion(wait);
        let made = self
            .project
            .layers
            .as_ref()
            .and_then(|l| l.height.as_ref())
            .and_then(|s| s.layers.iter().find(|l| l.owner.starts_with(OWNER)))
            .map(|l| signature_of(&l.owner));
        if self.project.erosion.is_none() {
            self.erosion_job = None;
            self.erosion_made = None;
            if made.is_some() {
                self.remove_erosion_layer();
            }

            return;
        }

        let Some(signature) = self.erosion_signature() else { return };
        // Made, and what it made besides the layer known (a tile opened again has the layer, not where the water ran: made again).
        if made == Some(Some(signature)) && self.erosion_made.as_ref().is_some_and(|m| m.owner == owner_of(signature)) {
            self.erosion_seen = signature;
            return;
        }

        // Still changing (a drag, an undo after another): the next look.
        if signature != self.erosion_seen && !wait {
            self.erosion_seen = signature;
            return;
        }

        if self.erosion_job.as_ref().is_some_and(|j| j.signature == signature) {
            if wait {
                self.take_erosion(true);
            }

            return;
        }

        self.erosion_seen = signature;
        if let Err(why) = self.start_erosion(signature) {
            wce_foundation::log::warn(format!("erosion: {why}"));
            return;
        }

        if wait {
            self.take_erosion(true);
        }
    }

    /// Starts making the erosion's layer off the editor's thread, on a copy of the height stack (its samples shared, cheap).
    fn start_erosion(&mut self, signature: u64) -> Result<(), String> {
        let settings = self.project.erosion.clone().ok_or("no erosion")?;
        let buildings = self.erosion_buildings();
        let p = &mut self.project;
        let (border, unit) = (p.border_m(), p.unit_scale);
        let (mx, mz) = (p.mpp_x(), p.mpp_z());
        let Some(hf) = p.height.as_ref() else { return Err("the tile has no height map".to_owned()) };
        let layers = p.layers.get_or_insert_with(TerrainLayers::default);
        let id = layers.new_id();
        let stack = layers.height.get_or_insert_with(|| Stack::new(hf)).clone();
        let before = stack.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let job = super::jobs::Job::spawn("erosion", move || {
            wce_foundation::wce_prof!("tile.erosion");
            let mut stack = stack;
            let (w, h) = stack.size();
            let (wu, hu) = (w as usize, h as usize);
            let mut px = [0.0f32];
            let mut ground = Vec::with_capacity(wu * hu);
            for y in 0..h {
                for x in 0..w {
                    stack.under_rank(RANK, x, y, &mut px);
                    ground.push((f64::from(px[0]) / unit) as f32);
                }
            }

            let mask = building_mask(&buildings, wu, hu, (mx, mz), border);
            // The drops step a sample at a time: on the finer of the two spacings (the maps are square in practice).
            let mpp = mx.min(mz) as f32;
            let eroded = erosion::erode(&ground, wu, hu, mpp, &mask, &settings, &|| stopped.load(Ordering::Relaxed))?;
            let owner = owner_of(signature);
            stack.layers.retain(|l| !l.owner.starts_with(OWNER));
            let id = stack.add(id, "Erosion", &owner);
            for y in 0..h {
                for x in 0..w {
                    let d = eroded.delta[y as usize * wu + x as usize];
                    // Left empty where nothing changed: its blocks stay unallocated.
                    if d.abs() > 1e-4 {
                        stack.put(id, x, y, &[(f64::from(d) * unit) as f32]);
                    }
                }
            }

            let made = ErosionMade { owner, w, h, flow: Arc::new(eroded.flow), deposit: Arc::new(eroded.deposit) };
            Some((stack, made))
        });
        self.erosion_job = Some(ErosionJob { job, signature, before, stop });
        Ok(())
    }

    /// The erosion's layer made off the editor's thread, put in place of the stack it was made from, and the height map composed again
    /// over where it was and is: when the stack is still as it was taken. Edited meanwhile, the job is let go and the layer made again.
    fn take_erosion(&mut self, wait: bool) {
        let Some(j) = &self.erosion_job else { return };
        let made = match if wait { j.job.wait() } else { j.job.poll() } {
            super::jobs::Poll::Running => return,
            super::jobs::Poll::Done(m) => m,
            super::jobs::Poll::Failed => None,
        };

        let j = self.erosion_job.take().expect("a job");
        let Some((stack, made)) = made else {
            if !j.stop.load(Ordering::Relaxed) {
                wce_foundation::log::warn("erosion: its layer could not be made");
            }
            return;
        };

        let p = &mut self.project;
        let (Some(hf), Some(layers)) = (p.height.as_mut(), p.layers.as_mut()) else { return };
        if !layers.height.as_ref().is_some_and(|s| s.unchanged_since(&j.before)) {
            return;
        }

        let rect = erosion_rect(&j.before, &stack);
        layers.height = Some(stack);
        layers.dirty = true;
        if let Some(s) = layers.height.as_mut() {
            if let Some(r) = rect {
                s.apply(hf, r);
            }

            s.remember();
        }

        // The compile and the game read the map, not the layers; the roads and the prefabs' ground lie on it.
        p.height_dirty |= rect.is_some();
        self.erosion_made = Some(made);
        self.roads_stale = true;
        self.prefab_ground_stale = true;
        self.layers_shown = true;
        if rect.is_some() {
            self.reload_grass();
            self.reload_tree_list();
        }
    }

    /// Takes the erosion's layer away (the settings went).
    fn remove_erosion_layer(&mut self) {
        let p = &mut self.project;
        let (Some(hf), Some(layers)) = (p.height.as_mut(), p.layers.as_mut()) else { return };
        let Some(s) = layers.height.as_mut() else { return };
        let before = s.clone();
        s.layers.retain(|l| !l.owner.starts_with(OWNER));
        if let Some(r) = erosion_rect(&before, s) {
            s.apply(hf, r);
            p.height_dirty = true;
        }

        s.remember();
        layers.dirty = true;
        self.roads_stale = true;
        self.prefab_ground_stale = true;
        self.layers_shown = true;
        self.reload_grass();
        self.reload_tree_list();
    }
}

/// How much the erosion may change each sample of a `w` × `h` height map: 0 on and near the buildings, fading in past them.
fn building_mask(buildings: &[[f64; 3]], w: usize, h: usize, (mx, mz): (f64, f64), border: f64) -> Vec<f32> {
    let mut mask = vec![1.0f32; w * h];
    let (keep, fade) = (f64::from(BUILDING_KEEP), f64::from(BUILDING_FADE));
    for &[bx, bz, r] in buildings {
        let reach = r + keep + fade;
        let (cx, cz) = ((bx + border) / mx, (bz + border) / mz);
        let (x0, x1) = (((cx - reach / mx).floor().max(0.0)) as usize, ((cx + reach / mx).ceil().max(0.0) as usize).min(w));
        let (y0, y1) = (((cz - reach / mz).floor().max(0.0)) as usize, ((cz + reach / mz).ceil().max(0.0) as usize).min(h));
        for y in y0..y1 {
            for x in x0..x1 {
                let d = ((x as f64 * mx - border - bx).powi(2) + (y as f64 * mz - border - bz).powi(2)).sqrt();
                let m = ((d - r - keep) / fade).clamp(0.0, 1.0) as f32;
                let k = y * w + x;
                mask[k] = mask[k].min(m);
            }
        }
    }

    mask
}

/// Where the erosion's layers of two stacks hold samples.
fn erosion_rect(old: &Stack<Height>, new: &Stack<Height>) -> Option<[u32; 4]> {
    old.layers
        .iter()
        .chain(&new.layers)
        .filter(|l| l.owner.starts_with(OWNER))
        .filter_map(|l| l.extent())
        .reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])
}

impl TileDocument {
    /// The tile's erosion settings, if it has erosion.
    #[must_use]
    pub fn erosion(&self) -> Option<&Settings> {
        self.project().erosion.as_ref()
    }

    /// Sets the tile's erosion (`None`: takes it and its layer away). One undo entry; the layer follows live.
    pub fn set_erosion(&mut self, settings: Option<Settings>) {
        if self.erosion() == settings.as_ref() {
            return;
        }

        self.end_stroke();
        let before = self.erosion().cloned();
        let name = match &settings {
            None => "Remove erosion".to_owned(),
            Some(_) if before.is_none() => "Add erosion".to_owned(),
            Some(_) => "Edit erosion".to_owned(),
        };
        self.history.push(Box::new(ErosionCommand { name, before, after: settings }));
    }

    /// The erosion's layer made now, waiting for one being made (an export; scripts).
    pub fn finish_erosion(&mut self) {
        let t = self.tile_mut();
        t.sync_terrain_layers();
        t.refresh_erosion_with(true);
    }

    /// Whether the erosion's layer is being made.
    #[must_use]
    pub fn erosion_busy(&self) -> bool {
        self.tile().erosion_job.is_some()
    }

    /// What the erosion's layer holds: how much ground it moved and the most it raised and lowered a sample (metres). `None` without it.
    #[must_use]
    pub fn erosion_summary(&self) -> Option<String> {
        let t = self.tile();
        let s = t.project.layers.as_ref()?.height.as_ref()?;
        let id = s.layers.iter().find(|l| l.owner.starts_with(OWNER))?.id;
        let unit = t.project.unit_scale;
        let (w, h) = s.size();
        let mut px = [0.0f32];
        let (mut changed, mut lo, mut hi) = (0usize, 0.0f64, 0.0f64);
        for y in 0..h {
            for x in 0..w {
                if s.get(id, x, y, &mut px) && px[0] != 0.0 {
                    let m = f64::from(px[0]) / unit;
                    changed += 1;
                    lo = lo.min(m);
                    hi = hi.max(m);
                }
            }
        }

        let streams = t.erosion_made.as_ref().map_or(0, |m| m.flow.iter().filter(|f| **f > 0.6).count());
        let n = (f64::from(w) * f64::from(h)).max(1.0);
        Some(format!(
            "erosion changed {:.1} % of the height map, from {lo:.2} m to {hi:.2} m; streams over {:.1} %",
            100.0 * changed as f64 / n,
            100.0 * streams as f64 / n
        ))
    }
}

/// An edit of the tile's erosion settings.
pub(crate) struct ErosionCommand {
    name: String,
    before: Option<Settings>,
    after: Option<Settings>,
}

impl Command<Tile> for ErosionCommand {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn apply(&mut self, t: &mut Tile) {
        t.project.erosion.clone_from(&self.after);
        t.project.erosion_dirty = true;
    }

    fn revert(&mut self, t: &mut Tile) {
        t.project.erosion.clone_from(&self.before);
        t.project.erosion_dirty = true;
    }

    fn dirty_items(&self) -> Vec<u64> {
        vec![TERRAIN]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
