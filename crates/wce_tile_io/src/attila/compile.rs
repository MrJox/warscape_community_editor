//! A TEd project as an exported (binary) tile: the model `attila.ted_project` reads turned into the documents `attila.tile_binary` writes
//! (docs/plans/tile_roundtrip.md, Phase 2 step 4). Stage A, TEd's export of the logic, first: the building lists.
//!
//! The numbers go through TEd's float arithmetic: a value is read from its text as `sscanf` reads it (f32), the world matrices are
//! `QTU::TedEntity::world_transform`'s (`wce_tile::world_transform::ted_rotation_f32` / `ted_mul_f32`).

use wce_tile::world_transform::{ted_mul_f32, ted_rotation_f32};
use wce_tile_model::{Document, Node, Sym, Tile, Value};

use super::decompile::{CategoryOf, LevelsOf, ThresholdsOf};
use super::schema::TedSchema;

/// The version to write a file in, by its name.
pub type VersionOf<'a> = &'a (dyn Fn(&str) -> Option<i64> + Sync);

/// What the compiler can be told besides the project.
#[derive(Default)]
pub struct CompileContext<'a> {
    /// The destruction levels of a building key (`decompile::destruction_levels`), for its starting damage = level / (levels - 1).
    pub building_levels: Option<LevelsOf<'a>>,

    /// The `destruct_thresholds` of a building key (`battlefield_buildings`).
    pub building_thresholds: Option<ThresholdsOf<'a>>,

    /// The `battlefield_buildings` category of a building key (a capture location links the `fort_tower` and `gate` buildings at it).
    pub building_category: Option<CategoryOf<'a>>,

    /// The version to write a markers file in (by file name); without it TEd's current one.
    pub marker_versions: Option<VersionOf<'a>>,
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

/// A marker table's key: a string (the model, the effect, the light's name; hashed by `marker_key_hash`), or a number (an enum: the AI hint
/// type; a uint table with the value as its hash), written as the number or as the enum's name.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum MarkerKey {
    Text(String),
    Number(u32),
    Named(String, u32),
}

impl MarkerKey {
    fn hash(&self) -> u32 {
        match self {
            Self::Text(s) => marker_key_hash(&[s]),
            Self::Number(v) | Self::Named(_, v) => *v,
        }
    }

    fn value(&self) -> Value {
        match self {
            Self::Text(s) | Self::Named(s, _) => Value::Str(s.clone()),
            Self::Number(v) => Value::Int(i64::from(*v)),
        }
    }
}

/// The order a `MARKER_TABLE` gives its keys (in the order of their first record): a hash table from 13 slots, linear probing,
/// growing to 2 n + 1 slots when full; the keys in slot order.
fn marker_table_order(keys: &[MarkerKey]) -> Vec<MarkerKey> {
    let mut slots: Vec<Option<MarkerKey>> = vec![None; 13];
    let insert = |slots: &mut Vec<Option<MarkerKey>>, k: MarkerKey| -> bool {
        let n = slots.len();
        let start = k.hash() as usize % n;
        match (0..n).map(|i| (start + i) % n).find(|&i| slots[i].is_none()) {
            Some(i) => {
                slots[i] = Some(k);
                true
            }
            None => false,
        }
    };

    for k in keys {
        if !insert(&mut slots, k.clone()) {
            let old = slots.iter().flatten().cloned().collect::<Vec<_>>();
            slots = vec![None; slots.len() * 2 + 1];
            for o in old {
                insert(&mut slots, o);
            }

            insert(&mut slots, k.clone());
        }
    }

    slots.into_iter().flatten().collect()
}

/// A markers document: the class's header, the records, the groups of the records by key in the marker table's order.
fn markers_doc(name: &str, class: &str, class_id: i64, version: i64, records: Vec<(MarkerKey, Node)>) -> Document {
    let mut root = Node::new(class);
    root.set("repository", Value::Str("BASE_MARKER_REPOSITORY".to_owned()));
    root.set("repository_id", Value::Int(645_563));
    root.set("repository_u32", Value::Int(1));
    root.set("repository_u16", Value::Int(1));
    root.set("class_id", Value::Int(class_id));
    root.set("version", Value::Int(version));
    let mut first: Vec<MarkerKey> = vec![];
    let mut indices: std::collections::HashMap<MarkerKey, Vec<i64>> = std::collections::HashMap::new();
    for (i, (k, _)) in records.iter().enumerate() {
        if !indices.contains_key(k) {
            first.push(k.clone());
        }

        indices.entry(k.clone()).or_default().push(i as i64);
    }

    let mut groups = Node::new("groups");
    for k in marker_table_order(&first) {
        let mut g = Node::new("group");
        let mut key = Node::new("key");
        key.set("value", k.value());
        g.children.push(key);
        g.set("indices", Value::Ints(indices[&k].clone()));
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
/// order. A record's row k is the world rotation in the binary's frame (z mirrored: F R^T F, F = diag(1, 1, -1)) times half the
/// instance's scale: its magnitude for a decal, f_k times it signed for a model (f = F); the translation (x / 2, y / 2, extent - z / 2).
/// The inverse of the decompiler's (docs/formats/tile_decompile.md, "Rotations").
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
            // A decal: |half scale| (version 5: signed); a model: f_k * the signed half scale (f = (1, 1, -1)).
            let len = if decal && version >= 6 { half[k].abs() } else if decal || k != 2 { half[k] } else { -half[k] };
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

        // A model named `water_plane` is flagged (FUN_00502c60 compares the instance's name; 1,184 of the kit's water planes are so
        // named, 8 `Object`).
        let water = !decal && n.get("name").and_then(Value::as_str) == Some("water_plane");
        r.set("unknown_69", Value::Int(i64::from(water)));
        if version >= 6 {
            r.set("seasons", Value::Int(if decal { 15 } else { seasons_mask(n) }));
        }

        if version == 8 {
            r.set("unknown_74", Value::Bytes(vec![0; 3]));
        } else if version >= 9 {
            r.set("unknown_74", Value::Int(0));
            for v in ["visible_inside_snow_zone", "visible_outside_snow_zone", "visible_inside_destruction_zone", "visible_outside_destruction_zone"] {
                r.set(v, Value::Int(i64::from(bool_of(n, v, true))));
            }
        }

        if decal {
            r.set("parallax_scale", Value::Float(f64::from(floats_of(n, "parallax_scale").and_then(|v| v.first().copied()).unwrap_or(0.0))));
            r.set("tiled", Value::Int(i64::from(bool_of(n, "tiled", false))));
            // Version 5 writes the magnitudes.
            for (k, h) in ["scale_x", "scale_y", "scale_z"].iter().zip(half) {
                r.set(*k, Value::Float(f64::from(if version >= 6 { h } else { h.abs() })));
            }
        }

        records.push((MarkerKey::Text(key), r));
    }

    (!records.is_empty()).then(|| markers_doc(NAME, "PROP_MARKER", 645_566, version, records))
}

/// `particle_emitter_marker.markers` (version 9; 4 to 8 for the older tiles' loop): the `VFXInstance`s of the normal layers, in the scene's order, grouped by `vfx`. The
/// matrix holds the world rotation transposed (not mirrored) and the translation (x / 2, y, extent - z / 2); `scale` is TEd's halved.
fn emitter_markers(placed: &[Placed], extent_z: f32, ctx: &CompileContext) -> Option<Document> {
    const NAME: &str = "particle_emitter_marker.markers";
    let version = ctx.marker_versions.and_then(|f| f(NAME)).unwrap_or(9);
    let mut records = vec![];
    for p in placed.iter().filter(|p| p.export == "LayerExportType_Normal" && p.node.class == "VFXInstance") {
        let n = p.node;
        let w = &p.world;
        let mut m = [0.0f64; 16];
        for k in 0..3 {
            for l in 0..3 {
                m[l * 4 + k] = f64::from(w[k * 4 + l]);
            }
        }

        (m[12], m[13], m[14], m[15]) = (f64::from(w[3] * 0.5), f64::from(w[7]), f64::from(extent_z - w[11] * 0.5), 1.0);
        let mut r = Node::new("record");
        r.set("transform", Value::Floats(m.to_vec()));
        let autoplay = u8::from(bool_of(n, "autoplay", true));
        let outfield = u8::from(bool_of(n, "use_in_outfield", false));
        let scale = floats_of(n, "scale").and_then(|v| v.first().copied()).unwrap_or(1.0) * 0.5;
        let seasons = seasons_mask(n) as u32;
        if version >= 9 {
            r.set("unknown_64", Value::Bytes(vec![0; 6]));
            r.set("autoplay", Value::Int(i64::from(autoplay)));
            r.set("use_in_outfield", Value::Int(i64::from(outfield)));
            for v in ["visible_inside_snow_zone", "visible_outside_snow_zone", "visible_inside_destruction_zone", "visible_outside_destruction_zone"] {
                r.set(v, Value::Int(i64::from(bool_of(n, v, true))));
            }

            r.set("scale", Value::Float(f64::from(scale)));
            r.set("seasons", Value::Int(i64::from(seasons)));
        } else {
            // Older versions keep the fields in the bytes after the matrix: 6 zeros, autoplay, (7+) use_in_outfield, (8) 2 zeros, the
            // scale, (5+) the seasons.
            let mut b = vec![0u8; 6];
            b.push(autoplay);
            if version >= 7 {
                b.push(outfield);
            }

            if version >= 8 {
                b.extend([0, 0]);
            }

            b.extend(scale.to_le_bytes());
            if version >= 5 {
                b.extend(seasons.to_le_bytes());
            }

            r.set("unknown_64", Value::Bytes(b));
        }
        records.push((MarkerKey::Text(n.get("vfx").and_then(Value::as_str).unwrap_or_default().to_owned()), r));
    }

    (!records.is_empty()).then(|| markers_doc(NAME, "PARTICLE_EMITTER_MARKER", 618_864, version, records))
}

/// `point_light_marker.markers` (version 3): the `POINT_LIGHT_INSTANCE`s of the normal layers, in the scene's order, grouped by name; a
/// light is a position (x / 2, y, extent - z / 2) and its parameters.
fn light_markers(placed: &[Placed], extent_z: f32, ctx: &CompileContext) -> Option<Document> {
    const NAME: &str = "point_light_marker.markers";
    let version = ctx.marker_versions.and_then(|f| f(NAME)).unwrap_or(3);
    let mut records = vec![];
    for p in placed.iter().filter(|p| p.export == "LayerExportType_Normal" && p.node.class == "POINT_LIGHT_INSTANCE") {
        let n = p.node;
        let w = &p.world;
        let num = |a: &str| f64::from(floats_of(n, a).and_then(|v| v.first().copied()).unwrap_or(0.0));
        let colour = match n.get("colour") {
            Some(Value::Colour(c)) => c.map(|v| f64::from(v as f32)),
            _ => [1.0; 4],
        };
        let speed = floats_of(n, "animation_speed_scale").unwrap_or_default();
        let text = |a: &str, d: &str| Value::Str(n.get(a).and_then(Value::as_str).unwrap_or(d).to_owned());
        let mut r = Node::new("record");
        r.set("x", Value::Float(f64::from(w[3] * 0.5)));
        r.set("y", Value::Float(f64::from(w[7])));
        r.set("z", Value::Float(f64::from(extent_z - w[11] * 0.5)));
        r.set("radius", Value::Float(num("radius")));
        r.set("red", Value::Float(colour[0]));
        r.set("green", Value::Float(colour[1]));
        r.set("blue", Value::Float(colour[2]));
        r.set("colour_scale", Value::Float(num("colour_scale")));
        if version >= 3 {
            r.set("unknown_32", Value::Int(1));
        }

        r.set("animation_type", text("animation_type", "LAT_NONE"));
        r.set("animation_speed_x", Value::Float(f64::from(speed.first().copied().unwrap_or(0.0))));
        r.set("animation_speed_y", Value::Float(f64::from(speed.get(1).copied().unwrap_or(0.0))));
        r.set("colour_min", Value::Float(num("colour_min")));
        r.set("random_offset", Value::Float(num("random_offset")));
        r.set("falloff_type", text("falloff_type", "WPLFT_SMOOTH"));
        records.push((MarkerKey::Text(n.get("name").and_then(Value::as_str).unwrap_or_default().to_owned()), r));
    }

    (!records.is_empty()).then(|| markers_doc(NAME, "POINT_LIGHT_MARKER", 12_051_502, version, records))
}

/// TEd's AI hint types, in the order of their numbers (wce_tile::property_enums::AI_HINT_TYPE).
const AI_HINT_TYPES: [&str; 41] = [
    "AIH_DEFENSIVE_LINE", "AIH_DEFENSIVE_HILL", "AIH_DEAD_GROUND", "AIH_IMPASSIBLE", "AIH_TOWN_CLUSTER", "AIH_CHOKE_POINT", "AIH_GOOD_CAVALRY_GROUND",
    "AIH_COVER", "AIH_AMBUSH_FOREST", "AIH_AMBUSH_BUILDINGS", "AIH_REDOUBT", "AIH_SUITABLE_GROUND_BOMBARDMENT", "AIH_SUITABLE_DEPLOYABLE_GABIONADE",
    "AIH_SUITABLE_DEPLOYABLE_FOUGASSE", "AIH_SUITABLE_DEPLOYABLE_CHEVAUX_DE_FRISE", "AIH_SUITABLE_DEPLOYABLE_EARTHWORKS", "AIH_FORDING_POINT",
    "AIH_FORT_APPROACH", "AIH_BRIDGE_CROSSING", "AIH_BEACH_LANDING", "AIH_RIVER_EDGE", "AIH_SIEGE_AREA_NODE", "AIH_SIEGE_INTERSECTION_NODE",
    "AIH_SIEGE_STREET_EDGE", "AIH_CAPTURE_POINT_MAJOR", "AIH_CAPTURE_POINT_MINOR", "AIH_SIEGE_WALL_AREA_NODE", "AIH_SIEGE_AREA_CONNECTION_EDGE",
    "AIH_SIEGE_WALL_EDGE", "AIH_COVER_LINE", "AIH_SIEGE_ENTRY_NODE", "AIH_AMBUSH_MELEE_LINE", "AIH_AMBUSH_MISSILE_LINE", "AIH_AMBUSH_CAVALRY_LINE",
    "AIH_AMBUSH_DEFENDER_INITIAL_PATH", "AIH_FORT_PERIMETER", "AIH_BEACH_LANDING_FORT_APPROACH", "AIH_AMBUSH_ASSAULT_AREA", "AIH_AMBUSH_FIREBALL_LINE",
    "AIH_DEFAULT_DEPLOYMENT_LINE", "AIH_SIEGE_PALISADE_EDGE",
];

/// A polygon instance's points in the binary's frame (x / 2, extent - z / 2), through its world matrix.
fn polygon_points(p: &Placed, extent_z: f32) -> Vec<f64> {
    let Some(points) = p.node.children.iter().find(|c| c.class == "points") else { return vec![] };
    let w = &p.world;
    points
        .children
        .iter()
        .filter_map(|pt| vec3_of(pt, "position"))
        .flat_map(|q| {
            let x = w[0] * q[0] + w[1] * q[1] + w[2] * q[2] + w[3];
            let z = w[8] * q[0] + w[9] * q[1] + w[10] * q[2] + w[11];
            [f64::from(x * 0.5), f64::from(extent_z - z * 0.5)]
        })
        .collect()
}

/// `ai_hint_polyline_marker.markers` (version 1): the `AI_HINT_POLYGON_INSTANCE`s of the normal layers, a polyline each (a closed one repeats
/// its first point at the end), grouped by hint type (its number).
fn ai_hint_markers(placed: &[Placed], extent_z: f32, ctx: &CompileContext) -> Option<Document> {
    const NAME: &str = "ai_hint_polyline_marker.markers";
    let version = ctx.marker_versions.and_then(|f| f(NAME)).unwrap_or(1);
    let mut records = vec![];
    for p in placed.iter().filter(|p| p.export == "LayerExportType_Normal" && p.node.class == "AI_HINT_POLYGON_INSTANCE") {
        let n = p.node;
        let mut pts = polygon_points(p, extent_z);
        if n.get("polygon_closed").and_then(Value::as_int) == Some(1) && pts.len() >= 2 {
            pts.extend([pts[0], pts[1]]);
        }

        let ty = n.get("hint_type").and_then(Value::as_str).unwrap_or_default();
        let number = AI_HINT_TYPES.iter().position(|t| *t == ty).unwrap_or(0) as u32;
        let mut r = Node::new("record");
        r.set("points", Value::Floats(pts));
        records.push((MarkerKey::Number(number), r));
    }

    (!records.is_empty()).then(|| markers_doc(NAME, "AI_HINT_POLYLINE_MARKER", 645_568, version, records))
}

/// `ai_separator_marker.markers` (version 1): the `AISeparatorInstance`s of the normal layers, a polyline each, grouped by type
/// (`PATHFINDING_AREA_LINK_TYPE`: PALT_NONE 0, PALT_GENERIC 1, PALT_FORT 2, written by name).
fn separator_markers(placed: &[Placed], extent_z: f32, ctx: &CompileContext) -> Option<Document> {
    const NAME: &str = "ai_separator_marker.markers";
    let version = ctx.marker_versions.and_then(|f| f(NAME)).unwrap_or(1);
    let mut records = vec![];
    for p in placed.iter().filter(|p| p.export == "LayerExportType_Normal" && p.node.class == "AISeparatorInstance") {
        let ty = p.node.get("type").and_then(Value::as_str).unwrap_or("PALT_NONE").to_owned();
        let number = ["PALT_NONE", "PALT_GENERIC", "PALT_FORT"].iter().position(|t| *t == ty).unwrap_or(0) as u32;
        let mut r = Node::new("record");
        r.set("points", Value::Floats(polygon_points(p, extent_z)));
        records.push((MarkerKey::Named(ty, number), r));
    }

    (!records.is_empty()).then(|| markers_doc(NAME, "AI_SEPARATOR_MARKER", 12_120_306, version, records))
}

/// `ai_hint_polyline_list_marker.markers` (version 1), the siege graph: a record per `SiegeAINodeInstance` / `SiegeAIEdgeInstance` of the
/// normal layers, a polyline per `SIEGE_AI_POLYGON_INSTANCE` under it, grouped by the hint type of its kind (a node: area 21,
/// intersection 22, wall area 26, entry 30; an edge: street 23, area connection 27, wall 28, palisade 40).
fn siege_markers(placed: &[Placed], extent_z: f32, ctx: &CompileContext) -> Option<Document> {
    const NAME: &str = "ai_hint_polyline_list_marker.markers";
    let version = ctx.marker_versions.and_then(|f| f(NAME)).unwrap_or(1);
    // The placed nodes by address, to find a node's polygons without a scan per node.
    let by_node = placed.iter().map(|p| (std::ptr::from_ref(p.node), p)).collect::<std::collections::HashMap<_, _>>();
    let mut records = vec![];
    for p in placed.iter().filter(|p| p.export == "LayerExportType_Normal" && matches!(p.node.class.as_str(), "SiegeAINodeInstance" | "SiegeAIEdgeInstance")) {
        let ty = p.node.get("type").and_then(Value::as_str).unwrap_or_default();
        let number = match ty {
            "SANT_AREA" => 21,
            "SANT_INTERSECTION" => 22,
            "SANT_WALL_AREA" => 26,
            "SANT_ENTRY" => 30,
            "SAET_STREET" => 23,
            "SAET_AREA_CONNECTION" => 27,
            "SAET_PALISADE" => 40,
            _ => 28,
        };

        let own = p.node.wrapped("children");
        let mut lists = Node::new("polylines");
        for child in own.iter().filter(|c| c.class == "SIEGE_AI_POLYGON_INSTANCE").filter_map(|c| by_node.get(&std::ptr::from_ref(c))) {
            let mut l = Node::new("polyline");
            l.set("points", Value::Floats(polygon_points(child, extent_z)));
            lists.children.push(l);
        }

        let mut r = Node::new("record");
        r.children.push(lists);
        records.push((MarkerKey::Number(number), r));
    }

    (!records.is_empty()).then(|| markers_doc(NAME, "AI_HINT_POLYLINE_LIST_MARKER", 12_052_401, version, records))
}

/// `definition.xml`: the map's size, (cells + 2) * 256 m per axis for a battle tile (25.6 for a campaign tile), and the climate TEd
/// previewed when it exported, which is not in the project: from a `definition.xml` the project carries (a decompiled one does), else
/// `default`.
fn definition(raw: &Tile, campaign: bool, cells: (i64, i64)) -> Document {
    let carried = raw.files.iter().find(|f| f.path == "definition.xml").and_then(|f| f.data.bytes().ok()).map(|b| String::from_utf8_lossy(&b).into_owned());
    let climate = carried
        .as_deref()
        .and_then(|t| t.split("climate='").nth(1))
        .and_then(|t| t.split('\'').next())
        .unwrap_or("default")
        .to_owned();
    let unit: f32 = if campaign { 25.6 } else { 256.0 };
    let size = |c: i64| format!("{:.6}", (c + 2) as f32 * unit);
    let mut root = Node::new("BATTLE_MAP_DEFINITION");
    for (k, v) in [
        ("bmd_type", "1".to_owned()), ("terrain_type", "3".to_owned()), ("climate", climate), ("base_terrain_width", size(cells.0)),
        ("base_terrain_height", size(cells.1)), ("disable_outlines", "false".to_owned()),
    ] {
        root.set(k, Value::Str(v));
    }

    bmd_xml_layout(&mut root);
    Document { name: "definition.xml".to_owned(), root }
}

/// The BMD XML files' layout (TEd's writer): two spaces between attributes, a space before `/>`; the document's style is
/// [`bmd_xml_style`].
fn bmd_xml_layout(n: &mut Node) {
    let count = n.props().len();
    let t = n.trivia_mut();
    for i in 1..count {
        t.layout.push((crate::xml::layout::ATTR_GAP + i as u32, b"  ".to_vec().into()));
    }

    t.layout.push((crate::xml::layout::BEFORE_CLOSE, b" ".to_vec().into()));
}

/// The BMD XML files' document style: single quotes, CRLF, no declaration, a CRLF after the root.
fn bmd_xml_style() -> crate::xml::DocStyle {
    crate::xml::DocStyle { prolog: vec![], epilog: b"\r\n".to_vec(), newline: b"\r\n".to_vec(), indent: b"\t".to_vec(), quote: b'\'' }
}

/// TEd's text of a coordinate in the logic XML files: the f32 with `%.6f`.
fn coord(v: f64) -> Value {
    Value::Str(wce_formats::c_num::printf_f_msvc(f64::from(v as f32), 6))
}

/// An outline file (`go_outlines.xml`, `non_terrain_outlines.xml`): an `OUTLINE` per polygon instance of the class in the normal layers,
/// its world points as `position` (x / 2, extent - z / 2), all on one line (`outline_style`).
fn outlines(placed: &[Placed], extent_z: f32, class: &str, name: &str) -> Option<(Document, crate::xml::DocStyle)> {
    let mut root = Node::new("OUTLINE_LIST");
    for p in placed.iter().filter(|p| p.export == "LayerExportType_Normal" && p.node.class == class) {
        let mut o = Node::new("OUTLINE");
        for (i, c) in polygon_points(p, extent_z).as_chunks::<2>().0.iter().enumerate() {
            let mut pos = Node::new("position");
            pos.set("x", coord(c[0]));
            pos.set("y", coord(c[1]));
            if i > 0 {
                pos.trivia_mut().layout.push((crate::xml::layout::BEFORE_START, Box::default()));
            }

            o.children.push(pos);
        }

        o.trivia_mut().layout.push((crate::xml::layout::BEFORE_END, Box::default()));
        root.children.push(o);
    }

    (!root.children.is_empty()).then(|| (Document { name: name.to_owned(), root }, outline_style()))
}

/// An element holding a text, closed on its line (`<zone_name>ZONE_POLYGON_INSTANCE</zone_name>`).
fn text_element(class: &str, text: &str) -> Node {
    let mut n = Node::new(class);
    let mut t = Node::new(crate::xml::TEXT);
    t.set("text", Value::Str(text.to_owned()));
    n.children.push(t);
    n.trivia_mut().layout.push((crate::xml::layout::BEFORE_END, Box::default()));
    n
}

/// `zones.xml`: a `TRANSFORMED_BATTLEFIELD_ZONE_TEMPLATE` per `ZONE_POLYGON_INSTANCE` of the normal layers: the template (its name
/// `ZONE_POLYGON_INSTANCE`, the world points on one line, an empty formation template) and an identity `Transformation` (every one of
/// the packs' 1,909 is so).
fn zones(placed: &[Placed], extent_z: f32) -> Option<(Document, crate::xml::DocStyle)> {
    use crate::xml::layout::{BEFORE_START, EMPTY_PAIR};
    let mut root = Node::new("BATTLEFIELD_ZONE_TEMPLATE_LIST");
    for p in placed.iter().filter(|p| p.export == "LayerExportType_Normal" && p.node.class == "ZONE_POLYGON_INSTANCE") {
        let mut template = Node::new("BATTLEFIELD_ZONE_TEMPLATE");
        template.children.push(text_element("zone_name", "ZONE_POLYGON_INSTANCE"));
        for (i, c) in polygon_points(p, extent_z).as_chunks::<2>().0.iter().enumerate() {
            let mut pos = Node::new("position");
            pos.set("x", coord(c[0]));
            pos.set("y", coord(c[1]));
            if i > 0 {
                pos.trivia_mut().layout.push((BEFORE_START, Box::default()));
            }

            template.children.push(pos);
        }

        let mut formation = Node::new("ENTITY_FORMATION_TEMPLATE");
        formation.set("name", Value::Str(String::new()));
        formation.trivia_mut().layout.extend([(BEFORE_START, Box::default()), (EMPTY_PAIR, b"\r\n".to_vec().into())]);
        template.children.push(formation);
        let mut transformation = Node::new("Transformation");
        for k in 0..16 {
            transformation.children.push(text_element(&format!("element_{k}"), if k % 5 == 0 { "1" } else { "0" }));
        }

        let mut t = Node::new("TRANSFORMED_BATTLEFIELD_ZONE_TEMPLATE");
        t.children.extend([template, transformation]);
        root.children.push(t);
    }

    (!root.children.is_empty()).then(|| (Document { name: "zones.xml".to_owned(), root }, outline_style()))
}

/// `deployment_areas.xml`: per alliance (by id) its areas (`DeploymentZonePolygonInstanceGroup`s of the normal layers: `alliance_id`,
/// `deployment_zone_id`, `deployment_category`), each with a `boundary` per `DEPLOYMENT_ZONE_POLYGON_INSTANCE` under it (its type, its
/// world points) and the facing direction. TEd clips the boundaries to the playable area on export (a decompiled project already holds
/// the clipped ones); that is not done here yet.
fn deployment(placed: &[Placed], extent_z: f32) -> Option<(Document, crate::xml::DocStyle)> {
    let by_node = placed.iter().map(|p| (std::ptr::from_ref(p.node), p)).collect::<std::collections::HashMap<_, _>>();
    let mut alliances: std::collections::BTreeMap<i64, Vec<Node>> = std::collections::BTreeMap::new();
    for p in placed.iter().filter(|p| p.export == "LayerExportType_Normal" && p.node.class == "DeploymentZonePolygonInstanceGroup") {
        let n = p.node;
        let int = |a: &str| n.get(a).and_then(Value::as_int).unwrap_or(0);
        let mut area = Node::new("deployment_area");
        area.set("id", Value::Str(int("deployment_zone_id").to_string()));
        area.set("category", Value::Str(n.get("deployment_category").and_then(Value::as_str).unwrap_or("DAC_DEFAULT").to_owned()));
        for c in n.wrapped("children").iter().filter(|c| c.class == "DEPLOYMENT_ZONE_POLYGON_INSTANCE").filter_map(|c| by_node.get(&std::ptr::from_ref(c))) {
            let kind = match c.node.get("deployment_polygon_type").and_then(Value::as_str).unwrap_or_default() {
                "DZPT_ADDITIVE" => "standard additive",
                "DZPT_SUBSTRACTIVE" => "standard subtractive",
                "DZPT_GUERRILLA_EXCLUSION_ADDITIVE" => "guerrilla exclusion additive",
                _ => "guerrilla exclusion subtractive",
            };
            let mut b = Node::new("boundary");
            b.set("type", Value::Str(kind.to_owned()));
            for xy in polygon_points(c, extent_z).as_chunks::<2>().0 {
                let mut pos = Node::new("position");
                pos.set("x", coord(xy[0]));
                pos.set("y", coord(xy[1]));
                b.children.push(pos);
            }

            area.children.push(b);
        }

        if let Some(f) = floats_of(n, "facing_direction").and_then(|v| v.first().copied()) {
            let mut o = Node::new("orientation");
            o.set("radians", coord(f64::from(f)));
            area.children.push(o);
        }

        alliances.entry(int("alliance_id")).or_default().push(area);
    }

    if alliances.is_empty() {
        return None;
    }

    let mut areas = Node::new("BATTLE_DEPLOYMENT_AREAS");
    for (id, list) in alliances {
        let mut a = Node::new("ALLIANCE");
        a.set("id", Value::Str(id.to_string()));
        a.children = list;
        areas.children.push(a);
    }

    let mut root = Node::new("BATTLE_DEPLOYMENT_AREA_HASH_TABLE");
    root.children.push(areas);
    Some((Document { name: "deployment_areas.xml".to_owned(), root }, bmd_xml_style()))
}

/// `capture_location_list.xml`: the `CaptureLocationInstance`s of the normal layers (the escalation layers' go into their BMDs). The
/// flag's world position, the boundary (a circle: half TEd's radius and its centre, the instance's position; a polygon: radius 0 and its
/// points), players, type, key, the facing as (cos, -sin), and the main list's towers and gates at it (`building_list`, their indices).
/// `buildings` are the main list's (key, x, z).
fn capture_locations(placed: &[Placed], extent_z: f32, buildings: &[(String, f32, f32)], ctx: &CompileContext) -> Option<(Document, crate::xml::DocStyle)> {
    use crate::xml::layout::BEFORE_START;
    let by_node = placed.iter().map(|p| (std::ptr::from_ref(p.node), p)).collect::<std::collections::HashMap<_, _>>();
    let ted = |v: f32| crate::xml::values::Num::Ted.format(f64::from(v));
    // flag_facing keeps one decimal (`1.0,-0.0`).
    let one = |v: f32| {
        let t = ted(v);
        if t.contains('.') { t } else { format!("{t}.0") }
    };
    let mut list = Node::new("CAPTURE_LOCATION_LIST");
    for p in placed.iter().filter(|p| p.export == "LayerExportType_Normal" && p.node.class == "CaptureLocationInstance") {
        let n = p.node;
        let w = &p.world;
        let fp = vec3_of(n, "flag_position").unwrap_or([0.0; 3]);
        let (fx, fz) = (w[0] * fp[0] + w[1] * fp[1] + w[2] * fp[2] + w[3], w[8] * fp[0] + w[9] * fp[1] + w[10] * fp[2] + w[11]);
        let boundary = n.wrapped("children").iter().find(|c| c.class == "CaptureLocationPolygonInstance").and_then(|c| by_node.get(&std::ptr::from_ref(c)));
        let circle = boundary.is_some_and(|b| b.node.get("polygon_shape").and_then(Value::as_str) == Some("PS_CIRCLE"));
        let (radius, points): (f32, Vec<(f32, f32)>) = match boundary {
            Some(b) if circle => {
                let r = floats_of(b.node, "radius").and_then(|v| v.first().copied()).unwrap_or(0.0) * 0.5;
                (r, vec![(w[3] * 0.5, extent_z - w[11] * 0.5)])
            }
            Some(b) => (0.0, polygon_points(b, extent_z).as_chunks::<2>().0.iter().map(|c| (c[0] as f32, c[1] as f32)).collect()),
            None => (0.0, vec![]),
        };

        // TEd links the towers and gates at the capture location (their footprints, not known here): those within 20 m of its boundary
        // (camps' corner towers 6 to 16 m from a 7.5 to 10 m circle, the next 47 m off; a gate and its two towers at a polygon).
        let near = |x: f32, z: f32| {
            if circle {
                return points.first().is_some_and(|c| (x - c.0).hypot(z - c.1) <= radius + 20.0);
            }

            inside(&points, x, z)
                || (0..points.len()).any(|k| {
                    let (a, b) = (points[k], points[(k + 1) % points.len()]);
                    let (dx, dz) = (b.0 - a.0, b.1 - a.1);
                    let t = (((x - a.0) * dx + (z - a.1) * dz) / (dx * dx + dz * dz).max(1e-12)).clamp(0.0, 1.0);
                    (x - (a.0 + t * dx)).hypot(z - (a.1 + t * dz)) <= 20.0
                })
        };
        let category = |k: &str| ctx.building_category.and_then(|f| f(k)).unwrap_or_default();
        let building = buildings
            .iter()
            .enumerate()
            .filter(|(_, (k, x, z))| !points.is_empty() && matches!(category(k).as_str(), "fort_tower" | "gate") && near(*x, *z))
            .map(|(i, _)| i.to_string())
            .collect::<Vec<_>>()
            .join(";");
        let facing = floats_of(n, "flag_facing").and_then(|v| v.first().copied()).unwrap_or(0.0);
        let int = |a: &str, d: i64| n.get(a).and_then(Value::as_int).unwrap_or(d).to_string();
        let mut c = Node::new("CAPTURE_LOCATION");
        for (k, v) in [
            ("radius", wce_formats::c_num::printf_f_msvc(f64::from(radius), 6)),
            ("position_x", wce_formats::c_num::printf_f_msvc(f64::from(fx * 0.5), 6)),
            ("position_y", wce_formats::c_num::printf_f_msvc(f64::from(extent_z - fz * 0.5), 6)),
            ("valid_for_min_num_players", int("min_num_players", 2)), ("valid_for_max_num_players", int("max_num_players", 8)),
            (
                "capture_point_type_v2",
                if n.get("type").and_then(Value::as_str) == Some("CLT_MAJOR") { "CAPTURE_LOCATION_MAJOR" } else { "CAPTURE_LOCATION_MINOR" }.to_owned(),
            ),
            ("capture_point_list", points.iter().map(|&(x, z)| format!("{},{}", ted(x), ted(z))).collect::<Vec<_>>().join(";")),
            ("building_list", building), ("database_key", n.get("database_type").and_then(Value::as_str).unwrap_or_default().to_owned()),
            ("flag_facing", format!("{},{}", one(facing.cos()), one(-facing.sin()))),
        ] {
            c.set(k, Value::Str(v));
        }

        bmd_xml_layout(&mut c);
        if list.children.is_empty() {
            c.trivia_mut().layout.push((BEFORE_START, b"\r\n\t\r\n".to_vec().into()));
        }

        list.children.push(c);
    }

    if list.children.is_empty() {
        return None;
    }

    list.trivia_mut().layout.push((BEFORE_START, b"\r\n\r\n\t\t".to_vec().into()));
    let mut root = Node::new("CAPTURE_LOCATION_SET");
    root.children.push(list);
    Some((Document { name: "capture_location_list.xml".to_owned(), root }, outline_style()))
}

/// The outline files' style: no indentation, single quotes, CRLF.
fn outline_style() -> crate::xml::DocStyle {
    crate::xml::DocStyle { indent: vec![], ..bmd_xml_style() }
}

/// The stage-A part of a project's exported tile (TEd's export), as the model `attila.tile_binary` writes: the building lists; the prop,
/// particle emitter, point light, AI hint, separator and siege graph markers; definition.xml (with its document style).
///
/// # Errors
///
/// The project has no scene or no entry to size it.
pub fn compile(raw: &Tile, ctx: &CompileContext) -> Result<(Tile, CompileNotes), String> {
    let scene = raw.scene.as_ref().ok_or("the project has no tile.ted")?;
    let entry = raw.entry.as_ref().ok_or("the project has no entry")?;
    let height = entry.root.get("height").and_then(Value::as_int).ok_or("the entry has no height")?;
    let width = entry.root.get("width").and_then(Value::as_int).unwrap_or(height);
    let campaign = entry.root.get("location").and_then(Value::as_str).is_some_and(|l| l.to_lowercase().contains("\\campaign\\"));
    let extent_z = height as f32 * 128.0;
    let mut placed = vec![];
    walk(scene, None, "LayerExportType_Normal", "", &mut placed);
    let mut notes = CompileNotes::default();
    let mut docs = building_lists(&placed, extent_z, ctx, &mut notes);
    docs.extend(prop_markers(&placed, extent_z, ctx));
    docs.extend(emitter_markers(&placed, extent_z, ctx));
    docs.extend(light_markers(&placed, extent_z, ctx));
    docs.extend(ai_hint_markers(&placed, extent_z, ctx));
    docs.extend(separator_markers(&placed, extent_z, ctx));
    docs.extend(siege_markers(&placed, extent_z, ctx));
    let mut xml = vec![(definition(raw, campaign, (width, height)), bmd_xml_style())];
    xml.extend(outlines(&placed, extent_z, "GO_REGION_POLYGON_INSTANCE", "go_outlines.xml"));
    xml.extend(outlines(&placed, extent_z, "NOGO_REGION_POLYGON_INSTANCE", "non_terrain_outlines.xml"));
    xml.extend(zones(&placed, extent_z));
    xml.extend(deployment(&placed, extent_z));
    // The main list's buildings, for the capture locations' links.
    let main = docs.iter().find(|d| d.name == "building_list").map_or(vec![], |d| {
        d.root
            .children
            .iter()
            .filter_map(|b| Some((b.get("key")?.as_str()?.to_owned(), b.get("position").and_then(Value::as_vec3)?)))
            .map(|(k, p)| (k, p[0] as f32, p[2] as f32))
            .collect::<Vec<_>>()
    });
    xml.extend(capture_locations(&placed, extent_z, &main, ctx));
    let mut out = Tile { source_format: Some(super::tile_binary::ID), variation: raw.variation.clone(), ..Default::default() };
    for (d, style) in xml {
        out.source.insert(d.name.clone(), std::sync::Arc::new(style));
        docs.push(d);
    }

    out.documents = docs;
    Ok((out, notes))
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
    fn marker_table_orders_like_ted() {
        // tdd_settlement_dwarven_cities/dwarven_city_a/small's prop_marker.markers: keys in the order of their first record, and the
        // groups' order in the file.
        let text = |v: &[&str]| v.iter().map(|k| MarkerKey::Text((*k).to_owned())).collect::<Vec<_>>();
        let first = text(&[
            "RigidModels/Buildings/water_plane/water_plane.rigid_model_v2", "rigidmodels\\decals\\battle\\road_1.rigid_model_v2",
            "rigidmodels\\decals\\battle\\road_7.rigid_model_v2", "rigidmodels\\decals\\battle\\road_4.rigid_model_v2",
            "rigidmodels\\decals\\battle\\road_5.rigid_model_v2",
        ]);
        let file = text(&[
            "rigidmodels\\decals\\battle\\road_4.rigid_model_v2", "rigidmodels\\decals\\battle\\road_5.rigid_model_v2",
            "rigidmodels\\decals\\battle\\road_7.rigid_model_v2", "RigidModels/Buildings/water_plane/water_plane.rigid_model_v2",
            "rigidmodels\\decals\\battle\\road_1.rigid_model_v2",
        ]);
        assert_eq!(marker_table_order(&first), file);

        // Numbers hash to themselves: slot = n % 13 (27 -> 1, 28 -> 2, 30 -> 4, 21 -> 8, 22 -> 9, 23 -> 10); 14 more grow the table.
        let numbers = |v: &[u32]| v.iter().map(|&n| MarkerKey::Number(n)).collect::<Vec<_>>();
        assert_eq!(marker_table_order(&numbers(&[21, 23, 27, 22, 28, 30])), numbers(&[27, 28, 30, 21, 22, 23]));
        let many = (0..14).map(|n| n * 13).collect::<Vec<u32>>();
        assert_eq!(marker_table_order(&numbers(&many)).len(), 14);
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
        let (out, _) = compile(&tile, &ctx).expect("compile");
        let docs = out.documents.iter().filter(|d| d.name.starts_with("building_list")).cloned().collect::<Vec<_>>();
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
