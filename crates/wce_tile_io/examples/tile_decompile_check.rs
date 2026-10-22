//! The decompiler (`attila::decompile`, docs/plans/tile_roundtrip.md Phase 2 step 3) on the packs' exported tiles, against the kit's raw
//! tiles where the kit has the same tile.
//!
//!   cargo run --release -p wce_tile_io --example tile_decompile_check -- <game folder> [--filter TEXT] [--limit N] [--out DIR]
//!       [--trees LIST]
//!
//! Per tile folder of the packs: read (`attila.tile_binary`), decompile, write as a TEd project and read it back (`attila.ted_project`).
//! With the kit's raw tile of the same location: the entry (byte for byte), the heights (metres, inside the border), the buildings (paired
//! by key and world position within 5 cm, then the attributes). `--out` writes the decompiled projects below DIR (`raw_data/terrain/tiles/
//! ...` and the entries in `.../_tile_database/TILES`). Prints totals, the losses by kind, the first problems.
//!
//! The kit is not always the version the packs were exported from, nor BOB's output (the heights: `height_map_0.png` is BOB's input), so
//! its agreement is evidence, not a gate. Environment: `WCE_DECOMPILE_EXAMPLES` (examples of differing attributes), `WCE_DECOMPILE_HEIGHTS`
//! (tiles whose heights differ), `WCE_DECOMPILE_ROTATIONS` (differing rotations by kind and tile family), `WCE_DECOMPILE_PROBLEMS=N` (print N
//! problems, default 30).

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rayon::prelude::*;
use rpfm_lib::files::Decodeable;
use rpfm_lib::files::pack::Pack;
use rpfm_lib::files::tile_database::TileDatabase;
use rpfm_lib::games::supported_games::SupportedGames;
use wce_tile::building_db::BuildingDb;
use wce_tile::ted_scene::parse_vec3;
use wce_tile::tile_project::TileProject;
use wce_tile::world_transform::{euler_from_matrix, euler_matrix, world_transform};
use wce_tile_io::attila::decompile::{Context, decompile, destruction_levels, split_location};
use wce_tile_io::attila::{TedProject, TileBinary};
use wce_tile_io::{TileFiles, TileFormat};
use wce_tile_model::{Blob, MapKind, Node, Samples, Value};

#[derive(Default)]
struct Totals {
    tiles: usize,
    failed: usize,
    with_kit: usize,
    entry_same: usize,
    entry_differ: usize,
    height_samples: usize,
    height_within_1cm: usize,
    flat_heights: usize,
    final_samples: usize,
    final_within_1cm: usize,
    final_tiles: usize,
    buildings: usize,
    kit_buildings: usize,
    paired: usize,
    attr_same: BTreeMap<String, (usize, usize)>,

    /// Per class: decompiled, in the kit, paired.
    classes: BTreeMap<String, (usize, usize, usize)>,
    losses: BTreeMap<String, usize>,
    problems: Vec<String>,
}

/// Every polygon of a decompiled scene with its world points (x, z): the scene's logic nodes carry no rotation, so a point is the sum of
/// its ancestors' positions and its own.
/// A polygon's points (x, z).
type Polygon = Vec<(f64, f64)>;

fn our_polygons<'a>(n: &'a Node, origin: (f64, f64), out: &mut Vec<(&'a Node, Vec<(f64, f64)>)>) {
    let p = n.get("position").and_then(Value::as_vec3).unwrap_or_default();
    let here = (origin.0 + p[0], origin.1 + p[2]);
    if let Some(points) = n.child("points") {
        let pts = points.children.iter().filter_map(|c| c.get("position").and_then(Value::as_vec3)).map(|q| (here.0 + q[0], here.1 + q[2])).collect();
        out.push((n, pts));
    }

    for c in &n.children {
        our_polygons(c, if n.class == "children" || n.class == "points" { origin } else { here }, out);
    }
}

/// A value as tile.ted writes it.
fn crate_value_text(class: &str, attr: &str, v: &Value) -> String {
    wce_tile_io::xml::value_text(&wce_tile_io::attila::schema::TedSchema, wce_tile_model::Sym::new(class), wce_tile_model::Sym::new(attr), v)
}

/// The first difference between two trees, as a path.
fn first_diff(a: &Node, b: &Node, path: &str) -> Option<String> {
    if a.class != b.class {
        return Some(format!("{path}: {} / {}", a.class, b.class));
    }

    for (pa, pb) in a.props().iter().zip(b.props()) {
        if pa != pb {
            return Some(format!("{path}/{}.{}: {:?} / {:?}", a.class, pa.name, pa.value, pb.value));
        }
    }

    if a.props().len() != b.props().len() || a.children.len() != b.children.len() {
        return Some(format!("{path}/{}: {} / {} props, {} / {} children", a.class, a.props().len(), b.props().len(), a.children.len(), b.children.len()));
    }

    a.children.iter().zip(&b.children).enumerate().find_map(|(i, (x, y))| first_diff(x, y, &format!("{path}/{}[{i}]", a.class)))
}

/// Every BUILDING_INSTANCE of a decompiled scene (top level of their layers).
fn buildings(scene: &Node) -> Vec<&Node> {
    let mut out = vec![];
    scene.walk(&mut |n| {
        if n.class == "BUILDING_INSTANCE" {
            out.push(n);
        }
    });
    out
}

fn check(name: &str, folder: &str, files: &TileFiles, ctx: &Context, kit: &Path, out: Option<&Path>, t: &Mutex<Totals>) {
    let mut local = Totals::default();
    let result = (|| -> Result<(), String> {
        let (binary, _) = TileBinary.read(files).map_err(|e| e.to_string())?;
        let (raw, losses) = decompile(&binary, folder, ctx)?;
        for l in &losses.notes {
            *local.losses.entry(l.split(':').next().unwrap_or(l).to_owned()).or_default() += 1;
        }

        let (written, _) = TedProject.write(&raw).map_err(|e| e.to_string())?;
        let (back, _) = TedProject.read(&written).map_err(|e| e.to_string())?;
        if back.semantic() != raw.semantic() {
            let what = match (&raw.scene, &back.scene, &raw.entry, &back.entry) {
                (Some(a), Some(b), _, _) if a.without_trivia() != b.without_trivia() => first_diff(a, b, ""),
                (_, _, Some(a), Some(b)) if a.root.without_trivia() != b.root.without_trivia() => first_diff(&a.root, &b.root, "entry"),
                _ => Some(format!("maps {:?} / {:?}", raw.maps.iter().map(|m| (m.kind, m.raster.width)).collect::<Vec<_>>(), back.maps.iter().map(|m| (m.kind, m.raster.width)).collect::<Vec<_>>())),
            };

            local.problems.push(format!("{name}: the written project reads into another model: {}", what.unwrap_or_default()));
        }

        if let Some(out) = out {
            let dir = out.join("raw_data").join(folder);
            let (location, _) = split_location(folder);
            let kind = location.split('\\').nth(2).unwrap_or("battle");
            written.write_to(&dir, Some(&out.join("raw_data/terrain/tiles").join(kind).join("_tile_database/TILES"))).map_err(|e| e.to_string())?;
        }

        // Against the kit's raw tile.
        let kit_dir = kit.join("raw_data").join(folder);
        if !kit_dir.join("tile.ted").is_file() {
            return Ok(());
        }

        local.with_kit += 1;
        let Ok(project) = TileProject::open(&kit_dir) else { return Ok(()) };
        if let (Some(e), Some(entry)) = (&project.entry, &written.entry) {
            // The kit's entry with the variation's min_height / scale the decompiler sets would be the same file: compare all but those.
            let ours = String::from_utf8_lossy(&entry.1.bytes().unwrap_or_default()).into_owned();
            let theirs = String::from_utf8_lossy(&e.save()).into_owned();
            let strip = |s: &str| s.lines().filter(|l| !l.contains("<VARIATION ")).collect::<Vec<_>>().join("\n");
            if strip(&ours) == strip(&theirs) {
                local.entry_same += 1;
            } else {
                local.entry_differ += 1;
            }
        }

        // Heights in metres, inside the border.
        if let (Some(field), Some(m)) = (&project.height, raw.map(MapKind::Height, 0))
            && let Samples::U16(ours) = &m.raster.samples
            && field.width() == m.raster.width
            && field.height() == m.raster.height
        {
            let entry = raw.entry.as_ref().expect("entry");
            let v = entry.root.child("VARIATIONS").and_then(|vs| vs.children.iter().find(|v| v.get("location").and_then(Value::as_str) == raw.variation.as_deref()));
            let (min, scale) = (v.and_then(|v| v.get("min_height")).and_then(Value::as_f64).unwrap_or(0.0), v.and_then(|v| v.get("scale")).and_then(Value::as_f64).unwrap_or(1.0));
            let (w, h) = (field.width() as usize, field.height() as usize);
            // A shipped CHMF of range ~0 is a flat tile (its terrain mesh is flat too; the kit's sources have relief): counted apart.
            if scale.abs() < 1e-6 {
                local.flat_heights += 1;
            }

            let ppc = (w / (project.info.cells_x as usize + 2)).max(1);
            let data = field.samples();
            let (mut n, mut good, mut diffs) = (0, 0, vec![]);
            for y in (ppc..h - ppc).step_by(7) {
                for x in (ppc..w - ppc).step_by(7) {
                    let kit_m = f64::from(data[y * w + x]) / 65535.0 * project.info.scale + project.info.min_height;
                    let our_m = f64::from(ours[y * w + x]) / 65535.0 * scale + min;
                    n += 1;
                    good += usize::from((kit_m - our_m).abs() < 0.01);
                    diffs.push(our_m - kit_m);
                }
            }

            // BOB's own heights of its last export of the kit tile (final_heights.dds: R32F, the CHMF's size, metres).
            if let Ok(dds) = std::fs::read(kit_dir.join("final_heights.dds"))
                && dds.len() >= 128
                && u32::from_le_bytes(dds[84..88].try_into().unwrap()) == 114
            {
                let (fh, fw) = (u32::from_le_bytes(dds[12..16].try_into().unwrap()) as usize, u32::from_le_bytes(dds[16..20].try_into().unwrap()) as usize);
                if fw + 2 * ppc - 1 == w && fh + 2 * ppc - 1 == h && dds.len() >= 128 + fw * fh * 4 {
                    let (mut fn_, mut fg) = (0, 0);
                    for y in (0..fh).step_by(3) {
                        for x in (0..fw).step_by(3) {
                            let o = 128 + (y * fw + x) * 4;
                            let theirs = f64::from(f32::from_le_bytes(dds[o..o + 4].try_into().unwrap()));
                            let our_m = f64::from(ours[(y + ppc) * w + x + ppc]) / 65535.0 * scale + min;
                            fn_ += 1;
                            fg += usize::from((theirs - our_m).abs() < 0.01);
                        }
                    }

                    local.final_tiles += 1;
                    local.final_samples += fn_;
                    local.final_within_1cm += fg;
                    if fg * 2 < fn_ && std::env::var_os("WCE_DECOMPILE_HEIGHTS").is_some() {
                        local.problems.push(format!("{name}: final_heights.dds {fg} / {fn_} within 1 cm"));
                    }
                }
            }

            if scale.abs() >= 1e-6 {
                local.height_samples += n;
                local.height_within_1cm += good;
            }

            if scale.abs() >= 1e-6 && good * 2 < n && std::env::var_os("WCE_DECOMPILE_HEIGHTS").is_some() {
                diffs.sort_by(f64::total_cmp);
                local.problems.push(format!(
                    "{name}: heights {good} / {n} within 1 cm; ours - kit median {:.3} (p10 {:.3}, p90 {:.3}); kit min {} scale {}, ours min {min} scale {scale}; {w} x {h}",
                    diffs[diffs.len() / 2], diffs[diffs.len() / 10], diffs[diffs.len() * 9 / 10], project.info.min_height, project.info.scale
                ));
            }

        }

        // Buildings: pair on key + world position (scene units), then compare the attributes.
        if let Some(scene) = &raw.scene {
            let ours = buildings(scene);
            local.buildings += ours.len();
            let sc = &project.scene;
            let kit_b = (0..sc.doc().elements().len()).filter(|&e| sc.type_of(e) == "BUILDING_INSTANCE").collect::<Vec<_>>();
            local.kit_buildings += kit_b.len();
            let mut by_key: BTreeMap<String, Vec<(usize, wce_foundation::math::Double3)>> = BTreeMap::new();
            for &e in &kit_b {
                by_key.entry(sc.doc().get(e, "key").unwrap_or_default()).or_default().push((e, world_transform(sc, e).position));
            }

            for b in ours {
                let key = b.get("key").and_then(Value::as_str).unwrap_or_default();
                let p = b.get("position").and_then(Value::as_vec3).unwrap_or_default();
                let Some(cands) = by_key.get_mut(key) else { continue };
                let ours_m = {
                    let o = b.get("orientation").and_then(Value::as_vec3).unwrap_or_default();
                    euler_matrix(wce_foundation::math::Double3::new(o[0], o[1], o[2]))
                };

                let rot_diff = |e: usize| {
                    let t = world_transform(sc, e).rotation;
                    (0..3).flat_map(|r| (0..3).map(move |k| (r, k))).map(|(r, k)| (ours_m.m[r][k] - t.m[r][k]).abs()).fold(0.0, f64::max)
                };

                // Copies stacked at one place (fences): the same rotation first, then the most attributes alike, then the closest rotation.
                let alike = |e: usize| {
                    ["destruction_level", "burn", "indestructible", "opacity", "building_tech_level"]
                        .iter()
                        .filter(|a| b.get(a).map(|v| crate_value_text("BUILDING_INSTANCE", a, v)) == sc.doc().get(e, a))
                        .count()
                };
                let rank = |e: usize| (rot_diff(e) >= 0.001, std::cmp::Reverse(alike(e)));
                let Some(ci) = cands
                    .iter()
                    .enumerate()
                    .filter(|(_, (_, q))| (q.x - p[0]).hypot(q.z - p[2]) < 0.1 && (q.y - p[1]).abs() < 0.1)
                    .min_by(|a, b| rank(a.1.0).cmp(&rank(b.1.0)).then_with(|| rot_diff(a.1.0).total_cmp(&rot_diff(b.1.0))))
                    .map(|(i, _)| i)
                else {
                    continue;
                };
                let (e, _) = cands.swap_remove(ci);
                local.paired += 1;
                for attr in ["indestructible", "toggleable", "burn", "destruction_level", "weak_wall", "ai_breachable", "dockable", "opacity", "building_tech_level"] {
                    let ours = b.get(attr).map(|v| match v {
                        Value::Bool(x) => x.to_string(),
                        Value::Int(x) => x.to_string(),
                        Value::Float(x) => format!("{x}"),
                        Value::Str(x) => x.clone(),
                        other => format!("{other:?}"),
                    });

                    // Older tile.ted files leave out attributes added later; TEd reads them as their default.
                    let theirs = sc.doc().get(e, attr).or_else(|| match attr {
                        "toggleable" => Some("false".to_owned()),
                        _ => None,
                    });
                    let c = local.attr_same.entry(attr.to_owned()).or_default();
                    if ours == theirs {
                        c.0 += 1;
                    } else {
                        c.1 += 1;
                        if attr == "destruction_level" && std::env::var_os("WCE_DECOMPILE_LEVELS").is_some() {
                            *local.losses.entry(format!("destruction_level differs: {key} ours {} kit {}", ours.clone().unwrap_or_default(), theirs.clone().unwrap_or_default())).or_default() += 1;
                        }

                        if c.1 <= 3 && std::env::var_os("WCE_DECOMPILE_EXAMPLES").is_some() {
                            local.problems.push(format!("{name}: {key} {attr}: ours {ours:?}, kit {theirs:?}, kit element {e} {}", sc.layer_path(e)));
                        }
                    }
                }

                // The same rotation (angles can differ by turns or at gimbal lock): the largest difference of the matrices.
                let ours = b.get("orientation").and_then(Value::as_vec3).unwrap_or_default();
                let m = euler_matrix(wce_foundation::math::Double3::new(ours[0], ours[1], ours[2]));
                let theirs = world_transform(sc, e).rotation;
                let c = local.attr_same.entry("orientation (world rotation, 0.001)".to_owned()).or_default();
                let diff = (0..3).flat_map(|r| (0..3).map(move |k| (r, k))).map(|(r, k)| (m.m[r][k] - theirs.m[r][k]).abs()).fold(0.0, f64::max);
                if diff < 0.001 {
                    c.0 += 1;
                } else {
                    c.1 += 1;
                    if c.1 <= 3 && std::env::var_os("WCE_DECOMPILE_EXAMPLES").is_some() {
                        local.problems.push(format!("{name}: {key} orientation: ours {ours:?}, kit world {:?}, file {:?} ({})", euler_from_matrix(&theirs), sc.doc().get(e, "orientation"), sc.layer_path(e)));
                    }
                }
            }
        }

        // The other instances: pair on (class, identifying attribute, world position), then compare attributes.
        if let Some(scene) = &raw.scene {
            let sc = &project.scene;
            for (class, id, attrs) in [
                ("ProjectedDecalInstance", "key", &["scale", "parallax_scale", "tiled"][..]),
                ("SIMPLE_MESH_INSTANCE", "key", &["scale"][..]),
                ("VFXInstance", "vfx", &["scale", "autoplay", "use_in_outfield", "seasons"][..]),
                ("POINT_LIGHT_INSTANCE", "name", &["colour", "radius", "colour_scale", "colour_min", "random_offset", "animation_type", "falloff_type", "animation_speed_scale"][..]),
                ("VEGETATION_INSTANCE", "key", &["scale"][..]),
                ("CaptureLocationInstance", "database_type", &["flag_facing", "type", "min_num_players", "max_num_players"][..]),
            ] {
                let mut ours = vec![];
                scene.walk(&mut |n| {
                    if n.class == class {
                        ours.push(n);
                    }
                });

                let kit_e = (0..sc.doc().elements().len()).filter(|&e| sc.type_of(e) == class).collect::<Vec<_>>();
                // A capture location is decompiled at its flag (the binary's location): the kit's flag is at position + flag_position.
                let place = |e: usize| {
                    let w = world_transform(sc, e);
                    match (class, sc.doc().get(e, "flag_position").and_then(|f| parse_vec3(&f, "vector"))) {
                        ("CaptureLocationInstance", Some(f)) => w.position + w.rotation * f,
                        _ => w.position,
                    }
                };
                let c = local.classes.entry(class.to_owned()).or_default();
                c.0 += ours.len();
                c.1 += kit_e.len();
                let mut by_key: BTreeMap<String, Vec<(usize, wce_foundation::math::Double3)>> = BTreeMap::new();
                for &e in &kit_e {
                    by_key.entry(sc.doc().get(e, id).unwrap_or_default()).or_default().push((e, place(e)));
                }

                for n in ours {
                    let key = n.get(id).and_then(Value::as_str).unwrap_or_default();
                    // A capture location is placed at its flag, as the kit's are (`place`).
                    let mut p = n.get("position").and_then(Value::as_vec3).unwrap_or_default();
                    if let Some(f) = n.get("flag_position").and_then(Value::as_vec3) {
                        p = [p[0] + f[0], p[1] + f[1], p[2] + f[2]];
                    }
                    // WCE_DECOMPILE_UNPAIRED: why an instance does not pair (by class).
                    let unpaired = std::env::var_os("WCE_DECOMPILE_UNPAIRED").is_some();
                    let why = |what: String, local: &mut Totals| {
                        if unpaired {
                            let family = folder.split('/').nth(3).unwrap_or_default();
                            let k = format!("{class} unpaired: {what}");
                            if local.losses.get(&k).copied().unwrap_or(0) < 2 {
                                local.problems.push(format!("{name}: {class} {key} at {p:?} unpaired: {what} ({family})"));
                            }
                            *local.losses.entry(k).or_default() += 1;
                        }
                    };
                    let Some(cands) = by_key.get_mut(key) else {
                        why("no kit instance of the key".to_owned(), &mut local);
                        continue;
                    };

                    // Several instances can share a place (lights of a window, stacked decals of a rock clump): the candidate whose attributes
                    // agree most, then whose world rotation is closest.
                    let agree = |e: usize| attrs.iter().filter(|a| n.get(a).map(|v| crate_value_text(class, a, v)) == sc.doc().get(e, a)).count();
                    let o = n.get("orientation").and_then(Value::as_vec3).unwrap_or_default();
                    let m = euler_matrix(wce_foundation::math::Double3::new(o[0], o[1], o[2]));
                    let rot_diff = |e: usize| {
                        let t = world_transform(sc, e).rotation;
                        (0..3).flat_map(|r| (0..3).map(move |k| (r, k))).map(|(r, k)| (m.m[r][k] - t.m[r][k]).abs()).fold(0.0, f64::max)
                    };
                    let Some(ci) = cands
                        .iter()
                        .enumerate()
                        .filter(|(_, (_, q))| (q.x - p[0]).hypot(q.z - p[2]) < 0.1 && (matches!(class, "VEGETATION_INSTANCE" | "CaptureLocationInstance") || (q.y - p[1]).abs() < 0.1))
                        .max_by(|a, b| agree(a.1.0).cmp(&agree(b.1.0)).then_with(|| rot_diff(b.1.0).total_cmp(&rot_diff(a.1.0))))
                        .map(|(i, _)| i)
                    else {
                        let near = cands.iter().map(|(_, q)| ((q.x - p[0]).hypot(q.z - p[2]), q.y - p[1])).min_by(|a, b| a.0.total_cmp(&b.0));
                        if unpaired && class == "ProjectedDecalInstance" {
                            *local.losses.entry(format!("decals by family: {} unpaired", folder.split('/').nth(3).unwrap_or_default())).or_default() += 1;
                        }
                        why(
                            match near {
                                None => "every kit instance of the key already paired".to_owned(),
                                Some((h, dy)) if h < 0.1 => format!("same place, height differs ({})", if dy.abs() < 1.0 { "< 1 m" } else { ">= 1 m" }),
                                Some((h, _)) if h < 2.0 => "nearest of the key within 2 units".to_owned(),
                                Some(_) => "nearest of the key further".to_owned(),
                            },
                            &mut local,
                        );
                        continue;
                    };

                    let (e, _) = cands.swap_remove(ci);
                    local.classes.get_mut(class).expect("class").2 += 1;
                    if unpaired && class == "ProjectedDecalInstance" {
                        *local.losses.entry(format!("decals by family: {} paired", folder.split('/').nth(3).unwrap_or_default())).or_default() += 1;
                    }
                    for attr in attrs {
                        let ours = n.get(attr).map(|v| crate_value_text(class, attr, v));
                        let theirs = sc.doc().get(e, attr);
                        let c = local.attr_same.entry(format!("{class}.{attr}")).or_default();
                        if ours == theirs {
                            c.0 += 1;
                        } else {
                            c.1 += 1;
                            if c.1 <= 2 && std::env::var_os("WCE_DECOMPILE_EXAMPLES").is_some() {
                                local.problems.push(format!("{name}: {class} {key} {attr}: ours {ours:?}, kit {theirs:?}"));
                            }
                        }
                    }

                    // A point light has no rotation; the trees are turned at random by the game.
                    if matches!(class, "POINT_LIGHT_INSTANCE" | "VEGETATION_INSTANCE" | "CaptureLocationInstance") {
                        continue;
                    }

                    let t = world_transform(sc, e).rotation;
                    let diff = rot_diff(e);
                    let c = local.attr_same.entry(format!("{class}.orientation (world)")).or_default();
                    if diff < 0.001 {
                        c.0 += 1;
                    } else {
                        c.1 += 1;
                        // WCE_DECOMPILE_ROTATIONS: what the differing rotations have in common.
                        if std::env::var_os("WCE_DECOMPILE_ROTATIONS").is_some() {
                            let d = [1.0, -1.0, 1.0];
                            let conj = (0..3).flat_map(|r| (0..3).map(move |k| (r, k))).map(|(r, k)| (m.m[r][k] - d[r] * t.m[r][k] * d[k]).abs()).fold(0.0, f64::max);
                            let (mut pitch, mut roll, mut x) = (false, false, Some(e));
                            while let Some(y) = x {
                                if let Some(o) = sc.euler(y) {
                                    pitch |= o.y.abs() > 1e-6;
                                    roll |= o.z.rem_euclid(std::f64::consts::TAU).min(std::f64::consts::TAU - o.z.rem_euclid(std::f64::consts::TAU)) > 1e-6;
                                }
                                x = sc.doc().elements()[y].parent;
                            }
                            let family = folder.split('/').nth(3).unwrap_or_default();
                            // Both turns about y alone: their difference in degrees.
                            let yaw = if !pitch && !roll && m.m[1][1] > 0.9999 {
                                let d = (t.m[0][2].atan2(t.m[0][0]) - m.m[0][2].atan2(m.m[0][0])).to_degrees().rem_euclid(360.0);
                                format!(", yaw + {:.0}", d)
                            } else {
                                String::new()
                            };
                            let k = format!("{class} rotation differs: chain pitch {pitch} roll {roll}, ours = D kit D {} ({family}{yaw})", conj < 0.001);
                            if local.losses.get(&k).copied().unwrap_or(0) < 2 {
                                local.problems.push(format!("{name}: {class} {key} ({k}): ours {o:?} {:?}, kit world {:?} ({}, scale {:?}, our scale {:?})", m.m, t.m, sc.layer_path(e), sc.doc().get(e, "scale"), n.get("scale")));
                            }

                            *local.losses.entry(k).or_default() += 1;
                        }

                        if c.1 <= 3 && std::env::var_os("WCE_DECOMPILE_EXAMPLES").is_some() {
                            local.problems.push(format!("{name}: {class} {key} orientation: ours {o:?} {:?}, kit world {:?} ({}, file {:?}, scale {:?})", m.m, t.m, sc.layer_path(e), sc.doc().get(e, "orientation"), sc.doc().get(e, "scale")));
                        }
                    }
                }

                // WCE_DECOMPILE_UNPAIRED: the kit's capture locations left without a decompiled one.
                if class == "CaptureLocationInstance" && std::env::var_os("WCE_DECOMPILE_UNPAIRED").is_some() {
                    for (key, left) in &by_key {
                        for (e, q) in left {
                            local.problems.push(format!("{name}: kit capture location {key} at ({:.1}, {:.1}) without ours ({})", q.x, q.z, sc.layer_path(*e)));
                        }
                    }
                }
            }
        }

        // Polygons: pair on (class, centre of the world points' bounding box within 1 scene unit), then compare.
        if let Some(scene) = &raw.scene {
            let sc = &project.scene;
            let mut ours = vec![];
            our_polygons(scene, (0.0, 0.0), &mut ours);
            let centre = |pts: &[(f64, f64)]| {
                let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
                for &(x, z) in pts {
                    lo = (lo.0.min(x), lo.1.min(z));
                    hi = (hi.0.max(x), hi.1.max(z));
                }

                ((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0)
            };

            let mut kit: BTreeMap<String, Vec<(usize, Polygon)>> = BTreeMap::new();
            for e in 0..sc.doc().elements().len() {
                let pts_e = sc.polygon_points(e);
                if pts_e.is_empty() {
                    continue;
                }

                let w = world_transform(sc, e);
                let pts = pts_e.iter().filter_map(|&p| sc.point_position(p)).map(|q| {
                    let v = w.position + w.rotation * q;
                    (v.x, v.z)
                }).collect::<Vec<_>>();
                kit.entry(sc.type_of(e).to_owned()).or_default().push((e, pts));
            }

            for (class, list) in &kit {
                local.classes.entry(class.clone()).or_default().1 += list.len();
            }

            for (n, pts) in ours {
                let class = n.class.as_str().to_owned();
                local.classes.entry(class.clone()).or_default().0 += 1;
                let Some(cands) = kit.get_mut(&class) else { continue };
                let c = centre(&pts);
                let Some(ci) = cands.iter().position(|(_, k)| k.len() == pts.len() && {
                    let kc = centre(k);
                    (kc.0 - c.0).hypot(kc.1 - c.1) < 1.0
                }) else {
                    if std::env::var_os("WCE_DECOMPILE_EXAMPLES").is_some() {
                        let near = cands.iter().map(|(e, k)| (e, k.len(), centre(k))).min_by(|a, b| (a.2.0 - c.0).hypot(a.2.1 - c.1).total_cmp(&(b.2.0 - c.0).hypot(b.2.1 - c.1)));
                        local.problems.push(format!("{name}: unpaired {class}: {} points, centre {c:?}; nearest kit {near:?}; ours {:?}", pts.len(), &pts[..pts.len().min(4)]));
                    }

                    continue;
                };

                let (e, _) = cands.swap_remove(ci);
                local.classes.get_mut(&class).expect("class").2 += 1;
                for attr in ["name", "polygon_closed", "hint_type", "type", "deployment_polygon_type", "civilian_type", "civilian_amount"] {
                    let Some(v) = n.get(attr) else { continue };
                    let ours = crate_value_text(&class, attr, v);
                    let c = local.attr_same.entry(format!("{class}.{attr}")).or_default();
                    if Some(&ours) == sc.doc().get(e, attr).as_ref() {
                        c.0 += 1;
                    } else {
                        c.1 += 1;
                        if c.1 <= 2 && std::env::var_os("WCE_DECOMPILE_EXAMPLES").is_some() {
                            local.problems.push(format!("{name}: {class} {attr}: ours {ours}, kit {:?}", sc.doc().get(e, attr)));
                        }
                    }
                }

                let ka = sc.doc().get(e, "area").and_then(|a| a.parse::<f64>().ok()).unwrap_or(0.0);
                let oa = n.get("area").and_then(Value::as_f64).unwrap_or(0.0);
                let c = local.attr_same.entry(format!("{class}.area (0.1 %)")).or_default();
                if (ka - oa).abs() <= 1e-3 * ka.abs().max(1.0) {
                    c.0 += 1;
                } else {
                    c.1 += 1;
                }
            }
        }

        Ok(())
    })();

    let mut t = t.lock().expect("totals");
    t.tiles += 1;
    if let Err(e) = result {
        t.failed += 1;
        t.problems.push(format!("{name}: {e}"));
    }

    t.with_kit += local.with_kit;
    t.entry_same += local.entry_same;
    t.entry_differ += local.entry_differ;
    t.height_samples += local.height_samples;
    t.height_within_1cm += local.height_within_1cm;
    t.flat_heights += local.flat_heights;
    t.final_samples += local.final_samples;
    t.final_within_1cm += local.final_within_1cm;
    t.final_tiles += local.final_tiles;
    t.buildings += local.buildings;
    t.kit_buildings += local.kit_buildings;
    t.paired += local.paired;
    for (k, (a, b)) in local.attr_same {
        let e = t.attr_same.entry(k).or_default();
        e.0 += a;
        e.1 += b;
    }

    for (k, n) in local.losses {
        *t.losses.entry(k).or_default() += n;
    }

    for (k, (a, b, c)) in local.classes {
        let e = t.classes.entry(k).or_default();
        e.0 += a;
        e.1 += b;
        e.2 += c;
    }

    t.problems.extend(local.problems);
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let game = PathBuf::from(args.first().expect("usage: tile_decompile_check <game folder> [--filter TEXT] [--limit N] [--out DIR]"));
    let filter = opt("--filter");
    let limit = opt("--limit").and_then(|n| n.parse().ok()).unwrap_or(usize::MAX);
    let out = opt("--out").map(PathBuf::from);
    let kit = game.join("assembly_kit");
    let games = SupportedGames::default();
    let attila = games.game("attila").expect("attila");
    let mut packs = std::fs::read_dir(game.join("data")).expect("data").flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "pack")).collect::<Vec<_>>();
    packs.sort();

    // The databases: the mods' first (they override the game's), then tiles.pack's.
    let mut dbs = vec![];
    let mut opened = vec![];
    // The destruction levels of the buildings: the building's collision file (`rigidmodels/buildings/<key>/<key>_tech.cs2.parsed`) lists
    // each piece's destructs; the building's count is the smallest over its pieces (roman_fort_straight: piece01 has 3, the others 2, TEd
    // uses 2 levels). Without one, the same from the models' names (`<piece>_destructNN.rigid_model_v2`).
    let mut pieces: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
    let mut from_cs2: BTreeMap<String, u32> = BTreeMap::new();
    let wanted = std::env::var("WCE_DECOMPILE_LEVELS").unwrap_or_default().to_lowercase();
    for p in &packs {
        let Ok(mut pack) = Pack::read_and_merge(std::slice::from_ref(p), attila, true, false, false) else { continue };
        let mut cs2 = vec![];
        for n in pack.files().keys() {
            let Some(rest) = n.strip_prefix("rigidmodels/buildings/") else { continue };
            let Some((key, file)) = rest.split_once('/') else { continue };
            if file.ends_with(".cs2.parsed") {
                cs2.push((key.to_owned(), n.clone()));
            } else if let Some(at) = file.find("_destruct")
                && file.ends_with(".rigid_model_v2")
            {
                let digits = file[at + 9..].chars().take_while(char::is_ascii_digit).collect::<String>();
                if let Ok(level) = digits.parse::<u32>() {
                    let e = pieces.entry(key.to_owned()).or_default().entry(file[..at].to_owned()).or_default();
                    *e = (*e).max(level);
                }
            }
        }

        for (key, n) in cs2 {
            let Some(Ok(Some(bytes))) = pack.files_mut().get_mut(&n).map(|f| f.encode(&None, false, false, true)) else { continue };
            let Some(levels) = destruction_levels(&bytes) else { continue };
            if wanted.split(',').any(|k| k == key) {
                println!("levels of {key} ({n}): {levels}");
            }

            from_cs2.insert(key, levels);
        }

        let names = pack.files().keys().filter(|n| n.starts_with("terrain/tiles/") && n.ends_with("/tile_database.bin")).cloned().collect::<Vec<_>>();
        for n in names {
            let bytes = pack.files_mut().get_mut(&n).unwrap().encode(&None, false, false, true).unwrap().unwrap();
            if let Ok(db) = TileDatabase::decode(&mut Cursor::new(&bytes), &None) {
                let is_mod = !p.file_name().is_some_and(|f| f.to_string_lossy().starts_with("tiles"));
                dbs.push((is_mod, db));
            }
        }

        opened.push((p.clone(), pack));
    }

    dbs.sort_by_key(|(is_mod, _)| !is_mod);
    let dbs = dbs.into_iter().map(|(_, d)| d).collect::<Vec<_>>();
    let mut levels = pieces.into_iter().map(|(k, p)| (k, p.values().copied().min().unwrap_or(1))).collect::<BTreeMap<_, _>>();
    levels.extend(from_cs2);
    let lookup = |key: &str| levels.get(&key.to_lowercase()).copied();
    let mut bdb = BuildingDb::default();
    for f in ["battlefield_buildings.xml", "TWaD_battlefield_buildings.xml"] {
        if let Ok(x) = std::fs::read_to_string(kit.join("raw_data/db").join(f)) {
            bdb.add_xml(&x);
        }
    }

    let category = |key: &str| bdb.category(key).map(str::to_owned);
    let trees = opt("--trees");
    let ctx = Context { databases: &dbs, building_levels: Some(&lookup), building_category: Some(&category), tree_list: trees.as_deref() };
    let totals = Mutex::new(Totals::default());
    let mut done = 0;
    for (p, pack) in &mut opened {
        let pack_name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut folders: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for n in pack.files().keys().filter(|n| n.starts_with("terrain/tiles/")) {
            let (dir, _) = n.rsplit_once('/').unwrap_or(("", n));
            folders.entry(dir.to_owned()).or_default().push(n.clone());
        }

        folders.retain(|d, names| names.iter().any(|n| n.ends_with("/hf_height_map.data")) && filter.as_ref().is_none_or(|f| d.contains(f.as_str())));
        let folders = folders.into_iter().take(limit.saturating_sub(done)).collect::<Vec<_>>();
        done += folders.len();
        for batch in folders.chunks(32) {
            let loaded = batch
                .iter()
                .map(|(dir, names)| {
                    let mut files = TileFiles::default();
                    for n in names {
                        let Some(rf) = pack.files_mut().get_mut(n) else { continue };
                        let Ok(Some(bytes)) = rf.encode(&None, false, false, true) else { continue };
                        files.folder.insert(n[dir.len() + 1..].to_owned(), Blob::Bytes(bytes.into()));
                    }

                    (format!("{pack_name}:{dir}"), dir.clone(), files)
                })
                .collect::<Vec<_>>();

            loaded.par_iter().for_each(|(name, dir, files)| check(name, dir, files, &ctx, &kit, out.as_deref(), &totals));
        }
    }

    let t = totals.into_inner().expect("totals");
    println!("tile folders: {} decompiled, {} failed; {} with the kit's raw tile", t.tiles - t.failed, t.failed, t.with_kit);
    println!("entries (but the variation's min_height / scale): {} identical to the kit's, {} differ", t.entry_same, t.entry_differ);
    println!("heights inside the border: {} of {} samples within 1 cm of the kit's ({:.1} %)", t.height_within_1cm, t.height_samples, 100.0 * t.height_within_1cm as f64 / t.height_samples.max(1) as f64);
    println!("heights against the kit's final_heights.dds (BOB's last export): {} tiles, {} of {} samples within 1 cm ({:.1} %)", t.final_tiles, t.final_within_1cm, t.final_samples, 100.0 * t.final_within_1cm as f64 / t.final_samples.max(1) as f64);
    println!("flat CHMF (range ~0, the mesh flat too; the kit's sources are not): {} tiles, left out of the heights above", t.flat_heights);
    println!("buildings: {} decompiled, {} in the kit's tile.ted, {} paired (key, world position within 10 cm)", t.buildings, t.kit_buildings, t.paired);
    for (k, (a, b)) in &t.attr_same {
        println!("  {k}: {a} same, {b} differ");
    }

    for (k, (a, b, c)) in &t.classes {
        println!("{k}: {a} decompiled, {b} in the kit, {c} paired");
    }

    println!("losses (tiles that report each):");
    for (k, n) in &t.losses {
        println!("  {n:>5} {k}");
    }

    if !t.problems.is_empty() {
        // WCE_DECOMPILE_PROBLEMS=N prints the first N (default 30).
        let n = std::env::var("WCE_DECOMPILE_PROBLEMS").ok().and_then(|n| n.parse().ok()).unwrap_or(30usize);
        println!("problems ({}; first {n}):", t.problems.len());
        for p in t.problems.iter().take(n) {
            println!("  {p}");
        }
    }
}
