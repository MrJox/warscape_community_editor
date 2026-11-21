//! The edit the mouse (or G) has under way in the Tile tool's viewport: one at a time (`TileTool::gesture`), its state with it. Each begins
//! where its press is handled and ends in one place, [`TileTool::end_gesture_in`]: on its release or click (confirmed), or on Esc, a right
//! click or another tile shown (cancelled). Undo waits while one is open (`Tool::open_gesture`) and a tile switch closes it, whatever it is:
//! a new gesture is a variant here, not one more flag those had to know about.

use wce_editor::editor::Editor;
use wce_editor_core::document::Document;
use wce_foundation::math::Float3;
use wce_scene::render_scene::{DocumentId, SourceRef};
use wce_tile_editor::tile_document::TileDocument;

use crate::create_panel::AltRotate;
use crate::tool::{TileTool, document_id};
use crate::veg_brush::Stroke;

/// A grab of the selected item onto the ground under the mouse (G, or a drag of the move gizmo's centre).
pub(crate) struct Grab {
    /// The item grabbed.
    pub elem: usize,

    /// The history's position before the grab: a cancel goes back to it.
    pub history: usize,

    /// Its undo gesture (its steps merge).
    pub gesture: u64,

    /// How far above the ground it was (metres): it keeps that height.
    pub y_offset: f64,

    /// The other selected items it carries, and where the grabbed one was last (metres): they move by its steps.
    pub others: Vec<SourceRef>,
    pub last: Float3,

    /// Begun by a drag of the move gizmo's centre: its release confirms it (a G grab waits for a click).
    pub drag: bool,
}

/// How far the mouse goes (pixels) before a pressed point is dragged: less is a click, which picks the point and leaves it where it is.
pub const POINT_DRAG_PIXELS: i32 = 4;

/// What the left button (or G) is doing in the viewport.
pub(crate) enum Gesture {
    Grab(Grab),

    /// A height, paint or ground-type brush stroke (the document holds its record: `TileDocument::brush_step`).
    TerrainStroke,

    /// A vegetation brush stroke: its trees planted or taken away so far, put into the tile when it ends.
    VegStroke(Stroke),

    /// A ground coverage brush stroke (its rocks and plants so far: `CoverageUi::stroke`), put into the tile when it ends.
    CoverageStroke,

    /// An area brush stroke (its dabs so far: `AreaBrushUi::stroke`), put into the tile when it ends.
    AreaStroke,

    /// A polygon's or spline's point dragged (`TileTool::poly_point`), or a spline's handle of it (`handle`: its controlOut (true) or
    /// controlIn). Nothing moves until the mouse has gone [`POINT_DRAG_PIXELS`] from where it was pressed (`press`): a click only picks
    /// the point (user 2026-11-16: a click moved it a little, and the whole line was made again). Then the point keeps its offset from the
    /// mouse (`grab`: where it was less where the mouse pressed, on the ground), not jumping onto it.
    Point {
        handle: Option<bool>,
        gesture: u64,
        press: (i32, i32),
        moving: bool,
        grab: Float3,
    },

    /// A point light's radius handle; `grab`: the radius minus where the mouse took the handle (it does not jump to the mouse).
    LightRadius {
        light: usize,
        grab: f32,
        gesture: u64,
    },

    /// A civilian shelter's target handle, over the ground.
    ShelterTarget {
        shelter: usize,
        gesture: u64,
    },

    /// Alt + a left drag in Select mode: the selection turning around its centre.
    AltRotate(AltRotate),
}

/// How a gesture ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum End {
    /// Its release or click: kept.
    Confirm,

    /// Esc, a right click, another tile shown: a grab goes back to where it began; the others keep what they did (their undo entry
    /// takes it back), as their release would.
    Cancel,
}

impl Gesture {
    /// As the status line names it ("No undo during the grab").
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Grab(_) => "the grab",
            Self::TerrainStroke | Self::VegStroke(_) | Self::CoverageStroke | Self::AreaStroke => "the brush stroke",
            Self::Point { .. } | Self::LightRadius { .. } | Self::ShelterTarget { .. } => "the handle drag",
            Self::AltRotate(_) => "the rotation",
        }
    }
}

impl TileTool {
    /// The grab under way.
    pub(crate) fn grab(&self) -> Option<&Grab> {
        match &self.gesture {
            Some(Gesture::Grab(g)) => Some(g),
            _ => None,
        }
    }

    pub(crate) fn grabbing(&self) -> bool {
        self.grab().is_some()
    }

    /// The vegetation brush's stroke under way.
    pub(crate) fn veg_stroke(&self) -> Option<&Stroke> {
        match &self.gesture {
            Some(Gesture::VegStroke(s)) => Some(s),
            _ => None,
        }
    }

    /// Ends the gesture under way on the tile shown ([`Self::end_gesture_in`]).
    pub(crate) fn end_gesture(&mut self, e: &mut Editor, end: End) {
        let id = document_id(e).unwrap_or(0);
        self.end_gesture_in(e, id, end);
    }

    /// Ends the gesture under way on tile `id` (the gesture's, which may no longer be the one shown): a grab joined into one undo entry
    /// (confirmed) or taken back (cancelled); a brush stroke closed; a vegetation stroke's trees put into the tile (its message on the
    /// status line); a drag's undo entry closed. Nothing without one.
    pub(crate) fn end_gesture_in(&mut self, e: &mut Editor, id: DocumentId, end: End) {
        let gesture = match self.gesture.take() {
            None => return,
            Some(Gesture::VegStroke(stroke)) => {
                self.status = self.commit_veg_stroke(e, id, stroke);
                return;
            }

            Some(Gesture::CoverageStroke) => {
                self.status = self.end_coverage_stroke(e).unwrap_or_else(|err| err);
                return;
            }

            Some(Gesture::AreaStroke) => {
                self.status = self.end_area_stroke(e).unwrap_or_else(|err| err);
                return;
            }

            Some(g) => g,
        };

        let Some(d) = e.docs.find_as_mut::<TileDocument>(id) else { return };
        match gesture {
            Gesture::Grab(grab) => {
                d.history_mut().break_merge();
                if end == End::Cancel {
                    let h = d.history_mut();
                    h.jump_to(grab.history);
                    h.drop_redo();
                } else {
                    let n = d.history().position().saturating_sub(grab.history);
                    if n > 1 {
                        d.join_last(n, "Grab");
                    }
                }
            }

            Gesture::TerrainStroke => d.end_stroke(),
            _ => d.history_mut().break_merge(),
        }
    }
}
