//! Generate SiegeAI (docs/plans/siege_ai.md): the tile's walls, no-go regions, buildings and captures to `wce_siege`'s input, and its
//! layout written into the `(AUTOGEN) SiegeAI Layout` layer in one undo entry. The layout the tile had goes out of the export: a layer
//! holding only siege AI content gets TEd's "Don't Export", such content in a layer with other things moves into `(OLD) SiegeAI Layout`
//! (not exported).
//!
//! Scene units throughout; the scene's y is the height map's metres.

use std::collections::BTreeMap;
use std::io::Cursor;
use std::sync::Arc;

use rpfm_lib::files::cs2_parsed::Cs2Parsed;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use serde_json::Value;
use wce_foundation::math::Double3;
use wce_siege::{Capture, Input, Layout, P, WallPiece};
use wce_tile::ted_scene::TedScene;
use wce_tile::world_transform::world_transform;
use wce_walls::building::folder;

use super::{Tile, TileDocument, item_of};

/// The layer the generated layout goes into (replaced when generated again).
pub const AUTOGEN_LAYER: &str = "(AUTOGEN) SiegeAI Layout";

/// Where the layout the tile had goes when it shares a layer with other things (not exported).
pub const OLD_LAYER: &str = "(OLD) SiegeAI Layout";

const OFF: &str = "LayerExportType_Off";

/// The hints that belong to the siege layout (the generator makes them).
const LAYOUT_HINTS: [&str; 5] = ["AIH_FORT_PERIMETER", "AIH_FORT_APPROACH", "AIH_DEFAULT_DEPLOYMENT_LINE", "AIH_BEACH_LANDING", "AIH_BEACH_LANDING_FORT_APPROACH"];

/// What the generator needs of a building's logic file (`<key>_tech.cs2.parsed`), in the building's frame.
#[derive(Clone, Debug, Default)]
pub struct LogicSummary {
    /// What blocks units (x, z): its hard collision outlines.
    pub hard: Vec<Vec<P>>,

    /// What it stands on (x, z): its `orange_thingies` (a plaza's floor, a road: walked on).
    pub footprint: Vec<Vec<P>>,

    /// Its raised surfaces (x, z): the platforms 3 units up or more (walkways, tower floors).
    pub platforms: Vec<Vec<P>>,

    /// A fort piece (wall, tower, gate): it has firing positions on a wall (crenels, high / low wall lines).
    pub fort: bool,

    /// It has a gate.
    pub gate: bool,

    /// Its ends along the wall: (x from, x to, z), the frame's x extent at the middle of its top platforms (z 0 without platforms).
    pub ends: (f64, f64, f64),

    /// Its collision hull seen from above (x, z): the triangles of `collision_3d` reaching 1.5 units up or more (the radar's footprint
    /// of a house, whose logic has no outlines).
    pub hull: Vec<[P; 3]>,
}

/// A logic file summed up; `None` when it doesn't read.
pub fn logic_summary(bytes: &[u8]) -> Option<LogicSummary> {
    let parsed = Cs2Parsed::decode(&mut Cursor::new(bytes), &Some(DecodeableExtraData::default())).ok()?;
    let v = serde_json::to_value(&parsed).ok()?;
    let f = |x: &Value| x.as_f64().unwrap_or(0.0);
    let bb = &v["bounding_box"];
    let mut out = LogicSummary::default();
    let (mut orange, mut outlines) = (vec![], vec![]);

    // Platforms on top: (mean y, their points).
    let mut tops: Vec<(f64, Vec<[f64; 3]>)> = vec![];
    for piece in v["pieces"].as_array().into_iter().flatten() {
        let m = piece_matrix(&piece["node_transform"]);
        let at = |x: f64, y: f64, z: f64| -> [f64; 3] {
            [x * m[0][0] + y * m[1][0] + z * m[2][0] + m[3][0], x * m[0][1] + y * m[1][1] + z * m[2][1] + m[3][1], x * m[0][2] + y * m[1][2] + z * m[2][2] + m[3][2]]
        };
        let ds = piece["destructs"].as_array().map_or(&[][..], Vec::as_slice);
        let Some(d) = ds.iter().find(|d| d["index"].as_u64() == Some(0)).or(ds.first()) else { continue };
        for ring in d["orange_thingies"].as_array().into_iter().flatten() {
            let r: Vec<P> = ring.as_array().into_iter().flatten().map(|q| {
                let p = at(f(&q["vertex"]["x"]), 0.0, f(&q["vertex"]["y"]));
                [p[0], p[2]]
            }).collect();
            if r.len() >= 3 {
                orange.push(r);
            }
        }

        for o in d["collision_outlines"].as_array().into_iter().flatten() {
            let r: Vec<P> = o["vertices"]["outline"].as_array().into_iter().flatten().map(|q| {
                let p = at(f(&q["x"]), f(&q["y"]), f(&q["z"]));
                [p[0], p[2]]
            }).collect();
            if r.len() >= 3 {
                outlines.push(r);
            }
        }

        let c3 = &d["collision_3d"];
        let verts: Vec<[f64; 3]> = c3["vertices"].as_array().into_iter().flatten().map(|q| at(f(&q["x"]), f(&q["y"]), f(&q["z"]))).collect();
        for t in c3["triangles"].as_array().into_iter().flatten() {
            let idx = ["vertex_1", "vertex_2", "vertex_3"].map(|k| t[k].as_u64().map(|i| i as usize).filter(|&i| i < verts.len()));
            let [Some(a), Some(b), Some(c)] = idx else { continue };
            let [a, b, c] = [verts[a], verts[b], verts[c]];
            if a[1].max(b[1]).max(c[1]) >= 1.5 {
                out.hull.push([[a[0], a[2]], [b[0], b[2]], [c[0], c[2]]]);
            }
        }

        out.gate |= d["gates"].as_array().is_some_and(|g| !g.is_empty());
        out.fort |= d["ef_lines"].as_array().into_iter().flatten().any(|l| {
            let a = l["action"].as_str().unwrap_or_default();
            a.starts_with("Crenel") || a == "HighWall" || a == "LowWall"
        });
        for p in d["platforms"].as_array().into_iter().flatten() {
            if f(&p["normal"]["y"]) < 0.8 {
                continue;
            }

            let pts: Vec<[f64; 3]> = p["vertices"]["outline"].as_array().into_iter().flatten().map(|q| at(f(&q["x"]), f(&q["y"]), f(&q["z"]))).collect();
            if pts.len() >= 3 {
                let y = pts.iter().map(|q| q[1]).sum::<f64>() / pts.len() as f64;
                if y >= 3.0 {
                    out.platforms.push(pts.iter().map(|q| [q[0], q[2]]).collect());
                }

                tops.push((y, pts));
            }
        }
    }

    out.hard = outlines;
    out.footprint = orange;

    let top = tops.iter().map(|t| t.0).fold(f64::MIN, f64::max);
    let (z0, z1) = tops.iter().filter(|t| t.0 >= top - 1.0).flat_map(|t| &t.1).fold((f64::MAX, f64::MIN), |(a, b), q| (a.min(q[2]), b.max(q[2])));
    let z = if top >= 3.0 && z1 >= z0 { (z0 + z1) / 2.0 } else { 0.0 };
    out.ends = (f(&bb["min_x"]), f(&bb["max_x"]), z);

    Some(out)
}

/// A piece's `node_transform` (row vectors, translation in row 3); the identity when there is none.
fn piece_matrix(t: &Value) -> [[f64; 3]; 4] {
    let mut m = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, 0.0]];
    for (r, row) in m.iter_mut().enumerate() {
        for (c, x) in row.iter_mut().enumerate() {
            if let Some(v) = t.get(format!("m{r}{c}")).and_then(Value::as_f64) {
                *x = v;
            }
        }
    }

    m
}

/// The bounding box of points.
fn bounds<'a>(pts: impl Iterator<Item = &'a P>) -> (P, P) {
    pts.fold(([f64::MAX; 2], [f64::MIN; 2]), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])]))
}

/// Whether an element is exported: no layer above it is "Don't Export".
pub fn exported(scene: &TedScene, elem: usize) -> bool {
    let els = scene.doc().elements();
    let mut p = Some(elem);
    while let Some(e) = p {
        if els[e].name == "LayerInstance" && scene.doc().get(e, "export_type").as_deref() == Some(OFF) {
            return false;
        }

        p = els[e].parent;
    }

    true
}

/// Whether an element is part of a siege layout (what Generate SiegeAI makes).
fn layout_item(scene: &TedScene, e: usize) -> bool {
    match scene.type_of(e) {
        "SiegeAINodeInstance" | "SiegeAIEdgeInstance" | "DeploymentZonePolygonInstanceGroup" => true,
        "AI_HINT_POLYGON_INSTANCE" => scene.doc().get(e, "hint_type").is_some_and(|h| LAYOUT_HINTS.contains(&h.as_str())),
        "AISeparatorInstance" => scene.doc().get(e, "type").as_deref() == Some("PALT_FORT"),
        _ => false,
    }
}

fn is_container(kind: &str) -> bool {
    matches!(kind, "LayerInstance" | "UserInstanceGroup" | "INSTANCE_GROUP" | "LAYER_INSTANCE_GROUP" | "SCENE_GRAPH")
}

/// The layer named `name` directly under the scene graph.
fn top_layer(scene: &TedScene, name: &str) -> Option<usize> {
    let root = scene.scene_graph()?;
    let list = scene.children(root).into_iter().find(|&c| scene.type_of(c) == "children").map_or_else(|| scene.children(root), |c| scene.children(c));
    list.into_iter().find(|&c| scene.type_of(c) == "LayerInstance" && scene.name(c) == name)
}

/// What one building gives the siege overlays and the generator.
#[derive(Debug, Default)]
pub(crate) struct BuildingSiege {
    palisade: Option<wce_siege::PalisadePiece>,
    wall: Option<WallPiece>,
    obstacles: Vec<Vec<P>>,
    raised: Option<WallPiece>,
}

/// What the siege overlays are read from (a building's logic, a polygon, a wall, a capture, what hides them when it is not exported).
fn feeds_siege(sc: &TedScene, e: usize) -> bool {
    let t = sc.type_of(e);
    sc.is_layer(e) || t.starts_with("SiegeAI") || t.starts_with("SIEGE_AI") || t.starts_with("DeploymentZone") || t.starts_with("DEPLOYMENT_ZONE")
        || matches!(t, "BUILDING_INSTANCE" | "NOGO_REGION_POLYGON_INSTANCE" | "CaptureLocationInstance" | super::WALL_SPLINE)
}

impl Tile {
    /// What goes on when the siege overlays have to be read again (only then: lights, trees and names do not feed them).
    pub fn siege_version(&self) -> u64 {
        self.siege_version
    }

    /// An edit's items are known: the buildings among them are read again, and the overlays if anything feeds them.
    pub(crate) fn siege_follows(&mut self, dirty: &wce_editor_core::command::Dirty) {
        if dirty.all {
            self.siege_buildings.get_mut().clear();
            self.siege_version += 1;
            return;
        }

        for &item in &dirty.items {
            if item == super::shift::SHIFTED_ITEM {
                self.siege_version += 1;
                continue;
            }

            let Some(e) = super::elem_of(item).filter(|&e| e < self.tscene().doc().elements().len()) else {
                self.siege_version += u64::from(item == super::TERRAIN);
                continue;
            };

            self.siege_buildings.get_mut().remove(&e);
            self.siege_version += u64::from(feeds_siege(self.tscene(), e));
        }
    }

    /// A building's logic summed up, read once.
    pub(crate) fn logic_of(&self, key: &str) -> Option<Arc<LogicSummary>> {
        if let Some(l) = self.logic_summaries.borrow().get(key) {
            return l.clone();
        }

        self.logic_summaries.borrow_mut().entry(key.to_owned()).or_insert_with(|| {
            let bytes = self.files.as_deref()?.read(&format!("{}{key}_tech.cs2.parsed", folder(key))).ok()?;
            logic_summary(&bytes).map(Arc::new)
        }).clone()
    }

    fn building_siege(&self, e: usize) -> BuildingSiege {
        let sc = self.tscene();
        let mut part = BuildingSiege::default();

        // The siege barricades stand across the streets (they are broken through, the graph runs over them); an encampment's
        // (`siege_barricade_wall_1_encampment`) are its palisade, on its line at their origin.
        let key = sc.doc().get(e, "key").unwrap_or_default();
        let lower = key.to_lowercase();
        if lower.contains("barricade") {
            if lower.contains("encampment") {
                let w = world_transform(sc, e);
                let at = [w.position.x, w.position.z];
                part.palisade = Some(wce_siege::PalisadePiece { at });
                let hard = self.logic_of(&key).map(|l| l.hard.clone()).unwrap_or_default();
                let footprint = hard.iter().map(|r| r.iter().map(|p| {
                    let q = w.position + w.rotation * Double3::new(p[0], 0.0, p[1]);
                    [q.x, q.z]
                }).collect()).collect();
                part.wall = Some(WallPiece { footprint, platforms: vec![], walkway: vec![], gate: false, ends: [at, at] });
            }

            return part;
        }

        let Some(l) = self.logic_of(&key) else { return part };
        let w = world_transform(sc, e);
        let at = |x: f64, z: f64| -> P {
            let p = w.position + w.rotation * Double3::new(x, 0.0, z);
            [p.x, p.z]
        };
        let place = |rings: &[Vec<P>]| -> Vec<Vec<P>> { rings.iter().map(|r| r.iter().map(|p| at(p[0], p[1])).collect()).collect() };
        let mut footprint = place(&l.footprint);
        footprint.extend(place(&l.hard));
        let (x0, x1, z) = l.ends;
        let piece = WallPiece { footprint, platforms: place(&l.platforms), walkway: vec![], gate: l.gate, ends: [at(x0, z), at(x1, z)] };
        if l.fort || l.gate {
            part.wall = Some(piece);
        } else {
            part.obstacles = place(&l.hard);
            part.raised = (!piece.platforms.is_empty()).then_some(piece);
        }

        part
    }

    /// The generator's input: the no-go regions, the buildings (what blocks of them; the fort pieces' footprints, platforms and gates, from
    /// their logic files), the procedural walls, the kit walls (their pieces, their stretches), the captures, the sea; what is not exported
    /// left out.
    pub fn siege_input(&self) -> Input {
        self.siege_input_with(true)
    }

    /// The input, the sea's heights only with `sea` (the generator's; what draws the layout has no use for them).
    pub(crate) fn siege_input_with(&self, sea: bool) -> Input {
        let sc = self.tscene();
        let u = self.unit();
        let mut input = Input { bounds: ([0.0, 0.0], [self.project.extent_x() * u, self.project.extent_z() * u]), ..Input::default() };
        let autogen = top_layer(sc, AUTOGEN_LAYER);
        let skip = |e: usize| !exported(sc, e) || autogen.is_some_and(|l| sc.is_descendant(e, l));

        let world_points = |e: usize| world_points(sc, e);

        for e in sc.find_all("NOGO_REGION_POLYGON_INSTANCE") {
            if !skip(e) {
                input.nogo.push(world_points(e));
            }
        }

        // The steep ground the game will not walk: the automatic no-go areas the tool worked out (metres).
        if let Some(outlines) = &self.auto_nogos {
            input.steep = outlines.iter().map(|o| o.iter().map(|p| [f64::from(p[0]) * u, f64::from(p[1]) * u]).collect()).collect();
        }

        let read = |key: &str| self.logic_of(key);

        // The minimap's read keeps each building's part: walking them all took 50 ms on a city, after each edit. Not while the numbers are
        // being changed, and not for the generator, which is not asked often.
        let kept = !sea && self.idx.valid && self.changes.is_empty();
        let mut raised = vec![];
        for e in sc.find_all("BUILDING_INSTANCE") {
            if skip(e) {
                continue;
            }

            let cached = kept.then(|| self.siege_buildings.borrow().get(&e).cloned()).flatten();
            let part = cached.unwrap_or_else(|| {
                let part = Arc::new(self.building_siege(e));
                if kept {
                    self.siege_buildings.borrow_mut().insert(e, part.clone());
                }

                part
            });

            input.palisade.extend(part.palisade);
            input.walls.extend(part.wall.iter().cloned());
            input.obstacles.extend(part.obstacles.iter().cloned());
            raised.extend(part.raised.iter().cloned());
        }


        // Raised buildings touching the fort (towers, corners without firing positions) are part of it.
        let boxes: Vec<(P, P)> = input.walls.iter().map(|w| bounds(w.footprint.iter().chain(&w.platforms).flatten())).collect();
        for piece in raised {
            let (lo, hi) = bounds(piece.footprint.iter().chain(&piece.platforms).flatten());
            if boxes.iter().any(|(a, b)| lo[0] <= b[0] + 3.0 && hi[0] >= a[0] - 3.0 && lo[1] <= b[1] + 3.0 && hi[1] >= a[1] - 3.0) {
                input.walls.push(piece);
            }
        }

        // A wall along a line (a procedural wall's plan, a kit wall's joints): its walkway from its base's, the footprint across its depth.
        // Each point: the centre line, its outer foot, its direction.
        let band = |points: &[([f64; 2], [f64; 2], [f64; 2])], profile: &wce_walls::profile::Profile| -> Option<WallPiece> {
            let z = read(profile.key).map_or(0.0, |l| l.ends.2);
            let (outer, inner) = profile.depth;
            let (mut walk, mut left, mut right) = (vec![], vec![], vec![]);
            for &(centre, foot, dir) in points {
                let s = Sample { centre, foot, dir };
                let d = [s.centre[0] - s.foot[0], s.centre[1] - s.foot[1]];
                let l = d[0].hypot(d[1]);
                let n = if l > 1e-6 { [d[0] / l, d[1] / l] } else { [-s.dir[1], s.dir[0]] };
                let off = |t: f64| [s.centre[0] + n[0] * t, s.centre[1] + n[1] * t];
                walk.push(off(z));
                left.push(off(inner));
                right.push(off(outer));
            }

            if walk.len() < 2 {
                return None;
            }

            right.reverse();
            left.extend(right);
            let ends = [walk[0], walk[walk.len() - 1]];
            Some(WallPiece { footprint: vec![left], platforms: vec![], walkway: walk, gate: false, ends })
        };
        let along = |plan: &wce_walls::solver::Plan, profile: &wce_walls::profile::Profile| band(&plan.samples.iter().map(|s| (s.centre, s.foot, s.dir)).collect::<Vec<_>>(), profile);

        // A game's piece placed as it is (a tower, a bastion, a cap, a kit wall's piece): what blocks of it, from its logic file, as a fort
        // building's.
        let game_piece = |key: &str, p: &wce_walls::pieces::Piece| -> Option<WallPiece> {
            let l = read(key)?;
            let (sn, cs) = p.yaw.sin_cos();
            let at = |x: f64, z: f64| -> P { [p.origin[0] + x * cs - z * sn, p.origin[2] + x * sn + z * cs] };
            let place = |rings: &[Vec<P>]| -> Vec<Vec<P>> { rings.iter().map(|r| r.iter().map(|q| at(q[0], q[1])).collect()).collect() };
            let mut footprint = place(&l.footprint);
            footprint.extend(place(&l.hard));
            let (x0, x1, z) = l.ends;
            Some(WallPiece { footprint, platforms: place(&l.platforms), walkway: vec![], gate: false, ends: [at(x0, z), at(x1, z)] })
        };

        // Procedural walls, with their towers and bastions and the caps past their free ends (the game's pieces: they stand out of the
        // wall's depth).
        for e in wce_walls::scene::walls(sc) {
            if skip(e) {
                continue;
            }

            let Some(layout) = self.wall_layout(e) else { continue };
            input.walls.extend(along(&layout.plan, layout.profile));
            let caps = wce_walls::pieces::cap_instances(&layout.plan, layout.profile).into_iter().map(|(_, p)| p);
            for p in wce_walls::pieces::pieces(&layout.plan, layout.profile).into_iter().filter(|p| p.rigid).chain(caps) {
                input.walls.extend(p.base.and_then(|key| game_piece(key, &p)));
            }
        }

        // Kit walls (docs/plans/kit_walls.md phase 5): their game pieces as they are, their stretches as procedural walls; as laid for
        // their preview.
        for e in wce_walls::scene::all_walls(sc).into_iter().filter(|&e| !wce_walls::scene::is_procedural(sc, e)) {
            if skip(e) {
                continue;
            }

            let (Some(layout), Some(profile)) = (self.kit_wall_report(e), wce_walls::scene::wall_profile(sc, e)) else { continue };

            // Its run of pieces as a procedural wall's (the joints its line: a barbarian piece's footprint is a strip along its outer face
            // only, which closes no outline); the caps, towers and bastions, which stand out of the wall's depth, from their logic files.
            let run = layout.placed.iter().filter(|p| p.role != wce_walls::kit::Role::Cap).collect::<Vec<_>>();
            let mut points = vec![];
            for (k, pl) in run.iter().enumerate() {
                let (a, b) = (pl.joints[0], pl.joints[1]);
                let l = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-9);
                let dir = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
                let z_axis = [-dir[1], dir[0]];
                let foot = |c: [f64; 2]| [c[0] + z_axis[0] * profile.outer_face, c[1] + z_axis[1] * profile.outer_face];
                // Every `solver::STEP` along, as a procedural wall's samples.
                let n = (l / wce_walls::solver::STEP).ceil().max(1.0) as usize;
                for i in usize::from(k > 0)..=n {
                    let t = i as f64 / n as f64;
                    let c = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
                    points.push((c, foot(c), dir));
                }
            }

            input.walls.extend(band(&points, profile));
            for pl in layout.placed.iter().filter(|p| matches!(p.role, wce_walls::kit::Role::Cap | wce_walls::kit::Role::Defence)) {
                let p = wce_walls::pieces::Piece { s: (0.0, 0.0), origin: pl.origin, yaw: pl.yaw, scale: 1.0, base: None, rigid: true };
                input.walls.extend(game_piece(&pl.key, &p));
            }

            for plan in self.kit_stretch_plans(e, layout) {
                input.walls.extend(along(&plan, profile));
                for p in wce_walls::pieces::pieces(&plan, profile).into_iter().filter(|p| p.rigid) {
                    input.walls.extend(p.base.and_then(|key| game_piece(key, &p)));
                }
            }
        }

        for e in sc.find_all("CaptureLocationInstance") {
            if skip(e) {
                continue;
            }

            let p = world_transform(sc, e).position;
            input.captures.push(Capture { at: [p.x, p.z], major: sc.doc().get(e, "type").as_deref() == Some("CLT_MAJOR") });
        }

        // Ground under the sea: the heights, sampled every 4 units (the generator keeps the water joined to the map's edge).
        let height = |x: f64, z: f64| self.project.height_at(x / u, z / u);
        let (hx, hz) = (input.bounds.1[0], input.bounds.1[1]);
        let any_sea = sea && (0..=32).any(|i| (0..=32).any(|j| height(hx * f64::from(i) / 32.0, hz * f64::from(j) / 32.0) < wce_siege::Sea::WATER));
        if any_sea {
            let cell = 4.0;
            let (w, h) = ((hx / cell).ceil() as usize, (hz / cell).ceil() as usize);
            let heights = (0..w * h).map(|k| height(((k % w) as f64 + 0.5) * cell, ((k / w) as f64 + 0.5) * cell) as f32).collect();
            input.sea = Some(wce_siege::Sea { x0: 0.0, z0: 0.0, cell, w, h, height: heights });
        }

        input
    }
}

/// A siege edge's ends and the nodes they join, as the game joins them (docs/re/siege_ai.md, "The connection rule", `FUN_1027b320`): the
/// node whose Bounding Box holds the Street's first point is the start, the one holding its last point the end, the last match winning;
/// street and area connection edges against area, intersection and entry nodes, wall and palisade edges against wall area nodes.
#[derive(Clone, Debug, PartialEq)]
pub struct SiegeEdgeEnds {
    /// The Street's first and last points (world metres).
    pub at: [wce_foundation::math::Float3; 2],
    pub nodes: [Option<usize>; 2],

    /// A wall or palisade edge: joined to wall area nodes only, which CA's layouts almost never have (2 in the kit), so an end in no
    /// node is the usual case and not a fault.
    pub walls: bool,
}

impl Tile {
    /// A siege polygon under an instance, by its name (`Street`, `Bounding Box`): its points in world metres (x, z; y 0).
    fn siege_polygon(&self, e: usize, name: &str) -> Option<Vec<wce_foundation::math::Float3>> {
        let sc = self.tscene();
        let u = self.unit();
        let c = sc.children(e).into_iter().find(|&c| sc.type_of(c) == "SIEGE_AI_POLYGON_INSTANCE" && sc.name(c) == name)?;
        let w = world_transform(sc, c);
        Some(sc.polygon_points(c).iter().filter_map(|&p| sc.point_position(p)).map(|v| {
            let p = w.position + w.rotation * v;
            wce_foundation::math::Float3::new((p.x / u) as f32, 0.0, (p.z / u) as f32)
        }).collect())
    }

    /// A siege edge's Street (world metres, y 0).
    pub fn siege_polygon_world(&self, edge: usize) -> Option<Vec<wce_foundation::math::Float3>> {
        self.siege_polygon(edge, "Street")
    }

    /// A siege node's rectangle (its Bounding Box's first four points, world metres), `None` without one.
    pub fn siege_node_rect(&self, node: usize) -> Option<[wce_foundation::math::Float3; 4]> {
        if self.tscene().type_of(node) != "SiegeAINodeInstance" {
            return None;
        }

        let b = self.siege_polygon(node, "Bounding Box")?;
        b.get(..4).and_then(|b| <[wce_foundation::math::Float3; 4]>::try_from(b).ok())
    }

    /// The exported siege nodes with a rectangle: element, whether a wall area node, its rectangle.
    pub fn siege_nodes(&self) -> Vec<(usize, bool, [wce_foundation::math::Float3; 4])> {
        let sc = self.tscene();
        sc.find_all("SiegeAINodeInstance").into_iter().filter(|&n| exported(sc, n)).filter_map(|n| {
            Some((n, sc.doc().get(n, "type").as_deref() == Some("SANT_WALL_AREA"), self.siege_node_rect(n)?))
        }).collect()
    }

    /// Which nodes a siege edge joins (`SiegeEdgeEnds`); `None` for anything else or an edge without a Street.
    pub fn siege_edge_ends(&self, edge: usize) -> Option<SiegeEdgeEnds> {
        self.siege_edge_ends_among(edge, &self.siege_nodes())
    }

    /// `siege_edge_ends` against `siege_nodes`' list.
    pub fn siege_edge_ends_among(&self, edge: usize, nodes: &[(usize, bool, [wce_foundation::math::Float3; 4])]) -> Option<SiegeEdgeEnds> {
        let sc = self.tscene();
        if sc.type_of(edge) != "SiegeAIEdgeInstance" {
            return None;
        }

        let line = self.siege_polygon(edge, "Street").filter(|l| l.len() >= 2)?;
        let at = [line[0], line[line.len() - 1]];
        let walls = matches!(sc.doc().get(edge, "type").as_deref(), Some("SAET_WALL" | "SAET_PALISADE"));
        let inside = |r: &[wce_foundation::math::Float3; 4], p: wce_foundation::math::Float3| {
            // In the quad when on the same side of its four edges (it is a rectangle, its points in order).
            let side = |a: wce_foundation::math::Float3, b: wce_foundation::math::Float3| (b.x - a.x) * (p.z - a.z) - (b.z - a.z) * (p.x - a.x);
            let s = [side(r[0], r[1]), side(r[1], r[2]), side(r[2], r[3]), side(r[3], r[0])];
            s.iter().all(|&v| v >= 0.0) || s.iter().all(|&v| v <= 0.0)
        };

        let mut found = [None, None];
        for (n, wall_area, r) in nodes {
            if *wall_area != walls {
                continue;
            }

            for k in 0..2 {
                if inside(r, at[k]) {
                    found[k] = Some(*n);
                }
            }
        }

        Some(SiegeEdgeEnds { at, nodes: found, walls })
    }

    /// TEd's scene integrity checks (Phase 6b of docs/plans/tile_ted_parity.md; its Integrity errors dialog, `FUN_004bb1e0`), but the
    /// vegetation's (the Problems section has it): each instance's own (`FUN_00452180` for the polygons: the points inside the tile's area,
    /// consecutive duplicates dropped; the siege AI nodes' and edges' shapes; a deployment zone's polygons). Each with its element: TEd's
    /// message where it has one.
    pub fn integrity_errors(&self) -> Vec<(usize, String)> {
        let sc = self.tscene();
        let (w, h) = (self.extent_x() as f32, self.extent_z() as f32);
        let mut out = vec![];
        for &e in &self.instances {
            let ty = sc.type_of(e);
            if self.is_polygon(e) && !wce_tile::splines::is_spline(ty) && !self.is_wall(e) {
                // The polygon (or line) clipped to the tile's area: what is left of it there.
                let closed = sc.doc().get(e, "polygon_closed").is_none_or(|v| v != "0");
                let pts = self.polygon_world_points(e);
                let inside = clip_to_rect(&pts, closed, w, h);
                if inside.is_empty() {
                    out.push((e, "Polygonal line should have at least one vertex inside the valid area of the tile".to_owned()));
                    continue;
                }

                let same = |a: &wce_foundation::math::Float3, b: &wce_foundation::math::Float3| (a.x - b.x).abs() < 1e-3 && (a.z - b.z).abs() < 1e-3;
                let mut distinct = inside.windows(2).filter(|p| !same(&p[0], &p[1])).count() + 1;
                if closed && inside.len() > 1 && same(&inside[0], &inside[inside.len() - 1]) {
                    distinct -= 1;
                }

                if !closed && distinct < 2 {
                    out.push((e, "Polygonal line has less than three valid vertices (TEd's words: it needs two)".to_owned()));
                } else if closed && distinct < 3 {
                    out.push((e, format!("A closed polygon must at least have three vertices. Current number of verts is {distinct}")));
                }
            }

            let named = |name: &str| sc.children(e).into_iter().any(|c| sc.type_of(c) == "SIEGE_AI_POLYGON_INSTANCE" && sc.name(c) == name);
            match ty {
                "SiegeAINodeInstance" => {
                    if !named("Bounding Box") {
                        out.push((e, "Siege AI Node should have a main box shape".to_owned()));
                    }

                    if !named("Boundary") {
                        out.push((e, "Siege AI Node has no boundary".to_owned()));
                    }
                }

                "SiegeAIEdgeInstance" => {
                    if !named("Street") {
                        out.push((e, "Siege AI Edge should have a main street shape".to_owned()));
                    }

                    let sides = sc.children(e).into_iter().filter(|&c| sc.type_of(c) == "SIEGE_AI_POLYGON_INSTANCE" && sc.name(c) == "Street boundary").count();
                    if sc.doc().get(e, "type").as_deref() == Some("SAET_STREET") && sides != 2 {
                        out.push((e, format!("Street Edge needs to have two boundary splines (it has {sides})")));
                    }
                }

                "DeploymentZonePolygonInstanceGroup" if !sc.children(e).into_iter().any(|c| sc.type_of(c) == "DEPLOYMENT_ZONE_POLYGON_INSTANCE") => {
                    out.push((e, "Deployment Zone must have at least one polygon".to_owned()));
                }

                _ => {}
            }
        }

        out
    }

    /// The siege layout the tile exports now (CA's, or a generated one): nodes, edges, the perimeter, the approaches, the deployment zones.
    pub fn current_siege_layout(&self) -> Layout {
        let sc = self.tscene();
        let world_points = |e: usize| world_points(sc, e);
        let polys = |e: usize| polys(sc, e);

        let mut l = Layout::default();
        l.nodes.extend(sc.find_all("SiegeAINodeInstance").into_iter().filter(|&e| exported(sc, e)).filter_map(|e| node_of(sc, e)));

        for e in sc.find_all("SiegeAIEdgeInstance").into_iter().filter(|&e| exported(sc, e)) {
            let kind = match sc.doc().get(e, "type").as_deref() {
                Some("SAET_STREET") => wce_siege::EdgeKind::Street,
                Some("SAET_AREA_CONNECTION") => wce_siege::EdgeKind::AreaConnection,
                Some("SAET_PALISADE") => wce_siege::EdgeKind::Palisade,
                _ => wce_siege::EdgeKind::Wall,
            };
            let ps = polys(e);
            let line = ps.iter().find(|p| p.0 == "Street").map(|p| p.1.clone()).unwrap_or_default();
            let sides = ps.iter().filter(|p| p.0 == "Street boundary").map(|p| p.1.clone()).collect();
            l.edges.push(wce_siege::Edge { kind, line, sides, nodes: None });
        }

        for e in sc.find_all("AI_HINT_POLYGON_INSTANCE").into_iter().filter(|&e| exported(sc, e)) {
            let p = world_points(e);
            let two = || (p.len() >= 2).then(|| [p[0], p[p.len() - 1]]);
            match sc.doc().get(e, "hint_type").as_deref() {
                Some("AIH_FORT_PERIMETER") => l.perimeter = p.clone(),
                Some("AIH_FORT_APPROACH") => l.approaches.extend(two()),
                Some("AIH_DEFAULT_DEPLOYMENT_LINE") => l.deployment_lines.extend(two()),
                Some("AIH_BEACH_LANDING") => l.beaches.push(p.clone()),
                Some("AIH_BEACH_LANDING_FORT_APPROACH") => l.beach_approaches.extend(two()),
                _ => {}
            }
        }

        for e in sc.find_all("AISeparatorInstance").into_iter().filter(|&e| exported(sc, e) && layout_item(sc, e)) {
            l.separators.push(world_points(e));
        }

        for (alliance, d) in self.deployment_zones() {
            if alliance == "1" {
                l.defender = d;
            } else {
                l.attacker = d;
            }
        }

        l
    }

    /// Every exported deployment zone group of the tile: its `alliance_id` and its polygons, in document order (the siege layout keeps the
    /// last one of each alliance).
    pub fn deployment_zones(&self) -> Vec<(String, wce_siege::Deployment)> {
        let sc = self.tscene();
        let zone = |t: &str| match t {
            "DZPT_SUBSTRACTIVE" => wce_siege::ZoneKind::Subtractive,
            "DZPT_GUERRILLA_EXCLUSION_ADDITIVE" => wce_siege::ZoneKind::GuerrillaAdditive,
            "DZPT_GUERRILLA_EXCLUSION_SUBTRACTIVE" => wce_siege::ZoneKind::GuerrillaSubtractive,
            _ => wce_siege::ZoneKind::Additive,
        };

        sc.find_all("DeploymentZonePolygonInstanceGroup").into_iter().filter(|&e| exported(sc, e)).map(|e| {
            let d = wce_siege::Deployment { polygons: polys(sc, e).into_iter().map(|(t, p)| (zone(&t), p)).collect() };
            (sc.doc().get(e, "alliance_id").unwrap_or_default(), d)
        }).collect()
    }
}

/// A polygon's points on the ground (x, z), scene units.
fn world_points(sc: &TedScene, e: usize) -> Vec<P> {
    let w = world_transform(sc, e);
    sc.polygon_points(e).iter().filter_map(|&p| sc.point_position(p)).map(|v| {
        let p = w.position + w.rotation * v;
        [p.x, p.z]
    }).collect()
}

/// A siege AI node as the generator reads it: its type, its Bounding Box (its first 4 points) and its Boundary. None without a Bounding
/// Box of 4 points.
fn node_of(sc: &TedScene, e: usize) -> Option<wce_siege::Node> {
    let kind = match sc.doc().get(e, "type").as_deref() {
        Some("SANT_INTERSECTION") => wce_siege::NodeKind::Intersection,
        Some("SANT_ENTRY") => wce_siege::NodeKind::Entry,
        _ => wce_siege::NodeKind::Area,
    };
    let ps = polys(sc, e);
    let find = |name: &str| ps.iter().find(|p| p.0 == name).map(|p| p.1.clone()).unwrap_or_default();
    let (bbox, boundary) = (find("Bounding Box"), find("Boundary"));
    let bbox = <[P; 4]>::try_from(bbox.get(..4)?).ok()?;
    let centre = [(bbox[0][0] + bbox[2][0]) / 2.0, (bbox[0][1] + bbox[2][1]) / 2.0];
    Some(wce_siege::Node { kind, centre, bbox, boundary })
}

/// The polygons under an instance: their name (a siege polygon's) or deployment type, and their points.
fn polys(sc: &TedScene, e: usize) -> Vec<(String, Vec<P>)> {
    // Its subtree only (a search of the whole document per instance was most of the time on a big tile).
    let els = sc.doc().elements();
    let (mut siege, mut zones) = (vec![], vec![]);
    let mut stack: Vec<usize> = els[e].children.iter().rev().copied().collect();
    while let Some(c) = stack.pop() {
        match els[c].name.as_str() {
            "SIEGE_AI_POLYGON_INSTANCE" => siege.push((sc.name(c), world_points(sc, c))),
            "DEPLOYMENT_ZONE_POLYGON_INSTANCE" => zones.push((sc.doc().get(c, "deployment_polygon_type").unwrap_or_default(), world_points(sc, c))),
            _ => {}
        }

        stack.extend(els[c].children.iter().rev());
    }

    siege.extend(zones);
    siege
}

impl TileDocument {
    /// The two nodes a siege AI edge joins, as the game finds them (docs/re/siege_ai.md): the node whose rectangle (its Bounding Box's
    /// first 4 points) holds its Street's first point, and the one holding its last (the last such node in the file, each). `None`
    /// without a Street, or when an end is in no node.
    pub fn street_nodes(&self, edge: usize) -> Option<(usize, usize)> {
        let sc = self.tile().tscene();
        if edge >= sc.doc().elements().len() || sc.type_of(edge) != "SiegeAIEdgeInstance" {
            return None;
        }

        let line = polys(sc, edge).into_iter().find(|p| p.0 == "Street").map(|p| p.1)?;
        let (first, last) = (*line.first()?, *line.last()?);
        let nodes = sc.find_all("SiegeAINodeInstance").into_iter().filter(|&e| exported(sc, e)).filter_map(|e| node_of(sc, e).map(|n| (e, n))).collect::<Vec<_>>();
        let holding = |p: P| nodes.iter().rev().find(|(_, n)| wce_siege::raster::inside(p, &n.bbox)).map(|(e, _)| *e);
        let (a, b) = (holding(first)?, holding(last)?);
        (a != b).then_some((a, b))
    }

    /// A street edge's two Street boundaries made again (its Inspector's *Fit the street boundaries*), as the generator makes them
    /// (`wce_siege::generate::street_sides`): along its Street between the two nodes it joins (`street_nodes`), at the corridor's faces
    /// (the no-go regions, buildings, fort pieces, steep ground, sea and the other nodes' Boundaries), each end ticked into the node's
    /// Boundary. They replace the ones it has, after its Street (one undo entry; the edge stays selected).
    ///
    /// # Errors
    ///
    /// Not a street edge, one that joins no two nodes, or no corridor between them.
    pub fn fit_street_sides(&mut self, edge: usize) -> Result<(), String> {
        let t = self.tile();
        let sc = t.tscene();
        if edge >= sc.doc().elements().len() || sc.type_of(edge) != "SiegeAIEdgeInstance" {
            return Err("not a siege AI edge".to_owned());
        }

        if sc.doc().get(edge, "type").is_some_and(|ty| ty != "SAET_STREET") {
            return Err("only a street edge has boundaries (an area connection, a wall or a palisade edge has none)".to_owned());
        }

        let (a, b) = self.street_nodes(edge).ok_or("the street joins no two nodes: its Street's ends must be in two nodes' Bounding Boxes")?;
        let line = polys(sc, edge).into_iter().find(|p| p.0 == "Street").map(|p| p.1).unwrap_or_default();
        let (na, nb) = (node_of(sc, a).ok_or("a node without a Bounding Box")?, node_of(sc, b).ok_or("a node without a Bounding Box")?);
        let others = sc.find_all("SiegeAINodeInstance").into_iter().filter(|&e| e != a && e != b && exported(sc, e)).filter_map(|e| node_of(sc, e)).collect::<Vec<_>>();
        let sides = wce_siege::generate::street_sides(&t.siege_input(), &line, &na, &nb, &others).ok_or("no corridor between the two nodes")?;

        // The new sides, local to the edge (each at the centre of its points, as a drawn one).
        let u = t.unit();
        let texts = sides.iter().map(|side| {
            let local = side.iter().map(|p| {
                let (x, z) = ((p[0] / u) as f32, (p[1] / u) as f32);
                t.to_local(edge, wce_foundation::math::Float3::new(x, t.terrain_height(x, z), z))
            }).collect::<Vec<_>>();
            let c = local.iter().fold(Double3::default(), |s, &p| s + p) * (1.0 / local.len().max(1) as f64);
            let n = super::edits::new_polygon("SIEGE_AI_POLYGON_INSTANCE", local.into_iter().map(|p| p - c).collect(), false).position(c).attr("name", "Street boundary");
            wce_tile::new_instances::element_text(&n)
        }).collect::<Result<Vec<_>, _>>()?;
        let old = sc.children(edge).into_iter().filter(|&c| sc.type_of(c) == "SIEGE_AI_POLYGON_INSTANCE" && sc.name(c) == "Street boundary").collect::<Vec<_>>();

        let mark = self.begin()?;
        let result = (|| -> Result<(), String> {
            for &c in old.iter().rev() {
                self.tscene_mut().remove(c)?;
            }

            for text in &texts {
                self.tscene_mut().insert_element_text(edge, text)?;
            }

            Ok(())
        })();

        match result {
            Ok(()) => {
                self.push_splice("Fit the street boundaries".to_owned(), mark, vec![item_of(edge)], vec![item_of(edge)]);
                Ok(())
            }

            Err(e) => {
                let record = self.tscene_mut().end_edit(mark).unwrap_or_default();
                let _ = self.tscene_mut().unapply(&record);
                Err(e)
            }
        }
    }

    /// A siege AI node's Boundary made again for the ground around it as it is (its Inspector's *Adjust the Boundary*): the free ground
    /// nearest to it, within `reach` metres of its centre (`None`: the generator's reach, from the room around it), out to the no-go
    /// regions, buildings, fort pieces, steep ground and sea around it, the other nodes' Boundaries, and halfway to the nodes without one
    /// (`wce_siege::generate::node_boundary`, as the generator makes them), grown onto the faces of the buildings and fort pieces. It
    /// replaces the Boundary it has, in its place (one undo entry); returns the new one.
    ///
    /// # Errors
    ///
    /// Not a siege AI node, a node without a Bounding Box of 4 points, or no free ground around it.
    pub fn adjust_node_boundary(&mut self, node: usize, reach: Option<f32>) -> Result<usize, String> {
        let t = self.tile();
        let sc = t.tscene();
        if node >= sc.doc().elements().len() || sc.type_of(node) != "SiegeAINodeInstance" {
            return Err("not a siege AI node".to_owned());
        }

        let me = node_of(sc, node).ok_or("the node has no Bounding Box of 4 points")?;
        let others = sc.find_all("SiegeAINodeInstance").into_iter().filter(|&e| e != node && exported(sc, e)).filter_map(|e| node_of(sc, e)).collect::<Vec<_>>();
        let reach = reach.filter(|&r| r > 0.0).map(|r| f64::from(r) * t.unit());
        let pts = wce_siege::generate::node_boundary(&t.siege_input(), &me, &others, reach);
        if pts.len() < 3 {
            return Err("no free ground around the node".to_owned());
        }

        let u = t.unit();
        let world = pts.iter().map(|p| {
            let (x, z) = ((p[0] / u) as f32, (p[1] / u) as f32);
            wce_foundation::math::Float3::new(x, t.terrain_height(x, z), z)
        }).collect();
        let attrs = BTreeMap::from([("name".to_owned(), "Boundary".to_owned())]);
        self.create_with("SIEGE_AI_POLYGON_INSTANCE", node, &super::Create::Line(world), "", &attrs)
    }

    /// Generate SiegeAI: the layout into its layer, the old one out of the export. One undo entry; returns the layout (its summary and
    /// warnings).
    ///
    /// # Errors
    ///
    /// A tile without a scene graph, or an edit the text does not allow.
    pub fn generate_siege_ai(&mut self) -> Result<Layout, String> {
        // The kit walls laid now where their previews have not been yet (a republish clears them until they are made again).
        self.tile_mut().ensure_kit_layouts();
        let input = self.tile().siege_input();
        let layout = wce_siege::generate::generate(&input);
        if layout.perimeter.len() < 3 {
            return Err(layout.warnings.join("; "));
        }

        let mut layout = layout;
        let report = wce_siege::check::check(&layout);
        layout.warnings.extend(report.problems.iter().map(|p| format!("check: {p}")));
        layout.warnings.extend(wce_siege::check::check_captures(&layout, &input.captures));
        layout.warnings.extend(wce_siege::check::check_steep(&layout, &input.steep).into_iter().map(|w| format!("check: {w}")));

        let root = self.tile().tscene().scene_graph().ok_or("this tile has no SCENE_GRAPH")?;
        let mark = self.begin()?;
        let result = self.write_layout(root, &layout);
        match result {
            Ok(layer) => {
                self.push_splice("Generate SiegeAI".to_owned(), mark, vec![], vec![item_of(layer)]);
                Ok(layout)
            }
            Err(e) => {
                let record = self.tscene_mut().end_edit(mark).unwrap_or_default();
                let _ = self.tscene_mut().unapply(&record);
                Err(e)
            }
        }
    }

    fn write_layout(&mut self, root: usize, layout: &Layout) -> Result<usize, String> {
        // The old layout out of the export (what the previous generation made is replaced).
        if let Some(l) = top_layer(self.tile().tscene(), AUTOGEN_LAYER) {
            self.tscene_mut().remove(l)?;
        }

        self.retire_old_layout(root)?;

        let u = self.tile().unit();
        let tile = self.tile();
        let height = |p: P| tile.project.height_at(p[0] / u, p[1] / u);
        let pts3 = |pts: &[P], origin: P| -> Vec<Double3> { pts.iter().map(|&p| Double3::new(p[0] - origin[0], height(p), p[1] - origin[1])).collect() };
        let s = |k: &str, v: &str| (k.to_owned(), v.to_owned());

        // Prepared first (the document is borrowed while writing).
        let nodes: Vec<_> = layout.nodes.iter().map(|n| (n.kind.ted(), n.centre, pts3(&n.bbox, n.centre), pts3(&n.boundary, n.centre))).collect();
        let edges: Vec<_> = layout.edges.iter().map(|e| (e.kind, pts3(&e.line, [0.0, 0.0]), e.sides.iter().map(|s| pts3(s, [0.0, 0.0])).collect::<Vec<_>>())).collect();
        let perimeter = pts3(&layout.perimeter, [0.0, 0.0]);
        let approaches: Vec<_> = layout.approaches.iter().map(|a| pts3(a, [0.0, 0.0])).collect();
        let separators: Vec<_> = layout.separators.iter().map(|l| pts3(l, [0.0, 0.0])).collect();
        let wall_zones: Vec<_> = layout.wall_zones.iter().map(|z| pts3(z, [0.0, 0.0])).collect();
        let mut coast: Vec<(&str, Vec<Double3>)> = layout.deployment_lines.iter().map(|l| ("AIH_DEFAULT_DEPLOYMENT_LINE", pts3(l, [0.0, 0.0]))).collect();
        coast.extend(layout.beaches.iter().map(|l| ("AIH_BEACH_LANDING", pts3(l, [0.0, 0.0]))));
        coast.extend(layout.beach_approaches.iter().map(|l| ("AIH_BEACH_LANDING_FORT_APPROACH", pts3(l, [0.0, 0.0]))));
        let zones: Vec<_> = [(1, &layout.defender, "argb(0.0,0.0,1.0,0.7)"), (0, &layout.attacker, "argb(1.0,0.0,0.0,0.7)")]
            .into_iter()
            .map(|(alliance, d, colour)| (alliance, colour, d.polygons.iter().map(|(k, p)| (k.ted(), pts3(p, [0.0, 0.0]))).collect::<Vec<_>>()))
            .collect();

        let sc = self.tscene_mut();
        let top = sc.add_layer(Some(root), AUTOGEN_LAYER)?;
        let polygon = |sc: &mut TedScene, parent: usize, kind: &str, name: &str, pts: &[Double3], extra: &[(&str, &str)]| -> Result<usize, String> {
            let extra: BTreeMap<String, String> = extra.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
            sc.add_polygon(Some(parent), kind, name, pts, &extra)
        };

        let streets = sc.add_layer(Some(top), "Streets")?;
        for (ty, c, bbox, boundary) in &nodes {
            let attrs = [
                s("flags", " OIF_VISIBLE|OIF_ACTIVE|OIF_DRAGGABLE"), s("lf_offset", "0"), s("position", &format!("vector({},0,{})", c[0], c[1])),
                s("add_boundary", "0"), s("type", ty), s("name", "Siege AI Node"), s("surface_mode", "MSM_FREEFORM"),
            ];
            let n = sc.add_container_instance(Some(streets), "SiegeAINodeInstance", &attrs)?;
            polygon(sc, n, "SIEGE_AI_POLYGON_INSTANCE", "Bounding Box", bbox, &[("polygon_closed", "1")])?;
            polygon(sc, n, "SIEGE_AI_POLYGON_INSTANCE", "Boundary", boundary, &[("polygon_closed", "1")])?;
        }

        let edge_attrs = |ty: &str| [
            s("flags", " OIF_VISIBLE|OIF_ACTIVE|OIF_DRAGGABLE"), s("lf_offset", "0"), s("add_boundary", "0"), s("type", ty), s("name", "Siege AI Edge"),
            s("surface_mode", "MSM_FREEFORM"),
        ];
        let on_walls = |k: wce_siege::EdgeKind| matches!(k, wce_siege::EdgeKind::Wall | wce_siege::EdgeKind::Palisade);
        for (kind, line, sides) in edges.iter().filter(|e| !on_walls(e.0)) {
            let e = sc.add_container_instance(Some(streets), "SiegeAIEdgeInstance", &edge_attrs(kind.ted()))?;
            polygon(sc, e, "SIEGE_AI_POLYGON_INSTANCE", "Street", line, &[])?;
            for side in sides {
                polygon(sc, e, "SIEGE_AI_POLYGON_INSTANCE", "Street boundary", side, &[])?;
            }
        }

        let walls = sc.add_layer(Some(top), "Walls")?;
        for (kind, line, _) in edges.iter().filter(|e| on_walls(e.0)) {
            let e = sc.add_container_instance(Some(walls), "SiegeAIEdgeInstance", &edge_attrs(kind.ted()))?;
            polygon(sc, e, "SIEGE_AI_POLYGON_INSTANCE", "Street", line, &[])?;
        }

        // The walls' battlefield zones (TEd's zone defaults: EFP_LOW_WALL).
        if !wall_zones.is_empty() {
            let z = sc.add_layer(Some(top), "Wall_Zones")?;
            for pts in &wall_zones {
                polygon(sc, z, "ZONE_POLYGON_INSTANCE", "Zone", pts, &[("polygon_closed", "1")])?;
            }
        }

        let fort = sc.add_layer(Some(top), "Fort")?;
        polygon(sc, fort, "AI_HINT_POLYGON_INSTANCE", "AI hint polygon", &perimeter, &[("hint_type", "AIH_FORT_PERIMETER"), ("polygon_closed", "1")])?;
        for a in &approaches {
            polygon(sc, fort, "AI_HINT_POLYGON_INSTANCE", "AI hint polygon", a, &[("hint_type", "AIH_FORT_APPROACH")])?;
        }

        for l in &separators {
            polygon(sc, fort, "AISeparatorInstance", "AI Separator", l, &[
                ("type", "PALT_FORT"), ("polygon_closed", "0"), ("subdivision_length", "5"), ("max_subdivisions", "100"),
            ])?;
        }

        if !coast.is_empty() {
            let c = sc.add_layer(Some(top), "Coast")?;
            for (hint, l) in &coast {
                polygon(sc, c, "AI_HINT_POLYGON_INSTANCE", "AI hint polygon", l, &[("hint_type", hint)])?;
            }
        }

        let deploy = sc.add_layer(Some(top), "Deployment_Zones")?;
        for (alliance, colour, polys) in &zones {
            let attrs = [
                s("flags", " OIF_VISIBLE|OIF_ACTIVE|OIF_DRAGGABLE"), s("deployment_category", "DAC_DEFAULT"), s("lf_offset", "0"), s("deployment_type", "EDZT_1V1"),
                s("new_polygon_type", "PDM_POLYGON"), s("facing_direction", "0"), s("deployment_zone_id", "0"), s("name", "Deployment Zone"), s("add_polygon", "0"),
                s("alliance_id", &alliance.to_string()), s("surface_mode", "MSM_FREEFORM"),
            ];
            let g = sc.add_container_instance(Some(deploy), "DeploymentZonePolygonInstanceGroup", &attrs)?;
            for (ty, pts) in polys {
                polygon(sc, g, "DEPLOYMENT_ZONE_POLYGON_INSTANCE", "Deployment Zone Polygon", pts, &[
                    ("line_colour", colour), ("subdivision_length", "5"), ("max_subdivisions", "100"), ("deployment_polygon_type", ty),
                ])?;
            }
        }

        Ok(top)
    }

    /// The exported siege layout the tile had (outside the generated layer) goes out of the export: its layers when they hold nothing
    /// else, else the items into [`OLD_LAYER`].
    fn retire_old_layout(&mut self, root: usize) -> Result<(), String> {
        let sc = self.tile().tscene();
        let els = sc.doc().elements();
        let mut items = vec![];
        // Per layer: (layout items in it, whether it holds anything else).
        let mut layers: BTreeMap<usize, (Vec<usize>, bool)> = BTreeMap::new();
        for e in 0..els.len() {
            let kind = els[e].name.as_str();
            if kind == "point" || is_container(kind) || sc.doc().get(e, "position").is_none() || !sc.is_descendant(e, root) {
                continue;
            }

            // Inside a layout item: part of it.
            let mut p = els[e].parent;
            let mut inside_item = false;
            let mut layer = root;
            while let Some(q) = p {
                if layout_item(sc, q) {
                    inside_item = true;
                }

                if layer == root && els[q].name == "LayerInstance" {
                    layer = q;
                }

                p = els[q].parent;
            }

            if inside_item || !exported(sc, e) {
                continue;
            }

            let entry = layers.entry(layer).or_default();
            if layout_item(sc, e) {
                entry.0.push(e);
                items.push(e);
            } else {
                entry.1 = true;
            }
        }

        let mut to_move = vec![];
        for (layer, (its, other)) in &layers {
            if its.is_empty() {
                continue;
            }

            if *other || *layer == root {
                to_move.extend(its.iter().copied());
            } else {
                self.tscene_mut().set_attr(*layer, "export_type", OFF);
            }
        }

        if to_move.is_empty() {
            return Ok(());
        }

        let old = match top_layer(self.tile().tscene(), OLD_LAYER) {
            Some(l) => l,
            None => {
                let l = self.tscene_mut().add_layer(Some(root), OLD_LAYER)?;
                self.tscene_mut().set_attr(l, "export_type", OFF);
                l
            }
        };

        let worlds: Vec<_> = to_move.iter().map(|&e| world_transform(self.tile().tscene(), e)).collect();
        let moved = self.tscene_mut().move_elements(to_move, old, None)?;
        self.keep_worlds(&moved, &worlds);
        Ok(())
    }
}

/// A polygon (closed) or a line clipped to the rectangle [0, w] x [0, h] (x, z): the closed one by Sutherland-Hodgman, the line segment by
/// segment (the pieces' points in order). Empty when nothing of it is inside.
fn clip_to_rect(pts: &[wce_foundation::math::Float3], closed: bool, w: f32, h: f32) -> Vec<wce_foundation::math::Float3> {
    use wce_foundation::math::Float3;
    let lerp = |a: Float3, b: Float3, t: f32| Float3::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, a.z + (b.z - a.z) * t);
    if closed {
        let mut poly = pts.to_vec();
        // The four sides: inside when the value is >= 0.
        let sides: [&dyn Fn(Float3) -> f32; 4] = [&|p| p.x, &|p| w - p.x, &|p| p.z, &|p| h - p.z];
        for side in sides {
            if poly.is_empty() {
                break;
            }

            let mut out = vec![];
            for i in 0..poly.len() {
                let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
                let (da, db) = (side(a), side(b));
                if da >= 0.0 {
                    out.push(a);
                }

                if (da >= 0.0) != (db >= 0.0) {
                    out.push(lerp(a, b, da / (da - db)));
                }
            }

            poly = out;
        }

        return poly;
    }

    // A line: each segment cut to the rectangle (Liang-Barsky).
    let mut out: Vec<Float3> = vec![];
    for s in pts.windows(2) {
        let (a, b) = (s[0], s[1]);
        let (dx, dz) = (b.x - a.x, b.z - a.z);
        let (mut t0, mut t1) = (0.0f32, 1.0f32);
        let mut keep = true;
        for (p, q) in [(-dx, a.x), (dx, w - a.x), (-dz, a.z), (dz, h - a.z)] {
            if p == 0.0 {
                if q < 0.0 {
                    keep = false;
                }
            } else {
                let r = q / p;
                if p < 0.0 { t0 = t0.max(r) } else { t1 = t1.min(r) }
            }
        }

        if keep && t0 <= t1 {
            out.push(lerp(a, b, t0));
            out.push(lerp(a, b, t1));
        }
    }

    out
}


/// A point of a wall's line for Siege AI (`siege_input_with`'s `band`).
struct Sample {
    centre: [f64; 2],
    foot: [f64; 2],
    dir: [f64; 2],
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use wce_editor_core::document::Document;
    use wce_foundation::math::Double3;
    use wce_scene::render_scene::RenderScene;
    use wce_tile::tile_project::TileProject;

    use super::LogicSummary;
    use crate::tile_document::TileDocument;

    const TED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project version="16">
    <scene>
        <SCENE_GRAPH name="root" position="vector(0,0,0)" orientation="euler(0,0,0)">
            <children>
                <LayerInstance flags=" OIF_VISIBLE|OIF_ACTIVE" export_type="LayerExportType_Normal" lf_offset="0" position="vector(0,0,0)" name="B" orientation="euler(0,0,0)">
                    <children>
                        <BUILDING_INSTANCE flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(10,0,10)" name="a" surface_mode="MSM_FREEFORM" orientation="euler(0,0,0)" key="house"/>
                        <POINT_LIGHT_INSTANCE flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(20,0,20)" name="l" surface_mode="MSM_FREEFORM" orientation="euler(0,0,0)"/>
                    </children>
                </LayerInstance>
                <LayerInstance flags=" OIF_VISIBLE|OIF_ACTIVE" export_type="LayerExportType_Normal" lf_offset="0" position="vector(0,0,0)" name="C" orientation="euler(0,0,0)">
                    <children>
                        <BUILDING_INSTANCE flags=" OIF_VISIBLE|OIF_ACTIVE" lf_offset="0" position="vector(30,0,30)" name="b" surface_mode="MSM_FREEFORM" orientation="euler(0,0,0)" key="house"/>
                    </children>
                </LayerInstance>
            </children>
        </SCENE_GRAPH>
    </scene>
</project>
"#;

    fn open(dir: &std::path::Path) -> TileDocument {
        std::fs::write(dir.join("tile.ted"), TED).unwrap();
        let (w, h) = (64u32, 64u32);
        let pixels = (0..h).flat_map(|y| (0..w).flat_map(move |x| ((x * 300 + y * 200) as u16).to_be_bytes())).collect();
        let png = wce_formats::png_file::PngFile { width: w, height: h, bit_depth: 16, color_type: 0, pixels, pixels_edited: true, ..Default::default() };
        std::fs::write(dir.join("height_map_0.png"), png.write().unwrap()).unwrap();
        let d = TileDocument::new(TileProject::open(dir).unwrap(), None);
        let logic = LogicSummary { hard: vec![vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]], ..LogicSummary::default() };
        d.tile().logic_summaries.borrow_mut().insert("house".to_owned(), Some(Arc::new(logic)));
        d
    }

    /// What the minimap has read against what a read without the kept buildings gives.
    fn check(d: &TileDocument, what: &str) {
        let (kept, read) = (d.tile().minimap_siege().input, d.tile().siege_input_with(true));
        assert_eq!(format!("{:?}", kept.obstacles), format!("{:?}", read.obstacles), "{what}");
    }

    #[test]
    fn what_is_kept_of_the_buildings_follows_every_edit_and_only_what_feeds_the_overlays_reads_again() {
        let dir = tempfile::tempdir().unwrap();
        let mut d = open(dir.path());
        let mut scene = RenderScene::default();
        let layer = scene.add_layer(d.id(), "tile");
        d.publish(&mut scene, layer);
        let follow = |d: &mut TileDocument, scene: &mut RenderScene| {
            let dirty = d.history_mut().take_dirty();
            d.republish(scene, layer, &dirty);
        };

        check(&d, "first read");
        assert_eq!(d.tile().siege_buildings.borrow().len(), 2);
        let sc = d.tile().tscene();
        let (a, light, later) = (sc.find_all("BUILDING_INSTANCE")[0], sc.find_all("POINT_LIGHT_INSTANCE")[0], sc.find_all("LayerInstance")[1]);

        let version = d.tile().siege_version();
        assert!(d.set_local(light, Some(Double3::new(25.0, 0.0, 25.0)), None, 0));
        follow(&mut d, &mut scene);
        assert!(d.rename(light, "lamp"));
        follow(&mut d, &mut scene);
        assert_eq!(d.tile().siege_version(), version, "a light feeds nothing");

        assert!(d.set_local(a, Some(Double3::new(12.0, 0.0, 10.0)), None, 0));
        follow(&mut d, &mut scene);
        assert!(d.tile().siege_version() != version, "a building does");
        assert_eq!(d.tile().siege_buildings.borrow().len(), 1, "only it goes");
        check(&d, "a building moved");

        assert!(d.set_local(later, Some(Double3::new(5.0, 0.0, 0.0)), None, 0));
        follow(&mut d, &mut scene);
        check(&d, "a layer moved");

        // Added before the last building: its number changes, what is kept of it goes with it.
        let first = d.tile().tscene().find_all("LayerInstance")[0];
        let items = d.copy_items(&[a]);
        d.paste(&items, first, wce_foundation::math::Float3::new(3.0, 0.0, 3.0)).unwrap();
        follow(&mut d, &mut scene);
        check(&d, "a building added");
        d.history_mut().undo();
        follow(&mut d, &mut scene);
        check(&d, "its undo");
        assert!(d.remove(a));
        follow(&mut d, &mut scene);
        check(&d, "a building removed");
    }
}
