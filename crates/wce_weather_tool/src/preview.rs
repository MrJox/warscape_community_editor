//! Second renderers for the Weather tool's previews (the split compare, the gallery, the sky strips): the editor's scene drawn with
//! another environment, by a renderer that shares the main one's models and textures but has its own frame state (exposure
//! adaptation, sky and environment cubes, wind, particles), so the main view's is not disturbed (`Renderer::new_sharing`). A preview
//! renderer that has not drawn for a while is dropped (its targets and cubes go with it).

use wce_editor::editor::Editor;
use wce_foundation::log;
use wce_render::render_target::RenderTarget;
use wce_render::renderer::{Renderer, ViewParams};
use wce_scene::camera::Camera;
use wce_scene::environment::EnvironmentParams;

/// Seconds without drawing after which a preview renderer is dropped.
const IDLE_DROP_S: f64 = 10.0;

#[derive(Default)]
pub(crate) struct Preview {
    renderer: Option<Renderer>,
    failed: bool,
    pub(crate) target: RenderTarget,
    last_used: f64,
}

impl Preview {
    /// Draws the editor's scene lit by `env` (in place of what lights it) from `camera` at `w` x `h` pixels: `frames` frames, `step`
    /// seconds apart, ending at `time` (more than one lets the exposure settle); `restart`: the temporal state starts over first. False
    /// when there is no renderer.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render(
        &mut self,
        e: &mut Editor,
        env: EnvironmentParams,
        camera: Camera,
        (w, h): (u32, u32),
        frames: u32,
        step: f32,
        restart: bool,
    ) -> bool {
        self.last_used = e.clock.time();
        if self.renderer.is_none() && !self.failed {
            let Some(main) = e.renderer() else { return false };
            match Renderer::new_sharing(main) {
                Ok(r) => self.renderer = Some(r),
                Err(err) => {
                    log::warn(format!("weather preview: no renderer ({})", err.0));
                    self.failed = true;
                }
            }
        }

        let main_settings = e.renderer().map(|m| m.settings.clone());
        let Some(r) = self.renderer.as_mut() else { return false };
        if let Some(mut s) = main_settings {
            s.selection.clear();
            s.tinted.clear();
            r.settings = s;
        }

        if w < 8 || h < 8 || !self.target.resize(e.device(), w, h) {
            return false;
        }

        if restart {
            r.restart_temporal();
        }

        // Only the environment slot is swapped: the scene (and its turn into a tile's battle frame) is the editor's.
        let slot = &mut e.scene.environment;
        let saved = (slot.active.replace(env), slot.blend.take(), slot.blend_t);
        slot.blend_t = 0.0;
        let time = e.clock.time() as f32;
        for k in 0..frames.max(1) {
            let t = time - step * (frames.max(1) - 1 - k) as f32;
            r.render(&e.scene, &ViewParams { camera: camera.clone(), time_s: t }, &mut self.target);
        }

        let slot = &mut e.scene.environment;
        (slot.active, slot.blend, slot.blend_t) = saved;
        true
    }

    /// Drops the renderer when it has not drawn for a while.
    pub(crate) fn drop_if_idle(&mut self, now: f64) {
        if self.renderer.is_some() && now - self.last_used > IDLE_DROP_S {
            self.renderer = None;
            self.target = RenderTarget::default();
        }
    }
}
