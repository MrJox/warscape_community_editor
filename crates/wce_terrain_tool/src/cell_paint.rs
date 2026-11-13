//! Painting a kit map's cell layers (docs/plans/terrain_editor.md, Phase 4): the tile map (tile sets, tiles, variations by their colours
//! in the tile database), the climates and the other layers, with a brush (round or square), a rectangle, a flood fill and a picker, in
//! the Map panel or on the ground of the 3D view. A stroke is one step on the map's history (`CellStrokeCommand`).

use std::sync::Arc;

use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_scene::render_scene::DocumentId;
use wce_terrain::LayerRole;
use wce_terrain::cell_edit::CellStroke;
use wce_terrain::cells::Rgba;
use wce_terrain_editor::MapDocument;
use wce_terrain_editor::cells::CellStrokeCommand;
use wce_tile::battle_map::tile_db::TileDatabase;

pub const MODES: [&str; 4] = ["brush", "rectangle", "fill", "pick"];

/// The most palette entries the list shows (the filter narrows it).
const LIST_MAX: usize = 400;

/// A layer's palette list: names and colours.
type Palette = Vec<(String, Rgba)>;

/// A stroke being painted.
struct Painting {
    doc: DocumentId,
    layer: usize,
    stroke: CellStroke,
    last: (i64, i64),
    start: (i64, i64),
}

pub struct CellPaint {
    pub on: bool,

    /// The layer's index in the model, the tool (`MODES`), the brush's radius in cells (0: one cell) and shape.
    pub layer: usize,
    pub mode: i32,
    pub radius: i32,
    pub round: bool,
    pub colour: Rgba,
    filter: String,

    /// The palette list of a layer (document, layer): names and colours.
    entries: Option<(DocumentId, usize, Palette)>,
    painting: Option<Painting>,

    /// The rectangle being dragged (cells, inclusive corners), for the Map panel to outline.
    pub dragging: Option<[i64; 4]>,
}

impl Default for CellPaint {
    fn default() -> Self {
        Self {
            on: false,
            layer: 0,
            mode: 0,
            radius: 2,
            round: true,
            colour: [0, 0, 0, 255],
            filter: String::new(),
            entries: None,
            painting: None,
            dragging: None,
        }
    }
}

/// A layer's name for the panel and the history.
pub fn role_name(role: &LayerRole) -> String {
    match role {
        LayerRole::TileMap => "tile map".to_owned(),
        LayerRole::Climate => "climates".to_owned(),
        LayerRole::Culture => "cultures".to_owned(),
        LayerRole::Snow(s) if s.is_empty() => "snow".to_owned(),
        LayerRole::Snow(s) => format!("snow ({s})"),
        LayerRole::SeasonMask(s) => format!("season mask ({s})"),
        LayerRole::Layout => "layout".to_owned(),
    }
}

fn rgba(c: [f32; 3]) -> Rgba {
    [c[0] as u8, c[1] as u8, c[2] as u8, 255]
}

/// What can be painted on a layer: the tile database's tile sets, tiles and variations (the tile map), its climates, else the colours the
/// layer has.
fn palette(db: &TileDatabase, role: &LayerRole, used: &[Rgba]) -> Palette {
    match role {
        LayerRole::TileMap => {
            let mut out = vec![("nothing (black)".to_owned(), [0, 0, 0, 255])];
            out.extend(db.sets.iter().map(|s| (format!("set {}", s.name), rgba(s.colour))));
            for t in &db.tiles {
                out.push((format!("tile {}", t.location), rgba(t.colour)));
                out.extend(t.variations.iter().map(|v| (format!("variation {}\\{}", t.location, v.location), rgba(v.colour))));
            }

            out
        }

        LayerRole::Climate => db.climates.iter().map(|c| (format!("climate {}", c.name), rgba(c.colour))).collect(),
        _ => used.iter().map(|c| (format!("{}, {}, {}", c[0], c[1], c[2]), *c)).collect(),
    }
}

impl CellPaint {
    /// The section in the Terrain panel.
    pub fn draw_section(&mut self, e: &mut Editor, doc: DocumentId) {
        if !gui::collapsing_header("Cell layers", 0) {
            return;
        }

        let Some(d) = e.docs.find_as::<MapDocument>(doc) else { return };
        let Some(m) = d.model() else {
            gui::text_disabled("Open a kit map (not one of the game's packs) to paint its cells.");
            self.on = false;
            return;
        };

        if m.built {
            gui::text_wrapped(
                "A map of a game without kit maps (Rome 2) is edited in its built form: its tiles in the Placements section, its LF with the LF brush.",
            );
            self.on = false;
            return;
        }

        let names: Vec<String> = m.layers.iter().map(|(r, item)| format!("{} - {}", role_name(r), item.path)).collect();
        if names.is_empty() {
            gui::text_disabled("This map has no cell layers.");
            return;
        }

        self.layer = self.layer.min(names.len() - 1);
        if gui::checkbox("paint (left button, Map panel or 3D view)", &mut self.on) && self.on {
            self.dragging = None;
        }

        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut k = self.layer as i32;
        if gui::combo("layer##cells", &mut k, &refs) {
            self.layer = k.max(0) as usize;
        }

        gui::combo("tool##cells", &mut self.mode, &MODES);
        if self.mode == 0 {
            gui::slider_int("radius (cells)", &mut self.radius, 0, 64);
            gui::checkbox("round", &mut self.round);
        }

        // The colour: a swatch and its name, then the list.
        let (role, layer) = (&m.layers[self.layer].0, m.layers[self.layer].1.get());
        if self.entries.as_ref().is_none_or(|(dd, l, _)| *dd != doc || *l != self.layer) {
            let used: Vec<Rgba> = layer.histogram().into_iter().map(|(i, _)| layer.palette()[usize::from(i)]).collect();
            self.entries = Some((doc, self.layer, palette(&d.db, role, &used)));
        }

        let entries = &self.entries.as_ref().expect("made above").2;
        let current =
            entries.iter().find(|(_, c)| c[..3] == self.colour[..3]).map_or_else(|| "a colour of no entry".to_owned(), |(n, _)| n.clone());
        let mut f = [f32::from(self.colour[0]) / 255.0, f32::from(self.colour[1]) / 255.0, f32::from(self.colour[2]) / 255.0];
        if gui::color_edit3("colour", &mut f, 0) {
            self.colour = [(f[0] * 255.0).round() as u8, (f[1] * 255.0).round() as u8, (f[2] * 255.0).round() as u8, self.colour[3]];
        }

        gui::text_wrapped(&current);
        gui::input_text("filter##cells", Some("tile set, tile, variation, climate"), &mut self.filter, 128, 0);
        let needle = self.filter.to_lowercase();
        let found: Vec<&(String, Rgba)> = entries.iter().filter(|(n, _)| needle.is_empty() || n.to_lowercase().contains(&needle)).collect();
        if gui::begin_child("cell palette", gui::v2(0.0, 220.0), 1, 0) {
            for (i, (name, c)) in found.iter().take(LIST_MAX).enumerate() {
                gui::push_id_int(i as i32);
                let pos = gui::cursor_screen_pos();
                gui::window_draw_list().rect_filled(pos, gui::v2(pos.x + 14.0, pos.y + 14.0), gui::col32(c[0], c[1], c[2], 255), 2.0);
                gui::dummy(gui::v2(18.0, 14.0));
                gui::same_line();
                if gui::selectable(name, c[..3] == self.colour[..3], 0) {
                    self.colour = *c;
                }

                gui::pop_id();
            }

            if found.len() > LIST_MAX {
                gui::text_disabled(&format!("{} more: narrow the filter", found.len() - LIST_MAX));
            }
        }

        gui::end_child();
    }

    /// The left button pressed on cell `at` of the shown map. Whether a gesture started (the tool owns it until release).
    pub fn press(&mut self, e: &mut Editor, doc: DocumentId, at: (i64, i64)) -> bool {
        let Some(d) = e.docs.find_as_mut::<MapDocument>(doc) else { return false };
        let Some(l) = d.cell_layer(self.layer) else { return false };
        if at.0 < 0 || at.1 < 0 || at.0 >= i64::from(l.width()) || at.1 >= i64::from(l.height()) {
            return false;
        }

        // The picker takes the colour and is done.
        if self.mode == 3 {
            self.colour = l.colour(at.0 as u32, at.1 as u32);
            return true;
        }

        self.painting = Some(Painting { doc, layer: self.layer, stroke: CellStroke::default(), last: at, start: at });
        match self.mode {
            1 => self.dragging = Some([at.0, at.1, at.0, at.1]),
            2 => self.apply(e, |s, l, c| s.flood(l, at.0 as u32, at.1 as u32, c)),
            _ => {
                let (r, round) = (self.radius.max(0) as u32, self.round);
                self.apply(e, |s, l, c| s.brush(l, at.0, at.1, r, round, c));
            }
        }

        true
    }

    /// The button held over cell `at`: the brush along the way from the last cell, the rectangle's corner.
    pub fn drag(&mut self, e: &mut Editor, at: (i64, i64)) {
        let Some(p) = self.painting.as_mut() else { return };
        match self.mode {
            0 => {
                let (from, r, round) = (p.last, self.radius.max(0) as u32, self.round);
                p.last = at;
                let n = (at.0 - from.0).abs().max((at.1 - from.1).abs());
                for k in 1..=n {
                    let (x, y) = (from.0 + (at.0 - from.0) * k / n, from.1 + (at.1 - from.1) * k / n);
                    self.apply(e, |s, l, c| s.brush(l, x, y, r, round, c));
                }
            }

            1 => {
                let s = p.start;
                self.dragging = Some([s.0.min(at.0), s.1.min(at.1), s.0.max(at.0), s.1.max(at.1)]);
            }

            _ => {}
        }
    }

    /// The button released: the rectangle painted, the stroke onto the history. What happened, for the status line.
    pub fn release(&mut self, e: &mut Editor) -> Option<String> {
        if let (1, Some(r)) = (self.mode, self.dragging.take()) {
            self.apply(e, |s, l, c| s.rect(l, [r[0], r[1], r[2] + 1, r[3] + 1], c));
        }

        let p = self.painting.take()?;
        let d = e.docs.find_as_mut::<MapDocument>(p.doc)?;
        let role = d.model().and_then(|m| m.layers.get(p.layer)).map(|(r, _)| role_name(r)).unwrap_or_default();
        let record = p.stroke.finish(d.cell_layer(p.layer)?)?;
        let (rect, kb) = (record.rect, record.bytes() / 1024);
        let watch = d.cells.clone();
        d.push_cells(CellStrokeCommand { layer: p.layer, name: format!("paint {role}"), record: Arc::new(record), watch });
        Some(format!("{role}: cells {} .. {} x {} .. {} painted, the record keeps {kb} KB", rect[0], rect[2], rect[1], rect[3]))
    }

    /// An edit of the stroke's layer with the current colour; what it changed goes to the document's watch.
    fn apply(&mut self, e: &mut Editor, f: impl FnOnce(&mut CellStroke, &mut wce_terrain::cells::CellLayer, Rgba) -> Option<[u32; 4]>) {
        let colour = self.colour;
        let Some(p) = self.painting.as_mut() else { return };
        let Some(d) = e.docs.find_as_mut::<MapDocument>(p.doc) else { return };
        let watch = d.cells.clone();
        let Some(l) = d.cell_layer_mut(p.layer) else { return };
        if let Some(rect) = f(&mut p.stroke, l, colour) {
            watch.note(p.layer, rect);
        }
    }

    pub fn painting(&self) -> bool {
        self.painting.is_some()
    }
}
