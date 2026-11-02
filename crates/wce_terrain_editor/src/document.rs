//! A battle map as an editor document: where it comes from, its model (a kit map's folder read into `wce_terrain::MapModel`; a map of the
//! packs has only its built files, shown read-only), the placements it shows (BOB's fill of a kit map, the packs' tile map) and its battle
//! locations. What the map looks like is not published by the document: the Terrain tool streams it around the camera on its own layer
//! (`wce_map_view::MapView`).

use std::any::Any;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use wce_editor_core::command::{CommandStack, History};
use wce_editor_core::document::{Document, DocumentCore};
use wce_scene::render_scene::{LayerId, RenderScene};
use wce_terrain::MapModel;
use wce_tile::battle_map::context::PlacedMap;
use wce_tile::battle_map::locations::BattleLocations;

/// Where a map comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapSource {
    /// A kit layout's map folder (`raw_data/terrain/battles/<map>`): the projects' or an Assembly Kit's.
    Kit(PathBuf),

    /// A folder of the game's packs (`terrain/battles/<map>`): its built files, read-only.
    Packs(String),
}

pub struct MapDocument {
    core: DocumentCore,
    history: CommandStack<Option<MapModel>>,

    /// The map's name as the map list shows it.
    pub name: String,
    pub source: MapSource,
    pub placed: Arc<PlacedMap>,
    pub locations: Option<BattleLocations>,
}

impl MapDocument {
    pub fn new(name: String, source: MapSource, model: Option<MapModel>, placed: Arc<PlacedMap>, locations: Option<BattleLocations>) -> Self {
        Self { core: DocumentCore::new(), history: CommandStack::new(model), name, source, placed, locations }
    }

    /// The map's files as data (a kit map; none for a map of the packs).
    pub fn model(&self) -> Option<&MapModel> {
        self.history.model().as_ref()
    }

    /// A line for the panel: the map's size and what it holds.
    pub fn summary(&self) -> String {
        let m = &self.placed;
        let mut s = format!("{} x {} cells, {} placements", m.width, m.height, m.placements.len());
        if let Some(lf) = &m.lf {
            s += &format!(", LF {} x {}", lf.w, lf.h);
        }

        if let Some(l) = &self.locations {
            s += &format!(", {} battle locations", l.types.iter().map(|(_, v)| v.len()).sum::<usize>());
        }

        if let Some(p) = self.model().and_then(|m| m.project.as_ref()) {
            s += &format!(", {} .terry entities", p.get().entities.len());
        }

        s
    }
}

impl Document for MapDocument {
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
        self.name.clone()
    }

    fn kind(&self) -> String {
        "battle_map".to_owned()
    }

    /// Nothing of its own: the tool's map view is on the scene.
    fn publish(&mut self, _scene: &mut RenderScene, _layer: LayerId) {}

    fn source_key(&self) -> String {
        match &self.source {
            MapSource::Packs(folder) => folder.to_lowercase(),
            MapSource::Kit(_) => String::new(),
        }
    }

    fn opened_from(&self, path: &str) -> bool {
        let MapSource::Kit(dir) = &self.source else { return false };
        let p = Path::new(path);
        p == dir || p.parent() == Some(dir.as_path())
    }

    fn shows_scene(&self) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
