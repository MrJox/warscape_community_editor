//! Pictures for tools that draw 2D content (the UI tool's canvas): a tool hands over decoded RGBA pixels under a key, and gets back the
//! Dear ImGui texture id once the frame has created the texture (the next frame; the renderer backend is only at hand inside `Editor::frame`).

use std::collections::HashMap;

use wce_d3d11::{ID3D11Device, ID3D11DeviceContext, ID3D11ShaderResourceView};
use wce_imgui::renderer::D3D11Renderer;
use wce_render::texture::{PatchableRgba, Texture, make_rgba_texture};

struct Pending {
    key: String,
    w: u32,
    h: u32,
    rgba: Vec<u8>,

    /// Written in place later (`patch`) rather than made again.
    patchable: bool,
}

/// A changed part of a patchable picture: the whole picture as it is now and the rectangle (x0, y0, x1, y1; end exclusive) that changed.
struct Patch {
    key: String,
    rgba: std::sync::Arc<Vec<u8>>,
    rect: [u32; 4],
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

    /// The textures behind `ready`, by id (a patchable one's in `patchable`).
    keep: HashMap<u64, Texture>,
    patchable: HashMap<u64, PatchableRgba>,
    patches: Vec<Patch>,

    /// Live views of a tool's own GPU textures (`show_view`), registered or re-pointed at the next flush.
    views: Vec<(String, ID3D11ShaderResourceView, u32, u32)>,

    /// Ids forgotten since the last flush: their textures go then (a frame may still draw them until it ends).
    retired: Vec<u64>,
}

impl ImageTextures {
    /// True if `key` is queued or ready.
    pub fn knows(&self, key: &str) -> bool {
        self.ready.contains_key(key) || self.pending.iter().any(|p| p.key == key)
    }

    /// Queues RGBA8 pixels (w * h * 4 bytes, rows top to bottom).
    pub fn submit(&mut self, key: &str, w: u32, h: u32, rgba: Vec<u8>) {
        if !self.knows(key) {
            self.pending.push(Pending { key: key.to_owned(), w, h, rgba, patchable: false });
        }
    }

    /// Queues a picture that changes in parts (a painted map's cells): `patch` writes a changed rectangle instead of the whole picture.
    pub fn submit_patchable(&mut self, key: &str, w: u32, h: u32, rgba: Vec<u8>) {
        if !self.knows(key) {
            self.pending.push(Pending { key: key.to_owned(), w, h, rgba, patchable: true });
        }
    }

    /// A patchable picture changed in `rect` (x0, y0, x1, y1; end exclusive): `rgba` is the whole picture as it is now. Written at the
    /// next frame; nothing for a picture that is not patchable (or not made yet: its pixels are taken when it is).
    pub fn patch(&mut self, key: &str, rgba: std::sync::Arc<Vec<u8>>, rect: [u32; 4]) {
        if let Some(p) = self.pending.iter_mut().find(|p| p.key == key) {
            if p.rgba.len() == rgba.len() {
                p.rgba.copy_from_slice(&rgba);
            }

            return;
        }

        match self.patches.iter_mut().find(|p| p.key == key) {
            Some(p) => {
                p.rgba = rgba;
                p.rect = [p.rect[0].min(rect[0]), p.rect[1].min(rect[1]), p.rect[2].max(rect[2]), p.rect[3].max(rect[3])];
            }

            None => self.patches.push(Patch { key: key.to_owned(), rgba, rect }),
        }
    }

    /// Shows a texture the tool renders into itself (a preview's render target) under `key`, without copying it: registered at the next
    /// flush, re-pointed when the view changes (a resized target makes a new one). The tool keeps the texture alive while it is shown.
    pub fn show_view(&mut self, key: &str, srv: &ID3D11ShaderResourceView, w: u32, h: u32) {
        self.views.retain(|v| v.0 != key);
        self.views.push((key.to_owned(), srv.clone(), w, h));
    }

    pub fn get(&self, key: &str) -> Option<ImageRef> {
        self.ready.get(key).copied()
    }

    /// Forgets one picture so the next `submit` replaces it (its file changed).
    pub fn forget(&mut self, key: &str) {
        self.pending.retain(|p| p.key != key);
        self.views.retain(|v| v.0 != key);
        self.patches.retain(|p| p.key != key);
        if let Some(r) = self.ready.remove(key) {
            self.retired.push(r.id);
        }
    }

    /// Forgets everything (the game folder changed).
    pub fn clear(&mut self) {
        self.pending.clear();
        self.patches.clear();
        self.retired.extend(self.ready.drain().map(|(_, r)| r.id));
    }

    pub(crate) fn flush(&mut self, dev: &ID3D11Device, ctx: &ID3D11DeviceContext, textures: &mut D3D11Renderer) {
        for id in self.retired.drain(..) {
            textures.unregister(dear_imgui_rs::TextureId::new(id));
            self.keep.remove(&id);
            self.patchable.remove(&id);
        }

        for (key, srv, w, h) in self.views.drain(..) {
            match self.ready.get(&key) {
                Some(r) => {
                    textures.replace(dear_imgui_rs::TextureId::new(r.id), &srv);
                    self.ready.insert(key, ImageRef { id: r.id, w, h });
                }

                None => {
                    let id = textures.register(&srv).id();
                    self.ready.insert(key, ImageRef { id, w, h });
                }
            }
        }

        for p in self.pending.drain(..) {
            if p.patchable {
                let Some(t) = PatchableRgba::new(dev, p.w, p.h, &p.rgba) else { continue };
                let Some(srv) = &t.texture.srv else { continue };
                let id = textures.register(srv).id();
                self.ready.insert(p.key, ImageRef { id, w: p.w, h: p.h });
                self.patchable.insert(id, t);
                continue;
            }

            let t = make_rgba_texture(dev, p.w, p.h, &p.rgba, false);
            if let Some(srv) = &t.srv {
                let id = textures.register(srv).id();
                self.ready.insert(p.key, ImageRef { id, w: p.w, h: p.h });
                self.keep.insert(id, t);
            }
        }

        for p in self.patches.drain(..) {
            if let Some(r) = self.ready.get(&p.key)
                && let Some(t) = self.patchable.get_mut(&r.id)
                && t.fits(r.w, r.h)
            {
                t.patch(ctx, &p.rgba, p.rect);
            }
        }
    }
}
