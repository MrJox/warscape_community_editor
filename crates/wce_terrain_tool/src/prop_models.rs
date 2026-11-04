//! A campaign map's props drawn as their models near the camera (the `.terry`'s `ECPropMesh` entities: `ECMesh model_path`), each with
//! the transform the export writes for it (`wce_terrain_io::attila::campaign::matrix`, the kit's `ECTransform`: its rotation and scale in
//! `.terry` units, so a model is scaled by 1 / the frame's unit into the view) and standing on the drawn ground. Drawn again when the
//! scene changes or the camera moves on; pickable (a click selects the prop). The overlays stay (their crosses mark the props too far
//! for a model).

use std::sync::Arc;

use wce_editor::editor::Editor;
use wce_foundation::math::Float4x4;
use wce_scene::render_scene::{DEFAULT_INSTANCE_PARAMS, DocumentId, EntityId, LayerId, MeshInstance, Payload, SourceRef};
use wce_terrain::terry::EntityKind;
use wce_terrain_editor::MapDocument;
use wce_tile_editor::tile_document::context::Frame;

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
}

impl Default for PropModels {
    fn default() -> Self {
        Self { on: true, radius_cells: 48.0, layer: 0, drawn: vec![], built: None, count: 0 }
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

        if self.layer == 0 || e.scene.layer(self.layer).is_none() {
            self.layer = e.scene.add_layer(doc, "prop models");
            self.drawn.clear();
        }

        for ent in self.drawn.drain(..) {
            e.scene.remove(ent);
        }

        self.count = out.len();
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
