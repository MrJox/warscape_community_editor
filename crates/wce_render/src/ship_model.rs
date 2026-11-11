//! A naval unit's ship (docs/plans/units.md, "Naval units"): its definition (`unit_variants_ships.variant`) assembled into rigid parts, each placed
//! in the ship's own space, with its oars and weapons where its logic (`models_naval`, docs/formats/models_naval.md) puts them, and where its men
//! stand. The pieces are the intact ones (`_dm1`), the shields go on their attachment points along the sides. The ship's space is the models' as
//! stored and the logic's: metres, x starboard, y up, +z its bow. **ASSUMPTION** (by the hulls' bounds and the logic's buoyancy spheres about 0,
//! not traced): its origin is at the waterline.
//!
//! Its motion: on the battle sea, [`ShipModel::afloat`] fits the ship to the waves under its buoyancy spheres (the logic's `bobbies`); its
//! oars row with their `models_oars` clips ([`ShipModel::oar_at`]).

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;

use rpfm_lib::files::rigidmodel::RigidModel;
use rpfm_lib::files::rigidmodel::materials::MaterialType;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use wce_formats::anim::Animation;
use wce_formats::models_naval::{EfLine, ShipLogic};
use wce_foundation::math::{Float3, Float4x4, cross, normalize, rotation_y, translation};

use crate::unit_assembly::{Attach, Reader, assemble_ship};
use crate::unit_catalog::{UnitCatalog, variant_path};

/// One rigid part of a ship.
#[derive(Clone, Debug)]
pub struct ShipPart {
    pub model: String,

    /// Where it is in the ship's space.
    pub local: Float4x4,

    /// The faction colour each channel of the colour mask takes.
    pub masks: [u8; 3],

    /// What its model holds: meshes the rigid path draws, ropes (`#ropes`), sails (WeightedCloth, the skinned path).
    pub what: PartMeshes,

    /// An oar's row (`models_oars.left_row` / `right_row`): its one bone turns the oar about the oarlock, `local`'s origin.
    pub row: Option<Arc<Animation>>,
}

/// The kinds of meshes in a part's model (LOD 0).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PartMeshes {
    pub solid: bool,
    pub ropes: bool,
    pub cloth: bool,
}

/// What a model holds; a model that can't be read is taken for a solid one (the rigid path reports it).
fn part_meshes(read: &Reader, model: &str) -> PartMeshes {
    let solid = PartMeshes { solid: true, ..PartMeshes::default() };
    let Some(bytes) = read(model) else { return solid };
    let Ok(m) = RigidModel::decode(&mut Cursor::new(bytes), &Some(DecodeableExtraData::default())) else { return solid };
    let mut out = PartMeshes::default();
    for block in m.lods().first().map(|l| l.mesh_blocks().as_slice()).unwrap_or_default() {
        match block.mesh().material_type() {
            MaterialType::Rope => out.ropes |= block.material().rope().nodes().len() > 1,
            MaterialType::WeightedCloth => out.cloth = true,

            // The cloth simulation's own meshes (`sim_*`, grey test textures) and its colliders: never drawn.
            MaterialType::Cloth | MaterialType::CollisionShape => {}
            _ => out.solid = true,
        }
    }

    out
}

/// A ship, ready to place.
#[derive(Clone, Debug, Default)]
pub struct ShipModel {
    /// The main unit.
    pub unit: String,

    /// Its definition.
    pub variant: String,
    pub parts: Vec<ShipPart>,

    /// Its logic (`ship_dbs.model`), when the table has it.
    pub logic: Option<Arc<ShipLogic>>,

    /// The faction's colours (primary, secondary, uniform, and a fourth), for the sails' masks.
    pub faction_colours: [[f32; 3]; 4],

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

/// The oar of a custom position's kind (6 to 8 port, levels 1 to 3; 9 to 11 starboard) with the ship's oar kinds (upper, middle, lower): its model
/// and its row clip. **ASSUMPTION**: level 1 is the lowest row.
fn oar_model(catalog: &UnitCatalog, oars: &[String; 3], kind: u32) -> Option<(String, String)> {
    let (starboard, level) = match kind {
        6..=8 => (false, kind - 6),
        9..=11 => (true, kind - 9),
        _ => return None,
    };

    let oar = catalog.oars.get(&oars[2 - level as usize])?;
    let model = if starboard { oar.model.replace("oar_left", "oar_right") } else { oar.model.clone() };
    Some((model, oar.rows[usize::from(starboard)].clone()))
}

/// A unit quaternion (x, y, z, w) and a translation as a matrix.
fn bone_matrix(t: [f32; 3], q: [f32; 4]) -> Float4x4 {
    let [x, y, z, w] = q;
    let mut m = Float4x4::IDENTITY;
    m.m[0][..3].copy_from_slice(&[1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)]);
    m.m[1][..3].copy_from_slice(&[2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)]);
    m.m[2][..3].copy_from_slice(&[2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)]);
    (m.m[0][3], m.m[1][3], m.m[2][3]) = (t[0], t[1], t[2]);
    m
}

/// The plane through points (x, z, height) that fits them best, each by its weight (least squares): height at the origin, rise along x and along
/// z. Flat at the weighted mean height when the points don't span a plane.
pub fn fit_plane(points: &[([f32; 2], f32, f32)]) -> (f32, f32, f32) {
    let total: f32 = points.iter().map(|p| p.2).sum::<f32>().max(1e-6);
    let mean = |f: &dyn Fn(&([f32; 2], f32, f32)) -> f32| points.iter().map(|p| f(p) * p.2).sum::<f32>() / total;
    let (mx, mz, mh) = (mean(&|p| p.0[0]), mean(&|p| p.0[1]), mean(&|p| p.1));
    let cov = |f: &dyn Fn(&([f32; 2], f32, f32)) -> f32, g: &dyn Fn(&([f32; 2], f32, f32)) -> f32| points.iter().map(|p| f(p) * g(p) * p.2).sum::<f32>() / total;
    let (dx, dz, dh): (&dyn Fn(&([f32; 2], f32, f32)) -> f32, &dyn Fn(&([f32; 2], f32, f32)) -> f32, &dyn Fn(&([f32; 2], f32, f32)) -> f32) =
        (&|p| p.0[0] - mx, &|p| p.0[1] - mz, &|p| p.1 - mh);
    let (sxx, szz, sxz, sxh, szh) = (cov(dx, dx), cov(dz, dz), cov(dx, dz), cov(dx, dh), cov(dz, dh));
    let det = sxx * szz - sxz * sxz;
    if det.abs() < 1e-9 {
        return (mh, 0.0, 0.0);
    }

    let (a, b) = ((sxh * szz - szh * sxz) / det, (szh * sxx - sxh * sxz) / det);
    (mh - a * mx - b * mz, a, b)
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

            ShipPart { model: p.model, local, masks: p.masks, what: PartMeshes::default(), row: None }
        }).collect();

        let logic = catalog.ship_logic.get(&naval.model).cloned();
        if let Some(logic) = &logic {
            let mut clips: HashMap<String, Option<Arc<Animation>>> = HashMap::new();
            let mut clip = |path: &str| clips.entry(path.to_owned()).or_insert_with(|| read(path).and_then(|b| Animation::decode(&b, usize::MAX)).map(Arc::new)).clone();
            let mut add = |model: String, local: Float4x4, row: Option<Arc<Animation>>| {
                if read(&model).is_some() {
                    parts.push(ShipPart { model, local, masks: [0, 1, 2], what: PartMeshes::default(), row });
                } else if !missing.contains(&model) {
                    missing.push(model);
                }
            };

            for p in &logic.custom_positions {
                if let Some((model, row)) = oar_model(catalog, &logic.oars, p.kind) {
                    add(model, translation(Float3::new(p.position[0], p.position[1], p.position[2])), clip(&row));
                }
            }

            for (purpose, weapon) in [(ENGINE_PLACEMENT, &naval.primary_weapon), (SECONDARY_ENGINE_PLACEMENT, &naval.secondary_weapon)] {
                let Some(model) = catalog.naval_weapons.get(weapon) else { continue };
                for line in logic.ef_lines.iter().filter(|l| l.purpose == purpose) {
                    add(model.clone(), on_line(line), None);
                }
            }
        }

        let mut seen: HashMap<String, PartMeshes> = HashMap::new();
        for p in &mut parts {
            p.what = *seen.entry(p.model.clone()).or_insert_with(|| part_meshes(read, &p.model));
        }

        let white = [[1.0; 3], [1.0; 3], [1.0; 3], [0.0; 3]];
        let faction_colours = catalog.colours.get(faction).copied().unwrap_or(white);
        Ok(Self { unit: unit.to_owned(), variant: entry.variant.clone(), parts, logic, faction_colours, missing })
    }

    /// Where `men` stand aboard, `spacing` metres apart along the lines; none without its logic.
    pub fn crew(&self, men: u32, spacing: f32) -> Vec<[f32; 3]> {
        self.logic.as_ref().map_or_else(Vec::new, |l| crew_spots(l, men, spacing))
    }

    /// The ship on the waves (its space, metres): `wave` the sea's lift at a point of its deck plan (x, z), sampled under its buoyancy spheres
    /// and fitted, each by its mass, with a plane (heave, pitch, roll). Level at the waterline without its logic. **ASSUMPTION**: the game
    /// floats a ship by a physics simulation over these spheres (their connections are springs, not read here); the editor takes where it
    /// would rest on the waves of the moment, without its inertia.
    pub fn afloat(&self, wave: &dyn Fn(f32, f32) -> f32) -> Float4x4 {
        let Some(logic) = self.logic.as_ref().filter(|l| !l.bobbies.is_empty()) else { return Float4x4::IDENTITY };
        let points: Vec<([f32; 2], f32, f32)> = logic.bobbies.iter().map(|b| ([b.position[0], b.position[2]], wave(b.position[0], b.position[2]), b.mass.max(0.0))).collect();
        let (heave, along_x, along_z) = fit_plane(&points);
        let up = normalize(Float3::new(-along_x, 1.0, -along_z));
        let right = normalize(cross(up, Float3::new(0.0, along_z, 1.0)));
        let forward = cross(right, up);
        let mut m = Float4x4::IDENTITY;
        for (column, v) in [right, up, forward].into_iter().enumerate() {
            (m.m[0][column], m.m[1][column], m.m[2][column]) = (v.x, v.y, v.z);
        }

        m.m[1][3] = heave;
        m
    }

    /// A part where it is at time `t` (seconds): an oar rowing turns about its oarlock with its clip; the rest stays at `local`.
    pub fn part_at(part: &ShipPart, t: f32, rowing: bool) -> Float4x4 {
        match part.row.as_ref().filter(|_| rowing).and_then(|clip| clip.sample(t.rem_euclid(clip.duration().max(1e-3))).first().copied()) {
            Some((offset, turn)) => part.local * bone_matrix(offset, turn),
            None => part.local,
        }
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

    use super::{bone_matrix, crew_spots, facing, fit_plane, on_line, rows_matrix};

    fn line(purpose: u32, a: [f32; 3], b: [f32; 3]) -> EfLine {
        EfLine { name: String::new(), purpose, ends: [a, b], direction: [0.0, 0.0, 1.0] }
    }

    #[test]
    fn a_plane_is_fitted_through_the_waves() {
        let plane = |x: f32, z: f32| 0.5 + 0.1 * x - 0.02 * z;
        let points: Vec<([f32; 2], f32, f32)> = [[-4.0, -20.0], [4.0, -20.0], [-4.0, 20.0], [4.0, 20.0], [0.0, 0.0]].iter().map(|p| (*p, plane(p[0], p[1]), 1.0)).collect();
        let (h, a, b) = fit_plane(&points);
        assert!((h - 0.5).abs() < 1e-4 && (a - 0.1).abs() < 1e-4 && (b + 0.02).abs() < 1e-4, "{h} {a} {b}");
        assert_eq!(fit_plane(&[([1.0, 1.0], 2.0, 1.0)]), (2.0, 0.0, 0.0), "one point: level at its height");
    }

    #[test]
    fn a_quarter_turn_about_y_takes_x_to_minus_z() {
        let s = std::f32::consts::FRAC_1_SQRT_2;
        let p = bone_matrix([0.0, 1.0, 0.0], [0.0, s, 0.0, s]).transform_point(Float3::new(1.0, 0.0, 0.0));
        assert!(p.x.abs() < 1e-5 && (p.y - 1.0).abs() < 1e-5 && (p.z + 1.0).abs() < 1e-5, "{p:?}");
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
