//! Pictures for tools that draw 2D content (the UI tool's canvas): a tool hands over decoded RGBA pixels under a key, and gets back the
//! Dear ImGui texture id once the frame has created the texture (the next frame; the renderer backend is only at hand inside `Editor::frame`).

use std::collections::HashMap;

use wce_d3d11::ID3D11Device;
use wce_imgui::renderer::D3D11Renderer;
use wce_render::texture::{Texture, make_rgba_texture};

struct Pending {
    key: String,
    w: u32,
    h: u32,
    rgba: Vec<u8>,
}

/// What a picture is on the GPU.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageRef {
    /// For `gui::image`.
    pub id: u64,
    pub w: u32,
    pub h: u32,
}

#[derive(Default)]
pub struct ImageTextures {
    pending: Vec<Pending>,
    ready: HashMap<String, ImageRef>,
    keep: Vec<Texture>,
}

impl ImageTextures {
    /// True if `key` is queued or ready.
    pub fn knows(&self, key: &str) -> bool {
        self.ready.contains_key(key) || self.pending.iter().any(|p| p.key == key)
    }

    /// Queues RGBA8 pixels (w * h * 4 bytes, rows top to bottom).
    pub fn submit(&mut self, key: &str, w: u32, h: u32, rgba: Vec<u8>) {
        if !self.knows(key) {
            self.pending.push(Pending { key: key.to_owned(), w, h, rgba });
        }
    }

    pub fn get(&self, key: &str) -> Option<ImageRef> {
        self.ready.get(key).copied()
    }

    /// Forgets one picture so the next `submit` replaces it (its file changed).
    pub fn forget(&mut self, key: &str) {
        self.pending.retain(|p| p.key != key);
        self.ready.remove(key);
    }

    /// Forgets everything (the game folder changed).
    pub fn clear(&mut self) {
        self.pending.clear();
        self.ready.clear();
        self.keep.clear();
    }

    pub(crate) fn flush(&mut self, dev: &ID3D11Device, textures: &mut D3D11Renderer) {
        for p in self.pending.drain(..) {
            let t = make_rgba_texture(dev, p.w, p.h, &p.rgba, false);
            if let Some(srv) = &t.srv {
                let id = textures.register(srv).id();
                self.ready.insert(p.key, ImageRef { id, w: p.w, h: p.h });
                self.keep.push(t);
            }
        }
    }
}
