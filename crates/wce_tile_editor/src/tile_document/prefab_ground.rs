//! The ground prefabs bring (docs/plans/prefab_tool.md, `wce_tile::prefab_terrain`): each reference to a prefab with a height layer owns a
//! height layer of the tile, made from the prefab's where the reference is. They are derived data, as the roads' (`roads.rs`): a reference
//! moving, turning, its settings, its prefab's layer or (normalized) the ground under it changing makes them again, so undo needs nothing of
//! their own. What a layer was made from is in its owner (`prefab:<n>#<signature>`, n its place among them), so opening a tile does not
//! make them again.
//!
//! They go over the user's layers and under the roads', a nested prefab's under the one it is in, otherwise in the scene's order; a road is
//! graded over them. They are not in the Tile panel's list: a reference's settings are its own attributes, which TEd never sees:
//! `wce_terrain="off"` (none), `wce_terrain_opacity` (0 to 1; a nested reference's is times the ones it is in) and `wce_terrain_feather`
//! (the prefab's feather, else).

use std::collections::BTreeMap;
use std::sync::Arc;

use wce_tile::prefab_terrain::{Grid, Placement, PrefabTerrain};
use wce_tile::project_files::PREFAB_ATTR;
use wce_tile::terrain_layers::{Stack, TerrainLayers};
use wce_tile::world_transform::world_transform;

use super::Tile;

/// A reference's ground switched off: `off`.
pub const TERRAIN_ATTR: &str = "wce_terrain";
pub const TERRAIN_OPACITY_ATTR: &str = "wce_terrain_opacity";
pub const TERRAIN_FEATHER_ATTR: &str = "wce_terrain_feather";

const OWNER: &str = "prefab:";

/// A reference that brings ground.
pub(crate) struct Ground {
    pub(crate) elem: usize,
    pub(crate) key: String,
    terrain: Arc<PrefabTerrain>,
    hash: u64,
    at: Placement,
    pub(crate) opacity: f32,
    pub(crate) feather: Option<f64>,
}

/// What some words hash to (the same in every run: it is saved).
fn fnv(h: u64, words: &[u64]) -> u64 {
    words.iter().flat_map(|w| w.to_le_bytes()).fold(h, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01B3))
}

const FNV_START: u64 = 0xCBF2_9CE4_8422_2325;

fn owner_of(n: usize, signature: u64) -> String {
    format!("{OWNER}{n}#{signature:x}")
}

/// The place and signature in a layer's owner.
fn parse_owner(owner: &str) -> Option<(usize, u64)> {
    let (n, signature) = owner.strip_prefix(OWNER)?.split_once('#')?;
    Some((n.parse().ok()?, u64::from_str_radix(signature, 16).ok()?))
}

fn is_prefab_layer(owner: &str) -> bool {
    owner.starts_with(OWNER)
}

/// A number attribute.
fn number(doc: &wce_formats::xml_doc::XmlDoc, e: usize, attr: &str) -> Option<f64> {
    doc.get(e, attr).and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite())
}

impl Tile {
    /// The references that bring ground, in the order their layers go: the ones inside another first (an element ends after what is in
    /// it), then the scene's.
    pub(crate) fn prefab_grounds(&self) -> Vec<Ground> {
        let Some(source) = self.project.prefab_ground.clone() else { return vec![] };
        let sc = self.tscene();
        let doc = sc.doc();
        let els = doc.elements();
        let is_ref = |g: usize| els[g].name == "UserInstanceGroup" && doc.get(g, PREFAB_ATTR).is_some_and(|k| !k.is_empty());
        let mut out = vec![];
        for e in sc.find_all("UserInstanceGroup") {
            if !is_ref(e) {
                continue;
            }

            // Off or faded along this one and the references it is in.
            let (mut off, mut opacity) = (false, 1.0f64);
            let mut at = Some(e);
            while let Some(g) = at {
                if is_ref(g) {
                    off |= doc.get(g, TERRAIN_ATTR).is_some_and(|v| v == "off");
                    opacity *= number(doc, g, TERRAIN_OPACITY_ATTR).unwrap_or(1.0).clamp(0.0, 1.0);
                }

                at = els[g].parent;
            }

            if off || opacity <= 0.0 {
                continue;
            }

            let key = doc.get(e, PREFAB_ATTR).unwrap_or_default();
            let Some((hash, terrain)) = (source.0)(&key) else { continue };
            let w = world_transform(sc, e);
            let feather = number(doc, e, TERRAIN_FEATHER_ATTR).map(|f| f.max(0.0));
            out.push(Ground { elem: e, key, terrain, hash, at: Placement::from_transform(w.position, &w.rotation), opacity: opacity as f32, feather });
        }

        out.sort_by_key(|g| els[g.elem].end_off);
        out
    }

    /// What a reference's layer is made from: its prefab's layer, where it is, its settings and, normalized, the ground under it (what lies
    /// below its layer `id`, when it has one).
    fn ground_signature(&self, g: &Ground, grid: &Grid, below_of: Option<u64>) -> u64 {
        let at = g.at;
        let (cx, cz) = at.to_scene(1.0, 0.0);
        let mut h = fnv(FNV_START, &[g.hash, at.x.to_bits(), at.z.to_bits(), cx.to_bits(), cz.to_bits(), u64::from(g.opacity.to_bits()), g.feather.map_or(u64::MAX, f64::to_bits)]);
        h = fnv(h, &g.key.to_lowercase().bytes().map(u64::from).collect::<Vec<_>>());
        if !g.terrain.normalize {
            return h;
        }

        // The ground it stands on, to the millimetre, over what its layer can reach.
        let (Some(stack), Some(id)) = (self.project.layers.as_ref().and_then(|l| l.height.as_ref()), below_of) else { return h };
        let [x0, y0, x1, y1] = g.terrain.reach(&at, grid, g.feather);
        let mut px = [0.0f32];
        let mut words = Vec::with_capacity(((x1 - x0) * (y1 - y0)) as usize);
        for y in y0..y1 {
            for x in x0..x1 {
                stack.below(id, x, y, &mut px);
                words.push((f64::from(px[0]) * 1000.0).round() as i64 as u64);
            }
        }

        fnv(h, &words)
    }

    /// Makes again the layers of the references whose ground changed, and takes out those of references that went. Waits for an open
    /// stroke. `sync_terrain_layers` has run; the roads come after.
    pub(super) fn refresh_prefab_ground(&mut self) {
        if self.stroke_open {
            return;
        }

        let has_layers = self.project.layers.as_ref().and_then(|l| l.height.as_ref()).is_some_and(|s| s.layers.iter().any(|l| is_prefab_layer(&l.owner)));
        if self.project.prefab_ground.is_none() && !has_layers {
            return;
        }

        // Looked at again only when something could have changed it.
        let version = self.project.height.as_ref().map_or(0, |f| f.version().1);
        let seen = fnv(FNV_START, &[self.history_version, self.structure_version, self.edits_seen, version, u64::from(self.prefab_ground_stale)]);
        if seen == self.prefab_ground_seen {
            return;
        }

        let Some(grid) = Grid::of(&self.project) else { return };
        let grounds = self.prefab_grounds();
        let made = self.project.layers.as_ref().and_then(|l| l.height.as_ref()).map(|s| s.layers.iter().filter(|l| is_prefab_layer(&l.owner)).map(|l| (l.id, l.owner.clone())).collect::<Vec<_>>()).unwrap_or_default();
        let wanted = grounds.iter().enumerate().map(|(n, g)| {
            let below_of = made.iter().find(|(_, o)| parse_owner(o).is_some_and(|(m, _)| m == n)).map(|(id, _)| *id);
            owner_of(n, self.ground_signature(g, &grid, below_of))
        }).collect::<Vec<_>>();

        let current = made.iter().map(|(_, o)| o.clone()).collect::<Vec<_>>();
        if current != wanted || self.prefab_ground_stale {
            if let Err(why) = self.make_prefab_ground(&grounds, &grid) {
                wce_foundation::log::warn(format!("prefab ground: {why}"));
            }

            self.layers_shown = true;
            self.roads_stale = true;
            self.prefab_ground_stale = false;
        }

        let version = self.project.height.as_ref().map_or(0, |f| f.version().1);
        self.prefab_ground_seen = fnv(FNV_START, &[self.history_version, self.structure_version, self.edits_seen, version, 0]);
    }

    fn make_prefab_ground(&mut self, grounds: &[Ground], grid: &Grid) -> Result<(), String> {
        wce_foundation::wce_prof!("tile.make_prefab_ground");
        let p = &mut self.project;
        let Some(hf) = p.height.as_mut() else { return Err("the tile has no height map".to_owned()) };
        let layers = p.layers.get_or_insert_with(TerrainLayers::default);
        let ids = grounds.iter().map(|_| layers.new_id()).collect::<Vec<_>>();
        layers.dirty = true;
        let height = layers.height.get_or_insert_with(|| Stack::new(hf));
        height.layers.retain(|l| !is_prefab_layer(&l.owner));
        height.put_owned_on_top();

        // Each over the ones before it, all under what other features own (the roads).
        let first = height.layers.iter().position(|l| !l.owner.is_empty()).unwrap_or(height.layers.len());
        let mut owners = vec![];
        for (n, (g, &id)) in grounds.iter().zip(&ids).enumerate() {
            height.add(id, &format!("Prefab {}", g.key), "pending");
            let layer = height.layers.pop().expect("the layer just added");
            height.layers.insert(first + n, layer);

            let mut out = BTreeMap::new();
            let ground = |x: u32, y: u32| {
                let mut px = [0.0f32];
                height.below(id, x, y, &mut px);
                f64::from(px[0])
            };

            g.terrain.rasterize(&g.at, grid, g.feather, &ground, &mut out);
            for ((x, y), v) in out {
                height.put(id, x, y, &[v]);
            }

            if let Some(l) = height.layers.iter_mut().find(|l| l.id == id) {
                l.opacity = g.opacity;
            }

            owners.push((n, id));
        }

        height.apply_all(hf);

        // The owners last: a signature looks at what lies below each layer, which is there now.
        for (n, id) in owners {
            let below_of = Some(id);
            let sig = self.ground_signature(&grounds[n], grid, below_of);
            if let Some(l) = self.project.layers.as_mut().and_then(|l| l.height.as_mut()).and_then(|s| s.layers.iter_mut().find(|l| l.id == id)) {
                l.owner = owner_of(n, sig);
            }
        }

        if let Some(s) = self.project.layers.as_mut().and_then(|l| l.height.as_mut()) {
            s.remember();
        }

        Ok(())
    }
}

impl super::TileDocument {
    /// The Inspector's rows of a reference's ground: whether it brings it, how much, the feather when normalized. A value being dragged is
    /// a draft, set (one undo entry) when the drag ends.
    pub(crate) fn prefab_ground_rows(&mut self, e: usize, ins: &mut dyn wce_editor_core::inspector::Inspector) {
        let t = self.tile();
        let doc = t.tscene().doc();
        let key = doc.get(e, PREFAB_ATTR).unwrap_or_default();
        let Some(source) = t.project.prefab_ground.clone() else { return };
        let Some((_, terrain)) = (source.0)(&key) else {
            ins.text("ground", "ground", "none (the prefab has no height layer)");
            return;
        };

        ins.section("Ground");
        let mut on = doc.get(e, TERRAIN_ATTR).is_none_or(|v| v != "off");
        let (w, h) = (f64::from(terrain.width) * terrain.spacing, f64::from(terrain.height) * terrain.spacing);
        let mode = if terrain.normalize { "normalized: level under it, feathered into the ground" } else { "adds to the ground under it" };
        let opacity = number(doc, e, TERRAIN_OPACITY_ATTR).unwrap_or(1.0).clamp(0.0, 1.0);
        let feather = number(doc, e, TERRAIN_FEATHER_ATTR);
        if ins.edit_bool("ground_on", "brings ground", &mut on) {
            self.set_attribute(e, TERRAIN_ATTR, if on { "on" } else { "off" });
        }

        ins.text("ground_mode", "mode", mode);
        ins.text("ground_size", "size", &format!("{w:.1} x {h:.1}"));

        // Opacity and feather: drafts while dragged.
        let draft = |d: &mut Self, attr: &str, now: f64, edit: &mut dyn FnMut(&mut dyn wce_editor_core::inspector::Inspector, &mut f32) -> bool, ins: &mut dyn wce_editor_core::inspector::Inspector| {
            let k = (e, attr.to_owned());
            let mut v = d.tile().drafts.get(&k).and_then(|s| s.parse::<f32>().ok()).unwrap_or(now as f32);
            if edit(ins, &mut v) {
                d.tile_mut().drafts.insert(k.clone(), wce_tile::ted_scene::format_number(f64::from(v)));
            }

            if ins.gesture_ended() && let Some(v) = d.tile_mut().drafts.remove(&k) {
                d.set_attribute(e, attr, &v);
            }
        };

        draft(self, TERRAIN_OPACITY_ATTR, opacity, &mut |ins, v| ins.edit_slider("ground_opacity", "opacity", v, 0.0, 1.0), ins);
        if terrain.normalize {
            draft(self, TERRAIN_FEATHER_ATTR, feather.unwrap_or(terrain.feather), &mut |ins, v| ins.edit_float("ground_feather", "feather", v, 0.1, 0.0, 100.0), ins);
            if feather.is_some() {
                ins.mark_fields(vec![("ground_feather".to_owned(), format!("Override: the prefab's is {}", wce_tile::ted_scene::format_number(terrain.feather)))]);
            }

            if ins.button("ground_feather_reset", "The prefab's feather", feather.is_some()) {
                self.set_attribute(e, TERRAIN_FEATHER_ATTR, "");
            }
        }

        ins.hint("Sculpting here goes under the prefab's ground: change it in the prefab, switch it off, or unpack the prefab.");
    }
}

impl super::TileDocument {
    /// A reference's place among those that bring ground (what its layer's owner says), none if it brings none.
    pub(crate) fn prefab_ground_place(&self, group: usize) -> Option<usize> {
        self.tile().prefab_grounds().iter().position(|g| g.elem == group)
    }

    /// Unpacking a reference (its place `n`, found before): its ground stays where it is, as a layer of the user's named after the prefab
    /// (an undo entry of its own, after the unpack's). Returns whether it had any.
    pub(crate) fn keep_prefab_ground(&mut self, n: usize, key: &str) -> bool {
        let t = self.tile();
        let has = t.project.layers.as_ref().and_then(|l| l.height.as_ref()).is_some_and(|s| s.layers.iter().any(|l| parse_owner(&l.owner).is_some_and(|(m, _)| m == n)));
        if !has {
            return false;
        }

        let name = format!("{key} ground");
        self.change_stack(&format!("Keep the ground of {key}"), super::layers::LayerType::Height, |p| {
            let Some(s) = p.layers.as_mut().and_then(|l| l.height.as_mut()) else { return };
            if let Some(l) = s.layers.iter_mut().find(|l| parse_owner(&l.owner).is_some_and(|(m, _)| m == n)) {
                l.owner.clear();
                l.name.clone_from(&name);
            }

            s.put_owned_on_top();
        });

        true
    }
}
