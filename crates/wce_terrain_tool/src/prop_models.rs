//! A campaign map's props drawn as their models near the camera (the `.terry`'s `ECPropMesh` entities: `ECMesh model_path`), each with
//! the transform the export writes for it (`wce_terrain_io::attila::campaign::matrix`, the kit's `ECTransform`: its rotation and scale in
//! `.terry` units, so a model is scaled by 1 / the frame's unit into the view) and standing on the drawn ground. Drawn again when the
//! scene changes or the camera moves on; pickable (a click selects the prop). The overlays stay (their crosses mark the props too far
//! for a model).
//!
//! With them the campaign's forests, as the game draws them: `campaign_maps/<map>/display/trees/trees.campaign_tree_list` in the packs
//! (BOB's; the kit has no source for it): u32 version (3), f32 bounds x0 z0 x1 z1 (the map: x east, z north, 678.5 x 556.1 on Attila's
//! main map, not square to its cells), u32 models, per model a u16-length path and a u32 count of 15-byte trees (f32 x, y, z; u8 0 / 1,
//! u8 0 .. 5, u8 15). Mapped by the bounds, all but 6 of 17 576 sampled trees stand on land (`campaign_tree_fit`; by the `.terry`'s 0.32 a
//! cell, a third in the sea or off the map). ASSUMPTIONS: the second byte is the turn in sixths (its six values equally common), and
//! a tree stands on the ground (y, -0.5 .. 4.9, unused); the first byte (1 on 5582)
//! and the last (15: the seasons?) are not read.

use std::sync::Arc;

use wce_editor::editor::Editor;
use wce_foundation::math::Float4x4;
use wce_scene::render_scene::{DEFAULT_INSTANCE_PARAMS, DocumentId, EntityId, LayerId, MeshInstance, Payload, SourceRef};
use wce_terrain::terry::EntityKind;
use wce_terrain_editor::MapDocument;
use wce_tile_editor::tile_document::context::Frame;

/// A campaign's tree list (`trees.campaign_tree_list`).
pub struct CampaignTrees {
    pub models: Vec<String>,

    /// (model, cell x, cell y (down), turn in sixths).
    pub trees: Vec<(u16, f32, f32, u8)>,

    /// Cells a unit of the list (its models' too: the game's vertex shader puts a model's vertices at the list's position plus 0.4 times
    /// theirs, `rigidcampaignvegetation.hlsl`; the 0.4 is the model cache's), across.
    pub cells_per_unit: f32,
}

impl CampaignTrees {
    /// The trees of a file on a map `w` x `h` cells; none for a file that does not read.
    pub fn parse(b: &[u8], w: u32, h: u32) -> Option<Self> {
        let u32_at = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]));
        let f32_at = |o: usize| u32_at(o).map(f32::from_bits);
        let (x1, z1) = (f32_at(12)?, f32_at(16)?);
        if u32_at(0)? != 3 || x1 <= 0.0 || z1 <= 0.0 {
            return None;
        }

        let (mut models, mut trees, mut at) = (vec![], vec![], 24);
        for m in 0..u32_at(20)? {
            let n = usize::from(u16::from_le_bytes([*b.get(at)?, *b.get(at + 1)?]));
            models.push(String::from_utf8_lossy(b.get(at + 2..at + 2 + n)?).to_lowercase().replace('\\', "/"));
            at += 2 + n;
            let count = u32_at(at)? as usize;
            at += 4;
            for k in 0..count {
                let o = at + 15 * k;
                let (x, z) = (f32_at(o)?, f32_at(o + 8)?);
                trees.push((m as u16, x / x1 * w as f32, (1.0 - z / z1) * h as f32, *b.get(o + 13)?));
            }

            at += 15 * count;
        }

        Some(Self { models, trees, cells_per_unit: w as f32 / x1 })
    }
}

/// What the drawn models are of: document, `.terry` revision, the cell the camera's target was in.
type Built = (DocumentId, u64, (i64, i64));

pub struct PropModels {
    pub on: bool,

    /// Props within this many cells of the camera's target get their model.
    pub radius_cells: f64,
    layer: LayerId,
    drawn: Vec<EntityId>,
    built: Option<Built>,
    pub count: usize,

    /// The shown campaign's trees (its document; none: no list).
    trees: Option<(DocumentId, Option<Arc<CampaignTrees>>)>,
    pub tree_count: usize,
}

impl Default for PropModels {
    fn default() -> Self {
        Self { on: true, radius_cells: 96.0, layer: 0, drawn: vec![], built: None, count: 0, trees: None, tree_count: 0 }
    }
}

impl PropModels {
    pub fn update(&mut self, e: &mut Editor, doc: DocumentId, frame: Option<Arc<Frame>>) {
        let Some(frame) = frame.filter(|_| self.on) else { return self.clear(e) };
        let Some(d) = e.docs.find_as::<MapDocument>(doc) else { return self.clear(e) };
        let Some(p) = d.project() else { return self.clear(e) };
        let tf = d.terry_frame;
        if !tf.y_from_lf {
            // Battle maps: their props come with the tiles.
            return self.clear(e);
        }

        let cam = e.viewport.camera();
        let (tx, tz) = tf.from_frame(f64::from(cam.target.x), f64::from(cam.target.z));
        let quarter = (self.radius_cells / 4.0).max(1.0) * tf.cell;
        let key = (doc, d.terry_revision.load(std::sync::atomic::Ordering::Relaxed), ((tx / quarter).floor() as i64, (tz / quarter).floor() as i64));
        if self.built == Some(key) {
            return;
        }

        if self.trees.as_ref().is_none_or(|(id, _)| *id != doc) {
            let (_, name) = crate::tool::map_kind(&d.name);
            let list = e.files().and_then(|f| f.read(&format!("campaign_maps/{name}/display/trees/trees.campaign_tree_list")).ok())
                .and_then(|b| CampaignTrees::parse(&b, frame.map.width, frame.map.height)).map(Arc::new);
            self.trees = Some((doc, list));
        }

        let r = self.radius_cells * tf.cell;
        let k = (1.0 / tf.unit) as f32;
        let mut out: Vec<(Float4x4, Payload, u64)> = vec![];
        for x in &p.entities {
            let EntityKind::Prop(model) = p.kind(x) else { continue };
            let w = p.world_position(x.id);
            if (w[0] - tx).hypot(w[2] - tz) > r || model.is_empty() {
                continue;
            }

            let m = wce_terrain_io::attila::campaign::matrix(p, x);
            let (fx, fz) = tf.to_frame(w[0], w[2]);
            let mut world = Float4x4::IDENTITY;
            for row in 0..3 {
                for col in 0..3 {
                    // The marker's row `col` is the transform's column `col`.
                    world.m[row][col] = m[col * 4 + row] * k;
                }
            }

            world.m[0][3] = fx as f32;
            world.m[1][3] = frame.lift(fx, fz);
            world.m[2][3] = fz as f32;
            let p16 = DEFAULT_INSTANCE_PARAMS;
            let mesh = MeshInstance {
                model: model.to_lowercase().replace('\\', "/"), casts_shadow: true, lod: true, p4: [p16[0], p16[1], p16[2], p16[3]], p5: [p16[4], p16[5], p16[6], p16[7]],
                p6: [p16[8], p16[9], p16[10], p16[11]], p7: [p16[12], p16[13], p16[14], p16[15]], burning: false,
            };
            out.push((world, Payload::MeshInstance(mesh), x.id));
        }

        // The forests near the camera: by cells (the trees' own frame), within the same radius.
        self.tree_count = 0;
        if let Some((_, Some(trees))) = &self.trees {
            let (cx, cy) = frame.cell_of(f64::from(cam.target.x), f64::from(cam.target.z));
            let rc = self.radius_cells as f32;
            let (a, b) = (frame.from_map.apply(0.0, 0.0), frame.from_map.apply(1.0, 0.0));
            let k = trees.cells_per_unit * (b.0 - a.0).hypot(b.1 - a.1) as f32;
            let (cx, cy) = (cx as f32, cy as f32);
            for &(m, x, y, turn) in &trees.trees {
                if (x - cx).hypot(y - cy) > rc {
                    continue;
                }

                let (fx, fz) = frame.from_map.apply(f64::from(x), f64::from(y));
                let (s, c) = (f32::from(turn) * std::f32::consts::FRAC_PI_3).sin_cos();
                let mut world = Float4x4::IDENTITY;
                world.m[0] = [c * k, 0.0, s * k, fx as f32];
                world.m[1] = [0.0, k, 0.0, frame.lift(fx, fz)];
                world.m[2] = [-s * k, 0.0, c * k, fz as f32];
                let p16 = DEFAULT_INSTANCE_PARAMS;
                let mesh = MeshInstance {
                    model: trees.models[usize::from(m)].clone(), casts_shadow: true, lod: true, p4: [p16[0], p16[1], p16[2], p16[3]], p5: [p16[4], p16[5], p16[6], p16[7]],
                    p6: [p16[8], p16[9], p16[10], p16[11]], p7: [p16[12], p16[13], p16[14], p16[15]], burning: false,
                };
                out.push((world, Payload::MeshInstance(mesh), 0));
                self.tree_count += 1;
            }
        }

        if self.layer == 0 || e.scene.layer(self.layer).is_none() {
            self.layer = e.scene.add_layer(doc, "prop models");
            self.drawn.clear();
        }

        for ent in self.drawn.drain(..) {
            e.scene.remove(ent);
        }

        self.count = out.len() - self.tree_count;
        for (world, payload, id) in out {
            self.drawn.push(e.scene.add(self.layer, world, payload, SourceRef { doc, item: id }));
        }

        self.built = Some(key);
    }

    pub fn clear(&mut self, e: &mut Editor) {
        if self.layer != 0 {
            e.scene.remove_layer(self.layer);
        }

        (self.layer, self.built, self.count) = (0, None, 0);
        self.drawn.clear();
    }
}
