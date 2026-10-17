//! The weather tool's sandbox document: the sample map with houses that can be selected, moved and rotated like any document's items. Item 0
//! is the ground, items 1..N the houses. Placements are edits on top of the generated layout; they go through the command stack (undo, redo,
//! merged drags) but are not saved, the layout is generated from the seed.

use std::any::Any;
use std::collections::BTreeMap;

use wce_editor_core::command::{Command, CommandStack, History};
use wce_editor_core::document::{Document, DocumentCore, OutlinerNode};
use wce_editor_core::inspector::Inspector;
use wce_foundation::math::{Float3, Float4x4, rotation_y, translation};
use wce_scene::render_scene::{LayerId, Payload, RenderScene, SourceRef};
use wce_vfs::files::Files;

use crate::sample_map::{BUILDINGS, SampleMapResult, publish_sample_map};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Placement {
    /// Metres, world space.
    pub offset: Float3,

    /// Radians about the house's centre.
    pub yaw: f32,
}

pub type Placements = BTreeMap<u64, Placement>;

struct PlacementCommand {
    item: u64,
    before: Placement,
    after: Placement,
    key: u64,
}

impl Command<Placements> for PlacementCommand {
    fn name(&self) -> String {
        format!("Move building {}", self.item)
    }

    fn apply(&mut self, placements: &mut Placements) {
        placements.insert(self.item, self.after);
    }

    fn revert(&mut self, placements: &mut Placements) {
        placements.insert(self.item, self.before);
    }

    fn merge_key(&self) -> u64 {
        self.key
    }

    fn merge_from(&mut self, next: &dyn Command<Placements>) -> bool {
        match next.as_any().downcast_ref::<Self>() {
            Some(n) if n.item == self.item => {
                self.after = n.after;
                true
            }

            _ => false,
        }
    }

    fn dirty_items(&self) -> Vec<u64> {
        vec![self.item]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// An entity of the generated layout.
struct Base {
    payload: Payload,
    world: Float4x4,
    item: u64,
}

pub struct SampleMapDocument {
    core: DocumentCore,
    history: CommandStack<Placements>,
    seed: u32,
    info: SampleMapResult,
    base: Vec<Base>,
    pieces: BTreeMap<u64, usize>,
}

impl SampleMapDocument {
    pub fn new(files: &dyn Files, seed: u32, count: usize) -> Self {
        let core = DocumentCore::new();
        let mut tmp = RenderScene::default();
        let l = tmp.add_layer(core.id(), "");
        let info = publish_sample_map(files, &mut tmp, l, core.id(), seed, count, BUILDINGS);
        let mut pieces = BTreeMap::new();
        let base = tmp.entities().iter().map(|e| {
            *pieces.entry(e.source.item).or_default() += 1;
            Base { payload: e.payload.clone(), world: e.world, item: e.source.item }
        }).collect();

        Self { core, history: CommandStack::default(), seed, info, base, pieces }
    }

    pub fn info(&self) -> &SampleMapResult {
        &self.info
    }

    pub fn placement(&self, item: u64) -> Placement {
        self.history.model().get(&item).copied().unwrap_or_default()
    }

    /// One undo entry (merged per gesture).
    pub fn set_placement(&mut self, item: u64, p: Placement, gesture: u64) {
        let before = self.placement(item);
        if before != p {
            self.history.push(Box::new(PlacementCommand { item, before, after: p, key: gesture }));
        }
    }

    fn centre(&self, item: u64) -> Option<Float3> {
        (item >= 1).then(|| self.info.centres.get(item as usize - 1).copied()).flatten()
    }

    /// What is applied to the generated world transform of the item's entities.
    fn placement_matrix(&self, item: u64) -> Float4x4 {
        let p = self.placement(item);
        let Some(c) = self.centre(item) else { return Float4x4::IDENTITY };
        translation(p.offset) * translation(c) * rotation_y(p.yaw) * translation(-c)
    }

    fn pieces_of(&self, item: u64) -> usize {
        self.pieces.get(&item).copied().unwrap_or(0)
    }
}

impl Document for SampleMapDocument {
    fn core(&self) -> &DocumentCore {
        &self.core
    }

    fn core_mut(&mut self) -> &mut DocumentCore {
        &mut self.core
    }

    fn history(&self) -> &dyn History {
        &self.history
    }

    fn history_mut(&mut self) -> &mut dyn History {
        &mut self.history
    }

    fn title(&self) -> String {
        format!("Sample map (seed {})", self.seed)
    }

    fn kind(&self) -> String {
        "sample_map".to_owned()
    }

    /// Hidden while a tile is open.
    fn stand_in(&self) -> bool {
        true
    }

    fn publish(&mut self, scene: &mut RenderScene, layer: LayerId) {
        scene.clear_layer(layer);
        for b in &self.base {
            scene.add(layer, self.placement_matrix(b.item) * b.world, b.payload.clone(), SourceRef { doc: self.id(), item: b.item });
        }
    }

    fn outline(&self) -> OutlinerNode {
        let node = |label: String, item: u64| OutlinerNode { label, source: SourceRef { doc: self.id(), item }, selectable: true, children: vec![] };
        let mut root = OutlinerNode { label: self.title(), source: SourceRef { doc: self.id(), item: 0 }, selectable: false, children: vec![node("Ground".to_owned(), 0)] };
        for i in 1..=self.info.centres.len() as u64 {
            root.children.push(node(self.describe(SourceRef { doc: self.id(), item: i }), i));
        }

        root
    }

    fn describe(&self, item: SourceRef) -> String {
        if item.item == 0 {
            return "Ground".to_owned();
        }

        format!("Building {} ({} pieces)", item.item, self.pieces_of(item.item))
    }

    fn inspect(&mut self, item: SourceRef, ins: &mut dyn Inspector) {
        if item.item == 0 {
            ins.section("Ground");
            ins.text("model", "terrain", "flat, 2000 m, tiled grass");
            return;
        }

        ins.section("Placement");
        let p = self.placement(item.item);
        let mut off = [p.offset.x, p.offset.y, p.offset.z];

        // Edits made in the inspector merge per field until the widget is released.
        let key = (u64::from(self.id()) << 40) ^ (item.item << 8) ^ 0x51;
        if ins.edit_float3("offset", "offset (m)", &mut off, 0.5) {
            let n = Placement { offset: Float3::new(off[0], off[1], off[2]), ..p };
            self.history.push(Box::new(PlacementCommand { item: item.item, before: p, after: n, key }));
        }

        if ins.gesture_ended() {
            self.history.break_merge();
        }

        let mut yaw_deg = p.yaw * 57.295_78;
        if ins.edit_float("yaw", "yaw (deg)", &mut yaw_deg, 0.5, -3600.0, 3600.0) {
            let n = Placement { yaw: yaw_deg / 57.295_78, ..p };
            self.history.push(Box::new(PlacementCommand { item: item.item, before: p, after: n, key: key + 1 }));
        }

        if ins.gesture_ended() {
            self.history.break_merge();
        }

        ins.section("Source");
        ins.text("pieces", "pieces", &self.pieces_of(item.item).to_string());
        if let Some(c) = self.centre(item.item) {
            ins.text("grid", "grid centre", &format!("{}, {}", c.x as i32, c.z as i32));
        }
    }

    fn item_transform(&self, item: SourceRef) -> Option<Float4x4> {
        let c = self.centre(item.item)?;
        let p = self.placement(item.item);
        Some(translation(p.offset + c) * rotation_y(p.yaw))
    }

    fn transform_items(&mut self, items: &[SourceRef], delta: &Float4x4, gesture: u64) {
        for &r in items {
            if r.doc != self.id() {
                continue;
            }

            let (Some(before), Some(c)) = (self.item_transform(r), self.centre(r.item)) else { continue };
            let now = *delta * before;
            let n = Placement { offset: Float3::new(now.m[0][3] - c.x, now.m[1][3] - c.y, now.m[2][3] - c.z), yaw: now.m[0][2].atan2(now.m[0][0]) };
            self.set_placement(r.item, n, gesture);
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
