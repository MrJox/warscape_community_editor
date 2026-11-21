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
use wce_scene::render_scene::{DEFAULT_INSTANCE_PARAMS, DocumentId, EntityId, LayerId, MeshInstance, Payload, SourceRef};
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

    /// An army's banner's models (`CampaignCatalog::banner`), none for the others.
    banner: Vec<String>,
}

/// An army's banner, as the game places it (captures A and T0: four armies facing pi, Roman, Frankish, Gothmog's): its pieces at
/// one point, this far west and south of the army's (world units, not turned with it) and this far over its feet, at this scale
/// (the DLL's `ARMY_FLAG_SCALE`), unturned; each piece's instance there plus its pivot, as the renderer adds it.
const BANNER_OFFSET: (f64, f64) = (-0.40, -0.11);
const BANNER_RISE: f32 = 0.80;
const BANNER_SCALE: f32 = 0.65;

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

    /// The player's faction (the Terrain panel's pick, its lines' faction): the armies the campaign's script raises for it at the
    /// start are drawn too (Thrones' starting rebels).
    pub player: Option<String>,

    /// The read of a document, for a season (winter or not: the art sets' `winter` rows) and a player.
    loaded: Option<((DocumentId, bool, Option<String>), Arc<OnceLock<Option<Arc<Loaded>>>>)>,
    layer: LayerId,
    entities: Vec<EntityId>,

    /// The banners' pieces drawn, by figure and piece.
    banners: std::collections::HashMap<(usize, usize), EntityId>,

    /// Figures drawn, and the start's figures in all.
    pub count: usize,
    pub total: usize,
}

impl Default for StartFigures {
    fn default() -> Self {
        Self {
            on: true,
            radius_cells: 160.0,
            player: None,
            loaded: None,
            layer: 0,
            entities: vec![],
            banners: Default::default(),
            count: 0,
            total: 0,
        }
    }
}

/// The read, on a worker: the campaign's startpos, its characters, the map's settlement cells, the tables and each figure.
/// A land army standing on water (the LF under the sea there) is at sea: its `sea_uniform` (a transport ship), as the game's.
fn load(e: &Editor, name: String, frame: Arc<Frame>, winter: bool, player: Option<String>) -> Arc<OnceLock<Option<Arc<Loaded>>>> {
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
            // The rebel armies the campaign's scripts raise at the start for the player (Thrones' `vik_starting_rebellions.lua`), as
            // armies of the culture's rebels (`<...>rebels_<culture>`, ASSUMPTION: capture V2's figure wears `vik_fact_rebels_welsh`'s
            // colours) at the script's hex, facing as the start's armies, with no legacy emblem (the default decorator).
            if let Some(player) = &player {
                let rebels: Vec<_> = files
                    .list(&format!("campaigns/{}/", start.campaign), ".lua")
                    .iter()
                    .filter_map(|p| files.read(p).ok())
                    .flat_map(|b| wce_terrain_io::attila::characters::scripted_rebels(&String::from_utf8_lossy(&b)))
                    .filter(|r| &r.player == player)
                    .collect();
                for (k, r) in rebels.into_iter().enumerate() {
                    let suffix = format!("rebels_{}", r.culture);
                    let Some(faction) = catalog.faction_keys().find(|f| f.ends_with(&suffix)).map(str::to_owned) else { continue };
                    let id = u32::MAX - k as u32;
                    chars.forces.insert(
                        id,
                        wce_terrain_io::attila::characters::StartForce {
                            id,
                            faction: faction.clone(),
                            commander: id,
                            navy: false,
                            residence: None,
                            units: vec![],
                            emblem: String::new(),
                        },
                    );
                    chars.characters.push(wce_terrain_io::attila::characters::StartCharacter {
                        id,
                        subculture: catalog.subculture(&faction).to_owned(),
                        faction,
                        kind: "general".to_owned(),
                        art_set: String::new(),
                        culture: String::new(),
                        world: cities.cell_centre(r.cell),
                        cell: r.cell,
                        facing: std::f32::consts::PI,
                        force: Some(id),
                        leader: false,
                    });
                }
            }
            let mut notes: Vec<String> = vec![];
            let mut at_sea = vec![];
            let (w, h) = (f64::from(frame.map.width), f64::from(frame.map.height));
            let on_water = |world: (f64, f64)| {
                let (fx, fz) = frame.from_map.apply(world.0 / cities.world.0 * w, (1.0 - world.1 / cities.world.1) * h);
                frame.lift(fx, fz) < frame.sea_y()
            };
            let exists = |p: &str| files.exists(p);
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
                    // A land army carries its banner; agents and fleets none.
                    let banner = match force {
                        Some(f) if afloat == Afloat::Land => catalog.banner(&c.faction, &f.emblem, &exists),
                        _ => vec![],
                    };
                    match catalog.figure(&read, &wanted) {
                        Ok(figure) => Some(Placed { world: c.world, facing: c.facing, figure, navy: afloat != Afloat::Land, banner }),
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
        let key = (doc, winter, self.player.clone());
        if self.loaded.as_ref().is_none_or(|(id, _)| *id != key) {
            let cell = load(e, name.to_owned(), frame.clone(), winter, self.player.clone());
            self.clear(e);
            self.loaded = Some((key, cell));
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
        let mut banners: Vec<((usize, usize), Float4x4, &str, [[f32; 3]; 4])> = vec![];
        let mut count = 0;
        let to_frame = |wx: f64, wy: f64| frame.from_map.apply(wx / loaded.world.0 * w, (1.0 - wy / loaded.world.1) * h);
        for (i, p) in loaded.placed.iter().enumerate() {
            let (x, y) = (p.world.0 / loaded.world.0 * w, (1.0 - p.world.1 / loaded.world.1) * h);
            if (x - cx).hypot(y - cy) > self.radius_cells {
                continue;
            }
            let (fx, fz) = frame.from_map.apply(x, y);
            let y = if p.navy { ground(fx, fz).max(frame.sea_y()) } else { ground(fx, fz) };
            if !p.banner.is_empty() {
                let (bx, bz) = to_frame(p.world.0 + BANNER_OFFSET.0, p.world.1 + BANNER_OFFSET.1);
                let s = k * BANNER_SCALE;
                let mut world = translation(Float3::new(bx as f32, y + BANNER_RISE * k, bz as f32));
                for row in 0..3 {
                    world.m[row][row] = s;
                }
                for (j, model) in p.banner.iter().enumerate() {
                    // The army's faction's colours (V2: Gwined's standard takes its uniform colours and its banner's primary).
                    banners.push(((i, j), world, model.as_str(), p.figure.colours));
                }
            }
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
            self.banners.clear();
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

        // The banners: added as they come near, moved with the ground under them (it rises as tiles load), gone as they leave.
        let keep: std::collections::HashSet<(usize, usize)> = banners.iter().map(|b| b.0).collect();
        let gone: Vec<std::ops::Range<EntityId>> =
            self.banners.iter().filter(|(k, _)| !keep.contains(k)).map(|(_, &id)| id..id + 1).collect();
        self.banners.retain(|k, _| keep.contains(k));
        e.scene.remove_ranges(&gone);
        for (key, world, model, colours) in banners {
            match self.banners.get(&key) {
                Some(&id) => {
                    if e.scene.find(id).is_some_and(|en| en.world != world) {
                        e.scene.update_world_quiet(id, world);
                    }
                }
                None => {
                    let p16 = DEFAULT_INSTANCE_PARAMS;
                    let mesh = MeshInstance {
                        model: model.to_owned(),
                        casts_shadow: true,
                        lod: true,
                        p4: [p16[0], p16[1], p16[2], p16[3]],
                        p5: [p16[4], p16[5], p16[6], p16[7]],
                        p6: [p16[8], p16[9], p16[10], p16[11]],
                        p7: [p16[12], p16[13], p16[14], p16[15]],
                        burning: false,
                        faction_colours: Some(wce_scene::render_scene::FactionColours { colours, flags_only: false }),
                    };
                    let id = e.scene.add(self.layer, world, Payload::MeshInstance(mesh), SourceRef { doc, item: u64::MAX });
                    if let Some(en) = e.scene.find_mut(id) {
                        en.locked = true;
                    }
                    self.banners.insert(key, id);
                }
            }
        }
    }

    pub fn clear(&mut self, e: &mut Editor) {
        if self.layer != 0 {
            e.scene.remove_layer(self.layer);
        }
        (self.layer, self.count) = (0, 0);
        self.entities.clear();
        self.banners.clear();
    }
}
