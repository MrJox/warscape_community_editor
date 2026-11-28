//! The foam map of a tile's rivers (`wce_tile::river_foam`): made from the editor's rivers (those its river shader draws), the ground
//! and the buildings standing in their water (the tile's own and the shores' rocks); drawn on the editor's water, saved with the project
//! for the export's river mesh.

use wce_tile::river_foam::{FILE, FoamMap, FoamRiver, FoamSample, Obstacle, foam_map};

use super::Tile;

/// How far past the tile's edges the map reaches (m): the rivers' ribbons go on past them.
const MAP_BORDER_M: f64 = 96.0;

/// A building reaching farther round than this (m) is no obstacle in the water: a bridge spans it.
const MAX_OBSTACLE_M: f64 = 12.0;

impl Tile {
    /// The foam map of the rivers the editor's river shader draws (`river_files::SHADER_RIVER_ATTRS`); none when there are none.
    #[must_use]
    pub fn river_foam_map(&self) -> Option<FoamMap> {
        let rivers = self
            .river_ribbons_marked()
            .into_iter()
            .map(|samples| FoamRiver {
                samples: samples.iter().map(|s| FoamSample { x: s.x, z: s.z, y: s.y, half: s.width * 0.5, flow: s.flow }).collect(),
            })
            .collect::<Vec<_>>();
        if rivers.is_empty() {
            return None;
        }

        let unit = self.unit();
        let sc = self.tscene();
        let radius = |m: &super::BuildingModel, scale: f64| {
            let across = [m.bmin.x.abs(), m.bmax.x.abs(), m.bmin.z.abs(), m.bmax.z.abs()].into_iter().fold(0.0f32, f32::max);
            f64::from(across) * scale / unit
        };
        // The tile's buildings (their models' bounds; not a bridge or anything as big, which spans the water), and the shores' rocks
        // (the rapids' by their table's radius, others by their model's when it is loaded).
        let mut obstacles = vec![];
        for &e in self.instances() {
            if sc.type_of(e) != "BUILDING_INSTANCE" {
                continue;
            }

            let Some(m) = self.model_of(e).filter(|m| m.ok && m.resolved) else { continue };
            let s = sc.scale(e).map_or(1.0, |s| s.x.abs().max(s.z.abs()));
            let p = self.world_position(e);
            let r = radius(m, s);
            if r <= MAX_OBSTACLE_M {
                obstacles.push(Obstacle { x: f64::from(p.x), z: f64::from(p.z), radius: r });
            }
        }

        let rapids = &wce_tile::shores::keys().rapids;
        for item in self.shore_items.iter().filter(|i| i.text.starts_with(b"<BUILDING_INSTANCE")) {
            let text = String::from_utf8_lossy(&item.text);
            let Some(key) = text.split("key=\"").nth(1).and_then(|k| k.split('"').next()) else { continue };
            let r = match rapids.iter().find(|r| r.key == key) {
                Some(rock) => f64::from(rock.radius()),
                None => match self.building_model(key).filter(|m| m.ok && m.resolved) {
                    Some(m) => radius(m, 1.0),
                    None => continue,
                },
            };
            obstacles.push(Obstacle { x: item.position[0] / unit, z: item.position[2] / unit, radius: r });
        }

        let ground = |x: f64, z: f64| f64::from(self.terrain_height(x as f32, z as f32));
        let (w, h) = (self.extent_x(), self.extent_z());
        Some(foam_map(&rivers, &obstacles, &ground, [-MAP_BORDER_M, -MAP_BORDER_M, w + MAP_BORDER_M, h + MAP_BORDER_M]))
    }

    /// Writes the rivers' foam map into the project's folder (`river_foam::FILE`), or takes an old one out when no river has foam to
    /// draw. A combined project's variations are left as they are.
    ///
    /// # Errors
    ///
    /// The file can't be written or removed.
    pub fn save_river_foam(&self) -> Result<(), String> {
        if self.project.is_combined() {
            return Ok(());
        }

        let path = self.project.dir.join(FILE);
        match self.river_foam_map() {
            Some(map) => wce_foundation::files::write_replacing(&path, map.to_bytes()).map_err(|e| format!("{}: {e}", path.display())),
            None if path.exists() => std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display())),
            None => Ok(()),
        }
    }
}
