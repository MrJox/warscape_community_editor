//! The platform check in the Tile panel's Problems (`Tile::check_platforms`): a button that checks the tile's bridges and wall walks against
//! the game's linking rules, the bridges units cannot cross and the breaks in the wall walks (a click selects one and frames it), and their
//! edges in the viewport.

use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_editor::icons;
use wce_editor_core::document::Document;
use wce_scene::render_scene::{DocumentId, Overlay, SourceRef, overlay_colour};
use wce_tile_editor::tile_document::{BridgeEdge, TileDocument, item_of};

use crate::tool::TileTool;

/// What Check platforms does (its tooltip).
const CHECK_TIP: &str = "Checks the tile's platforms as the game links them (docs/re/platforms_and_bridges.md): a platform edge links to the \
     ground only along an edge of its collision outline, with the ground within 1 m of it there; platforms join only along edges with the same \
     two corners (within 1.4 cm, and 10 cm in height). Lists the bridges (buildings of the bridge category and the roads' generated bridges) \
     units cannot get onto or that reach the ground on one side only, and the fort walls, towers and gates whose walkways meet but do not \
     join (units on the walls stop there). The viewport shows the edges: green ground links, cyan joints, blue ways onto other buildings, \
     red ramp feet the ground does not link to, orange wall walks that do not join. The ground is the editor's heights.";

impl TileTool {
    /// Runs the check on the open tile; the message says what it found.
    pub(crate) fn run_bridge_check(&mut self, e: &mut Editor, id: DocumentId) -> String {
        let Some(d) = e.docs.find_as_mut::<TileDocument>(id) else { return "no tile is open".to_owned() };
        let stamp = (id, d.history().version(), d.tile().structure_version());
        let report = d.tile_mut().check_platforms();
        let gaps = report.problems.iter().filter(|p| p.message.starts_with("walls ")).count();
        let bridges = report.problems.len() - gaps;
        let msg = format!(
            "{} bridge(s), {bridges} that units cannot cross; {} wall building(s), {} joint(s), {gaps} break(s) in the wall walks",
            report.bridges, report.walls, report.wall_joints
        );

        let msg =
            if report.unread.is_empty() { msg } else { format!("{msg}; without a logic file (not checked): {}", report.unread.join(", ")) };
        self.bridge_check = Some((stamp, report));
        msg
    }

    /// The check's part of the Problems section. True when a click selected a bridge (the section stops there).
    pub(crate) fn draw_bridge_check(&mut self, e: &mut Editor, id: DocumentId, stamp: (DocumentId, u64, u64)) -> bool {
        if gui::button(&format!("{}  Check platforms", icons::ROUTE)) {
            self.status = self.run_bridge_check(e, id);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(CHECK_TIP);
        }

        let Some((when, report)) = self.bridge_check.as_ref().filter(|(s, _)| s.0 == id) else { return false };
        gui::same_line();
        gui::checkbox("show edges##platforms", &mut self.bridge_overlay);
        if gui::is_item_hovered() {
            gui::set_tooltip("Draws the checked edges in the viewport (colours: see Check platforms' tooltip).");
        }

        if *when != stamp {
            gui::text_disabled("The tile changed since: check again.");
        }

        if report.problems.is_empty() {
            gui::text_disabled(&format!("{} bridge(s) and {} wall building(s), none with a problem.", report.bridges, report.walls));
            gui::separator();
            return false;
        }

        gui::text_wrapped(&format!(
            "{} problem(s) in {} bridge(s) and {} wall building(s):",
            report.problems.len(),
            report.bridges,
            report.walls
        ));
        let mut pick = None;
        for (i, p) in report.problems.iter().enumerate() {
            if gui::selectable(&format!("{}  {}##bridge{i}", icons::TRIANGLE_ALERT, p.message), false, 0) {
                pick = Some(p.elems.clone());
            }

            if gui::is_item_hovered() {
                gui::set_tooltip("Selects the buildings concerned and frames them.");
            }
        }

        gui::separator();
        let Some(elems) = pick else { return false };
        e.selection.clear();
        for el in elems {
            e.selection.add(SourceRef { doc: id, item: item_of(el) });
        }

        let _ = wce_editor_core::command_registry::CommandRegistry::run(e, "focus");
        true
    }

    /// The check's edges over the viewport, while they are shown and are of this tile.
    pub(crate) fn bridge_overlay(&self, d: &TileDocument, o: &mut Overlay) {
        let Some((when, report)) = &self.bridge_check else { return };
        if !self.bridge_overlay || when.0 != d.id() {
            return;
        }

        let lift = wce_foundation::math::Float3::new(0.0, 0.08, 0.0);
        for &(a, b, kind) in &report.edges {
            let rgba = match kind {
                BridgeEdge::Ground => overlay_colour(0.2, 1.0, 0.3, 1.0),
                BridgeEdge::Joint => overlay_colour(0.2, 0.9, 1.0, 1.0),
                BridgeEdge::Exit => overlay_colour(0.3, 0.45, 1.0, 1.0),
                BridgeEdge::LooseFoot => overlay_colour(1.0, 0.2, 0.15, 1.0),
                BridgeEdge::WallJoint => overlay_colour(0.2, 0.9, 1.0, 1.0),
                BridgeEdge::WallGap => overlay_colour(1.0, 0.55, 0.0, 1.0),
            };

            // Thick enough to see from afar: three lines side by side.
            for k in [-0.04f32, 0.0, 0.04] {
                let up = wce_foundation::math::Float3::new(0.0, k, 0.0);
                o.line(a + lift + up, b + lift + up, rgba);
            }
        }
    }
}
