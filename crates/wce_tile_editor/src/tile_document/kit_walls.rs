//! Walls of kit pieces in the document (docs/plans/kit_walls.md): a `WceWallSpline` with *Procedural* off is laid with its culture's own
//! fort pieces (`wce_walls::kit`) along the same centre line a procedural wall follows (its points, handles and sharp marks), and drawn as
//! those pieces, whole, through the procedural walls' preview (`walls.rs`: the technology overlay and Check platforms read them the same
//! way). Phase 2: on flat ground (the level under the line's first point), free ends capped. Phase 3: an end on a fort building's end (as
//! a procedural wall's attaches, `scene::ATTACH`) meets that building's joint, by the kit's pieces or by a procedural stretch made as the
//! procedural walls' pieces are (`spawn_wall_job`).
//!
//! Scene units throughout, as `walls.rs`.

use std::collections::HashMap;
use std::sync::Arc;

use wce_foundation::log;
use wce_scene::render_scene::RenderScene;
use wce_foundation::math::Double3;
use wce_tile::terrain_layers::{Stack, TerrainLayers};
use wce_walls::kit::{Catalogue, EndJoint, KitLayout, LayOptions, catalogue, culture_keys, lay_on};
use wce_walls::pieces::Piece;
use wce_tile::new_instances::WALL_ID;
use wce_walls::solver::Plan;

use super::Tile;

impl Tile {
    /// A culture's pieces (`wce_walls::kit::catalogue`), read once; `None` without the game's files or when they have none of its pieces.
    pub(crate) fn kit_catalogue(&mut self, base: &str) -> Option<Arc<Catalogue>> {
        if let Some(c) = self.wall_preview.kits.get(base) {
            return c.clone();
        }

        let files = self.files.clone()?;
        let all = match &self.wall_preview.building_keys {
            Some(k) => k.clone(),
            None => {
                let k = files.list("rigidmodels/buildings/", "_tech.cs2.parsed").into_iter().filter_map(|p| p.rsplit('/').next()?.strip_suffix("_tech.cs2.parsed").map(str::to_owned)).collect::<Vec<_>>();
                let k = Arc::new(k);
                self.wall_preview.building_keys = Some(k.clone());
                k
            }
        };

        let keys = culture_keys(base, all.iter().map(String::as_str));
        let read = |k: &str| files.read(&format!("{}{k}_tech.cs2.parsed", wce_walls::building::folder(k))).ok();
        let c = catalogue(base, &keys, &read);
        let c = if c.pieces.is_empty() {
            log::warn(format!("kit wall: the game's files have no pieces of {base}"));
            None
        } else {
            Some(Arc::new(c))
        };

        self.wall_preview.kits.insert(base.to_owned(), c.clone());
        c
    }

    /// The joint of the fort building an end of a wall at `p` (x, z) is attached to (`wall_anchors`, within `scene::ATTACH`): of its ends
    /// (through its instance's transform) the nearest to the anchor.
    fn kit_end_joint(&self, anchors: &[wce_walls::scene::Anchor], p: [f64; 2]) -> Option<(EndJoint, String)> {
        let d = |a: &wce_walls::scene::Anchor| (a.at[0] - p[0]).hypot(a.at[2] - p[1]);
        let a = anchors.iter().filter(|a| d(a) <= wce_walls::scene::ATTACH).min_by(|a, b| d(a).total_cmp(&d(b)))?;
        let w = wce_tile::world_transform::world_transform(self.tscene(), a.elem);
        let flat = |v: [f32; 2]| {
            let r = w.rotation * Double3::new(f64::from(v[0]), 0.0, f64::from(v[1]));
            let l = r.x.hypot(r.z);
            if l > 1e-9 { [r.x / l, r.z / l] } else { [0.0, 0.0] }
        };

        // Its joints as the kit's pieces' are read (`kit::joints`: its end caps, else its walk's ends: the 8 m cultures' gates have no caps),
        // else `piece_ends` (its frame's ends).
        // Its platforms' edges on the joint (`kit::joint_edges`): which pieces join it.
        let logic = self.files.as_deref().and_then(|f| f.read(&format!("{}{}_tech.cs2.parsed", wce_walls::building::folder(&a.key), a.key)).ok());
        let ends = logic.as_deref().map(wce_walls::kit::joints).filter(|e| !e.is_empty()).unwrap_or_else(|| self.piece_ends(&a.key));
        let world = |e: &wce_walls::logic::PieceEnd| w.position + w.rotation * Double3::new(f64::from(e.at[0]), f64::from(e.at[1]), f64::from(e.at[2]));
        let e = ends.into_iter().min_by(|x, y| {
            let d = |e: &wce_walls::logic::PieceEnd| {
                let p = world(e);
                (p.x - a.at[0]).hypot(p.z - a.at[2])
            };
            d(x).total_cmp(&d(y))
        })?;
        let at = world(&e);
        let edges = logic.as_deref().map(|b| wce_walls::kit::joint_edges(b, &[e]).remove(0)).unwrap_or_default();
        let joint = EndJoint { at: [at.x, at.z], out: flat(e.out), side: flat(e.side), walk: at.y, level: w.position.y, edges };
        Some((joint, a.key.clone()))
    }

    /// A wall of kit pieces laid (`wce_walls::kit::lay_on`): its centre line (the procedural walls' `solver::centre_line_marked`, reversed
    /// when flipped, so that its outer face is on the same side), its culture (`wall_base`), the defence and access spacings, the joints of
    /// the buildings its ends are attached to; over the ground without its own height layer (`ground_below`), from the level under its first
    /// point, stepping where it rises or drops; free ends capped. `None` for an element that is not a
    /// kit wall, a wall of fewer than 2 points, or a culture without pieces.
    pub fn kit_layout(&mut self, elem: usize) -> Option<KitLayout> {
        let sc = self.tscene();
        if !self.is_wall(elem) || wce_walls::scene::is_procedural(sc, elem) {
            return None;
        }

        let profile = wce_walls::scene::wall_profile(sc, elem)?;
        let (points, sharp) = wce_walls::scene::wall_points(sc, elem);
        if points.len() < 2 {
            return None;
        }

        let handles = wce_walls::scene::wall_handles(sc, elem);
        let flip = sc.doc().get(elem, "wall_flip").is_some_and(|v| wce_tile::properties::parse_bool(&v));
        let every = |attr: &str, default: f64| sc.doc().get(elem, attr).and_then(|t| t.trim().parse::<f64>().ok()).unwrap_or(default).max(0.0);
        let (defence_every, access_every) = (every("wall_defence_every", 0.0), every("wall_access_every", wce_walls::pieces::DOOR_EVERY));
        let anchors = self.wall_anchors();
        let mut ends = [self.kit_end_joint(&anchors, points[0]), self.kit_end_joint(&anchors, points[points.len() - 1])];
        let (line, _) = wce_walls::solver::centre_line_marked(&points, &sharp, &handles, profile.fold_radius());
        let mut line = line.into_iter().map(|(p, _)| p).collect::<Vec<_>>();
        if flip {
            line.reverse();
            ends.swap(0, 1);
        }

        // On its building's joint, wherever its point was left within reach.
        let last = line.len() - 1;
        for (k, i) in [(0, 0), (1, last)] {
            if let Some((j, _)) = &ends[k] {
                line[i] = j.at;
            }
        }

        // Over the ground without its own height layer (rule 1): the level under its first point, steps where it rises or drops.
        let cat = self.kit_catalogue(profile.key)?;
        let key = self.wall_layer_key(elem);
        let ground = self.ground_below(key.as_deref());
        let level = ground(line[0][0], line[0][1]);
        let options = LayOptions { level, defence_every, access_every, caps: [true, true], ends: [ends[0].as_ref().map(|e| e.0.clone()), ends[1].as_ref().map(|e| e.0.clone())] };
        let mut laid = lay_on(&line, &cat, options, Some(&ground));
        for (k, e) in ends.iter().enumerate() {
            if let Some((_, key)) = e {
                for n in laid.notes.iter_mut().filter(|n| n.starts_with(if k == 0 { "the first end" } else { "the last end" })) {
                    *n = format!("{n} ({key})");
                }
            }
        }

        Some(laid)
    }

    /// A kit wall's procedural stretches laid out (`solver::solve` on each `Stretch::input`, the wall's base and tolerance).
    pub(super) fn kit_stretch_plans(&self, elem: usize, layout: &KitLayout) -> Vec<Plan> {
        let sc = self.tscene();
        let Some(profile) = wce_walls::scene::wall_profile(sc, elem) else { return vec![] };
        let tolerance = sc.doc().get(elem, "wall_tolerance").and_then(|t| t.trim().parse::<f64>().ok()).unwrap_or(0.5).max(0.0);
        let key = self.wall_layer_key(elem);
        let ground = self.ground_below(key.as_deref());
        layout.stretches.iter().map(|st| wce_walls::solver::solve(&st.input(tolerance), profile, &ground)).collect()
    }

    /// A kit wall's pieces on the scene in place of its old ones: the game's own pieces, whole, where its layout puts them. A hidden wall
    /// (or one that can't be laid) has none.
    pub(super) fn make_kit_wall_pieces(&mut self, scene: &mut RenderScene, elem: usize) {
        let before = self.wall_preview.shown_as.remove(&elem);
        let ids = self.wall_preview.entities.remove(&elem).unwrap_or_default();
        let Some(layout) = self.kit_layout(elem).filter(|_| self.shown(elem)) else {
            for id in ids {
                scene.remove(id);
            }

            self.sweep_wall_ground();

            self.drop_unused_wall_pieces();
            return;
        };

        for n in &layout.notes {
            log::warn(format!("kit wall {}: {n}", self.tscene().name(elem)));
        }


        // Its height layer: the ground to its pieces' levels (made again only when it or the ground below it changed); the other walls
        // stand on what it changed.
        if self.kit_wall_ground(elem, &layout) {
            self.walls_changed();
        }

        let placed = layout.placed.iter().map(|p| (p.key.clone(), Piece { s: (0.0, 0.0), origin: p.origin, yaw: p.yaw, scale: 1.0, base: None, rigid: true })).collect::<Vec<_>>();
        let plans = self.kit_stretch_plans(elem, &layout);
        let profile = wce_walls::scene::wall_profile(self.tscene(), elem);

        // Kept by what was laid (a kit wall's own signature, never a procedural one's): its pieces, and its stretches as a procedural
        // wall's are (their layout and the ground under them).
        let signature = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            "kit wall".hash(&mut h);
            for (key, p) in &placed {
                key.hash(&mut h);
                p.origin.map(f64::to_bits).hash(&mut h);
                p.yaw.to_bits().hash(&mut h);
            }

            if let Some(profile) = profile {
                for plan in &plans {
                    let l = wce_walls::scene::WallLayout { plan: plan.clone(), profile, ends: [None, None] };
                    self.wall_signature(&l, &[]).hash(&mut h);
                }
            }

            h.finish()
        };

        self.wall_preview.kit_layouts.insert(elem, layout);

        // As it was: its pieces stay where they are.
        if Some(signature) == before && !ids.is_empty() {
            self.wall_preview.entities.insert(elem, ids);
            self.wall_preview.shown_as.insert(elem, signature);
            return;
        }

        // Made before, or without stretches: on the scene at once.
        let shown = self.show_made_wall_pieces(scene, elem, signature, &ids);
        if shown || plans.is_empty() {
            if !shown {
                for id in ids {
                    scene.remove(id);
                }

                let mut own = HashMap::new();
                for (key, _) in &placed {
                    if !own.contains_key(key) && let Some(b) = self.wall_base(key) {
                        own.insert(key.clone(), b);
                    }
                }

                self.show_wall_pieces(scene, elem, signature, vec![], &placed, &own);
            }

            if Some(signature) != before {
                self.tech_wall_changed(elem);
            }

            return;
        }

        // The stretches' pieces off the editor's thread (the old pieces stay until they are made).
        self.wall_preview.entities.insert(elem, ids);
        if let Some(b) = before {
            self.wall_preview.shown_as.insert(elem, b);
        }

        let Some(profile) = profile else { return };
        self.spawn_wall_job(elem, signature, profile, plans.into_iter().map(|p| (p, vec![])).collect(), placed);
    }

    /// Every kit wall without a layout kept (its preview not made yet: a republish clears them) laid now and kept, for what reads them
    /// from `&self` (Generate SiegeAI).
    pub(crate) fn ensure_kit_layouts(&mut self) {
        let sc = self.tscene();
        let walls = wce_walls::scene::all_walls(sc).into_iter().filter(|&e| !wce_walls::scene::is_procedural(sc, e) && !self.wall_preview.kit_layouts.contains_key(&e)).collect::<Vec<_>>();
        for e in walls {
            if let Some(l) = self.kit_layout(e) {
                self.wall_preview.kit_layouts.insert(e, l);
            }
        }
    }

    /// A kit wall's last layout (as drawn), for `wall_info`.
    pub fn kit_wall_report(&self, elem: usize) -> Option<&KitLayout> {
        self.wall_preview.kit_layouts.get(&elem)
    }

    /// Per piece of a kit wall's last layout: its key, its levels at its joints, the ground below the wall's layer and as it shows (with
    /// the layer) halfway between its joints (scene y), and its joints (x, z metres), for `wall_levels`.
    pub fn kit_wall_levels(&self, elem: usize) -> Vec<PieceLevels> {
        let Some(l) = self.wall_preview.kit_layouts.get(&elem) else { return vec![] };
        let key = self.wall_layer_key(elem);
        let below = self.ground_below(key.as_deref());
        let u = self.unit();
        l.placed.iter().map(|p| {
            let m = [(p.joints[0][0] + p.joints[1][0]) / 2.0, (p.joints[0][1] + p.joints[1][1]) / 2.0];
            (p.key.clone(), p.levels, below(m[0], m[1]), self.project.height_at(m[0] / u, m[1] / u), p.joints.map(|j| [j[0] / u, j[1] / u]))
        }).collect()
    }

    /// An id for a new wall (`WALL_ID`): after the highest.
    pub(super) fn next_wall_id(&self) -> u32 {
        let sc = self.tscene();
        wce_walls::scene::all_walls(sc).into_iter().filter_map(|e| sc.doc().get(e, WALL_ID)?.trim().parse::<u32>().ok()).max().unwrap_or(0) + 1
    }

    /// What a kit wall's height layer is known by (`wce_walls::scene::wall_key`).
    pub(super) fn wall_layer_key(&self, elem: usize) -> Option<String> {
        wce_walls::scene::wall_key(self.tscene(), elem)
    }

    /// The kit walls as an export lowers them (`wce_walls::export::KitWall`, phase 5 of docs/plans/kit_walls.md): each laid as the editor
    /// lays it (on the ground below its own height layer), its game pieces and its stretches' plans. A wall that can't be laid is not in it
    /// (the export leaves it out, noted).
    pub fn kit_walls_for_export(&mut self) -> Vec<wce_walls::export::KitWall> {
        let walls = wce_walls::scene::all_walls(self.tscene()).into_iter().filter(|&e| !wce_walls::scene::is_procedural(self.tscene(), e)).collect::<Vec<_>>();
        let mut out = vec![];
        for e in walls {
            let (Some(key), Some(layout)) = (self.wall_layer_key(e), self.kit_layout(e)) else { continue };
            let placed = layout.placed.iter().map(|p| (p.key.clone(), Piece { s: (0.0, 0.0), origin: p.origin, yaw: p.yaw, scale: 1.0, base: None, rigid: true })).collect();
            let stretches = self.kit_stretch_plans(e, &layout);
            out.push(wce_walls::export::KitWall { key, placed, stretches });
        }

        out
    }

    /// The height stack's layer of a wall's key, if it has one.
    fn wall_layer_id(&self, key: &str) -> Option<u64> {
        let s = self.project.layers.as_ref()?.height.as_ref()?;
        s.layers.iter().find(|l| parse_owner(&l.owner).is_some_and(|(k, _)| k == key)).map(|l| l.id)
    }

    /// The ground (scene y at x, z, scene units) as it is below a wall's height layer (`key`; none, or no layer: the ground as it shows):
    /// what a kit wall is laid on, so that its layer never moves it (rule 1 of docs/plans/kit_walls.md).
    pub(super) fn ground_below(&self, key: Option<&str>) -> impl Fn(f64, f64) -> f64 + '_ {
        let u = self.unit();
        let p = &self.project;
        let stack = p.layers.as_ref().and_then(|l| l.height.as_ref());
        let id = key.and_then(|k| self.wall_layer_id(k));
        let dims = p.height.as_ref().map(|h| (h.width(), h.height()));
        move |x: f64, z: f64| match (stack, id, dims) {
            (Some(s), Some(id), Some((w, h))) => {
                let (fx, fz) = (p.pixel_x(x / u).clamp(0.0, f64::from(w - 1)), p.pixel_z(z / u).clamp(0.0, f64::from(h - 1)));
                let (x0, y0) = (fx.floor() as u32, fz.floor() as u32);
                let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
                let at = |x: u32, y: u32| {
                    let mut px = [0.0f32];
                    s.below(id, x, y, &mut px);
                    f64::from(px[0])
                };
                let (tx, tz) = (fx - f64::from(x0), fz - f64::from(y0));
                let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * tx;
                let bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * tx;
                top + (bottom - top) * tz
            }
            _ => p.height_at(x / u, z / u),
        }
    }

    /// A kit wall's height layer (phase 4 of docs/plans/kit_walls.md), made again when what it is made from changed (its pieces and their
    /// levels, the ground below it: in its owner, `wall:<key>#<signature>`): under each piece's area (`KitPiece::area`) the ground at its level
    /// (a step's going from the one to the other between its joints), feathered into the ground over [`FEATHER_M`] outside. It goes over the
    /// roads' layers, the walls' in the document's order. Returns whether it changed.
    pub(super) fn kit_wall_ground(&mut self, elem: usize, layout: &KitLayout) -> bool {
        let Some(key) = self.wall_layer_key(elem) else { return false };
        let Some(profile) = wce_walls::scene::wall_profile(self.tscene(), elem) else { return false };
        let Some(cat) = self.kit_catalogue(profile.key) else { return false };
        let u = self.unit();

        // The pads: each piece's footprint in the world, its joints and the levels at them.
        let mut pads: Vec<Pad> = vec![];
        for pl in &layout.placed {
            let Some(piece) = cat.pieces.iter().find(|k| k.key == pl.key) else { continue };
            let (sn, cs) = pl.yaw.sin_cos();
            if piece.area.len() >= 3 {
                let world = piece.area.iter().map(|q| {
                    let (x, z) = (f64::from(q[0]), f64::from(q[1]));
                    [pl.origin[0] + x * cs - z * sn, pl.origin[2] + x * sn + z * cs]
                }).collect();
                pads.push((world, pl.joints, pl.levels));
            }
        }

        let Some(hf) = self.project.height.as_ref() else { return false };
        let (w, h) = (hf.width(), hf.height());
        let p = &self.project;
        let feather = FEATHER_M * u;
        let lo = pads.iter().flat_map(|r| r.0.iter()).fold([f64::MAX; 2], |a, q| [a[0].min(q[0]), a[1].min(q[1])]);
        let hi = pads.iter().flat_map(|r| r.0.iter()).fold([f64::MIN; 2], |a, q| [a[0].max(q[0]), a[1].max(q[1])]);
        let px = |x: f64, z: f64| (p.pixel_x(x / u), p.pixel_z(z / u));
        let rect = if pads.is_empty() {
            [0, 0, 0, 0]
        } else {
            let (a, b) = (px(lo[0] - feather, lo[1] - feather), px(hi[0] + feather, hi[1] + feather));
            let c = |v: f64, n: u32| (v.max(0.0) as u32).min(n);
            [c(a.0.floor(), w), c(a.1.floor(), h), c(b.0.ceil() + 1.0, w), c(b.1.ceil() + 1.0, h)]
        };

        // Each pixel's level and weight: the pad it is in, else the nearest within the feather; of pads alike (overlapping areas), the one
        // whose joints it is nearest the line between.
        let world_of = |x: u32, y: u32| [(f64::from(x) * p.mpp_x() - p.border_m()) * u, (f64::from(y) * p.mpp_z() - p.border_m()) * u];
        let mut wants = vec![];
        for y in rect[1]..rect[3] {
            for x in rect[0]..rect[2] {
                let q = world_of(x, y);
                let best = pads.iter().filter_map(|(ring, joints, levels)| {
                    let d = if inside(q, ring) { 0.0 } else { ring_distance(q, ring) };
                    (d < feather).then(|| {
                        let (a, b) = (joints[0], joints[1]);
                        let ab = [b[0] - a[0], b[1] - a[1]];
                        let len2 = ab[0] * ab[0] + ab[1] * ab[1];
                        let t = if len2 > 1e-9 { (((q[0] - a[0]) * ab[0] + (q[1] - a[1]) * ab[1]) / len2).clamp(0.0, 1.0) } else { 0.0 };
                        let s = 1.0 - d / feather;
                        let off = (q[0] - a[0] - ab[0] * t).hypot(q[1] - a[1] - ab[1] * t);
                        (s * s * (3.0 - 2.0 * s), -off, levels[0] + (levels[1] - levels[0]) * t)
                    })
                }).max_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
                if let Some((weight, _, level)) = best {
                    wants.push((x, y, weight, level));
                }
            }
        }

        // What it is made from: the pads and the ground below (to the millimetre).
        let stack = p.layers.as_ref().and_then(|l| l.height.as_ref());
        let old = self.wall_layer_id(&key);
        let below = |x: u32, y: u32| {
            let mut v = [0.0f32];
            match stack {
                Some(s) => s.below(old.unwrap_or(u64::MAX), x, y, &mut v),
                None => v[0] = hf.metres(x, y) as f32,
            }
            f64::from(v[0])
        };
        let writes = wants.iter().map(|&(x, y, weight, level)| (x, y, ((level - below(x, y)) * weight) as f32)).collect::<Vec<_>>();

        // How far the ground under the pieces is moved (metres): cut where it was higher, filled where lower.
        let (cut, fill) = wants.iter().zip(&writes).filter(|(w, _)| w.2 >= 1.0).fold((0.0f64, 0.0f64), |(c, f), (_, wr)| {
            let d = f64::from(wr.2) / u;
            (c.max(-d), f.max(d))
        });
        let signature = {
            use std::hash::{Hash, Hasher};
            let mut hs = std::collections::hash_map::DefaultHasher::new();
            rect.hash(&mut hs);
            for (x, y, v) in &writes {
                (x, y, (f64::from(*v) * 1000.0).round() as i64).hash(&mut hs);
            }

            hs.finish()
        };
        let owner = format!("{OWNER}{key}#{signature:x}");
        let current = stack.and_then(|s| old.and_then(|id| s.layers.iter().find(|l| l.id == id)).map(|l| l.owner.clone()));
        if writes.is_empty() && old.is_none() || current.as_deref() == Some(owner.as_str()) {
            return false;
        }

        // Where it goes: its own place, else before the first layer of a wall after it in the document (all over the roads').
        let order = self.wall_order();
        let name = format!("Kit wall {}", self.tscene().name(elem));
        let p = &mut self.project;
        let Some(hf) = p.height.as_mut() else { return false };
        let layers = p.layers.get_or_insert_with(TerrainLayers::default);
        let new_id = layers.new_id();
        layers.dirty = true;
        let height = layers.height.get_or_insert_with(|| Stack::new(hf));
        let mut changed = height.layers.iter().find(|l| Some(l.id) == old).and_then(|l| l.extent());
        if writes.is_empty() {
            height.layers.retain(|l| Some(l.id) != old);
        } else {
            let id = match old {
                Some(id) => {
                    height.clear_layer(id);
                    id
                }
                None => {
                    height.add(new_id, &name, "pending");
                    let layer = height.layers.pop().expect("the layer just added");
                    let rank = |o: &str| parse_owner(o).and_then(|(k, _)| order.iter().position(|q| q == k));
                    let mine = order.iter().position(|q| *q == key);
                    let at = height.layers.iter().position(|l| rank(&l.owner).zip(mine).is_some_and(|(r, m)| r > m)).unwrap_or(height.layers.len());
                    height.layers.insert(at, layer);
                    new_id
                }
            };

            for (x, y, v) in &writes {
                height.put(id, *x, *y, &[*v]);
            }

            if let Some(l) = height.layers.iter_mut().find(|l| l.id == id) {
                l.owner = owner;
                l.name = name;
            }

            let r = [rect[0], rect[1], rect[2], rect[3]];
            changed = Some(changed.map_or(r, |c| [c[0].min(r[0]), c[1].min(r[1]), c[2].max(r[2]), c[3].max(r[3])]));
        }

        if let Some(r) = changed {
            height.apply(hf, r);
        }

        height.remember();
        p.height_dirty = true;
        self.layers_shown = true;
        if cut.max(fill) > MOVED_WARN_M {
            log::warn(format!("kit wall {}: the ground under its pieces is cut by up to {cut:.1} m and filled by up to {fill:.1} m (its steps can't follow it closer)", self.tscene().name(elem)));
        }

        true
    }

    /// The kit walls' height layers, in the stack's order: each one's name and owner (`wall_layers`).
    pub fn kit_wall_layers(&self) -> Vec<(String, String)> {
        let Some(s) = self.project.layers.as_ref().and_then(|l| l.height.as_ref()) else { return vec![] };
        s.layers.iter().filter(|l| l.owner.starts_with(OWNER)).map(|l| (l.name.clone(), l.owner.clone())).collect()
    }

    /// The keys of the kit walls' layers, in the document's order.
    fn wall_order(&self) -> Vec<String> {
        wce_walls::scene::all_walls(self.tscene()).into_iter().filter_map(|e| self.wall_layer_key(e)).collect()
    }

    /// The walls' height layers that no shown kit wall owns (a wall gone, hidden, made procedural) taken out.
    pub(super) fn sweep_wall_ground(&mut self) {
        let sc = self.tscene();
        let live = wce_walls::scene::all_walls(sc).into_iter().filter(|&e| !wce_walls::scene::is_procedural(sc, e) && self.shown(e)).filter_map(|e| self.wall_layer_key(e)).collect::<Vec<_>>();
        let p = &mut self.project;
        let (Some(hf), Some(layers)) = (p.height.as_mut(), p.layers.as_mut()) else { return };
        let Some(height) = layers.height.as_mut() else { return };
        let gone = height.layers.iter().filter(|l| parse_owner(&l.owner).is_some_and(|(k, _)| !live.iter().any(|q| q == k))).map(|l| (l.id, l.extent())).collect::<Vec<_>>();
        if gone.is_empty() {
            return;
        }

        height.layers.retain(|l| !gone.iter().any(|g| g.0 == l.id));
        for r in gone.iter().filter_map(|g| g.1) {
            height.apply(hf, r);
        }

        height.remember();
        layers.dirty = true;
        p.height_dirty = true;
        self.layers_shown = true;
        self.walls_changed();
    }
}

/// A kit wall's piece as `wall_levels` reports it: its key, its levels, the ground below the wall's layer and as it shows, its joints.
pub type PieceLevels = (String, [f64; 2], f64, f64, [[f64; 2]; 2]);

/// A piece's ground in a wall's height layer: its area (x, z, the world), its joints and its levels at them.
type Pad = (Vec<[f64; 2]>, [[f64; 2]; 2], [f64; 2]);

/// The owner prefix of the walls' height layers.
pub(super) const OWNER: &str = "wall:";

/// How far the ground under a kit wall's pieces is moved before it says so (metres).
const MOVED_WARN_M: f64 = 5.0;

/// How far a wall's height layer feathers into the ground outside its pieces' footprints (metres).
const FEATHER_M: f64 = 4.0;

/// The wall's key and signature in a layer's owner.
pub(super) fn parse_owner(owner: &str) -> Option<(&str, u64)> {
    let (k, signature) = owner.strip_prefix(OWNER)?.split_once('#')?;
    Some((k, u64::from_str_radix(signature, 16).ok()?))
}

fn inside(p: [f64; 2], ring: &[[f64; 2]]) -> bool {
    let mut c = false;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
            c = !c;
        }
    }

    c
}

fn ring_distance(p: [f64; 2], ring: &[[f64; 2]]) -> f64 {
    (0..ring.len()).map(|i| {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        let ab = [b[0] - a[0], b[1] - a[1]];
        let len2 = ab[0] * ab[0] + ab[1] * ab[1];
        let t = if len2 > 1e-12 { (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / len2).clamp(0.0, 1.0) } else { 0.0 };
        (p[0] - a[0] - ab[0] * t).hypot(p[1] - a[1] - ab[1] * t)
    }).fold(f64::MAX, f64::min)
}
