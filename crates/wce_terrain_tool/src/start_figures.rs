//! A campaign map's armies, navies and agents at its start, drawn as the game draws them (docs/plans/startpos_editing.md; read only):
//! the characters the map shows (`wce_terrain_io::attila::characters`), each its art set's figure (`wce_render::campaign_character`)
//! idling at its world point, on the ground, turned by its facing. Read on a thread (the startpos and the models take seconds); the
//! figures near the camera are posed every frame, as the Tile tool's reference soldiers (`scene.update_pose`).
//!
//! Size: a model unit is a world unit (capture T2, TDD: the Isengard spy's figure 1.79 world units tall, its model 1.79; the instance
//! matrix's scale 1, turned by the facing, pi).

use std::sync::{Arc, OnceLock};

use wce_editor::editor::Editor;
use wce_foundation::math::{Float3, Float4x4, rotation_y, translation};
use wce_render::campaign_character::{Afloat, CampaignCatalog, CampaignFigure, Wanted};
use wce_scene::render_scene::{DocumentId, EntityId, LayerId, Payload, SourceRef};
use wce_terrain_editor::document::MapDocument;
use wce_tile_editor::tile_document::context::Frame;

/// A character drawn.
struct Placed {
    /// Its world point (x east, y north) and facing (radians).
    world: (f64, f64),
    facing: f32,
    figure: CampaignFigure,

    /// A navy, or an army at sea: afloat, on the sea's level.
    navy: bool,
}

/// What the read gives: the figures, the world's size, and what could not be drawn.
struct Loaded {
    placed: Vec<Placed>,

    /// The armies at sea (their transports'), by world point.
    at_sea: Vec<(f64, f64)>,
    world: (f64, f64),
    notes: Vec<String>,
}

/// The start's figures of the shown campaign map.
pub struct StartFigures {
    pub on: bool,

    /// How far from the camera's target figures are drawn (map cells).
    pub radius_cells: f64,
    /// The read of a document, for a season (winter or not: the art sets' `winter` rows).
    loaded: Option<((DocumentId, bool), Arc<OnceLock<Option<Arc<Loaded>>>>)>,
    layer: LayerId,
    entities: Vec<EntityId>,

    /// Figures drawn, and the start's figures in all.
    pub count: usize,
    pub total: usize,
}

impl Default for StartFigures {
    fn default() -> Self {
        Self { on: true, radius_cells: 160.0, loaded: None, layer: 0, entities: vec![], count: 0, total: 0 }
    }
}

/// The read, on a worker: the campaign's startpos, its characters, the map's settlement cells, the tables and each figure.
/// A land army standing on water (the LF under the sea there) is at sea: its `sea_uniform` (a transport ship), as the game's.
fn load(e: &Editor, name: String, frame: Arc<Frame>, winter: bool) -> Arc<OnceLock<Option<Arc<Loaded>>>> {
    let cell = Arc::new(OnceLock::new());
    let out = cell.clone();
    let game = e.game();
    let (files, dir) = (e.files(), e.settings_ref().game_dir());
    std::thread::spawn(move || {
        let loaded = (|| {
            let files = files?;
            let schema = rpfm_lib::schema::Schema::load(&wce_foundation::settings::rpfm_schema_path(game)?, None).ok()?;
            let list = |p: &str| files.list(p, "");
            let read = |p: &str| files.read(p).ok().map(|b| b.to_vec());
            let reader = crate::tool::startpos_reader(&files, dir.as_deref());
            let start = wce_terrain_io::attila::start::find(&schema, &name, &list, &read, &reader).ok()?;
            let mut chars = wce_terrain_io::attila::characters::read(&start.startpos).ok()?;
            // The map's own slot cells too: a settlement whose startpos leaves its cells unset still keeps its characters in.
            let map_data = files.read(&format!("campaign_maps/{name}/map_data.esf")).ok()?;
            let cities = wce_terrain_io::attila::cities::read(&map_data, &start.startpos, &Default::default()).ok()?;
            chars.settlement_cells.extend(cities.slots.iter().map(|s| (u32::from(s.cell.0), u32::from(s.cell.1))));
            let catalog = CampaignCatalog::load(files.as_ref(), &schema);
            let mut notes: Vec<String> = vec![];
            let mut at_sea = vec![];
            let (w, h) = (f64::from(frame.map.width), f64::from(frame.map.height));
            let on_water = |world: (f64, f64)| {
                let (fx, fz) = frame.from_map.apply(world.0 / cities.world.0 * w, (1.0 - world.1 / cities.world.1) * h);
                frame.lift(fx, fz) < frame.sea_y()
            };
            let placed = chars
                .on_map()
                .filter_map(|c| {
                    let force = chars.force_of(c);
                    let afloat = match force {
                        Some(f) if f.navy => Afloat::Navy,
                        Some(_) if on_water(c.world) => Afloat::Sea,
                        _ => Afloat::Land,
                    };
                    if afloat == Afloat::Sea {
                        at_sea.push(c.world);
                    }
                    let art = if c.art_set.is_empty() {
                        catalog.art_set_for(&c.kind, &c.faction, &c.subculture, &c.culture).unwrap_or_default().to_owned()
                    } else {
                        c.art_set.clone()
                    };
                    let wanted =
                        Wanted { art_set: art, afloat, leader: c.leader, winter, faction: c.faction.clone(), seed: u64::from(c.id) };
                    match catalog.figure(&read, &wanted) {
                        Ok(figure) => Some(Placed { world: c.world, facing: c.facing, figure, navy: afloat != Afloat::Land }),
                        Err(err) => {
                            notes.push(format!("{} {} ({}): {err}", c.kind, c.id, c.faction));
                            None
                        }
                    }
                })
                .collect();
            Some(Arc::new(Loaded { placed, at_sea, world: cities.world, notes }))
        })();
        if let Some(l) = &loaded {
            wce_foundation::log::info(format!(
                "terrain {name}: the start's figures: {} drawn ({} armies at sea: {:?}){}",
                l.placed.len(),
                l.at_sea.len(),
                l.at_sea.iter().take(4).collect::<Vec<_>>(),
                if l.notes.is_empty() { String::new() } else { format!(", {} not: {}", l.notes.len(), l.notes.join("; ")) }
            ));
        } else {
            wce_foundation::log::info(format!("terrain {name}: the start's characters not read"));
        }
        let _ = out.set(loaded);
    });
    cell
}

impl StartFigures {
    /// Waits up to `max` for the read (scripts: `terrain_settle`). False: still reading.
    pub fn wait(&self, max: std::time::Duration) -> bool {
        let Some((_, cell)) = &self.loaded else { return true };
        let t0 = std::time::Instant::now();
        while cell.get().is_none() {
            if t0.elapsed() >= max {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        true
    }

    /// The figures near the camera, posed for the editor's clock (`ground`: the drawn ground at a frame point).
    /// `winter`: the view's season is winter.
    pub fn update(&mut self, e: &mut Editor, doc: DocumentId, frame: Option<Arc<Frame>>, ground: &dyn Fn(f64, f64) -> f32, winter: bool) {
        let Some(frame) = frame.filter(|_| self.on && e.scene.campaign) else { return self.clear(e) };
        let Some(d) = e.docs.find_as::<MapDocument>(doc) else { return self.clear(e) };
        let (kind, name) = crate::tool::map_kind(&d.name);
        if kind != wce_terrain::MapKind::Campaign {
            return self.clear(e);
        }
        if self.loaded.as_ref().is_none_or(|(id, _)| *id != (doc, winter)) {
            let cell = load(e, name.to_owned(), frame.clone(), winter);
            self.clear(e);
            self.loaded = Some(((doc, winter), cell));
        }
        let Some(loaded) = self.loaded.as_ref().and_then(|(_, c)| c.get().cloned().flatten()) else { return };
        self.total = loaded.placed.len();

        let (w, h) = (f64::from(frame.map.width), f64::from(frame.map.height));
        let (a, b) = (frame.from_map.apply(0.0, 0.0), frame.from_map.apply(1.0, 0.0));
        // Frame units a world unit (a model unit: capture T2).
        let k = ((b.0 - a.0).hypot(b.1 - a.1) * w / loaded.world.0) as f32;
        let cam = e.viewport.camera();
        let (cx, cy) = frame.cell_of(f64::from(cam.target.x), f64::from(cam.target.z));
        let time = e.clock.time() as f32;
        let mut placed: Vec<(Float4x4, wce_scene::render_scene::SkinnedInstance)> = vec![];
        let mut count = 0;
        for p in &loaded.placed {
            let (x, y) = (p.world.0 / loaded.world.0 * w, (1.0 - p.world.1 / loaded.world.1) * h);
            if (x - cx).hypot(y - cy) > self.radius_cells {
                continue;
            }
            let (fx, fz) = frame.from_map.apply(x, y);
            let y = if p.navy { ground(fx, fz).max(frame.sea_y()) } else { ground(fx, fz) };
            let place = translation(Float3::new(fx as f32, y, fz as f32)) * rotation_y(p.facing);
            for figure in p.figure.instances(time, true) {
                let mut world = place;
                let s = k * figure.scale;
                for row in 0..3 {
                    for column in 0..3 {
                        world.m[row][column] *= s;
                    }
                }
                placed.push((world * figure.local, figure.instance));
            }
            count += 1;
        }
        self.count = count;

        if self.layer == 0 || e.scene.layer(self.layer).is_none() {
            self.layer = e.scene.add_layer(doc, "start figures");
            self.entities.clear();
        }
        if self.entities.len() != placed.len() || self.entities.iter().any(|&id| e.scene.find(id).is_none()) {
            let gone: Vec<std::ops::Range<EntityId>> = self.entities.drain(..).map(|id| id..id + 1).collect();
            e.scene.remove_ranges(&gone);
            self.entities = placed
                .iter()
                .map(|_| {
                    let id = e.scene.add(
                        self.layer,
                        Float4x4::IDENTITY,
                        Payload::CameraRig(Default::default()),
                        SourceRef { doc, item: u64::MAX },
                    );
                    // Read only: nothing of the document's to select.
                    if let Some(en) = e.scene.find_mut(id) {
                        en.locked = true;
                    }
                    id
                })
                .collect();
        }
        for (&entity, (world, instance)) in self.entities.iter().zip(placed) {
            e.scene.update_pose(entity, world, instance);
        }
    }

    pub fn clear(&mut self, e: &mut Editor) {
        if self.layer != 0 {
            e.scene.remove_layer(self.layer);
        }
        (self.layer, self.count) = (0, 0);
        self.entities.clear();
    }
}
