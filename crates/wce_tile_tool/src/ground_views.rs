//! The ground views (docs/editor.md): what the ground shows, picked from a chip at the top right of the viewport, as the weather chips at
//! its top left: the view (`TerrainLook::view`: the colour or a debug view of the tile's maps), the climate and season, the water depth
//! view's tools, the water, the sea, the procedural trees, the grass and the automatic no-go areas. The Minimap has a chip of its own with
//! the same views and its own (minimap_panel.rs).

use wce_editor::editor::Editor;
use wce_editor::{gui, icons, theme};
use wce_scene::render_scene::DocumentId;
use wce_tile_editor::terrain_bake::TerrainView;
use wce_tile_editor::tile_document::{GENERATED_PREFIX, TileDocument};

use crate::tool::{PAINT_WATER_TIP, TileTool, WATER_VIEW_TIP, document_id, set_sea_state};

/// The views of the ground the viewport and the minimap share: the view, its name, what it shows.
pub(crate) const GROUND_VIEWS: [(TerrainView, &str, &str); 5] = [
    (TerrainView::Colour, "colour", "The ground's colour: the tile's blend map mixed from the climate's textures, by the game's shaders."),
    (TerrainView::BlendGroups, "blend groups", "TEd's debug view of the blend map: each texture group in a colour of its own."),
    (TerrainView::GroundTypes, "ground types", "The ground type map as stored (ground_type_map.png): what the units walk on. The Ground brush shows it while it paints."),
    (TerrainView::Protection, "protection", "The protection map (protection_map.dds): where nothing is placed by the map's generation."),
    (TerrainView::WaterDepth, "water depth", WATER_VIEW_TIP),
];

pub(crate) const AUTO_NOGO_TIP: &str = "The no-go areas the export adds by itself (BOB's terrain_outlines.xml): the ground steeper than the units can \
climb (30 degrees) in the final heights, less the go regions, as the game gets them. Worked out from the tile as it is (stage B's heights), \
again once it has not changed for a moment after an edit.";

/// A view's name.
pub(crate) fn view_name(view: TerrainView) -> &'static str {
    GROUND_VIEWS.iter().find(|v| v.0 == view).map_or("colour", |v| v.1)
}

/// The views as selectables, `current` marked; the one clicked.
pub(crate) fn view_items(current: Option<TerrainView>, id: &str) -> Option<TerrainView> {
    let mut picked = None;
    for (view, name, tip) in GROUND_VIEWS {
        if gui::selectable(&format!("{name}##{id}"), current == Some(view), 0) {
            picked = Some(view);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(tip);
        }
    }

    picked
}

/// A chip: an icon, lit while `on`; true when clicked.
pub(crate) fn chip(on: bool, icon: &str, id: &str, tip: &str) -> bool {
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

    clicked
}

impl TileTool {
    /// The ground views' chip at the top right of the viewport (`top` down from its edge); its popup.
    pub(crate) fn draw_ground_chip(&mut self, e: &mut Editor, origin: gui::Vec2, size: gui::Vec2, top: f32) {
        let Some(id) = document_id(e) else { return };
        let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
        let (look, nogo) = (d.tile().terrain_look().clone(), d.tile().display().show_auto_nogo);
        let view = if look.blended { view_name(look.view) } else { "plain tiled texture (blend off)" };
        let tip = format!(
            "Ground: {view}{}{}\nClick: the views, the climate and season, the water, the sea, the trees, the grass, the automatic no-go areas",
            if look.blended { format!(" ({}, {})", look.climate, season_name(&look.season)) } else { String::new() },
            if nogo { ", automatic no-go areas" } else { "" },
        );

        gui::set_cursor_screen_pos(gui::v2(origin.x + size.x - self.ground_chip_width - 8.0, origin.y + top));
        gui::push_style_color(gui::COL_CHILD_BG, gui::v4(0.08, 0.08, 0.09, 0.78));
        if gui::begin_child("##groundchip", gui::v2(0.0, 0.0), gui::CHILD_AUTO_RESIZE, gui::WINDOW_NO_SCROLLBAR) {
            if chip(look.view != TerrainView::Colour || nogo, icons::LAYERS, "groundviews", &tip) {
                gui::open_popup("groundviews");
            }

            if gui::begin_popup("groundviews") {
                self.draw_ground_popup(e, id);
                gui::end_popup();
            }
        }

        gui::end_child();
        let (lo, hi) = gui::item_rect();
        self.ground_chip_width = hi.x - lo.x;
        gui::pop_style_color(1);
    }

    fn draw_ground_popup(&mut self, e: &mut Editor, id: DocumentId) {
        let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
        let mut look = d.tile().terrain_look().clone();
        let mut changed = false;
        gui::text_disabled("Ground view");
        // A view needs the blend: picking one turns it on (as `tile_ground_view`).
        if let Some(v) = view_items(look.blended.then_some(look.view), "groundview") {
            (look.view, look.blended) = (v, true);
            changed = true;
        }

        let climates = d.tile().climates().climates.iter().map(|c| c.name.clone()).collect::<Vec<_>>();
        if look.blended && !climates.is_empty() {
            gui::separator();
            let mut ci = climates.iter().position(|c| *c == look.climate).unwrap_or(0) as i32;
            gui::set_next_item_width(200.0);
            if gui::combo("climate##ground", &mut ci, &climates.iter().map(String::as_str).collect::<Vec<_>>()) {
                look.climate = climates[ci as usize].clone();
                changed = true;
            }

            // The game's four battle seasons; a texture without the season's variant falls back to summer (fn 0x1180cb20).
            let seasons = ["sp", "su", "au", "wi"];
            let mut si = seasons.iter().position(|s| *s == look.season).unwrap_or(1) as i32;
            gui::set_next_item_width(200.0);
            if gui::combo("season##ground", &mut si, &["spring", "summer", "autumn", "winter"]) {
                look.season = seasons[si as usize].to_owned();
                changed = true;
            }
        }

        let water_view = look.blended && look.view == TerrainView::WaterDepth;
        if changed {
            d.tile_mut().set_terrain_look(&mut e.scene, look);
        }

        // The water depth view: the water ground types painted from the depth.
        if water_view {
            gui::separator();
            gui::set_next_item_width(70.0);
            gui::drag_float("shallow from (m)##water", &mut self.water_shallow_m, 0.05, 0.0, 20.0, "%.2f");
            if gui::is_item_hovered() {
                gui::set_tooltip("Water shallower than this is left unpainted (the water at a river's banks, which lies under the ground or barely over it).");
            }

            gui::same_line();
            gui::set_next_item_width(70.0);
            gui::drag_float("deep from (m)##water", &mut self.water_deep_m, 0.05, 0.0, 50.0, "%.2f");
            if gui::is_item_hovered() {
                gui::set_tooltip("Water at least this deep is painted deep water (units cannot cross it); shallower water is shallow water (units wade).");
            }

            self.water_deep_m = self.water_deep_m.max(self.water_shallow_m);
            if gui::button("Paint water ground types") {
                let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
                self.status = match d.paint_water_ground_types(self.water_shallow_m, self.water_deep_m) {
                    Ok(n) => format!("{n} ground type pixel(s) painted from the water depth"),
                    Err(why) => why,
                };
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(PAINT_WATER_TIP);
            }
        }

        gui::separator();
        self.draw_ground_layers(e, id);
    }

    /// The water, the sea, the procedural trees, the grass and the automatic no-go areas.
    fn draw_ground_layers(&mut self, e: &mut Editor, id: DocumentId) {
        let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
        let mut nogo = d.tile().display().show_auto_nogo;
        if gui::checkbox("automatic no-go areas", &mut nogo) {
            d.tile_mut().display_mut().show_auto_nogo = nogo;
            d.tile_mut().display_changed();
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(AUTO_NOGO_TIP);
        }

        if nogo {
            gui::same_line();
            match d.tile().auto_nogos() {
                Some(n) => gui::text_disabled(&format!("{} area(s)", n.len())),
                None => gui::text_disabled("being worked out"),
            }
        }

        let mut water = d.tile().display().show_water;
        if gui::checkbox("water (rivers)", &mut water) {
            d.tile_mut().display_mut().show_water = water;
            d.tile_mut().display_changed();
        }

        gui::same_line();
        let mut sea = d.tile().display().show_sea;
        if gui::checkbox("sea at 0 m", &mut sea) {
            self.set_show_sea(e, id, sea);
        }

        // The battle's wind picks the sea's waves and whitecaps.
        if let Some(r) = e.renderer() {
            let states = r.sea_states();
            if !states.is_empty() {
                let mut k = states.iter().position(|s| *s == r.settings.sea_state).unwrap_or(0) as i32;
                gui::set_next_item_width(170.0);
                if gui::combo("sea state (battle wind)", &mut k, &states.iter().map(String::as_str).collect::<Vec<_>>()) {
                    let key = states[k as usize].clone();
                    set_sea_state(e, &key);
                }
            }
        }

        let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");

        // BOB's procedural trees: one list per climate key, plus default; gen:<list> generates them from the seeds.
        let mut lists = vec!["none".to_owned()];
        lists.extend(d.tile().tree_lists().iter().cloned());
        let shown = d.tile().display().tree_list.clone();
        let generated = format!("{GENERATED_PREFIX}{shown}");
        let mut li = lists.iter().position(|l| *l == shown).or_else(|| lists.iter().position(|l| *l == generated)).unwrap_or(0) as i32;
        let items = lists.iter().map(String::as_str).collect::<Vec<_>>();
        gui::set_next_item_width(160.0);
        if gui::combo("procedural trees", &mut li, &items) {
            d.tile_mut().display_mut().tree_list = if li == 0 { String::new() } else { lists[li as usize].clone() };
        }

        if lists[li as usize].starts_with(GENERATED_PREFIX) {
            gui::same_line();
            if gui::button("regenerate") {
                d.tile_mut().reload_tree_list();
            }
        }

        let mut grass = d.tile().display().show_grass;
        if gui::checkbox("grass", &mut grass) {
            d.tile_mut().display_mut().show_grass = grass;
        }

        gui::same_line();
        let mut generated = d.tile().display().generate_grass;
        if gui::checkbox("generated##grass", &mut generated) {
            d.tile_mut().display_mut().generate_grass = generated;
        }

        if generated {
            gui::same_line();
            if gui::button("regenerate##grass") {
                d.tile_mut().reload_grass();
            }
        }
    }
}

/// A battle season's name.
pub(crate) fn season_name(season: &str) -> &'static str {
    match season {
        "sp" => "spring",
        "au" => "autumn",
        "wi" => "winter",
        _ => "summer",
    }
}

