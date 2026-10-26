//! Exporting a tile with procedural walls (docs/plans/procedural_walls.md, phase 5): the walls become buildings of their own, in one pack
//! with the compiled tile.
//!
//! - **Lowering** ([`lower_walls`]): every `WceWallSpline` of the scene is laid out, cut into pieces and replaced by a `BUILDING_INSTANCE`
//!   per piece, keyed `<prefix>_<wall>_<piece>`; each piece's files are generated in full (every level's meshes, the logic and zone
//!   files, the collapses left out) from its base (the straight, or the door section).
//! - **Rows:** the bases' `battlefield_buildings` and `models_building` rows copied under the new keys (`db::copy_rows`); the
//!   `models_building` row's firing lines rebuilt from the piece's logic file (their action codes learnt from the bases' rows), its
//!   docking lines the base's, warped, cut in lines of at most 5 units and left out over the step sections.
//! - **Export** ([`save_exported_walls`]): the project compiled with the editor's scene, its walls lowered and then made TEd's
//!   (`project_files::to_ted`; the project on disk keeps its walls), then a pack with everything the compile wrote for the tile, the tile
//!   database, the pieces and the rows.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rpfm_lib::schema::Schema;
use wce_tile::ted_scene::TedScene;
use wce_tile::tile_project::TileProject;
use wce_tile_io::attila::workflow::{Game, save_exported_with, tile_folder};
use wce_vfs::files::Files;

use crate::building::{BuildingFiles, folder};
use crate::db::{self, bytes_mut, cell_mut};
use crate::lines::{self, DockingLine};
use crate::pieces::{Piece, PieceWarp, StepFiles, make_piece, pieces, step_ranges};
use crate::pack::PROC_PACK;
use crate::scene;

/// Docking lines of a generated piece are cut in lines of at most this many units (CA's 20 m radius curve has about 5).
pub const DOCKING_STEP: f32 = 5.0;

/// What lowering a scene's walls made.
#[derive(Default)]
pub struct LoweredWalls {
    /// The pieces' files, by their paths in a pack.
    pub files: Vec<(String, Vec<u8>)>,

    /// The pieces' rows, by table.
    pub tables: BTreeMap<String, rpfm_lib::files::db::DB>,

    /// Each new key and the base it was made from (for the compile's destruction levels).
    pub bases: Vec<(String, String)>,

    pub notes: Vec<String>,
}

/// A building's files through the editor's sources, read once per key.
struct Bases<'a> {
    files: &'a dyn Files,
    read: HashMap<String, Option<Arc<BuildingFiles>>>,
}

impl Bases<'_> {
    fn get(&mut self, key: &str) -> Option<Arc<BuildingFiles>> {
        if key.is_empty() {
            return None;
        }

        let files = self.files;
        self.read.entry(key.to_owned()).or_insert_with(|| BuildingFiles::read_files(files, key).ok().map(Arc::new)).clone()
    }
}

/// The x extent of a building's frame: its logic file's box.
fn extent(files: &dyn Files, key: &str) -> Option<(f64, f64)> {
    let b = files.read(&format!("{}{key}_tech.cs2.parsed", folder(key))).ok()?;
    crate::logic::extent_x(&b).ok().map(|(a, b)| (f64::from(a), f64::from(b))).filter(|(a, b)| b > a)
}

/// Lowers the scene's walls (see the module), `ground(x, z)` the terrain in scene units, keys prefixed with `prefix`; the pieces go in each
/// wall's layer, or in the top layer named `into`.
///
/// # Errors
///
/// A base's files or rows don't read, a piece can't be made, or the scene can't be edited.
pub fn lower_walls(scene: &mut TedScene, ground: &dyn Fn(f64, f64) -> f64, files: &dyn Files, schema: &Schema, prefix: &str, into: Option<&str>) -> Result<LoweredWalls, String> {
    let mut out = LoweredWalls::default();
    let walls = scene::walls(scene);
    if walls.is_empty() {
        return Ok(out);
    }

    let mut extents: HashMap<String, Option<(f64, f64)>> = HashMap::new();
    let mut extent_of = |k: &str| *extents.entry(k.to_owned()).or_insert_with(|| extent(files, k));
    let keys = scene.find_all("BUILDING_INSTANCE").into_iter().map(|e| scene.doc().get(e, "key").unwrap_or_default()).collect::<Vec<_>>();
    let known = keys.iter().map(|k| (k.clone(), extent_of(k))).collect::<HashMap<_, _>>();
    let anchors = scene::anchors(scene, &|k| known.get(k).copied().flatten());
    let radius = |k: &str| known.get(k).copied().flatten().map_or(4.0, |(a, b)| (b - a) / 2.0);

    // The layouts, pieces and doors first: the keys decide which rows are copied.
    struct Planned {
        wall: usize,
        layout: scene::WallLayout,
        pieces: Vec<Piece>,
        rigid: Vec<Piece>,
        doors: Vec<usize>,
        keys: Vec<String>,
    }

    let mut planned = vec![];
    for (w, &wall) in walls.iter().enumerate() {
        let Some(layout) = scene::layout(scene, wall, &anchors, ground) else {
            out.notes.push(format!("wall {w} ({}): fewer than 2 points, left out", scene.name(wall)));
            continue;
        };

        // The towers and bastions are the game's pieces, placed as they are; the rest is generated.
        let (rigid, ps): (Vec<Piece>, Vec<Piece>) = pieces(&layout.plan, layout.profile).into_iter().partition(|p| p.rigid);
        let (doors, skipped) = scene::doors(scene, &layout, &ps, ground, &radius);
        let defences_skipped = crate::pieces::defence_slots(&layout.plan, layout.profile).1;
        out.notes.extend(layout.plan.warnings.iter().chain(&skipped).chain(&defences_skipped).map(|n| format!("wall {w} ({}): {n}", scene.name(wall))));
        let keys = (0..ps.len()).map(|i| format!("{prefix}_{w}_{i:02}")).collect();
        planned.push(Planned { wall, layout, pieces: ps, rigid, doors, keys });
    }

    // The rows of every piece's base under its key.
    let base_of = |p: &Planned, i: usize| match p.pieces[i].base {
        Some(b) => b,
        None if p.doors.contains(&i) => p.layout.profile.door,
        None => p.layout.profile.key,
    };
    let renames = planned.iter().flat_map(|p| p.keys.iter().enumerate().map(move |(i, k)| (base_of(p, i).to_owned(), k.clone()))).collect::<Vec<_>>();
    let rows = db::copy_rows(files, schema, &renames)?;
    out.tables = rows.tables;
    for (base, key) in &renames {
        out.bases.push((key.clone(), base.clone()));
        if !out.tables.values().any(|t| db_has(t, key)) {
            out.notes.push(format!("{key}: no database row of its base {base} was found"));
        }
    }

    // The pieces' files, their rows' lines.
    let mut bases = Bases { files, read: HashMap::new() };
    let mut codes = HashMap::new();
    let mut lowered = vec![];
    for p in &planned {
        let profile = p.layout.profile;
        let (up, down) = (bases.get(profile.step_up), bases.get(profile.step_down));
        let steps = StepFiles { up: up.as_deref(), down: down.as_deref() };
        for source in [profile.key, profile.door, profile.step_up, profile.step_down, profile.cap_start, profile.cap_end] {
            learn_codes(&mut bases, rows_of(files, schema, source).as_deref(), source, &mut codes);
        }

        let mut placed = vec![];
        for (i, &piece) in p.pieces.iter().enumerate() {
            let key = &p.keys[i];
            let from_key = base_of(p, i);
            let from = bases.get(from_key).ok_or_else(|| format!("{from_key}: its files are in no pack"))?;
            let warp = PieceWarp::new(&p.layout.plan, profile, piece, ground);
            let made = make_piece(&from, key, &warp, steps, true, None)?;

            // The row's lines: the firing lines of the logic file just made, the base's docking lines warped (none on the steps).
            let logic = made.files.get(&made.logic_name()).cloned().unwrap_or_default();
            let cut_out = step_ranges(&p.layout.plan, profile, &piece);
            if let Some(table) = out.tables.get_mut("models_building_tables") {
                let (ef, unknown) = lines::ef_lines_of(&logic, &codes)?;
                if unknown > 0 {
                    out.notes.push(format!("{key}: {unknown} firing line(s) of an action without a known code left out of its row"));
                }

                if let Some(b) = cell_mut(table, key, "EFLines").and_then(bytes_mut) {
                    *b = lines::write_ef_lines(&ef);
                }

                if let Some(b) = cell_mut(table, key, "DockingLines").and_then(bytes_mut) {
                    let old: Vec<DockingLine> = lines::read_docking_lines(b)?;
                    let keep = |x: f32| !cut_out.iter().any(|&(a, b)| x > a && x < b);
                    let mut docking = lines::warp_docking_lines_where(&old, &warp, DOCKING_STEP, &keep);

                    // The copies filling a corner's outer side dock too (cut at the corner like the piece).
                    for dx in warp.overhangs() {
                        let shifted = crate::warp::Shifted { inner: &warp, dx };
                        docking.extend(lines::warp_docking_lines_where(&old, &shifted, DOCKING_STEP, &|_| true));
                    }

                    // Their names numbered again in order.
                    for (n, l) in docking.iter_mut().enumerate() {
                        let stem = l.name.rsplit_once("_line").map_or(l.name.clone(), |(s, _)| s.to_owned());
                        l.name = format!("{stem}_line{:02}", n + 1);
                    }

                    *b = lines::write_docking_lines(&docking);
                }
            }

            out.files.extend(made.into_pack_files());
            placed.push((key.clone(), piece));
        }

        // The caps past free ends and the towers and bastions: the game's pieces, as they are.
        placed.extend(crate::pieces::cap_instances(&p.layout.plan, profile).into_iter().map(|(k, piece)| (k.to_owned(), piece)));
        placed.extend(p.rigid.iter().filter_map(|piece| piece.base.map(|k| (k.to_owned(), *piece))));
        lowered.push((p.wall, placed));
    }

    // Lower the walls, the last first (an edit only moves the indices of the elements after it).
    lowered.sort_by(|a, b| b.0.cmp(&a.0));
    for (wall, placed) in lowered {
        // A layer made at the end of the scene graph comes after every wall: removing one does not move it.
        scene::lower(scene, wall, &placed, into)?;
    }

    Ok(out)
}

fn db_has(t: &rpfm_lib::files::db::DB, key: &str) -> bool {
    let id = t.column_position_by_name("key").unwrap_or(0);
    t.data().iter().any(|r| r[id].data_to_string().eq_ignore_ascii_case(key))
}

/// A building's own `models_building` row's `EFLines` (to learn the action codes from).
fn rows_of(files: &dyn Files, schema: &Schema, key: &str) -> Option<Vec<u8>> {
    if key.is_empty() {
        return None;
    }

    let rows = db::copy_rows(files, schema, &[(key.to_owned(), key.to_owned())]).ok()?;
    let mut table = rows.tables.get("models_building_tables")?.clone();
    cell_mut(&mut table, key, "EFLines").and_then(bytes_mut).cloned()
}

/// The action codes of a building's firing lines (its row's and its logic file's, paired by name) added to `codes`.
fn learn_codes(bases: &mut Bases, row: Option<&[u8]>, key: &str, codes: &mut HashMap<String, u32>) {
    let (Some(row), Some(files)) = (row, bases.get(key)) else { return };
    if let Some(logic) = files.files.get(&files.logic_name()) {
        let _ = lines::action_codes(row, logic, codes);
    }
}

/// The pack an exported tile goes into: `wce_<the tile's folder, under terrain/tiles/<kind>/, joined by _>.pack`.
pub fn pack_name(folder: &str) -> String {
    let rest = folder.trim_start_matches("terrain/tiles/").split('/').skip(1).collect::<Vec<_>>().join("_");
    format!("wce_{}.pack", rest.to_lowercase())
}

/// The key prefix of a tile's walls: `wce_` and its folder under `terrain/tiles/<kind>/` joined by `_` (for instance
/// `wce_middle_men_city_edoras_medium`), unique to the tile: the tiles saved for the Assembly Kit share [`PROC_PACK`].
pub fn key_prefix(folder: &str) -> String {
    let rest = folder.trim_start_matches("terrain/tiles/").split('/').skip(1).collect::<Vec<_>>().join("_").to_lowercase();
    format!("wce_{}", rest.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect::<String>())
}

/// The layer the walls' pieces go into in TEd's tile (Save for Assembly Kit): Import from Assembly Kit drops it and puts the project's walls
/// back.
pub const KIT_LAYER: &str = "WCE procedural walls";

/// The polyfill of the procedural walls for TEd (Save for Assembly Kit): `scene` (the editor's tile.wce text, prefab references with their
/// contents) with its walls lowered into [`KIT_LAYER`], and the tile's pieces put into [`PROC_PACK`] in `data` (the game's data folder) in
/// place of what it had of the tile's. Returns the scene's text, the walls lowered and the notes.
///
/// # Errors
///
/// The project or the scene doesn't read, a wall can't be made, or the pack can't be written.
pub fn walls_for_kit(project: &TileProject, scene: Vec<u8>, files: &dyn Files, schema: &Schema, folder: &str, data: &Path) -> Result<(Vec<u8>, usize, Vec<String>), String> {
    let unit = project.unit_scale;
    let ground = |x: f64, z: f64| project.height_at(x / unit, z / unit);
    let mut scene = TedScene::parse_any(scene)?;
    let walls = scene::walls(&scene).len();
    let prefix = key_prefix(folder);
    let lowered = lower_walls(&mut scene, &ground, files, schema, &prefix, Some(KIT_LAYER))?;
    let mut notes = lowered.notes;
    let pieces = lowered.bases.len();
    let pack = data.join(PROC_PACK);
    let others = crate::pack::replace_in_proc_pack(&pack, "attila", &prefix, lowered.files, lowered.tables.into_iter().collect())?;
    notes.push(format!("{pieces} wall piece(s) of {prefix} written into {} ({others} file(s) of other tiles kept)", pack.display()));
    Ok((scene.save(), walls, notes))
}

/// Exports a project with walls: `scene` (the editor's tile.wce text, prefab references with their contents) compiled with its walls
/// lowered (see the module) into `out` (`<out>/terrain/tiles/...` and the tile database, as `save_exported`), then the pack
/// `<out>/<pack_name>` with what the compile wrote for the tile, the tile database, the walls' pieces and their rows. Returns the pack,
/// the exported folder and the notes.
///
/// # Errors
///
/// The project or the scene doesn't read, a wall can't be made, the compile fails, or a file can't be written.
pub fn save_exported_walls(game: &Game, files: &dyn Files, schema: &Schema, project_dir: &Path, scene: Vec<u8>, out: &Path) -> Result<(PathBuf, PathBuf, Vec<String>), String> {
    // An extracted tile goes back to its own name, as the compile does.
    let folder = tile_folder(project_dir).ok_or_else(|| format!("{} is not under terrain/tiles/", project_dir.display()))?;
    let project = TileProject::open(project_dir).map_err(|e| e.to_string())?;
    let unit = project.unit_scale;
    let ground = |x: f64, z: f64| project.height_at(x / unit, z / unit);
    let mut scene = TedScene::parse_any(scene)?;
    let lowered = lower_walls(&mut scene, &ground, files, schema, &key_prefix(&folder), None)?;
    let (ted, _) = wce_tile::project_files::to_ted(scene.save())?;
    let bases = lowered.bases.iter().cloned().collect::<HashMap<_, _>>();
    let base_of = |k: &str| bases.get(k).cloned();
    let (dir, compiled) = save_exported_with(game, project_dir, Some(ted), out, &base_of)?;
    let mut notes = lowered.notes;
    notes.extend(compiled.notes);

    // The pack: the tile's exported files, the tile database, the pieces, the rows.
    let mut pack_files = vec![];
    for (rel, bytes) in files_below(&dir)?.into_iter().filter(|(rel, _)| !NOT_SHIPPED.iter().any(|n| rel.eq_ignore_ascii_case(n))) {
        pack_files.push((format!("{folder}/{rel}"), bytes));
    }

    let kind = folder.split('/').nth(2).unwrap_or("battle");
    let database = out.join("terrain/tiles").join(kind).join("tile_database.bin");
    if let Ok(b) = std::fs::read(&database) {
        pack_files.push((format!("terrain/tiles/{kind}/tile_database.bin"), b));
    }

    let walls = lowered.bases.len();
    pack_files.extend(lowered.files);
    let pack = out.join(pack_name(&folder));
    crate::pack::write_pack(&pack, "attila", pack_files, lowered.tables.into_iter().collect())?;
    notes.push(format!("{walls} wall piece(s) generated"));
    Ok((pack, dir, notes))
}

/// What the compile writes that the game's packs never ship: BOB's list of the tile's buildings and files (`tile.agf`), and TEd's XML
/// twin of the building list.
const NOT_SHIPPED: [&str; 2] = ["tile.agf", "building_list.xml"];

/// Every file below a folder, by its path relative to it.
fn files_below(dir: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut out = vec![];
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).map_err(|e| format!("{}: {e}", d.display()))?.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                let rel = p.strip_prefix(dir).expect("below").to_string_lossy().replace('\\', "/");
                out.push((rel, std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?));
            }
        }
    }

    out.sort();
    Ok(out)
}
