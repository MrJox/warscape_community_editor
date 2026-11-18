//! Biome areas in a tile (docs/plans/auto_detail.md, C): an area is a vegetation exclusion polygon with `wce_biome` (the biome's name) and
//! `wce_biome_seed`; the tile keeps the biomes its areas use (`TileProject::biomes`, their own file). Taking a biome in is an undo entry
//! ([`BiomesCommand`]).

use std::any::Any;

use wce_editor_core::command::Command;
use wce_tile::biomes::{AREA_TYPE, BIOME_ATTR, Biome, SEED_ATTR};

use super::{TERRAIN, Tile, TileDocument};

/// An area: its element, its biome's name, its seed and its polygon (world metres, x and z).
#[derive(Clone, Debug, PartialEq)]
pub struct BiomeArea {
    pub elem: usize,
    pub biome: String,
    pub seed: u64,
    pub points: Vec<(f32, f32)>,
}

impl Tile {
    /// The tile's biome areas, in the scene's order (hidden ones too: their contents go with their visibility, the caller's to say).
    #[must_use]
    pub fn biome_areas(&self) -> Vec<BiomeArea> {
        let sc = self.tscene();
        sc.find_all(AREA_TYPE)
            .into_iter()
            .filter_map(|e| {
                let biome = sc.get(e, BIOME_ATTR).filter(|b| !b.is_empty())?;
                let seed = sc.get(e, SEED_ATTR).and_then(|s| s.trim().parse().ok()).unwrap_or(e as u64);
                let points = self.polygon_world_points(e).into_iter().map(|p| (p.x, p.z)).collect();
                Some(BiomeArea { elem: e, biome, seed, points })
            })
            .collect()
    }

    /// A biome by name: the tile's copy, else the built-in one of that name.
    #[must_use]
    pub fn biome_named(&self, name: &str) -> Option<Biome> {
        self.project
            .biomes
            .iter()
            .find(|b| b.name == name)
            .cloned()
            .or_else(|| wce_tile::biomes::builtin().into_iter().find(|b| b.name == name))
    }
}

impl TileDocument {
    /// The biomes the tile keeps.
    #[must_use]
    pub fn tile_biomes(&self) -> &[Biome] {
        &self.project().biomes
    }

    /// Takes a biome into the tile (in place of its copy of that name; a preset's file and folder are not the tile's): one undo entry, none
    /// when the tile has it so already.
    pub fn adopt_biome(&mut self, biome: &Biome) {
        let b = Biome { folder: String::new(), path: None, ..biome.clone() };
        let before = self.project().biomes.clone();
        let mut after = before.clone();
        match after.iter_mut().find(|o| o.name == b.name) {
            Some(o) if *o == b => return,
            Some(o) => *o = b,
            None => after.push(b),
        }

        self.end_stroke();
        self.history.push(Box::new(BiomesCommand { name: format!("Biome {}", biome.name), before, after }));
    }

    /// Takes the tile's biomes no area uses out (one undo entry, none when every one is used).
    pub fn drop_unused_biomes(&mut self) {
        let used = self.tile().biome_areas().into_iter().map(|a| a.biome).collect::<std::collections::HashSet<_>>();
        let before = self.project().biomes.clone();
        let after = before.iter().filter(|b| used.contains(&b.name)).cloned().collect::<Vec<_>>();
        if after.len() != before.len() {
            self.end_stroke();
            self.history.push(Box::new(BiomesCommand { name: "Unused biomes".to_owned(), before, after }));
        }
    }
}

/// A change of the tile's biomes.
pub(crate) struct BiomesCommand {
    name: String,
    before: Vec<Biome>,
    after: Vec<Biome>,
}

impl Command<Tile> for BiomesCommand {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn apply(&mut self, t: &mut Tile) {
        t.project.biomes.clone_from(&self.after);
        t.project.biomes_dirty = true;
    }

    fn revert(&mut self, t: &mut Tile) {
        t.project.biomes.clone_from(&self.before);
        t.project.biomes_dirty = true;
    }

    fn dirty_items(&self) -> Vec<u64> {
        vec![TERRAIN]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
