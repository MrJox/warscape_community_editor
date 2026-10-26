//! An in-game test of docs/plans/procedural_walls.md on a settlement: Edoras at level 2 (The Dawnless Days,
//! `tdd_settlement_middle_men_cities/middle_men_city_edoras/medium`) with one of its outside walls rebuilt as a procedural wall, its
//! AI layout following it.
//!
//!   cargo run --release -p wce_walls --example wall_edoras_test -- <game folder> <rpfm schema_att.ron> <workspace folder> <out folder>
//!       [--mods a.pack;b.pack]
//!
//! The tile is decompiled fresh from the game's packs into the workspace. The wall: the south run west of the west gate, from the gate
//! to the rock pile where it ends. Its pieces go (12 straights, the bastion by the gate, the endcap), and a `WceWallSpline` on
//! barbarian_fort_straight replaces them, attached to the gate's end. The run stood on flat ground, so the new wall bulges 40 units out
//! over a 6 m hill raised on the plain outside: it turns and climbs. Its level holds within 1 m of the ground (`wall_tolerance`; the
//! old ring sits buried in the terraces' banks, which wave by that much). Towers and bastions every 120 units.
//!
//! The AI layout along the old run follows the new wall: every point of the polygons that run along the wall's line (the siege AI's
//! wall edge, `SAET_WALL`; the wall's battlefield zone, `EFP_LOW_WALL`; the fort perimeter hints, `AIH_FORT_PERIMETER`; the deployment
//! zones), between the run's ends and within [`NEAR`] of its line, goes to the same place relative to the new wall (as far along it,
//! as far off it), and their edges along the run get points every [`EVERY`] so they bend with it. What crosses the line or stands off it
//! (the streets, the areas) is left alone.
//!
//! The project is saved and exported with its wall (`save_exported_walls`), which gives the pack to install.
//!
//! Two variants split the test when something breaks in game (units could not cross from docked ladders and towers onto the walls):
//! - `--variant terrain-only`: the hill alone, every wall as it was; the pack carries our compile of the terrain (every edit remakes it).
//! - `--variant wall-only`: the wall and its AI layout alone, no hill; the tile's files are CA's but for those the wall changes (the
//!   building list, the AI hints and polylines, the zones, the deployment areas).
//!
//! Each variant writes its own pack (`wce_edoras_<variant>.pack` in the out folder).

use std::path::{Path, PathBuf};

use wce_foundation::math::Double3;
use wce_tile::height_field::Brush;
use wce_tile::tile_project::TileProject;
use wce_tile::ted_scene::TedScene;
use wce_tile::world_transform::world_transform;
use wce_tile_io::attila::workflow::{Game, open_exported};
use wce_vfs::{Config, GameProfile, Vfs};
use wce_walls::building::folder;
use wce_walls::pieces::sample_at;
use wce_walls::scene::{Anchor, WALL_SPLINE, anchors, layout, wall_points};
use wce_walls::solver::Plan;

const TILE: &str = "tdd_settlement_middle_men_cities/middle_men_city_edoras/medium";
const BASE: &str = "barbarian_fort_straight";

/// The run: from the west gate's end (snapped onto it) to its free end at the rock pile (scene x, z).
const GATE_END: [f64; 2] = [1110.0, 1123.4];
const FREE_END: [f64; 2] = [865.7, 1034.5];

/// A piece of the old run: a fort piece (not a gate nor a watchtower) whose origin is this close to the run's line.
const ON_RUN: f64 = 3.0;

/// An end within this of a gate's end goes onto it (scene units).
const SNAP: f64 = 3.0;

/// How far off the ground the wall's level may be (metres).
const TOLERANCE: f64 = 1.0;

/// The layout's points this close to the old run's line follow the wall (scene units): the wall's own polygons are within 5 of it, the
/// outer fort perimeter 10 outside, the areas inside 25 and more.
const NEAR: f64 = 15.0;

/// An edge of the layout runs along the wall when it is this parallel to it (the change of its distance to the line per unit along it:
/// 3 degrees). The layout's long edges are not quite parallel to the pieces: the wall zone's drifts 2 units over 700.
const PARALLEL: f64 = 0.05;

/// The spacing of the points the layout's edges get along the new wall (scene units).
const EVERY: f64 = 5.0;

fn main() {
    if let Err(e) = run() {
        eprintln!("wall_edoras_test: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let option = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let mods = option("--mods");
    let variant = option("--variant").unwrap_or_else(|| "full".to_owned());
    let positional = args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !args[i - 1].starts_with("--"))).map(|(_, a)| a.clone()).collect::<Vec<_>>();
    let [game_dir, schema, workspace, out] = positional.as_slice() else {
        return Err("usage: wall_edoras_test <game folder> <rpfm schema_att.ron> <workspace folder> <out folder> [--mods a.pack;b.pack] [--variant full|terrain-only|wall-only]".to_owned());
    };

    let (hill, wall) = match variant.as_str() {
        "full" => (true, true),
        "terrain-only" => (true, false),
        "wall-only" => (false, true),
        other => return Err(format!("no variant {other}")),
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
    // The hill first: the wall's layout (and the AI layout following it) is made on the ground it stands on.
    if hill {
        raise(&mut project, offset(&[GATE_END, FREE_END], 0.5, 40.0), 44.0, 6.0)?;
    }

    if wall {
        rebuild_wall(&mut project, &vfs)?;
    }

    let (_, msg) = project.save(&project_dir, true, project.height_dirty)?;
    println!("{msg}");

    let (pack, dir, notes) = wce_walls::export::save_exported_walls(&game, &vfs, &schema, &project_dir, project.scene.save(), Path::new(out))?;
    for n in &notes {
        println!("export: {n}");
    }

    let named = Path::new(out).join(format!("wce_edoras_{variant}.pack"));
    if hill {
        std::fs::rename(&pack, &named).map_err(|e| format!("{}: {e}", named.display()))?;
    } else {
        // CA's tile but for what the wall changes.
        let ours = pack_files(&pack)?;
        let prefix = format!("{folder_path}/");
        let mut files = ours.into_iter().filter(|(n, _)| !n.starts_with(&prefix) || WALL_FILES.iter().any(|w| n[prefix.len()..].eq_ignore_ascii_case(w))).collect::<Vec<_>>();
        for (name, blob) in &exported.folder {
            // Not TEd's twin of the building list (another mod's Edoras in `data` can add one).
            if !WALL_FILES.iter().any(|w| name.eq_ignore_ascii_case(w)) && !name.eq_ignore_ascii_case("building_list.xml") {
                files.push((format!("{prefix}{name}"), blob.bytes().map_err(|e| format!("{name}: {e}"))?.to_vec()));
            }
        }

        wce_walls::pack::write_pack(&named, "attila", files, vec![])?;
        std::fs::remove_file(&pack).map_err(|e| format!("{}: {e}", pack.display()))?;
    }

    println!("exported into {}; the pack: {}", dir.display(), named.display());
    Ok(())
}

/// The tile's files a wall changes: the building list, the AI layout (the hints and the siege AI polylines), the zones and the deployment
/// areas.
const WALL_FILES: [&str; 5] = ["building_list", "ai_hint_polyline_list_marker.markers", "ai_hint_polyline_marker.markers", "zones.xml", "deployment_areas.xml"];

/// The files of a pack (its tables too), as they are.
fn pack_files(path: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    use rpfm_lib::files::pack::Pack;
    let games = rpfm_lib::games::supported_games::SupportedGames::default();
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

/// The run west of the west gate replaced by a procedural wall, the AI layout along it following it.
fn rebuild_wall(project: &mut TileProject, vfs: &Vfs) -> Result<(), String> {
    let unit = project.unit_scale;

    // The old run's pieces.
    let key_of = |s: &TedScene, e: usize| s.doc().get(e, "key").unwrap_or_default().to_lowercase();
    let gone = project.scene.find_all("BUILDING_INSTANCE").into_iter().filter(|&e| {
        let k = key_of(&project.scene, e);
        let p = world_transform(&project.scene, e).position;
        k.contains("_fort_") && !k.contains("_fort_gateway_") && !k.contains("_fort_watchtower_") && distance_to_run([p.x, p.z]) <= ON_RUN
    }).collect::<Vec<_>>();
    let names = gone.iter().map(|&e| key_of(&project.scene, e)).collect::<Vec<_>>();
    for &e in gone.iter().rev() {
        project.scene.remove(e).map_err(|err| format!("removing #{e}: {err}"))?;
    }

    println!("removed the run's {} pieces: {}", names.len(), names.join(", "));
    if names.len() != 14 {
        return Err(format!("expected the run's 14 pieces, found {}", names.len()));
    }

    let points = bulged(GATE_END, FREE_END, &[(0.25, 22.0), (0.5, 40.0), (0.75, 22.0)]);

    // The wall, in the gate's layer, its first end on the gate.
    let extent = |key: &str| {
        let bytes = vfs.read(&format!("{}{key}_tech.cs2.parsed", folder(key))).ok()?;
        wce_walls::logic::extent_x(&bytes).ok().map(|(a, b)| (f64::from(a), f64::from(b))).filter(|(a, b)| b > a)
    };
    let all = anchors(&project.scene, &extent);
    let gate = nearest(&all, GATE_END).filter(|a| a.key.to_lowercase().contains("_fort_gateway_")).ok_or("no gate end at the run's start")?.clone();
    let mut points = points;
    points[0] = [gate.at[0], gate.at[2]];
    let layer = {
        let el = project.scene.doc().elements();
        el[gate.elem].parent.and_then(|c| el[c].parent).or(project.scene.scene_graph()).ok_or("the gate is in no layer")?
    };

    let world = points.iter().map(|p| Double3::new(p[0], project.height_at(p[0] / unit, p[1] / unit), p[1])).collect::<Vec<_>>();
    let extra = [("wall_base", BASE.to_owned()), ("wall_tolerance", TOLERANCE.to_string()), ("wall_defence_every", "120".to_owned())].into_iter()
        .map(|(k, v)| (k.to_owned(), v)).collect();
    let wall = project.scene.add_polygon(Some(layer), WALL_SPLINE, "Wall: west of the west gate", &world, &extra)?;

    let ground = |x: f64, z: f64| project.height_at(x / unit, z / unit);
    let l = layout(&project.scene, wall, &all, &ground).ok_or("the wall has no layout")?;
    let p = &l.plan;
    let runs = p.runs.iter().map(|r| format!("{:.0}..{:.0} at {:.1}", r.s.0, r.s.1, r.level)).collect::<Vec<_>>();
    let ends = l.ends.iter().map(|a| a.as_ref().map_or("free".to_owned(), |a| a.key.clone())).collect::<Vec<_>>();
    println!("the wall: {:.0} long, ends {} / {}, {} step section(s), runs [{}]", p.length, ends[0], ends[1], p.steps.len(), runs.join(", "));
    for w in &p.warnings {
        println!("  warning: {w}");
    }

    if l.ends[0].is_none() || p.warnings.iter().any(|w| w.contains("flip")) {
        return Err("the wall is not on the gate, or faces the wrong way".to_owned());
    }

    // The AI layout along the run.
    let plan = l.plan.clone();
    // Last first: adding points renumbers the elements after the polygon.
    let mut polygons = layout_polygons(&project.scene);
    polygons.sort_unstable_by(|a, b| b.cmp(a));
    let mut moved = 0;
    for e in polygons {
        let (old, _) = wall_points(&project.scene, e);
        let closed = project.scene.doc().get(e, "polygon_closed").is_some_and(|v| v.trim() == "1");
        let new = follow(&old, closed, &plan);
        if new == old {
            continue;
        }

        let what = describe(&project.scene, e);
        println!("{what}: {} points, now {}", old.len(), new.len());
        set_points(&mut project.scene, e, &new)?;
        moved += 1;
    }

    if moved == 0 {
        return Err("nothing of the AI layout runs along the wall".to_owned());
    }

    Ok(())
}

/// The old run's frame: its length, its direction, and its outer side (the left of its direction, the pieces' -z).
fn run_frame() -> (f64, [f64; 2], [f64; 2]) {
    let (dx, dz) = (FREE_END[0] - GATE_END[0], FREE_END[1] - GATE_END[1]);
    let len = dx.hypot(dz);
    let d = [dx / len, dz / len];
    (len, d, [d[1], -d[0]])
}

/// How far along the old run and how far off it (to its outer side) a point is.
fn run_coords(p: [f64; 2]) -> (f64, f64) {
    let (_, d, n) = run_frame();
    let r = [p[0] - GATE_END[0], p[1] - GATE_END[1]];
    (r[0] * d[0] + r[1] * d[1], r[0] * n[0] + r[1] * n[1])
}

fn distance_to_run(p: [f64; 2]) -> f64 {
    let (len, _, _) = run_frame();
    let (u, v) = run_coords(p);
    if u < 0.0 { (u * u + v * v).sqrt() } else if u > len { ((u - len).powi(2) + v * v).sqrt() } else { v.abs() }
}

/// Where a point `u` along the old run and `v` off it goes: as far along the new wall (in proportion) and as far off it.
fn on_wall(plan: &Plan, u: f64, v: f64) -> [f64; 2] {
    let (len, _, _) = run_frame();
    let at = sample_at(plan, (u / len).clamp(0.0, 1.0) * plan.length);
    let outer = [at.dir[1], -at.dir[0]];
    [at.centre[0] + outer[0] * v, at.centre[1] + outer[1] * v]
}

/// A polygon's points with the run's stretch moved onto the new wall: the points along it (within the run's ends and [`NEAR`] of its
/// line) moved, and the edges along it (both ends near the line, within [`PARALLEL`] of its direction) given points every [`EVERY`]
/// between the run's ends.
fn follow(points: &[[f64; 2]], closed: bool, plan: &Plan) -> Vec<[f64; 2]> {
    let (len, _, _) = run_frame();
    let along = |p: [f64; 2]| {
        let (u, v) = run_coords(p);
        (v.abs() <= NEAR && (0.0..=len).contains(&u)).then_some((u, v))
    };

    let mut out = vec![];
    let n = points.len();
    let edges = if closed { n } else { n.saturating_sub(1) };
    for i in 0..n {
        let p = points[i];
        out.push(along(p).map_or(p, |(u, v)| on_wall(plan, u, v)));
        if i >= edges {
            continue;
        }

        let q = points[(i + 1) % n];
        let ((up, vp), (uq, vq)) = (run_coords(p), run_coords(q));
        if vp.abs() > NEAR || vq.abs() > NEAR || (vp - vq).abs() > PARALLEL * (up - uq).abs() {
            continue;
        }

        // The stretch of the edge between the run's ends, in the edge's direction.
        let (lo, hi) = (up.min(uq).max(0.0), up.max(uq).min(len));
        if hi - lo < 1e-6 {
            continue;
        }

        let mut us = vec![lo, hi];
        let mut u = (lo / EVERY).floor() * EVERY + EVERY;
        while u < hi {
            us.push(u);
            u += EVERY;
        }

        us.sort_by(f64::total_cmp);
        if uq < up {
            us.reverse();
        }

        for u in us {
            // Not the edge's own ends (they are points of the polygon).
            if (u - up).abs() < 1e-6 || (u - uq).abs() < 1e-6 {
                continue;
            }

            let v = vp + (vq - vp) * (u - up) / (uq - up);
            out.push(on_wall(plan, u, v));
        }
    }

    out
}

/// The AI layout's polygons that can run along a wall: the siege AI's wall edges, the walls' battlefield zones, the fort perimeters,
/// the deployment zones.
fn layout_polygons(scene: &TedScene) -> Vec<usize> {
    let attr = |e: usize, k: &str| scene.doc().get(e, k).unwrap_or_default();
    let mut out = vec![];
    for edge in scene.find_all("SiegeAIEdgeInstance").into_iter().filter(|&e| attr(e, "type") == "SAET_WALL") {
        out.extend(scene.find_all("SIEGE_AI_POLYGON_INSTANCE").into_iter().filter(|&p| scene.is_descendant(p, edge)));
    }

    out.extend(scene.find_all("ZONE_POLYGON_INSTANCE").into_iter().filter(|&e| attr(e, "purpose") == "EFP_LOW_WALL"));
    out.extend(scene.find_all("AI_HINT_POLYGON_INSTANCE").into_iter().filter(|&e| attr(e, "hint_type") == "AIH_FORT_PERIMETER"));
    out.extend(scene.find_all("DEPLOYMENT_ZONE_POLYGON_INSTANCE"));
    out
}

fn describe(scene: &TedScene, e: usize) -> String {
    let attr = |k: &str| scene.doc().get(e, k).unwrap_or_default();
    let kind = match scene.type_of(e) {
        "SIEGE_AI_POLYGON_INSTANCE" => "siege AI wall edge".to_owned(),
        "ZONE_POLYGON_INSTANCE" => format!("battlefield zone {}", attr("purpose")),
        "AI_HINT_POLYGON_INSTANCE" => format!("AI hint {}", attr("hint_type")),
        other => other.to_owned(),
    };
    format!("{kind} #{e} '{}'", scene.name(e))
}

/// Replaces a polygon's points (world x, z; their local y kept, the first's for added ones).
fn set_points(scene: &mut TedScene, e: usize, points: &[[f64; 2]]) -> Result<(), String> {
    let w = world_transform(scene, e);
    let inverse = w.rotation.transpose();
    let y = scene.polygon_points(e).first().and_then(|&p| scene.point_position(p)).map_or(0.0, |p| p.y);
    let local = |p: [f64; 2]| {
        let mut l = inverse * (Double3::new(p[0], 0.0, p[1]) - w.position);
        l.y = y;
        l
    };

    while scene.polygon_points(e).len() > points.len().max(2) {
        let n = scene.polygon_points(e).len();
        scene.remove_polygon_point(e, n - 1)?;
    }

    for (i, &p) in points.iter().enumerate() {
        if i < scene.polygon_points(e).len() {
            scene.set_polygon_point(e, i, local(p))?;
        } else {
            scene.insert_polygon_point(e, i - 1, local(p))?;
        }
    }

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

/// The fort building end nearest to a point, within [`SNAP`].
fn nearest(ends: &[Anchor], p: [f64; 2]) -> Option<&Anchor> {
    let d = |a: &Anchor| (a.at[0] - p[0]).hypot(a.at[2] - p[1]);
    ends.iter().filter(|a| d(a) <= SNAP).min_by(|a, b| d(a).total_cmp(&d(b)))
}

/// Raises a bell of ground (scene units), and moves the buildings standing on it by as much, as the editor's brush does: those that sit
/// on the landscape (`MSM_LANDSCAPE`), not the freeform ones (the walls).
fn raise(project: &mut TileProject, at: [f64; 2], radius: f64, metres: f64) -> Result<(), String> {
    let unit = project.unit_scale;
    let buildings = project.scene.find_all("BUILDING_INSTANCE").into_iter()
        .filter(|&e| project.scene.doc().get(e, "surface_mode").is_some_and(|m| m == "MSM_LANDSCAPE")).collect::<Vec<_>>();
    let before = buildings.iter().map(|&e| {
        let p = world_transform(&project.scene, e).position;
        project.height_at(p.x / unit, p.z / unit)
    }).collect::<Vec<_>>();

    let (x, z) = (at[0] / unit, at[1] / unit);
    let brush = Brush { cx: project.pixel_x(x), cy: project.pixel_z(z), radius: radius / unit / project.mpp_x(), strength: 1.0, falloff: 1.0 };
    let field = project.height.as_mut().ok_or("the tile has no height map")?;
    field.begin_edit()?;
    field.raise(&brush, metres)?;
    field.end_edit();
    project.height_dirty = true;
    println!("ground {metres:+.0} m at ({:.0}, {:.0}): now {:.1}", at[0], at[1], project.height_at(x, z));

    let mut moved = 0;
    for (&e, h0) in buildings.iter().zip(before) {
        let p = world_transform(&project.scene, e).position;
        let dh = project.height_at(p.x / unit, p.z / unit) - h0;
        if dh.abs() > 0.01 {
            wce_tile::world_transform::set_world_position(&mut project.scene, e, Double3::new(p.x, p.y + dh, p.z));
            moved += 1;
        }
    }

    println!("{moved} building(s) moved with the ground");
    Ok(())
}
