//! The Inspector's sky strips (the Sky section of an .environment file): the sky the game draws with the file, looking up from the
//! horizon towards the sun and away from it, drawn by a preview renderer of its own (`preview.rs`) through the whole pipeline (sky,
//! clouds, fog, exposure, tone map), so what the strips show is the game's sky, not a model of it. Drawn again when the file's light
//! changes, at most four times a second; each strip's exposure starts over and settles on its own, as the game's adapts to what the
//! camera sees.

use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_foundation::math::Float3;
use wce_scene::camera::Camera;
use wce_scene::environment::EnvironmentParams;

/// A strip's size (points), its vertical field of view and how far up its middle looks (degrees).
pub(crate) const STRIP: (f32, f32) = (56.0, 150.0);
const FOV_DEG: f32 = 86.0;
const PITCH_DEG: f32 = 41.0;

/// The least time between two drawings (seconds), and the frames that settle the exposure.
const EVERY_S: f64 = 0.25;
const FRAMES: u32 = 12;
const STEP_S: f32 = 0.25;

/// The image keys of the two strips.
pub(crate) const KEYS: [&str; 2] = ["weather_sky_sun", "weather_sky_away"];

#[derive(Default)]
pub(crate) struct SkyStrips {
    preview: crate::preview::Preview,
    drawn: Option<String>,
    at: f64,
    made: [bool; 2],

    /// The last pixels of each strip (`weather_sky_png`).
    last: [Option<(u32, u32, Vec<u8>)>; 2],
}

impl SkyStrips {
    /// Draws the strips again when `env` (what lights the viewport) changed since the last time; nothing while `doc` is 0.
    pub(crate) fn update(&mut self, e: &mut Editor, shown: bool, env: &EnvironmentParams) {
        let now = e.clock.time();
        let state = format!("{env:?}");
        if !shown || self.drawn.as_deref() == Some(state.as_str()) || now - self.at < EVERY_S {
            self.preview.drop_if_idle(now);
            return;
        }

        self.at = now;
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
            if !self.preview.render(e, env.clone(), camera, px, FRAMES, STEP_S, true) {
                return;
            }

            // A copy: the next strip draws into the same target.
            if let Some(rgba) = self.preview.target.read_rgba8(e.device(), e.context()) {
                self.last[k] = Some((px.0, px.1, rgba.clone()));
                // Written in place once made (no frame without it); made again when its size changed.
                match e.images.get(KEYS[k]) {
                    Some(img) if self.made[k] && (img.w, img.h) == px => {
                        e.images.patch(KEYS[k], std::sync::Arc::new(rgba), [0, 0, px.0, px.1])
                    }
                    _ => {
                        e.images.forget(KEYS[k]);
                        e.images.submit_patchable(KEYS[k], px.0, px.1, rgba);
                    }
                }

                self.made[k] = true;
            }
        }

        self.drawn = Some(state);
    }

    /// Writes the two strips side by side as a PNG (`weather_sky_png`, for checks).
    ///
    /// # Errors
    ///
    /// Not drawn yet, or the file can't be written.
    pub(crate) fn save_png(&self, path: &std::path::Path) -> Result<(), String> {
        let (Some((w, h, a)), Some((w2, h2, b))) = (&self.last[0], &self.last[1]) else {
            return Err("the sky strips are not drawn yet".to_owned());
        };
        if (w, h) != (w2, h2) {
            return Err("the strips differ in size".to_owned());
        }

        let (w, h) = (*w as usize, *h as usize);
        let mut pixels = Vec::with_capacity(w * 2 * h * 3);
        for y in 0..h {
            for src in [a, b] {
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
