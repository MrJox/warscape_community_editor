//! The first road type, `dirt` (docs/plans/procedural_roads.md, "Road pieces"): the base pieces with the kit's own road decals laid on
//! them, generated so that the pieces can be seen at work in the tool, and refined by hand afterwards. The decals and how the kit lays them
//! come from its `global_dirt_road_*` prefabs: a decal's x runs along the road; `path_1` (10 x 2.5 m there) is the ruts, laid end to end down
//! the middle; `border_1` (6 x 1.5 m) and `border_2` (3 x 1.5 m) the worn edges; rocks, puddles and erosion the spots, the puddles and the
//! erosion turned across. `border_*` have a dark lip along one side, which the kit turns either way; here it faces out of the road. The
//! kit's `path_2` is a bent rut, not a patch: it is not used.
//!
//! **How long a part may be.** A part is moved onto the road's curve rigidly, so it is a chord of it, off it by `L x L / 8R` at its middle.
//! At 0.3 m at most: a straight (bent up to 15 degrees over 8 m, R of about 30 m) takes parts up to 8.56 m, the ruts as one decal a
//! straight; a curve (bent up to 90 degrees, R down to 7.5 m) up to 4.2 m. Corners, junctions and ends are not bent: their arms turn whole.
//! Where a kit decal is shortened, it is by no more than 2.5 times (the curve's and the end's 4 m ruts from the kit's 10 m). Corners and
//! junctions are their arms' ruts run on to cross in the middle, worn where they overlap.

use std::path::{Path, PathBuf};

use crate::road_pieces::{self, ANCHOR_ATTR, CURVE_RADIUS, CURVE_TURN, PIECES, UNIT, VERGE, WIDTH, base_guides, group_text, key};

/// The road type's folder and name.
pub const TYPE: &str = "dirt";

/// The kit's decals the pieces are made of (`rigidmodels\decals\battle\<name>.rigid_model_v2`).
pub const DECALS: [&str; 7] =
    ["path_1", "border_1", "border_2", "global_terrain_rocks_01", "global_terrain_rocks_04", "global_terrain_puddle_02", "global_terrain_erossion_12"];

/// The kit's prefabs that hold every one of [`DECALS`] between them: what says that a game's kit has them.
pub const KIT_PREFABS: &str = "global_dirt_road_";

/// Whether a prefab library's kit road prefabs hold every decal the pieces use.
#[must_use]
pub fn kit_has_decals(lib: &crate::prefabs::PrefabLibrary) -> bool {
    let texts = lib.entries().filter(|e| e.key.to_lowercase().starts_with(KIT_PREFABS)).filter_map(|e| lib.contents(&e.key)).collect::<Vec<_>>();
    DECALS.iter().all(|d| {
        let needle = format!("\\{d}.rigid_model_v2");
        texts.iter().any(|t| t.windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle.as_bytes())))
    })
}

/// Whether every dirt piece is in a prefab folder.
#[must_use]
pub fn all_written(prefab_root: &Path) -> bool {
    PIECES.iter().all(|p| prefab_root.join(road_pieces::FOLDER).join(TYPE).join(format!("{}.wce", key(TYPE, p))).exists())
}

/// How tall a decal's box is (metres): enough for a road's banks.
const DEPTH: f64 = 15.0;

/// The most a part of a piece may be off the road's curve where the piece is bent the most (metres).
pub const MAX_SAGITTA: f64 = 0.3;

/// The longest a part may be along the road in a piece (metres), by [`MAX_SAGITTA`] at the tightest curve the piece is bent to; unbent
/// pieces have no limit.
#[must_use]
pub fn max_part_length(piece: &str) -> f64 {
    let tightest = match piece {
        // 15 degrees over 8 m.
        "straight" | "ford" => road_pieces::STRAIGHT / 15f64.to_radians(),
        // 90 degrees over the curve's length.
        "curve" => CURVE_RADIUS * CURVE_TURN / std::f64::consts::FRAC_PI_2,
        _ => return f64::MAX,
    };

    (8.0 * tightest * MAX_SAGITTA).sqrt()
}

/// A decal of the kit: its middle, the way its x runs (a unit vector, the piece's frame: x, z), its length along that and its width
/// (metres), and its own anchor when it is not where it stands.
fn decal(name: &str, at: [f64; 2], way: [f64; 2], length: f64, width: f64, anchor: Option<&str>) -> Vec<u8> {
    // Its x is turned by Ry(yaw) to (cos, -sin) (`world_transform::euler_matrix`).
    let yaw = (-way[1]).atan2(way[0]);
    let n = |v: f64| crate::ted_scene::format_number(v);
    let anchor = anchor.map(|a| format!(" {ANCHOR_ATTR}=\"{a}\"")).unwrap_or_default();
    format!(
        "<ProjectedDecalInstance flags=\" OIF_VISIBLE|OIF_ACTIVE\" scale=\"vector({},{},{})\" lf_offset=\"0\" position=\"vector({},0,{})\" key=\"rigidmodels\\decals\\battle\\{name}.rigid_model_v2\" name=\"{name}\" surface_mode=\"MSM_LANDSCAPE\" orientation=\"euler({},0,0)\" parallax_scale=\"0.1\" tiled=\"false\"{anchor}/>\n",
        n(length * UNIT), n(DEPTH * UNIT), n(-width * UNIT), n(at[0] * UNIT), n(at[1] * UNIT), n(yaw)
    ).into_bytes()
}

const UP: [f64; 2] = [0.0, 1.0];
const DOWN: [f64; 2] = [0.0, -1.0];
const RIGHT: [f64; 2] = [1.0, 0.0];
const LEFT: [f64; 2] = [-1.0, 0.0];

/// The ruts down a stretch along z: two strips side by side, the left one turned end for end so that they do not repeat.
fn ruts(z: f64, length: f64) -> Vec<Vec<u8>> {
    let x = WIDTH * 0.25 - 0.2;
    vec![decal("path_1", [-x, z], DOWN, length, 2.5, None), decal("path_1", [x, z], UP, length, 2.5, None)]
}

/// The ruts down a stretch along x (a branch to the right or left).
fn ruts_x(x: f64, length: f64) -> Vec<Vec<u8>> {
    let z = WIDTH * 0.25 - 0.2;
    vec![decal("path_1", [x, -z], LEFT, length, 2.5, None), decal("path_1", [x, z], RIGHT, length, 2.5, None)]
}

/// Where roads meet the middle is worn all over, not grass between two pairs of ruts: a rut laid across it diagonally (`way`, not a unit
/// vector), where the piece has it.
fn worn_middle(way: [f64; 2]) -> Vec<u8> {
    let l = way[0].hypot(way[1]);
    decal("path_1", [0.0, 0.0], [way[0] / l, way[1] / l], 5.0, 3.0, Some("fixed"))
}

/// The decals of a piece of the dirt road.
#[must_use]
pub fn parts(piece: &str) -> Vec<Vec<u8>> {
    let h = WIDTH * 0.5;
    let mut out = vec![];
    match piece {
        "straight" | "ford" => {
            out.extend(ruts(0.0, 8.0));
            out.push(decal("border_1", [-h, 0.0], DOWN, 8.0, 1.5, None));
            out.push(decal("border_1", [h, 0.0], UP, 8.0, 1.5, None));
            out.push(decal("global_terrain_rocks_04", [h - 0.6, -2.2], UP, 2.5, 2.5, None));
            out.push(decal("global_terrain_rocks_01", [-h - 0.5, 1.6], UP, 2.5, 2.5, None));
            out.push(decal("global_terrain_erossion_12", [h + VERGE * 0.6, 0.8], RIGHT, 2.5, 6.0, None));
            if piece == "straight" {
                out.push(decal("global_terrain_puddle_02", [0.7, 1.9], RIGHT, 2.5, 5.0, None));
            } else {
                // A ford: wet all over, stones across.
                out.push(decal("global_terrain_puddle_02", [-1.2, -2.0], RIGHT, 2.5, 5.0, None));
                out.push(decal("global_terrain_puddle_02", [1.3, 2.1], LEFT, 2.5, 5.0, None));
                out.push(decal("global_terrain_rocks_01", [-0.4, 0.3], RIGHT, 2.5, 2.5, None));
                out.push(decal("global_terrain_rocks_04", [1.6, -0.6], UP, 2.5, 2.5, None));
            }
        }

        "end" => {
            // The road stops at z = -2: worn ground fading over the end, its edges closed across the back.
            out.extend(ruts(0.0, 4.0));
            out.push(decal("border_1", [-h, 0.0], DOWN, 4.0, 1.5, None));
            out.push(decal("border_1", [h, 0.0], UP, 4.0, 1.5, None));
            out.push(decal("border_2", [0.0, -2.0], LEFT, 4.0, 1.5, None));
            out.push(decal("global_terrain_rocks_04", [-1.8, -2.4], UP, 2.5, 2.5, None));
            out.push(decal("global_terrain_rocks_01", [2.0, -2.6], UP, 2.5, 2.5, None));
        }

        "curve" => {
            // Along the arc, four 4 m ruts and borders a side, chords of it, overlapping a little (the outer side's are 3.2 m apart).
            let (c, r) = ([CURVE_RADIUS, 0.0], CURVE_RADIUS);
            let at = |rr: f64, t: f64| [c[0] - rr * t.cos(), c[1] + rr * t.sin()];
            let way = |t: f64| [t.sin(), t.cos()];
            let back = |t: f64| [-t.sin(), -t.cos()];
            for i in 0..4 {
                let t = (f64::from(i) - 1.5) * CURVE_TURN / 4.0;
                let w = if i % 2 == 0 { back(t) } else { way(t) };
                out.push(decal("path_1", at(r + 1.3, t), w, 4.0, 2.5, None));
                out.push(decal("path_1", at(r - 1.3, t), way(t), 4.0, 2.5, None));
                out.push(decal("border_1", at(r + h, t), back(t), 4.0, 1.5, None));
                out.push(decal("border_1", at(r - h, t), way(t), 4.0, 1.5, None));
            }

            out.push(decal("global_terrain_rocks_04", at(r - h + 0.6, -0.2), UP, 2.5, 2.5, None));
            out.push(decal("global_terrain_rocks_01", at(r + h + 0.5, 0.25), UP, 2.5, 2.5, None));
            out.push(decal("global_terrain_erossion_12", at(r + h + VERGE * 0.6, -0.05), RIGHT, 2.5, 4.0, None));
        }

        "corner" => {
            // From the back (-z) turning right (+x): each arm's ruts run on across the middle, the outer edge round the corner.
            out.extend(ruts(-2.25, 7.5));
            out.extend(ruts_x(2.25, 7.5));
            out.push(worn_middle([1.0, 1.0]));
            out.push(decal("border_1", [-h, -3.0], DOWN, 6.0, 1.5, None));
            out.push(decal("border_1", [0.0, h], LEFT, 6.0, 1.5, None));
            out.push(decal("border_2", [-h, h], UP, 2.5, 2.5, Some("fixed")));
            out.push(decal("border_2", [4.5, h], LEFT, 3.0, 1.5, None));
            out.push(decal("border_2", [h, -4.5], UP, 3.0, 1.5, None));
            out.push(decal("border_2", [4.5, -h], RIGHT, 3.0, 1.5, None));
            out.push(decal("global_terrain_rocks_04", [-h - 0.4, h + 0.4], UP, 2.5, 2.5, None));
            out.push(decal("global_terrain_rocks_01", [h - 0.2, -h + 0.2], UP, 2.5, 2.5, None));
        }

        "t_junction" => {
            // The through road along z, the branch to the right (+x).
            out.extend(ruts(-3.0, 6.0));
            out.extend(ruts(3.0, 6.0));
            out.extend(ruts_x(2.25, 7.5));
            out.push(worn_middle([1.0, 1.0]));
            out.push(decal("border_1", [-h, -3.0], DOWN, 6.0, 1.5, None));
            out.push(decal("border_1", [-h, 3.0], DOWN, 6.0, 1.5, None));
            out.push(decal("border_2", [h, -4.5], UP, 3.0, 1.5, None));
            out.push(decal("border_2", [h, 4.5], UP, 3.0, 1.5, None));
            out.push(decal("border_2", [4.5, -h], RIGHT, 3.0, 1.5, None));
            out.push(decal("border_2", [4.5, h], LEFT, 3.0, 1.5, None));
            out.push(decal("global_terrain_rocks_04", [-h - 0.5, -0.8], UP, 2.5, 2.5, None));
        }

        "x_junction" => {
            out.extend(ruts(-3.0, 6.0));
            out.extend(ruts(3.0, 6.0));
            out.extend(ruts_x(-3.0, 6.0));
            out.extend(ruts_x(3.0, 6.0));
            out.push(worn_middle([1.0, 1.0]));
            out.push(worn_middle([1.0, -1.0]));
            for (x, z, way) in [(h, -4.5, UP), (h, 4.5, UP), (-h, -4.5, DOWN), (-h, 4.5, DOWN), (4.5, -h, RIGHT), (4.5, h, LEFT), (-4.5, -h, RIGHT), (-4.5, h, LEFT)] {
                out.push(decal("border_2", [x, z], way, 3.0, 1.5, None));
            }

            out.push(decal("global_terrain_rocks_01", [h + 0.3, h + 0.3], UP, 2.5, 2.5, None));
        }

        _ => {}
    }

    out
}

/// Writes the dirt pieces into a prefab folder (the projects'), each only where it is missing. Returns the files written.
///
/// # Errors
///
/// A file can't be written.
pub fn write(prefab_root: &Path) -> Result<Vec<PathBuf>, String> {
    let dir = prefab_root.join(road_pieces::FOLDER).join(TYPE);
    let mut written = vec![];
    for piece in PIECES {
        let key = key(TYPE, piece);
        let path = dir.join(format!("{key}.wce"));
        if path.exists() {
            continue;
        }

        let mut children = base_guides(piece);
        children.extend(parts(piece));
        crate::prefabs::write(&path, &group_text(&key, &children), None)?;
        written.push(path);
    }

    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::road_pieces::{Anchor, read};

    #[test]
    fn every_dirt_piece_reads_and_its_parts_fit() {
        for piece in PIECES {
            let mut children = base_guides(piece);
            children.extend(parts(piece));
            let p = read(&group_text(&key(TYPE, piece), &children)).unwrap();
            assert!(p.notes.is_empty(), "{piece}: {:?}", p.notes);
            assert!(p.parts.len() >= 6, "{piece}: {} parts", p.parts.len());
            assert!(p.parts.iter().all(|q| !q.solid), "{piece}: no building, so none on the road");

            // Every decal of the kit's road set, and only those.
            assert!(p.parts.iter().all(|q| DECALS.iter().any(|d| q.key.ends_with(&format!("\\{d}.rigid_model_v2")))), "{piece}");

            let most = max_part_length(piece);
            for q in &p.parts {
                assert!(q.size[0] <= most + 1e-6, "{piece}: {} is {:.2} m along, more than {most:.2}", q.key, q.size[0]);
            }

            // The ruts and the patches lie on the road, the borders on its edges.
            for q in &p.parts {
                let name = q.key.rsplit('\\').next().unwrap_or_default();
                if name.starts_with("path_") && q.anchor != Anchor::Fixed {
                    assert_eq!(q.anchor, Anchor::Surface, "{piece}: {q:?}");
                }

                if name.starts_with("border_1") {
                    assert_eq!(q.anchor, Anchor::Edge, "{piece}: {q:?}");
                }
            }
        }
    }

    #[test]
    fn the_ruts_run_along_the_road() {
        let mut children = base_guides("straight");
        children.extend(parts("straight"));
        let p = read(&group_text("road_dirt_straight", &children)).unwrap();
        for q in p.parts.iter().filter(|q| q.key.contains("path_1") || q.key.contains("border_1")) {
            assert!(q.turn.sin().abs() < 1e-6, "{q:?}");
            assert!((q.size[0] - 8.0).abs() < 1e-6, "{q:?}");
        }
    }

    #[test]
    fn the_limits_follow_the_bends() {
        assert!((max_part_length("straight") - 8.56).abs() < 0.01, "{}", max_part_length("straight"));
        assert!((max_part_length("curve") - 4.24).abs() < 0.01, "{}", max_part_length("curve"));
        assert_eq!(max_part_length("corner"), f64::MAX);
    }

    #[test]
    fn written_once_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(write(dir.path()).unwrap().len(), PIECES.len());
        assert!(write(dir.path()).unwrap().is_empty());
        let lib = crate::prefabs::PrefabLibrary::scan_folders(&[dir.path().to_path_buf()]);
        let curve = lib.load("road_dirt_curve").unwrap();
        assert_eq!(read(&curve.group_text).unwrap().arms.len(), 1);
        assert_eq!(lib.get("road_dirt_curve").unwrap().folder, "roads_proc/dirt");
    }
}
