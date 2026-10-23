//! Stage B of the compile: what BOB's Tile processor makes of a project, as the exported tile's files: `blend.dds`, `index.dds`,
//! `normal.dds`, `ground_types.dds` and `tile.agf` (`bob_tile::textures`); `outfield_mesh.rigid_model_v2`, `mesh.rigid_model_v2`,
//! `hf_height_map.data`, `hf_water_map.data` and `terrain_outlines.xml` (`terrain_mesh::terrain_files`). docs/formats/tile_compile.md,
//! stage B.

use std::path::Path;

use wce_tile_model::Tile;

use super::compile::CompileNotes;

/// What stage B needs besides the project: the kit's `raw_data` (the tile database's `_settings.xml`, the linking tiles' final
/// heights, the db tables and the buildings' raw height-map meshes) and a model's bytes by key (BOB's VFS: the game's packs, for the
/// water planes).
#[derive(Clone, Copy)]
pub struct StageB<'a> {
    pub raw_data: &'a Path,
    pub models: &'a dyn Fn(&str) -> Option<Vec<u8>>,
}

/// Files by name, as they go into the exported tile's folder.
pub type Files = Vec<(String, Vec<u8>)>;

/// The files BOB writes into the raw folder rather than the exported tile.
const RAW_FOLDER_FILES: [&str; 2] = ["final_heights.dds", "final_alpha.dds"];

/// The project folder a decompiled tile's stage-B files are carried in ([`super::carry`]).
pub const CARRIED_DIR: &str = "decompiled_terrain";

/// The groups of stage B's files, each made again on its own when its inputs change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    /// `blend.dds`, `index.dds`, `ground_types.dds`: the blend and ground type maps.
    Textures,

    /// `normal.dds`, `tile.agf`, the meshes, `hf_height_map.data`, `terrain_outlines.xml`: the heights, buildings, go outlines, rivers.
    Terrain,

    /// `hf_water_map.data`: the rivers and water planes.
    Water,
}

impl Group {
    pub const ALL: [Self; 3] = [Self::Textures, Self::Terrain, Self::Water];

    fn name(self) -> &'static str {
        match self {
            Self::Textures => "textures",
            Self::Terrain => "terrain",
            Self::Water => "water",
        }
    }
}

/// The group of a stage-B file, by name.
pub fn group_of(name: &str) -> Option<Group> {
    Some(match name.to_lowercase().as_str() {
        "blend.dds" | "index.dds" | "ground_types.dds" => Group::Textures,
        "normal.dds" | "tile.agf" | "mesh.rigid_model_v2" | "outfield_mesh.rigid_model_v2" | "hf_height_map.data" | "terrain_outlines.xml" => Group::Terrain,
        "hf_water_map.data" => Group::Water,
        _ => return None,
    })
}

/// What each group is made from, hashed: the entry (all), the blend and ground type maps (textures), the other maps, the buildings'
/// keys and world matrices, the go outlines and the rivers (terrain), the rivers and the simple meshes (the water planes).
pub fn fingerprints(raw: &Tile) -> std::collections::BTreeMap<&'static str, u64> {
    use super::carry::Fnv;
    use wce_tile_model::MapKind;
    let entry = raw.entry.as_ref().map(|e| crate::xml::write(&e.root, &super::ted_project::entry_style(), &super::schema::EntrySchema)).unwrap_or_default();
    let mut base = Fnv::new();
    base.bytes(&entry).str(raw.variation.as_deref().unwrap_or_default());
    let mut rivers = Fnv::new();
    for r in &raw.rivers {
        rivers.u32(r.index.unwrap_or(u32::MAX)).bytes(&crate::xml::write(&r.root, &crate::xml::DocStyle::ted(), &super::schema::RiverSchema));
    }

    let instances = |h: &mut Fnv, list: Vec<(String, [f32; 16])>| {
        for (k, m) in list {
            h.str(&k);
            for v in m {
                h.f32(v);
            }
        }
    };

    let mut textures = base;
    let mut terrain = base;
    for m in &raw.maps {
        let h = if matches!(m.kind, MapKind::Blend | MapKind::GroundType) { &mut textures } else { &mut terrain };
        h.u32(m.kind as u32).u32(m.index).raster(&m.raster);
    }

    instances(&mut terrain, super::compile::buildings(raw));
    for o in super::compile::go_outlines(raw) {
        terrain.u32(o.len() as u32);
        for (x, z) in o {
            terrain.f32(x).f32(z);
        }
    }

    terrain.u32(rivers.0 as u32).u32((rivers.0 >> 32) as u32);
    let mut water = base;
    water.u32(rivers.0 as u32).u32((rivers.0 >> 32) as u32);
    instances(&mut water, super::compile::instances(raw, "SIMPLE_MESH_INSTANCE"));
    std::collections::BTreeMap::from([("textures", textures.0), ("terrain", terrain.0), ("water", water.0)])
}

/// Whether a decompiled project's group is as it was decompiled (its stored fingerprint is the project's).
pub fn unchanged(raw: &Tile, group: Group) -> bool {
    let (_, stored) = super::carry::carried(raw, CARRIED_DIR);
    stored.get(group.name()).is_some_and(|v| fingerprints(raw).get(group.name()) == Some(v))
}

/// Stage B's files of a project as the exported tile's: a group whose files the project carries (a decompiled tile's, in
/// [`CARRIED_DIR`]) and whose inputs are unchanged ([`fingerprints`]) is the carried files; the others are made ([`made_files`]). A
/// group that can't be made is noted, the rest kept.
///
/// # Errors
///
/// Nothing is carried and nothing can be made (the reason of [`made_files`]).
pub fn stage_b_files(raw: &Tile, ctx: &StageB) -> Result<(Files, CompileNotes), String> {
    let (carried, stored) = super::carry::carried(raw, CARRIED_DIR);
    let current = fingerprints(raw);
    let has = |g: Group| carried.iter().any(|(n, _)| group_of(n) == Some(g));
    // An unchanged group the binary had no file of stays without one.
    let keep = |g: Group| stored.get(g.name()).is_some_and(|v| current.get(g.name()) == Some(v));
    let mut out = vec![];
    for (name, blob) in &carried {
        if group_of(name).is_some_and(keep) {
            out.push(((*name).to_owned(), blob.bytes().map_err(|e| format!("{CARRIED_DIR}/{name}: {e}"))?.to_vec()));
        }
    }

    let mut notes = CompileNotes::default();
    let wanted = Group::ALL.into_iter().filter(|&g| !keep(g)).collect::<Vec<_>>();
    if wanted.is_empty() {
        return Ok((out, notes));
    }

    for g in wanted.iter().filter(|&&g| has(g)) {
        notes.notes.push(format!("stage B: the decompiled tile's {} files made again (the project changed since it was decompiled)", g.name()));
    }

    match made_files(raw, ctx) {
        Ok((made, more)) => {
            notes.notes.extend(more.notes);
            out.extend(made.into_iter().filter(|(n, _)| group_of(n).is_some_and(|g| wanted.contains(&g))));
            Ok((out, notes))
        }

        // Carried groups stand; the others are missing, noted.
        Err(e) if !out.is_empty() => {
            let names = wanted.iter().map(|g| g.name()).collect::<Vec<_>>().join(", ");
            notes.notes.push(format!("stage B not compiled for the {names} files ({e})"));
            Ok((out, notes))
        }

        Err(e) => Err(e),
    }
}

/// Stage B's files made from the project, in the order BOB's processor declares them, with notes on what BOB itself would not finish.
///
/// # Errors
///
/// The project lacks a blend or height map, or a linking tile's heights are missing.
pub fn made_files(raw: &Tile, ctx: &StageB) -> Result<(Files, CompileNotes), String> {
    let mut notes = CompileNotes::default();
    let mut out = super::bob_tile::textures(raw)?;
    let (stamps, missing) = super::terrain_mesh::building_stamps(raw, ctx.raw_data);
    if !missing.is_empty() {
        notes.notes.push(format!(
            "stage B: no height-map mesh for {} (BOB: \"Height map mesh file ... is missing\", no hf_height_map.data nor terrain_outlines.xml)",
            missing.join(", ")
        ));
    }

    let angle = super::terrain_mesh::decimation_angle(ctx.raw_data);
    let terrain = super::terrain_mesh::terrain_files(raw, ctx.raw_data, angle, &stamps, ctx.models)?;
    out.extend(terrain.into_iter().filter(|(name, _)| !RAW_FOLDER_FILES.contains(&name.as_str())));
    Ok((out, notes))
}

/// The files of a game's `data` folder as BOB's VFS sees them: the packs in name order, the first that holds a path (case aside) wins.
/// The packs' file lists are indexed on first use.
pub struct PackIndex {
    data: std::path::PathBuf,
    index: std::sync::OnceLock<std::collections::HashMap<String, (std::path::PathBuf, String)>>,
}

impl PackIndex {
    pub fn new(data: &Path) -> Self {
        Self { data: data.to_path_buf(), index: std::sync::OnceLock::new() }
    }

    fn game() -> Option<rpfm_lib::games::GameInfo> {
        rpfm_lib::games::supported_games::SupportedGames::default().game("attila").cloned()
    }

    fn index(&self) -> &std::collections::HashMap<String, (std::path::PathBuf, String)> {
        use rpfm_lib::files::pack::Pack;
        self.index.get_or_init(|| {
            let mut packs = std::fs::read_dir(&self.data).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "pack")).collect::<Vec<_>>();
            packs.sort();
            let mut out = std::collections::HashMap::new();
            let Some(game) = Self::game() else { return out };
            for p in packs {
                let Ok(pack) = Pack::read_and_merge(std::slice::from_ref(&p), &game, true, false, false) else { continue };
                for name in pack.files().keys() {
                    out.entry(name.to_lowercase()).or_insert_with(|| (p.clone(), name.clone()));
                }
            }

            out
        })
    }

    fn key(path: &str) -> String {
        path.replace('\\', "/").to_lowercase()
    }

    /// Whether a pack holds the path.
    pub fn exists(&self, path: &str) -> bool {
        self.index().contains_key(&Self::key(path))
    }

    /// The files below a folder (`terrain/tiles/battle/.../aa`), by their path relative to it, each from the first pack that holds it;
    /// each pack is opened once.
    pub fn folder(&self, folder: &str) -> Vec<(String, Vec<u8>)> {
        use rpfm_lib::files::pack::Pack;
        let prefix = format!("{}/", Self::key(folder).trim_end_matches('/'));
        let mut by_pack: std::collections::BTreeMap<&std::path::PathBuf, Vec<(&String, String)>> = std::collections::BTreeMap::new();
        for (key, (pack, name)) in self.index() {
            if let Some(rel) = key.strip_prefix(&prefix) {
                by_pack.entry(pack).or_default().push((name, name[name.len() - rel.len()..].to_owned()));
            }
        }

        let Some(game) = Self::game() else { return vec![] };
        let mut out = vec![];
        for (pack, names) in by_pack {
            let Ok(mut pack) = Pack::read_and_merge(std::slice::from_ref(pack), &game, true, false, false) else { continue };
            for (name, rel) in names {
                if let Some(Ok(Some(bytes))) = pack.files_mut().get_mut(name).map(|f| f.encode(&None, false, false, true)) {
                    out.push((rel, bytes));
                }
            }
        }

        out.sort();
        out
    }

    /// The bytes of a path, from the first pack that holds it.
    pub fn read(&self, path: &str) -> Option<Vec<u8>> {
        use rpfm_lib::files::pack::Pack;
        let (pack, name) = self.index().get(&Self::key(path))?;
        let game = Self::game()?;
        let mut pack = Pack::read_and_merge(std::slice::from_ref(pack), &game, true, false, false).ok()?;
        pack.files_mut().get_mut(name)?.encode(&None, false, false, true).ok().flatten()
    }
}

/// A model lookup over a game's `data` folder, as BOB's VFS finds the water planes' models ([`PackIndex::read`]).
pub fn pack_models(data: &Path) -> impl Fn(&str) -> Option<Vec<u8>> + Send + Sync + use<> {
    let index = PackIndex::new(data);
    move |key: &str| index.read(key)
}
