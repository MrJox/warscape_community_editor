//! An in-game test of docs/plans/procedural_walls.md on a whole settlement: Edoras at level 2 (The Dawnless Days,
//! `tdd_settlement_middle_men_cities/middle_men_city_edoras/medium`) with its wall ring rebuilt as procedural walls.
//!
//!   cargo run --release -p wce_walls --example wall_ring_test -- <game folder> <rpfm schema_att.ron> <workspace folder> <out folder>
//!       [--mods a.pack;b.pack]
//!
//! The tile is decompiled from the game's packs into the workspace. Every piece of the ring goes except the three gates: the straights,
//! bastions, steps, kinks, curves and endcaps. The watchtowers behind the walls and the two loose straights south of the town stay. The
//! runs between and past the gates become `WALL_SPLINE`s on barbarian_fort_straight, attached to the gates' ends and following the old
//! line. The ring stands on flat terraces (5, 10 and 15 m), so a few changes make the walls climb and turn:
//! - west of the west gate, the run bulges 40 units out over a 6 m hill raised on the plain outside;
//! - east of the west gate, it bulges 35 units out over a 2 m hollow, then turns sharply and climbs onto the 15 m terrace at its end;
//! - west of the north gate, the run bows out a little, its free end coming down the rocky bank it starts on;
//! - the far west run keeps its natural climb from 5 to 15 m (the old step pieces), and the north-east run keeps its kinks, now smooth.
//!
//! The walls hold their level within 1 m of the ground (`wall_tolerance`): the old ring sits buried in the terraces' banks, which wave
//! by that much under it, and 0.5 m makes a step of every wave.
//!
//! Every instance standing on the edited ground moves with it. The longer runs get towers and bastions every 120 units, and doors every
//! 200 (the defaults). The project is saved and exported with its walls (`save_exported_walls`), which gives the pack to install.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use wce_foundation::math::Double3;
use wce_tile::height_field::Brush;
use wce_tile::tile_project::TileProject;
use wce_tile::world_transform::{set_world_position, world_transform};
use wce_tile_io::attila::workflow::{Game, open_exported};
use wce_vfs::{Config, GameProfile, Vfs};
use wce_walls::building::folder;
use wce_walls::scene::{Anchor, WALL_SPLINE, anchors, layout, walls};

const TILE: &str = "tdd_settlement_middle_men_cities/middle_men_city_edoras/medium";
const BASE: &str = "barbarian_fort_straight";

/// The pieces of the ring that stay: the gates, and the watchtowers behind the walls.
const KEPT: [&str; 2] = ["_fort_gateway_", "_fort_watchtower_"];

/// The phase 0 test pack (`wall_test_maps`) put copies of the straight into the ring: with it still installed, they come back with the
/// tile, and go too.
const SPIKE: &str = "wce_spike_";

/// The ring is north of this (scene z): the two loose straights south of the town stay.
const RING_SOUTH: f64 = 1300.0;

/// An end within this of a gate's end goes onto it (scene units).
const SNAP: f64 = 3.0;

/// How far off the ground the walls' level may be (metres).
const TOLERANCE: f64 = 1.0;

/// A run: its name, its points (scene x, z; along the old pieces' x axis, so the outer side is where theirs was), which are sharp,
/// and the spacing of its towers and bastions.
struct Run {
    name: &'static str,
    points: Vec<[f64; 2]>,
    sharp: Vec<usize>,
    defence_every: f64,
}

/// A change of the ground: a bell of `metres` (negative: a hollow) over `radius` scene units.
struct Bump {
    at: [f64; 2],
    radius: f64,
    metres: f64,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("wall_ring_test: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mods = args.iter().position(|a| a == "--mods").and_then(|i| args.get(i + 1)).cloned();
    let positional = args.iter().enumerate().filter(|(i, a)| *a != "--mods" && (*i == 0 || args[i - 1] != "--mods")).map(|(_, a)| a.clone()).collect::<Vec<_>>();
    let [game_dir, schema, workspace, out] = positional.as_slice() else {
        return Err("usage: wall_ring_test <game folder> <rpfm schema_att.ron> <workspace folder> <out folder> [--mods a.pack;b.pack]".to_owned());
    };

    let game = Game::load(Path::new(game_dir))?;
    let mut config = Config::new(PathBuf::from(game_dir), GameProfile::ATTILA);
    config.mod_packs = mods.map(|m| m.split(';').filter(|s| !s.is_empty()).map(str::to_owned).collect());
    let vfs = Vfs::mount(&config).map_err(|e| e.to_string())?;
    let schema = rpfm_lib::schema::Schema::load(Path::new(schema), None).map_err(|e| format!("{schema}: {e}"))?;

    let folder_path = format!("terrain/tiles/battle/{TILE}");
    let exported = game.exported_from_packs(&folder_path)?;
    let (project_dir, losses) = open_exported(&game, &exported, &folder_path, Path::new(workspace))?;
    println!("decompiled into {} ({} losses noted)", project_dir.display(), losses.notes.len());

    let mut project = TileProject::open(&project_dir).map_err(|e| e.to_string())?;
    let unit = project.unit_scale;

    // The ring, but the gates and the watchtowers.
    let key_of = |p: &TileProject, e: usize| p.scene.doc().get(e, "key").unwrap_or_default().to_lowercase();
    let gone = project.scene.find_all("BUILDING_INSTANCE").into_iter().filter(|&e| {
        let k = key_of(&project, e);
        let ring = (k.contains("_fort_") && !KEPT.iter().any(|x| k.contains(x))) || k.starts_with(SPIKE);
        ring && world_transform(&project.scene, e).position.z < RING_SOUTH
    }).collect::<Vec<_>>();
    let mut removed: BTreeMap<String, usize> = BTreeMap::new();
    for &e in &gone {
        *removed.entry(key_of(&project, e)).or_default() += 1;
    }

    // Children first (a higher index is never a parent of a lower one in the document).
    for &e in gone.iter().rev() {
        project.scene.remove(e).map_err(|err| format!("removing #{e}: {err}"))?;
    }

    println!("removed {} ring pieces: {removed:?}", gone.len());

    let gates = project.scene.find_all("BUILDING_INSTANCE").into_iter().filter(|&e| key_of(&project, e).contains("_fort_gateway_")).collect::<Vec<_>>();
    println!("{} gates kept", gates.len());
    if gates.len() != 3 {
        return Err(format!("expected 3 gates, found {}", gates.len()));
    }

    // The runs, along the old line (the ends at the gates are snapped onto their anchors below).
    let (w2, s1, n1) = (bulged([1110.0, 1123.4], [865.7, 1034.5], &[(0.25, 22.0), (0.5, 40.0), (0.75, 22.0)]),
        bulged([1391.9, 1226.0], [1147.6, 1137.0], &[(0.2, 0.0), (0.4, 25.0), (0.55, 35.0), (0.7, 20.0), (0.88, 0.0)]),
        bulged([1174.0, 537.5], [1283.0, 587.9], &[(0.5, 12.0)]));
    let runs = vec![
        Run { name: "far west", points: vec![[844.6, 1029.6], [713.2, 981.9]], sharp: vec![], defence_every: 0.0 },
        Run { name: "west of the west gate", points: w2.clone(), sharp: vec![], defence_every: 120.0 },
        Run { name: "east of the west gate", points: [vec![[1404.0, 1256.0], [1387.0, 1246.0]], s1.clone()].concat(), sharp: vec![2], defence_every: 120.0 },
        Run { name: "west of the north gate", points: n1.clone(), sharp: vec![], defence_every: 120.0 },
        Run {
            name: "east of the north gate",
            points: vec![[1319.2, 604.7], [1482.4, 680.4], [1504.8, 679.7], [1559.1, 705.1], [1581.5, 704.4], [1599.6, 712.9]],
            sharp: vec![],
            defence_every: 120.0,
        },
        Run { name: "west of the east gate", points: vec![[1789.3, 1052.5], [1825.5, 1069.2]], sharp: vec![], defence_every: 0.0 },
        Run { name: "east of the east gate", points: vec![[1861.9, 1086.1], [1916.3, 1111.4]], sharp: vec![], defence_every: 0.0 },
    ];

    // The ground: a hill under the west bulge, a hollow under the east one.
    let bumps = [Bump { at: offset(&w2, 0.5, 40.0), radius: 44.0, metres: 6.0 }, Bump { at: offset(&s1, 0.55, 45.0), radius: 36.0, metres: -2.0 }];
    edit_ground(&mut project, &bumps)?;

    // The gates' ends.
    let extent = |key: &str| {
        let bytes = vfs.read(&format!("{}{key}_tech.cs2.parsed", folder(key))).ok()?;
        wce_walls::logic::extent_x(&bytes).ok().map(|(a, b)| (f64::from(a), f64::from(b))).filter(|(a, b)| b > a)
    };
    let ends = anchors(&project.scene, &extent).into_iter().filter(|a| a.key.to_lowercase().contains("_fort_gateway_")).collect::<Vec<_>>();
    println!("{} gate ends", ends.len());

    // The walls, in the gates' layer.
    let layer = {
        let el = project.scene.doc().elements();
        el[gates[0]].parent.and_then(|c| el[c].parent).or(project.scene.scene_graph()).ok_or("the gates are in no layer")?
    };

    let ground = |p: &TileProject, x: f64, z: f64| p.height_at(x / unit, z / unit);
    let mut attached = 0;
    for r in &runs {
        let mut points = r.points.clone();
        let last = points.len() - 1;
        for i in [0, last] {
            if let Some(a) = nearest(&ends, points[i]) {
                points[i] = [a.at[0], a.at[2]];
                attached += 1;
            }
        }

        let world = points.iter().map(|p| Double3::new(p[0], ground(&project, p[0], p[1]), p[1])).collect::<Vec<_>>();
        let extra = [("wall_base", BASE.to_owned()), ("wall_tolerance", TOLERANCE.to_string()), ("wall_defence_every", r.defence_every.to_string())].into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
        let e = project.scene.add_polygon(Some(layer), WALL_SPLINE, &format!("Wall: {}", r.name), &world, &extra)?;
        let pts = project.scene.polygon_points(e);
        for &i in &r.sharp {
            project.scene.set_attr(pts[i], "sharp", "true");
        }
    }

    println!("{attached} wall ends on gates (6 expected)");
    if attached != 6 {
        return Err("not every gate end has a wall".to_owned());
    }

    // What the walls come out as.
    let all = anchors(&project.scene, &extent);
    let g = |x: f64, z: f64| project.height_at(x / unit, z / unit);
    let mut bad = 0;
    for e in walls(&project.scene) {
        let Some(l) = layout(&project.scene, e, &all, &g) else { return Err(format!("wall #{e} has no layout")) };
        let p = &l.plan;
        let runs = p.runs.iter().map(|r| format!("{:.0}..{:.0} at {:.1}", r.s.0, r.s.1, r.level)).collect::<Vec<_>>();
        let ends = l.ends.iter().map(|a| a.as_ref().map_or("free".to_owned(), |a| a.key.clone())).collect::<Vec<_>>();
        println!("{}: {:.0} long, ends {} / {}, {} corner(s), {} step section(s), runs [{}]", project.scene.name(e), p.length, ends[0], ends[1], p.corners.len(), p.steps.len(), runs.join(", "));
        for w in &p.warnings {
            println!("  warning: {w}");
            bad += usize::from(w.contains("flip"));
        }
    }

    if bad != 0 {
        return Err(format!("{bad} wall(s) face the wrong way"));
    }

    let (_, msg) = project.save(&project_dir, true, project.height_dirty)?;
    println!("{msg}");

    let (pack, dir, notes) = wce_walls::export::save_exported_walls(&game, &vfs, &schema, &project_dir, Path::new(out))?;
    for n in &notes {
        println!("export: {n}");
    }

    println!("exported into {}; the pack: {}", dir.display(), pack.display());
    Ok(())
}

/// A straight run from `a` to `b` with points pushed out (to the run's outer side, the left of its direction) at fractions of it.
fn bulged(a: [f64; 2], b: [f64; 2], out: &[(f64, f64)]) -> Vec<[f64; 2]> {
    let mut pts = vec![a];
    pts.extend(out.iter().map(|&(t, d)| offset(&[a, b], t, d)));
    pts.push(b);
    pts
}

/// The point at a fraction of the straight from the first to the last point, pushed `d` to the outer side.
fn offset(pts: &[[f64; 2]], t: f64, d: f64) -> [f64; 2] {
    let (a, b) = (pts[0], pts[pts.len() - 1]);
    let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
    let len = dx.hypot(dz);
    let (ux, uz) = (dx / len, dz / len);
    [a[0] + dx * t + uz * d, a[1] + dz * t - ux * d]
}

/// The gate end nearest to a point, within [`SNAP`].
fn nearest(ends: &[Anchor], p: [f64; 2]) -> Option<&Anchor> {
    let d = |a: &Anchor| (a.at[0] - p[0]).hypot(a.at[2] - p[1]);
    ends.iter().filter(|a| d(a) <= SNAP).min_by(|a, b| d(a).total_cmp(&d(b)))
}

/// Raises (or lowers) the bells, and moves every building standing on the changed ground by as much.
fn edit_ground(project: &mut TileProject, bumps: &[Bump]) -> Result<(), String> {
    let unit = project.unit_scale;
    let buildings = project.scene.find_all("BUILDING_INSTANCE");
    let before = buildings.iter().map(|&e| {
        let p = world_transform(&project.scene, e).position;
        project.height_at(p.x / unit, p.z / unit)
    }).collect::<Vec<_>>();

    let brushes = bumps.iter().map(|b| {
        let (x, z) = (b.at[0] / unit, b.at[1] / unit);
        (Brush { cx: project.pixel_x(x), cy: project.pixel_z(z), radius: b.radius / unit / project.mpp_x(), strength: 1.0, falloff: 1.0 }, b.metres)
    }).collect::<Vec<_>>();
    let field = project.height.as_mut().ok_or("the tile has no height map")?;
    for (brush, metres) in &brushes {
        field.begin_edit()?;
        field.raise(brush, *metres)?;
        field.end_edit();
    }

    project.height_dirty = true;
    for b in bumps {
        let h = project.height_at(b.at[0] / unit, b.at[1] / unit);
        println!("ground {:+.0} m at ({:.0}, {:.0}): now {h:.1}", b.metres, b.at[0], b.at[1]);
    }

    let mut moved = 0;
    for (&e, h0) in buildings.iter().zip(before) {
        let p = world_transform(&project.scene, e).position;
        let dh = project.height_at(p.x / unit, p.z / unit) - h0;
        if dh.abs() > 0.01 {
            set_world_position(&mut project.scene, e, Double3::new(p.x, p.y + dh, p.z));
            moved += 1;
        }
    }

    println!("{moved} building(s) moved with the ground");
    Ok(())
}
