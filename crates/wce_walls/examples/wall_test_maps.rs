//! Phase 0 of docs/plans/procedural_walls.md, in game: two vanilla siege tiles with the spike pieces of `wall_spike` in them, compiled and
//! packed with those pieces, to test in a custom siege battle.
//!
//!   cargo run --release -p wce_walls --example wall_test_maps -- <game folder> <wce_wall_spike.pack> <out .pack> <workspace folder>
//!
//! Per tile (`settlement_nordic_cities/nordic_city_b/medium` for the 8 m wall, `settlement_western_cities/western_city_b/medium` for the
//! 15 m one): the exported tile is decompiled from the game's packs into the workspace; two of the base's instances in the wall ring become
//! the renamed copy (same shape, so the ring stays closed); outside the ring, on the flattest free ground found, a short run is built: a
//! vanilla straight, the bent piece, and a vanilla straight turned by the bend at its end, the outer face away from the settlement. The
//! project is compiled (`save_exported_with`, the spike keys taking their bases' destruction data) and its `building_list` goes into the
//! pack, with the spike pack's files and rows. Only the building list: it is the one exported file that names the buildings of the normal
//! layers (the outlines and zones come from polygons), it comes back record for record from an unedited decompile and compile (12 781 of
//! 12 781 on nordic_city_b/medium), while the markers and the terrain would be our compile's instead of CA's for no reason (the terrain is
//! remade because its fingerprint covers every building, though walls don't stamp it). The tile database is left out: the entries don't
//! change.

use std::path::{Path, PathBuf};

use rpfm_lib::files::pack::Pack;
use rpfm_lib::games::supported_games::SupportedGames;
use wce_foundation::math::{Double3, Double3x3};
use wce_tile::tile_project::TileProject;
use wce_tile::world_transform::{euler_matrix, set_world_position, set_world_rotation, world_transform};
use wce_tile_io::attila::workflow::{Game, open_exported, save_exported_with};
use wce_walls::pack;

/// One wall family's test.
struct Family {
    tile: &'static str,
    base: &'static str,
    copy: &'static str,
    bent: &'static str,
    /// The vanilla straight the run starts and ends with.
    straight: &'static str,
    /// Where a piece's frame starts along x (the 8 m pieces run 0..20 from their origin, the 15 m ones -10..10).
    start: f64,
    /// The bent piece's end in its frame, and the bend.
    end: (f64, f64),
    angle_deg: f64,
    /// The footprint across the wall (z, the outer side first).
    depth: (f64, f64),
}

const FAMILIES: [Family; 2] = [
    Family {
        tile: "settlement_nordic_cities/nordic_city_b/medium",
        base: "barbarian_fort_straight",
        copy: "wce_spike_wall8_copy",
        bent: "wce_spike_wall8_bent",
        straight: "barbarian_fort_straight",
        start: 0.0,
        end: (19.899, 1.741),
        angle_deg: 10.0,
        depth: (-3.0, 12.0),
    },
    Family {
        tile: "settlement_western_cities/western_city_b/medium",
        base: "roman_fort_straight_with_doors_a",
        copy: "wce_spike_wall15_copy",
        bent: "wce_spike_wall15_bent",
        straight: "roman_fort_straight",
        start: -10.0,
        end: (9.899, 1.741),
        angle_deg: 10.0,
        depth: (-6.0, 16.0),
    },
];

/// How far outside the ring's box the run may go (tried in this order), and how far from any other instance it must stay (scene units).
const OUTSIDE: [f64; 4] = [120.0, 200.0, 300.0, 400.0];
const CLEAR: f64 = 20.0;

fn main() {
    if let Err(e) = run() {
        eprintln!("wall_test_maps: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let [game_dir, spike, out, workspace] = args.as_slice() else {
        return Err("usage: wall_test_maps <game folder> <wce_wall_spike.pack> <out .pack> <workspace folder>".to_owned());
    };

    let game = Game::load(Path::new(game_dir))?;
    for w in &game.warnings {
        println!("game: {w}");
    }

    let workspace = PathBuf::from(workspace);
    let mut files = vec![];
    for f in &FAMILIES {
        files.extend(edit_tile(&game, f, &workspace)?);
    }

    let n_tile = files.len();
    files.extend(pack_files(Path::new(spike))?);
    let out = PathBuf::from(out);
    let count = files.len();
    pack::write_pack(&out, "attila", files, vec![])?;
    println!("wrote {} ({n_tile} tile files, {} spike files)", out.display(), count - n_tile);
    Ok(())
}

/// The family's tile decompiled, edited, compiled; its exported files by their paths in a pack.
fn edit_tile(game: &Game, f: &Family, workspace: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let folder = format!("terrain/tiles/battle/{}", f.tile);
    println!("== {folder}");
    let exported = game.exported_from_packs(&folder)?;
    let (project_dir, losses) = open_exported(game, &exported, &folder, workspace, false)?;
    println!("decompiled into {} ({} losses noted)", project_dir.display(), losses.notes.len());

    let mut project = TileProject::open(&project_dir).map_err(|e| e.to_string())?;
    let unit = project.unit_scale;
    let extent = (project.extent_x() * unit, project.extent_z() * unit);
    let scene = &mut project.scene;
    let key_of = |s: &wce_tile::ted_scene::TedScene, e: usize| s.doc().get(e, "key").unwrap_or_default();
    let buildings = scene.find_all("BUILDING_INSTANCE");

    // Two of the base's instances in the ring become the copy.
    let bases = buildings.iter().copied().filter(|&e| key_of(scene, e) == f.base && scene.visible(e)).collect::<Vec<_>>();
    if bases.is_empty() {
        return Err(format!("{}: no {} in the tile", f.tile, f.base));
    }

    for &e in [bases[bases.len() / 3], bases[2 * bases.len() / 3]].iter().collect::<std::collections::BTreeSet<_>>() {
        scene.set_attr(e, "key", f.copy);
        scene.set_attr(e, "name", f.copy);
        let p = world_transform(scene, e).position;
        println!("{} at ({:.1}, {:.1}, {:.1}) is now {}", f.base, p.x, p.y, p.z, f.copy);
    }

    // The heights of the walls against the terrain: how scene y relates to the height map (metres).
    let walls = buildings.iter().copied().filter(|&e| key_of(scene, e).contains("_fort_")).collect::<Vec<_>>();
    let ground = |p: Double3| project_height(&project_dir, unit, p);
    let mut offsets = walls.iter().map(|&e| { let p = world_transform(scene, e).position; p.y - ground(p) }).collect::<Vec<_>>();
    offsets.sort_by(f64::total_cmp);
    let offset = offsets.get(offsets.len() / 2).copied().unwrap_or(0.0);
    println!("the ring's walls stand {offset:.2} above the height map (median over {}; spread {:.2} .. {:.2})", offsets.len(),
        offsets.first().unwrap_or(&0.0), offsets.last().unwrap_or(&0.0));

    // The run: on the side of the ring's box with the flattest free ground, its outer face away from the settlement.
    let ring = walls.iter().map(|&e| world_transform(scene, e).position).collect::<Vec<_>>();
    let (lo, hi) = ring.iter().fold((Double3::new(f64::MAX, 0.0, f64::MAX), Double3::new(f64::MIN, 0.0, f64::MIN)), |(lo, hi), p| {
        (Double3::new(lo.x.min(p.x), 0.0, lo.z.min(p.z)), Double3::new(hi.x.max(p.x), 0.0, hi.z.max(p.z)))
    });
    let centre = Double3::new((lo.x + hi.x) / 2.0, 0.0, (lo.z + hi.z) / 2.0);
    let others = buildings.iter().map(|&e| world_transform(scene, e).position).collect::<Vec<_>>();

    let plan = |origin: Double3, yaw: f64| run_layout(f, origin, yaw);
    let mut best: Option<(f64, Double3, f64, f64)> = None;
    for (out, side, k) in OUTSIDE.iter().flat_map(|&o| (0..4).flat_map(move |side| (-6..=6).map(move |k| (o, side, k)))) {
        if best.is_some_and(|b| b.3 < out) {
            break;
        }

        {
            let t = f64::from(k) / 6.0;
            let (origin, away) = match side {
                0 => (Double3::new(centre.x + t * (hi.x - lo.x) / 2.0 - 30.0, 0.0, lo.z - out), Double3::new(0.0, 0.0, -1.0)),
                1 => (Double3::new(centre.x + t * (hi.x - lo.x) / 2.0 + 30.0, 0.0, hi.z + out), Double3::new(0.0, 0.0, 1.0)),
                2 => (Double3::new(lo.x - out, 0.0, centre.z + t * (hi.z - lo.z) / 2.0 + 30.0), Double3::new(-1.0, 0.0, 0.0)),
                _ => (Double3::new(hi.x + out, 0.0, centre.z + t * (hi.z - lo.z) / 2.0 - 30.0), Double3::new(1.0, 0.0, 0.0)),
            };

            // The yaw that turns the pieces' outer side (-z) away from the ring.
            let yaw = [0.0, 90.0, 180.0, 270.0].into_iter().map(f64::to_radians).max_by(|&a, &b| {
                let face = |y: f64| { let d = euler_matrix(Double3::new(y, 0.0, 0.0)) * Double3::new(0.0, 0.0, -1.0); d.x * away.x + d.z * away.z };
                face(a).total_cmp(&face(b))
            }).expect("four yaws");

            let pieces = plan(origin, yaw);
            let samples = pieces.iter().flat_map(|(_, p, r, _)| {
                let (p, r) = (*p, *r);
                let (z0, z1) = f.depth;
                (0..=4).flat_map(move |i| [z0, (z0 + z1) / 2.0, z1].map(move |z| p + r * Double3::new(f.start + 5.0 * f64::from(i), 0.0, z)))
            }).collect::<Vec<_>>();
            let inside = samples.iter().all(|s| s.x > 200.0 && s.z > 200.0 && s.x < extent.0 - 200.0 && s.z < extent.1 - 200.0);
            let free = samples.iter().all(|s| others.iter().all(|o| (o.x - s.x).hypot(o.z - s.z) > CLEAR));
            if !inside || !free {
                continue;
            }

            let heights = samples.iter().map(|&s| ground(s)).collect::<Vec<_>>();
            let spread = heights.iter().copied().fold(f64::MIN, f64::max) - heights.iter().copied().fold(f64::MAX, f64::min);
            if best.is_none_or(|b| spread < b.0) {
                best = Some((spread, origin, yaw, out));
            }
        }
    }

    let Some((spread, origin, yaw, _)) = best else { return Err(format!("{}: no free ground for the run", f.tile)) };

    // All pieces at one height: the lowest ground under them plus the ring's offset (higher ground buries the foot).
    let samples_y = run_layout(f, origin, yaw).iter().flat_map(|(_, p, r, _)| {
        let (p, r) = (*p, *r);
        let (z0, z1) = f.depth;
        (0..=4).flat_map(move |i| [z0, z1].map(move |z| p + r * Double3::new(f.start + 5.0 * f64::from(i), 0.0, z)))
    }).map(ground).fold(f64::MAX, f64::min);
    let y = samples_y + offset;
    let template = bases[0];
    for (key, p, r, what) in run_layout(f, origin, yaw) {
        let e = scene.duplicate(template).map_err(|e| e.to_string())?;
        scene.set_attr(e, "key", key);
        scene.set_attr(e, "name", key);
        set_world_position(scene, e, Double3::new(p.x, y, p.z));
        set_world_rotation(scene, e, &r);
        let w = world_transform(scene, e);
        println!("{what}: {key} at ({:.2}, {:.2}, {:.2})", w.position.x, w.position.y, w.position.z);
    }

    println!("the run: ground spread {spread:.2} under it; in editor metres around ({:.0}, {:.0})", origin.x / unit, origin.z / unit);
    std::fs::write(project_dir.join("tile.ted"), scene.save()).map_err(|e| e.to_string())?;

    let compiled = workspace.join("exported");
    let base_of = |k: &str| [(f.copy, f.base), (f.bent, f.base)].iter().find(|(c, _)| c.eq_ignore_ascii_case(k)).map(|(_, b)| (*b).to_owned());
    let (dir, notes) = save_exported_with(game, &project_dir, None, &compiled, &base_of)?;
    for n in &notes.notes {
        println!("compile: {n}");
    }

    let list = std::fs::read(dir.join("building_list")).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(vec![(format!("{folder}/building_list"), list)])
}

/// The run's pieces: key, origin, rotation and what it is. The straights' and the bent piece's frames start at `f.start` along their x.
fn run_layout(f: &Family, origin: Double3, yaw: f64) -> Vec<(&'static str, Double3, Double3x3, &'static str)> {
    let base = euler_matrix(Double3::new(yaw, 0.0, 0.0));
    // The turn that takes +x towards +z by the bend (the bent piece curves towards its +z).
    let a = f.angle_deg.to_radians();
    let turn = [-a, a].into_iter().find(|&t| (euler_matrix(Double3::new(t, 0.0, 0.0)) * Double3::new(1.0, 0.0, 0.0)).z > 0.0).expect("a sign");
    let len = 20.0;
    let first = origin;
    let bent = origin + base * Double3::new(len, 0.0, 0.0);
    // The bent piece's end (its frame) is where the next piece's frame starts.
    let end = bent + base * Double3::new(f.end.0, 0.0, f.end.1);
    let last_rot = base * euler_matrix(Double3::new(turn, 0.0, 0.0));
    let last = end + last_rot * Double3::new(-f.start, 0.0, 0.0);
    vec![(f.straight, first, base, "vanilla straight"), (f.bent, bent, base, "bent piece"), (f.straight, last, last_rot, "vanilla straight, turned")]
}

/// The terrain under a scene point, in the scene's height units (the project's height map read again: the scene is borrowed).
fn project_height(dir: &Path, unit: f64, p: Double3) -> f64 {
    thread_local! {
        static CACHE: std::cell::RefCell<Option<(PathBuf, TileProject)>> = const { std::cell::RefCell::new(None) };
    }

    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.as_ref().is_none_or(|(d, _)| d != dir) {
            *c = TileProject::open(dir).ok().map(|t| (dir.to_path_buf(), t));
        }

        c.as_ref().map_or(0.0, |(_, t)| t.height_at(p.x / unit, p.z / unit))
    })
}

/// The files of a pack (its tables too), as they are.
fn pack_files(path: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let games = SupportedGames::default();
    let attila = games.game("attila").ok_or("rpfm knows no attila")?;
    let mut pack = Pack::read_and_merge(&[path.to_path_buf()], attila, true, false, false).map_err(|e| format!("{}: {e}", path.display()))?;
    let names = pack.files().keys().cloned().collect::<Vec<_>>();
    let mut out = vec![];
    for n in names {
        let Some(Ok(Some(bytes))) = pack.files_mut().get_mut(&n).map(|f| f.encode(&None, false, false, true)) else { return Err(format!("{n}: no bytes")) };
        out.push((n, bytes));
    }

    Ok(out)
}
