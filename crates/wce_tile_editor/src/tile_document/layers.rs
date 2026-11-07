//! A tile's terrain layers (`wce_tile::terrain_layers`, docs/plans/procedural_roads.md), as the Tile panel and the `terrain_layer` command use
//! them. Edits are not routed here: every brush edits the fields, and [`Tile::sync_terrain_layers`] takes what they changed into the working
//! layer when the edit closes.
//!
//! A change of the stack itself (a layer shown, hidden, moved, removed, merged, its opacity) rewrites the field, and the field's undo records
//! would then restore pixels of another composite: it clears that field's undo. ASSUMPTION of the first version: such a change is not undoable.

use wce_tile::terrain_layers::{Stack, TerrainLayers};

use super::{Tile, TileDocument};

/// A map layers go over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerType {
    Height,
    Blend,
    Ground,
}

impl LayerType {
    pub const ALL: [Self; 3] = [Self::Height, Self::Blend, Self::Ground];

    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::Height => "Heights",
            Self::Blend => "Textures",
            Self::Ground => "Ground types",
        }
    }

    /// By the name a command gives it.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "height" => Some(Self::Height),
            "blend" | "texture" => Some(Self::Blend),
            "ground" => Some(Self::Ground),
            _ => None,
        }
    }
}

/// A layer, as the panel lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerRow {
    pub id: u64,
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    pub owner: String,
    pub bytes: usize,
}

/// Runs a body on the stack of a type and its field.
macro_rules! with_stack {
    ($project:expr, $kind:expr, |$stack:ident, $field:ident| $body:expr) => {{
        let p = $project;
        match (p.layers.as_mut(), $kind) {
            (Some(l), LayerType::Height) => match (l.height.as_mut(), p.height.as_mut()) {
                (Some($stack), Some($field)) => Some($body),
                _ => None,
            },

            (Some(l), LayerType::Blend) => match (l.blend.as_mut(), p.blend.as_mut()) {
                (Some($stack), Some($field)) => Some($body),
                _ => None,
            },

            (Some(l), LayerType::Ground) => match (l.ground.as_mut(), p.ground.as_mut()) {
                (Some($stack), Some($field)) => Some($body),
                _ => None,
            },

            _ => None,
        }
    }};
}

fn rows<K: wce_tile::terrain_layers::Kind>(s: &Stack<K>) -> (Vec<LayerRow>, Option<u64>) {
    let list = s.layers.iter().map(|l| LayerRow { id: l.id, name: l.name.clone(), visible: l.visible, opacity: l.opacity, owner: l.owner.clone(), bytes: l.bytes() }).collect();
    (list, s.working)
}

impl Tile {
    /// Takes what edits changed into the layers. An open stroke waits for its end.
    pub(super) fn sync_terrain_layers(&mut self) {
        if self.stroke_open {
            return;
        }

        let p = &mut self.project;
        let Some(l) = p.layers.as_mut() else { return };
        let mut changed = false;
        if let (Some(s), Some(f)) = (l.height.as_mut(), p.height.as_mut())
            && let Some(r) = f.take_edited()
        {
            changed |= s.absorb(f, [r.0, r.1, r.2, r.3]);
        }

        if let (Some(s), Some(f)) = (l.blend.as_mut(), p.blend.as_mut())
            && let Some(r) = f.take_edited()
        {
            changed |= s.absorb(f, [r.0, r.1, r.2, r.3]);
        }

        if let (Some(s), Some(f)) = (l.ground.as_mut(), p.ground.as_mut())
            && let Some(r) = f.take_edited()
        {
            changed |= s.absorb(f, [r.0, r.1, r.2, r.3]);
        }

        l.dirty |= changed;
    }
}

impl TileDocument {
    /// The layers of a type, in the order they apply (the last is on top), and the working layer.
    #[must_use]
    pub fn terrain_layers(&self, kind: LayerType) -> (Vec<LayerRow>, Option<u64>) {
        let Some(l) = self.project().layers.as_ref() else { return (vec![], None) };
        match kind {
            LayerType::Height => l.height.as_ref().map(rows),
            LayerType::Blend => l.blend.as_ref().map(rows),
            LayerType::Ground => l.ground.as_ref().map(rows),
        }.unwrap_or_default()
    }

    /// Whether the stacks changed since they were last saved.
    #[must_use]
    pub fn terrain_layers_dirty(&self) -> bool {
        self.project().layers.as_ref().is_some_and(|l| l.dirty)
    }

    /// A new layer on top of a type, made the working one; its id.
    ///
    /// # Errors
    ///
    /// The tile has no such map.
    pub fn add_terrain_layer(&mut self, kind: LayerType, name: &str, owner: &str) -> Result<u64, String> {
        let t = self.tile_mut();
        t.sync_terrain_layers();
        let p = &mut t.project;
        match kind {
            LayerType::Height if p.height.is_none() => return Err("the tile has no height map".to_owned()),
            LayerType::Blend => {
                p.load_blend()?;
            }

            LayerType::Ground => {
                p.load_ground()?;
            }

            LayerType::Height => {}
        }

        let layers = p.layers.get_or_insert_with(TerrainLayers::default);
        let id = layers.new_id();
        match kind {
            LayerType::Height => {
                let f = p.height.as_mut().expect("checked above");
                f.take_edited();
                let s = layers.height.get_or_insert_with(|| Stack::new(f));
                s.working = Some(s.add(id, name, owner));
            }

            LayerType::Blend => {
                let f = p.blend.as_mut().expect("loaded above");
                f.take_edited();
                let s = layers.blend.get_or_insert_with(|| Stack::new(f));
                s.working = Some(s.add(id, name, owner));
            }

            LayerType::Ground => {
                let f = p.ground.as_mut().expect("loaded above");
                f.take_edited();
                let s = layers.ground.get_or_insert_with(|| Stack::new(f));
                s.working = Some(s.add(id, name, owner));
            }
        }

        layers.dirty = true;
        Ok(id)
    }

    /// Sets the layer edits go to (none: the base).
    pub fn set_working_layer(&mut self, kind: LayerType, id: Option<u64>) {
        let t = self.tile_mut();
        t.sync_terrain_layers();
        with_stack!(&mut t.project, kind, |s, _f| s.working = id);
        if let Some(l) = t.project.layers.as_mut() {
            l.dirty = true;
        }
    }

    /// A change of a layer that does not touch what it holds (its name).
    pub fn rename_terrain_layer(&mut self, kind: LayerType, id: u64, name: &str) {
        let t = self.tile_mut();
        with_stack!(&mut t.project, kind, |s, _f| {
            if let Some(i) = s.index_of(id) {
                s.layers[i].name = name.to_owned();
            }
        });

        if let Some(l) = t.project.layers.as_mut() {
            l.dirty = true;
        }
    }

    /// Shows or hides a layer, and sets its opacity.
    pub fn set_terrain_layer_look(&mut self, kind: LayerType, id: u64, visible: bool, opacity: f32) {
        let t = self.tile_mut();
        t.sync_terrain_layers();
        with_stack!(&mut t.project, kind, |s, f| {
            if let Some(i) = s.index_of(id) {
                s.layers[i].visible = visible;
                s.layers[i].opacity = opacity.clamp(0.0, 1.0);
                s.apply_all(f);
                f.clear_undo();
            }
        });

        if let Some(l) = t.project.layers.as_mut() {
            l.dirty = true;
        }
    }

    /// Moves a layer up (`by` 1, later in the order) or down the list.
    pub fn move_terrain_layer(&mut self, kind: LayerType, id: u64, by: i32) {
        let t = self.tile_mut();
        t.sync_terrain_layers();
        with_stack!(&mut t.project, kind, |s, f| {
            if let Some(i) = s.index_of(id) {
                let j = i as i32 + by;
                if j >= 0 && (j as usize) < s.layers.len() {
                    s.layers.swap(i, j as usize);
                    s.apply_all(f);
                    f.clear_undo();
                }
            }
        });

        if let Some(l) = t.project.layers.as_mut() {
            l.dirty = true;
        }
    }

    /// Merges a layer into the one below it, or the base.
    pub fn merge_terrain_layer(&mut self, kind: LayerType, id: u64) {
        let t = self.tile_mut();
        t.sync_terrain_layers();
        with_stack!(&mut t.project, kind, |s, f| {
            if let Some(i) = s.index_of(id) {
                s.merge_down(i);
                s.apply_all(f);
                f.clear_undo();
            }
        });

        if let Some(l) = t.project.layers.as_mut() {
            l.dirty = true;
        }
    }

    /// Removes a layer and what it did.
    pub fn remove_terrain_layer(&mut self, kind: LayerType, id: u64) {
        let t = self.tile_mut();
        t.sync_terrain_layers();
        with_stack!(&mut t.project, kind, |s, f| {
            if let Some(i) = s.index_of(id) {
                s.layers.remove(i);
                if s.working == Some(id) {
                    s.working = None;
                }

                s.apply_all(f);
                f.clear_undo();
            }
        });

        if let Some(l) = t.project.layers.as_mut() {
            l.dirty = true;
        }
    }
}
