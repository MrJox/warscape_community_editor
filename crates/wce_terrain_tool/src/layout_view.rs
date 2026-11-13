//! A campaign map's layout guide in the 3D view (Terry's `CampaignLayoutDrawer`, docs/re/terry.md): the cells of `layout_map.png` whose
//! colour `layout_map_legend.xml` lists (Terry places a guide model on each) outlined on the ground in their colour, within a radius of
//! the camera's target, drawn again when the layer changes or the camera moves on. The layer itself is painted in the Map panel.
//! DEVIATION: outlines, not Terry's guide models (101 251 cells of `main_attila_map` hold the purple one).

use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_foundation::math::Float4x4;
use wce_scene::render_scene::{DocumentId, EntityId, LayerId, Overlay, Payload, SourceRef, overlay_colour};
use wce_terrain::LayerRole;
use wce_terrain_editor::MapDocument;
use wce_tile_editor::tile_document::context::Frame;

/// The legend: a colour and its guide model.
pub type Legend = Vec<([u8; 3], String)>;

/// `layout_map_legend.xml`: `<layout_map_entry color="r;g;b">model</layout_map_entry>` lines.
pub fn parse_legend(text: &str) -> Legend {
    let mut out = vec![];
    for part in text.split("<layout_map_entry").skip(1) {
        let Some(c) = part.split("color=\"").nth(1).and_then(|r| r.split('"').next()) else { continue };
        let rgb: Vec<u8> = c.split(';').filter_map(|v| v.trim().parse().ok()).collect();
        let model = part.split('>').nth(1).and_then(|r| r.split('<').next()).unwrap_or("").trim().to_owned();
        if let [r, g, b] = rgb[..] {
            out.push(([r, g, b], model));
        }
    }

    out
}

/// What the drawn outlines are of: document, its history version, the centre cell, the radius in cells.
type Built = (DocumentId, u64, (i64, i64), u32);

pub struct LayoutView {
    pub on: bool,

    /// Cells within this of the camera's target are outlined.
    pub radius_cells: u32,
    legend: Option<Legend>,
    layer: LayerId,
    drawn: EntityId,
    built: Option<Built>,
    pub cells_drawn: usize,
}

impl Default for LayoutView {
    fn default() -> Self {
        Self { on: true, radius_cells: 96, legend: None, layer: 0, drawn: 0, built: None, cells_drawn: 0 }
    }
}

impl LayoutView {
    /// The legend from the projects' kit layout, then the Assembly Kits (`editor_working_data/Terry/layout_map_legend.xml`); none found:
    /// empty.
    fn legend(&mut self, e: &Editor) -> &Legend {
        self.legend.get_or_insert_with(|| {
            crate::tool::kit_roots(e)
                .iter()
                .filter_map(|r| r.parent())
                .map(|k| k.join("editor_working_data/Terry/layout_map_legend.xml"))
                .find_map(|p| std::fs::read(p).ok())
                .map(|b| parse_legend(&String::from_utf8_lossy(&b)))
                .unwrap_or_default()
        })
    }

    pub fn update(&mut self, e: &mut Editor, doc: DocumentId, frame: Option<std::sync::Arc<Frame>>) {
        let Some(frame) = frame.filter(|_| self.on) else { return self.clear(e) };
        let legend = self.legend(e).clone();
        let Some(d) = e.docs.find_as::<MapDocument>(doc) else { return self.clear(e) };
        let Some(layout) = d.model().and_then(|m| m.layer(&LayerRole::Layout)).map(|l| l.get()) else { return self.clear(e) };
        let tf = d.terry_frame;
        let (w, h) = (layout.width(), layout.height());
        let cam = e.viewport.camera();
        let (tx, tz) = tf.from_frame(f64::from(cam.target.x), f64::from(cam.target.z));
        let (cx, cy) = tf.cell_of(tx, tz, w, h);
        let centre = ((cx / 16.0).floor() as i64 * 16, (cy / 16.0).floor() as i64 * 16);
        let version = wce_editor_core::document::Document::history(d).version();
        let key = (doc, version, centre, self.radius_cells);
        if self.built == Some(key) {
            return;
        }

        // The legend's colour of a cell (its index), none for the others and outside the map.
        let wanted = |x: i64, y: i64| -> Option<usize> {
            if x < 0 || y < 0 || x >= i64::from(w) || y >= i64::from(h) {
                return None;
            }

            let c = layout.colour(x as u32, y as u32);
            legend.iter().position(|(rgb, _)| rgb[..] == c[..3])
        };

        let r = i64::from(self.radius_cells);
        let mut lines = vec![];
        let mut cells = 0;
        let corner = |x: i64, y: i64| {
            let (px, pz) = tf.of_cell(x as f64, y as f64, w, h);
            let (fx, fz) = tf.to_frame(px, pz);
            wce_foundation::math::Float3::new(fx as f32, frame.lift(fx, fz) + 1.5, fz as f32)
        };

        for y in (centre.1 - r).max(0)..(centre.1 + r).min(i64::from(h)) {
            for x in (centre.0 - r).max(0)..(centre.0 + r).min(i64::from(w)) {
                let Some(k) = wanted(x, y) else { continue };
                cells += 1;
                let c = legend[k].0;
                let colour = overlay_colour(f32::from(c[0]) / 255.0, f32::from(c[1]) / 255.0, f32::from(c[2]) / 255.0, 1.0);
                // The sides facing a cell of another colour.
                for (nx, ny, a, b) in [
                    (x, y - 1, (x, y), (x + 1, y)),
                    (x, y + 1, (x, y + 1), (x + 1, y + 1)),
                    (x - 1, y, (x, y), (x, y + 1)),
                    (x + 1, y, (x + 1, y), (x + 1, y + 1)),
                ] {
                    if wanted(nx, ny) != Some(k) {
                        lines.push(wce_scene::render_scene::OverlayVertex { p: corner(a.0, a.1), rgba: colour });
                        lines.push(wce_scene::render_scene::OverlayVertex { p: corner(b.0, b.1), rgba: colour });
                    }
                }
            }
        }

        if self.layer == 0 || e.scene.layer(self.layer).is_none() {
            self.layer = e.scene.add_layer(doc, "layout guide");
            self.drawn = 0;
        }

        if self.drawn != 0 {
            e.scene.remove(self.drawn);
            self.drawn = 0;
        }

        if !lines.is_empty() {
            let o = Overlay { lines, depth_test: false, pickable: false, ..Default::default() };
            self.drawn = e.scene.add(self.layer, Float4x4::IDENTITY, Payload::Overlay(o), SourceRef { doc, item: u64::MAX });
        }

        self.cells_drawn = cells;
        self.built = Some(key);
    }

    pub fn clear(&mut self, e: &mut Editor) {
        if self.layer != 0 {
            e.scene.remove_layer(self.layer);
        }

        (self.layer, self.drawn, self.built, self.cells_drawn) = (0, 0, None, 0);
    }

    /// The section's lines (in the Terry entities section of a campaign map).
    pub fn draw_controls(&mut self, e: &Editor, doc: DocumentId) {
        let has = e.docs.find_as::<MapDocument>(doc).and_then(|d| d.model()).is_some_and(|m| m.layer(&LayerRole::Layout).is_some());
        if !has {
            return;
        }

        gui::checkbox("layout guide", &mut self.on);
        if gui::is_item_hovered() {
            gui::set_tooltip(
                "layout_map.png's legend colours (layout_map_legend.xml) outlined on the ground; paint the layer in the Map panel",
            );
        }

        gui::same_line();
        let mut r = self.radius_cells as f32;
        if gui::slider_float("guide within (cells)", &mut r, 16.0, 512.0, "%.0f") {
            self.radius_cells = r as u32;
        }

        if self.on {
            gui::text_disabled(&format!(
                "{} guide cells drawn; legend: {} colours",
                self.cells_drawn,
                self.legend.as_ref().map_or(0, Vec::len)
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn legend() {
        let l = super::parse_legend(
            "<layout_map>\n\t<layout_map_entry color=\"0;0;255\">a/blue.rigid_model_v2</layout_map_entry>\n\t<layout_map_entry color=\"150;0;255\">b</layout_map_entry>\n</layout_map>",
        );
        assert_eq!(l, vec![([0, 0, 255], "a/blue.rigid_model_v2".to_owned()), ([150, 0, 255], "b".to_owned())]);
    }
}
