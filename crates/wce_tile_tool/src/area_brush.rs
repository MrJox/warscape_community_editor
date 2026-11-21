//! The area brush (`tile_document::area_brush`): every closed polygon can be painted instead of clicked. While a closed shape is drawn
//! (the Create panel, a holder's buttons, a biome) **Paint** makes a left drag grow the shape of that kind under the mouse, or make a new
//! one with the draft's values; Ctrl + drag takes away. A selected closed polygon's **Paint its shape** grows or cuts that one. The stroke
//! shows the area as it will be, filled and outlined on the ground.

use wce_editor::editor::Editor;
use wce_editor::{gui, icons};
use wce_foundation::math::Float3;
use wce_scene::render_scene::{Overlay, OverlayVertex, overlay_colour};
use wce_tile_editor::tile_document::TileDocument;
use wce_tile_editor::tile_document::area_brush::{NewArea, paintable_kind, stroke_cell};

use crate::gesture::Gesture;
use crate::styled;
use crate::tool::{TileMode, TileTool, document, document_id, primary};

/// The brush's radius at first, metres.
pub const DEFAULT_RADIUS: f32 = 12.0;

/// The area brush's state, shared by every closed type.
#[derive(Default)]
pub struct AreaBrushUi {
    /// Closed shapes being drawn are painted (else their points are clicked).
    pub paint: bool,

    /// The selected closed polygon's shape is painted.
    pub reshape: bool,

    /// The radius (0: the default).
    pub radius: f32,

    pub(crate) stroke: Option<AreaStroke>,
}

/// A stroke: taking away or adding, the area it works on (none: a new one) and its outline when it began, the dabs so far (world metres),
/// and what it shows: the area as the stroke leaves it, filled and outlined on the ground (made again as dabs come).
pub(crate) struct AreaStroke {
    erase: bool,
    area: Option<usize>,
    points: Vec<(f32, f32)>,
    dabs: Vec<(f32, f32)>,
    fill: Vec<OverlayVertex>,
    outline: Vec<OverlayVertex>,
}

/// The stroke's colours: the area it leaves, filled and outlined.
fn stroke_fill() -> u32 {
    overlay_colour(0.3, 0.95, 0.45, 0.22)
}

fn stroke_line() -> u32 {
    overlay_colour(0.35, 1.0, 0.5, 1.0)
}

/// How far over the ground the stroke is drawn, metres.
const LIFT: f32 = 0.3;

impl TileTool {
    pub(crate) fn area_radius(&self) -> f32 {
        if self.area_brush.radius > 0.0 { self.area_brush.radius } else { DEFAULT_RADIUS }
    }

    /// Whether the shape being drawn is a closed one that can be painted.
    pub(crate) fn draft_paintable(&self) -> bool {
        self.draft.as_ref().is_some_and(|d| {
            paintable_kind(&d.kind, d.attrs.get("name").map(String::as_str), d.attrs.get("polygon_shape").map(String::as_str))
        })
    }

    /// The closed shape being drawn is painted.
    pub(crate) fn painting_draft(&self) -> bool {
        self.drawing && self.area_brush.paint && self.draft_paintable()
    }

    /// The selected polygon whose shape is painted (Paint its shape on, a closed one, nothing drawn or placed).
    pub(crate) fn reshaped_area(&self, e: &Editor) -> Option<usize> {
        if !self.area_brush.reshape || self.drawing || self.placing.is_some() {
            return None;
        }

        let sel = primary(e)?;
        document_id(e)?;
        let d = e.docs.find_as::<TileDocument>(document_id(e)?)?;
        d.tile().paintable(sel).then_some(sel)
    }

    /// Whether a left drag in the viewport is the area brush's (Select mode).
    pub(crate) fn area_painting(&self, e: &Editor) -> bool {
        self.mode == TileMode::Select && (self.painting_draft() || self.reshaped_area(e).is_some())
    }

    /// A stroke begins at the cursor: on the selected polygon (Paint its shape), else on the area of the drawn kind under it (Ctrl: taking
    /// away from it), else a new one.
    pub(crate) fn begin_area_stroke(&mut self, e: &mut Editor, erase: bool) {
        let at = (self.cursor.x, self.cursor.z);
        let r = self.area_radius();
        let reshaped = self.reshaped_area(e);
        let sel = primary(e);
        let Some(d) = document(e) else { return };
        let target = match reshaped {
            Some(a) => Some((a, d.tile().polygon_world_points(a).into_iter().map(|p| (p.x, p.z)).collect::<Vec<_>>())),
            None => {
                let Some(draft) = &self.draft else { return };
                let needs_holder = !wce_tile::parents::holders(&draft.kind).is_empty();
                let within = if needs_holder { self.drawing_into(d, sel).ok() } else { None };
                let biome = draft.attrs.contains_key(wce_tile::biomes::BIOME_ATTR);
                let areas = d.tile().paint_targets(&draft.kind, biome, within);
                let edge = |p: &[(f32, f32)]| wce_tile::biomes::boundary_distance(p, at.0, at.1);
                areas
                    .iter()
                    .find(|a| wce_tile::biomes::inside(&a.1, at.0, at.1))
                    .or_else(|| areas.iter().filter(|a| edge(&a.1) < r).min_by(|a, b| edge(&a.1).total_cmp(&edge(&b.1))))
                    .cloned()
            }
        };

        if erase && target.is_none() {
            self.status = "nothing under the brush to take away from".to_owned();
            return;
        }

        let (area, points) = target.map_or((None, vec![]), |(a, p)| (Some(a), p));
        self.area_brush.stroke = Some(AreaStroke { erase, area, points, dabs: vec![at], fill: vec![], outline: vec![] });
        self.gesture = Some(Gesture::AreaStroke);
        self.area_stroke_preview(e);
    }

    /// A dab at the cursor, a quarter of the radius from the last one at least; what the stroke shows follows.
    pub(crate) fn area_dab(&mut self, e: &mut Editor) {
        let r = self.area_radius();
        let at = (self.cursor.x, self.cursor.z);
        if let Some(s) = self.area_brush.stroke.as_mut()
            && s.dabs.last().is_none_or(|l| (l.0 - at.0).hypot(l.1 - at.1) >= r * 0.25)
        {
            s.dabs.push(at);
            self.area_stroke_preview(e);
        }
    }

    /// The stroke under way, in words (a script's check): its dabs and what it shows.
    pub(crate) fn area_stroke_state(&self) -> String {
        self.area_brush.stroke.as_ref().map_or_else(
            || "no stroke".to_owned(),
            |s| format!("{} dab(s), {} fill triangle(s), {} outline segment(s)", s.dabs.len(), s.fill.len() / 3, s.outline.len() / 2),
        )
    }

    /// The stroke into the tile (`TileDocument::area_stroke`, one undo entry). A new area takes the draft's values, its seeds rolled again
    /// for the next one.
    ///
    /// # Errors
    ///
    /// No tile, nowhere to put a new area (a holder's shape with no holder), or the stroke made nothing.
    pub(crate) fn end_area_stroke(&mut self, e: &mut Editor) -> Result<String, String> {
        let Some(stroke) = self.area_brush.stroke.take() else { return Ok(String::new()) };
        let radius = self.area_radius();
        let reshaping = self.reshaped_area(e).is_some();
        let sel = primary(e);
        let id = document_id(e).ok_or("no tile is open")?;
        let d = document(e).ok_or("no tile is open")?;
        let out = match stroke.area {
            Some(area) => d.area_stroke(Some(area), None, &stroke.dabs, radius, stroke.erase)?,
            None => {
                let draft = self.draft.clone().ok_or("nothing is being drawn")?;
                let into = self.drawing_into(d, sel)?;
                let new = NewArea { kind: &draft.kind, into, key: &draft.key, attrs: &draft.attrs };
                let out = d.area_stroke(None, Some(new), &stroke.dabs, radius, false)?;
                if let Some(dr) = self.draft.as_mut() {
                    let seeds = dr.attrs.keys().filter(|k| k.ends_with("seed")).cloned().collect::<Vec<_>>();
                    for k in seeds {
                        dr.attrs.insert(k, ((self.random.unit() * 1.0e6) as u32).to_string());
                    }
                }

                out
            }
        };

        // Painting a selected one: it stays selected (the largest piece); drawing: nothing is.
        e.selection.clear();
        if reshaping && let Some(&first) = out.first() {
            e.selection.set(wce_scene::render_scene::SourceRef { doc: id, item: wce_tile_editor::tile_document::item_of(first) });
        }

        Ok(match (stroke.erase, stroke.area, out.len()) {
            (true, _, 0) => "area brush: the area went".to_owned(),
            (true, _, n) => format!("area brush: taken away ({n} piece(s) left)"),
            (false, None, _) => "area brush: a new area".to_owned(),
            (false, Some(_), _) => "area brush: the area grown".to_owned(),
        })
    }

    /// The area as the stroke leaves it, on the ground: its cells filled, its outline (as the stroke's end makes it) drawn over them.
    fn area_stroke_preview(&mut self, e: &mut Editor) {
        let radius = self.area_radius();
        let Some(d) = document(e) else { return };
        let Some(s) = self.area_brush.stroke.as_mut() else { return };
        let t = d.tile();
        let ground = |x: f32, z: f32| Float3::new(x, t.terrain_height(x, z) + LIFT, z);

        // The same grid as the stroke's end (`TileDocument::area_stroke`): its outlines, filled (their triangles cut until they follow the
        // ground).
        let cell = stroke_cell(radius);
        let outlines = wce_tile::biomes::stroke_outline(&s.points, &s.dabs, radius, s.erase, cell);
        s.fill.clear();
        let colour = stroke_fill();
        for poly in &outlines {
            let mut todo =
                wce_tile::biomes::triangulate(poly).into_iter().map(|t| [poly[t[0]], poly[t[1]], poly[t[2]]]).collect::<Vec<_>>();
            while let Some([a, b, c]) = todo.pop() {
                let long = |p: (f32, f32), q: (f32, f32)| (p.0 - q.0).hypot(p.1 - q.1) > 2.0 * cell;
                if long(a, b) || long(b, c) || long(c, a) {
                    let mid = |p: (f32, f32), q: (f32, f32)| ((p.0 + q.0) * 0.5, (p.1 + q.1) * 0.5);
                    let (ab, bc, ca) = (mid(a, b), mid(b, c), mid(c, a));
                    todo.extend([[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]]);
                    continue;
                }

                for p in [a, b, c] {
                    s.fill.push(OverlayVertex { p: ground(p.0, p.1), rgba: colour });
                }
            }
        }

        // The outlines, their edges cut a cell long so that they follow the ground.
        s.outline.clear();
        let colour = stroke_line();
        for poly in &outlines {
            for k in 0..poly.len() {
                let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
                let steps = ((b.0 - a.0).hypot(b.1 - a.1) / cell).ceil().max(1.0) as usize;
                for n in 0..steps {
                    let (t0, t1) = (n as f32 / steps as f32, (n + 1) as f32 / steps as f32);
                    let p = |t: f32| ground(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                    s.outline.push(OverlayVertex { p: p(t0), rgba: colour });
                    s.outline.push(OverlayVertex { p: p(t1), rgba: colour });
                }
            }
        }
    }

    /// The stroke under way in the viewport (the brush's circle is the tool's, at the mouse).
    pub(crate) fn area_brush_overlay(&self, o: &mut Overlay) {
        if let Some(s) = self.area_brush.stroke.as_ref() {
            o.triangles.extend_from_slice(&s.fill);
            o.lines.extend_from_slice(&s.outline);
        }
    }

    /// How a closed shape being drawn is drawn: its points clicked, or painted; the brush's radius. Shown above the shape's values.
    pub(crate) fn area_paint_rows(&mut self) {
        if !self.drawing || !self.draft_paintable() {
            return;
        }

        let row = styled::row(
            "Draw by",
            "Click its points on the ground, or paint it: a left drag grows the area under the mouse or makes a new one, Ctrl + drag takes away",
        );
        if let Some(i) = styled::segmented("area_draw", &["Click points", "Paint"], Some(usize::from(self.area_brush.paint)), row.width) {
            self.set_area_paint(i == 1);
        }

        if self.area_brush.paint {
            self.area_radius_row();
        }
    }

    /// Paints the closed shapes being drawn, or clicks their points: the points clicked so far go.
    pub(crate) fn set_area_paint(&mut self, paint: bool) {
        self.area_brush.paint = paint;
        self.draw_pts.clear();
        self.area_brush.reshape = false;
    }

    fn area_radius_row(&mut self) {
        let row = styled::row("Radius", "The brush's radius (Ctrl + click to type)");
        let mut r = self.area_radius();
        gui::set_next_item_width(row.width);
        if gui::slider_float("##area_radius", &mut r, 1.0, 100.0, "%.1f m") {
            self.area_brush.radius = r;
        }
    }

    /// A selected closed polygon: Paint its shape (left drag grows it, Ctrl + drag cuts it) and the brush's radius.
    pub(crate) fn inspect_area_reshape(&mut self, e: &mut Editor) {
        if self.drawing || self.placing.is_some() {
            return;
        }

        let Some(sel) = primary(e) else { return };
        let one = e.selection.items().len() == 1;
        let Some(d) = document(e) else { return };
        if !one || !d.tile().paintable(sel) {
            return;
        }

        gui::separator_text("Shape");
        let w = gui::content_region_avail().x;
        if styled::toggle_chip(
            "area_reshape",
            &format!("{}  Paint its shape", icons::BRUSH),
            self.area_brush.reshape,
            w,
            "Left drag in the viewport (Select mode): grows it under the brush; Ctrl + drag cuts it (what is cut off in pieces becomes copies of it)",
        ) {
            self.area_brush.reshape = !self.area_brush.reshape;
        }

        if self.area_brush.reshape {
            self.area_radius_row();
        }
    }
}
