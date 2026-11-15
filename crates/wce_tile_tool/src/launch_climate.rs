//! Launch in game's *the tile alone, in the editor's climate* (`LaunchMap::Tile`): a tile launched alone is drawn in the `default`
//! climate whatever the battle asks (climate index 0, summer; docs/re/game_command_line.md), so the launch's files give the `default`
//! climate the chosen climate's look for what the tile uses: its texture groups' ground textures (the chosen season's, as the game falls
//! back: the season's, then summer, then `default`'s) under `default`'s summer names, its grass keys' models under `default`'s names
//! (a key the climate has no model for leaves the grass list: `default`'s rock and mud patches are drawn where a real climate has none),
//! and the climate's tree list and procedural props as `default`'s.

use wce_formats::tile_climate::climate_texture_path;
use wce_tile_io::attila::TileBinary;
use wce_tile_io::{TileFiles, TileFormat};
use wce_tile_model::{Blob, Tile, Value};
use wce_vfs::Vfs;

/// The climate the tile alone is launched in.
#[derive(Clone, Debug)]
pub(crate) struct AloneClimate {
    /// As the climates name it (`ROME_Continental`).
    pub climate: String,

    /// `su`, `au`, `wi`, `sp`.
    pub season: String,

    /// The tile's texture groups (its entry's).
    pub groups: Vec<String>,
}

const GRASS_LIST: &str = "default.grass_list.bin";

impl AloneClimate {
    /// Gives `files` (the launch's, by their paths in the game's files; the exported tile's under `folder`) the climate's look as the
    /// `default` climate's. What it did, for the log.
    ///
    /// # Errors
    ///
    /// A pack file or the grass list does not read.
    pub(crate) fn apply(&self, files: &mut Vec<(String, Vec<u8>)>, vfs: &Vfs, folder: &str) -> Result<String, String> {
        let climate = self.climate.to_ascii_lowercase();
        if climate.eq_ignore_ascii_case(crate::launch::ALONE_CLIMATE) {
            return Ok("the tile alone in the default climate".to_owned());
        }

        let exists = |p: &str| vfs.owner(p).is_some();
        let read = |p: &str| vfs.read(p).map_err(|e| format!("{p}: {e}"));
        let put = |files: &mut Vec<(String, Vec<u8>)>, path: String, bytes: Vec<u8>| match files.iter_mut().find(|(p, _)| *p == path) {
            Some(f) => f.1 = bytes,
            None => files.push((path, bytes)),
        };

        // The ground: each group's four variations, diffuse and normal.
        let source = format!("terrain/textures/battle/{climate}");
        let mut textures = 0;
        for group in self.groups.iter().filter(|g| !g.is_empty()) {
            for variation in 0..4 {
                for normal in [false, true] {
                    let Some(from) = climate_texture_path(&source, group, variation, &self.season, normal, exists) else { continue };
                    let to =
                        format!("terrain/textures/battle/default/{group}/{variation}_su_{}.dds", if normal { "normal" } else { "diffuse" });
                    if from != to {
                        put(files, to, read(&from)?);
                        textures += 1;
                    }
                }
            }
        }

        // The grass: the keys the climate has a model for, their models as default's.
        let list = format!("{folder}/{GRASS_LIST}");
        let mut grass = (0, 0);
        if let Some(i) = files.iter().position(|(p, _)| *p == list) {
            let mut kept = vec![];
            let (doc, dropped) = keep_grass(&files[i].1, |key| {
                let ok = exists(&grass_model(&climate, key));
                if ok {
                    kept.push(key.to_owned());
                }
                ok
            })?;
            files[i].1 = doc;
            for key in &kept {
                put(files, grass_model("default", key), read(&grass_model(&climate, key))?);
            }

            grass = (kept.len(), dropped);
        }

        // The trees and the procedural props: the climate's lists as default's.
        let mut lists = vec![];
        for (from, to) in [
            (format!("{climate}.tree_list.bin"), "default.tree_list.bin"),
            (format!("{climate}_procedural_bmd_data.bin"), "default_procedural_bmd_data.bin"),
            (format!("{climate}_procedural_bmd_data.xml"), "default_procedural_bmd_data.xml"),
        ] {
            // In any case: a carried list is named as the packs have it (`rome_continental`), a generated one as the climate is
            // (`ROME_Continental`); the game's paths are not case-sensitive.
            let path = format!("{folder}/{from}");
            if let Some((_, bytes)) = files.iter().find(|(p, _)| p.eq_ignore_ascii_case(&path)) {
                let bytes = bytes.clone();
                put(files, format!("{folder}/{to}"), bytes);
                lists.push(from);
            }
        }

        let trees = if lists.is_empty() {
            format!("no {climate} tree list in the tile: default's trees")
        } else {
            format!("{} as default's", lists.join(", "))
        };
        Ok(format!(
            "the tile alone in {} ({}): {textures} ground texture(s) and {} grass model(s) as default's, {} grass key(s) {climate} has no model \
             for left out; {trees}",
            self.climate, self.season, grass.0, grass.1
        ))
    }
}

fn grass_model(climate: &str, key: &str) -> String {
    format!("terrain/vegetation/battle/grass/{climate}/{key}.rigid_model_v2")
}

/// A grass list (`default.grass_list.bin`) with only the keys `keep` takes, and how many it left out.
fn keep_grass(bytes: &[u8], mut keep: impl FnMut(&str) -> bool) -> Result<(Vec<u8>, usize), String> {
    let files = TileFiles {
        folder: [(GRASS_LIST.to_owned(), Blob::Bytes(bytes.to_vec().into()))].into_iter().collect(),
        entry: None,
        variation: None,
    };
    let (read, _) = TileBinary.read(&files).map_err(|e| format!("{GRASS_LIST}: {e}"))?;
    let mut doc = read.documents.into_iter().find(|d| d.name == GRASS_LIST).ok_or_else(|| format!("{GRASS_LIST}: not read"))?;
    let before = doc.root.children.len();
    doc.root.children.retain(|c| c.class != "type" || keep(c.get("key").and_then(Value::as_str).unwrap_or_default()));
    let dropped = before - doc.root.children.len();
    let one = Tile { documents: vec![doc], source_format: read.source_format, source: read.source, ..Default::default() };
    let (out, _) = TileBinary.write(&one).map_err(|e| format!("{GRASS_LIST}: {e}"))?;
    let blob = out.folder.get(GRASS_LIST).ok_or_else(|| format!("{GRASS_LIST}: not written"))?;
    Ok((blob.bytes().map_err(|e| e.to_string())?.to_vec(), dropped))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wce_tile_model::{Document, Node};

    #[test]
    fn a_grass_list_keeps_the_keys_asked_for() {
        let mut root = Node::new("GRASS_LIST_BIN");
        root.set("version", Value::Int(1));
        for key in ["grass/0", "rock/1", "mud/2"] {
            let mut n = Node::new("type");
            n.set("key", Value::Str(key.to_owned()));
            n.set("position", Value::Floats(vec![1.0, 2.0, 3.0]));
            root.children.push(n);
        }

        let tile = Tile { documents: vec![Document { name: GRASS_LIST.to_owned(), root }], ..Default::default() };
        let (files, _) = TileBinary.write(&tile).unwrap();
        let bytes = files.folder[GRASS_LIST].bytes().unwrap().to_vec();
        let (kept, dropped) = keep_grass(&bytes, |k| k.starts_with("grass")).unwrap();
        assert_eq!(dropped, 2);
        let (_, again) = keep_grass(&kept, |_| true).unwrap();
        assert_eq!(again, 0);
        let (none, gone) = keep_grass(&kept, |_| false).unwrap();
        assert_eq!(gone, 1);
        assert!(none.len() < kept.len());
    }
}
