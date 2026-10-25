//! Procedural walls in a tile.ted scene (docs/plans/procedural_walls.md): what the editor and the exporter share. A `WceWallSpline` is
//! stored like the polygons (a `<points>` list, `polygon_closed="0"`) with `wall_base`, `wall_tolerance`, `wall_flip` and its points'
//! `sharp`. Scene units throughout (the models' units; the height map's metres are the scene's y).

use wce_foundation::math::Double3;
use wce_tile::properties::parse_bool;
use wce_tile::ted_scene::TedScene;
use wce_tile::world_transform::{euler_matrix, set_world_position, set_world_rotation, world_transform};

use crate::pieces::{Piece, door_pieces};
use crate::profile::{Profile, profile};
use crate::solver::{Plan, WallInput, solve};

pub use wce_tile::new_instances::WALL_SPLINE;

/// The base a wall without (or with an unknown) `wall_base` uses.
pub const DEFAULT_BASE: &str = "barbarian_fort_straight";

/// An end within this of a building's end is attached to it (scene units, horizontally).
pub const ATTACH: f64 = 0.5;

/// A fort building's end a wall can attach to: the building element and the end (its y is the level the building stands on); the
/// building's x axis (x, z: the normal of its end faces) and its outer side (its -z axis).
#[derive(Clone, Debug, PartialEq)]
pub struct Anchor {
    pub elem: usize,
    pub key: String,
    pub at: [f64; 3],
    pub axis: [f64; 2],
    pub outer: [f64; 2],
}

/// A wall's layout and what it was made from.
#[derive(Clone, Debug)]
pub struct WallLayout {
    pub plan: Plan,
    pub profile: &'static Profile,

    /// The building end each end of the wall is attached to.
    pub ends: [Option<Anchor>; 2],
}

/// The walls of a scene, in document order.
pub fn walls(scene: &TedScene) -> Vec<usize> {
    scene.find_all(WALL_SPLINE)
}

/// The ends of the fort buildings (`*_fort_*` keys, not generated ones): their frame's x extent (`extent(key)`: the logic file's box)
/// at z = 0, through the instance's transform.
pub fn anchors(scene: &TedScene, extent: &dyn Fn(&str) -> Option<(f64, f64)>) -> Vec<Anchor> {
    let mut out = vec![];
    for e in scene.find_all("BUILDING_INSTANCE") {
        let key = scene.doc().get(e, "key").unwrap_or_default();
        let lower = key.to_lowercase();
        if !lower.contains("_fort_") || lower.starts_with("wce_") {
            continue;
        }

        let Some((lo, hi)) = extent(&key) else { continue };
        let w = world_transform(scene, e);
        let x_axis = w.rotation * Double3::new(1.0, 0.0, 0.0);
        let outer = w.rotation * Double3::new(0.0, 0.0, -1.0);
        for x in [lo, hi] {
            let p = w.position + w.rotation * Double3::new(x, 0.0, 0.0);
            out.push(Anchor { elem: e, key: key.clone(), at: [p.x, w.position.y, p.z], axis: [x_axis.x, x_axis.z], outer: [outer.x, outer.z] });
        }
    }

    out
}

/// A wall's points (x, z) and their `sharp` marks.
pub fn wall_points(scene: &TedScene, elem: usize) -> (Vec<[f64; 2]>, Vec<bool>) {
    let w = world_transform(scene, elem);
    let pts = scene.polygon_points(elem);
    let points = pts.iter().filter_map(|&p| scene.point_position(p)).map(|v| {
        let p = w.position + w.rotation * v;
        [p.x, p.z]
    }).collect();
    let sharp = pts.iter().map(|&p| scene.doc().get(p, "sharp").is_some_and(|v| parse_bool(&v))).collect();
    (points, sharp)
}

/// A wall's layout; `None` for an element that is not a wall or has fewer than 2 points.
pub fn layout(scene: &TedScene, elem: usize, anchors: &[Anchor], ground: &dyn Fn(f64, f64) -> f64) -> Option<WallLayout> {
    if scene.type_of(elem) != WALL_SPLINE {
        return None;
    }

    let base = scene.doc().get(elem, "wall_base").unwrap_or_default();
    let profile = profile(&base).or_else(|| profile(DEFAULT_BASE))?;
    let tolerance = scene.doc().get(elem, "wall_tolerance").and_then(|t| t.trim().parse::<f64>().ok()).unwrap_or(0.5).max(0.0);
    let flip = scene.doc().get(elem, "wall_flip").is_some_and(|v| parse_bool(&v));
    let every = |attr: &str, default: f64| scene.doc().get(elem, attr).and_then(|t| t.trim().parse::<f64>().ok()).unwrap_or(default).max(0.0);
    let (points, sharp) = wall_points(scene, elem);
    if points.len() < 2 {
        return None;
    }

    let attached = |p: [f64; 2]| anchors.iter().filter(|a| (a.at[0] - p[0]).hypot(a.at[2] - p[1]) <= ATTACH).min_by(|a, b| {
        let d = |x: &Anchor| (x.at[0] - p[0]).hypot(x.at[2] - p[1]);
        d(a).total_cmp(&d(b))
    }).cloned();
    let ends = [attached(points[0]), attached(points[points.len() - 1])];
    let input = WallInput {
        points,
        sharp,
        flip,
        tolerance,
        ends: [ends[0].as_ref().map(|a| a.at[1]), ends[1].as_ref().map(|a| a.at[1])],
        end_planes: [ends[0].as_ref().map(|a| a.axis), ends[1].as_ref().map(|a| a.axis)],
        access_every: every("wall_access_every", crate::pieces::DOOR_EVERY),
        defence_every: every("wall_defence_every", 0.0),
    };

    let mut plan = solve(&input, profile, ground);

    // An attached end whose outer side is the other way from the building's: the wall's outer face would meet its inner one.
    if let (Some(first), Some(last)) = (plan.samples.first().copied(), plan.samples.last().copied()) {
        // The plan runs from the last point when flipped.
        let (at_first, at_last) = if flip { (&ends[1], &ends[0]) } else { (&ends[0], &ends[1]) };
        for (sm, a) in [(first, at_first), (last, at_last)] {
            let Some(a) = a else { continue };
            let wall_outer = [sm.dir[1], -sm.dir[0]];
            if wall_outer[0] * a.outer[0] + wall_outer[1] * a.outer[1] < 0.0 {
                plan.warnings.push(format!("the end on {} has its outer side the other way from the building's: flip the wall", a.key));
            }
        }
    }

    Some(WallLayout { plan, profile, ends })
}

/// Which pieces of a wall are door sections, and the doors skipped (`pieces::door_pieces`): a landing is free when no point of a 5 x 5
/// grid over it is in a shown no-go polygon and no shown building stands in it (within its `radius(key)`, scene units).
pub fn doors(scene: &TedScene, layout: &WallLayout, ps: &[Piece], ground: &dyn Fn(f64, f64) -> f64, radius: &dyn Fn(&str) -> f64) -> (Vec<usize>, Vec<String>) {
    if layout.profile.door.is_empty() {
        return (vec![], vec![]);
    }

    let shown = |e: usize| scene.visible(e) && scene.active(e);
    let nogo = scene.find_all("NOGO_REGION_POLYGON_INSTANCE").into_iter().filter(|&e| shown(e)).map(|e| wall_points(scene, e).0).collect::<Vec<_>>();
    let buildings = scene.find_all("BUILDING_INSTANCE").into_iter().filter(|&e| shown(e)).map(|e| {
        let p = world_transform(scene, e).position;
        ([p.x, p.z], radius(&scene.doc().get(e, "key").unwrap_or_default()))
    }).collect::<Vec<_>>();

    let landing_ok = |sq: &[[f64; 2]; 4]| {
        let inside = |p: [f64; 2], poly: &[[f64; 2]]| {
            let mut c = false;
            for i in 0..poly.len() {
                let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
                if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
                    c = !c;
                }
            }
            c
        };

        let lerp = |a: [f64; 2], b: [f64; 2], t: f64| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        let grid = (0..5).flat_map(|i| (0..5).map(move |j| (f64::from(i) / 4.0, f64::from(j) / 4.0)))
            .map(|(s, t)| lerp(lerp(sq[0], sq[1], s), lerp(sq[3], sq[2], s), t)).collect::<Vec<_>>();
        let in_nogo = grid.iter().any(|&p| nogo.iter().any(|poly| inside(p, poly)));
        let centre = lerp(lerp(sq[0], sq[1], 0.5), lerp(sq[3], sq[2], 0.5), 0.5);
        let half = (sq[0][0] - sq[2][0]).hypot(sq[0][1] - sq[2][1]) / 2.0;
        let blocked = buildings.iter().any(|&(p, r)| (p[0] - centre[0]).hypot(p[1] - centre[1]) < half + r * 0.5);
        !in_nogo && !blocked
    };

    door_pieces(&layout.plan, layout.profile, ps, ground, &landing_ok)
}

/// What a generated piece's instance carries (TEd's attributes of a placed fort wall: breachable, dockable, intact).
const INSTANCE_ATTRS: [(&str, &str); 15] = [
    ("ai_breachable", "true"), ("surface_mode", "MSM_FREEFORM"), ("opacity", "1"), ("burn_amount", "1"), ("destruction_level", "0"),
    ("bounds_colour", "argb(1.0,0.0,0.0,1.0)"), ("indestructible", "false"), ("flags", " OIF_VISIBLE|OIF_ACTIVE|OIF_DRAGGABLE"),
    ("burn", "false"), ("toggleable", "false"), ("glow_amount", "1"), ("weak_wall", "false"), ("building_tech_level", "TL_TECH_LEVEL1"),
    ("dirt_level", "1"), ("dockable", "true"),
];

/// Replaces a wall by the buildings of its pieces: a `BUILDING_INSTANCE` per piece (key, the piece's frame as its world transform) in
/// the wall's layer, or in the top layer named `into` (made when there is none), the wall removed. Returns the new elements' count.
///
/// # Errors
///
/// The wall's layer has no children element, or the text can't be spliced.
pub fn lower(scene: &mut TedScene, wall: usize, placed: &[(String, Piece)], into: Option<&str>) -> Result<usize, String> {
    let layer = match into {
        Some(name) => top_layer(scene, name)?,
        None => scene.doc().elements()[wall].parent.and_then(|c| scene.doc().elements()[c].parent).or(scene.scene_graph()).ok_or("the wall is in no layer")?,
    };
    let name = scene.name(wall);
    for (key, piece) in placed {
        let mut attrs = INSTANCE_ATTRS.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect::<Vec<_>>();
        attrs.push(("key".to_owned(), key.clone()));
        attrs.push(("name".to_owned(), format!("{name}: {key}")));
        let e = scene.add_instance(Some(layer), "BUILDING_INSTANCE", &attrs)?;
        set_world_position(scene, e, Double3::new(piece.origin[0], piece.origin[1], piece.origin[2]));
        set_world_rotation(scene, e, &euler_matrix(Double3::new(-piece.yaw, 0.0, 0.0)));
    }

    // The layer comes after the wall (it is made at the end): its index stays.
    scene.remove(wall)?;
    Ok(placed.len())
}

/// The layer of this name right under the scene graph, made at its end when there is none.
///
/// # Errors
///
/// The scene has no scene graph, or the text can't be spliced.
pub fn top_layer(scene: &mut TedScene, name: &str) -> Result<usize, String> {
    let graph = scene.scene_graph().ok_or("the scene has no SCENE_GRAPH")?;
    match scene.children(graph).into_iter().find(|&c| scene.is_layer(c) && scene.name(c) == name) {
        Some(l) => Ok(l),
        None => scene.add_layer(Some(graph), name),
    }
}
