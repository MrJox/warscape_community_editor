//! A naval unit's ship (docs/plans/units.md, "Naval units"): its definition (`unit_variants_ships.variant`) assembled into rigid parts, each placed
//! in the ship's own space, with its oars and weapons where its logic (`models_naval`, docs/formats/models_naval.md) puts them, and where its men
//! stand. The pieces are the intact ones (`_dm1`), the shields go on their attachment points along the sides. The ship's space is the models' as
//! stored and the logic's: metres, x starboard, y up, +z its bow. **ASSUMPTION** (by the hulls' bounds and the logic's buoyancy spheres about 0,
//! not traced): its origin is at the waterline.

use std::sync::Arc;

use wce_formats::models_naval::{EfLine, ShipLogic};
use wce_foundation::math::{Float4x4, rotation_y, translation};
use wce_foundation::math::Float3;

use crate::unit_assembly::{Attach, Reader, assemble_ship};
use crate::unit_catalog::{UnitCatalog, variant_path};

/// One rigid part of a ship.
#[derive(Clone, Debug, PartialEq)]
pub struct ShipPart {
    pub model: String,

    /// Where it is in the ship's space.
    pub local: Float4x4,

    /// The faction colour each channel of the colour mask takes.
    pub masks: [u8; 3],
}

/// A ship, ready to place.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShipModel {
    /// The main unit.
    pub unit: String,

    /// Its definition.
    pub variant: String,
    pub parts: Vec<ShipPart>,

    /// Its logic (`ship_dbs.model`), when the table has it.
    pub logic: Option<Arc<ShipLogic>>,

    /// The definitions and models named that could not be read.
    pub missing: Vec<String>,
}

/// The EF lines' purposes the men fill, in this order (`models_naval::EF_PURPOSES`): marines, the firing positions, seamen, overflow, the
/// perimeter, boarding. **ASSUMPTION**: the game's order isn't traced; the lines of the first are where the oracle ship's men are drawn.
const CREW_PURPOSES: [u32; 8] = [4, 10, 11, 12, 5, 3, 13, 9];

/// The weapon lines: the primary weapon's, the secondary's (**ASSUMPTION**, by their names).
const ENGINE_PLACEMENT: u32 = 22;
const SECONDARY_ENGINE_PLACEMENT: u32 = 23;

/// A 3 x 4 matrix's rows as a 4 x 4.
fn rows_matrix(rows: &[f32; 12]) -> Float4x4 {
    let mut m = Float4x4::IDENTITY;
    for (r, row) in m.m.iter_mut().take(3).enumerate() {
        row.copy_from_slice(&rows[r * 4..r * 4 + 4]);
    }

    m
}

/// The turn about y that takes +z to `direction`.
fn facing(direction: [f32; 3]) -> f32 {
    direction[0].atan2(direction[2])
}

/// At a line's middle, facing its way.
fn on_line(line: &EfLine) -> Float4x4 {
    let [a, b] = line.ends;
    translation(Float3::new((a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5, (a[2] + b[2]) * 0.5)) * rotation_y(facing(line.direction))
}

/// The oar of a custom position's kind (6 to 8 port, levels 1 to 3; 9 to 11 starboard) with the ship's oar kinds (upper, middle, lower): its model.
/// **ASSUMPTION**: level 1 is the lowest row.
fn oar_model(catalog: &UnitCatalog, oars: &[String; 3], kind: u32) -> Option<String> {
    let (starboard, level) = match kind {
        6..=8 => (false, kind - 6),
        9..=11 => (true, kind - 9),
        _ => return None,
    };

    let port = catalog.oars.get(&oars[2 - level as usize])?;
    Some(if starboard { port.replace("oar_left", "oar_right") } else { port.clone() })
}

/// Where `men` stand on a ship (its space): along its lines of the crew's purposes in [`CREW_PURPOSES`]' order, each line from its first end,
/// `spacing` metres apart, as many as it holds. Fewer when the lines are full.
pub fn crew_spots(logic: &ShipLogic, men: u32, spacing: f32) -> Vec<[f32; 3]> {
    let spacing = if spacing > 0.1 { spacing } else { 0.8 };
    let mut out = Vec::with_capacity(men as usize);
    for purpose in CREW_PURPOSES {
        for line in logic.ef_lines.iter().filter(|l| l.purpose == purpose) {
            let [a, b] = line.ends;
            let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            let fits = (length / spacing).floor() as u32 + 1;

            // Centred on the line.
            let margin = (length - (fits - 1) as f32 * spacing) * 0.5 / length.max(1e-6);
            for k in 0..fits {
                if out.len() >= men as usize {
                    return out;
                }

                let t = if fits == 1 { 0.5 } else { margin + k as f32 * spacing / length };
                out.push([a[0] + d[0] * t, a[1] + d[1] * t, a[2] + d[2] * t]);
            }
        }
    }

    out
}

impl ShipModel {
    /// The ship of naval unit `unit` (a `main_units` key) for `faction`, the same for the same seed. `exists` says whether a model is there.
    ///
    /// # Errors
    ///
    /// Returns which link of the unit's data is missing.
    pub fn load(read: &Reader, catalog: &UnitCatalog, unit: &str, faction: &str, seed: u64) -> Result<Self, String> {
        let naval = catalog.naval_units.get(unit).ok_or_else(|| format!("'{unit}' is not a naval unit"))?;
        let entry = catalog.ship_entry(&naval.naval_unit, faction).ok_or_else(|| format!("no unit_variants_ships row for '{}'", naval.naval_unit))?;
        let definition = variant_path(&entry.variant);
        let assembled = assemble_ship(read, &definition, seed).ok_or_else(|| format!("{definition} could not be read"))?;
        let mut missing = assembled.missing;
        let mut parts: Vec<ShipPart> = assembled.parts.into_iter().map(|p| {
            let local = match &p.attach {
                Attach::Point { matrix, .. } => rows_matrix(matrix),
                Attach::None | Attach::Bone(_) => Float4x4::IDENTITY,
            };

            ShipPart { model: p.model, local, masks: p.masks }
        }).collect();

        let logic = catalog.ship_logic.get(&naval.model).cloned();
        if let Some(logic) = &logic {
            let mut add = |model: String, local: Float4x4| {
                if read(&model).is_some() {
                    parts.push(ShipPart { model, local, masks: [0, 1, 2] });
                } else if !missing.contains(&model) {
                    missing.push(model);
                }
            };

            for p in &logic.custom_positions {
                if let Some(model) = oar_model(catalog, &logic.oars, p.kind) {
                    add(model, translation(Float3::new(p.position[0], p.position[1], p.position[2])));
                }
            }

            for (purpose, weapon) in [(ENGINE_PLACEMENT, &naval.primary_weapon), (SECONDARY_ENGINE_PLACEMENT, &naval.secondary_weapon)] {
                let Some(model) = catalog.naval_weapons.get(weapon) else { continue };
                for line in logic.ef_lines.iter().filter(|l| l.purpose == purpose) {
                    add(model.clone(), on_line(line));
                }
            }
        }

        Ok(Self { unit: unit.to_owned(), variant: entry.variant.clone(), parts, logic, missing })
    }

    /// Where `men` stand aboard, `spacing` metres apart along the lines; none without its logic.
    pub fn crew(&self, men: u32, spacing: f32) -> Vec<[f32; 3]> {
        self.logic.as_ref().map_or_else(Vec::new, |l| crew_spots(l, men, spacing))
    }

    /// The models, each once.
    pub fn models(&self) -> Vec<&str> {
        let mut models: Vec<&str> = self.parts.iter().map(|p| p.model.as_str()).collect();
        models.sort_unstable();
        models.dedup();
        models
    }
}

#[cfg(test)]
mod tests {
    use wce_formats::models_naval::{EfLine, ShipLogic};
    use wce_foundation::math::Float3;

    use super::{crew_spots, facing, on_line, rows_matrix};

    fn line(purpose: u32, a: [f32; 3], b: [f32; 3]) -> EfLine {
        EfLine { name: String::new(), purpose, ends: [a, b], direction: [0.0, 0.0, 1.0] }
    }

    #[test]
    fn an_attachment_point_moves_its_part() {
        let m = rows_matrix(&[1.0, 0.0, 0.0, 2.0, 0.0, 1.0, 0.0, 3.0, 0.0, 0.0, 1.0, 4.0]);
        let p = m.transform_point(Float3::new(1.0, 1.0, 1.0));
        assert_eq!((p.x, p.y, p.z), (3.0, 4.0, 5.0));
    }

    #[test]
    fn men_fill_the_marines_lines_first_centred() {
        let logic = ShipLogic { ef_lines: vec![line(5, [0.0, 1.0, 9.0], [2.0, 1.0, 9.0]), line(4, [-1.0, 2.0, 0.0], [1.0, 2.0, 0.0])], ..ShipLogic::default() };
        let spots = crew_spots(&logic, 4, 0.8);
        assert_eq!(spots.len(), 4);
        assert!(spots[..3].iter().all(|s| s[1] == 2.0), "the marines' line holds three: {spots:?}");
        assert!((spots[0][0] + 0.8).abs() < 1e-5 && spots[1][0].abs() < 1e-5 && (spots[2][0] - 0.8).abs() < 1e-5, "{spots:?}");
        assert_eq!(spots[3][1], 1.0);
        assert_eq!(crew_spots(&logic, 100, 0.8).len(), 6, "no more than the lines hold");
    }

    #[test]
    fn a_weapon_stands_at_its_lines_middle_facing_its_way() {
        let mut l = line(22, [-2.0, 4.0, 20.0], [2.0, 4.0, 20.0]);
        l.direction = [1.0, 0.0, 0.0];
        let p = on_line(&l).transform_point(Float3::new(0.0, 0.0, 1.0));
        assert!((p.x - 1.0).abs() < 1e-5 && (p.y - 4.0).abs() < 1e-5 && (p.z - 20.0).abs() < 1e-5, "{p:?}");
        assert!(facing([0.0, 0.0, 1.0]).abs() < 1e-6);
    }
}
