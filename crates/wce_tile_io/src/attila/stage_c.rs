//! Stage C of the compile: what BOB's Vegetation processor makes of a project (`wce_tile::vegetation`, docs/re/vegetation.md), as the
//! exported tile's documents: per generation category `<category>.tree_list.bin` (generated and static trees) and
//! `<category>_procedural_bmd_data.bin` with its XML twin (generated props, decals and VFX), and `default.grass_list.bin`.
//! docs/formats/tile_compile.md, stage C.

use std::path::Path;

use rayon::prelude::*;

use wce_formats::tile_entry::TileEntry;
use wce_tile::ted_scene::TedScene;
use wce_tile::vegetation::{self, GenerationParams, TileInputs, grass::GrassParams};
use wce_tile_model::{Document, MapKind, Node, Samples, Tile, Value};

use super::compile::{CompileNotes, Json, bmd_twin, jf, json_matrix};
use super::schema::{EntrySchema, TedSchema};

/// What stage C needs besides the project: the kit's vegetation parameters (`working_data/BattleTerrain/vegetation`), its grass
/// parameters (`max_grass.xml`), the tile database's `_settings.xml` (the links that blend) and whether a model exists (BOB's VFS: the
/// game's packs; a tree whose model doesn't exist is left out of the lists).
#[derive(Clone, Copy)]
pub struct StageC<'a> {
    pub params: &'a GenerationParams,
    pub grass: Option<&'a GrassParams>,
    pub settings: Option<&'a Path>,
    pub model_exists: &'a (dyn Fn(&str) -> bool + Sync),
}

/// The documents stage C writes, and the styles of the XML ones.
pub struct StageCOutput {
    pub documents: Vec<Document>,
    pub xml: Vec<String>,
}

/// The kit's vegetation parameters of a `raw_data` folder: `working_data/BattleTerrain/vegetation` and
/// `working_data/terrain/vegetation/battle/grass/max_grass.xml` next to it (the grass ones may be missing).
///
/// # Errors
///
/// The tree parameters don't load.
pub fn kit_params(raw_data: &Path) -> Result<(GenerationParams, Option<GrassParams>), String> {
    let working = raw_data.parent().ok_or("raw_data has no parent")?.join("working_data");
    let params = GenerationParams::load(&working.join("BattleTerrain/vegetation"))?;
    let grass = ["working_data", "raw_data"]
        .iter()
        .map(|d| working.parent().unwrap_or(&working).join(d).join("terrain/vegetation/battle/grass/max_grass.xml"))
        .find(|p| p.is_file())
        .and_then(|p| GrassParams::load(&p).ok());
    Ok((params, grass))
}

/// The project's scene as TEd's tile.ted and its entry as the database's XML, read back by `wce_tile`'s readers (the generator's
/// arithmetic is theirs: world transforms from the text, as BOB reads tile.ted).
fn ted_parts(raw: &Tile) -> Result<(TedScene, TileEntry, usize), String> {
    let scene = raw.scene.as_ref().ok_or("the project has no tile.ted")?;
    let entry = raw.entry.as_ref().ok_or("the project has no entry")?;
    let scene = TedScene::parse(crate::xml::write(scene, &crate::xml::DocStyle::ted(), &TedSchema))?;
    let entry = TileEntry::parse(crate::xml::write(&entry.root, &super::ted_project::entry_style(), &EntrySchema)).map_err(|e| e.to_string())?;
    let variation = match &raw.variation {
        Some(name) => entry.variations().iter().position(|v| v.location == *name).ok_or_else(|| format!("the entry has no variation {name}"))?,
        None => 0,
    };
    Ok((scene, entry, variation))
}

/// The generator's inputs from the project's model.
///
/// # Errors
///
/// The project lacks its scene, entry, height or blend map, or their sizes disagree.
pub fn inputs(raw: &Tile, settings: Option<&Path>) -> Result<(TileInputs, TedScene), String> {
    let (scene, entry, variation) = ted_parts(raw)?;
    let height = raw.map(MapKind::Height, 0).ok_or("the project has no height map")?;
    let Samples::U16(h) = &height.raster.samples else { return Err("the height map is not 16-bit".to_owned()) };
    let blend = raw.map(MapKind::Blend, 0).ok_or("the project has no blend map")?;
    let Samples::U16(b) = &blend.raster.samples else { return Err("the blend map is not 16-bit".to_owned()) };
    if blend.raster.channels != 16 {
        return Err(format!("the blend map has {} channels, not 16", blend.raster.channels));
    }

    if (blend.raster.width, blend.raster.height) != (height.raster.width, height.raster.height) {
        return Err(format!("the blend map is {}x{}, the height map {}x{}", blend.raster.width, blend.raster.height, height.raster.width, height.raster.height));
    }

    let blend = b.iter().map(|&v| f32::from(v) * (1.0 / 65535.0)).collect();
    let size = (height.raster.width as usize, height.raster.height as usize);
    let inputs = vegetation::tile_inputs_from(&entry, variation, h, size, blend, vegetation::exclusions(&scene), settings)?;
    Ok((inputs, scene))
}

/// A value as BOB's f16 conversion (`FUN_10057610`) stores it: to nearest, ties toward zero; the result converts to f16 exactly.
fn f16_bob(v: f32) -> f32 {
    if v == 0.0 || !v.is_finite() {
        return v;
    }

    let ulp = 2f64.powf(f64::from(v.abs()).log2().floor().max(-14.0) - 10.0);
    let r = f64::from(v.abs()) / ulp;
    let n = if r.fract() == 0.5 { r.floor() } else { r.round() };
    (n * ulp).copysign(f64::from(v)) as f32
}

/// BOB's hash of a `CA::String` key in its chained hash maps (FUN_10044a10): from the last byte back, each (signed) byte shifted 0, 8,
/// 16, 24, 0 ... bits and xored into 0xf73a51c9.
fn ca_string_hash(s: &str) -> u32 {
    let (mut h, mut shift) = (0xf73a_51c9u32, 0u32);
    for &b in s.as_bytes().iter().rev() {
        h ^= ((b as i8 as i32) << shift) as u32;
        shift = (shift + 8) & 31;
    }

    h
}

/// The order BOB's tree map (path -> trees; FUN_10044a10, rehash FUN_10044660) gives its keys, inserted in this order: 4 bucket lists,
/// a key appended to its bucket's; when the count would pass the bucket count (load factor 1) the table first grows to 2 n + 1, moving
/// the entries bucket by bucket in order. Written bucket by bucket. 101 of 101 of BOB's lists on the oracle tiles.
fn ca_map_order<'a>(keys: &[&'a str]) -> Vec<&'a str> {
    let mut buckets: Vec<Vec<&str>> = vec![vec![]; 4];
    for (count, k) in keys.iter().enumerate() {
        if count + 1 > buckets.len() {
            let n = buckets.len() * 2 + 1;
            let mut grown: Vec<Vec<&str>> = vec![vec![]; n];
            for x in buckets.into_iter().flatten() {
                grown[ca_string_hash(x) as usize % n].push(x);
            }

            buckets = grown;
        }

        let n = buckets.len();
        buckets[ca_string_hash(k) as usize % n].push(k);
    }

    buckets.into_iter().flatten().collect()
}

/// A tree list in the document form `attila.tile_binary` writes (version 3, BOB's): one `type` per model, the models in BOB's map's
/// order of the path-sorted list ([`ca_map_order`]), each model's trees in the list's order.
fn tree_list_doc(name: &str, trees: &[vegetation::ListTree]) -> Document {
    let mut root = Node::new("TREE_LIST_BIN");
    root.set("version", Value::Int(3));
    let mut groups: Vec<(&str, Vec<&vegetation::ListTree>)> = vec![];
    for t in trees {
        match groups.iter_mut().find(|(p, _)| *p == t.path) {
            Some((_, v)) => v.push(t),
            None => groups.push((&t.path, vec![t])),
        }
    }

    let order = ca_map_order(&groups.iter().map(|(p, _)| *p).collect::<Vec<_>>());
    groups.sort_by_key(|(p, _)| order.iter().position(|o| o == p));
    for (path, v) in groups {
        let mut n = Node::new("type");
        n.set("key", Value::Str(path.to_owned()));
        n.set("position", Value::Floats(v.iter().flat_map(|t| [t.x, t.y, t.z]).map(f64::from).collect()));
        n.set("rotation", Value::Ints(vec![0; v.len()]));
        n.set("scale", Value::Floats(v.iter().map(|t| f64::from(f32::from(t.scale_byte) / 127.0)).collect()));
        n.set("flags", Value::Ints(v.iter().map(|t| i64::from(t.freeform)).collect()));
        n.set("is_burnable", Value::Ints(vec![0; v.len()]));
        root.children.push(n);
    }

    Document { name: name.to_owned(), root }
}

/// The grass list in the document form `attila.tile_binary` writes (version 1): one `type` per key in BOB's map's order, positions as
/// BOB's f16s.
fn grass_list_doc(items: &[vegetation::grass::GrassItem]) -> Document {
    let mut root = Node::new("GRASS_LIST_BIN");
    root.set("version", Value::Int(1));
    let mut groups: Vec<(&str, Vec<f64>)> = vec![];
    for g in items {
        let p = [g.x, g.y, g.z].map(|v| f64::from(f16_bob(v)));
        match groups.iter_mut().find(|(k, _)| *k == g.key) {
            Some((_, v)) => v.extend(p),
            None => groups.push((&g.key, p.to_vec())),
        }
    }

    // The keys in BOB's map's order, as the tree lists' (13 of 13 oracle lists).
    let order = ca_map_order(&groups.iter().map(|(k, _)| *k).collect::<Vec<_>>());
    groups.sort_by_key(|(k, _)| order.iter().position(|o| o == k));

    for (key, pos) in groups {
        let mut n = Node::new("type");
        n.set("key", Value::Str(key.to_owned()));
        n.set("position", Value::Floats(pos));
        root.children.push(n);
    }

    Document { name: "default.grass_list.bin".to_owned(), root }
}

/// A category's `_procedural_bmd_data` (`BATTLE_MAP_DEFINITION_DATA` v6, everything but the props and particle emitters empty) and its
/// XML twin.
fn procedural_bmd(category: &str, props: &[vegetation::generate::ListProp], vfx: &[vegetation::generate::ListVfx]) -> Result<(Document, Document), String> {
    let props = marker_order(props, |p| &p.key);
    let vfx = marker_order(vfx, |v| &v.key);
    let mut v = serde_json::to_value(rpfm_lib::files::bmd::Bmd::default()).map_err(|e| e.to_string())?;
    v["serialise_version"] = Json::from(6);
    for section in ["battlefield_building_list", "battlefield_building_list_far", "bmd_outline_list", "capture_location_set", "deployment_list", "prefab_instance_list", "zones_template_list"] {
        v[section]["serialise_version"] = Json::from(1);
    }

    // One capture location list, empty.
    v["capture_location_set"]["capture_location_sets"] = serde_json::json!([{ "capture_locations": [] }]);
    v["prop_list"]["markers"] = Json::Array(
        props
            .iter()
            .map(|p| {
                let m = p.transform.map(f64::from);
                serde_json::json!({
                    "key": p.key, "serialise_version": 2, "transform": json_matrix(&m), "clamp_to_surface": false, "prop_type": p.prop_type,
                    "allow_in_outfield": p.allow_in_outfield, "decal_parallax": jf(p.decal_parallax),
                    "decal_scale": { "x": jf(p.decal_scale[0]), "y": jf(p.decal_scale[1]), "z": jf(p.decal_scale[2]) },
                    "decal_tiled": u8::from(p.decal_tiled), "is_fauna": 0, "snow": 1, "destruction": 1,
                })
            })
            .collect(),
    );
    v["particle_emitter_list"]["markers"] = Json::Array(
        vfx.iter()
            .map(|e| {
                let m = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, f64::from(e.x), f64::from(e.y), f64::from(e.z), 1.0];
                serde_json::json!({
                    "key": e.key, "transform": json_matrix(&m), "emission_rate": jf(1.0), "instance_name": e.instance_name, "autoplay": true,
                    "scale": jf(e.scale), "use_in_outfield": e.use_in_outfield, "snow_inside": true, "snow_outside": true,
                    "destruction_inside": true, "destruction_outside": true,
                })
            })
            .collect(),
    );

    let twin = bmd_twin(&v, &format!("{category}_procedural_bmd_data.xml"));

    // The binary's flag bytes are not the twin's booleans: BOB writes `decal_tiled` 64 for a prop (its twin: true) and 0 for a decal,
    // `is_fauna` and `snow` 0 and `destruction` 8 (the twin: false, true, true), on every prop of the oracle tiles.
    if let Some(markers) = v["prop_list"]["markers"].as_array_mut() {
        for m in markers {
            let tiled = if m["prop_type"].as_u64() == Some(0) { 64 } else { 0 };
            m["decal_tiled"] = Json::from(tiled);
            m["is_fauna"] = Json::from(0);
            m["snow"] = Json::from(0);
            m["destruction"] = Json::from(8);
        }
    }

    let name = format!("{category}_procedural_bmd_data.bin");
    let bmd: rpfm_lib::files::bmd::Bmd = serde_json::from_value(v).map_err(|e| format!("{name}: {e}"))?;
    Ok((Document { name, root: crate::serde_node::to_node("BMD", &bmd)? }, twin))
}

/// Entries grouped by key (each key's in their order), the keys in the order BOB's `MARKER_TABLE` gives them (the table TEd's markers
/// files go through: `compile::marker_table_order`, keys inserted in the order they first appear).
fn marker_order<T>(items: &[T], key: impl Fn(&T) -> &str) -> Vec<&T> {
    use super::compile::{MarkerKey, marker_table_order};
    let mut first: Vec<MarkerKey> = vec![];
    for i in items {
        let k = MarkerKey::Text(key(i).to_owned());
        if !first.contains(&k) {
            first.push(k);
        }
    }

    let mut out = Vec::with_capacity(items.len());
    for k in marker_table_order(&first) {
        let MarkerKey::Text(k) = k else { continue };
        out.extend(items.iter().filter(|i| key(i) == k));
    }

    out
}

/// Which outputs stage C writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outputs {
    pub trees: bool,
    pub procedural: bool,
    pub grass: bool,
}

impl Outputs {
    pub const ALL: Self = Self { trees: true, procedural: true, grass: true };
}

/// Stage C's documents of a battle project: per category (in the parameters' order, named as they name it) its tree list (none when
/// it is empty) and procedural BMD with the twin, then the grass list; `which` picks the outputs.
///
/// # Errors
///
/// The project's inputs don't read ([`inputs`]), or a BMD doesn't encode.
pub fn stage_c_documents(raw: &Tile, ctx: &StageC, which: Outputs) -> Result<(StageCOutput, CompileNotes), String> {
    let mut notes = CompileNotes::default();
    let (inputs, scene) = {
        wce_foundation::wce_prof!("export.compile.stage_c.inputs");
        let _a = wce_foundation::activity::start("Stage C: the vegetation's inputs");
        inputs(raw, ctx.settings)?
    };
    let seeds = vegetation::seeds(&scene).unwrap_or_default();
    let statics = if which.trees { vegetation::static_trees(&scene, &inputs) } else { vec![] };
    let mut out = StageCOutput { documents: vec![], xml: vec![] };

    // Each category's trees and procedural BMD, and the grass, on the thread pool (each is generated from its own seeds, so in any order);
    // the documents are taken in category order, as BOB writes them.
    let category = |(category, name): (usize, &String)| -> Result<(Option<Document>, Option<(Document, Document)>), String> {
        let trees = if which.trees {
            wce_foundation::wce_prof!("export.compile.stage_c.trees");
            let _a = wce_foundation::activity::start(format!("Stage C: trees of {name}"));
            // An empty category's list is not written (FUN_10033450; BOB's rule `DeleteEmptyClimateTreeLists` then removes a stale one).
            let trees = vegetation::tree_list(&inputs, ctx.params, category, seeds, &statics, ctx.model_exists);
            (!trees.is_empty()).then(|| tree_list_doc(&format!("{name}.tree_list.bin"), &trees))
        } else {
            None
        };

        let procedural = if which.procedural {
            wce_foundation::wce_prof!("export.compile.stage_c.procedural");
            let _a = wce_foundation::activity::start(format!("Stage C: props, decals and VFX of {name}"));
            let props = vegetation::generated_props(&inputs, ctx.params, category, seeds);
            let vfx = vegetation::generated_vfx(&inputs, ctx.params, category, seeds);
            Some(procedural_bmd(name, &props, &vfx)?)
        } else {
            None
        };

        Ok((trees, procedural))
    };

    let grass = || {
        let params = ctx.grass.filter(|_| which.grass)?;
        wce_foundation::wce_prof!("export.compile.stage_c.grass");
        let _a = wce_foundation::activity::start("Stage C: grass");
        let seed = vegetation::grass_seed(&scene).unwrap_or(0);
        Some(grass_list_doc(&vegetation::grass::generate_grass(&inputs, params, seed)))
    };

    let (categories, grass) = rayon::join(|| ctx.params.categories.par_iter().enumerate().map(category).collect::<Vec<_>>(), grass);
    for c in categories {
        let (trees, procedural) = c?;
        out.documents.extend(trees);
        if let Some((bin, twin)) = procedural {
            out.documents.push(bin);
            out.xml.push(twin.name.clone());
            out.documents.push(twin);
        }
    }

    if which.grass {
        match grass {
            Some(doc) => out.documents.push(doc),
            None => notes.notes.push("stage C: no max_grass.xml in the kit, no grass list".to_owned()),
        }
    }

    Ok((out, notes))
}


// ---- a decompiled project's vegetation: carried while what made it is unchanged

/// The project folder a decompiled tile's vegetation files are carried in (their names as the exported tile's), with
/// [`FINGERPRINTS`].
pub const CARRIED_DIR: &str = "decompiled_vegetation";

pub use super::carry::FINGERPRINTS;
use super::carry::Fnv;

/// The kinds of stage C's outputs, each regenerated on its own when its inputs change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The tree lists (and the older `.tree_list` files the packs ship).
    Trees,

    /// The procedural BMDs and their twins (and the per-climate particle emitter markers the packs ship).
    Procedural,

    /// The grass lists.
    Grass,
}

/// The kind of an exported tile's vegetation file, by name; `None` for other files (the tile's own `particle_emitter_marker.markers`
/// is stage A's).
pub fn kind_of(name: &str) -> Option<Kind> {
    let n = name.to_lowercase();
    if n.ends_with(".tree_list.bin") || n.ends_with(".tree_list") {
        Some(Kind::Trees)
    } else if n.ends_with(".grass_list.bin") || n.ends_with(".grass_list") {
        Some(Kind::Grass)
    } else if n.ends_with("_procedural_bmd_data.bin") || n.ends_with("_procedural_bmd_data.xml") || (n.ends_with("particle_emitter_marker.markers") && n != "particle_emitter_marker.markers") {
        Some(Kind::Procedural)
    } else {
        None
    }
}

/// What each kind of stage C's output is made from, hashed: the entry, the height and blend maps (all kinds), the grass seed (grass),
/// the vegetation seeds and exclusion polygons (props, decals, VFX and trees) and the static trees as BOB collects them (trees).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fingerprints {
    pub trees: u64,
    pub procedural: u64,
    pub grass: u64,
}

impl Fingerprints {
    /// The project's.
    ///
    /// # Errors
    ///
    /// The project's generator inputs don't read ([`inputs`]).
    pub fn of(raw: &Tile) -> Result<Self, String> {
        let (inputs, scene) = inputs(raw, None)?;
        let entry = raw.entry.as_ref().map(|e| crate::xml::write(&e.root, &super::ted_project::entry_style(), &EntrySchema)).unwrap_or_default();
        let mut base = Fnv::new();
        base.bytes(&entry).str(raw.variation.as_deref().unwrap_or_default()).u32(inputs.map_w as u32).u32(inputs.map_h as u32);
        for &v in inputs.heights.iter().chain(&inputs.blend) {
            base.f32(v);
        }

        let mut grass = base;
        grass.u32(vegetation::grass_seed(&scene).unwrap_or(0));
        let seeds = vegetation::seeds(&scene).unwrap_or_default();
        let mut procedural = base;
        procedural.u32(seeds.trees).u32(seeds.decals).u32(seeds.vfx);
        for poly in &inputs.exclusions {
            procedural.u32(poly.len() as u32);
            for &(x, z) in poly {
                procedural.f32(x).f32(z);
            }
        }

        let mut trees = procedural;
        for t in vegetation::static_trees(&scene, &inputs) {
            trees.str(&t.key).f32(t.x).f32(t.y).f32(t.height).f32(t.size).u32(u32::from(t.freeform));
        }

        Ok(Self { trees: trees.0, procedural: procedural.0, grass: grass.0 })
    }

    pub fn of_kind(&self, kind: Kind) -> u64 {
        match kind {
            Kind::Trees => self.trees,
            Kind::Procedural => self.procedural,
            Kind::Grass => self.grass,
        }
    }

    /// As [`FINGERPRINTS`] holds them.
    pub fn text(&self) -> String {
        super::carry::text(&std::collections::BTreeMap::from([("trees", self.trees), ("procedural", self.procedural), ("grass", self.grass)]))
    }

    pub fn parse(text: &str) -> Option<Self> {
        let p = super::carry::parse(text);
        Some(Self { trees: *p.get("trees")?, procedural: *p.get("procedural")?, grass: *p.get("grass")? })
    }
}

/// Every vegetation file the project carries (a decompiled tile's), as the exported tile's.
///
/// # Errors
///
/// A carried file doesn't read.
pub fn carried_files(raw: &Tile) -> Result<super::stage_b::Files, String> {
    let (carried, _) = super::carry::carried(raw, CARRIED_DIR);
    carried.iter().map(|(n, b)| Ok(((*n).to_owned(), b.bytes().map_err(|e| format!("{CARRIED_DIR}/{n}: {e}"))?.to_vec()))).collect()
}

/// Stage C's files of a battle project, as the exported tile's: a kind whose files the project carries (a decompiled tile's, in
/// [`CARRIED_DIR`]) and whose inputs are unchanged since ([`Fingerprints`]) is the carried files; any other kind is generated
/// ([`stage_c_documents`]), noted when it replaces carried files.
///
/// # Errors
///
/// A kind has to be generated and the project's inputs don't read, or a file doesn't write.
pub fn stage_c_files(raw: &Tile, ctx: &StageC) -> Result<(super::stage_b::Files, CompileNotes), String> {
    let mut notes = CompileNotes::default();
    let (carried, prints) = super::carry::carried(raw, CARRIED_DIR);
    let stored = (|| Some(Fingerprints { trees: *prints.get("trees")?, procedural: *prints.get("procedural")?, grass: *prints.get("grass")? }))();
    let current = Fingerprints::of(raw);
    let kinds = [Kind::Trees, Kind::Procedural, Kind::Grass];
    let has = |k: Kind| carried.iter().any(|(n, _)| kind_of(n) == Some(k));
    // An unchanged kind the binary had no file of stays without one.
    let keep = |k: Kind| matches!((&stored, &current), (Some(s), Ok(c)) if s.of_kind(k) == c.of_kind(k));
    let mut out = vec![];
    for (name, blob) in &carried {
        if kind_of(name).is_some_and(keep) {
            out.push(((*name).to_owned(), blob.bytes().map_err(|e| format!("{CARRIED_DIR}/{name}: {e}"))?.to_vec()));
        }
    }

    let which = Outputs { trees: !keep(Kind::Trees), procedural: !keep(Kind::Procedural), grass: !keep(Kind::Grass) };
    for k in kinds.into_iter().filter(|&k| has(k) && !keep(k)) {
        let what = match k {
            Kind::Trees => "tree lists",
            Kind::Procedural => "procedural props, decals and VFX",
            Kind::Grass => "grass",
        };
        notes.notes.push(format!("stage C: the decompiled tile's {what} regenerated (the project changed since it was decompiled)"));
    }

    if which != (Outputs { trees: false, procedural: false, grass: false }) {
        let (generated, more) = stage_c_documents(raw, ctx, which)?;
        notes.notes.extend(more.notes);
        let mut tile = Tile { source_format: Some(super::tile_binary::ID), ..Default::default() };
        for name in &generated.xml {
            tile.source.insert(name.clone(), std::sync::Arc::new(super::compile::bmd_xml_style()));
        }

        tile.documents = generated.documents;
        let (files, _) = crate::TileFormat::write(&super::TileBinary, &tile).map_err(|e| e.to_string())?;
        for (name, blob) in files.folder {
            out.push((name.clone(), blob.bytes().map_err(|e| format!("{name}: {e}"))?.to_vec()));
        }
    }

    Ok((out, notes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vegetation_files_by_kind() {
        assert_eq!(kind_of("ROME_Med.tree_list.bin"), Some(Kind::Trees));
        assert_eq!(kind_of("default.tree_list"), Some(Kind::Trees));
        assert_eq!(kind_of("default.grass_list.bin"), Some(Kind::Grass));
        assert_eq!(kind_of("ROME_Med_procedural_bmd_data.xml"), Some(Kind::Procedural));
        assert_eq!(kind_of("rome_medparticle_emitter_marker.markers"), Some(Kind::Procedural));
        assert_eq!(kind_of("particle_emitter_marker.markers"), None);
        assert_eq!(kind_of("bmd_data_escalation1.bin"), None);
    }

    #[test]
    fn fingerprints_read_back() {
        let f = Fingerprints { trees: 1, procedural: u64::MAX, grass: 0x1234_5678_9abc_def0 };
        assert_eq!(Fingerprints::parse(&f.text()), Some(f));
        assert_eq!(Fingerprints::parse("trees 1\ngrass 2\n"), None);
    }

    #[test]
    fn map_order_grows_before_inserting() {
        // A single key, then the oracle's rule: 4 buckets, grown to 9 at the fifth key.
        assert_eq!(ca_map_order(&["a"]), ["a"]);
        let keys = ["mud/0", "mud/1", "mud/2", "mud/3", "grass/0", "grass/1", "grass/2", "grass/3", "forest/0", "forest/1"];
        let mut sorted = keys.to_vec();
        sorted.sort_unstable();
        // terrain_plains/1x1/aa's grass list, as BOB wrote it.
        assert_eq!(ca_map_order(&sorted), keys);
    }

    #[test]
    fn f16_ties_go_toward_zero() {
        // 1025.5 is between the f16s 1025 and 1026 (step 1 from 1024): a tie, toward zero.
        assert_eq!(f16_bob(1025.5), 1025.0);
        assert_eq!(f16_bob(-1025.5), -1025.0);
        assert_eq!(f16_bob(1025.6), 1026.0);
        assert_eq!(f16_bob(0.5), 0.5);
        assert_eq!(f16_bob(0.0), 0.0);
    }
}
