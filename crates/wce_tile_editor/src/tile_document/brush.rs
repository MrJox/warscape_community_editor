//! What the vegetation brush needs of a tile (the tool paints, `wce_tile_tool::veg_brush`): where things already stand, and the painted
//! trees put in as one undo entry.

use wce_foundation::math::{Double3, Float3};
use wce_tile::world_transform::set_world_position;

use super::edits::scene_point;
use super::{TileDocument, item_of};

/// Something standing on the tile, seen from above: its centre (world metres), its radius (its model's bounds across, scaled; 0 for a
/// point), whether it is a manual tree, and its element.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Footprint {
    pub x: f32,
    pub z: f32,
    pub radius: f32,
    pub tree: bool,
    pub elem: usize,
}

impl TileDocument {
    /// The footprints of the tile's instances (not the polygons, groups and layers).
    pub fn footprints(&self) -> Vec<Footprint> {
        let t = self.tile();
        let sc = t.tscene();
        t.instances().iter().copied().filter(|&e| !t.is_polygon(e) && sc.doc().elements()[e].children.is_empty()).map(|e| self.footprint(e)).collect()
    }

    /// An instance's footprint: where it is, and its model's radius across (a tree's, and one whose model has not loaded: 0).
    pub fn footprint(&self, e: usize) -> Footprint {
        let t = self.tile();
        let sc = t.tscene();
        let p = t.world_position(e);
        let tree = sc.type_of(e) == "VEGETATION_INSTANCE";
        let radius = match t.model_of(e).filter(|m| m.ok && m.resolved && !tree) {
            Some(m) => {
                let s = sc.scale(e).unwrap_or(Double3::new(1.0, 1.0, 1.0));
                let across = [m.bmin.x.abs(), m.bmax.x.abs(), m.bmin.z.abs(), m.bmax.z.abs()].into_iter().fold(0.0f32, f32::max);
                across * (s.x.abs().max(s.z.abs()) / t.unit()) as f32
            }

            None => 0.0,
        };

        Footprint { x: p.x, z: p.z, radius, tree, elem: e }
    }

    /// Puts painted things into the layer `into`: each element's text at its world point (metres). One undo entry named `what`; the
    /// selection afterwards is them.
    ///
    /// # Errors
    ///
    /// A text is not an element, or there is no such layer.
    pub fn add_painted(&mut self, what: &str, into: usize, items: &[(Vec<u8>, Float3)]) -> Result<Vec<usize>, String> {
        if items.is_empty() {
            return Ok(vec![]);
        }

        self.check_structure(&[], Some(into))?;

        let mark = self.begin()?;
        let mut added: Vec<usize> = vec![];
        let result = (|| -> Result<(), String> {
            for (text, at) in items {
                let e = self.tscene_mut().insert_element_text(into, text)?;
                let unit = self.tile().unit();
                set_world_position(self.tscene_mut(), e, scene_point(*at, unit));
                added.push(e);
            }

            Ok(())
        })();

        match result {
            Ok(()) => {
                // Inserting appends inside the layer, so the elements added first keep their indices.
                self.push_splice(what.to_owned(), mark, vec![], added.iter().map(|&e| item_of(e)).collect());
                Ok(added)
            }

            Err(err) => {
                let record = self.tscene_mut().end_edit(mark).unwrap_or_default();
                let _ = self.tscene_mut().unapply(&record);
                Err(err)
            }
        }
    }
}
