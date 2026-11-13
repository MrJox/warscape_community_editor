//! The Inspector's sky strips (the Sky section of an .environment file): the sky the game draws with the file, looking up from the
//! horizon towards the sun and away from it, each drawn by a preview renderer of its own (`preview.rs`) through the whole pipeline (sky,
//! clouds, fog, exposure, tone map), so what the strips show is the game's sky, not a model of it. Drawn again once the light has been
//! still for a moment after a change (not while the sun is dragged: each drawing makes the preview's sky and environment cubes again),
//! with a few frames that let its exposure follow, as the game's adapts to what the camera sees. Shown straight from the preview's target
//! (`ImageTextures::show_view`): no copy back from the GPU.

use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_foundation::math::Float3;
use wce_scene::camera::Camera;
use wce_scene::environment::EnvironmentParams;

/// A strip's size (points), its vertical field of view and how far up its middle looks (degrees).
pub(crate) const STRIP: (f32, f32) = (56.0, 150.0);
const FOV_DEG: f32 = 86.0;
const PITCH_DEG: f32 = 41.0;

/// How long the light must stay unchanged before the strips are drawn again (seconds), and the frames that let the exposure follow
/// (the first drawing starts it over and gives it more).
const SETTLE_S: f64 = 0.3;
const FRAMES: u32 = 4;
const FIRST_FRAMES: u32 = 12;
const STEP_S: f32 = 0.25;

/// The image keys of the two strips.
pub(crate) const KEYS: [&str; 2] = ["weather_sky_sun", "weather_sky_away"];

#[derive(Default)]
pub(crate) struct SkyStrips {
    previews: [crate::preview::Preview; 2],
    drawn: Option<String>,

    /// The light last seen and since when it has not changed.
    seen: Option<String>,
    since: f64,
}

impl SkyStrips {
    /// Draws the strips again once `env` (what lights the viewport) has been unchanged for `SETTLE_S` after differing from what they show;
    /// the previews go when no environment file is shown for a while.
    pub(crate) fn update(&mut self, e: &mut Editor, shown: bool, env: &EnvironmentParams) {
        let now = e.clock.time();
        if !shown {
            for p in &mut self.previews {
                p.drop_if_idle(now);
            }

            return;
        }

        let state = format!("{env:?}");
        if self.seen.as_deref() != Some(state.as_str()) {
            self.seen = Some(state);
            self.since = now;
            return;
        }

        if self.drawn == self.seen || now - self.since < SETTLE_S {
            return;
        }

        let first = self.drawn.is_none();
        let eye = e.viewport.camera().eye;
        let density = gui::io().DisplayFramebufferScale.x.max(1.0);
        let px = ((STRIP.0 * density) as u32, (STRIP.1 * density) as u32);
        for (k, turn) in [0.0f32, 180.0].into_iter().enumerate() {
            let (az, pitch) = ((env.sun_azimuth + turn).to_radians(), PITCH_DEG.to_radians());
            let d = Float3::new(pitch.cos() * az.sin(), pitch.sin(), pitch.cos() * az.cos());
            let camera = Camera {
                eye,
                target: Float3::new(eye.x + d.x, eye.y + d.y, eye.z + d.z),
                up: Float3::new(0.0, 1.0, 0.0),
                fov_y: FOV_DEG.to_radians(),
                ..Camera::default()
            };
            let frames = if first { FIRST_FRAMES } else { FRAMES };
            if !self.previews[k].render(e, env.clone(), camera, px, frames, STEP_S, first) {
                return;
            }

            if let Some(srv) = self.previews[k].target.srv() {
                e.images.show_view(KEYS[k], srv, px.0, px.1);
            }
        }

        self.drawn = self.seen.clone();
    }

    /// Writes the two strips side by side as a PNG (`weather_sky_png`, for checks).
    ///
    /// # Errors
    ///
    /// Not drawn yet, or the file can't be written.
    pub(crate) fn save_png(&self, e: &Editor, path: &std::path::Path) -> Result<(), String> {
        let read = |k: usize| {
            let t = &self.previews[k].target;
            t.read_rgba8(e.device(), e.context()).map(|px| (t.width() as usize, t.height() as usize, px))
        };
        let (Some((w, h, a)), Some((w2, h2, b))) = (read(0), read(1)) else {
            return Err("the sky strips are not drawn yet".to_owned());
        };
        if (w, h) != (w2, h2) {
            return Err("the strips differ in size".to_owned());
        }

        let mut pixels = Vec::with_capacity(w * 2 * h * 3);
        for y in 0..h {
            for src in [&a, &b] {
                for x in 0..w {
                    let i = (y * w + x) * 4;
                    pixels.extend_from_slice(&src[i..i + 3]);
                }
            }
        }

        let png = wce_formats::png_file::PngFile {
            width: (w * 2) as u32,
            height: h as u32,
            bit_depth: 8,
            color_type: 2,
            pixels,
            ..Default::default()
        };
        let bytes = png.encode().map_err(|err| format!("{err:?}"))?;
        std::fs::write(path, bytes).map_err(|err| err.to_string())
    }

    /// The strips for the Inspector: (texture id, width, height, caption), when they are drawn.
    pub(crate) fn images(e: &Editor) -> Vec<(u64, f32, f32, &'static str)> {
        let captions = ["towards the sun", "away from it"];
        KEYS.iter().zip(captions).filter_map(|(k, c)| e.images.get(k).map(|img| (img.id, STRIP.0, STRIP.1, c))).collect()
    }
}
