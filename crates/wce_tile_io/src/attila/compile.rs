//! A TEd project as an exported (binary) tile: the model `attila.ted_project` reads turned into the documents `attila.tile_binary` writes
//! (docs/plans/tile_roundtrip.md, Phase 2 step 4). Stage A, TEd's export of the logic, first: the building lists.
//!
//! The numbers go through TEd's float arithmetic: a value is read from its text as `sscanf` reads it (f32), the world matrices are
//! `QTU::TedEntity::world_transform`'s (`wce_tile::world_transform::ted_rotation_f32` / `ted_mul_f32`).

use wce_tile::world_transform::{ted_mul_f32, ted_rotation_f32};
use wce_tile_model::{Document, Node, Sym, Tile, Value};

use super::decompile::{LevelsOf, ThresholdsOf};
use super::schema::TedSchema;

/// What the compiler can be told besides the project.
#[derive(Default)]
pub struct CompileContext<'a> {
    /// The destruction levels of a building key (`decompile::destruction_levels`), for its starting damage = level / (levels - 1).
    pub building_levels: Option<LevelsOf<'a>>,

    /// The `destruct_thresholds` of a building key (`battlefield_buildings`).
    pub building_thresholds: Option<ThresholdsOf<'a>>,

    /// The version to write a markers file in (by file name); without it TEd's current one.
    pub marker_versions: Option<&'a (dyn Fn(&str) -> Option<i64> + Sync)>,
}

/// What a compilation left out or made up, as notes.
#[derive(Default, Debug)]
pub struct CompileNotes {
    pub notes: Vec<String>,
}

/// An instance of the scene with its world matrix and the layer it is exported with.
struct Placed<'a> {
    node: &'a Node,
    world: [f32; 16],
    export: &'a str,
    layer: &'a str,
}

/// The f32s of a value's text, as `sscanf` reads them (`vector(x,y,z)`, `euler(a,b,c)`).
fn floats_of(n: &Node, attr: &str) -> Option<Vec<f32>> {
    let v = n.get(attr)?;
    let text = crate::xml::value_text(&TedSchema, n.class, Sym::new(attr), v);
    let inner = text.find('(').map_or(text.as_str(), |i| &text[i + 1..]).trim_end_matches(')');
    inner.split(',').map(|t| t.trim().parse::<f32>().ok()).collect()
}

fn vec3_of(n: &Node, attr: &str) -> Option<[f32; 3]> {
    let v = floats_of(n, attr)?;
    Some([*v.first()?, *v.get(1)?, *v.get(2)?])
}

/// Every positioned node below `n` (in document order) with its world matrix and its layer's export type and name.
fn walk<'a>(n: &'a Node, world: Option<[f32; 16]>, export: &'a str, layer: &'a str, out: &mut Vec<Placed<'a>>) {
    let positioned = n.get("position").is_some() || n.get("orientation").is_some();
    let world = if positioned {
        let mut local = ted_rotation_f32(vec3_of(n, "orientation").unwrap_or([0.0; 3]));
        let p = vec3_of(n, "position").unwrap_or([0.0; 3]);
        (local[3], local[7], local[11]) = (p[0], p[1], p[2]);
        Some(world.map_or(local, |w| ted_mul_f32(&w, &local)))
    } else {
        world
    };

    let (export, layer) = if n.class == "LayerInstance" {
        (n.get("export_type").and_then(Value::as_str).unwrap_or("LayerExportType_Normal"), n.get("name").and_then(Value::as_str).unwrap_or_default())
    } else {
        (export, layer)
    };

    if let Some(w) = world
        && positioned
    {
        out.push(Placed { node: n, world: w, export, layer });
    }

    for c in &n.children {
        walk(c, world, export, layer, out);
    }
}

/// The quaternion (i, j, k, w) of a world matrix as TEd's BMD export makes it (`FUN_00474030` of TEd.AssemblyKit.exe, Shoemake's method):
/// the x87 works in double (53-bit precision control) and rounds to f32 where the code stores a float. `e(r, c)` is TEd's element of the
/// rotation.
fn quaternion(m: &[f32; 16]) -> [f32; 4] {
    // TEd's MATRIX_44 keeps e(r, c) at r + 4 c: the transpose of the row-major matrix here.
    let e = |r: usize, c: usize| f64::from(m[c * 4 + r]);
    let f = |v: f64| v as f32;
    let trace = f(e(2, 2) + f64::from(f(e(1, 1) + f64::from(f(e(0, 0))))) + 1.0);
    let mut q = [0.0f32; 4];
    if trace < 1.0 {
        let next = [1usize, 2, 0];
        let mut i = 0;
        if e(0, 0) < e(1, 1) {
            i = 1;
        }

        if e(1, 1) < e(2, 2) {
            i = 2;
        }

        let j = next[i];
        let k = next[j];
        let r = f64::from(f(f64::from(f(e(i, i) - e(j, j))) - e(k, k) + 1.0)).sqrt();
        q[i] = 0.5 * f(r);
        let s = 0.5 / f(r);
        q[3] = f(-((e(k, j) - e(j, k)) * f64::from(s)));
        q[j] = f((e(i, j) + e(j, i)) * f64::from(s));
        q[k] = f((e(i, k) + e(k, i)) * f64::from(s));
    } else {
        let s = f(f64::from(trace).sqrt() * 2.0);
        q[3] = 0.25 * s;
        let c = 1.0 / s;
        q[0] = f((e(1, 2) - e(2, 1)) * f64::from(c));
        q[1] = f((e(2, 0) - e(0, 2)) * f64::from(c));
        q[2] = f((e(0, 1) - e(1, 0)) * f64::from(c));
    }

    q
}

fn bool_of(n: &Node, attr: &str, default: bool) -> bool {
    n.get(attr).and_then(Value::as_bool).unwrap_or(default)
}

/// Whether (x, z) is inside the polygon (even-odd rule).
fn inside(poly: &[(f32, f32)], x: f32, z: f32) -> bool {
    let mut c = false;
    for k in 0..poly.len() {
        let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
        if (a.1 > z) != (b.1 > z) && x < a.0 + (z - a.1) / (b.1 - a.1) * (b.0 - a.0) {
            c = !c;
        }
    }

    c
}

/// A LiteBuildingZone's points in the scene (x, z).
fn zone_points(p: &Placed) -> Vec<(f32, f32)> {
    let Some(points) = p.node.children.iter().find(|c| c.class == "points") else { return vec![] };
    points
        .children
        .iter()
        .filter_map(|pt| vec3_of(pt, "position"))
        .map(|q| {
            let w = &p.world;
            (w[0] * q[0] + w[1] * q[1] + w[2] * q[2] + w[3], w[8] * q[0] + w[9] * q[1] + w[10] * q[2] + w[11])
        })
        .collect()
}

/// The building lists of a project: `building_list` from the normal layers, `building_list_<layer name>` from each separate layer, the
/// buildings in the scene's order (docs/formats/tile_decompile.md for the frames: the list's x = scene x / 2, z = extent - scene z / 2).
fn building_lists(placed: &[Placed], extent_z: f32, ctx: &CompileContext, notes: &mut CompileNotes) -> Vec<Document> {
    let zones = placed.iter().filter(|p| p.node.class == "LiteBuildingZone" && p.export == "LayerExportType_Normal").map(zone_points).collect::<Vec<_>>();
    // A separate layer exports its list even when it has no buildings.
    let mut lists: Vec<(String, Node)> = vec![];
    for p in placed.iter().filter(|p| p.node.class == "LayerInstance" && p.export == "LayerExportType_Separate") {
        let name = format!("building_list_{}", p.layer.to_lowercase());
        if !lists.iter().any(|(n, _)| *n == name) {
            let mut l = Node::new("BATTLEFIELD_BUILDING_LIST");
            l.set("version", Value::Int(12));
            lists.push((name, l));
        }
    }

    let mut unknown_levels = 0;
    for p in placed.iter().filter(|p| p.node.class == "BUILDING_INSTANCE") {
        let name = match p.export {
            "LayerExportType_Normal" => "building_list".to_owned(),
            "LayerExportType_Separate" => format!("building_list_{}", p.layer.to_lowercase()),
            _ => continue,
        };

        let n = p.node;
        let w = &p.world;
        let (x, y, z) = (w[3], w[7], w[11]);
        let key = n.get("key").and_then(Value::as_str).unwrap_or_default();
        let level = n.get("destruction_level").and_then(Value::as_int).unwrap_or(0);
        // `Ted::BMDExporter::export_` ({level, min 0, max levels - 1}): without `destruct_thresholds`, level / max (past 1 for a level
        // past the model's); with them, level 0 is 0, level max 1, any other its threshold (index level - 1).
        let thresholds = ctx.building_thresholds.and_then(|f| f(key)).unwrap_or_default();
        let damage = match (level, ctx.building_levels.and_then(|f| f(key))) {
            (0, _) => 0.0,
            (l, Some(levels)) if levels > 1 && !thresholds.is_empty() => {
                if l == i64::from(levels - 1) { 1.0 } else { thresholds.get(l as usize - 1).copied().unwrap_or(1.0) }
            }
            (l, Some(levels)) if levels > 1 => (l as f32) / ((levels - 1) as f32),
            _ => {
                unknown_levels += 1;
                1.0
            }
        };

        let mut b = Node::new("building");
        b.set("key", Value::Str(key.to_owned()));
        b.set("position", Value::Vec3([f64::from(x * 0.5), f64::from(y), f64::from(extent_z - z * 0.5)]));
        b.set("orientation", Value::Floats(quaternion(w).iter().map(|&v| f64::from(v)).collect()));
        b.set("scale", Value::Vec3([1.0; 3]));
        b.set("starting_damage", Value::Float(f64::from(damage)));
        for (k, attr, default) in [
            ("on_fire", "burn", false), ("start_disabled", "", false), ("weak_point", "weak_wall", false), ("ai_breachable", "ai_breachable", true),
            ("indestructible", "indestructible", false), ("dockable", "dockable", true),
        ] {
            b.set(k, Value::Bool(if attr.is_empty() { default } else { bool_of(n, attr, default) }));
        }

        b.set("position_type", Value::Int(2));
        b.set("toggleable", Value::Bool(bool_of(n, "toggleable", false)));
        b.set("lite", Value::Bool(name == "building_list" && zones.iter().any(|z| inside(z, x, z_of(w)))));
        match lists.iter_mut().find(|(n, _)| *n == name) {
            Some((_, l)) => l.children.push(b),
            None => {
                let mut l = Node::new("BATTLEFIELD_BUILDING_LIST");
                l.set("version", Value::Int(12));
                l.children.push(b);
                lists.push((name, l));
            }
        }
    }

    if unknown_levels > 0 {
        notes.notes.push(format!("building lists: {unknown_levels} damaged buildings without their destruction levels (starting damage 1)"));
    }

    lists.into_iter().map(|(name, root)| Document { name, root }).collect()
}

fn z_of(w: &[f32; 16]) -> f32 {
    w[11]
}

// ---- marker tables

/// `EMPIREUTILITY::MARKER_TABLE`'s hash of a key's parts (docs/re/ted_export.md, "Marker tables").
fn marker_key_hash(parts: &[&str]) -> u32 {
    let part = |s: &str| {
        let (mut h, mut shift) = (0xf73a_51c9u32, 0u32);
        for u in s.encode_utf16().collect::<Vec<_>>().into_iter().rev() {
            h ^= u32::from(u) << shift;
            shift = (shift + 8) % 32;
        }
        h
    };
    parts.iter().fold(0u32, |h, p| {
        let p = part(p);
        let b = ((p as u8 ^ h as u8).wrapping_add(13) & 31) as u32;
        ((h >> b) | (h << (b ^ 31))) ^ p
    })
}

/// The order a `MARKER_TABLE` gives its keys (in the order of their first record): a hash table from 13 slots, linear probing,
/// growing to 2 n + 1 slots when full; the keys in slot order.
fn marker_table_order<'k>(keys: &[&'k str]) -> Vec<&'k str> {
    let mut slots: Vec<Option<&str>> = vec![None; 13];
    let insert = |slots: &mut Vec<Option<&'k str>>, k: &'k str| -> bool {
        let n = slots.len();
        let start = marker_key_hash(&[k]) as usize % n;
        (0..n).map(|i| (start + i) % n).find(|&i| slots[i].is_none()).map(|i| slots[i] = Some(k)).is_some()
    };

    for &k in keys {
        if !insert(&mut slots, k) {
            let old = slots.iter().flatten().copied().collect::<Vec<_>>();
            slots = vec![None; slots.len() * 2 + 1];
            for o in old {
                insert(&mut slots, o);
            }

            insert(&mut slots, k);
        }
    }

    slots.into_iter().flatten().collect()
}

/// A markers document: the class's header, the records, the groups of the records by key in the marker table's order.
fn markers_doc(name: &str, class: &str, class_id: i64, version: i64, records: Vec<(String, Node)>) -> Document {
    let mut root = Node::new(class);
    root.set("repository", Value::Str("BASE_MARKER_REPOSITORY".to_owned()));
    root.set("repository_id", Value::Int(645_563));
    root.set("repository_u32", Value::Int(1));
    root.set("repository_u16", Value::Int(1));
    root.set("class_id", Value::Int(class_id));
    root.set("version", Value::Int(version));
    let mut first: Vec<&str> = vec![];
    let mut indices: std::collections::HashMap<&str, Vec<i64>> = std::collections::HashMap::new();
    for (i, (k, _)) in records.iter().enumerate() {
        if !indices.contains_key(k.as_str()) {
            first.push(k);
        }

        indices.entry(k).or_default().push(i as i64);
    }

    let mut groups = Node::new("groups");
    for k in marker_table_order(&first) {
        let mut g = Node::new("group");
        let mut key = Node::new("key");
        key.set("value", Value::Str(k.to_owned()));
        g.children.push(key);
        g.set("indices", Value::Ints(indices[k].clone()));
        groups.children.push(g);
    }

    root.children.extend(records.into_iter().map(|(_, r)| r));
    root.children.push(groups);
    Document { name: name.to_owned(), root }
}

/// TEd's seasons flags as the bit mask (bit 0 winter, 1 spring, 2 summer, 3 autumn); without the attribute every season.
fn seasons_mask(n: &Node) -> i64 {
    let Some(Value::Flags(f)) = n.get("seasons") else { return 15 };
    ["SEASON_BF_WINTER", "SEASON_BF_SPRING", "SEASON_BF_SUMMER", "SEASON_BF_AUTUMN"]
        .iter()
        .enumerate()
        .filter(|(_, s)| f.iter().any(|x| x.as_str() == **s))
        .map(|(i, _)| 1 << i)
        .sum()
}

/// `prop_marker.markers`: the decals (`ProjectedDecalInstance`) and models (`SIMPLE_MESH_INSTANCE`) of the normal layers, in the scene's
/// order. A record's 3 x 3 rows are the world rotation in the binary's frame (z mirrored: F R^T F, F = diag(1, 1, -1)) times half the
/// instance's scale, a model's y and z rows negated; the translation (x / 2, y / 2, extent - z / 2). The inverse of the decompiler's
/// (docs/formats/tile_decompile.md, "Rotations").
fn prop_markers(placed: &[Placed], extent_z: f32, ctx: &CompileContext) -> Option<Document> {
    const NAME: &str = "prop_marker.markers";
    let version = ctx.marker_versions.and_then(|f| f(NAME)).unwrap_or(9);
    let mut records = vec![];
    for p in placed.iter().filter(|p| p.export == "LayerExportType_Normal") {
        let n = p.node;
        let decal = match n.class.as_str() {
            "ProjectedDecalInstance" => true,
            "SIMPLE_MESH_INSTANCE" => false,
            _ => continue,
        };

        let key = n.get("key").and_then(Value::as_str).unwrap_or_default().to_owned();
        let scale = vec3_of(n, "scale").unwrap_or([1.0; 3]);
        let half = scale.map(|s| s * 0.5);
        let w = &p.world;
        let f = [1.0f32, 1.0, -1.0];
        let mut m = [0.0f64; 16];
        for k in 0..3 {
            let sign = if decal || k == 0 { 1.0 } else { -1.0 };
            let len = half[k].abs() * sign;
            for l in 0..3 {
                // a[k][l] = f_l f_k R[l][k] with R[l][k] = w[l * 4 + k].
                m[k * 4 + l] = f64::from(f[l] * f[k] * w[l * 4 + k] * len);
            }
        }

        (m[12], m[13], m[14], m[15]) = (f64::from(w[3] * 0.5), f64::from(w[7] * 0.5), f64::from(extent_z - w[11] * 0.5), 1.0);
        let mut r = Node::new("record");
        r.set("transform", Value::Floats(m.to_vec()));
        r.set("unknown_64", Value::Int(0));
        r.set("is_decal", Value::Int(i64::from(decal)));
        for u in ["unknown_66", "unknown_67", "unknown_68"] {
            r.set(u, Value::Int(0));
        }

        // A water plane model is flagged (FUN_00502c60 compares the model's name with `water_plane`).
        let water = !decal && key.replace('\\', "/").rsplit('/').next().is_some_and(|f| f.eq_ignore_ascii_case("water_plane.rigid_model_v2"));
        r.set("unknown_69", Value::Int(i64::from(water)));
        if version >= 6 {
            r.set("seasons", Value::Int(if decal { 15 } else { seasons_mask(n) }));
        }

        if version == 8 {
            r.set("unknown_74", Value::Bytes(vec![0; 3].into()));
        } else if version >= 9 {
            r.set("unknown_74", Value::Int(0));
            for v in ["visible_inside_snow_zone", "visible_outside_snow_zone", "visible_inside_destruction_zone", "visible_outside_destruction_zone"] {
                r.set(v, Value::Int(i64::from(bool_of(n, v, true))));
            }
        }

        if decal {
            r.set("parallax_scale", Value::Float(f64::from(floats_of(n, "parallax_scale").and_then(|v| v.first().copied()).unwrap_or(0.0))));
            r.set("tiled", Value::Int(i64::from(bool_of(n, "tiled", false))));
            for (k, h) in ["scale_x", "scale_y", "scale_z"].iter().zip(half) {
                r.set(*k, Value::Float(f64::from(h)));
            }
        }

        records.push((key, r));
    }

    (!records.is_empty()).then(|| markers_doc(NAME, "PROP_MARKER", 645_566, version, records))
}

/// The stage-A documents of a project (TEd's export): the building lists and the prop markers so far.
///
/// # Errors
///
/// The project has no scene or no entry to size it.
pub fn compile(raw: &Tile, ctx: &CompileContext) -> Result<(Vec<Document>, CompileNotes), String> {
    let scene = raw.scene.as_ref().ok_or("the project has no tile.ted")?;
    let entry = raw.entry.as_ref().ok_or("the project has no entry")?;
    let height = entry.root.get("height").and_then(Value::as_int).ok_or("the entry has no height")?;
    let extent_z = height as f32 * 128.0;
    let mut placed = vec![];
    walk(scene, None, "LayerExportType_Normal", "", &mut placed);
    let mut notes = CompileNotes::default();
    let mut docs = building_lists(&placed, extent_z, ctx, &mut notes);
    docs.extend(prop_markers(&placed, extent_z, ctx));
    Ok((docs, notes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(name: &str, export: &str, children: Vec<Node>) -> Node {
        let mut l = Node::new("LayerInstance");
        l.set("export_type", Value::Str(export.to_owned()));
        l.set("position", Value::Vec3([0.0; 3]));
        l.set("name", Value::Str(name.to_owned()));
        l.set("orientation", Value::Euler([0.0; 3]));
        let mut c = Node::new("children");
        c.children = children;
        l.children.push(c);
        l
    }

    fn building(key: &str, pos: [f64; 3], yaw: f64, level: i64) -> Node {
        let mut b = Node::new("BUILDING_INSTANCE");
        b.set("key", Value::Str(key.to_owned()));
        b.set("destruction_level", Value::Int(level));
        b.set("position", Value::Vec3(pos));
        b.set("orientation", Value::Euler([yaw, 0.0, 0.0]));
        b
    }

    fn project(layers: Vec<Node>) -> Tile {
        let mut scene = Node::new("project");
        let mut sc = Node::new("scene");
        let mut graph = layer("root", "LayerExportType_Normal", layers);
        graph.class = Sym::new("SCENE_GRAPH");
        sc.children.push(graph);
        scene.children.push(sc);
        let mut entry = Node::new("TILE");
        entry.set("height", Value::Int(2));
        Tile { scene: Some(scene), entry: Some(wce_tile_model::Entry { file_name: "t.xml".to_owned(), root: entry }), ..Default::default() }
    }

    #[test]
    fn quaternion_of_a_turn_about_y() {
        let a = 0.7f32;
        let q = quaternion(&ted_rotation_f32([a, 0.0, 0.0]));
        assert!(q[0].abs() < 1e-7 && q[2].abs() < 1e-7, "{q:?}");
        assert!((q[1] - (a / 2.0).sin()).abs() < 1e-6 && (q[3] - (a / 2.0).cos()).abs() < 1e-6, "{q:?}");
        assert_eq!(quaternion(&ted_rotation_f32([0.0; 3])), [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn building_lists_frames_damage_and_separate_layers() {
        let tile = project(vec![
            layer("Buildings", "LayerExportType_Normal", vec![building("wall", [100.0, 3.0, 50.0], 0.0, 1), building("house", [10.0, 0.0, 20.0], 0.0, 2)]),
            layer("Veg", "LayerExportType_Separate", vec![]),
            layer("Off", "LayerExportType_Off", vec![building("hidden", [0.0; 3], 0.0, 0)]),
        ]);
        let levels = |k: &str| Some(if k == "wall" { 3 } else { 2 });
        let thresholds = |k: &str| (k == "wall").then(|| vec![0.8f32]);
        let ctx = CompileContext { building_levels: Some(&levels), building_thresholds: Some(&thresholds), ..Default::default() };
        let (docs, _) = compile(&tile, &ctx).expect("compile");
        assert_eq!(docs.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), ["building_list_veg", "building_list"]);
        let list = &docs[1].root.children;
        assert_eq!(list.len(), 2);
        // The list's frame: x / 2, y, extent (2 cells: 256) - z / 2.
        assert_eq!(list[0].get("position"), Some(&Value::Vec3([50.0, 3.0, 231.0])));
        // A threshold for the wall's level 1; level 2 of a 2-level house past the model's: 2 / 1.
        assert_eq!(list[0].get("starting_damage").and_then(Value::as_f64), Some(f64::from(0.8f32)));
        assert_eq!(list[1].get("starting_damage").and_then(Value::as_f64), Some(2.0));
        assert!(docs[0].root.children.is_empty());
    }
}
