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
use wce_walls::kit::{Catalogue, EndJoint, KitLayout, LayOptions, catalogue, culture_keys, lay};
use wce_walls::pieces::Piece;
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

    /// A wall of kit pieces laid (`wce_walls::kit::lay`): its centre line (the procedural walls' `solver::centre_line_marked`, reversed when
    /// flipped, so that its outer face is on the same side), its culture (`wall_base`), the defence and access spacings, the joints of the
    /// buildings its ends are attached to; flat at the ground under its first point, free ends capped. `None` for an element that is not a
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

        let u = self.unit();
        let level = self.project.height_at(line[0][0] / u, line[0][1] / u);
        let options = LayOptions { level, defence_every, access_every, caps: [true, true], ends: [ends[0].as_ref().map(|e| e.0.clone()), ends[1].as_ref().map(|e| e.0.clone())] };
        let cat = self.kit_catalogue(profile.key)?;
        let mut laid = lay(&line, &cat, options);
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
    fn kit_stretch_plans(&self, elem: usize, layout: &KitLayout) -> Vec<Plan> {
        let sc = self.tscene();
        let Some(profile) = wce_walls::scene::wall_profile(sc, elem) else { return vec![] };
        let tolerance = sc.doc().get(elem, "wall_tolerance").and_then(|t| t.trim().parse::<f64>().ok()).unwrap_or(0.5).max(0.0);
        let u = self.unit();
        let ground = |x: f64, z: f64| self.project.height_at(x / u, z / u);
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

            self.drop_unused_wall_pieces();
            return;
        };

        for n in &layout.notes {
            log::warn(format!("kit wall {}: {n}", self.tscene().name(elem)));
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

    /// A kit wall's last layout (as drawn), for `wall_info`.
    pub fn kit_wall_report(&self, elem: usize) -> Option<&KitLayout> {
        self.wall_preview.kit_layouts.get(&elem)
    }
}
