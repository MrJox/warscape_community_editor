//! A battle map as an editor document: where it comes from, its model (a kit map's folder read into `wce_terrain::MapModel`; a map of the
//! packs has only its built files, shown read-only), the placements it shows (BOB's fill of a kit map, the packs' tile map) and its battle
//! locations. What the map looks like is not published by the document: the Terrain tool streams it around the camera on its own layer
//! (`wce_map_view::MapView`).

use std::any::Any;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use wce_editor_core::command::{CommandStack, History};
use wce_editor_core::document::{Document, DocumentCore, SaveTarget};
use wce_scene::render_scene::{LayerId, RenderScene};
use wce_terrain::MapModel;
use wce_tile::battle_map::context::PlacedMap;
use wce_tile::battle_map::locations::BattleLocations;
use wce_tile::battle_map::tile_db::TileDatabase;

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

    /// The tile database the map was filled with (its sets', tiles' and climates' colours name the cells).
    pub db: Arc<TileDatabase>,

    /// Where Save writes the map's folder (the projects' kit layout: `<projects>/assembly_kit/raw_data/terrain/battles/<map>`, the first
    /// place every loader looks), and what the last save said.
    pub save_dir: Option<PathBuf>,
    message: Option<String>,
}

impl MapDocument {
    pub fn new(name: String, source: MapSource, model: Option<MapModel>, placed: Arc<PlacedMap>, locations: Option<BattleLocations>, db: Arc<TileDatabase>) -> Self {
        Self { core: DocumentCore::new(), history: CommandStack::new(model), name, source, placed, locations, db, save_dir: None, message: None }
    }

    /// The map's files as data (a kit map; none for a map of the packs).
    pub fn model(&self) -> Option<&MapModel> {
        self.history.model().as_ref()
    }

    /// A field of the model to paint on (a kit map's LF; none for a map of the packs). The dabs of a stroke change it directly (the
    /// file is marked edited); the finished stroke goes on the history with `push_stroke`.
    pub fn lf_field_mut(&mut self, role: wce_terrain::LfRole) -> Option<&mut wce_terrain::paged::PagedU16> {
        self.history.model_mut().as_mut().and_then(|m| m.lf_mut(role)).map(|item| item.edit())
    }

    pub fn lf_field(&self, role: wce_terrain::LfRole) -> Option<&wce_terrain::paged::PagedU16> {
        self.model().and_then(|m| m.lf(role)).map(|item| item.get())
    }

    /// A finished stroke onto the history (it is in the field already: applying it again changes nothing).
    pub fn push_stroke(&mut self, cmd: crate::lf::LfStrokeCommand) {
        use wce_editor_core::command::History;
        self.history.push(Box::new(cmd));
        self.history.break_merge();
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

    fn can_save(&self) -> bool {
        self.model().is_some() && self.save_dir.is_some()
    }

    fn save_message(&self) -> Option<String> {
        self.message.clone()
    }

    /// The map's folder into the projects' kit layout: the files made again from the model where they were edited (the others as they
    /// were: copied the first time, then left when the folder has them at the same size).
    fn save(&mut self, target: &SaveTarget) -> Result<(), String> {
        let dir = if target.path.as_os_str().is_empty() { self.save_dir.clone().ok_or("no projects folder to save into")? } else { target.path.clone() };
        let model = self.model().ok_or("a map of the game's packs is read-only")?;
        let files = wce_terrain_io::attila::write(model).map_err(|e| e.to_string())?;
        let (mut written, mut kept) = (0, 0);
        for (rel, blob) in &files.folder {
            let to = dir.join(rel);
            if let wce_tile_model::Blob::File(src) = blob {
                let same_size = std::fs::metadata(src).ok().zip(std::fs::metadata(&to).ok()).is_some_and(|(a, b)| a.len() == b.len());
                if *src == to || same_size {
                    kept += 1;
                    continue;
                }
            }

            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            }

            match blob {
                wce_tile_model::Blob::File(src) => std::fs::copy(src, &to).map(|_| ()),
                wce_tile_model::Blob::Bytes(b) => std::fs::write(&to, b),
            }
            .map_err(|e| format!("{}: {e}", to.display()))?;
            written += 1;
        }

        self.history.mark_saved();
        self.message = Some(format!("{}: {written} files written, {kept} already there", dir.display()));
        Ok(())
    }

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
