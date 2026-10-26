//! Phase 0 of docs/plans/procedural_walls.md, in game: two siege tiles with the spike pieces of `wall_spike` in them, compiled and packed
//! with those pieces, to test in a custom siege battle.
//!
//!   cargo run --release -p wce_walls --example wall_test_maps -- <game folder> <wce_wall_spike.pack> <out .pack> <workspace folder>
//!       [--vanilla]
//!
//! The tiles: with The Dawnless Days (whose presets table replaces the game's), Edoras and Dale at settlement level 2
//! (`tdd_settlement_middle_men_cities/middle_men_city_{edoras,dale}/medium`: every culture's level 2 rule keeps them); with `--vanilla`,
//! `settlement_nordic_cities/nordic_city_b/medium` and `settlement_western_cities/western_city_b/medium` (Uburzis / Vesontio, level 2,
//! a barbarian / Roman defender). The first gets the 8 m pieces, the second the 15 m ones.
//!
//! Per tile: the exported tile is decompiled from the game's packs into the workspace; two of the base's instances in the wall ring become
//! the renamed copy (same shape, so the ring stays closed); outside the ring, on the flattest free ground found, a short run is built: a
//! vanilla straight, the bent piece, and a vanilla straight turned by the bend at its end, the outer face away from the settlement. The
//! project is compiled (`save_exported_with`, the spike keys taking their bases' destruction data) and its `building_list` goes into the
//! pack, with the spike pack's files and rows. Only the building list: it is the one exported file that names the buildings of the normal
//! layers (the outlines and zones come from polygons), it comes back record for record from an unedited decompile and compile (12 781 of
//! 12 781 on nordic_city_b/medium), while the markers and the terrain would be our compile's instead of CA's for no reason (the terrain is
//! remade because its fingerprint covers every building, though walls don't stamp it). The tile database is left out: the entries don't
//! change.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use rpfm_lib::files::Decodeable;
use rpfm_lib::files::esf::ESF;
use rpfm_lib::files::pack::Pack;
use rpfm_lib::games::supported_games::SupportedGames;
use wce_foundation::math::{Double3, Double3x3};
use wce_tile::tile_project::TileProject;
use wce_tile::world_transform::{euler_matrix, set_world_position, set_world_rotation, world_transform};
use wce_tile_io::attila::building_list::read;
use wce_tile_io::attila::workflow::{Game, open_exported, save_exported_with};
use wce_walls::pack;

/// The tiles of the test: the 8 m one, then the 15 m one.
const TDD_TILES: [&str; 2] = ["tdd_settlement_middle_men_cities/middle_men_city_edoras/medium", "tdd_settlement_middle_men_cities/middle_men_city_dale/medium"];
const VANILLA_TILES: [&str; 2] = ["settlement_nordic_cities/nordic_city_b/medium", "settlement_western_cities/western_city_b/medium"];

/// One wall family's test.
struct Family {
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

/// How far outside the ring's box the run must be at least, and how far from any building or tree it must stay (scene units).
const OUTSIDE: f64 = 100.0;

/// How far from the ring's box the run may be (scene units: 250 m, to stay on the battlefield), and the slope under it that counts as
/// flat (metres; Dale has no free spot under 1 m near its walls).
const REACH: f64 = 500.0;
const FLAT: f64 = 2.5;
const CLEAR: f64 = 20.0;

fn main() {
    if let Err(e) = run() {
        eprintln!("wall_test_maps: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    let vanilla = args.iter().any(|a| a == "--vanilla");
    args.retain(|a| a != "--vanilla");
    let [game_dir, spike, out, workspace] = args.as_slice() else {
        return Err("usage: wall_test_maps <game folder> <wce_wall_spike.pack> <out .pack> <workspace folder> [--vanilla]".to_owned());
    };

    let game = Game::load(Path::new(game_dir))?;
    for w in &game.warnings {
        println!("game: {w}");
    }

    let workspace = PathBuf::from(workspace);
    let mut files = vec![];
    let tiles = if vanilla { VANILLA_TILES } else { TDD_TILES };
    for (f, tile) in FAMILIES.iter().zip(tiles) {
        files.extend(edit_tile(&game, f, tile, &workspace)?);
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
fn edit_tile(game: &Game, f: &Family, tile: &str, workspace: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let folder = format!("terrain/tiles/battle/{tile}");
    println!("== {folder}");
    let exported = game.exported_from_packs(&folder)?;
    let (project_dir, losses) = open_exported(game, &exported, &folder, workspace)?;
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
        return Err(format!("{tile}: no {} in the tile", f.base));
    }

    for &e in [bases[bases.len() / 3], bases[2 * bases.len() / 3]].iter().collect::<std::collections::BTreeSet<_>>() {
        scene.set_attr(e, "key", f.copy);
        scene.set_attr(e, "name", f.copy);
        let p = world_transform(scene, e).position;
        println!("{} at ({:.1}, {:.1}, {:.1}) is now {}", f.base, p.x, p.y, p.z, f.copy);
    }

    // The heights of the ring's walls against the terrain (for the log: scene y and the height map are both metres).
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
    let mut others = buildings.iter().map(|&e| world_transform(scene, e).position).collect::<Vec<_>>();
    let trees = tree_positions(&exported, extent.1 / unit)?;
    println!("{} trees in the tile's tree lists", trees.len());
    others.extend(trees.iter().map(|&(x, z)| Double3::new(x * unit, 0.0, z * unit)));

    let plan = |origin: Double3, yaw: f64| run_layout(f, origin, yaw);
    // Obstacles by cell of CLEAR metres: a sample is free when no obstacle of its cell or the eight around is within CLEAR.
    let cell = |x: f64, z: f64| ((x / CLEAR).floor() as i64, (z / CLEAR).floor() as i64);
    let mut grid: std::collections::HashMap<(i64, i64), Vec<Double3>> = std::collections::HashMap::new();
    for o in &others {
        grid.entry(cell(o.x, o.z)).or_default().push(*o);
    }

    let free_at = |s: &Double3| {
        let (cx, cz) = cell(s.x, s.z);
        (-1..=1).all(|dx| (-1..=1).all(|dz| grid.get(&(cx + dx, cz + dz)).is_none_or(|v| v.iter().all(|o| (o.x - s.x).hypot(o.z - s.z) > CLEAR))))
    };

    // Every 40 units between the ring's box grown by OUTSIDE and by REACH; the flat enough spots (FLAT) win by their distance to the ring,
    // else the flattest.
    let mut best: Option<(f64, f64, Double3, f64)> = None;
    let step = 40.0;
    let (mut gx, x_end, z_end) = (200.0, extent.0 - 200.0, extent.1 - 200.0);
    while gx < x_end {
        let mut gz = 200.0;
        while gz < z_end {
            let origin = Double3::new(gx, 0.0, gz);
            gz += step;
            let near = origin.x > lo.x - REACH && origin.x < hi.x + REACH && origin.z > lo.z - REACH && origin.z < hi.z + REACH;
            if !near || (origin.x > lo.x - OUTSIDE && origin.x < hi.x + OUTSIDE && origin.z > lo.z - OUTSIDE && origin.z < hi.z + OUTSIDE) {
                continue;
            }

            // The yaw that turns the pieces' outer side (-z) away from the ring.
            let away = Double3::new(origin.x - centre.x, 0.0, origin.z - centre.z);
            let yaw = [0.0, 90.0, 180.0, 270.0].into_iter().map(f64::to_radians).max_by(|&a, &b| {
                let face = |y: f64| { let d = euler_matrix(Double3::new(y, 0.0, 0.0)) * Double3::new(0.0, 0.0, -1.0); d.x * away.x + d.z * away.z };
                face(a).total_cmp(&face(b))
            }).expect("four yaws");

            let samples = plan(origin, yaw).iter().flat_map(|(_, p, r, _)| {
                let (p, r) = (*p, *r);
                let (z0, z1) = f.depth;
                (0..=4).flat_map(move |i| [z0, (z0 + z1) / 2.0, z1].map(move |z| p + r * Double3::new(f.start + 5.0 * f64::from(i), 0.0, z)))
            }).collect::<Vec<_>>();
            let inside = samples.iter().all(|s| s.x > 200.0 && s.z > 200.0 && s.x < x_end && s.z < z_end);
            if !inside || !samples.iter().all(free_at) {
                continue;
            }

            let heights = samples.iter().map(|&s| ground(s)).collect::<Vec<_>>();
            let spread = heights.iter().copied().fold(f64::MIN, f64::max) - heights.iter().copied().fold(f64::MAX, f64::min);
            let distance = away.x.hypot(away.z);
            let score = if spread < FLAT { distance } else { 1e6 + spread };
            if best.is_none_or(|b| score < b.0) {
                best = Some((score, spread, origin, yaw));
            }
        }

        gx += step;
    }

    let Some((_, spread, origin, yaw)) = best else { return Err(format!("{tile}: no free ground for the run")) };

    // All pieces at one height: 0.3 m into the lowest ground under them (higher ground buries the foot). Not the ring's offset: a ring on
    // built-up ground (Dale's walls stand -47 .. +10 m off the height map) says nothing about open ground.
    let samples_y = run_layout(f, origin, yaw).iter().flat_map(|(_, p, r, _)| {
        let (p, r) = (*p, *r);
        let (z0, z1) = f.depth;
        (0..=4).flat_map(move |i| [z0, z1].map(move |z| p + r * Double3::new(f.start + 5.0 * f64::from(i), 0.0, z)))
    }).map(ground).fold(f64::MAX, f64::min);
    let y = samples_y - 0.3;
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

    // The edits must be in the main list (an instance of an escalation or separate layer goes to another file, which is not packed).
    let esf = ESF::decode(&mut Cursor::new(&list), &None).map_err(|e| e.to_string())?;
    let records = read(&esf)?;
    let count = |k: &str| records.iter().filter(|r| r.key == k).count();
    println!("building_list: {} records, {} x {}, {} x {}", records.len(), count(f.copy), f.copy, count(f.bent), f.bent);
    if count(f.copy) == 0 || count(f.bent) != 1 {
        return Err(format!("{tile}: the edits are not all in building_list (escalation or separate layers?)"));
    }

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

/// The trees of an exported tile's tree lists (`*.tree_list.bin`, the generated and static ones), in editor metres: the lists hold the
/// game's tile frame (x / 2, extent - z / 2 of the scene, docs/formats/tile_compile.md), `extent_z_m` the tile's depth in metres.
fn tree_positions(files: &wce_tile_io::TileFiles, extent_z_m: f64) -> Result<Vec<(f64, f64)>, String> {
    use rpfm_lib::files::fastbin_lists::TreeListFile;
    let mut out = vec![];
    for (name, blob) in &files.folder {
        if !name.to_lowercase().ends_with(".tree_list.bin") {
            continue;
        }

        let bytes = blob.bytes().map_err(|e| e.to_string())?;
        let list = TreeListFile::decode(&mut Cursor::new(bytes.as_ref()), &None).map_err(|e| format!("{name}: {e}"))?;
        let v = serde_json::to_value(&list).map_err(|e| e.to_string())?;
        for group in v["tree_list"]["tree_list"].as_array().into_iter().flatten() {
            for t in group["value"].as_array().into_iter().flatten() {
                let (x, z) = (t["x"].as_f64().unwrap_or(0.0), t["z"].as_f64().unwrap_or(0.0));
                out.push((x, extent_z_m - z));
            }
        }
    }

    Ok(out)
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
