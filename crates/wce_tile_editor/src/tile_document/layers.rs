//! A tile's terrain layers (`wce_tile::terrain_layers`, docs/plans/procedural_roads.md), as the Tile panel and the `terrain_layer` command use
//! them. Edits are not routed here: every brush edits the fields, and [`Tile::sync_terrain_layers`] takes what they changed into the working
//! layer when the edit closes.
//!
//! A change of the stack itself (a layer added, shown, hidden, moved, removed, merged, its name or opacity) is an undo entry of its own,
//! [`LayerStackCommand`]; what edits do is undone by the stack (`Stack::sync`).

use std::any::Any;

use wce_editor_core::command::Command;

use wce_tile::terrain_layers::{AnyState, Stack, TerrainLayers};

use super::{Tile, TileDocument};

/// The layer of the user's that paint goes to when the tile has texture rules and no working layer ([`TileDocument::paint_over_texture_rules`]).
pub const PAINTED: &str = "Painted";

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
    pub bytes: usize,

    /// The scene layer it exports with (`wce_layer_id`), empty: always.
    pub depends: String,

    /// A combined project's variations it belongs to, empty: all.
    pub variations: Vec<String>,

    /// It shows in the viewed variation (else it belongs to others only).
    pub in_view: bool,
}

/// What a terrain layer's dependency comes to, for the panel.
#[derive(Clone, Debug, PartialEq)]
pub enum Dependency {
    Always,
    Gone,

    /// The scene layer, its name and how it exports (`TileProject::layer_export`).
    On {
        elem: usize,
        name: String,
        export: String,
    },
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
    // What a procedural feature owns is not listed: it is made from the feature, and goes with it.
    let list = s
        .layers
        .iter()
        .filter(|l| l.owner.is_empty())
        .map(|l| LayerRow {
            id: l.id,
            name: l.name.clone(),
            visible: l.visible,
            opacity: l.opacity,
            bytes: l.bytes(),
            depends: l.depends.clone(),
            variations: l.variations.clone(),
            in_view: !s.hidden_by_view(l.id),
        })
        .collect();
    (list, s.working)
}

/// A change of a stack: its states before and after. Made after the change is.
pub(super) struct LayerStackCommand {
    name: String,
    before: AnyState,
    after: AnyState,
    first: bool,
}

impl Command<Tile> for LayerStackCommand {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn apply(&mut self, t: &mut Tile) {
        if !std::mem::take(&mut self.first) {
            t.end_stroke_edit();
            t.sync_terrain_layers();
            t.put_layer_state(&self.after);
        }
    }

    fn revert(&mut self, t: &mut Tile) {
        t.end_stroke_edit();
        t.sync_terrain_layers();
        t.put_layer_state(&self.before);
    }

    fn dirty_items(&self) -> Vec<u64> {
        vec![super::TERRAIN]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
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
            && s.sync(f)
        {
            changed = true;
            self.roads_stale = true;
            self.rivers_stale = true;
        }

        if let (Some(s), Some(f)) = (l.blend.as_mut(), p.blend.as_mut()) {
            changed |= s.sync(f);
        }

        if let (Some(s), Some(f)) = (l.ground.as_mut(), p.ground.as_mut()) {
            changed |= s.sync(f);
        }

        l.dirty |= changed;
        self.layers_shown |= changed;
    }

    fn put_layer_state(&mut self, state: &AnyState) {
        let p = &mut self.project;
        let Some(l) = p.layers.as_mut() else { return };
        match state {
            AnyState::Height(st) => {
                if let (Some(s), Some(f)) = (l.height.as_mut(), p.height.as_mut()) {
                    s.put_state(st, f);
                }
            }

            AnyState::Blend(st) => {
                if let (Some(s), Some(f)) = (l.blend.as_mut(), p.blend.as_mut()) {
                    s.put_state(st, f);
                }
            }

            AnyState::Ground(st) => {
                if let (Some(s), Some(f)) = (l.ground.as_mut(), p.ground.as_mut()) {
                    s.put_state(st, f);
                }
            }
        }

        l.dirty = true;
        self.layers_shown = true;

        // A layer's variations may have changed: what the viewed one shows.
        self.project.refresh_view();

        // A layer shown, hidden or moved changes what the grass stands on.
        self.reload_grass();
    }

    fn layer_state(&self, kind: LayerType) -> Option<AnyState> {
        let l = self.project.layers.as_ref()?;
        Some(match kind {
            LayerType::Height => AnyState::Height(l.height.as_ref()?.state()),
            LayerType::Blend => AnyState::Blend(l.blend.as_ref()?.state()),
            LayerType::Ground => AnyState::Ground(l.ground.as_ref()?.state()),
        })
    }
}

impl TileDocument {
    /// Makes a change of a stack, its composite shown, as one undo entry. A change in progress (`preview_stack`) ends here.
    pub(super) fn change_stack(&mut self, name: &str, kind: LayerType, mut change: impl FnMut(&mut wce_tile::tile_project::TileProject)) {
        let t = self.tile_mut();
        t.sync_terrain_layers();
        let before = t.project.layers.as_mut().and_then(|l| l.pending.take()).or_else(|| t.layer_state(kind));
        change(&mut t.project);
        let after = t.layer_state(kind);
        if let Some(l) = t.project.layers.as_mut() {
            l.dirty = true;
        }

        if let (Some(after), Some(before)) = (after, before) {
            t.put_layer_state(&after);
            self.history.push(Box::new(LayerStackCommand { name: name.to_owned(), before, after, first: true }));
        }
    }

    /// Makes a change of a stack with no undo entry yet: the first of a drag keeps the state before it, and `commit_stack` ends it.
    fn preview_stack(&mut self, kind: LayerType, change: impl FnOnce(&mut wce_tile::tile_project::TileProject)) {
        let t = self.tile_mut();
        t.sync_terrain_layers();
        let before = t.layer_state(kind);
        if let Some(l) = t.project.layers.as_mut() {
            if l.pending.is_none() {
                l.pending = before;
            }

            l.dirty = true;
        }

        change(&mut t.project);
    }

    /// Ends a change in progress as one undo entry.
    pub fn commit_terrain_layer(&mut self, name: &str, kind: LayerType) {
        if self.project().layers.as_ref().is_some_and(|l| l.pending.is_some()) {
            self.change_stack(name, kind, |_| {});
        }
    }

    /// The user's layers of a type, in the order they apply (the last is on top), and the working layer.
    #[must_use]
    pub fn terrain_layers(&self, kind: LayerType) -> (Vec<LayerRow>, Option<u64>) {
        let Some(l) = self.project().layers.as_ref() else { return (vec![], None) };
        match kind {
            LayerType::Height => l.height.as_ref().map(rows),
            LayerType::Blend => l.blend.as_ref().map(rows),
            LayerType::Ground => l.ground.as_ref().map(rows),
        }
        .unwrap_or_default()
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
    pub fn add_terrain_layer(&mut self, kind: LayerType, name: &str) -> Result<u64, String> {
        self.add_terrain_layer_for(kind, name, &[])
    }

    /// [`Self::add_terrain_layer`] of some variations of a combined project only (empty: all): *New layer for this variation*.
    ///
    /// # Errors
    ///
    /// The tile has no such map.
    pub fn add_terrain_layer_for(&mut self, kind: LayerType, name: &str, variations: &[String]) -> Result<u64, String> {
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

        // The stack first, so that the undo entry's state before has no layer.
        let layers = p.layers.get_or_insert_with(TerrainLayers::default);
        match kind {
            LayerType::Height => {
                let f = p.height.as_mut().expect("checked above");
                layers.height.get_or_insert_with(|| Stack::new(f));
            }

            LayerType::Blend => {
                let f = p.blend.as_mut().expect("loaded above");
                layers.blend.get_or_insert_with(|| Stack::new(f));
            }

            LayerType::Ground => {
                let f = p.ground.as_mut().expect("loaded above");
                layers.ground.get_or_insert_with(|| Stack::new(f));
            }
        }

        let id = layers.new_id();
        self.change_stack(&format!("New layer {name}"), kind, |p| {
            with_stack!(p, kind, |s, _f| {
                let id = s.add(id, name, "");
                if let Some(i) = s.user_index(id) {
                    s.layers[i].variations = variations.to_vec();
                }

                s.working = Some(id);
            });
        });

        Ok(id)
    }

    /// Before paint goes on the blend map (or the ground type map, `kind`): when the tile has texture rules and the paint would go to the
    /// base (no working layer), which lies under the rules' layer and would not show where they cover it, a layer of the user's
    /// [`PAINTED`] (the first of that name, else a new one, its own undo entry) is made the working one (docs/plans/auto_detail.md, A: hand
    /// paint goes over the rules).
    pub(crate) fn paint_over_texture_rules(&mut self, kind: LayerType) {
        let rules = match kind {
            LayerType::Blend => !self.tile().all_texture_rules().0.is_empty(),
            LayerType::Ground => self.project().texture_rules.as_ref().is_some_and(|r| wce_tile::texture_rules::lays_ground(&r.rules)),
            LayerType::Height => return,
        };
        let layers = self.project().layers.as_ref();
        let (working, under, painted) = match kind {
            LayerType::Ground => under_state(layers.and_then(|l| l.ground.as_ref())),
            _ => under_state(layers.and_then(|l| l.blend.as_ref())),
        };
        if working || !(rules || under) {
            return;
        }

        match painted {
            Some(id) => self.set_working_layer(kind, Some(id)),
            None => {
                let _ = self.add_terrain_layer(kind, PAINTED);
            }
        }
    }

    /// Sets the layer edits go to (none: the base). Not an undo entry: a choice, as the brush is.
    pub fn set_working_layer(&mut self, kind: LayerType, id: Option<u64>) {
        let t = self.tile_mut();
        t.sync_terrain_layers();
        with_stack!(&mut t.project, kind, |s, _f| {
            s.working = id.filter(|&i| s.user_index(i).is_some());
            s.remember();
        });
    }

    /// The variations of a combined project a layer belongs to (empty: all). One undo entry; the viewed variation's terrain follows.
    pub fn set_terrain_layer_variations(&mut self, kind: LayerType, id: u64, variations: &[String]) {
        let name =
            if variations.is_empty() { "Layer in every variation".to_owned() } else { format!("Layer in {}", variations.join(", ")) };
        self.change_stack(&name, kind, |p| {
            with_stack!(p, kind, |s, _f| {
                if let Some(i) = s.user_index(id) {
                    s.layers[i].variations = variations.to_vec();
                }
            });
        });
    }

    /// Changes a layer's name, in progress: `commit_terrain_layer` ends it.
    pub fn rename_terrain_layer(&mut self, kind: LayerType, id: u64, name: &str) {
        self.preview_stack(kind, |p| {
            with_stack!(p, kind, |s, _f| {
                if let Some(i) = s.user_index(id) {
                    s.layers[i].name = name.to_owned();
                }
            });
        });
    }

    /// Shows or hides a layer.
    pub fn set_terrain_layer_visible(&mut self, kind: LayerType, id: u64, visible: bool) {
        self.change_stack(if visible { "Show layer" } else { "Hide layer" }, kind, |p| {
            with_stack!(p, kind, |s, _f| {
                if let Some(i) = s.user_index(id) {
                    s.layers[i].visible = visible;
                }
            });
        });
    }

    /// Changes a layer's opacity, in progress (a slider being dragged): `commit_terrain_layer` ends it.
    pub fn set_terrain_layer_opacity(&mut self, kind: LayerType, id: u64, opacity: f32) {
        self.preview_stack(kind, |p| {
            with_stack!(p, kind, |s, f| {
                if let Some(i) = s.user_index(id) {
                    s.layers[i].opacity = opacity.clamp(0.0, 1.0);
                    s.apply_all(f);
                }
            });
        });
    }

    /// Moves a layer up (`by` 1, later in the order) or down the list.
    pub fn move_terrain_layer(&mut self, kind: LayerType, id: u64, by: i32) {
        self.change_stack("Move layer", kind, |p| {
            with_stack!(p, kind, |s, _f| {
                if let Some(i) = s.user_index(id) {
                    let j = i as i32 + by;
                    if j >= 0 && s.layers.get(j as usize).is_some_and(|l| l.owner.is_empty()) {
                        s.layers.swap(i, j as usize);
                    }
                }
            });
        });
    }

    /// Merges a layer into the one below it, or the base.
    pub fn merge_terrain_layer(&mut self, kind: LayerType, id: u64) {
        self.change_stack("Merge layer", kind, |p| {
            with_stack!(p, kind, |s, _f| {
                // Not into what a procedural feature owns (the texture rules' layer, under the user's): it would go when that is made
                // again, and merging it into the base would put it under the rules.
                if let Some(i) = s.user_index(id)
                    && i.checked_sub(1).is_none_or(|j| s.layers[j].owner.is_empty())
                {
                    s.merge_down(i);
                }
            });
        });
    }

    /// Makes a layer export only with a scene layer (`None`: always). The scene layer gets its `wce_layer_id` if it has none, an undo entry of
    /// its own; the dependency is another.
    pub fn set_terrain_layer_dependency(&mut self, kind: LayerType, id: u64, scene_layer: Option<usize>) {
        let key = match scene_layer {
            None => String::new(),
            Some(elem) => match self.tile().tscene().get(elem, "wce_layer_id").filter(|k| !k.is_empty()) {
                Some(k) => k,
                None => {
                    let next = self
                        .tile()
                        .tscene()
                        .find_all("LayerInstance")
                        .into_iter()
                        .filter_map(|e| self.tile().tscene().get(e, "wce_layer_id")?.parse::<u64>().ok())
                        .max()
                        .unwrap_or(0)
                        + 1;
                    if !self.set_attribute(elem, "wce_layer_id", &next.to_string()) {
                        return;
                    }

                    next.to_string()
                }
            },
        };

        self.change_stack("Layer exports with", kind, |p| {
            with_stack!(p, kind, |s, _f| {
                if let Some(i) = s.user_index(id) {
                    s.layers[i].depends.clone_from(&key);
                }
            });
        });
    }

    /// What a dependency comes to now.
    #[must_use]
    pub fn terrain_layer_dependency(&self, depends: &str) -> Dependency {
        if depends.is_empty() {
            return Dependency::Always;
        }

        let p = self.project();
        match p.layer_by_key(depends) {
            None => Dependency::Gone,
            Some(elem) => {
                Dependency::On { elem, name: self.tile().tscene().get(elem, "name").unwrap_or_default(), export: p.layer_export(elem) }
            }
        }
    }

    /// The scene's layers, in the scene's order, with their names.
    #[must_use]
    pub fn scene_layers(&self) -> Vec<(usize, String)> {
        let sc = self.tile().tscene();
        sc.find_all("LayerInstance").into_iter().map(|e| (e, sc.get(e, "name").unwrap_or_default())).collect()
    }

    /// Removes a layer and what it did.
    pub fn remove_terrain_layer(&mut self, kind: LayerType, id: u64) {
        self.change_stack("Remove layer", kind, |p| {
            with_stack!(p, kind, |s, _f| {
                if let Some(i) = s.user_index(id) {
                    s.layers.remove(i);
                    if s.working == Some(id) {
                        s.working = None;
                    }
                }
            });
        });
    }
}

/// Of a stack: whether it has a working layer, whether a layer goes under the user's (the texture rules'), and the user's layer
/// [`PAINTED`].
fn under_state<K: wce_tile::terrain_layers::Kind>(s: Option<&Stack<K>>) -> (bool, bool, Option<u64>) {
    let Some(s) = s else { return (false, false, None) };
    (
        s.working.is_some(),
        s.layers.iter().any(|l| wce_tile::terrain_layers::order_of(&l.owner) == 0),
        s.layers.iter().find(|l| l.owner.is_empty() && l.name == PAINTED).map(|l| l.id),
    )
}
