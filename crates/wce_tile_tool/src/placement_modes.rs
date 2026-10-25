//! How things are placed (the chips at the top right of the viewport while placing or grabbing): on what surface and turned how.
//! - Freeform / Landscape: Above: TEd's surface modes, on the terrain under the mouse.
//! - Entity: Above: on whatever is under the mouse (a building, a rock, the terrain where nothing is), kept as freeform at that height with
//!   `wce_surface="entity"` in tile.wce (TEd gets it as freeform: `project_files::to_ted`). It does not follow the thing it stands on.
//! - Follow surface: tilted so that its up is the surface's normal (its turn about it kept), `wce_rotation="surface"` in tile.wce (TEd gets
//!   the tilted orientation).

use wce_editor::editor::Editor;
use wce_editor::tool::ViewportEvent;
use wce_editor::{gui, icons, theme};
use wce_foundation::math::{Double3, Float3};
use wce_tile_editor::tile_document::{TileDocument, elem_of, piece_of, tilted, with_root_attr};

use crate::settings::{RotationMode, SETTINGS_ID, SurfaceMode, TileSettings};
use crate::tool::{TileTool, document, document_id};

/// The surface under the mouse found for Entity: Above: its point and normal (world metres).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SurfacePoint {
    pub point: Float3,
    pub normal: Float3,
}

impl TileTool {
    /// The modes, read from the settings the first time.
    pub(crate) fn placement_modes(&mut self, e: &Editor) -> (SurfaceMode, RotationMode) {
        if self.modes.is_none() {
            let s = e.tool_settings::<TileSettings>(SETTINGS_ID);
            self.modes = Some((s.place_surface, s.place_rotation));
        }

        self.modes.unwrap_or_default()
    }

    fn set_placement_modes(&mut self, e: &mut Editor, surface: SurfaceMode, rotation: RotationMode) {
        self.modes = Some((surface, rotation));
        e.update_tool_settings(SETTINGS_ID, |s: &mut TileSettings| {
            s.place_surface = surface;
            s.place_rotation = rotation;
        });
    }

    /// While placing or grabbing with Entity: Above: the surface under the mouse (the item grabbed left out), found again when the mouse
    /// moved.
    pub(crate) fn hover_surface(&mut self, e: &mut Editor, ev: &ViewportEvent) {
        let (surface, _) = self.placement_modes(e);
        if surface != SurfaceMode::Entity || (self.placing.is_none() && !self.grabbing) {
            self.surface_hit = None;
            return;
        }

        if self.surface_px == Some((ev.x, ev.y)) {
            return;
        }

        self.surface_px = Some((ev.x, ev.y));
        let (doc, grabbed) = (document_id(e).unwrap_or(0), self.grab_elem.filter(|_| self.grabbing));
        let skip = |en: &wce_scene::render_scene::Entity| {
            en.source.doc == doc && grabbed.is_some_and(|g| elem_of(en.source.item) == Some(g) || piece_of(en.source.item).is_some_and(|(p, _)| p == g))
        };

        self.surface_hit = wce_editor::viewport::surface_under(e, ev.x, ev.y, &skip).map(|h| SurfacePoint { point: h.point, normal: h.normal });
    }

    /// Where a click places now: the surface under the mouse (Entity: Above) or the terrain under it; true: on the terrain.
    pub(crate) fn place_point(&self) -> (Float3, bool) {
        match (self.modes.unwrap_or_default().0, self.surface_hit) {
            (SurfaceMode::Entity, Some(h)) => (h.point, false),
            _ => (self.cursor, true),
        }
    }

    /// The normal of the surface a click places on: the thing under the mouse's (Entity: Above), else the terrain's at the mouse.
    fn surface_normal(&self, d: &TileDocument) -> Double3 {
        if let (SurfaceMode::Entity, Some(h)) = (self.modes.unwrap_or_default().0, self.surface_hit) {
            return Double3::new(f64::from(h.normal.x), f64::from(h.normal.y), f64::from(h.normal.z));
        }

        terrain_normal(d, self.cursor.x, self.cursor.z)
    }

    /// A text to place (an instance's, a prefab's group) with the modes applied: its surface mode, the editor's marks, the tilt (to the
    /// surface under the mouse).
    pub(crate) fn apply_placement_modes(&self, d: &TileDocument, text: Vec<u8>) -> Vec<u8> {
        self.apply_placement_modes_on(text, self.surface_normal(d))
    }

    /// The same, tilted to a given surface normal (a painted tree: the terrain's at its own point, not at the mouse).
    pub(crate) fn apply_placement_modes_on(&self, text: Vec<u8>, normal: Double3) -> Vec<u8> {
        let (surface, rotation) = self.modes.unwrap_or_default();
        let mode = if surface == SurfaceMode::Landscape { "MSM_LANDSCAPE" } else { "MSM_FREEFORM" };
        let mut text = with_root_attr(&text, "surface_mode", mode);
        if surface == SurfaceMode::Entity {
            text = with_root_attr(&text, "wce_surface", "entity");
        }

        if rotation == RotationMode::Surface {
            text = with_root_attr(&tilted(&text, normal), "wce_rotation", "surface");
        }

        text
    }

    /// The chips at the top right of the viewport while placing or grabbing: the surface modes and the rotation modes.
    pub(crate) fn draw_placement_chips(&mut self, e: &mut Editor, origin: gui::Vec2, size: gui::Vec2, top: f32) {
        if (self.placing.is_none() && !self.grabbing) || document(e).is_none() {
            return;
        }

        let (surface, rotation) = self.placement_modes(e);
        gui::set_cursor_screen_pos(gui::v2(origin.x + size.x - self.place_chips_width - 8.0, origin.y + top));
        gui::push_style_color(gui::COL_CHILD_BG, gui::v4(0.08, 0.08, 0.09, 0.78));
        let mut picked = None;
        if gui::begin_child("##placechips", gui::v2(0.0, 0.0), gui::CHILD_AUTO_RESIZE, gui::WINDOW_NO_SCROLLBAR) {
            let chip = |on: bool, icon: &str, id: &str, tip: &str| -> bool {
                if on {
                    let h = theme::palette().highlight;
                    gui::push_style_color(gui::COL_TEXT, gui::v4(h[0], h[1], h[2], h[3]));
                }

                let clicked = gui::small_button(&format!("{icon}##{id}"));
                if on {
                    gui::pop_style_color(1);
                }

                if gui::is_item_hovered() {
                    gui::set_tooltip(tip);
                }

                gui::same_line();
                clicked
            };

            let surfaces = [
                (SurfaceMode::Freeform, icons::MOVE, "Freeform: on the terrain under the mouse, keeping that height"),
                (SurfaceMode::Landscape, icons::MOUNTAIN, "Landscape: Above: stands on the terrain (snapped to it when the terrain changes)"),
                (SurfaceMode::Entity, icons::BOXES, "Entity: Above: on whatever is under the mouse (a building, a rock; the terrain where nothing is); TEd gets it as freeform"),
            ];
            for (mode, icon, tip) in surfaces {
                if chip(surface == mode, icon, &format!("surface{mode:?}"), tip) {
                    picked = Some((mode, rotation));
                }
            }

            gui::text_disabled("|");
            gui::same_line();
            let rotations = [
                (RotationMode::None, icons::ARROW_BIG_UP, "Free: as you turn it (R / Shift+R; the Inspector's orientation); not tilted"),
                (RotationMode::Surface, icons::ROTATE_3D, "Follow surface: tilted to the surface under it (the turn of R / Shift+R kept)"),
            ];
            for (mode, icon, tip) in rotations {
                if chip(rotation == mode, icon, &format!("rotation{mode:?}"), tip) {
                    picked = Some((surface, mode));
                }
            }

            // A tree: one per click, or the brush.
            let tree = self.placing == Some(crate::tool::Placing::Draft) && self.draft.as_ref().is_some_and(|d| d.kind == "VEGETATION_INSTANCE");
            let mut brush = None;
            if tree {
                gui::text_disabled("|");
                gui::same_line();
                let on = self.veg_brush(e).on;
                if chip(!on, icons::TREE_PINE, "vegsingle", "One tree per click") {
                    brush = Some(false);
                }

                if chip(on, icons::BRUSH, "vegbrush", "Brush: paint trees (Ctrl + drag takes them away); its settings are in the Inspector") {
                    brush = Some(true);
                }
            }

            if let Some(on) = brush {
                let mut b = self.veg_brush(e);
                b.on = on;
                self.set_veg_brush(e, b);
            }

            gui::new_line();
        }

        gui::end_child();
        let (lo, hi) = gui::item_rect();
        self.place_chips_width = hi.x - lo.x;
        gui::pop_style_color(1);
        if let Some((s, r)) = picked {
            self.set_placement_modes(e, s, r);
            self.surface_px = None;
        }
    }
}

/// The terrain's normal at a ground point (from the heights half a metre around it; not normalised).
pub(crate) fn terrain_normal(d: &TileDocument, x: f32, z: f32) -> Double3 {
    let t = d.tile();
    let h = |x: f32, z: f32| f64::from(t.terrain_height(x, z));
    Double3::new(h(x - 0.5, z) - h(x + 0.5, z), 1.0, h(x, z - 0.5) - h(x, z + 0.5))
}
