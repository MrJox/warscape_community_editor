//! Walls of kit pieces in the document (docs/plans/kit_walls.md): a `WceWallSpline` with *Procedural* off is laid with its culture's own
//! fort pieces (`wce_walls::kit`) along the same centre line a procedural wall follows (its points, handles and sharp marks), and drawn as
//! those pieces, whole, through the procedural walls' preview (`walls.rs`: the technology overlay and Check platforms read them the same
//! way). Phase 2: on flat ground (the level under the line's first point), both ends capped.
//!
//! Scene units throughout, as `walls.rs`.

use std::collections::HashMap;
use std::sync::Arc;

use wce_foundation::log;
use wce_scene::render_scene::RenderScene;
use wce_walls::kit::{Catalogue, KitLayout, LayOptions, catalogue, culture_keys, lay};
use wce_walls::pieces::Piece;

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

    /// A wall of kit pieces laid (`wce_walls::kit::lay`): its centre line (the procedural walls' `solver::centre_line_marked`, reversed when
    /// flipped, so that its outer face is on the same side), its culture (`wall_base`), the defence and access spacings; flat at the ground
    /// under its first point, both ends capped. `None` for an element that is not a kit wall, a wall of fewer than 2 points, or a culture
    /// without pieces.
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
        let (line, _) = wce_walls::solver::centre_line_marked(&points, &sharp, &handles, profile.fold_radius());
        let mut line = line.into_iter().map(|(p, _)| p).collect::<Vec<_>>();
        if flip {
            line.reverse();
        }

        let u = self.unit();
        let level = self.project.height_at(line[0][0] / u, line[0][1] / u);
        let options = LayOptions { level, defence_every, access_every, caps: [true, true] };
        let cat = self.kit_catalogue(profile.key)?;
        Some(lay(&line, &cat, options))
    }

    /// A kit wall's pieces on the scene in place of its old ones: the game's own pieces, whole, where its layout puts them. A hidden wall
    /// (or one that can't be laid) has none.
    pub(super) fn make_kit_wall_pieces(&mut self, scene: &mut RenderScene, elem: usize) {
        let before = self.wall_preview.shown_as.remove(&elem);
        for id in self.wall_preview.entities.remove(&elem).unwrap_or_default() {
            scene.remove(id);
        }

        let Some(layout) = self.kit_layout(elem).filter(|_| self.shown(elem)) else {
            self.drop_unused_wall_pieces();
            return;
        };

        for n in &layout.notes {
            log::warn(format!("kit wall {}: {n}", self.tscene().name(elem)));
        }

        let placed = layout.placed.iter().map(|p| (p.key.clone(), Piece { s: (0.0, 0.0), origin: p.origin, yaw: p.yaw, scale: 1.0, base: None, rigid: true })).collect::<Vec<_>>();
        let mut own = HashMap::new();
        for (key, _) in &placed {
            if !own.contains_key(key) && let Some(b) = self.wall_base(key) {
                own.insert(key.clone(), b);
            }
        }

        // Kept by what was laid (a kit wall's own signature, never a procedural one's).
        let signature = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            "kit wall".hash(&mut h);
            for (key, p) in &placed {
                key.hash(&mut h);
                p.origin.map(f64::to_bits).hash(&mut h);
                p.yaw.to_bits().hash(&mut h);
            }

            h.finish()
        };

        self.show_wall_pieces(scene, elem, signature, vec![], &placed, &own);
        self.wall_preview.kit_layouts.insert(elem, layout);
        if Some(signature) != before {
            self.tech_wall_changed(elem);
        }
    }

    /// A kit wall's last layout (as drawn), for `wall_info`.
    pub fn kit_wall_report(&self, elem: usize) -> Option<&KitLayout> {
        self.wall_preview.kit_layouts.get(&elem)
    }
}
