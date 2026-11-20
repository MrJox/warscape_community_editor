//! Paths in the Inspector (docs/plans/auto_detail_2.md, H; `wce_tile::paths`, `TileDocument::find_path`): under a selection of buildings,
//! "Connect to the roads" makes a path from each to the nearest procedural road. (A path between points is the Create panel's *Path*.)

use wce_editor::editor::Editor;
use wce_editor::{gui, icons};

use crate::tool::{TileTool, document};

impl TileTool {
    /// The selected buildings, when the tile has procedural roads to connect them to.
    fn buildings_to_connect(e: &mut Editor) -> Vec<usize> {
        let sel = e.selection.items().iter().filter_map(|i| wce_tile_editor::tile_document::elem_of(i.item)).collect::<Vec<_>>();
        let Some(d) = document(e) else { return vec![] };
        let t = d.tile();
        if t.tscene().find_all(wce_tile_editor::tile_document::ROAD_SPLINE).is_empty() {
            return vec![];
        }

        sel.into_iter().filter(|&el| el < t.tscene().doc().elements().len() && t.tscene().type_of(el) == "BUILDING_INSTANCE").collect()
    }

    /// Below the selection: Connect to the roads, for its buildings.
    pub(crate) fn inspect_connect_roads(&mut self, e: &mut Editor) {
        let buildings = Self::buildings_to_connect(e);
        if buildings.is_empty() {
            return;
        }

        gui::separator_text("Paths");
        let label =
            if buildings.len() == 1 { "Connect to the roads".to_owned() } else { format!("Connect {} to the roads", buildings.len()) };
        if gui::button_sized(&format!("{}  {label}", icons::ROUTE), gui::v2(-1.0, 0.0)) {
            self.status = self.connect_to_roads(e, &buildings);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(
                "A path from each selected building to the nearest procedural road, found over the ground (round buildings, up gentle \
                 slopes, across water where it is narrowest): each a procedural road of its own you can edit",
            );
        }
    }

    /// Paths from buildings to the roads, into the working layer; what came of it.
    pub(crate) fn connect_to_roads(&mut self, e: &mut Editor, buildings: &[usize]) -> String {
        let Some(d) = document(e) else { return "no tile is open".to_owned() };
        let Some(into) = self.new_items_layer(d) else { return "this tile has no scene graph".to_owned() };
        match d.connect_to_roads(buildings, into) {
            Ok((made, failed)) if failed.is_empty() => format!("{made} paths to the roads"),
            Ok((made, failed)) => format!("{made} paths to the roads; none for {}", failed.join("; ")),
            Err(err) => err,
        }
    }
}
