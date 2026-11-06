//! An edit that adds or removes elements renumbers the ones after them, and all that is kept by element with them. Publishing the tile again
//! took half a second on a big one for one building: the scene is told the new numbers instead, and only what came is built.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::hash::Hash;

use wce_scene::render_scene::{EntityId, LayerId, RenderScene};

use super::{Tile, elem_of, item_of};

/// The item a command says it touched when it renumbered the elements (`Tile::shifts` says how).
pub(crate) const SHIFTED_ITEM: u64 = u64::MAX;

/// Sorted and joined spans (start, length).
pub(crate) fn merged_spans(mut spans: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    spans.sort_unstable();
    let mut out: Vec<(usize, usize)> = vec![];
    for (at, n) in spans {
        match out.last_mut() {
            Some(last) if last.0 + last.1 == at => last.1 += n,
            _ => out.push((at, n)),
        }
    }

    out
}

/// Elements that came or went: `spans` (start, length; sorted) in the numbers of the list with them in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ElementShift {
    pub spans: Vec<(usize, usize)>,
    pub came: bool,
}

/// The old numbers to the new ones.
pub(crate) struct ElementMap {
    to: Vec<Option<usize>>,

    /// The elements that came, in the new numbers.
    came: Vec<usize>,
}

impl ElementShift {
    /// The map of a list of `old_len` elements.
    pub(crate) fn map(&self, old_len: usize) -> ElementMap {
        let moved = self.spans.iter().map(|s| s.1).sum::<usize>();
        let full = if self.came { old_len + moved } else { old_len };
        let inside = |e: usize| {
            let i = self.spans.partition_point(|s| s.0 <= e);
            i != 0 && e < self.spans[i - 1].0 + self.spans[i - 1].1
        };

        if self.came {
            ElementMap { to: (0..full).filter(|&e| !inside(e)).map(Some).collect(), came: (0..full).filter(|&e| inside(e)).collect() }
        } else {
            let mut to = vec![None; full];
            for (new, old) in (0..full).filter(|&e| !inside(e)).enumerate() {
                to[old] = Some(new);
            }

            ElementMap { to, came: vec![] }
        }
    }
}

impl ElementMap {
    /// The new number of an element, `None` when it went.
    pub(crate) fn map(&self, e: usize) -> Option<usize> {
        self.to.get(e).copied().flatten()
    }

    pub(crate) fn keys<V>(&self, kept: &mut HashMap<usize, V>) {
        *kept = std::mem::take(kept).into_iter().filter_map(|(k, v)| Some((self.map(k)?, v))).collect();
    }

    pub(crate) fn set(&self, kept: &mut HashSet<usize>) {
        *kept = kept.iter().filter_map(|&e| self.map(e)).collect();
    }

    pub(crate) fn sorted(&self, kept: &mut BTreeSet<usize>) {
        *kept = kept.iter().filter_map(|&e| self.map(e)).collect();
    }

    fn second<K: Eq + Hash, V>(&self, kept: &mut HashMap<(usize, K), V>) {
        *kept = std::mem::take(kept).into_iter().filter_map(|((e, k), v)| Some(((self.map(e)?, k), v))).collect();
    }
}

impl Tile {
    /// All that is kept by element takes the new numbers.
    fn renumber(&mut self, scene: &mut RenderScene, layer: LayerId, map: &ElementMap, new_len: usize) {
        scene.renumber_sources(layer, |item| match elem_of(item) {
            Some(e) => map.map(e).map(item_of),
            None => Some(item),
        });

        let (mut first, mut count) = (vec![0; new_len], vec![0; new_len]);
        for e in 0..self.first_entity.len() {
            if let Some(n) = map.map(e) {
                (first[n], count[n]) = (self.first_entity[e], self.entity_count[e]);
            }
        }

        (self.first_entity, self.entity_count) = (first, count);
        map.keys(&mut self.outline_entity);
        map.second(&mut self.piece_props);
        map.second(&mut self.drafts);
        self.piece_pending = self.piece_pending.iter().filter_map(|&(e, k)| Some((map.map(e)?, k))).collect();
        self.unlocked_reference = self.unlocked_reference.and_then(|e| map.map(e));
        self.covers_shifted(map);
        self.columns_shifted(map);
        self.tech_shifted(map);
        self.walls_shifted(map);
        self.fire_shifted(map);
    }

    /// The scene follows the one renumbering waiting (`Tile::shifts`): the entities of what stays stay, what went goes, what came is added.
    pub(crate) fn publish_shift(&mut self, scene: &mut RenderScene, layer: LayerId) {
        wce_foundation::wce_prof!("tile.publish_shift");
        let new_len = self.tscene().doc().elements().len();
        let old_len = self.first_entity.len();
        let moved = self.shifts.first().map_or(0, |s| s.spans.iter().map(|s| s.1).sum::<usize>());
        let fits = self.shifts.first().is_some_and(|s| if s.came { old_len + moved == new_len } else { old_len == new_len + moved });
        if !fits || self.pieces_collapsing() || self.idx.type_.len() != old_len {
            self.rebuild_items(true);
            self.publish_items(scene, layer, true);
            return;
        }

        let map = self.shifts[0].map(old_len);
        self.shifts.clear();

        // What went, while the index still says what it was.
        let names = self.idx.type_names.clone();
        let building = names.iter().position(|t| t == "BUILDING_INSTANCE");
        let river = names.iter().position(|t| t == "SplineRiverInstance");
        let went = (0..old_len).filter(|&e| map.map(e).is_none()).map(|e| self.idx.type_[e] as usize).collect::<BTreeSet<_>>();
        self.renumber(scene, layer, &map, new_len);

        let dirty = std::mem::take(&mut self.dirty_type);
        self.rebuild_tables(true);
        if self.idx.type_names != names || dirty.len() != self.dirty_type.len() {
            self.columns_changed();
            self.tech.dirty = true;
            self.markers_dirty = true;
            self.polygons_dirty = true;
            self.publish_items(scene, layer, true);
            return;
        }

        self.dirty_type = dirty;
        self.edits_seen = self.edits_seen.wrapping_add(1);
        self.last_highlight = None;
        self.last_mask = None;
        self.meshed_for = (vec![], u64::MAX);
        self.links_dirty = true;

        let hide_buildings = self.display.hidden_types.contains("BUILDING_INSTANCE");
        let mut buildings = false;
        for &t in &went {
            self.dirty_type[t] = 1;
            self.water_dirty |= Some(t) == river;
            if Some(t) == building {
                buildings = true;
                self.columns.dirty = true;
                self.tech.dirty = true;
            }
        }

        for &e in &map.came {
            if !self.is_instance(e) {
                continue;
            }

            self.add_item_entities(scene, layer, e);
            let t = self.idx.type_[e] as usize;
            if self.markers.slot[e].is_some() {
                self.dirty_type[t] = 1;
            }

            self.water_dirty |= Some(t) == river;
            if self.is_wall(e) {
                self.wall_changed(e);
            }

            if Some(t) == building {
                buildings = true;
                self.building_moved(e);
                self.columns_element_changed(e);
                self.tech_element_changed(e);
                if hide_buildings {
                    let first = self.first_entity[e];
                    for id in first..first + EntityId::from(self.entity_count[e]) {
                        if let Some(en) = scene.find_mut(id) {
                            en.visible = false;
                        }
                    }
                }
            }
        }

        // A building is what a wall's end is attached to.
        self.polygons_dirty |= buildings && self.has_walls();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_numbers_follow() {
        let came = ElementShift { spans: vec![(2, 2), (6, 1)], came: true }.map(5);
        assert_eq!((0..5).map(|e| came.map(e)).collect::<Vec<_>>(), [Some(0), Some(1), Some(4), Some(5), Some(7)]);
        assert_eq!(came.came, [2, 3, 6]);

        let went = ElementShift { spans: vec![(2, 2), (6, 1)], came: false }.map(8);
        assert_eq!((0..8).map(|e| went.map(e)).collect::<Vec<_>>(), [Some(0), Some(1), None, None, Some(2), Some(3), None, Some(4)]);
    }

    #[test]
    fn spans_join() {
        assert_eq!(merged_spans(vec![(5, 1), (2, 3), (9, 2)]), [(2, 4), (9, 2)]);
    }
}
