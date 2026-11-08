//! The first road type, `dirt` (docs/plans/procedural_roads.md, "Road pieces"): the base pieces with the kit's own road decals laid on
//! them, generated so that the pieces can be seen at work in the tool, and refined by hand afterwards. The decals and how the kit lays them
//! come from its `global_dirt_road_*` prefabs: a decal's x runs along the road; `path_1` (10 x 2.5 m there) is the ruts, laid end to end down
//! the middle; `border_1` (6 x 1.5 m) and `border_2` (3 x 1.5 m) the worn edges; rocks, puddles and erosion the spots, the puddles and the
//! erosion turned across. `border_*` have a dark lip along one side, which the kit turns either way; here it faces out of the road. The
//! kit's `path_2` is a bent rut, not a patch: it is not used.
//!
//! **How long a part may be.** A part is moved onto the road's curve rigidly, so it is a chord of it, off it by `L x L / 8R` at its middle.
//! At 0.4 m at most: a straight (bent up to 15 degrees over 8 m, R of about 30 m) takes parts up to 9.9 m: its ruts and borders are one
//! 9.5 m decal each, past its ports by [`OVERLAP`] (a decal fades out at its ends: end to end, the fades showed as bands of grass across
//! the road); a curve (bent up to 90 degrees, R down to 7.5 m) up to 4.9 m, its five 4.6 m chords a side. Corners, junctions and ends are
//! not bent: their arms turn whole, and their ruts and borders run past their ports too. Where a kit decal is shortened, it is by no more
//! than 2.2 times (the curve's 4.6 m ruts from the kit's 10 m). Corners and
//! junctions are their arms' ruts run on to cross in the middle, worn where they overlap.

use std::path::{Path, PathBuf};

use crate::road_pieces::{self, ANCHOR_ATTR, CURVE_RADIUS, CURVE_TURN, PIECES, UNIT, VARIANTS, VERGE, WIDTH, base_guides};

/// The road type's folder and name.
pub const TYPE: &str = "dirt";

/// The kit's decals the pieces are made of (`rigidmodels\decals\battle\<name>.rigid_model_v2`).
pub const DECALS: [&str; 9] = [
    "path_1", "border_1", "border_2", "global_terrain_rocks_01", "global_terrain_rocks_04", "global_terrain_puddle_02", "global_terrain_erossion_12",
    "global_terrain_erossion_03", "global_terrain_path_04",
];

/// The kit's bank of ground along a road (`global_ground_pathborder`, a building): a ridge of grassy ground along its x, 18.6 m long and
/// about 10 m across, its crest over its origin. The kit's prefabs lay it at a road's edge, its +z out of the road, rolled 10 degrees to
/// raise its outer side, 0.4 m down; their roads lie on slopes, which shows it. On the level ground a generated road levels, it sank out
/// of sight there: here it lies out on the verge, [`BANK_OUT`] past the edge, [`BANK_DOWN`] down, rolled as the kit's.
pub const BANK_KEY: &str = "global_ground_pathborder";
const BANK_ROLL: f64 = 0.174_533;
pub const BANK_OUT: f64 = 2.5;
pub const BANK_DOWN: f64 = 0.1;

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

/// Whether every dirt piece is in a prefab folder as this generation makes it, or as the user made it.
#[must_use]
pub fn all_written(prefab_root: &Path) -> bool {
    !road_pieces::generated_stale(prefab_root, TYPE, &pieces())
}

/// The kit's fence the dirt road's fence piece is (`lowfence_3`, about 5 m long along its x: the kit's road prefabs space them so).
pub const FENCE_KEY: &str = "lowfence_3";

/// How tall a decal's box is (metres): enough for a road's banks.
const DEPTH: f64 = 15.0;

/// The most a part of a piece may be off the road's curve where the piece is bent the most (metres).
pub const MAX_SAGITTA: f64 = 0.4;

/// The longest a part may be along the road in a piece (metres), by [`MAX_SAGITTA`] at the tightest curve the piece is bent to; unbent
/// pieces have no limit.
#[must_use]
pub fn max_part_length(piece: &str) -> f64 {
    let piece = road_pieces::varies(piece).unwrap_or(piece);
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

/// How far a piece's ruts and borders run past its ports (metres): into the next piece, so that the two overlap by twice this. A decal
/// fades out at its ends, and end to end the fades showed as bands of grass across the road (user, 2026-11-08: still too little at 0.25).
pub const OVERLAP: f64 = 0.75;

/// The ruts' and the borders' width (metres): a border overlaps the ruts beside it, so no grass shows between them.
const RUT: f64 = 2.6;
const BORDER: f64 = 2.0;

/// The ruts down a stretch along z, from `z0` to `z1`: two strips side by side, the left one turned end for end so that they do not repeat.
fn ruts(z0: f64, z1: f64) -> Vec<Vec<u8>> {
    let (x, z, length) = (WIDTH * 0.25 - 0.2, (z0 + z1) * 0.5, z1 - z0);
    vec![decal("path_1", [-x, z], DOWN, length, RUT, None), decal("path_1", [x, z], UP, length, RUT, None)]
}

/// The ruts down a stretch along x, from `x0` to `x1` (a branch to the right or left).
fn ruts_x(x0: f64, x1: f64) -> Vec<Vec<u8>> {
    let (z, x, length) = (WIDTH * 0.25 - 0.2, (x0 + x1) * 0.5, x1 - x0);
    vec![decal("path_1", [x, -z], LEFT, length, RUT, None), decal("path_1", [x, z], RIGHT, length, RUT, None)]
}

/// An edge's border from one place to another along z (`side`: its x) or along x (`side`: its z), its lip facing out of the road (`out`:
/// the side the road is not on, + or -).
fn border_z(name: &str, side: f64, z0: f64, z1: f64) -> Vec<u8> {
    // The lip is on the right of the way the decal's x runs: up on the right edge, down on the left.
    let way = if side > 0.0 { UP } else { DOWN };
    decal(name, [side, (z0 + z1) * 0.5], way, z1 - z0, BORDER, None)
}

fn border_x(name: &str, side: f64, x0: f64, x1: f64) -> Vec<u8> {
    let way = if side > 0.0 { LEFT } else { RIGHT };
    decal(name, [(x0 + x1) * 0.5, side], way, x1 - x0, BORDER, None)
}

/// A bank along an edge (`side`: its x), its middle at `z`: as the kit lays one ([`BANK_KEY`]), held to the edge (`edge`).
fn bank(side: f64, z: f64) -> Vec<u8> {
    // Its x along the road, its +z out of it (`world_transform::euler_matrix`: Ry(pi/2) takes z to +x, Ry(-pi/2) to -x).
    let yaw = if side > 0.0 { std::f64::consts::FRAC_PI_2 } else { -std::f64::consts::FRAC_PI_2 };
    let n = crate::ted_scene::format_number;
    format!(
        "<BUILDING_INSTANCE flags=\" OIF_VISIBLE|OIF_ACTIVE\" key=\"{BANK_KEY}\" name=\"{BANK_KEY}\" surface_mode=\"MSM_FREEFORM\" position=\"vector({},{},{})\" orientation=\"euler({},{},0)\" {ANCHOR_ATTR}=\"edge\"/>\n",
        n((side + side.signum() * BANK_OUT) * UNIT), n(-BANK_DOWN * UNIT), n(z * UNIT), n(yaw), n(BANK_ROLL)
    ).into_bytes()
}

/// Where roads meet the middle is worn all over, not grass between two pairs of ruts: a rut laid across it diagonally (`way`, not a unit
/// vector), where the piece has it.
fn worn_middle(way: [f64; 2]) -> Vec<u8> {
    let l = way[0].hypot(way[1]);
    decal("path_1", [0.0, 0.0], [way[0] / l, way[1] / l], 5.5, 3.2, Some("fixed"))
}

/// Decals along a quarter circle about `c` (a corner of the piece's middle square, its arms 6 out), radius `r`, from one arm's port to
/// the other's, in `n` chords running a little past each other: as a road (`surface`), so that a wider road widens them and each goes with
/// the arm it is nearer. `lip_to_c`: an edge's lip towards `c` (an inside edge: the grass is there) or away from it (an outside edge);
/// none: ruts, no lip.
fn quarter(out: &mut Vec<Vec<u8>>, c: [f64; 2], r: f64, n: u32, name: &str, width: f64, lip_to_c: Option<bool>) {
    // The quarter faces the piece's middle from `c`: (qx, qz) its signs.
    let (qx, qz) = (-c[0].signum(), -c[1].signum());
    let step = std::f64::consts::FRAC_PI_2 / f64::from(n);
    for k in 0..n {
        let t = (f64::from(k) + 0.5) * step;
        let at = [c[0] + qx * r * t.cos(), c[1] + qz * r * t.sin()];
        let way = [-qx * t.sin(), qz * t.cos()];
        // A border's lip is on the right of the way its x runs: towards c when qz = -qx going this way.
        let right_to_c = qz == -qx;
        let way = match lip_to_c {
            Some(to_c) if to_c != right_to_c => [-way[0], -way[1]],
            _ => way,
        };

        out.push(decal(name, at, way, r * step + 2.0 * OVERLAP, width, Some("surface")));
    }
}

/// The decals of a piece of the dirt road.
#[must_use]
pub fn parts(piece: &str) -> Vec<Vec<u8>> {
    let h = WIDTH * 0.5;
    let (s, e, j) = (road_pieces::STRAIGHT * 0.5 + OVERLAP, road_pieces::END * 0.5, road_pieces::JUNCTION * 0.5 + OVERLAP);
    let j0 = road_pieces::JUNCTION * 0.5;
    let mut out = vec![];
    match piece {
        "straight" | "ford" => {
            out.extend(ruts(-s, s));
            out.push(border_z("border_1", -h, -s, s));
            out.push(border_z("border_1", h, -s, s));
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

        // The straight's variants (`road_pieces::VARIANTS`), the kit's other things along its roads in each: its banks of ground on one side,
        // the other or both, its worn patch (`path_04`), a puddle, erosion beside the road, rocks. Laid one or another along a road.
        "straight_2" => {
            out.extend(ruts(-s, s));
            out.push(border_z("border_1", -h, -s, s));
            out.push(border_z("border_1", h, -s, s));
            out.push(bank(h, 0.0));
            out.push(decal("global_terrain_path_04", [-0.8, -1.2], UP, 7.5, 3.5, None));
            out.push(decal("global_terrain_erossion_03", [-h - 1.4, 0.8], UP, 6.0, 2.5, None));
            out.push(decal("global_terrain_rocks_04", [-h + 0.4, 2.4], UP, 2.5, 2.5, None));
        }

        "straight_3" => {
            out.extend(ruts(-s, s));
            out.push(border_z("border_1", -h, -s, s));
            out.push(border_z("border_1", h, -s, s));
            out.push(bank(-h, 0.0));
            out.push(decal("global_terrain_puddle_02", [0.9, -1.4], RIGHT, 2.5, 5.0, None));
            out.push(decal("global_terrain_erossion_12", [h + VERGE * 0.5, -1.0], RIGHT, 2.5, 6.0, None));
            out.push(decal("global_terrain_rocks_01", [h - 0.5, 2.5], UP, 2.5, 2.5, None));
        }

        "straight_4" => {
            out.extend(ruts(-s, s));
            out.push(border_z("border_1", -h, -s, s));
            out.push(border_z("border_1", h, -s, s));
            out.push(bank(-h, 0.5));
            out.push(bank(h, -0.5));
            out.push(decal("global_terrain_path_04", [1.0, 1.5], DOWN, 7.5, 3.5, None));
            out.push(decal("global_terrain_rocks_04", [0.3, -2.0], UP, 2.5, 2.5, None));
        }

        "straight_5" => {
            out.extend(ruts(-s, s));
            out.push(border_z("border_1", -h, -s, s));
            out.push(border_z("border_1", h, -s, s));
            out.push(decal("global_terrain_erossion_03", [h + 1.3, -0.6], DOWN, 6.0, 2.5, None));
            out.push(decal("global_terrain_rocks_04", [-1.5, 2.8], UP, 2.5, 2.5, None));
            out.push(decal("global_terrain_rocks_04", [h + 0.9, -1.8], UP, 2.5, 2.5, None));
            out.push(decal("global_terrain_rocks_01", [-h - 0.6, -2.6], UP, 2.5, 2.5, None));
        }

        "end" => {
            // The road stops at z = -2: worn ground fading over the end, its edges closed across the back.
            out.extend(ruts(-e, e + OVERLAP));
            out.push(border_z("border_1", -h, -e, e + OVERLAP));
            out.push(border_z("border_1", h, -e, e + OVERLAP));
            out.push(border_x("border_2", -e, -h - 0.5, h + 0.5));
            out.push(decal("global_terrain_rocks_04", [-1.8, -2.4], UP, 2.5, 2.5, None));
            out.push(decal("global_terrain_rocks_01", [2.0, -2.6], UP, 2.5, 2.5, None));
        }

        "curve" => {
            // Along the arc, five 4.6 m ruts and borders a side, chords of it overlapping each other, the end ones past the ports.
            let (c, r) = ([CURVE_RADIUS, 0.0], CURVE_RADIUS);
            let at = |rr: f64, t: f64| [c[0] - rr * t.cos(), c[1] + rr * t.sin()];
            let way = |t: f64| [t.sin(), t.cos()];
            let back = |t: f64| [-t.sin(), -t.cos()];
            for i in 0..5 {
                let t = (f64::from(i) - 2.0) * CURVE_TURN / 5.0;
                let w = if i % 2 == 0 { back(t) } else { way(t) };
                out.push(decal("path_1", at(r + 1.3, t), w, 4.6, RUT, None));
                out.push(decal("path_1", at(r - 1.3, t), way(t), 4.6, RUT, None));
                out.push(decal("border_1", at(r + h, t), back(t), 4.6, BORDER, None));
                out.push(decal("border_1", at(r - h, t), way(t), 4.6, BORDER, None));
            }

            out.push(decal("global_terrain_rocks_04", at(r - h + 0.6, -0.2), UP, 2.5, 2.5, None));
            out.push(decal("global_terrain_rocks_01", at(r + h + 0.5, 0.25), UP, 2.5, 2.5, None));
            out.push(decal("global_terrain_erossion_12", at(r + h + VERGE * 0.6, -0.05), RIGHT, 2.5, 4.0, None));
        }

        "corner" => {
            // From the back (-z) turning right (+x), as a quarter circle from port to port about the inside of the turn (user 2026-11-08:
            // the L of two arms' ruts and edges was too abrupt): the ruts and the edges along it in chords, the outer edge's lip out of
            // the road, the inner's towards the turn's middle.
            let c = [j0, -j0];
            quarter(&mut out, c, j0 + h, 4, "border_1", BORDER, Some(false));
            quarter(&mut out, c, j0 - h, 2, "border_2", BORDER, Some(true));
            quarter(&mut out, c, j0 + 1.3, 3, "path_1", RUT, None);
            quarter(&mut out, c, j0 - 1.3, 2, "path_1", RUT, None);
            out.push(decal("global_terrain_rocks_04", [-h - 1.2, h + 1.2], UP, 2.5, 2.5, None));
            out.push(decal("global_terrain_rocks_01", [h + 0.6, -h - 0.6], UP, 2.5, 2.5, None));
        }

        "fence" => {
            // One fence along the road, its x along it (as the kit's prefabs have them), where the fence line is. Its model runs from its
            // pivot along its x (0 to 5.25 m): the pivot at the piece's back port, so that it spans the piece.
            out.push(format!(
                "<BUILDING_INSTANCE flags=\" OIF_VISIBLE|OIF_ACTIVE\" key=\"{FENCE_KEY}\" name=\"{FENCE_KEY}\" surface_mode=\"MSM_LANDSCAPE\" position=\"vector(0,0,{})\" orientation=\"euler({},0,0)\" {ANCHOR_ATTR}=\"fixed\"/>\n",
                crate::ted_scene::format_number(-road_pieces::FENCE * 0.5 * UNIT),
                crate::ted_scene::format_number(-std::f64::consts::FRAC_PI_2)
            ).into_bytes());
        }

        "t_junction" => {
            // The through road along z, straight, its far side's edge unbroken; the branch to the right (+x) turns into it either way
            // (user 2026-11-08: the curves of the road, not two pairs of ruts crossing): its ruts and its inside edges on quarter circles
            // about the two inside corners, as the corner's.
            out.extend(ruts(-j, 0.25));
            out.extend(ruts(-0.25, j));
            out.push(border_z("border_1", -h, -j, 0.25));
            out.push(border_z("border_1", -h, -0.25, j));
            for c in [[j0, -j0], [j0, j0]] {
                quarter(&mut out, c, j0 - h, 2, "border_2", BORDER, Some(true));
                quarter(&mut out, c, j0 - 1.3, 2, "path_1", RUT, None);
                quarter(&mut out, c, j0 + 1.3, 3, "path_1", RUT, None);
            }

            out.push(worn_middle([1.0, 1.0]));
            out.push(decal("global_terrain_rocks_04", [-h - 0.5, -0.8], UP, 2.5, 2.5, None));
        }

        "x_junction" => {
            // Both roads straight through, and in each quarter a turn from one into the other: its inside edge and a rut on quarter
            // circles about the inside corner, as the corner's.
            out.extend(ruts(-j, 0.25));
            out.extend(ruts(-0.25, j));
            out.extend(ruts_x(-j, 0.25));
            out.extend(ruts_x(-0.25, j));
            for c in [[j0, -j0], [j0, j0], [-j0, j0], [-j0, -j0]] {
                quarter(&mut out, c, j0 - h, 2, "border_2", BORDER, Some(true));
                quarter(&mut out, c, j0 - 1.3, 2, "path_1", RUT, None);
            }

            out.push(worn_middle([1.0, 1.0]));
            out.push(worn_middle([1.0, -1.0]));
            out.push(decal("global_terrain_rocks_01", [h + 0.9, h + 0.9], UP, 2.5, 2.5, None));
        }

        _ => {}
    }

    out
}

/// The dirt pieces' contents: the base's guides and the decals.
#[must_use]
pub fn pieces() -> Vec<(&'static str, Vec<Vec<u8>>)> {
    PIECES.iter().copied().chain(VARIANTS.iter().map(|v| v.0)).map(|p| {
        let mut children = base_guides(p);
        children.extend(parts(p));
        (p, children)
    }).collect()
}

/// Writes the dirt pieces into a prefab folder (the projects'): each where it is missing, or an older generation nobody edited
/// (`road_pieces::write_generated`). Returns the files written.
///
/// # Errors
///
/// A file can't be written.
pub fn write(prefab_root: &Path) -> Result<Vec<PathBuf>, String> {
    road_pieces::write_generated(prefab_root, TYPE, &pieces())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::road_pieces::{Anchor, group_text, key, read};

    #[test]
    fn every_dirt_piece_reads_and_its_parts_fit() {
        for piece in PIECES.into_iter().chain(VARIANTS.iter().map(|v| v.0)).filter(|&p| p != "fence") {
            let mut children = base_guides(piece);
            children.extend(parts(piece));
            let p = read(&group_text(&key(TYPE, piece), &children)).unwrap();
            assert!(p.notes.is_empty(), "{piece}: {:?}", p.notes);
            assert!(p.parts.len() >= 6, "{piece}: {} parts", p.parts.len());
            // No building on the road: the banks alone, held to its edges.
            assert!(p.parts.iter().filter(|q| q.solid).all(|q| q.key == BANK_KEY && q.anchor == Anchor::Edge), "{piece}");

            // Every decal of the kit's road set, and only those.
            assert!(p.parts.iter().filter(|q| !q.solid).all(|q| DECALS.iter().any(|d| q.key.ends_with(&format!("\\{d}.rigid_model_v2")))), "{piece}");

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

                // The corner's edges are arcs, laid as the road is (`surface`).
                if name.starts_with("border_1") && piece != "corner" {
                    assert_eq!(q.anchor, Anchor::Edge, "{piece}: {q:?}");
                }
            }
        }
    }

    #[test]
    fn a_bank_lies_along_its_edge_its_outer_side_raised() {
        let mut children = base_guides("straight_2");
        children.extend(parts("straight_2"));
        let p = read(&group_text(&key(TYPE, "straight_2"), &children)).unwrap();
        let b = p.parts.iter().find(|q| q.key == BANK_KEY).unwrap();
        assert!((b.across - WIDTH * 0.5 - BANK_OUT).abs() < 1e-6 && (b.up + BANK_DOWN).abs() < 1e-6, "{b:?}");
        assert!(b.turn.sin().abs() < 1e-6, "its x along the road: {b:?}");
        // Its +z out of the road (+x on the right edge), rising.
        let z = b.rotation * wce_foundation::math::Double3::new(0.0, 0.0, 1.0);
        assert!(z.x > 0.9 && z.y > 0.1, "{z:?}");
    }

    #[test]
    fn the_fence_is_one_building_along_its_line() {
        let mut children = base_guides("fence");
        children.extend(parts("fence"));
        let p = read(&group_text(&key(TYPE, "fence"), &children)).unwrap();
        assert!(p.notes.is_empty(), "{:?}", p.notes);
        assert_eq!(p.parts.len(), 1);
        let q = &p.parts[0];
        assert!(q.solid && q.key == FENCE_KEY && q.anchor == Anchor::Fixed, "{q:?}");
        assert!(q.turn.sin().abs() < 1e-6 && q.across.abs() < 1e-9, "its x along the line: {q:?}");
        // Its pivot (the model's end) at the back port: the model runs forward from it over the piece.
        assert!(q.along.abs() < 1e-9, "its pivot at the back: {q:?}");
    }

    #[test]
    fn the_corner_runs_round_from_port_to_port() {
        let mut children = base_guides("corner");
        children.extend(parts("corner"));
        let p = read(&group_text(&key(TYPE, "corner"), &children)).unwrap();
        // The outer edge's chords lie on a quarter circle of radius 9 about (6, -6): none at the L's corner (-3, 3) any more.
        let outer = p.parts.iter().filter(|q| q.key.contains("border_1")).collect::<Vec<_>>();
        assert_eq!(outer.len(), 4);
        for q in outer {
            let r = (q.at[0] - 6.0).hypot(q.at[1] + 6.0);
            assert!((r - 9.0).abs() < 1e-6, "{r}");
        }
    }

    #[test]
    fn the_ruts_run_along_the_road() {
        let mut children = base_guides("straight");
        children.extend(parts("straight"));
        let p = read(&group_text("road_dirt_straight", &children)).unwrap();
        for q in p.parts.iter().filter(|q| q.key.contains("path_1") || q.key.contains("border_1")) {
            assert!(q.turn.sin().abs() < 1e-6, "{q:?}");
            assert!((q.size[0] - 9.5).abs() < 1e-6, "{q:?}");
        }
    }

    #[test]
    fn the_limits_follow_the_bends() {
        assert!((max_part_length("straight") - 9.89).abs() < 0.01, "{}", max_part_length("straight"));
        assert!((max_part_length("curve") - 4.9).abs() < 0.01, "{}", max_part_length("curve"));
        assert_eq!(max_part_length("corner"), f64::MAX);
    }

    #[test]
    fn written_once_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(write(dir.path()).unwrap().len(), PIECES.len() + VARIANTS.len());
        assert!(write(dir.path()).unwrap().is_empty());
        let lib = crate::prefabs::PrefabLibrary::scan_folders(&[dir.path().to_path_buf()]);
        let curve = lib.load("road_dirt_curve").unwrap();
        assert_eq!(read(&curve.group_text).unwrap().arms.len(), 1);
        assert_eq!(lib.get("road_dirt_curve").unwrap().folder, "roads_proc/dirt");
    }
}
