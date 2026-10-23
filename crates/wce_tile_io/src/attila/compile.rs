//! A TEd project as an exported (binary) tile: the model `attila.ted_project` reads turned into the documents `attila.tile_binary` writes
//! (docs/plans/tile_roundtrip.md, Phase 2 step 4). Stage A, TEd's export of the logic, first (docs/formats/tile_compile.md).
//!
//! The numbers go through TEd's float arithmetic: a value is read from its text as `sscanf` reads it (f32), the world matrices are
//! `QTU::TedEntity::world_transform`'s (`wce_tile::world_transform::ted_rotation_f32` / `ted_mul_f32`).

use wce_tile::world_transform::{ted_mul_f32, ted_rotation_f32};
use wce_tile_model::{Document, Node, Sym, Tile, Value};

use super::decompile::{LevelsOf, ThresholdsOf};
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

    /// The version to write a markers file in (by file name); without it TEd's current one.
    pub marker_versions: Option<VersionOf<'a>>,

    /// Stage B too (BOB's Tile processor: textures, meshes, heights, outlines), with what it needs from the kit; without it, stage A
    /// alone.
    pub stage_b: Option<super::stage_b::StageB<'a>>,
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

    // A normal layer inside another exports with it (the kit's escalation layers hold their content in normal sub-layers).
    let (export, layer) = match n.get("export_type").and_then(Value::as_str).filter(|_| n.class == "LayerInstance") {
        Some(e) if e != "LayerExportType_Normal" || export == "LayerExportType_Normal" => (e, n.get("name").and_then(Value::as_str).unwrap_or_default()),
        None if n.class == "LayerInstance" && export == "LayerExportType_Normal" => ("LayerExportType_Normal", n.get("name").and_then(Value::as_str).unwrap_or_default()),
        _ => (export, layer),
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

        let lite = name == "building_list" && zones.iter().any(|z| inside(z, p.world[3], z_of(&p.world)));
        let b = building_record(p, extent_z, ctx, lite, &mut unknown_levels);
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

/// A building of an exported list (the building list's record) from its BUILDING_INSTANCE (`Ted::BMDExporter::export_`); a building
/// whose destruction levels are not known counts in `unknown_levels` (starting damage 1).
fn building_record(p: &Placed, extent_z: f32, ctx: &CompileContext, lite: bool, unknown_levels: &mut usize) -> Node {
    let n = p.node;
    let w = &p.world;
    let (x, y, z) = (w[3], w[7], w[11]);
    let key = n.get("key").and_then(Value::as_str).unwrap_or_default();
    let level = n.get("destruction_level").and_then(Value::as_int).unwrap_or(0);
    // {level, min 0, max levels - 1}: without `destruct_thresholds`, level / max (past 1 for a level past the model's); with them, level 0
    // is 0, level max 1, any other its threshold (index level - 1).
    let thresholds = ctx.building_thresholds.and_then(|f| f(key)).unwrap_or_default();
    let damage = match (level, ctx.building_levels.and_then(|f| f(key))) {
        (0, _) => 0.0,
        (l, Some(levels)) if levels > 1 && !thresholds.is_empty() => {
            if l == i64::from(levels - 1) { 1.0 } else { thresholds.get(l as usize - 1).copied().unwrap_or(1.0) }
        }
        (l, Some(levels)) if levels > 1 => (l as f32) / ((levels - 1) as f32),
        _ => {
            *unknown_levels += 1;
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
    b.set("lite", Value::Bool(lite));
    b
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
pub(crate) enum MarkerKey {
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

/// The slots of a `MARKER_TABLE` after inserting the keys in this order (the index of the key in each slot): 13 slots, linear probing,
/// growing to 2 n + 1 when full by re-inserting the keys in slot order.
fn marker_table(keys: &[MarkerKey]) -> Vec<Option<usize>> {
    let insert = |slots: &mut Vec<Option<usize>>, i: usize| -> bool {
        let n = slots.len();
        let start = keys[i].hash() as usize % n;
        match (0..n).map(|j| (start + j) % n).find(|&j| slots[j].is_none()) {
            Some(j) => {
                slots[j] = Some(i);
                true
            }
            None => false,
        }
    };

    let mut slots: Vec<Option<usize>> = vec![None; 13];
    for i in 0..keys.len() {
        if !insert(&mut slots, i) {
            let old = slots.iter().flatten().copied().collect::<Vec<_>>();
            slots = vec![None; slots.len() * 2 + 1];
            for o in old {
                insert(&mut slots, o);
            }

            insert(&mut slots, i);
        }
    }

    slots
}

/// The order a `MARKER_TABLE` gives its keys (inserted in the order of their first record): the keys in slot order.
pub(crate) fn marker_table_order(keys: &[MarkerKey]) -> Vec<MarkerKey> {
    marker_table(keys).into_iter().flatten().map(|i| keys[i].clone()).collect()
}

/// A small deterministic generator (xorshift), so a decompiled project is the same every time.
struct Xorshift(u64);

impl Xorshift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// The keys between each key's home slot and its slot (indices into `keys`), at a capacity; `None` when one of those slots is empty.
fn probe_deps(keys: &[MarkerKey], slot: &[usize], cap: usize) -> Option<Vec<Vec<usize>>> {
    let at = slot.iter().enumerate().map(|(i, &s)| (s, i)).collect::<std::collections::HashMap<_, _>>();
    keys.iter()
        .enumerate()
        .map(|(i, k)| {
            let (mut d, mut j) = (vec![], k.hash() as usize % cap);
            while j != slot[i] {
                d.push(*at.get(&j)?);
                j = (j + 1) % cap;
            }

            Some(d)
        })
        .collect()
}

/// An insertion order (indices into `keys`) that leaves each key in its `slot` of a table of `cap` slots, growth included: the keys
/// inserted before the last growth are re-inserted in their slot order there (a full table of the previous capacity), which must
/// itself be reachable. That order is drawn among those that respect the probing, favouring keys near their home slot below.
fn growth_order(keys: &[MarkerKey], slot: &[usize], cap: usize, rng: &mut Xorshift, budget: usize) -> Option<Vec<usize>> {
    let deps = probe_deps(keys, slot, cap)?;
    let n = keys.len();
    let ready = |done: &[bool], i: usize| !done[i] && deps[i].iter().all(|&d| done[d]);
    if cap == 13 {
        let (mut done, mut out) = (vec![false; n], vec![]);
        while out.len() < n {
            let i = (0..n).find(|&i| ready(&done, i))?;
            done[i] = true;
            out.push(i);
        }

        return Some(out);
    }

    let prev = (cap - 1) / 2;
    for _ in 0..budget {
        let (mut done, mut tau) = (vec![false; n], Vec::with_capacity(n));
        while tau.len() < n {
            let r = (0..n).filter(|&i| ready(&done, i)).collect::<Vec<_>>();
            if r.is_empty() {
                return None;
            }

            let pos = tau.len();
            let pick = if pos < prev {
                // Weighted by the distance from the key's home in the smaller table to the slot it would take there.
                let w = r.iter().map(|&i| (-0.7 * ((pos + prev - keys[i].hash() as usize % prev) % prev) as f64).exp()).collect::<Vec<_>>();
                let mut x = rng.unit() * w.iter().sum::<f64>();
                let mut k = r.len() - 1;
                for (j, wj) in w.iter().enumerate() {
                    if x < *wj {
                        k = j;
                        break;
                    }

                    x -= wj;
                }

                r[k]
            } else {
                r[rng.below(r.len())]
            };
            done[pick] = true;
            tau.push(pick);
        }

        let early = tau[..prev].iter().map(|&i| keys[i].clone()).collect::<Vec<_>>();
        if let Some(sub) = growth_order(&early, &(0..prev).collect::<Vec<_>>(), prev, rng, (budget / 4).max(1)) {
            return Some(sub.into_iter().map(|j| tau[j]).chain(tau[prev..].iter().copied()).collect());
        }
    }

    None
}

/// An insertion order of keys whose [`marker_table_order`] is `order`, for files that keep only the grouped records (the BMDs' props
/// and emitters). Linear probing fills the same slots whatever the order, so `order` gives every key its final slot, and a key must come
/// after the keys between its home slot and its own; [`growth_order`] searches the orders that respect that through the table's growth,
/// and a local search (simulated annealing on the keys left in their slot) takes over where it finds none.
pub(crate) fn marker_insertion_order(order: &[MarkerKey]) -> Option<Vec<MarkerKey>> {
    let n = order.len();
    let mut cap = 13;
    while cap < n {
        cap = cap * 2 + 1;
    }

    let mut occupied = vec![false; cap];
    for k in order {
        let home = k.hash() as usize % cap;
        let j = (0..cap).map(|i| (home + i) % cap).find(|&j| !occupied[j])?;
        occupied[j] = true;
    }

    let slot = (0..cap).filter(|&j| occupied[j]).collect::<Vec<_>>();
    let keys_of = |o: &[usize]| o.iter().map(|&i| order[i].clone()).collect::<Vec<_>>();
    let mut rng = Xorshift(0x9e37_79b9_7f4a_7c15);
    if let Some(o) = growth_order(order, &slot, cap, &mut rng, 200)
        && marker_table_order(&keys_of(&o)) == order
    {
        return Some(keys_of(&o));
    }

    // The local search: move one key at a time, keeping a worse order now and then while the temperature is high.
    let score = |o: &[usize]| {
        let table = marker_table(&keys_of(o));
        table.iter().enumerate().filter(|(j, k)| k.is_some_and(|k| slot.get(o[k]) == Some(j))).count()
    };
    let mut cur = (0..n).collect::<Vec<_>>();
    let (mut sc, mut t) = (score(&cur), 2.0f64);
    for _ in 0..100_000 {
        if sc == n {
            return Some(keys_of(&cur));
        }

        let mut next = cur.clone();
        let x = next.remove(rng.below(n));
        next.insert(rng.below(n), x);
        let s2 = score(&next);
        if s2 >= sc || rng.unit() < ((s2 as f64 - sc as f64) / t).exp() {
            (cur, sc) = (next, s2);
        }

        t = (t * 0.9999).max(0.05);
    }

    None
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
    let records = placed.iter().filter(|p| p.export == "LayerExportType_Normal").filter_map(|p| prop_record(p, extent_z, version)).collect::<Vec<_>>();
    (!records.is_empty()).then(|| markers_doc(NAME, "PROP_MARKER", 645_566, version, records))
}

/// A prop marker record (of the markers version) and its key from a `ProjectedDecalInstance` or `SIMPLE_MESH_INSTANCE`.
fn prop_record(p: &Placed, extent_z: f32, version: i64) -> Option<(MarkerKey, Node)> {
    let n = p.node;
    let decal = match n.class.as_str() {
        "ProjectedDecalInstance" => true,
        "SIMPLE_MESH_INSTANCE" => false,
        _ => return None,
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

    Some((MarkerKey::Text(key), r))
}

/// `particle_emitter_marker.markers` (version 9; 4 to 8 for the older tiles' loop): the `VFXInstance`s of the normal layers, in the scene's order, grouped by `vfx`. The
/// matrix holds the world rotation transposed (not mirrored) and the translation (x / 2, y, extent - z / 2); `scale` is TEd's halved.
fn emitter_markers(placed: &[Placed], extent_z: f32, ctx: &CompileContext) -> Option<Document> {
    const NAME: &str = "particle_emitter_marker.markers";
    let version = ctx.marker_versions.and_then(|f| f(NAME)).unwrap_or(9);
    let records = placed.iter().filter(|p| p.export == "LayerExportType_Normal" && p.node.class == "VFXInstance").map(|p| emitter_record(p, extent_z, version)).collect::<Vec<_>>();
    (!records.is_empty()).then(|| markers_doc(NAME, "PARTICLE_EMITTER_MARKER", 618_864, version, records))
}

/// A particle emitter marker record (of the markers version) and its key from a `VFXInstance`.
fn emitter_record(p: &Placed, extent_z: f32, version: i64) -> (MarkerKey, Node) {
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
    (MarkerKey::Text(n.get("vfx").and_then(Value::as_str).unwrap_or_default().to_owned()), r)
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
    let by_node = by_node(placed);
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

/// The go outlines of `go_outlines.xml` (see [`outlines`]) as BOB reads that file back: each point's `%.6f` text through CA's float
/// parser (`river::ca_parse_f32`), x and y (y = extent - z / 2).
pub fn go_outlines(raw: &Tile) -> Vec<Vec<(f32, f32)>> {
    let (Some(scene), Some(entry)) = (raw.scene.as_ref(), raw.entry.as_ref()) else { return vec![] };
    let Some(height) = entry.root.get("height").and_then(Value::as_int) else { return vec![] };
    let extent_z = height as f32 * 128.0;
    let mut placed = vec![];
    walk(scene, None, "LayerExportType_Normal", "", &mut placed);
    let read = |v: f64| coord(v).as_str().and_then(super::river::ca_parse_f32).unwrap_or(0.0);
    placed
        .iter()
        .filter(|p| p.export == "LayerExportType_Normal" && p.node.class == "GO_REGION_POLYGON_INSTANCE")
        .map(|p| polygon_points(p, extent_z).as_chunks::<2>().0.iter().map(|c| (read(c[0]), read(c[1]))).collect::<Vec<_>>())
        .filter(|o| !o.is_empty())
        .collect()
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

/// TEd's rounded literals for a quarter and a full turn (a deployment area's orientation): `1.570796` and `6.283185`, not the f32 of
/// pi / 2 and 2 pi (the packs' `3.141592`).
#[allow(clippy::approx_constant)]
pub(crate) const TED_HALF_PI: f32 = 1.570_796;
#[allow(clippy::approx_constant)]
pub(crate) const TED_TWO_PI: f32 = 6.283_185;

/// A side of the playable area: whether a point is inside, and where a segment crosses it.
type ClipSide<'a> = (&'a dyn Fn((f32, f32)) -> bool, &'a dyn Fn((f32, f32), (f32, f32)) -> (f32, f32));

/// A deployment boundary (the binary's frame) clipped to the playable area, the tile less 32.5 m on every side (Sutherland-Hodgman against
/// y >= 32.5, x <= width - 32.5, y <= height - 32.5, x >= 32.5 in that order, each edge from the previous point: the order and start TEd's
/// output has, on all 17 clipped boundaries of 9 kit tiles).
fn clip_to_playable(points: &[(f32, f32)], extent: (f32, f32)) -> Vec<(f32, f32)> {
    const MARGIN: f32 = 32.5;
    let (w, h) = (extent.0 - MARGIN, extent.1 - MARGIN);
    // In double (the x87), stored as f32.
    let lerp = |a: f32, b: f32, t: f64| (f64::from(a) + t * (f64::from(b) - f64::from(a))) as f32;
    let at_x = |a: (f32, f32), b: (f32, f32), x: f32| (x, lerp(a.1, b.1, (f64::from(x) - f64::from(a.0)) / (f64::from(b.0) - f64::from(a.0))));
    let at_y = |a: (f32, f32), b: (f32, f32), y: f32| (lerp(a.0, b.0, (f64::from(y) - f64::from(a.1)) / (f64::from(b.1) - f64::from(a.1))), y);
    let sides: [ClipSide; 4] = [
        (&|p| p.1 >= MARGIN, &|a, b| at_y(a, b, MARGIN)),
        (&|p| p.0 <= w, &|a, b| at_x(a, b, w)),
        (&|p| p.1 <= h, &|a, b| at_y(a, b, h)),
        (&|p| p.0 >= MARGIN, &|a, b| at_x(a, b, MARGIN)),
    ];
    let mut poly = points.to_vec();
    for (inside, cross) in sides {
        let mut out = vec![];
        for k in 0..poly.len() {
            let (s, p) = (poly[(k + poly.len() - 1) % poly.len()], poly[k]);
            if inside(p) {
                if !inside(s) {
                    out.push(cross(s, p));
                }

                out.push(p);
            } else if inside(s) {
                out.push(cross(s, p));
            }
        }

        poly = out;
    }

    poly
}

/// `deployment_areas.xml`: per alliance (by id) its areas (`DeploymentZonePolygonInstanceGroup`s of the normal layers: `alliance_id`,
/// `deployment_zone_id`, `deployment_category`), each with a `boundary` per `DEPLOYMENT_ZONE_POLYGON_INSTANCE` under it (its type, its
/// world points, clipped to the playable area: [`clip_to_playable`]) and the facing direction.
fn deployment(placed: &[Placed], extent: (f32, f32)) -> Option<(Document, crate::xml::DocStyle)> {
    let extent_z = extent.1;
    let by_node = by_node(placed);
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
            let points = polygon_points(c, extent_z).as_chunks::<2>().0.iter().map(|xy| (xy[0] as f32, xy[1] as f32)).collect::<Vec<_>>();
            for (x, y) in clip_to_playable(&points, extent) {
                let mut pos = Node::new("position");
                pos.set("x", coord(f64::from(x)));
                pos.set("y", coord(f64::from(y)));
                b.children.push(pos);
            }

            area.children.push(b);
        }

        // The facing turned a quarter, wrapped into (-pi, pi], in f32 with TEd's constants 1.570796 and 6.283185 (the kit's 0, 1.570796
        // and 4.712389 give the packs' 1.570796, 3.141592 and 0).
        if let Some(f) = floats_of(n, "facing_direction").and_then(|v| v.first().copied()) {
            let mut r = f + TED_HALF_PI;
            if r > std::f32::consts::PI {
                r -= TED_TWO_PI;
            }

            let mut o = Node::new("orientation");
            o.set("radians", coord(f64::from(r)));
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

/// A capture location as TEd exports it (the binary's frame): the flag's position, the boundary (a circle: half TEd's radius and its
/// centre; a polygon: radius 0 and its points), players, type, key, the facing angle and the linked buildings (indices in `buildings`).
struct Capture {
    radius: f32,
    flag: (f32, f32),
    points: Vec<(f32, f32)>,
    min_players: i64,
    max_players: i64,
    major: bool,
    key: String,
    facing: f32,
    links: Vec<usize>,
}

/// The nodes of a scene by address, for finding a node's placement.
type ByNode<'p, 'a> = std::collections::HashMap<*const Node, &'p Placed<'a>>;

fn by_node<'p, 'a>(placed: &'p [Placed<'a>]) -> ByNode<'p, 'a> {
    placed.iter().map(|p| (std::ptr::from_ref(p.node), p)).collect()
}

/// The `capture_location` of each building among the placed nodes, in the list's order.
fn capture_names<'p, 'a: 'p>(placed: impl Iterator<Item = &'p Placed<'a>>) -> Vec<Option<String>> {
    placed
        .filter(|p| p.node.class == "BUILDING_INSTANCE")
        .map(|p| p.node.get("capture_location").and_then(Value::as_str).filter(|c| !c.is_empty()).map(str::to_owned))
        .collect()
}

/// A `CaptureLocationInstance` as a [`Capture`]; `buildings` are the `capture_location`s of the list it links into (the main list, or
/// its escalation layer's).
fn capture_record(p: &Placed, by_node: &ByNode, extent_z: f32, buildings: &[Option<String>]) -> Capture {
    let n = p.node;
    let w = &p.world;
    let fp = vec3_of(n, "flag_position").unwrap_or([0.0; 3]);
    let (fx, fz) = (w[0] * fp[0] + w[1] * fp[1] + w[2] * fp[2] + w[3], w[8] * fp[0] + w[9] * fp[1] + w[10] * fp[2] + w[11]);
    let boundary = n.wrapped("children").iter().find(|c| c.class == "CaptureLocationPolygonInstance").and_then(|c| by_node.get(&std::ptr::from_ref(c)));
    let circle = boundary.is_some_and(|b| b.node.get("polygon_shape").and_then(Value::as_str) == Some("PS_CIRCLE"));
    let (radius, points): (f32, Vec<(f32, f32)>) = match boundary {
        Some(b) if circle => {
            // The circle's centre is its polygon's position (the kit's sit off their capture location's).
            let r = floats_of(b.node, "radius").and_then(|v| v.first().copied()).unwrap_or(0.0) * 0.5;
            (r, vec![(b.world[3] * 0.5, extent_z - b.world[11] * 0.5)])
        }
        Some(b) => (0.0, polygon_points(b, extent_z).as_chunks::<2>().0.iter().map(|c| (c[0] as f32, c[1] as f32)).collect()),
        None => (0.0, vec![]),
    };

    // TEd links the buildings whose `capture_location` is the capture location's name (towers and gates, set in their properties).
    let name = n.get("name").and_then(Value::as_str).unwrap_or_default();
    let links = buildings.iter().enumerate().filter(|(_, c)| !name.is_empty() && c.as_deref() == Some(name)).map(|(i, _)| i).collect();
    let int = |a: &str, d: i64| n.get(a).and_then(Value::as_int).unwrap_or(d);
    Capture {
        radius,
        flag: (fx * 0.5, extent_z - fz * 0.5),
        points,
        min_players: int("min_num_players", 2),
        max_players: int("max_num_players", 8),
        major: n.get("type").and_then(Value::as_str) == Some("CLT_MAJOR"),
        key: n.get("database_type").and_then(Value::as_str).unwrap_or_default().to_owned(),
        facing: floats_of(n, "flag_facing").and_then(|v| v.first().copied()).unwrap_or(0.0),
        links,
    }
}

/// `capture_location_list.xml`: the `CaptureLocationInstance`s of the normal layers (the escalation layers' go into their BMDs), linking
/// into the main list (`buildings`, their `capture_location`s). The facing is written as (cos, -sin).
fn capture_locations(placed: &[Placed], extent_z: f32, buildings: &[Option<String>]) -> Option<(Document, crate::xml::DocStyle)> {
    use crate::xml::layout::BEFORE_START;
    let by_node = by_node(placed);
    let ted = |v: f32| crate::xml::values::Num::Ted.format(f64::from(v));
    // flag_facing and the points keep one decimal (`1.0,-0.0`, `492.5,576.0`).
    let one = |v: f32| {
        let t = ted(v);
        if t.contains('.') { t } else { format!("{t}.0") }
    };
    let mut list = Node::new("CAPTURE_LOCATION_LIST");
    for p in placed.iter().filter(|p| p.export == "LayerExportType_Normal" && p.node.class == "CaptureLocationInstance") {
        let c = capture_record(p, &by_node, extent_z, buildings);
        let mut n = Node::new("CAPTURE_LOCATION");
        for (k, v) in [
            ("radius", wce_formats::c_num::printf_f_msvc(f64::from(c.radius), 6)),
            ("position_x", wce_formats::c_num::printf_f_msvc(f64::from(c.flag.0), 6)),
            ("position_y", wce_formats::c_num::printf_f_msvc(f64::from(c.flag.1), 6)),
            ("valid_for_min_num_players", c.min_players.to_string()), ("valid_for_max_num_players", c.max_players.to_string()),
            ("capture_point_type_v2", if c.major { "CAPTURE_LOCATION_MAJOR" } else { "CAPTURE_LOCATION_MINOR" }.to_owned()),
            ("capture_point_list", c.points.iter().map(|&(x, z)| format!("{},{}", one(x), one(z))).collect::<Vec<_>>().join(";")),
            ("building_list", c.links.iter().map(ToString::to_string).collect::<Vec<_>>().join(";")), ("database_key", c.key.clone()),
            ("flag_facing", format!("{},{}", one(c.facing.cos()), one(-c.facing.sin()))),
        ] {
            n.set(k, Value::Str(v));
        }

        bmd_xml_layout(&mut n);
        if list.children.is_empty() {
            n.trivia_mut().layout.push((BEFORE_START, b"\r\n\t\r\n".to_vec().into()));
        }

        list.children.push(n);
    }

    if list.children.is_empty() {
        return None;
    }

    list.trivia_mut().layout.push((BEFORE_START, b"\r\n\r\n\t\t".to_vec().into()));
    let mut root = Node::new("CAPTURE_LOCATION_SET");
    root.children.push(list);
    Some((Document { name: "capture_location_list.xml".to_owned(), root }, outline_style()))
}

// ---- the escalation BMDs and the civilian deployment list (rpfm_lib's structs, built through serde_json)

type Json = serde_json::Value;

fn jf(v: f32) -> Json {
    Json::from(f64::from(v))
}

fn json_point(x: f32, y: f32) -> Json {
    serde_json::json!({ "x": jf(x), "y": jf(y) })
}

fn json_points(points: &[(f32, f32)]) -> Json {
    Json::Array(points.iter().map(|&(x, y)| json_point(x, y)).collect())
}

/// A `Transform4x4` from a record's row-major matrix (element (r, c) at r * 4 + c, as `m{r}{c}`).
fn json_matrix(m: &[f64]) -> Json {
    let mut t = serde_json::Map::new();
    for r in 0..4 {
        for c in 0..4 {
            t.insert(format!("m{r}{c}"), jf(m.get(r * 4 + c).copied().unwrap_or(0.0) as f32));
        }
    }

    Json::Object(t)
}

fn identity_json() -> Json {
    json_matrix(&[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0])
}

fn record_f32(r: &Node, k: &str) -> f32 {
    r.get(k).and_then(Value::as_f64).unwrap_or(0.0) as f32
}

fn record_flag(r: &Node, k: &str) -> bool {
    match r.get(k) {
        Some(Value::Bool(b)) => *b,
        Some(Value::Int(i)) => *i != 0,
        _ => false,
    }
}

/// A BMD building (`Building`, version 3) from a building list record.
fn bmd_building(b: &Node) -> Json {
    let p = b.get("position").and_then(Value::as_vec3).unwrap_or_default().map(|v| v as f32);
    let q = b.get("orientation").and_then(Value::as_floats).unwrap_or_default();
    let q = |i: usize| jf(q.get(i).copied().unwrap_or(0.0) as f32);
    let mut t = serde_json::Map::new();
    for r in 0..4 {
        for c in 0..3 {
            t.insert(format!("m{r}{c}"), jf(0.0));
        }
    }

    let flag = |k: &str| record_flag(b, k);
    serde_json::json!({
        "serialise_version": 3, "building_id": "", "parent_id": 0, "building_key": b.get("key").and_then(Value::as_str).unwrap_or_default(),
        "position_type": "BBPT_LF_RELATIVE", "transform": t, "position": { "x": jf(p[0]), "y": jf(p[1]), "z": jf(p[2]) },
        "orientation": { "i": q(0), "j": q(1), "k": q(2), "w": q(3) }, "scale": { "x": jf(1.0), "y": jf(1.0), "z": jf(1.0) },
        "properties": {
            "serialise_version": 0, "building_id": "", "starting_damage_unary": jf(record_f32(b, "starting_damage")), "on_fire": flag("on_fire"),
            "start_disabled": flag("start_disabled"), "weak_point": flag("weak_point"), "ai_breachable": flag("ai_breachable"),
            "indestructible": flag("indestructible"), "dockable": flag("dockable"), "toggleable": flag("toggleable"), "lite": flag("lite"),
            "clamp_to_surface": false, "cast_shadows": false, "dont_merge_building": false, "key_building": false, "key_building_use_fort": false,
            "is_prop_in_outfield": false, "settlement_level_configurable": false, "hide_tooltip": false, "include_in_fog": false,
            "tint_inherit_from_parent": false,
        },
        "height_mode": "", "uid": 0,
    })
}

/// A BMD prop (`PropMarker`, version 1 or 2) from a prop marker record of version 9. TEd's serialiser (`FUN_004d38e0`) writes the flags
/// word's bits (clamp 2, outfield = the water plane 3, fauna 4); `snow` and `destruction` come from locals it never sets, and a model's
/// decal fields from members its export never sets: those bytes are TEd's stack and heap garbage, written here as 0.
fn bmd_prop(key: &str, r: &Node, version: i64) -> Json {
    let decal = record_flag(r, "is_decal");
    let m = r.get("transform").and_then(Value::as_floats).unwrap_or_default();
    let d = |k: &str| jf(if decal { record_f32(r, k) } else { 0.0 });
    serde_json::json!({
        "key": key, "serialise_version": version, "transform": json_matrix(m), "clamp_to_surface": false, "prop_type": u32::from(decal),
        "allow_in_outfield": record_flag(r, "unknown_69"), "decal_parallax": d("parallax_scale"),
        "decal_scale": { "x": d("scale_x"), "y": d("scale_y"), "z": d("scale_z") }, "decal_tiled": u8::from(decal && record_flag(r, "tiled")),
        "is_fauna": 0, "snow": 0, "destruction": 0,
    })
}

/// A BMD particle emitter (`ParticleEmitterMarker`) from a particle emitter marker record of version 9.
fn bmd_emitter(key: &str, r: &Node) -> Json {
    let m = r.get("transform").and_then(Value::as_floats).unwrap_or_default();
    let flag = |k: &str| record_flag(r, k);
    serde_json::json!({
        "key": key, "transform": json_matrix(m), "emission_rate": jf(0.0), "instance_name": "", "autoplay": flag("autoplay"),
        "scale": jf(record_f32(r, "scale")), "use_in_outfield": flag("use_in_outfield"), "snow_inside": flag("visible_inside_snow_zone"),
        "snow_outside": flag("visible_outside_snow_zone"), "destruction_inside": flag("visible_inside_destruction_zone"),
        "destruction_outside": flag("visible_outside_destruction_zone"),
    })
}

/// A BMD capture location (`CaptureLocation`); the links index the BMD's own buildings.
fn bmd_capture(c: &Capture) -> Json {
    let links = c
        .links
        .iter()
        .map(|&i| {
            serde_json::json!({
                "serialise_version": 0, "building_index": i, "prefab_index": 0, "prefab_building_key": "", "uid": 0, "prefab_uid": 0,
                "building_reference": { "serialise_version": 0, "building_index": 0 },
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "id": 0, "location": json_point(c.flag.0, c.flag.1), "radius": jf(c.radius), "valid_for_min_num_players": c.min_players,
        "valid_for_max_num_players": c.max_players,
        "capture_point_type": if c.major { "CAPTURE_LOCATION_MAJOR" } else { "CAPTURE_LOCATION_MINOR" }, "restore_type": "",
        "location_points": json_points(&c.points), "database_key": c.key, "flag_facing": json_point(c.facing.cos(), -c.facing.sin()),
        "destroy_building_on_capture": false, "disable_building_abilities_when_no_original_owner": false, "abilities_affect_globally": false,
        "building_links": links, "toggle_slots_links": [], "ai_hints_links": [], "script_id": "", "is_time_based": false,
    })
}

/// The civilian deployment zones (`CivilianDeploymentZone`s) of the placed nodes as a `CivilianDeploymentList`'s items.
fn civilian_json(placed: &[&Placed], extent_z: f32) -> Vec<Json> {
    placed
        .iter()
        .filter(|p| p.node.class == "CivilianDeploymentZone")
        .map(|p| {
            let pts = polygon_points(p, extent_z).as_chunks::<2>().0.iter().map(|c| (c[0] as f32, c[1] as f32)).collect::<Vec<_>>();
            let attr = |k: &str| p.node.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
            serde_json::json!({ "kind": attr("civilian_type"), "size": attr("civilian_amount"), "area": { "points": json_points(&pts) } })
        })
        .collect()
}

/// `civilian_deployment.bin`: the civilian deployment zones of the normal layers (written when there are some; tiles that ship it
/// empty carry it in the project).
fn civilian_deployment(placed: &[Placed], extent_z: f32) -> Result<Option<Document>, String> {
    let normal = placed.iter().filter(|p| p.export == "LayerExportType_Normal").collect::<Vec<_>>();
    let zones = civilian_json(&normal, extent_z);
    if zones.is_empty() {
        return Ok(None);
    }

    let v = serde_json::json!({ "serialise_version": 1, "civilian_deployment_list": { "civilian_deployment_list": zones } });
    let file: rpfm_lib::files::fastbin_lists::CivilianDeploymentFile = serde_json::from_value(v).map_err(|e| format!("civilian_deployment.bin: {e}"))?;
    Ok(Some(Document { name: "civilian_deployment.bin".to_owned(), root: crate::serde_node::to_node("CIVILIAN_DEPLOYMENT", &file)? }))
}

/// `bmd_data_escalation<n>.bin` (BMD version 6 unless told) and its XML twin: an escalation layer's buildings, props, particle emitters,
/// capture locations (linking into its own buildings), no-go regions, zones and civilian deployment zones. TEd writes a file per
/// escalation layer, empty or not.
fn escalation_bmds(placed: &[Placed], extent_z: f32, ctx: &CompileContext, notes: &mut CompileNotes) -> Result<Vec<(Document, Document)>, String> {
    let by_node = by_node(placed);
    let mut out = vec![];
    for n in 1..=3 {
        let export = format!("LayerExportType_Escalation{n}");
        if !placed.iter().any(|p| p.node.class == "LayerInstance" && p.export == export) {
            continue;
        }

        let name = format!("bmd_data_escalation{n}.bin");
        let version = ctx.marker_versions.and_then(|f| f(&name)).unwrap_or(6);
        let prop_version = ctx.marker_versions.and_then(|f| f(&format!("{name}/props"))).unwrap_or(2);
        let layer = placed.iter().filter(|p| p.export == export).collect::<Vec<_>>();
        let mut unknown_levels = 0;
        let buildings = layer.iter().filter(|p| p.node.class == "BUILDING_INSTANCE").map(|p| building_record(p, extent_z, ctx, false, &mut unknown_levels)).collect::<Vec<_>>();
        if unknown_levels > 0 {
            notes.notes.push(format!("{name}: {unknown_levels} damaged buildings without their destruction levels (starting damage 1)"));
        }

        let links = capture_names(layer.iter().copied());
        // Props and emitters grouped by key in the marker table's order, as the markers files.
        let grouped = |records: Vec<(MarkerKey, Node)>| {
            let mut first: Vec<MarkerKey> = vec![];
            for (k, _) in &records {
                if !first.contains(k) {
                    first.push(k.clone());
                }
            }

            marker_table_order(&first).into_iter().flat_map(|k| records.iter().filter(move |(r, _)| *r == k).cloned().collect::<Vec<_>>()).collect::<Vec<_>>()
        };
        let text = |k: &MarkerKey| match k {
            MarkerKey::Text(s) | MarkerKey::Named(s, _) => s.clone(),
            MarkerKey::Number(v) => v.to_string(),
        };
        let props = grouped(layer.iter().filter_map(|p| prop_record(p, extent_z, 9)).collect());
        let emitters = grouped(layer.iter().filter(|p| p.node.class == "VFXInstance").map(|p| emitter_record(p, extent_z, 9)).collect());
        let captures = layer
            .iter()
            .filter(|p| p.node.class == "CaptureLocationInstance")
            .map(|p| bmd_capture(&capture_record(p, &by_node, extent_z, &links)))
            .collect::<Vec<_>>();
        let outline = |class: &str| {
            layer
                .iter()
                .filter(|p| p.node.class == class)
                .map(|p| polygon_points(p, extent_z).as_chunks::<2>().0.iter().map(|c| (c[0] as f32, c[1] as f32)).collect::<Vec<_>>())
                .collect::<Vec<_>>()
        };
        let nogo = outline("NOGO_REGION_POLYGON_INSTANCE").iter().map(|o| serde_json::json!({ "outline": json_points(o) })).collect::<Vec<_>>();
        let zones = outline("ZONE_POLYGON_INSTANCE")
            .iter()
            .map(|o| {
                serde_json::json!({
                    "battlefield_zone_template": {
                        "zone_name": "ZONE_POLYGON_INSTANCE", "outline": { "outline": json_points(o) },
                        "entity_formation_template": { "name": "", "lines": [] },
                    },
                    "transform": identity_json(),
                })
            })
            .collect::<Vec<_>>();

        let mut v = serde_json::to_value(rpfm_lib::files::bmd::Bmd::default()).map_err(|e| e.to_string())?;
        v["serialise_version"] = Json::from(version);
        for section in ["battlefield_building_list", "battlefield_building_list_far", "bmd_outline_list", "capture_location_set", "deployment_list", "prefab_instance_list", "zones_template_list"] {
            v[section]["serialise_version"] = Json::from(1);
        }

        v["battlefield_building_list"]["buildings"] = Json::Array(buildings.iter().map(bmd_building).collect());
        v["prop_list"]["markers"] = Json::Array(props.iter().map(|(k, r)| bmd_prop(&text(k), r, prop_version)).collect());
        v["particle_emitter_list"]["markers"] = Json::Array(emitters.iter().map(|(k, r)| bmd_emitter(&text(k), r)).collect());
        if !captures.is_empty() {
            v["capture_location_set"]["capture_location_sets"] = serde_json::json!([{ "capture_locations": captures }]);
        }

        v["non_terrain_outlines"]["empire_outline"] = Json::Array(nogo);
        v["zones_template_list"]["zones"] = Json::Array(zones);
        v["civilian_deployment_list"]["civilian_deployment_list"] = Json::Array(civilian_json(&layer, extent_z));
        let twin = bmd_twin(&v, &format!("bmd_data_escalation{n}.xml"));
        let bmd: rpfm_lib::files::bmd::Bmd = serde_json::from_value(v).map_err(|e| format!("{name}: {e}"))?;
        out.push((Document { name, root: crate::serde_node::to_node("BMD", &bmd)? }, twin));
    }

    Ok(out)
}

/// An element with its attributes in order (the BMD XML twin's).
fn el(class: &str, attrs: Vec<(&str, String)>) -> Node {
    let mut n = Node::new(class);
    for (k, v) in attrs {
        n.push_read(Sym::new(k), Value::Str(v), None);
    }

    n
}

fn el_with(class: &str, attrs: Vec<(&str, String)>, children: Vec<Node>) -> Node {
    let mut n = el(class, attrs);
    n.children = children;
    n
}

/// The BMD XML twin's texts: floats `%f` of the f32, booleans `true` / `false`.
fn tf(v: &Json) -> String {
    wce_formats::c_num::printf_f_msvc(f64::from(v.as_f64().unwrap_or(0.0) as f32), 6)
}

fn tb(v: &Json) -> String {
    match v {
        Json::Bool(b) => b.to_string(),
        other => (other.as_i64().unwrap_or(0) != 0).to_string(),
    }
}

fn ti(v: &Json) -> String {
    v.as_i64().map_or_else(|| v.to_string(), |i| i.to_string())
}

fn ts(v: &Json) -> String {
    v.as_str().unwrap_or_default().to_owned()
}

/// The `x`, `y` (and `z`) of a point as attributes.
fn xy(v: &Json, class: &str) -> Node {
    el(class, vec![("x", tf(&v["x"])), ("y", tf(&v["y"]))])
}

fn matrix_el(m: &Json) -> Node {
    let mut attrs = vec![];
    for r in 0..4 {
        for c in 0..4 {
            let k = format!("m{r}{c}");
            attrs.push((k.clone(), tf(&m[k.as_str()])));
        }
    }

    let mut n = Node::new("transform");
    for (k, v) in attrs {
        n.push_read(Sym::new(&k), Value::Str(v), None);
    }

    n
}

fn array(v: &Json) -> &[Json] {
    v.as_array().map_or(&[][..], Vec::as_slice)
}

/// The XML twin TEd writes next to a BMD (`BATTLE_MAP_DEFINITION_DATA`): the same data in its XML serialiser's names, in the binary's
/// section order (version 5 adds the civilian deployment list, 6 the shelter list).
fn bmd_twin(v: &Json, name: &str) -> Document {
    let version = v["serialise_version"].as_i64().unwrap_or(6);
    let sv = |n: i64| ("serialise_version", n.to_string());
    let props = array(&v["prop_list"]["markers"])
        .iter()
        .map(|p| {
            let mut attrs = vec![
                ("serialise_version", ti(&p["serialise_version"])), ("clamp_to_surface", tb(&p["clamp_to_surface"])), ("type", ti(&p["prop_type"])),
                ("allow_in_outfield", tb(&p["allow_in_outfield"])), ("decal_params_parallax", tf(&p["decal_parallax"])),
                ("decal_params_scalex", tf(&p["decal_scale"]["x"])), ("decal_params_scaley", tf(&p["decal_scale"]["y"])),
                ("decal_params_scalez", tf(&p["decal_scale"]["z"])), ("decal_params_tiled", tb(&p["decal_tiled"])),
            ];
            if p["serialise_version"].as_i64() == Some(2) {
                attrs.extend([("is_fauna", tb(&p["is_fauna"])), ("snow", tb(&p["snow"])), ("destruction", tb(&p["destruction"]))]);
            }

            el_with("PROP", vec![("key", ts(&p["key"]))], vec![el_with("marker", attrs, vec![matrix_el(&p["transform"])])])
        })
        .collect();
    let emitters = array(&v["particle_emitter_list"]["markers"])
        .iter()
        .map(|e| {
            let attrs = vec![
                ("serialise_version", "4".to_owned()), ("emission_rate", tf(&e["emission_rate"])), ("instance_name", ts(&e["instance_name"])),
                ("autoplay", tb(&e["autoplay"])), ("scale", tf(&e["scale"])), ("use_in_outfield", tb(&e["use_in_outfield"])),
                ("snow_inside", tb(&e["snow_inside"])), ("snow_outside", tb(&e["snow_outside"])), ("destruction_inside", tb(&e["destruction_inside"])),
                ("destruction_outside", tb(&e["destruction_outside"])),
            ];
            el_with("PARTICLE_EMITTER", vec![("key", ts(&e["key"]))], vec![el_with("marker", attrs, vec![matrix_el(&e["transform"])])])
        })
        .collect();
    let buildings = array(&v["battlefield_building_list"]["buildings"])
        .iter()
        .map(|b| {
            let pr = &b["properties"];
            let attrs = vec![
                ("serialise_version", ti(&b["serialise_version"])), ("building_key", ts(&b["building_key"])), ("position_type", ts(&b["position_type"])),
                ("starting_damage_unary", tf(&pr["starting_damage_unary"])), ("on_fire", tb(&pr["on_fire"])), ("start_disabled", tb(&pr["start_disabled"])),
                ("weak_point", tb(&pr["weak_point"])), ("ai_breachable", tb(&pr["ai_breachable"])), ("indestructible", tb(&pr["indestructible"])),
                ("dockable", tb(&pr["dockable"])), ("toggleable", tb(&pr["toggleable"])), ("lite", tb(&pr["lite"])),
            ];
            let (p, q, s) = (&b["position"], &b["orientation"], &b["scale"]);
            el_with("BUILDING", attrs, vec![
                el("position", vec![("x", tf(&p["x"])), ("y", tf(&p["y"])), ("z", tf(&p["z"]))]),
                el("orientation", vec![("i", tf(&q["i"])), ("j", tf(&q["j"])), ("k", tf(&q["k"])), ("w", tf(&q["w"]))]),
                el("scale", vec![("x", tf(&s["x"])), ("y", tf(&s["y"])), ("z", tf(&s["z"]))]),
            ])
        })
        .collect();
    let sets = array(&v["capture_location_set"]["capture_location_sets"])
        .iter()
        .map(|set| {
            let locs = array(&set["capture_locations"])
                .iter()
                .map(|c| {
                    let attrs = vec![
                        ("radius", tf(&c["radius"])), ("valid_for_min_num_players", ti(&c["valid_for_min_num_players"])),
                        ("valid_for_max_num_players", ti(&c["valid_for_max_num_players"])), ("capture_point_type", ts(&c["capture_point_type"])),
                        ("database_key", ts(&c["database_key"])),
                    ];
                    let indices = el("building_indices", array(&c["building_links"]).iter().map(|l| ("index", ti(&l["building_index"]))).collect());
                    el_with("CAPTURE_LOCATION", attrs, vec![
                        xy(&c["location"], "location"),
                        el_with("location_points", vec![], array(&c["location_points"]).iter().map(|p| xy(p, "point")).collect()),
                        indices,
                        xy(&c["flag_facing"], "flag_facing"),
                    ])
                })
                .collect();
            el_with("CAPTURE_LOCATION_LIST", vec![], vec![el_with("CAPTURE_LOCATION_LIST", vec![], locs)])
        })
        .collect();
    let outline = |o: &Json, class: &str| el_with(class, vec![], array(o).iter().map(|p| xy(p, "position")).collect());
    let nogo = array(&v["non_terrain_outlines"]["empire_outline"]).iter().map(|o| el_with("EMPIRE_OUTLINE", vec![], vec![outline(&o["outline"], "OUTLINE")])).collect();
    let zones = array(&v["zones_template_list"]["zones"])
        .iter()
        .map(|z| {
            let t = &z["battlefield_zone_template"];
            el_with("ZONE", vec![], vec![
                el_with("BATTLEFIELD_ZONE_TEMPLATE", vec![("zone_name", ts(&t["zone_name"]))], vec![
                    outline(&t["outline"]["outline"], "OUTLINE"),
                    el_with("entity_formation_template", vec![("name", String::new())], vec![Node::new("lines")]),
                ]),
                matrix_el(&z["transform"]),
            ])
        })
        .collect();
    let civilians = array(&v["civilian_deployment_list"]["civilian_deployment_list"])
        .iter()
        .map(|c| {
            el_with("CIVILIAN_DEPLOYMENT", vec![sv(1), ("civilian_type", ts(&c["kind"])), ("civilian_amount", ts(&c["size"]))], vec![el_with(
                "outline",
                vec![],
                array(&c["area"]["points"]).iter().map(|p| xy(p, "point")).collect(),
            )])
        })
        .collect();

    let mut root = el("BATTLE_MAP_DEFINITION_DATA", vec![sv(version)]);
    root.children = vec![
        el_with("PROPS", vec![], props),
        el_with("PARTICLE_EMITTERS", vec![], emitters),
        Node::new("AI_HINT_POLYLINES"),
        Node::new("AI_HINT_POLYLINE_LISTS"),
        Node::new("AI_SEPARATORS"),
        el_with("BATTLE_DEPLOYMENT_AREA_MANAGER", vec![sv(1)], vec![Node::new("BATTLE_DEPLOYMENT_AREA_MANAGER")]),
        el_with("BATTLEFIELD_BUILDING_LIST", vec![sv(1)], vec![el_with("BUILDINGS", vec![], buildings)]),
        el_with("BATTLEFIELD_BUILDING_LIST_FAR", vec![sv(1)], vec![Node::new("BUILDINGS")]),
        el_with("CAPTURE_LOCATION_SET", vec![sv(1)], vec![el_with("CAPTURE_LOCATION_SET", vec![], sets)]),
        el_with("EF_LINE_LIST", vec![], vec![Node::new("LINES")]),
        Node::new("GO_OUTLINES"),
        el_with("NON_TERRAIN_OUTLINES", vec![], nogo),
        el_with("ZONES_TEMPLATE_LIST", vec![sv(1)], vec![el_with("ZONES", vec![], zones)]),
        el_with("PREFAB_INSTANCE_LIST", vec![sv(1)], vec![Node::new("PREFAB_INSTANCE")]),
        el_with("BMD_OUTLINE_LIST", vec![sv(1)], vec![Node::new("BMD_OUTLINES")]),
        Node::new("TERRAIN_OUTLINES"),
        Node::new("LITE_BUILDING_OUTLINES"),
    ];
    if version >= 5 {
        root.children.push(el_with("CIVILIAN_DEPLOYMENT_LIST", vec![], civilians));
    }

    if version >= 6 {
        root.children.push(Node::new("CIVILIAN_SHELTER_LIST"));
    }

    Document { name: name.to_owned(), root }
}

/// The outline files' style: no indentation, single quotes, CRLF.
fn outline_style() -> crate::xml::DocStyle {
    crate::xml::DocStyle { indent: vec![], ..bmd_xml_style() }
}

/// Every BUILDING_INSTANCE of the project (in document order, whatever its layer) with its key and TEd's world matrix (row-major, the
/// translation in the last column; x and z in TEd's half metres), as BOB_Tile's `QTU::TedScene::elements("BUILDING_INSTANCE")` and
/// `QTU::TedEntity::world_transform` give them.
pub fn buildings(raw: &Tile) -> Vec<(String, [f32; 16])> {
    instances(raw, "BUILDING_INSTANCE")
}

/// [`buildings`] for any class of entity (`SIMPLE_MESH_INSTANCE`: BOB's water planes), an instance's own `scale` (a vector) applied
/// to its matrix's columns (rotation x scale; the order is an assumption: the vanilla water planes are not rotated).
pub fn instances(raw: &Tile, class: &str) -> Vec<(String, [f32; 16])> {
    let Some(scene) = raw.scene.as_ref() else { return vec![] };
    let mut placed = vec![];
    walk(scene, None, "LayerExportType_Normal", "", &mut placed);
    placed
        .iter()
        .filter(|p| p.node.class == class)
        .map(|p| {
            let mut m = p.world;
            if let Some(s) = vec3_of(p.node, "scale") {
                for r in 0..3 {
                    for c in 0..3 {
                        m[r * 4 + c] *= s[c];
                    }
                }
            }

            (p.node.get("key").and_then(Value::as_str).unwrap_or_default().to_owned(), m)
        })
        .collect()
}

/// A project's exported tile, as the model `attila.tile_binary` writes. Stage A (TEd's export): the building lists; the prop, particle
/// emitter, point light, AI hint, separator and siege graph markers; definition.xml, the outline files, zones, deployment areas and
/// capture locations (with their document styles); civilian_deployment.bin; the escalation BMDs and their XML twins. With
/// `ctx.stage_b`, stage B too ([`super::stage_b::stage_b_files`]).
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
    xml.extend(deployment(&placed, (width as f32 * 128.0, extent_z)));

    // The main list's buildings' capture locations, for the links.
    let main = capture_names(placed.iter().filter(|p| p.export == "LayerExportType_Normal"));
    xml.extend(capture_locations(&placed, extent_z, &main));
    docs.extend(civilian_deployment(&placed, extent_z)?);
    for (bin, twin) in escalation_bmds(&placed, extent_z, ctx, &mut notes)? {
        docs.push(bin);
        xml.push((twin, bmd_xml_style()));
    }

    let mut out = Tile { source_format: Some(super::tile_binary::ID), variation: raw.variation.clone(), ..Default::default() };
    for (d, style) in xml {
        out.source.insert(d.name.clone(), std::sync::Arc::new(style));
        docs.push(d);
    }

    out.documents = docs;
    if let Some(b) = &ctx.stage_b {
        // Stage B's files read back as the exported tile's parts (maps, documents, files), then merged in.
        let (files, more) = super::stage_b::stage_b_files(raw, b)?;
        notes.notes.extend(more.notes);
        let folder = files.into_iter().map(|(name, bytes)| (name, wce_tile_model::Blob::Bytes(bytes.into()))).collect();
        let tf = crate::TileFiles { folder, entry: None, variation: raw.variation.clone() };
        let (read, _) = crate::TileFormat::read(&super::tile_binary::TileBinary, &tf).map_err(|e| e.to_string())?;
        out.maps.extend(read.maps);
        out.documents.extend(read.documents);
        out.files.extend(read.files);
        for (name, style) in read.source {
            out.source.insert(name, style);
        }
    }

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
    fn insertion_order_gives_a_bmd_group_order_back() {
        // tiles4.pack's western_city_b/small, bmd_data_escalation2.bin: the props' groups (24 keys: the table grows from 13 to 27).
        let order = [
            "rigidmodels\\decals\\battle\\global_terrain_path_04.rigid_model_v2",
            "rigidmodels\\decals\\battle\\border_1.rigid_model_v2",
            "rigidmodels\\decals\\battle\\rubble_3.rigid_model_v2",
            "rigidmodels\\decals\\battle\\global_terrain_mud_02.rigid_model_v2",
            "rigidmodels\\decals\\battle\\rubble_4.rigid_model_v2",
            "rigidmodels\\decals\\battle\\global_terrain_mud_01.rigid_model_v2",
            "rigidmodels\\decals\\battle\\global_terrain_puddle_01.rigid_model_v2",
            "rigidmodels\\decals\\battle\\rubble_1.rigid_model_v2",
            "rigidmodels\\decals\\battle\\dirt_1.rigid_model_v2",
            "rigidmodels\\decals\\battle\\border_2.rigid_model_v2",
            "rigidmodels\\decals\\battle\\rubble_2.rigid_model_v2",
            "rigidmodels\\decals\\battle\\dirt_2.rigid_model_v2",
            "rigidmodels\\decals\\battle\\global_terrain_cracked_01.rigid_model_v2",
            "rigidmodels\\decals\\battle\\global_terrain_erossion_03.rigid_model_v2",
            "rigidmodels\\decals\\battle\\global_terrain_rocks_01.rigid_model_v2",
            "rigidmodels\\decals\\battle\\global_terrain_erossion_10.rigid_model_v2",
            "rigidmodels\\decals\\battle\\global_terrain_puddle_02.rigid_model_v2",
            "rigidmodels\\decals\\battle\\global_terrain_rocks_04.rigid_model_v2",
            "rigidmodels\\decals\\battle\\path_plans.rigid_model_v2",
            "rigidmodels\\decals\\battle\\global_terrain_path_03.rigid_model_v2",
            "rigidmodels\\decals\\battle\\global_terrain_erossion_08.rigid_model_v2",
            "rigidmodels\\decals\\battle\\global_terrain_erossion_09.rigid_model_v2",
            "rigidmodels\\decals\\battle\\global_terrain_erossion_12.rigid_model_v2",
            "rigidmodels\\decals\\battle\\global_terrain_erossion_06.rigid_model_v2",
        ]
        .map(|k| MarkerKey::Text(k.to_owned()));
        let insertion = marker_insertion_order(&order).expect("an insertion order");
        assert_eq!(marker_table_order(&insertion), order);
        assert_ne!(insertion, order);
    }

    #[test]
    fn normal_sub_layers_export_with_their_escalation_layer() {
        // The kit's layout: an escalation layer's buildings in a normal sub-layer.
        let tile = project(vec![
            layer("Buildings", "LayerExportType_Normal", vec![building("house", [10.0, 0.0, 20.0], 0.0, 0)]),
            layer("Escalation 1", "LayerExportType_Escalation1", vec![layer("Gates and Towers", "LayerExportType_Normal", vec![building("gate", [0.0; 3], 0.0, 0)])]),
        ]);
        let (out, _) = compile(&tile, &CompileContext::default()).expect("compile");
        let main = out.documents.iter().find(|d| d.name == "building_list").expect("the main list");
        assert_eq!(main.root.children.len(), 1);
        let bmd = out.documents.iter().find(|d| d.name == "bmd_data_escalation1.bin").expect("the escalation BMD");
        let buildings = bmd.root.child("battlefield_building_list").and_then(|l| l.child("buildings")).expect("buildings");
        assert_eq!(buildings.children.len(), 1);
        assert_eq!(buildings.children[0].get("building_key").and_then(Value::as_str), Some("gate"));
        assert!(out.documents.iter().any(|d| d.name == "bmd_data_escalation1.xml"));
    }

    #[test]
    fn deployment_clipping_starts_where_ted_does() {
        // western_city_a/medium's guerrilla exclusion square, past the tile on every side: TEd's output starts at the corner its last
        // side's clipping leaves first.
        let square = [(1055.43, 1068.96), (-47.08, 1075.1), (-52.32, -51.19), (1059.66, -49.73)];
        assert_eq!(clip_to_playable(&square, (1024.0, 1024.0)), [(32.5, 32.5), (991.5, 32.5), (991.5, 991.5), (32.5, 991.5)]);

        // An edge crossing x = 32.5 is cut there; points inside stay.
        let cut = clip_to_playable(&[(19.93, 429.89), (42.94, 430.56), (42.94, 500.0)], (1024.0, 1024.0));
        assert_eq!(cut.len(), 4);
        assert!(cut.contains(&(42.94, 430.56)) && cut.iter().all(|p| p.0 >= 32.5));
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
