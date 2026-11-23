//! The Create panel and its Space popup, as the Tile tool has them: everything that can be put in a scene, in one list searched by
//! words. Units (the game's cards, the chosen army's faction's first), effects, armies (by faction) and the camera. A click on a unit
//! or an effect starts placing it: clicks on the viewport's ground or the Layout put copies there (R / Shift+R turn it, Esc or a
//! right click stops); while placing, the Inspector shows what every copy takes (the army, men, width, facing). Space over the
//! viewport opens the same search at the mouse: what is picked is placed there.

use wce_cinematic::scene::{Army, Project, SceneUnit, TrackContent, VfxInstance};
use wce_editor::editor::Editor;
use wce_editor::gui::{self, v2};
use wce_editor::icons;

use crate::pickers::{Cards, GameLists, Item};
use crate::scene_ui::{add_track, edit};

pub const CATEGORIES: &[(&str, &str)] =
    &[("Units", icons::USERS), ("Effects", icons::SPARKLES), ("Armies", icons::FLAG), ("Camera", icons::CAMERA)];

/// What is being placed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Making {
    Unit(String),
    Vfx(String),
    /// A camera path: each click a key, a few seconds after the last.
    CameraPath,
}

/// What every copy placed takes (kept between placings, as the Tile tool's drafts).
#[derive(Clone, Debug)]
pub struct Draft {
    pub army: usize,
    /// Set to the unit's own (`main_units.num_men`) when one is picked; 0 takes its own too.
    pub men: u32,
    pub width: f32,
    /// Degrees from the army's facing.
    pub turn: f32,
    pub general: bool,
    pub vfx_length: f32,
    pub vfx_scale: f32,
    /// A camera path's keys: metres above the ground, seconds apart, field of view.
    pub cam_height: f32,
    pub cam_step: f32,
    pub cam_fov: f32,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            army: 0,
            men: 0,
            width: 30.0,
            turn: 0.0,
            general: false,
            vfx_length: 5.0,
            vfx_scale: 1.0,
            cam_height: 20.0,
            cam_step: 3.0,
            cam_fov: 35.0,
        }
    }
}

/// What the panel asks the tool for (it has the clip and the camera).
#[derive(Clone, Debug)]
pub enum CreateRequest {
    Placed(crate::items::SceneItem),
    /// The Camera tab's: a new camera where the viewport looks from, a key of the view at the playhead, recording the camera.
    NewCamera,
    KeyView,
    Record,
}

#[derive(Default)]
pub struct Create {
    pub filter: String,
    pub category: usize,
    pub list_view: bool,
    pub making: Option<Making>,
    pub draft: Draft,
    /// The ground under the mouse while placing (metres), from the viewport or the Layout.
    pub ghost: Option<[f32; 2]>,
    cards: Option<Cards>,
    cards_of: usize,
    /// The Space popup: asked for, its search, the row lit, where it places.
    quick_wanted: Option<[f32; 2]>,
    quick_at: Option<[f32; 2]>,
    quick_filter: String,
    quick_row: usize,
    pub requests: Vec<CreateRequest>,
    /// The unit the draft's men were set for.
    drafted: Option<String>,
}

fn words(filter: &str) -> Vec<String> {
    filter.to_lowercase().split_whitespace().map(str::to_owned).collect()
}

fn passes(i: &Item, words: &[String]) -> bool {
    let hay = format!("{} {}", i.name.to_lowercase(), i.key.to_lowercase());
    words.iter().all(|w| hay.contains(w.as_str()))
}

/// The side of a card cell, points.
const CARD_W: f32 = 66.0;
const CARD_H: f32 = 132.0;

impl Create {
    pub fn stop(&mut self) {
        self.making = None;
        self.ghost = None;
    }

    /// The pictures of units read from these packs (made again when the mount changes).
    pub fn set_files(&mut self, files: Option<&std::sync::Arc<dyn wce_vfs::files::Files>>, mount: usize) {
        if self.cards_of != mount || self.cards.is_none() {
            self.cards = files.map(|f| Cards::new(f.clone()));
            self.cards_of = mount;
        }
    }

    pub fn card(&mut self, e: &mut Editor, path: &str) -> Option<wce_editor::images::ImageRef> {
        self.cards.as_mut()?.get(&mut e.images, path)
    }

    /// A new unit being placed: the draft's men become its own.
    pub fn sync_draft(&mut self, lists: &GameLists) {
        if let Some(Making::Unit(k)) = &self.making
            && self.drafted.as_ref() != Some(k)
        {
            self.draft.men = lists.men.get(k).copied().unwrap_or(160).max(1);
            self.drafted = Some(k.clone());
        }
    }

    pub fn turn_by(&mut self, deg: f32) {
        self.draft.turn = (self.draft.turn + deg + 540.0).rem_euclid(360.0) - 180.0;
    }

    pub fn ask_quick_add(&mut self, at: [f32; 2]) {
        self.quick_wanted = Some(at);
    }

    /// The units listed for the draft's army: its faction's first.
    fn unit_rows(&self, lists: &GameLists, p: &Project, words: &[String]) -> (String, Vec<Item>, Vec<Item>) {
        let faction = p.armies.get(self.draft.army).map(|a| a.faction.clone()).unwrap_or_default();
        let own = lists.units_of_faction.get(&faction);
        let (mut mine, mut rest) = (vec![], vec![]);
        for i in lists.units.iter().filter(|i| passes(i, words)) {
            if own.is_some_and(|s| s.contains(&i.key)) {
                mine.push(i.clone());
            } else {
                rest.push(i.clone());
            }
        }
        (faction, mine, rest)
    }

    /// The panel. `start` draws the New / Open buttons when no scene is open.
    pub fn panel(&mut self, e: &mut Editor, lists: &GameLists, project: Option<&Project>, start: &mut dyn FnMut()) {
        gui::set_next_window_size(v2(340.0, 520.0), gui::COND_FIRST_USE_EVER);
        if !gui::begin("Create", None, 0) {
            gui::end();
            return;
        }
        let Some(p) = project else {
            start();
            gui::end();
            return;
        };
        if let Some(m) = &self.making {
            let what = match m {
                Making::Unit(k) => lists.unit_name(k),
                Making::Vfx(k) => k.clone(),
                Making::CameraPath => "a camera path".to_owned(),
            };
            gui::text_colored(wce_editor::theme::palette().highlight, &format!("{}  Placing {what}", icons::MOUSE_POINTER_2));
            if gui::is_item_hovered() {
                gui::set_tooltip(
                    "Click the viewport's ground or the Layout for each copy; R / Shift+R turn it; Esc or a right click stops",
                );
            }
            gui::same_line();
            if gui::small_button("Stop") {
                self.stop();
            }
        }
        gui::set_next_item_width(-1.0);
        let enter = gui::input_text(
            "##createfilter",
            Some(&format!("{}  search (Enter: the first; Space in the viewport)", icons::SEARCH)),
            &mut self.filter,
            128,
            gui::INPUT_ENTER_RETURNS_TRUE,
        );
        // The categories: a button each.
        for (c, (title, icon)) in CATEGORIES.iter().enumerate() {
            if c > 0 {
                gui::same_line();
            }
            let on = self.category == c;
            if on {
                gui::push_style_color(gui::COL_BUTTON, gui::style_color(gui::COL_BUTTON_ACTIVE));
            }
            if gui::button(&format!("{icon} {title}##cat{c}")) {
                self.category = c;
            }
            if on {
                gui::pop_style_color(1);
            }
        }
        let w = words(&self.filter);
        match self.category {
            0 => self.units_list(e, lists, p, &w, enter),
            1 => self.effects_list(lists, &w, enter),
            2 => self.armies_list(e, lists, p, &w, enter),
            _ => self.camera_list(),
        }
        gui::end();
    }

    fn units_list(&mut self, e: &mut Editor, lists: &GameLists, p: &Project, w: &[String], enter: bool) {
        // Which army the units go to, and grid or list.
        let armies: Vec<String> =
            p.armies.iter().enumerate().map(|(i, a)| format!("{}: {}", i + 1, lists.faction_name(&a.faction))).collect();
        let refs: Vec<&str> = armies.iter().map(String::as_str).collect();
        let mut army = self.draft.army.min(p.armies.len().saturating_sub(1)) as i32;
        gui::set_next_item_width(-36.0);
        if gui::combo("##createarmy", &mut army, &refs) {
            self.draft.army = army as usize;
        }
        if gui::is_item_hovered() {
            gui::set_tooltip("The army the units placed join; its faction's units are listed first");
        }
        gui::same_line();
        if gui::small_button(if self.list_view { icons::LAYOUT_GRID } else { icons::LIST_TREE }) {
            self.list_view = !self.list_view;
        }
        if gui::is_item_hovered() {
            gui::set_tooltip(if self.list_view { "Show the units' cards" } else { "Show the units as a list" });
        }
        let (faction, mine, rest) = self.unit_rows(lists, p, w);
        if enter && let Some(i) = mine.first().or(rest.first()) {
            self.making = Some(Making::Unit(i.key.clone()));
        }
        if lists.units.is_empty() {
            gui::text_disabled("Reading the game's units...");
            return;
        }
        gui::begin_child("##createunits", v2(0.0, 0.0), 0, 0);
        let filtering = !w.is_empty();
        let fname = lists.faction_name(&faction);
        if gui::tree_node_ex(&format!("{fname}'s units ({})##mine", mine.len()), gui::TREE_DEFAULT_OPEN) {
            self.unit_cells(e, lists, &faction, &mine, "m");
            gui::tree_pop();
        }
        let flags = if filtering || mine.is_empty() { gui::TREE_DEFAULT_OPEN } else { 0 };
        if gui::tree_node_ex(&format!("Every other unit ({})##rest", rest.len()), flags) {
            self.unit_cells(e, lists, &faction, &rest, "r");
            gui::tree_pop();
        }
        gui::end_child();
    }

    fn unit_cells(&mut self, e: &mut Editor, lists: &GameLists, faction: &str, rows: &[Item], uid: &str) {
        if self.list_view {
            gui::clipped(rows.len(), |k| {
                let i = &rows[k];
                let on = self.making == Some(Making::Unit(i.key.clone()));
                if gui::selectable(&format!("{}  {}##{uid}{k}", icons::USERS, i.name), on, 0) {
                    self.making = Some(Making::Unit(i.key.clone()));
                }
                if gui::is_item_hovered() {
                    unit_tip(lists, i);
                }
            });
            return;
        }
        let spacing = gui::item_spacing().x;
        let across = (((gui::content_region_avail().x + spacing) / (CARD_W + spacing)) as usize).max(1);
        let lines = rows.len().div_ceil(across);
        let line_h = CARD_H + gui::text_line_height_with_spacing() + gui::item_spacing().y;
        let mut clicked = None;
        let making = self.making.clone();
        let cards = &mut self.cards;
        let images = &mut e.images;
        gui::clipped_rows(lines, line_h, |l| {
            for (n, k) in (l * across..((l + 1) * across).min(rows.len())).enumerate() {
                if n > 0 {
                    gui::same_line();
                }
                let i = &rows[k];
                let at = gui::cursor_screen_pos();
                let on = making == Some(Making::Unit(i.key.clone()));
                if gui::selectable_sized(&format!("##card{uid}{k}"), on, 0, v2(CARD_W, CARD_H + gui::text_line_height())) {
                    clicked = Some(i.key.clone());
                }
                if gui::is_item_hovered() {
                    unit_tip(lists, i);
                }
                let dl = gui::window_draw_list();
                let (lo, hi) = (v2(at.x + 1.0, at.y + 1.0), v2(at.x + CARD_W - 1.0, at.y + CARD_H - 1.0));
                let pic = lists.card(&i.key, faction).and_then(|c| cards.as_mut()?.get(images, &c));
                match pic {
                    Some(img) if img.w > 0 && img.h > 0 => {
                        // Fitted in the cell, its proportions kept.
                        let k = ((hi.x - lo.x) / img.w as f32).min((hi.y - lo.y) / img.h as f32);
                        let (w, h) = (img.w as f32 * k, img.h as f32 * k);
                        let o = v2(lo.x + (hi.x - lo.x - w) * 0.5, lo.y + (hi.y - lo.y - h) * 0.5);
                        dl.image(img.id, o, v2(o.x + w, o.y + h), v2(0.0, 0.0), v2(1.0, 1.0), 0xFFFF_FFFF);
                    }
                    _ => {
                        dl.rect_filled(lo, hi, 0xFF26_2420, 3.0);
                        let sz = gui::calc_text_size(icons::USERS);
                        dl.text(v2(at.x + (CARD_W - sz.x) * 0.5, at.y + (CARD_H - sz.y) * 0.5), 0xFF80_8080, icons::USERS);
                    }
                }
                let mut name = i.name.clone();
                while name.chars().count() > 3 && gui::calc_text_width(&name) > CARD_W - 2.0 {
                    name.pop();
                    name.pop();
                    name.push('…');
                }
                dl.text(v2(at.x + 1.0, at.y + CARD_H), wce_editor::theme::u32(wce_editor::theme::text()), &name);
            }
        });
        if let Some(k) = clicked {
            self.making = Some(Making::Unit(k));
        }
    }

    fn effects_list(&mut self, lists: &GameLists, w: &[String], enter: bool) {
        let rows: Vec<&Item> = lists.vfx.iter().filter(|i| passes(i, w)).collect();
        if enter && let Some(i) = rows.first() {
            self.making = Some(Making::Vfx(i.key.clone()));
        }
        if lists.vfx.is_empty() {
            gui::text_disabled("No effects in the packs");
            return;
        }
        gui::begin_child("##createvfx", v2(0.0, 0.0), 0, 0);
        gui::clipped(rows.len(), |k| {
            let i = rows[k];
            let on = self.making == Some(Making::Vfx(i.key.clone()));
            if gui::selectable(&format!("{}  {}##vfx{k}", icons::SPARKLES, i.name), on, 0) {
                self.making = Some(Making::Vfx(i.key.clone()));
            }
        });
        gui::end_child();
    }

    fn armies_list(&mut self, e: &mut Editor, lists: &GameLists, p: &Project, w: &[String], enter: bool) {
        let rows: Vec<&Item> = lists.factions.iter().filter(|i| passes(i, w)).collect();
        let mut add = enter.then(|| rows.first().map(|i| i.key.clone())).flatten();
        gui::begin_child("##createarmies", v2(0.0, 0.0), 0, 0);
        gui::clipped(rows.len(), |k| {
            let i = rows[k];
            if gui::selectable(&format!("{}  {}##fac{k}", icons::FLAG, i.name), false, 0) {
                add = Some(i.key.clone());
            }
            if gui::is_item_hovered() {
                gui::set_tooltip(&format!("A new army of {} ({})", i.name, i.key));
            }
        });
        gui::end_child();
        if let Some(faction) = add {
            let n = p.armies.len();
            edit(e, "add army", |p| p.armies.push(new_army(p, &faction)));
            self.requests.push(CreateRequest::Placed(crate::items::SceneItem::Army(n)));
        }
    }

    /// The camera's ways: a new path by clicks, recording, a key of the view.
    fn camera_list(&mut self) {
        if gui::selectable(&format!("{}  New camera from this view", icons::PLUS), false, 0) {
            self.requests.push(CreateRequest::NewCamera);
        }
        if gui::is_item_hovered() {
            gui::set_tooltip("A camera object where the viewport looks from, keyed at the playhead (cut to it to show it)");
        }
        let on = self.making == Some(Making::CameraPath);
        if gui::selectable(&format!("{}  Camera path: click its points", icons::ROUTE), on, 0) {
            self.making = if on { None } else { Some(Making::CameraPath) };
        }
        if gui::is_item_hovered() {
            gui::set_tooltip("Each click on the ground a camera key, a few seconds after the last, looking on to the next (Esc stops)");
        }
        if gui::selectable(&format!("{}  Key the camera here", icons::CAMERA), false, 0) {
            self.requests.push(CreateRequest::KeyView);
        }
        if gui::is_item_hovered() {
            gui::set_tooltip("A camera key at the playhead holding the viewport's view");
        }
        if gui::selectable(&format!("{}  Record the camera", icons::CIRCLE_DOT_DASHED), false, 0) {
            self.requests.push(CreateRequest::Record);
        }
        if gui::is_item_hovered() {
            gui::set_tooltip("The clip plays and your camera moves are keyed every half second (again to stop)");
        }
    }

    /// A camera path's point clicked at `at` (metres, the ground's height there): a key `cam_height` above it, `cam_step` seconds
    /// after the clip camera's last (at the playhead for the first), looking on to the next one placed (the last looks ahead).
    pub fn path_point(
        &mut self,
        e: &mut Editor,
        p: &Project,
        clip: Option<u32>,
        time: f64,
        at: [f32; 3],
        ahead: [f32; 2],
        camera: Option<u32>,
    ) -> Option<crate::items::SceneItem> {
        let clip = clip?;
        let d = self.draft.clone();
        let pos = [at[0], at[1] + d.cam_height, at[2]];
        let mut made = None;
        edit(e, "camera path point", |p| {
            let track = camera.or_else(|| add_track(p, clip, "Camera", None, ""));
            let Some(track) = track else { return };
            crate::scene_ui::for_track(p, clip, track, |t| {
                if let TrackContent::Camera { keys } = &mut t.content {
                    let t_new = keys.last().map_or(time, |k| k.t + f64::from(d.cam_step));
                    // The last one looks on to this one; this one ahead, as the last did.
                    let target = [pos[0] + ahead[0] * 60.0, pos[1] - d.cam_height * 0.8, pos[2] + ahead[1] * 60.0];
                    if let Some(last) = keys.last_mut() {
                        last.target = [pos[0], pos[1] - d.cam_height * 0.8, pos[2]];
                    }
                    keys.push(wce_cinematic::scene::CameraKey {
                        t: t_new,
                        position: pos,
                        target,
                        fov: d.cam_fov,
                        interp: wce_cinematic::scene::Interp::Smooth,
                    });
                    made = Some((track, keys.len() - 1));
                }
            });
        });
        let _ = p;
        made.map(|(track, index)| crate::items::SceneItem::CameraKey { track, index })
    }

    /// The Space popup at the mouse: the search, the first matches of units and effects; Enter takes the lit one.
    pub fn quick_add(&mut self, e: &mut Editor, lists: &GameLists, p: &Project, clip: Option<u32>, time: f64) {
        if let Some(at) = self.quick_wanted.take() {
            self.quick_at = Some(at);
            self.quick_filter.clear();
            self.quick_row = 0;
            gui::open_popup("cinematic_quick_add");
        }
        if !gui::begin_popup("cinematic_quick_add") {
            return;
        }
        if gui::is_window_appearing() {
            gui::set_keyboard_focus_here(0);
        }
        gui::set_next_item_width(320.0);
        let enter =
            gui::input_text("##quick", Some("add here: a unit or an effect"), &mut self.quick_filter, 128, gui::INPUT_ENTER_RETURNS_TRUE);
        let w = words(&self.quick_filter);
        let (_, mine, rest) = self.unit_rows(lists, p, &w);
        let mut rows: Vec<(Making, String)> = mine
            .iter()
            .chain(rest.iter())
            .take(10)
            .map(|i| (Making::Unit(i.key.clone()), format!("{}  {}", icons::USERS, i.name)))
            .collect();
        rows.extend(
            lists
                .vfx
                .iter()
                .filter(|i| passes(i, &w))
                .take(5)
                .map(|i| (Making::Vfx(i.key.clone()), format!("{}  {}", icons::SPARKLES, i.key))),
        );
        if gui::is_key_pressed(gui::KEY_DOWN_ARROW, true) {
            self.quick_row = (self.quick_row + 1).min(rows.len().saturating_sub(1));
        }
        if gui::is_key_pressed(gui::KEY_UP_ARROW, true) {
            self.quick_row = self.quick_row.saturating_sub(1);
        }
        let mut picked = enter.then(|| rows.get(self.quick_row).map(|r| r.0.clone())).flatten();
        for (k, (m, label)) in rows.iter().enumerate() {
            if gui::selectable(&format!("{label}##q{k}"), k == self.quick_row, 0) {
                picked = Some(m.clone());
            }
        }
        if rows.is_empty() {
            gui::text_disabled("Nothing matches");
        }
        if let (Some(m), Some(at)) = (picked, self.quick_at) {
            let item = self.place(e, lists, p, &m, at, clip, time);
            self.requests.extend(item.map(CreateRequest::Placed));
            gui::close_current_popup();
        }
        gui::end_popup();
    }

    /// Puts one of `m` at `at` (metres): a unit joins the draft's army, an effect starts at the playhead in the clip's VFX track.
    #[allow(clippy::too_many_arguments)]
    pub fn place(
        &mut self,
        e: &mut Editor,
        lists: &GameLists,
        p: &Project,
        m: &Making,
        at: [f32; 2],
        clip: Option<u32>,
        time: f64,
    ) -> Option<crate::items::SceneItem> {
        match m {
            Making::Unit(key) => {
                let army = self.draft.army.min(p.armies.len().saturating_sub(1));
                let facing = p.armies.get(army).map_or(0.0, |a| a.deployment_orientation) + self.draft.turn.to_radians();
                let men = if self.draft.men > 0 { self.draft.men } else { lists.men.get(key).copied().unwrap_or(160).max(1) };
                let name = lists.unit_name(key);
                let d = self.draft.clone();
                let mut made = 0;
                edit(e, "place unit", |p| {
                    let id = p.new_id();
                    p.units.push(SceneUnit {
                        id,
                        name,
                        unit: key.clone(),
                        army,
                        start: at,
                        facing: crate::scene_doc::wrap_angle(facing),
                        width: d.width,
                        men,
                        seed: id,
                        general: d.general,
                    });
                    made = id;
                });
                Some(crate::items::SceneItem::Unit(made))
            }
            Making::CameraPath => None,
            Making::Vfx(key) => {
                let clip = clip?;
                let (len, scale) = (f64::from(self.draft.vfx_length), self.draft.vfx_scale);
                let turn = self.draft.turn.to_radians();
                let mut made = None;
                edit(e, "place effect", |p| {
                    let track = p
                        .clip(clip)
                        .and_then(|c| c.tracks.iter().find(|t| matches!(t.content, TrackContent::Vfx { .. })).map(|t| t.id))
                        .or_else(|| add_track(p, clip, "VFX", None, ""));
                    let Some(track) = track else { return };
                    let inst = VfxInstance {
                        start: time,
                        end: time + len,
                        vfx: key.clone(),
                        position: [at[0], 0.0, at[1]],
                        rotation_y: turn,
                        scale,
                    };
                    crate::scene_ui::for_track(p, clip, track, |t| {
                        if let TrackContent::Vfx { instances } = &mut t.content {
                            instances.push(inst.clone());
                        }
                    });
                    // Its index once sorted by start (as the document keeps them).
                    p.normalize();
                    made = p.clip(clip).and_then(|c| c.tracks.iter().find(|t| t.id == track)).and_then(|t| match &t.content {
                        TrackContent::Vfx { instances } => instances.iter().position(|v| *v == inst).map(|index| (track, index)),
                        _ => None,
                    });
                });
                made.map(|(track, index)| crate::items::SceneItem::Vfx { track, index })
            }
        }
    }
}

fn unit_tip(lists: &GameLists, i: &Item) {
    let men = lists.men.get(&i.key).map_or(String::new(), |m| format!(", {m} men"));
    let caste = lists.caste.get(&i.key).map_or(String::new(), |c| format!(", {c}"));
    gui::set_tooltip(&format!("{}  ({}{caste}{men})  ·  click, then click where it stands", i.name, i.key));
}

/// A new army of `faction`: on the side facing the player's, its zone beside the others of its side.
pub fn new_army(p: &Project, faction: &str) -> Army {
    let player_side = p.armies.iter().find(|a| a.player).map_or(0, |a| a.alliance);
    let side = if player_side == 0 { 1 } else { 0 };
    let same = p.armies.iter().filter(|a| a.alliance == side).count() as f32;
    let z = if side == 0 { -368.0 } else { 368.0 };
    Army {
        alliance: side,
        faction: faction.to_owned(),
        deployment_centre: [900.0 * same, z],
        deployment_size: [832.0, 352.0],
        deployment_orientation: if z > 0.0 { std::f32::consts::PI } else { 0.0 },
        player: false,
    }
}
