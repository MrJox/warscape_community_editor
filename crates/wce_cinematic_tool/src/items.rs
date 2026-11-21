//! What can be selected in a scene, as the editor's selection names it (`SourceRef::item`): the battle (its setup and texts), an
//! army, a unit, a VFX instance of a clip. The Scene outliner, the viewport, the Layout and the Inspector all go through these, as
//! the Tile tool's panels go through its tile's elements.

use wce_cinematic::scene::{Project, TrackContent};
use wce_editor::editor::Editor;
use wce_scene::render_scene::{DocumentId, SourceRef};

const KIND_SHIFT: u32 = 56;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SceneItem {
    Battle,
    Army(usize),
    Unit(u32),
    /// A VFX instance: its track (ids are unique in a project) and its index there.
    Vfx {
        track: u32,
        index: usize,
    },
}

impl SceneItem {
    pub fn encode(self) -> u64 {
        let k = |kind: u64, v: u64| (kind << KIND_SHIFT) | v;
        match self {
            SceneItem::Battle => k(1, 0),
            SceneItem::Army(i) => k(2, i as u64),
            SceneItem::Unit(id) => k(3, u64::from(id)),
            SceneItem::Vfx { track, index } => k(4, (u64::from(track) << 24) | (index as u64 & 0xFF_FFFF)),
        }
    }

    pub fn decode(item: u64) -> Option<Self> {
        let v = item & ((1 << KIND_SHIFT) - 1);
        Some(match item >> KIND_SHIFT {
            1 => SceneItem::Battle,
            2 => SceneItem::Army(v as usize),
            3 => SceneItem::Unit(v as u32),
            4 => SceneItem::Vfx { track: (v >> 24) as u32, index: (v & 0xFF_FFFF) as usize },
            _ => return None,
        })
    }

    pub fn source(self, doc: DocumentId) -> SourceRef {
        SourceRef { doc, item: self.encode() }
    }

    /// What the Inspector's title and the outliner call it.
    pub fn describe(self, p: &Project, faction_name: impl Fn(&str) -> String) -> String {
        match self {
            SceneItem::Battle => format!("Battle: {}", p.battle.key),
            SceneItem::Army(i) => {
                p.armies.get(i).map_or_else(|| "Army".into(), |a| format!("Army {}: {}", i + 1, faction_name(&a.faction)))
            }
            SceneItem::Unit(id) => p.unit(id).map_or_else(|| "Unit".into(), |u| format!("Unit: {}", u.name)),
            SceneItem::Vfx { track, index } => vfx_of(p, track, index).map_or_else(
                || "Effect".into(),
                |v| format!("Effect: {}", if v.vfx.is_empty() { "(none picked)" } else { v.vfx.as_str() }),
            ),
        }
    }

    pub fn exists(self, p: &Project) -> bool {
        match self {
            SceneItem::Battle => true,
            SceneItem::Army(i) => i < p.armies.len(),
            SceneItem::Unit(id) => p.unit(id).is_some(),
            SceneItem::Vfx { track, index } => vfx_of(p, track, index).is_some(),
        }
    }
}

/// A VFX instance by its track and index, in whichever clip has the track.
pub fn vfx_of(p: &Project, track: u32, index: usize) -> Option<&wce_cinematic::scene::VfxInstance> {
    p.clips.iter().flat_map(|c| &c.tracks).find(|t| t.id == track).and_then(|t| match &t.content {
        TrackContent::Vfx { instances } => instances.get(index),
        _ => None,
    })
}

pub fn vfx_of_mut(p: &mut Project, track: u32, index: usize) -> Option<&mut wce_cinematic::scene::VfxInstance> {
    p.clips.iter_mut().flat_map(|c| &mut c.tracks).find(|t| t.id == track).and_then(|t| match &mut t.content {
        TrackContent::Vfx { instances } => instances.get_mut(index),
        _ => None,
    })
}

/// The scene items selected (of the scene's document), primary first.
pub fn selected(e: &Editor) -> Vec<SceneItem> {
    let Some(doc) = e.docs.of_tool("cinematic") else { return vec![] };
    e.selection.items().iter().filter(|r| r.doc == doc).filter_map(|r| SceneItem::decode(r.item)).collect()
}

pub fn primary(e: &Editor) -> Option<SceneItem> {
    selected(e).first().copied()
}

/// Selects one item (or adds it / toggles it).
pub fn select(e: &mut Editor, item: SceneItem, how: How) {
    let Some(doc) = e.docs.of_tool("cinematic") else { return };
    let r = item.source(doc);
    match how {
        How::Set => e.selection.set(r),
        How::Add => e.selection.add(r),
        How::Toggle => e.selection.toggle(r),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum How {
    Set,
    Add,
    Toggle,
}

#[cfg(test)]
mod tests {
    use super::SceneItem;

    #[test]
    fn items_survive_their_encoding() {
        for i in [SceneItem::Battle, SceneItem::Army(3), SceneItem::Unit(4_000_000), SceneItem::Vfx { track: 77, index: 12 }] {
            assert_eq!(SceneItem::decode(i.encode()), Some(i));
        }
        assert_eq!(SceneItem::decode(0), None, "the ground's item is none of them");
    }
}
