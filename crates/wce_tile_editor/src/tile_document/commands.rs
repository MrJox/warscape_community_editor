//! The commands of a tile document. They edit the [`Tile`]; their names are made when they are pushed (the C++ code read the element's name when
//! the history panel asked, after later structural edits had possibly moved the indices).
//!
//! Undo puts the attribute texts an edit replaced back as they were (writing the old numbers again could give other text than the file's,
//! and the text splices of earlier structural commands must still fit the text).

use wce_formats::tile_entry::{TileLink, TileLinkTarget};
use std::any::Any;
use std::collections::HashMap;

use wce_editor_core::command::Command;
use wce_formats::xml_doc::XmlSplice;
use wce_foundation::math::Double3;

use super::{BrushKind, BrushSettings, Follow, TERRAIN, Tile, item_of};

/// A move and / or rotation of one element (local position and euler angles); the steps of one gesture merge.
pub(crate) struct TedMoveCommand {
    pub name: String,
    pub elem: usize,
    pub old_pos: Option<Double3>,
    pub new_pos: Option<Double3>,
    pub old_euler: Option<Double3>,
    pub new_euler: Option<Double3>,
    pub old_pos_text: Option<String>,
    pub old_euler_text: Option<String>,
    pub gesture: u64,

    /// A layer or group: everything below it moved, the whole tile is republished.
    pub whole: bool,
}

impl TedMoveCommand {
    fn set(&self, t: &mut Tile, p: Option<Double3>, e: Option<Double3>) {
        if let Some(p) = p {
            t.project.scene.set_position(self.elem, p);
        }

        if let Some(e) = e {
            t.project.scene.set_euler(self.elem, e);
        }

        // What asks for the world position before the next republish sees the move.
        t.index_edited(self.elem);
    }
}

impl Command<Tile> for TedMoveCommand {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn apply(&mut self, t: &mut Tile) {
        self.set(t, self.new_pos, self.new_euler);
    }

    fn revert(&mut self, t: &mut Tile) {
        let sc = &mut t.project.scene;
        if let Some(p) = self.old_pos {
            match &self.old_pos_text {
                Some(text) => sc.set_attr(self.elem, "position", text),
                None => sc.set_position(self.elem, p),
            }
        }

        if let Some(e) = self.old_euler {
            match &self.old_euler_text {
                Some(text) => sc.set_attr(self.elem, "orientation", text),
                None => sc.set_euler(self.elem, e),
            }
        }

        t.index_edited(self.elem);
    }

    fn merge_key(&self) -> u64 {
        if self.gesture != 0 { self.gesture ^ ((self.elem as u64) << 20) } else { 0 }
    }

    fn merge_from(&mut self, next: &dyn Command<Tile>) -> bool {
        let Some(n) = next.as_any().downcast_ref::<Self>().filter(|n| n.elem == self.elem) else { return false };
        if n.new_pos.is_some() {
            self.new_pos = n.new_pos;
        }

        if n.new_euler.is_some() {
            self.new_euler = n.new_euler;
        }

        if self.old_pos.is_none() {
            self.old_pos = n.old_pos;
            self.old_pos_text = n.old_pos_text.clone();
        }

        if self.old_euler.is_none() {
            self.old_euler = n.old_euler;
            self.old_euler_text = n.old_euler_text.clone();
        }

        true
    }

    fn dirty_items(&self) -> Vec<u64> {
        if self.whole { vec![] } else { vec![item_of(self.elem)] }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A move of one polygon point; the steps of one gesture merge.
pub(crate) struct TedPointCommand {
    pub elem: usize,
    pub index: usize,
    pub before: Double3,
    pub after: Double3,

    /// The point's position text and the polygon's area text (an edit recomputes the area, which need not be what the file had).
    pub before_text: Option<String>,
    pub area_text: Option<String>,
    pub gesture: u64,
}

impl Command<Tile> for TedPointCommand {
    fn name(&self) -> String {
        format!("Move polygon point {}", self.index)
    }

    fn apply(&mut self, t: &mut Tile) {
        let _ = t.project.scene.set_polygon_point(self.elem, self.index, self.after);
    }

    fn revert(&mut self, t: &mut Tile) {
        let sc = &mut t.project.scene;
        let Some(text) = &self.before_text else {
            let _ = sc.set_polygon_point(self.elem, self.index, self.before);
            return;
        };

        let point = sc.polygon_points(self.elem)[self.index];
        sc.set_attr(point, "position", text);
        if let Some(area) = &self.area_text {
            sc.set_attr(self.elem, "area", area);
        }
    }

    fn merge_key(&self) -> u64 {
        if self.gesture != 0 { self.gesture ^ ((self.elem as u64) << 20) ^ ((self.index as u64) << 44) } else { 0 }
    }

    fn merge_from(&mut self, next: &dyn Command<Tile>) -> bool {
        let Some(n) = next.as_any().downcast_ref::<Self>().filter(|n| n.elem == self.elem && n.index == self.index) else { return false };
        self.after = n.after;
        true
    }

    fn dirty_items(&self) -> Vec<u64> {
        vec![item_of(self.elem)]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Attributes of points of one polygon or spline (a spline point's position and handles, its width ...); the steps of one gesture merge.
pub(crate) struct TedPointAttrsCommand {
    pub name: String,
    pub elem: usize,

    /// (point index, attribute, text before, text after)
    pub steps: Vec<(usize, String, String, String)>,
    pub gesture: u64,
}

impl TedPointAttrsCommand {
    fn put(&self, t: &mut Tile, after: bool) {
        let sc = &mut t.project.scene;
        let points = sc.polygon_points(self.elem);
        for (i, attr, old, new) in &self.steps {
            if let Some(&p) = points.get(*i) {
                sc.set_attr(p, attr, if after { new } else { old });
            }
        }
    }
}

impl Command<Tile> for TedPointAttrsCommand {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn apply(&mut self, t: &mut Tile) {
        self.put(t, true);
    }

    fn revert(&mut self, t: &mut Tile) {
        self.put(t, false);
    }

    fn merge_key(&self) -> u64 {
        if self.gesture != 0 { self.gesture ^ ((self.elem as u64) << 20) } else { 0 }
    }

    fn merge_from(&mut self, next: &dyn Command<Tile>) -> bool {
        let Some(n) = next.as_any().downcast_ref::<Self>().filter(|n| n.elem == self.elem && n.steps.len() == self.steps.len()) else { return false };
        if n.steps.iter().zip(&self.steps).any(|(a, b)| a.0 != b.0 || a.1 != b.1) {
            return false;
        }

        for (mine, theirs) in self.steps.iter_mut().zip(&n.steps) {
            mine.3.clone_from(&theirs.3);
        }

        true
    }

    fn dirty_items(&self) -> Vec<u64> {
        vec![item_of(self.elem)]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A structural edit changes element indices; its undo record is the list of text splices it made (a few hundred bytes, not a copy of the
/// document). The scene already holds the result when the command is pushed.
pub(crate) struct TedSpliceCommand {
    pub name: String,
    pub record: Vec<XmlSplice>,
    pub sel_before: Vec<u64>,
    pub sel_after: Vec<u64>,
    pub first: bool,
}

impl Command<Tile> for TedSpliceCommand {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn apply(&mut self, t: &mut Tile) {
        if self.first {
            self.first = false;
        } else {
            let _ = t.project.scene.apply(&self.record);
        }

        t.idx.valid = false;
        t.structure_version += 1;
        t.piece_props.clear();
        t.hint = Some(self.sel_after.clone());
    }

    fn revert(&mut self, t: &mut Tile) {
        let _ = t.project.scene.unapply(&self.record);
        t.idx.valid = false;
        t.structure_version += 1;
        t.piece_props.clear();
        t.hint = Some(self.sel_before.clone());
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// One property of the Property Editor: the attribute texts of one element (the property and its twin) are replaced, undo puts the old bytes
/// back. The steps of one drag of a slider merge.
pub(crate) struct TedAttrCommand {
    pub name: String,
    pub elem: usize,

    /// (attribute, text before, text after)
    pub steps: Vec<(String, String, String)>,
    pub gesture: u64,

    /// The edit changes what is drawn from the element index (a building's key): the index and the models are rebuilt.
    pub rebuild: bool,
}

impl TedAttrCommand {
    fn put(&self, t: &mut Tile, after: bool) {
        for (attr, old, new) in &self.steps {
            let text = if after { new } else { old };
            t.project.scene.set_attr(self.elem, attr, text);
            t.sync_pieces(self.elem, attr, text);
        }

        if self.rebuild {
            t.idx.valid = false;
            t.structure_version += 1;
        } else {
            t.index_attr_edited(self.elem);
        }
    }
}

impl Command<Tile> for TedAttrCommand {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn apply(&mut self, t: &mut Tile) {
        self.put(t, true);
    }

    fn revert(&mut self, t: &mut Tile) {
        self.put(t, false);
    }

    fn merge_key(&self) -> u64 {
        if self.gesture != 0 { self.gesture ^ ((self.elem as u64) << 20) } else { 0 }
    }

    fn merge_from(&mut self, next: &dyn Command<Tile>) -> bool {
        let Some(n) = next.as_any().downcast_ref::<Self>().filter(|n| n.elem == self.elem && n.steps.len() == self.steps.len()) else { return false };
        if n.steps.iter().zip(&self.steps).any(|(a, b)| a.0 != b.0) {
            return false;
        }

        for (mine, theirs) in self.steps.iter_mut().zip(&n.steps) {
            mine.2.clone_from(&theirs.2);
        }

        true
    }

    fn dirty_items(&self) -> Vec<u64> {
        if self.rebuild { vec![] } else { vec![item_of(self.elem)] }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// One property of several elements at once (the Inspector with several items selected): each element's [`TedAttrCommand`] in one undo
/// entry. The steps of one drag merge.
pub(crate) struct TedAttrsCommand {
    pub name: String,
    pub edits: Vec<TedAttrCommand>,
    pub gesture: u64,
}

impl Command<Tile> for TedAttrsCommand {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn apply(&mut self, t: &mut Tile) {
        for e in &self.edits {
            e.put(t, true);
        }
    }

    fn revert(&mut self, t: &mut Tile) {
        for e in self.edits.iter().rev() {
            e.put(t, false);
        }
    }

    fn merge_key(&self) -> u64 {
        self.gesture
    }

    fn merge_from(&mut self, next: &dyn Command<Tile>) -> bool {
        let Some(n) = next.as_any().downcast_ref::<Self>().filter(|n| n.edits.len() == self.edits.len()) else { return false };
        let same = n.edits.iter().zip(&self.edits).all(|(a, b)| a.elem == b.elem && a.steps.len() == b.steps.len() && a.steps.iter().zip(&b.steps).all(|(x, y)| x.0 == y.0));
        if !same {
            return false;
        }

        for (mine, theirs) in self.edits.iter_mut().zip(&n.edits) {
            for (m, t) in mine.steps.iter_mut().zip(&theirs.steps) {
                m.2.clone_from(&t.2);
            }
        }

        true
    }

    fn dirty_items(&self) -> Vec<u64> {
        if self.edits.iter().any(|e| e.rebuild) { vec![] } else { self.edits.iter().map(|e| item_of(e.elem)).collect() }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The eye / cursor of one or more layers or instances: the flags text is replaced, so undo restores the original bytes. Visibility changes what
/// is drawn below the element, so the document republishes everything (no dirty items).
pub(crate) struct TedFlagCommand {
    pub name: String,
    pub elems: Vec<usize>,
    pub old: Vec<Option<String>>,
    pub new: Vec<Option<String>>,
}

impl TedFlagCommand {
    fn put(elems: &[usize], v: &[Option<String>], t: &mut Tile) {
        for (&e, value) in elems.iter().zip(v) {
            if let Some(value) = value {
                t.project.scene.set_attr(e, "flags", value);
            }
        }

        t.idx.valid = false;
        t.structure_version += 1;
    }
}

impl Command<Tile> for TedFlagCommand {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn apply(&mut self, t: &mut Tile) {
        Self::put(&self.elems, &self.new, t);
    }

    fn revert(&mut self, t: &mut Tile) {
        Self::put(&self.elems, &self.old, t);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// One instance of a placement edit.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlacementItem {
    pub elem: usize,
    pub mode_old: Option<String>,
    pub mode_new: Option<String>,
    pub pos_old: Option<Double3>,
    pub pos_new: Option<Double3>,
    pub idx_old: u8,
    pub idx_new: u8,

    /// Undo puts these bytes back.
    pub pos_old_text: Option<String>,
}

/// Placement edits of several instances at once: surface_mode and the local position of each (a landscape instance is snapped to the terrain
/// when it becomes one). Undo puts the old attribute texts back.
pub(crate) struct TedPlacementCommand {
    pub name: String,
    pub items: Vec<PlacementItem>,
}

impl TedPlacementCommand {
    fn put(&self, t: &mut Tile, forward: bool) {
        for i in &self.items {
            let (mode, pos) = if forward { (&i.mode_new, i.pos_new) } else { (&i.mode_old, i.pos_old) };
            if let Some(mode) = mode {
                t.project.scene.set_attr(i.elem, "surface_mode", mode);
            }

            match (pos, &i.pos_old_text) {
                (Some(_), Some(text)) if !forward => t.project.scene.set_attr(i.elem, "position", text),
                (Some(pos), _) => t.project.scene.set_position(i.elem, pos),
                _ => {}
            }

            if t.idx.valid && i.elem < t.idx.mode.len() {
                t.idx.mode[i.elem] = if forward { i.idx_new } else { i.idx_old };
            }

            if pos.is_some() {
                t.index_edited(i.elem);
            }
        }
    }
}

impl Command<Tile> for TedPlacementCommand {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn apply(&mut self, t: &mut Tile) {
        self.put(t, true);
    }

    fn revert(&mut self, t: &mut Tile) {
        self.put(t, false);
    }

    fn dirty_items(&self) -> Vec<u64> {
        self.items.iter().map(|i| item_of(i.elem)).collect()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// One brush step; the steps of a stroke merge into one entry. The first step runs when pushed; undo and redo use the height field's own
/// records (`HeightField::end_edit` stores them, so a stroke is closed before it can be undone). The landscape instances under the brush move
/// with the terrain: the command keeps their local positions before the stroke and after its last step.
pub(crate) struct BrushStepCommand {
    pub kind: BrushKind,
    pub x: f32,
    pub z: f32,
    pub settings: BrushSettings,
    pub dt: f32,
    pub stroke: u64,
    pub first: bool,
    pub moved: Vec<Follow>,
    pub where_: HashMap<usize, usize>,
}

impl Command<Tile> for BrushStepCommand {
    fn name(&self) -> String {
        let n = match self.kind {
            BrushKind::Raise => "Raise",
            BrushKind::Lower => "Lower",
            BrushKind::Smooth => "Smooth",
            BrushKind::Flatten => "Flatten",
            BrushKind::Level => "Level",
            BrushKind::Noise => "Noise",
            BrushKind::Stamp => "Stamp",
            BrushKind::Paint(_) => return "Paint ground".to_owned(),
            BrushKind::Ground(_) => return "Paint ground types".to_owned(),
        };

        format!("Terrain {n}")
    }

    fn apply(&mut self, t: &mut Tile) {
        if self.first {
            self.first = false;
            t.apply_brush_following(self.kind, self.x, self.z, &self.settings, self.dt, &mut self.moved);
        } else {
            t.end_stroke_edit();
            match self.kind {
                BrushKind::Paint(_) => {
                    if let Some(b) = &mut t.project.blend {
                        b.redo();
                    }
                }

                BrushKind::Ground(_) => {
                    if let Some(g) = &mut t.project.ground {
                        g.redo();
                    }
                }

                _ => {
                    if let Some(h) = &mut t.project.height {
                        h.redo();
                    }
                }
            }

            t.put_positions(&self.moved, true);
        }
    }

    fn revert(&mut self, t: &mut Tile) {
        t.end_stroke_edit();
        match self.kind {
            BrushKind::Paint(_) => {
                if let Some(b) = &mut t.project.blend {
                    b.undo();
                }
            }

            BrushKind::Ground(_) => {
                if let Some(g) = &mut t.project.ground {
                    g.undo();
                }
            }

            _ => {
                if let Some(h) = &mut t.project.height {
                    h.undo();
                }
            }
        }

        t.put_positions(&self.moved, false);
    }

    fn merge_key(&self) -> u64 {
        self.stroke
    }

    fn merge_from(&mut self, next: &dyn Command<Tile>) -> bool {
        let Some(n) = next.as_any().downcast_ref::<Self>() else { return false };
        if self.where_.len() != self.moved.len() {
            for (i, f) in self.moved.iter().enumerate() {
                self.where_.insert(f.elem, i);
            }
        }

        for f in &n.moved {
            match self.where_.get(&f.elem) {
                Some(&i) => self.moved[i].after = f.after,
                None => {
                    self.where_.insert(f.elem, self.moved.len());
                    self.moved.push(f.clone());
                }
            }
        }

        true
    }

    fn dirty_items(&self) -> Vec<u64> {
        std::iter::once(TERRAIN).chain(self.moved.iter().map(|f| item_of(f.elem))).collect()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// One of the entry's four texture groups (the blend map's groups) set to another of the game's; the ground's material is made again.
pub(crate) struct TextureGroupCommand {
    pub slot: usize,
    pub before: String,
    pub after: String,
}

impl TextureGroupCommand {
    fn put(&self, t: &mut Tile, name: &str) {
        if let Some(entry) = t.project.entry.as_mut() {
            entry.set_texture_group(self.slot, name);
        }

        t.material_base = None;
        t.skin = None;
    }
}

impl Command<Tile> for TextureGroupCommand {
    fn name(&self) -> String {
        format!("Texture group {}: {}", wce_formats::tile_entry::TileEntry::TEXTURE_GROUP_SLOTS[self.slot], self.after)
    }

    fn apply(&mut self, t: &mut Tile) {
        self.put(t, &self.after.clone());
    }

    fn revert(&mut self, t: &mut Tile) {
        self.put(t, &self.before.clone());
    }

    fn dirty_items(&self) -> Vec<u64> {
        vec![TERRAIN]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// An edit of an attribute of the tile's database entry (Tile properties): of its `<TILE>` element, or of its variation (`variation`).
pub(crate) struct EntryAttrCommand {
    pub variation: Option<usize>,
    pub attr: String,
    pub before: String,
    pub after: String,
    pub gesture: u64,
}

impl EntryAttrCommand {
    fn put(&self, t: &mut Tile, value: &str) {
        if let Some(entry) = t.project.entry.as_mut() {
            match self.variation {
                Some(v) => entry.set_variation_attr(v, &self.attr, value),
                None => entry.set_tile_attr(&self.attr, value),
            };
        }

        if self.attr == "normal_strength" {
            t.project.info.normal_strength = value.trim().parse().unwrap_or(t.project.info.normal_strength);
            t.material_base = None;
            t.skin = None;
        }

        t.links_dirty = true;
    }
}

impl Command<Tile> for EntryAttrCommand {
    fn name(&self) -> String {
        format!("Tile property {}: {}", self.attr, self.after)
    }

    fn apply(&mut self, t: &mut Tile) {
        self.put(t, &self.after.clone());
    }

    fn revert(&mut self, t: &mut Tile) {
        self.put(t, &self.before.clone());
    }

    fn merge_key(&self) -> u64 {
        if self.gesture != 0 { self.gesture ^ 0x5052_4f50_0000_0000 } else { 0 }
    }

    fn merge_from(&mut self, next: &dyn Command<Tile>) -> bool {
        let Some(n) = next.as_any().downcast_ref::<Self>().filter(|n| n.attr == self.attr && n.variation == self.variation) else { return false };
        self.after = n.after.clone();
        true
    }

    fn dirty_items(&self) -> Vec<u64> {
        vec![TERRAIN]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// An edit of the tile's links (its database entry's `TILE_LINK_TARGETS` and `TILE_LINKS`): both lists before and after.
pub(crate) struct LinksCommand {
    pub name: String,
    pub before: (Vec<TileLinkTarget>, Vec<TileLink>),
    pub after: (Vec<TileLinkTarget>, Vec<TileLink>),

    /// The steps of one field's edit (typing a number) merge.
    pub gesture: u64,
}

impl LinksCommand {
    fn put(t: &mut Tile, v: &(Vec<TileLinkTarget>, Vec<TileLink>)) {
        if let Some(entry) = t.project.entry.as_mut()
            && let Err(err) = entry.set_links(&v.0, &v.1)
        {
            wce_foundation::log::warn(format!("tile links: {err}"));
        }

        t.links_dirty = true;
    }
}

impl Command<Tile> for LinksCommand {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn apply(&mut self, t: &mut Tile) {
        Self::put(t, &self.after);
    }

    fn merge_key(&self) -> u64 {
        if self.gesture != 0 { self.gesture ^ 0x4c49_4e4b_0000_0000 } else { 0 }
    }

    fn merge_from(&mut self, next: &dyn Command<Tile>) -> bool {
        let Some(n) = next.as_any().downcast_ref::<Self>() else { return false };
        self.after = n.after.clone();
        true
    }

    fn revert(&mut self, t: &mut Tile) {
        Self::put(t, &self.before);
    }

    fn dirty_items(&self) -> Vec<u64> {
        vec![TERRAIN]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A whole-map edit of the ground type map (the water ground types painted from the depth): undo and redo use the field's own records,
/// as the brush strokes do; the edit is made before the command is pushed.
pub(crate) struct GroundFillCommand {
    pub name: String,
    pub first: bool,
}

impl Command<Tile> for GroundFillCommand {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn apply(&mut self, t: &mut Tile) {
        if std::mem::take(&mut self.first) {
            return;
        }

        if let Some(g) = &mut t.project.ground {
            g.redo();
        }
    }

    fn revert(&mut self, t: &mut Tile) {
        if let Some(g) = &mut t.project.ground {
            g.undo();
        }
    }

    fn dirty_items(&self) -> Vec<u64> {
        vec![TERRAIN]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A whole-map edit of the heights (a height map imported): undo and redo use the height field's records; the landscape instances moved
/// with the ground go back and forth with them. The edit is made before the command is pushed.
pub(crate) struct HeightFillCommand {
    pub name: String,
    pub first: bool,
    pub moved: Vec<Follow>,
}

impl Command<Tile> for HeightFillCommand {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn apply(&mut self, t: &mut Tile) {
        if std::mem::take(&mut self.first) {
            return;
        }

        t.end_stroke_edit();
        if let Some(h) = &mut t.project.height {
            h.redo();
        }

        t.put_positions(&self.moved, true);
    }

    fn revert(&mut self, t: &mut Tile) {
        t.end_stroke_edit();
        if let Some(h) = &mut t.project.height {
            h.undo();
        }

        t.put_positions(&self.moved, false);
    }

    fn dirty_items(&self) -> Vec<u64> {
        std::iter::once(TERRAIN).chain(self.moved.iter().map(|f| item_of(f.elem))).collect()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
