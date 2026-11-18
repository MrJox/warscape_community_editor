//! The files the particle system reads, as the open documents have them: an open `vfx/*.xml` or weather definitions file plays its edits
//! live whichever tool is active (`Document::particle_file`). A tool can lay preview-only bytes over a file (the VFX tool's muted emitters,
//! a curve while it is dragged): they win over the document's and are never saved.

use std::collections::HashMap;
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

        let mut rev = 0u64;
        let mut live = vec![];
        for &id in self.docs.order() {
            let Some(d) = self.docs.find(id) else { continue };
            let Some(path) = d.particle_file() else { continue };
            let mut h = DefaultHasher::new();
            path.hash(&mut h);
            rev = rev
                .wrapping_mul(1_000_003)
                .wrapping_add(h.finish())
                .wrapping_add(d.history().version().wrapping_mul(7919))
                .wrapping_add(u64::from(d.modified()));
            live.push((id, path));
        }

        let mut overrides = self.particle_overlay.overrides.iter().collect::<Vec<_>>();
        overrides.sort_by(|a, b| a.0.cmp(b.0));
        for (path, (r, _)) in &overrides {
            let mut h = DefaultHasher::new();
            path.hash(&mut h);
            rev = rev.wrapping_mul(1_000_033).wrapping_add(h.finish() ^ r.wrapping_mul(0x9E37_79B9_7F4A_7C15));
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
        }

        if let Some(r) = self.renderer.as_mut() {
            r.settings.vfx_revision = rev;
        }
    }
}
