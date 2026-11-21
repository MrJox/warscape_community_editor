//! The Scene panel: the scene as a tree, laid out like the Tile tool's Scene panel. The battle (its setup), each army with its units,
//! the effects of the clip shown. A click selects (Ctrl toggles, Shift adds) and the Inspector shows it; a unit dragged onto another
//! army joins it; the eye hides a unit in the editor's preview (not in the exports); right click: the row's menu.

use wce_cinematic::scene::{Project, TrackContent};
use wce_editor::editor::Editor;
use wce_editor::gui::{self, v2};
use wce_editor::icons;

use crate::create::Create;
use crate::items::{self, How, SceneItem};
use crate::scene_ui::{SceneUi, edit, scene_doc};

const UNIT_PAYLOAD: &str = "CIN_UNIT";

fn how() -> How {
    let io = gui::io();
    if io.KeyCtrl {
        How::Toggle
    } else if io.KeyShift {
        How::Add
    } else {
        How::Set
    }
}

fn side_colour(alliance: u32) -> [f32; 4] {
    match alliance {
        0 => [0.36, 0.55, 0.9, 1.0],
        1 => [0.86, 0.36, 0.32, 1.0],
        2 => [0.86, 0.78, 0.36, 1.0],
        _ => [0.6, 0.8, 0.5, 1.0],
    }
}

/// The panel. `start` draws the New / Open buttons when no scene is open.
pub fn panel(e: &mut Editor, ui: &mut SceneUi, create: &mut Create) {
    gui::set_next_window_size(v2(320.0, 560.0), gui::COND_FIRST_USE_EVER);
    if !gui::begin("Scene", None, 0) {
        gui::end();
        return;
    }
    ui.outliner_hovered = gui::is_window_hovered_with_children();
    let Some(p) = scene_doc(e).map(|d| d.project().clone()) else {
        ui.start_buttons();
        gui::end();
        return;
    };
    let selected = items::selected(e);
    let is_sel = |i: SceneItem| selected.contains(&i);
    gui::set_next_item_width(-64.0);
    gui::input_text("##outlinerfilter", Some(&format!("{}  filter", icons::SEARCH)), &mut ui.outliner_filter, 64, 0);
    gui::same_line();
    if gui::button(&format!("{}##addmenu", icons::PLUS)) {
        gui::open_popup("cin_add_menu");
    }
    if gui::is_item_hovered() {
        gui::set_tooltip("Add a unit, an effect or an army (the Create panel)");
    }
    if gui::begin_popup("cin_add_menu") {
        for (c, (title, icon)) in crate::create::CATEGORIES.iter().enumerate() {
            if gui::menu_item(&format!("{icon}  {title}..."), None, false, true) {
                create.category = c;
                gui::set_window_focus("Create");
            }
        }
        gui::end_popup();
    }
    let filter = ui.outliner_filter.to_lowercase();
    let shows = |name: &str| filter.is_empty() || name.to_lowercase().contains(&filter);

    gui::begin_child("##outliner", v2(0.0, 0.0), 0, 0);
    let mut pending: Vec<Box<dyn FnOnce(&mut Project)>> = vec![];
    let mut select: Option<(SceneItem, How)> = None;

    // The battle.
    let flags = gui::TREE_LEAF | gui::TREE_SPAN_AVAIL_WIDTH | if is_sel(SceneItem::Battle) { gui::TREE_SELECTED } else { 0 };
    if gui::tree_node_ex_id("battle", flags, &format!("{}  Battle: {}", icons::CLAPPERBOARD, p.battle.key)) {
        gui::tree_pop();
    }
    if gui::is_item_clicked() {
        select = Some((SceneItem::Battle, How::Set));
    }
    if gui::is_item_hovered() {
        gui::set_tooltip("The battle's setup: map, weather, season, the menu's texts");
    }

    // The armies and their units.
    for (i, a) in p.armies.iter().enumerate() {
        let item = SceneItem::Army(i);
        let name = ui.lists.faction_name(&a.faction);
        let units: Vec<&wce_cinematic::scene::SceneUnit> = p.units.iter().filter(|u| u.army == i && shows(&u.name)).collect();
        let flags = gui::TREE_OPEN_ON_ARROW
            | gui::TREE_SPAN_AVAIL_WIDTH
            | gui::TREE_DEFAULT_OPEN
            | if is_sel(item) { gui::TREE_SELECTED } else { 0 };
        let label = format!(
            "{}  {}  ·  side {}{}  ({})",
            icons::FLAG,
            name,
            a.alliance + 1,
            if a.player { "  ★" } else { "" },
            p.units.iter().filter(|u| u.army == i).count()
        );
        let c = side_colour(a.alliance);
        gui::push_style_color(gui::COL_TEXT, gui::v4(c[0], c[1], c[2], c[3]));
        let open = gui::tree_node_ex_id(&format!("army{i}"), flags, &label);
        gui::pop_style_color(1);
        if gui::is_item_clicked() && !gui::is_item_toggled_open() {
            select = Some((item, how()));
        }
        if gui::begin_drag_drop_target() {
            if let Some((data, true)) = gui::accept_drag_drop_payload(UNIT_PAYLOAD) {
                let ids: Vec<u32> = data.iter().map(|&d| d as u32).collect();
                pending.push(Box::new(move |p: &mut Project| {
                    for u in p.units.iter_mut().filter(|u| ids.contains(&u.id)) {
                        u.army = i;
                    }
                }));
            }
            gui::end_drag_drop_target();
        }
        if gui::begin_popup_context_item(&format!("armymenu{i}")) {
            if gui::menu_item(&format!("{}  Place units...", icons::PLUS), None, false, true) {
                create.draft.army = i;
                create.category = 0;
                gui::set_window_focus("Create");
            }
            if gui::menu_item("Make it the player's", None, a.player, true) {
                pending.push(Box::new(move |p: &mut Project| {
                    for (k, x) in p.armies.iter_mut().enumerate() {
                        x.player = k == i;
                    }
                }));
            }
            gui::separator();
            if gui::menu_item(&format!("{}  Delete army", icons::TRASH_2), None, false, p.armies.len() > 1) {
                select = Some((item, How::Set));
                ui.request_delete = true;
            }
            gui::end_popup();
        }
        if open {
            for u in units {
                let ui_item = SceneItem::Unit(u.id);
                let hidden = ui.hidden.contains(&u.id);
                gui::push_id_int(u.id as i32);
                let w = gui::content_region_avail().x - gui::frame_height() - gui::item_spacing().x * 2.0;
                if hidden {
                    gui::push_style_color(gui::COL_TEXT, gui::style_color(gui::COL_TEXT_DISABLED));
                }
                let general = if u.general { "  (general)" } else { "" };
                let clicked = gui::selectable_sized(
                    &format!("{}  {}{general}  ·  {} men##u", icons::USERS, u.name, u.men),
                    is_sel(ui_item),
                    gui::SELECTABLE_ALLOW_DOUBLE_CLICK,
                    v2(w, 0.0),
                );
                if hidden {
                    gui::pop_style_color(1);
                }
                if clicked {
                    select = Some((ui_item, how()));
                    if gui::is_mouse_double_clicked(0) {
                        ui.look_at = Some(u.id);
                    }
                }
                if gui::is_item_hovered() {
                    gui::set_tooltip(&format!(
                        "{} ({})  ·  double-click: look at it; drag onto an army: move it there",
                        ui.lists.unit_name(&u.unit),
                        u.unit
                    ));
                }
                if gui::begin_drag_drop_source() {
                    // The selected units go together when this one is among them.
                    let ids: Vec<i32> = if is_sel(ui_item) {
                        selected.iter().filter_map(|s| if let SceneItem::Unit(id) = s { Some(*id as i32) } else { None }).collect()
                    } else {
                        vec![u.id as i32]
                    };
                    gui::set_drag_drop_payload(UNIT_PAYLOAD, &ids);
                    gui::text(&format!("{} unit(s): onto an army", ids.len()));
                    gui::end_drag_drop_source();
                }
                if gui::begin_popup_context_item("unitmenu") {
                    if !is_sel(ui_item) {
                        select = Some((ui_item, How::Set));
                    }
                    if gui::menu_item(
                        &format!("{}  Duplicate", icons::COPY),
                        wce_editor::shortcuts::label("cinematic.duplicate").as_deref(),
                        false,
                        true,
                    ) {
                        ui.request_duplicate = true;
                    }
                    if gui::menu_item(&format!("{}  Look at it", icons::FOCUS), None, false, true) {
                        ui.look_at = Some(u.id);
                    }
                    gui::separator();
                    if gui::menu_item(
                        &format!("{}  Delete", icons::TRASH_2),
                        wce_editor::shortcuts::label("cinematic.delete").as_deref(),
                        false,
                        true,
                    ) {
                        ui.request_delete = true;
                    }
                    gui::end_popup();
                }
                gui::same_line();
                if gui::small_button(if hidden { icons::EYE_OFF } else { icons::EYE }) {
                    if hidden {
                        ui.hidden.remove(&u.id);
                    } else {
                        ui.hidden.insert(u.id);
                    }
                }
                if gui::is_item_hovered() {
                    gui::set_tooltip(if hidden { "Show it in the editor" } else { "Hide it in the editor (the exports keep it)" });
                }
                gui::pop_id();
            }
            if p.units.iter().all(|u| u.army != i) {
                if gui::small_button(&format!("{}  Place units...##place{i}", icons::PLUS)) {
                    create.draft.army = i;
                    create.category = 0;
                    gui::set_window_focus("Create");
                }
                if gui::is_item_hovered() {
                    gui::set_tooltip("Pick a unit in the Create panel, then click where it stands");
                }
            }
            gui::tree_pop();
        }
    }

    // The camera keys of the clip shown.
    if let Some(clip) = ui.current_clip(&p) {
        let keys: Vec<(u32, usize, f64)> = clip
            .tracks
            .iter()
            .flat_map(|t| match &t.content {
                TrackContent::Camera { keys } => keys.iter().enumerate().map(|(k, c)| (t.id, k, c.t)).collect::<Vec<_>>(),
                _ => vec![],
            })
            .collect();
        if gui::tree_node_ex_id(
            "camera",
            gui::TREE_SPAN_AVAIL_WIDTH,
            &format!("{}  Camera in {} ({})", icons::CAMERA, clip.name, keys.len()),
        ) {
            for (track, index, t) in keys {
                let item = SceneItem::CameraKey { track, index };
                if gui::selectable(&format!("{}  at {t:.2} s##c{track}_{index}", icons::CAMERA), is_sel(item), 0) {
                    select = Some((item, how()));
                    ui.time = t;
                }
            }
            gui::tree_pop();
        }
    }
    // The effects of the clip shown.
    if let Some(clip) = ui.current_clip(&p) {
        let effects: Vec<(u32, usize, String)> = clip
            .tracks
            .iter()
            .flat_map(|t| match &t.content {
                TrackContent::Vfx { instances } => instances
                    .iter()
                    .enumerate()
                    .map(|(k, v)| {
                        (t.id, k, format!("{} ({:.1} to {:.1} s)", if v.vfx.is_empty() { "(none)" } else { &v.vfx }, v.start, v.end))
                    })
                    .collect::<Vec<_>>(),
                _ => vec![],
            })
            .filter(|x| shows(&x.2))
            .collect();
        if gui::tree_node_ex_id(
            "effects",
            gui::TREE_SPAN_AVAIL_WIDTH,
            &format!("{}  Effects in {} ({})", icons::SPARKLES, clip.name, effects.len()),
        ) {
            for (track, index, label) in effects {
                let item = SceneItem::Vfx { track, index };
                if gui::selectable(&format!("{}  {label}##v{track}_{index}", icons::SPARKLES), is_sel(item), 0) {
                    select = Some((item, how()));
                }
            }
            gui::tree_pop();
        }
    }
    gui::end_child();
    gui::end();

    if let Some((item, how)) = select {
        items::select(e, item, how);
        ui.key_focus = false;
    }
    for f in pending {
        edit(e, "scene", f);
    }
}
