//! Baking a river (user 2026-11-28): what the editor makes of it on its own made its user's, to edit by hand. Its shore (reeds,
//! stones, decals, rapids and their splashes, cliff pieces and their end rocks, the fords' splashes), its AI hints and the exclusion
//! polygons that keep its reeds in the game become elements of their own in a layer of their own; its bed in the ground (its height,
//! texture and ground type layers) becomes terrain layers of the user's. The river moves into a Don't Export layer, where it makes none
//! of them any more (`siege::exported`); its water (its file) is still made from it. Editing it or baking it again doesn't touch what
//! was baked: that may no longer fit, and baking again adds another set.

use super::layers::LayerType;
use super::{TileDocument, item_of};

/// The layer of a river's baked elements: `<name> (autogen)`; the layer the river goes into: `<name> (baked river)`, not exported.
pub const BAKED_LAYER_SUFFIX: &str = "(autogen)";
pub const BAKED_RIVER_LAYER_SUFFIX: &str = "(baked river)";

/// Its terrain layers (height, texture, ground type), the user's from then on: `<name> (baked) height` and so on.
pub const BAKED_TERRAIN_SUFFIX: &str = "(baked)";

impl TileDocument {
    /// Whether a river can be baked now (a river, in a layer that is exported), or why not.
    ///
    /// # Errors
    ///
    /// Not a river, or baked already (in a Don't Export layer).
    pub fn can_bake_river(&self, river: usize) -> Result<(), String> {
        let sc = self.tile().tscene();
        if river >= sc.doc().elements().len() || sc.type_of(river) != "SplineRiverInstance" {
            return Err("only a river can be baked".to_owned());
        }

        if !super::siege::exported(sc, river) {
            return Err("the river is in a Don't Export layer (baked already?): it makes nothing to bake".to_owned());
        }

        Ok(())
    }

    /// Bakes a river, one undo entry: its shore's items, its AI hints and its reeds' exclusion polygons as they are now, made elements
    /// of their own in a new layer `<name> (autogen)` beside it (in the layer it is in); the river moved into a new Don't Export layer
    /// `<name> (baked river)` there, so that it makes them no more. Returns what was done.
    ///
    /// # Errors
    ///
    /// Not a river that can be baked ([`Self::can_bake_river`]), or the scene can't be edited.
    pub fn bake_river(&mut self, river: usize) -> Result<String, String> {
        use wce_editor_core::command::History;

        self.can_bake_river(river)?;
        self.check_structure(&[river], None)?;
        // Its shore and its ground's layers as the river is now (made again off the editor's thread when it changed).
        self.finish_shores();
        self.tile_mut().finish_river_jobs();

        let t = self.tile();
        let sc = t.tscene();
        let name = match sc.name(river) {
            n if n.trim().is_empty() => "River".to_owned(),
            n => n,
        };
        let layer = {
            let els = sc.doc().elements();
            let mut p = els[river].parent;
            while let Some(q) = p.filter(|&q| !matches!(sc.type_of(q), "LayerInstance" | "SCENE_GRAPH")) {
                p = els[q].parent;
            }

            p.ok_or("the river is in no layer")?
        };
        let mut texts = t.shore_items.iter().filter(|i| i.area == river).map(|i| i.text.clone()).collect::<Vec<_>>();
        let (items, hints) = (texts.len(), t.river_hints_of(Some(river)));
        let exclusions = t.reed_exclusions(Some(river));
        let (hint_count, exclusion_count) = (hints.len(), exclusions.len());
        texts.extend(hints);
        texts.extend(exclusions);
        let w = wce_tile::world_transform::world_transform(sc, river);
        let id = sc.get(river, super::river_links::RIVER_ID_ATTR).and_then(|v| v.trim().parse::<u32>().ok());

        let before = self.history.size();
        let mark = self.begin()?;
        let made = (|| -> Result<Vec<usize>, String> {
            let sc = self.tscene_mut();
            let baked = sc.add_layer(Some(layer), &format!("{name} {BAKED_LAYER_SUFFIX}")).map_err(|e| e.to_string())?;
            sc.insert_elements_text(baked, &texts).map_err(|e| e.to_string())?;
            let kept = sc.add_layer(Some(layer), &format!("{name} {BAKED_RIVER_LAYER_SUFFIX}")).map_err(|e| e.to_string())?;
            sc.set_attr(kept, "export_type", wce_walls::scene::EXPORT_OFF);
            let moved = sc.move_to_layer(river, kept).map_err(|e| e.to_string())?;
            wce_tile::world_transform::set_world_position(sc, moved, w.position);
            if sc.euler(moved).is_some() {
                wce_tile::world_transform::set_world_rotation(sc, moved, &w.rotation);
            }

            let mut made = sc.children(baked);
            made.push(moved);
            Ok(made)
        })();
        let made = match made {
            Ok(m) => m,
            Err(e) => {
                self.push_splice("Bake river".to_owned(), mark, vec![], vec![]);
                self.history.undo();
                self.history.drop_redo();
                return Err(e);
            }
        };

        self.push_splice("Bake river".to_owned(), mark, vec![item_of(river)], made.iter().map(|&e| item_of(e)).collect());

        // Its ground: its height, texture and ground type layers no longer the river's (which, not exported, would take them out), the
        // user's, named for it.
        let mut terrain = vec![];
        if let Some(id) = id {
            for (kind, what) in [(LayerType::Height, "height"), (LayerType::Blend, "texture"), (LayerType::Ground, "ground type")] {
                let label = format!("{name} {BAKED_TERRAIN_SUFFIX} {what}");
                let owned = |owner: &str| super::river_beds::is_owner_of_river(owner, id);
                let has = match self.tile().project.layers.as_ref() {
                    Some(l) => match kind {
                        LayerType::Height => l.height.as_ref().is_some_and(|s| s.layers.iter().any(|l| owned(&l.owner))),
                        LayerType::Blend => l.blend.as_ref().is_some_and(|s| s.layers.iter().any(|l| owned(&l.owner))),
                        LayerType::Ground => l.ground.as_ref().is_some_and(|s| s.layers.iter().any(|l| owned(&l.owner))),
                    },
                    None => false,
                };
                if !has {
                    continue;
                }

                self.change_stack("Bake river", kind, |p| {
                    let Some(l) = p.layers.as_mut() else { return };
                    let hand_over = |owner: &mut String, layer_name: &mut String| {
                        if owned(owner) {
                            owner.clear();
                            layer_name.clone_from(&label);
                        }
                    };
                    match kind {
                        LayerType::Height => {
                            l.height.iter_mut().flat_map(|s| s.layers.iter_mut()).for_each(|l| hand_over(&mut l.owner, &mut l.name))
                        }
                        LayerType::Blend => {
                            l.blend.iter_mut().flat_map(|s| s.layers.iter_mut()).for_each(|l| hand_over(&mut l.owner, &mut l.name))
                        }
                        LayerType::Ground => {
                            l.ground.iter_mut().flat_map(|s| s.layers.iter_mut()).for_each(|l| hand_over(&mut l.owner, &mut l.name))
                        }
                    }
                });
                terrain.push(format!("\"{label}\""));
            }
        }

        let pushed = self.history.size() - before;
        self.history.join_last(pushed, "Bake river");
        let terrain =
            if terrain.is_empty() { String::new() } else { format!("; its ground kept as the terrain layer(s) {}", terrain.join(", ")) };
        Ok(format!(
            "{name}: {items} shore item(s), {hint_count} AI hint(s) and {exclusion_count} reed exclusion polygon(s) baked into the layer \
             \"{name} {BAKED_LAYER_SUFFIX}\"{terrain}; the river is in the Don't Export layer \"{name} {BAKED_RIVER_LAYER_SUFFIX}\""
        ))
    }
}
