//! The files the particle system reads, as the open documents have them: an open `vfx/*.xml` or weather definitions file plays its edits
//! live whichever tool is active (`Document::particle_file`). A tool can lay preview-only bytes over a file (the VFX tool's muted emitters,
//! a curve while it is dragged): they win over the document's and are never saved.

use std::collections::{BTreeMap, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex};

use wce_vfs::{Vfs, normalize};

use crate::editor::Editor;

/// In-pack path -> bytes, shared with the particle system's reader.
type Files = Arc<Mutex<HashMap<String, Vec<u8>>>>;

#[derive(Default)]
pub struct ParticleOverlay {
    files: Files,

    /// The revision the files were gathered for (None: never).
    rev: Option<u64>,

    /// The renderer generation and the packs the reader was installed for.
    installed: Option<(u64, usize)>,

    /// Preview-only bytes by in-pack path, with the revision their owner gave them.
    overrides: HashMap<String, (u64, Vec<u8>)>,

    /// Each file's revision when the overlay was gathered last: what changed since is told to the renderer.
    per_file: BTreeMap<String, u64>,
}

/// `vfx/a/b.xml` -> `a/b`, the name the particle system loads; None for a file that is not an effect.
fn effect_name(path: &str) -> Option<String> {
    Some(path.strip_prefix("vfx/")?.strip_suffix(".xml")?.to_owned())
}

impl ParticleOverlay {
    /// The particle system reads through the overlay, then the packs.
    fn install(&mut self, r: &mut wce_render::renderer::Renderer, vfs: Arc<Vfs>) {
        let files = self.files.clone();
        r.particles().set_reader(Box::new(move |path: &str| {
            let p = normalize(path);
            if let Some(bytes) = files.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(&p) {
                return Ok(bytes.clone());
            }

            vfs.read(&p).map_err(|e| e.to_string())
        }));
    }
}

impl Editor {
    /// Lays preview-only bytes over an in-pack file the particle system reads (None takes them away). `rev` changes whenever the bytes do.
    pub fn set_particle_override(&mut self, path: &str, bytes: Option<(u64, Vec<u8>)>) {
        let p = normalize(path);
        match bytes {
            Some(b) => {
                self.particle_overlay.overrides.insert(p, b);
            }

            None => {
                self.particle_overlay.overrides.remove(&p);
            }
        }
    }

    /// Once a frame: the reader installed on a new renderer, the overlay gathered again when an open file or an override changed, and the
    /// renderer told (`RenderSettings::vfx_revision`: the weather's and the placed effects' definitions reload).
    pub(crate) fn update_particle_overlay(&mut self) {
        let Some(vfs) = self.vfs().cloned() else { return };
        let generation = self.renderer_generation;
        let key = (generation, Arc::as_ptr(&vfs) as usize);
        if self.particle_overlay.installed != Some(key) {
            let Some(r) = self.renderer.as_mut() else { return };
            self.particle_overlay.install(r, vfs);
            self.particle_overlay.installed = Some(key);
        }

        // Each file's revision: its document's edits, then an override over it.
        let mut per_file = BTreeMap::<String, u64>::new();
        let mut live = vec![];
        for &id in self.docs.order() {
            let Some(d) = self.docs.find(id) else { continue };
            let Some(path) = d.particle_file() else { continue };
            let r = d.history().version().wrapping_mul(7919).wrapping_add(u64::from(d.modified())).wrapping_add(u64::from(id) << 32);
            per_file.insert(path.clone(), r);
            live.push((id, path));
        }

        let overrides = self.particle_overlay.overrides.iter().collect::<Vec<_>>();
        for (path, (r, _)) in &overrides {
            let e = per_file.entry((*path).clone()).or_default();
            *e = e.wrapping_mul(1_000_033) ^ r.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x5157;
        }

        let mut rev = 0u64;
        for (path, r) in &per_file {
            let mut h = DefaultHasher::new();
            (path, r).hash(&mut h);
            rev = rev.wrapping_mul(1_000_003).wrapping_add(h.finish());
        }

        if self.particle_overlay.rev != Some(rev) {
            let mut files = self.particle_overlay.files.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            files.clear();
            for (id, path) in live {
                if let Some(d) = self.docs.find(id) {
                    files.insert(path, d.particle_bytes());
                }
            }

            for (path, (_, bytes)) in overrides {
                files.insert(path.clone(), bytes.clone());
            }

            drop(files);
            self.particle_overlay.rev = Some(rev);

            // The effects whose files changed (added, edited, closed): only the placed effects that read them are read again.
            let old = std::mem::take(&mut self.particle_overlay.per_file);
            let changed = per_file
                .iter()
                .filter(|(p, r)| old.get(*p) != Some(r))
                .map(|(p, _)| p)
                .chain(old.keys().filter(|p| !per_file.contains_key(*p)))
                .filter_map(|p| effect_name(p))
                .collect::<Vec<_>>();
            self.particle_overlay.per_file = per_file;
            if let Some(r) = self.renderer.as_mut() {
                for name in changed {
                    if !r.settings.vfx_changed.contains(&name) {
                        r.settings.vfx_changed.push(name);
                    }
                }
            }
        }

        if let Some(r) = self.renderer.as_mut() {
            r.settings.vfx_revision = rev;
        }
    }
}
