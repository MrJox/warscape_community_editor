//! A river tile made by a seed on the open tile (docs/plans/river_bridges.md, step 3; `wce_tile::river_tile_gen`): fresh relief as its
//! heights, and a river from its entry link to its exit link along a natural line, snapped to the links (its bed carved and its shore on,
//! as a drawn river's), its widths varied; then dressed (step 4): a cliff on the outer side of its sharpest bend with rocks on its face, and
//! woods away from it. One undo entry.

use wce_editor_core::document::Document;
use wce_foundation::math::Float3;
use wce_tile::river_tile_gen::{
    ForestSettings, LineSettings, LinkEnd, ReliefSettings, Valley, cliff_rock_places, forest_areas, outer_bend_cliff, relief_in_valley,
    river_line, smooth_line,
};

use super::{BrushKind, BrushSettings, Create, TileDocument};

/// What a fresh generated tile clears first: the kit tile's own river, its buildings, plants, hints, decals and water.
const CLEARED: [&str; 8] = [
    "SplineRiverInstance",
    "WceRoadSpline",
    "BUILDING_INSTANCE",
    "VEGETATION_INSTANCE",
    "AI_HINT_POLYGON_INSTANCE",
    "ProjectedDecalInstance",
    "SIMPLE_MESH_INSTANCE",
    "PARTICLE_EMITTER_INSTANCE",
];

/// The river's valley: its floor over the links' water, and how far from the river it reaches (metres).
const VALLEY_FLOOR_M: f64 = 1.0;
const VALLEY_HALF_WIDTH_M: f64 = 70.0;

/// How much a generated river's widths vary (percent of the width, from and to).
const WIDTH_VARIATION: (f64, f64) = (-20.0, 30.0);

/// The cliff on the outer side of the river's sharpest bend: how many of the smoothed line's samples it runs along, how far past the
/// river's widest water its foot is, how wide its climb and how high (metres).
const CLIFF_SAMPLES: usize = 18;
const CLIFF_FOOT_PAST_WATER_M: f64 = 3.0;
const CLIFF_RUN_M: f64 = 9.0;
const CLIFF_HEIGHT_M: f64 = 9.0;

/// The rocks along the cliff's face: their keys (Attila's kit rocks, as the built-in biomes use them), how far past its foot, how far
/// apart (metres).
const CLIFF_ROCKS: [&str; 2] = ["gaul_rock_01", "gaul_rock_02"];
const CLIFF_ROCK_OFF_M: f64 = 6.0;
const CLIFF_ROCK_STEP_M: f64 = 9.0;

/// How far past the river's widest water the woods keep (metres).
const WOODS_PAST_WATER_M: f64 = 15.0;

/// The river's points per stretch of its line (the line's own points are few, the river's spline would turn sharply at them).
const RIVER_POINTS_PER_STRETCH: usize = 2;

impl TileDocument {
    /// Makes the open tile's ground and river by `seed` (one undo entry): `clear` first takes away what the tile has
    /// ([`CLEARED`]); the heights are replaced by fresh relief (rolling, flat at the edges, `wce_tile::river_tile_gen::relief`); a river
    /// is drawn from the tile's first river link to its second along a meandering line, snapped to the links and its widths varied.
    /// Returns what was made.
    ///
    /// # Errors
    ///
    /// The tile has no height map, or not two river links.
    pub fn generate_river_tile(&mut self, seed: u64, clear: bool) -> Result<String, String> {
        let links = self.tile().river_link_points();
        if links.len() != 2 {
            return Err(format!("a river tile with two river links (this one has {})", links.len()));
        }

        self.end_stroke();
        let before = self.history().position();
        let result = self.generate_river_tile_steps(seed, clear, &links);
        let n = self.history().position() - before;
        if n > 1 {
            self.join_last(n, "Generate river tile");
        }

        result
    }

    fn generate_river_tile_steps(
        &mut self,
        seed: u64,
        clear: bool,
        links: &[super::river_links::RiverLinkPoint],
    ) -> Result<String, String> {
        let mut made = vec![];
        if clear {
            // From the scene itself: the index may be a step behind it (an element just made).
            let sc = self.tile().tscene();
            let mut gone = CLEARED.iter().flat_map(|k| sc.find_all(k)).collect::<Vec<_>>();
            gone.sort_unstable();
            gone.dedup();
            if !gone.is_empty() && self.remove_items(&gone) {
                made.push(format!("{} instance(s) cleared", gone.len()));
            }

            // Its paint too: the whole tile its first texture, its ground types cleared (the kit river's bed and road painted).
            let fill = BrushSettings { strength: 1.0, opacity: 1.0, falloff: 0.0, ..BrushSettings::default() };
            // The ground's texture: the tile's group named like grass (its first variation), else its first.
            let names = self.tile().texture_group_names();
            let ground = names.iter().position(|n| n.to_lowercase().contains("grass")).unwrap_or(0) * 4;
            if self.paint_whole_tile(BrushKind::Paint(ground as u8), &fill).unwrap_or(false) {
                made.push("textures reset".to_owned());
            }

            let cleared = if self.tile().project.dir.join("ground_type_map.png").is_file() { self.clear_ground_types()? } else { 0 };
            if cleared > 0 {
                made.push(format!("{cleared} ground type pixel(s) cleared"));
            }
        }

        // The river's line first: from the first link to the second, natural and meandering.
        let p = &self.tile().project;
        let (ex, ez, border, mx, mz, unit) = (p.extent_x(), p.extent_z(), p.border_m(), p.mpp_x(), p.mpp_z(), p.unit_scale);
        let end = |l: &super::river_links::RiverLinkPoint| LinkEnd {
            at: [f64::from(l.snapped.x), f64::from(l.snapped.z)],
            inward: [f64::from(l.inward[0]), f64::from(l.inward[1])],
        };
        let line = river_line(end(&links[0]), end(&links[1]), (ex, ez), seed, &LineSettings::default());

        // The relief with the river's valley in it: the ground along the river a metre over the links' water (their connections'
        // water, in the game's units), hills beyond; an image of the height map's size, each pixel the relief at its place.
        let water = (links[0].connection.water + links[1].connection.water) * 0.5 / unit;
        let mut valley =
            Valley { line: smooth_line(&line, 8), floor: water + VALLEY_FLOOR_M, half_width: VALLEY_HALF_WIDTH_M, cliff: None };
        let (w, h) = p.height.as_ref().map(|hf| (hf.width(), hf.height())).ok_or("the tile has no height map")?;
        let relief_image = |valley: &Valley| {
            let s = ReliefSettings::default();
            let values = (0..h)
                .flat_map(|y| (0..w).map(move |x| (x, y)))
                .map(|(x, y)| relief_in_valley(seed, f64::from(x) * mx - border, f64::from(y) * mz - border, (ex, ez), &s, valley))
                .collect::<Vec<_>>();
            let (lo, hi) = values.iter().fold((f64::MAX, f64::MIN), |(l, h), v| (l.min(*v), h.max(*v)));
            let span = (hi - lo).max(1e-6);
            let v = values.iter().map(|v| ((v - lo) / span) as f32).collect();
            (wce_tile::brush_image::BrushImage { path: format!("relief {seed}"), w, h, v }, lo, hi)
        };
        let (image, lo, hi) = relief_image(&valley);
        let changed = self.import_height_image(&image, lo as f32, hi as f32)?;
        made.push(format!("relief {lo:.1} .. {hi:.1} m ({changed} samples)"));

        // The river along the line, snapped to the links; its water from one link's to the other's by the distance along (never uphill
        // over the hills it was drawn on); its widths varied.
        let points = smooth_line(&line, RIVER_POINTS_PER_STRETCH)
            .iter()
            .map(|q| Float3::new(q[0] as f32, self.tile().terrain_height(q[0] as f32, q[1] as f32), q[1] as f32))
            .collect::<Vec<_>>();
        let graph = self.tile().tscene().scene_graph().ok_or("the tile has no scene graph")?;
        let river = self.create("SplineRiverInstance", graph, &Create::Line(points), "")?;
        self.snap_river_to_links(river)?;
        if let Some(profile) = self.river_profile(river).filter(|p| p.points.len() >= 2) {
            let (first, last) = (&profile.points[0], &profile.points[profile.points.len() - 1]);
            let length = (last.along - first.along).max(1e-6);
            let heights = profile.points[1..profile.points.len() - 1]
                .iter()
                .map(|q| (q.index, first.water + (last.water - first.water) * ((q.along - first.along) / length).clamp(0.0, 1.0)))
                .collect::<Vec<_>>();
            let g = self.next_gesture();
            self.set_river_heights(river, &heights, "River water by its links", g)?;
        }

        let varied = self.randomize_river_widths(river, WIDTH_VARIATION.0, WIDTH_VARIATION.1, seed)?;
        made.push(format!(
            "a river of {} points from link {:?} to {:?}, {varied} width(s) varied",
            line.len(),
            links[0].cell,
            links[1].cell
        ));

        // How far from its line its water reaches at most: the cliff and the woods keep past it.
        let water_half = self
            .tile()
            .river_width_handles(river)
            .iter()
            .flat_map(|(c, a, b)| [a, b].map(|e| f64::from((e.x - c.x).hypot(e.z - c.z))))
            .fold(0.0, f64::max);

        // Every other seed has a cliff, on the outer side of the river's sharpest bend: the relief made again with it.
        valley.cliff = (seed % 2 == 0)
            .then(|| outer_bend_cliff(&valley.line, CLIFF_SAMPLES, water_half + CLIFF_FOOT_PAST_WATER_M, CLIFF_RUN_M, CLIFF_HEIGHT_M))
            .flatten();
        if let Some(c) = valley.cliff {
            let (image, lo, hi) = relief_image(&valley);
            self.import_height_image(&image, lo as f32, hi as f32)?;

            // Its rocks, on its face (where it stands at least half its height).
            let mut rocks = 0;
            for (k, (q, rise)) in cliff_rock_places(&valley.line, &c, CLIFF_ROCK_OFF_M, CLIFF_ROCK_STEP_M).into_iter().enumerate() {
                if rise < 0.5 {
                    continue;
                }

                let at = Float3::new(q[0] as f32, self.tile().terrain_height(q[0] as f32, q[1] as f32), q[1] as f32);
                let key = CLIFF_ROCKS[(seed as usize + k) % CLIFF_ROCKS.len()];
                match self.add_building_in(key, None, Some(at), super::Placement::Landscape, Some(graph)) {
                    Ok(_) => rocks += 1,
                    Err(e) => wce_foundation::log::warn(format!("river tile: a cliff rock {key}: {e}")),
                }
            }

            let mid = valley.line[(c.from + c.to) / 2];
            made.push(format!("a cliff by {:.0}, {:.0} ({rocks} rock(s))", mid[0], mid[1]));
        }

        // Woods away from the river: broadleaf, or pine on some seeds.
        let keep: [(&[[f64; 2]], f64); 1] = [(&valley.line, water_half + WOODS_PAST_WATER_M)];
        let woods = forest_areas(seed, (ex, ez), &keep, &ForestSettings::default());
        let biome_name = if seed % 3 == 2 { "Pine forest" } else { "Broadleaf wood" };
        if !woods.is_empty()
            && let Some(biome) = wce_tile::biomes::builtin().into_iter().find(|b| b.name == biome_name)
        {
            use wce_tile::biomes::{AREA_TYPE, BIOME_ATTR, SEED_ATTR};
            self.adopt_biome(&biome);
            let mut made_woods = 0;
            for (k, outline) in woods.iter().enumerate() {
                let t = self.tile();
                let points = outline
                    .iter()
                    .map(|q| Float3::new(q[0] as f32, t.terrain_height(q[0] as f32, q[1] as f32), q[1] as f32))
                    .collect::<Vec<_>>();
                let attrs =
                    [(BIOME_ATTR, biome_name.to_owned()), (SEED_ATTR, (seed + k as u64).to_string()), ("name", biome_name.to_owned())]
                        .into_iter()
                        .map(|(a, v)| (a.to_owned(), v))
                        .collect();
                if self.create_with(AREA_TYPE, graph, &Create::Line(points), "", &attrs).is_ok() {
                    made_woods += 1;
                }
            }

            made.push(format!("{made_woods} wood(s) of {biome_name}"));
        }

        Ok(made.join("; "))
    }

    /// Clears the whole ground type map (every pixel's type from its texture again): one undo entry; returns the pixels changed.
    ///
    /// # Errors
    ///
    /// The tile has no ground type map that loads.
    pub fn clear_ground_types(&mut self) -> Result<usize, String> {
        use wce_tile::ground_types::NO_GROUND_TYPE;
        self.end_stroke();
        let g = self.tile_mut().project.load_ground()?;
        g.begin_edit()?;
        let mut changed = 0;
        for y in 0..g.height() {
            for x in 0..g.width() {
                changed += usize::from(g.set_rgb(x, y, NO_GROUND_TYPE));
            }
        }

        g.end_edit();
        if changed > 0 {
            self.history.push(Box::new(super::commands::GroundFillCommand { name: "Clear ground types".to_owned(), first: true }));
        }

        Ok(changed)
    }

    /// Replaces the heights with an image stretched over the height map (black `low_m`, white `high_m`): one undo entry; returns the
    /// samples changed.
    ///
    /// # Errors
    ///
    /// The tile has no height map.
    pub fn import_height_image(&mut self, image: &wce_tile::brush_image::BrushImage, low_m: f32, high_m: f32) -> Result<usize, String> {
        self.end_stroke();
        let (changed, moved) = self.tile_mut().import_heights(image, low_m, high_m, false)?;
        if changed == 0 && moved.is_empty() {
            return Ok(0);
        }

        self.history.push(Box::new(super::commands::HeightFillCommand { name: "Generated relief".to_owned(), first: true, moved }));
        Ok(changed)
    }
}
