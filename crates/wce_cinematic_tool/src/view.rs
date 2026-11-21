//! The scene in the 3D viewport, as the Tile tool works there: a click selects a unit or an effect (Ctrl toggles, Shift adds), the
//! editor's gizmo moves and turns the selection (a unit with a path is keyed at the playhead), a left drag in Select mode draws a
//! box; while the Create panel places, the ground under the mouse shows where it goes and a click puts it there. Over the picture:
//! each unit's formation on the ground in its side's colour, the selected ones lit with their names and their path in the clip.

use wce_cinematic::scene::{Project, SceneUnit, TrackContent};
use wce_editor::editor::Editor;
use wce_editor::gui::{self, v2};
use wce_editor::tool::{ViewportEvent, ViewportEventKind};
use wce_foundation::math::Float3;

use crate::create::{Create, CreateRequest, Making};
use crate::ground::Ground;
use crate::items::{self, How, SceneItem};
use crate::scene_doc::DocView;
use crate::scene_ui::{SceneUi, scene_doc};

#[derive(Default)]
pub struct SceneView {
    /// The game's fonts (the subtitle's).
    fonts: wce_ui_model::fonts::FontLibrary,
    /// The ground under the mouse (metres), for the quick-add popup.
    pub hover_ground: Option<[f32; 2]>,
    /// The item under the mouse (its name shows).
    hovered: Option<SceneItem>,
    /// The right click's menu, for this item.
    menu: Option<SceneItem>,
    menu_open: bool,
    /// The ground where the menu was asked for.
    menu_at: Option<[f32; 2]>,
    /// The box being drawn (image pixels).
    boxed: Option<((i32, i32), (i32, i32))>,
}

/// The ground under a pixel of the image, metres: the drawn terrain, else the plane at 0.
pub fn ground_point(e: &mut Editor, x: i32, y: i32) -> Option<[f32; 2]> {
    let units = e.scene.game_units.max(1e-3);
    let Editor { viewport, scene, .. } = e;
    if let Some(p) = viewport.ground_under(scene, x, y) {
        return Some([p.x * units, p.z * units]);
    }
    let (o, d) = e.viewport.ray(x, y)?;
    if d.y.abs() < 1e-6 {
        return None;
    }
    let t = -o.y / d.y;
    (t > 0.0).then_some([(o.x + d.x * t) * units, (o.z + d.z * t) * units])
}

/// A unit's formation on the ground: its four corners (metres) from its men and width (ranks 1.4 m apart, files 1.1 m).
pub fn footprint(u: &SceneUnit, at: [f32; 2], facing: f32) -> [[f32; 2]; 4] {
    let files = (u.width / 1.1).max(1.0);
    let depth = ((u.men as f32 / files).ceil() * 1.4).max(3.0);
    let (sin, cos) = facing.sin_cos();
    let (hw, hd) = (u.width * 0.5, depth * 0.5);
    let c = |x: f32, z: f32| [at[0] + x * cos + z * sin, at[1] - x * sin + z * cos];
    [c(-hw, -hd), c(hw, -hd), c(hw, hd), c(-hw, hd)]
}

fn inside(poly: &[[f32; 2]; 4], p: [f32; 2]) -> bool {
    let mut sign = 0.0f32;
    for k in 0..4 {
        let (a, b) = (poly[k], poly[(k + 1) % 4]);
        let cross = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
        if cross.abs() < 1e-9 {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
}

fn place_of(view: &DocView, u: &SceneUnit) -> ([f32; 2], f32) {
    view.placed.get(&u.id).copied().unwrap_or((u.start, u.facing))
}

/// Where a point on the ground is in the image (pixels).
fn to_image(e: &Editor, m: [f32; 3]) -> Option<(f32, f32)> {
    let units = e.scene.game_units.max(1e-3);
    e.viewport.project(Float3::new(m[0] / units, m[1] / units, m[2] / units))
}

/// The effects alive at the playhead: their items and places.
fn effects(p: &Project, ui: &SceneUi, g: &Ground) -> Vec<(SceneItem, [f32; 3], String)> {
    let Some(clip) = ui.clip.and_then(|c| p.clip(c)) else { return vec![] };
    clip.tracks
        .iter()
        .flat_map(|t| match &t.content {
            TrackContent::Vfx { instances } => instances
                .iter()
                .enumerate()
                .filter(|(_, v)| ui.time >= v.start - 1e-6 && ui.time <= v.end + 1e-6)
                .map(|(k, v)| {
                    let at = [v.position[0], v.position[1] + g.height(v.position[0], v.position[2]), v.position[2]];
                    (SceneItem::Vfx { track: t.id, index: k }, at, v.vfx.clone())
                })
                .collect::<Vec<_>>(),
            _ => vec![],
        })
        .collect()
}

/// The camera keys of the clip shown (drawn while the viewport's camera is free): their items, places and targets.
fn camera_keys(p: &Project, ui: &SceneUi) -> Vec<(SceneItem, [f32; 3], [f32; 3], f64)> {
    if ui.follows_keys() {
        return vec![];
    }
    let Some(clip) = ui.clip.and_then(|c| p.clip(c)) else { return vec![] };
    clip.tracks
        .iter()
        .flat_map(|t| match &t.content {
            TrackContent::Camera { keys } => keys
                .iter()
                .enumerate()
                .map(|(k, c)| (SceneItem::CameraKey { track: t.id, index: k }, c.position, c.target, c.t))
                .collect::<Vec<_>>(),
            _ => vec![],
        })
        .collect()
}

/// The item under a pixel: an effect's or a camera key's marker within 12 pixels, else the unit whose formation the ground point is in.
fn pick(e: &mut Editor, p: &Project, ui: &SceneUi, view: &DocView, g: &Ground, x: i32, y: i32) -> Option<SceneItem> {
    let near = effects(p, ui, g)
        .into_iter()
        .map(|(i, at, _)| (i, at))
        .chain(camera_keys(p, ui).into_iter().flat_map(|(i, at, target, _)| {
            let SceneItem::CameraKey { track, index } = i else { return vec![(i, at)] };
            vec![(i, at), (SceneItem::CameraTarget { track, index }, target)]
        }))
        .filter_map(|(i, at)| to_image(e, at).map(|(px, py)| (i, (px - x as f32).hypot(py - y as f32))))
        .filter(|(_, d)| *d <= 12.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i);
    if near.is_some() {
        return near;
    }
    let g = ground_point(e, x, y)?;
    p.units
        .iter()
        .filter(|u| !ui.hidden.contains(&u.id))
        .filter(|u| {
            let (at, f) = place_of(view, u);
            inside(&footprint(u, at, f), g) || (at[0] - g[0]).hypot(at[1] - g[1]) < 3.0
        })
        .min_by(|a, b| {
            let (pa, pb) = (place_of(view, a).0, place_of(view, b).0);
            (pa[0] - g[0]).hypot(pa[1] - g[1]).total_cmp(&(pb[0] - g[0]).hypot(pb[1] - g[1]))
        })
        .map(|u| SceneItem::Unit(u.id))
}

impl SceneView {
    /// A subtitle as the game draws a cutscene's: its `subtitle` font category (`ui/fontcategories.fc`: la_gioconda 22, white, no
    /// shadow) through the UI tool's fonts (`.cuf`), at the game's size for a 1080-line screen scaled to the viewport, centred at the
    /// bottom (in the bottom bar when the bars show). The editor's font when the game's can't be read.
    fn subtitle(&mut self, e: &mut Editor, dl: &gui::DrawList, text: &str, origin: gui::Vec2, size: gui::Vec2, bar: f32) {
        let k = size.y / 1080.0;
        let resolved = e.vfs().cloned().and_then(|v| self.fonts.resolve(&v, "", 0, "subtitle")).filter(|r| r.cuf.glyph_count() < 5000);
        let Some(r) = resolved else {
            let px = (22.0 * k).max(8.0);
            let tw = gui::calc_text_width(text) * px / gui::text_line_height().max(1.0);
            let y = if bar >= px * 1.2 { origin.y + size.y - bar * 0.5 - px * 0.5 } else { origin.y + size.y - px * 2.5 };
            dl.text_sized(px, v2(origin.x + (size.x - tw) * 0.5, y), 0xFFFF_FFFF, text);
            return;
        };
        let akey = format!("font:{}", r.key);
        if !e.images.knows(&akey) {
            let at = r.cuf.atlas();
            e.images.submit(&akey, at.w, at.h, at.rgba);
        }
        let Some(tex) = e.images.get(&akey) else { return };
        let c = r.color.unwrap_or([255; 4]);
        let col = gui::col32(c[0], c[1], c[2], c[3]);
        let lines = r.cuf.wrap(text, (1080.0 * size.x / size.y.max(1.0) * 0.8) as i32);
        let lh = r.cuf.line_height() as f32;
        let block = lh * lines.len() as f32 * k;
        let top = if bar >= block * 1.1 { origin.y + size.y - bar * 0.5 - block * 0.5 } else { origin.y + size.y - block - size.y * 0.06 };
        let origins = r.cuf.atlas_origins();
        let (tw, th) = (tex.w as f32, tex.h as f32);
        for (i, line) in lines.iter().enumerate() {
            let x0 = origin.x + (size.x - r.cuf.line_width(line) as f32 * k) * 0.5;
            let mut placed = vec![];
            r.cuf.place_line(line, 0, 0, &mut placed);
            for g in placed {
                let m = r.cuf.metric(g.glyph);
                let (ox, oy) = origins[g.glyph];
                let tl = v2(x0 + g.x as f32 * k, top + (i as f32 * lh + g.y as f32) * k);
                let br = v2(tl.x + f32::from(m.width) * k, tl.y + f32::from(m.height) * k);
                dl.image(
                    tex.id,
                    tl,
                    br,
                    v2(ox as f32 / tw, oy as f32 / th),
                    v2((ox + u32::from(m.width)) as f32 / tw, (oy + u32::from(m.height)) as f32 / th),
                    col,
                );
            }
        }
    }

    /// The viewport's mouse: true when the scene took it.
    pub fn input(&mut self, e: &mut Editor, ui: &mut SceneUi, create: &mut Create, ground: &Ground, ev: &ViewportEvent) -> bool {
        let Some((p, view)) = scene_doc(e).map(|d| (d.project().clone(), d.view.clone())) else { return false };
        match ev.kind {
            ViewportEventKind::Hover => {
                if create.making.is_some() {
                    create.ghost = ground_point(e, ev.x, ev.y);
                    self.hovered = None;
                } else {
                    self.hovered = pick(e, &p, ui, &view, ground, ev.x, ev.y);
                }
                false
            }
            // While placing the tool owns the left button: the editor sends its press (not a click), which puts one there.
            ViewportEventKind::Press => {
                let Some(m) = create.making.clone() else { return false };
                if m == Making::CameraPath {
                    if let Some(at) = ground_point(e, ev.x, ev.y) {
                        let cam = e.viewport.camera();
                        let (dx, dz) = (cam.target.x - cam.eye.x, cam.target.z - cam.eye.z);
                        let len = dx.hypot(dz).max(1e-6);
                        let h = ground.height(at[0], at[1]);
                        if let Some(item) = create.path_point(e, &p, ui.clip, ui.time, [at[0], h, at[1]], [dx / len, dz / len]) {
                            create.requests.push(CreateRequest::Placed(item));
                        }
                    }
                    return true;
                }
                if let Some(at) = ground_point(e, ev.x, ev.y) {
                    let lists = ui.lists.clone();
                    if let Some(item) = create.place(e, &lists, &p, &m, at, ui.clip, ui.time) {
                        create.requests.push(CreateRequest::Placed(item));
                    }
                }
                true
            }
            ViewportEventKind::Drag | ViewportEventKind::Release => create.making.is_some(),
            ViewportEventKind::Click if ev.button == 0 => {
                if create.making.is_some() {
                    return true;
                }
                let how = if ev.ctrl {
                    How::Toggle
                } else if ev.shift {
                    How::Add
                } else {
                    How::Set
                };
                match pick(e, &p, ui, &view, ground, ev.x, ev.y) {
                    Some(i) => items::select(e, i, how),
                    None if how == How::Set => e.selection.clear(),
                    None => {}
                }
                ui.key_focus = false;
                true
            }
            ViewportEventKind::ContextMenu => {
                if create.making.is_some() {
                    create.stop();
                    return true;
                }
                let hit = pick(e, &p, ui, &view, ground, ev.x, ev.y);
                if let Some(i) = hit
                    && !items::selected(e).contains(&i)
                {
                    items::select(e, i, How::Set);
                }
                self.menu = hit;
                self.menu_at = ground_point(e, ev.x, ev.y);
                self.menu_open = true;
                true
            }
            _ => false,
        }
    }

    /// The box select of a left drag in Select mode: the units whose place is inside (Shift adds, Ctrl takes away).
    pub fn box_drag(&mut self, e: &mut Editor, ground: &Ground, from: (i32, i32), to: (i32, i32), done: bool) {
        self.boxed = (!done).then_some((from, to));
        if !done {
            return;
        }
        let Some((p, view)) = scene_doc(e).map(|d| (d.project().clone(), d.view.clone())) else { return };
        let (x0, x1) = (from.0.min(to.0) as f32, from.0.max(to.0) as f32);
        let (y0, y1) = (from.1.min(to.1) as f32, from.1.max(to.1) as f32);
        let inside: Vec<SceneItem> = p
            .units
            .iter()
            .filter(|u| {
                let (at, _) = place_of(&view, u);
                to_image(e, [at[0], ground.height(at[0], at[1]), at[1]]).is_some_and(|(x, y)| x >= x0 && x <= x1 && y >= y0 && y <= y1)
            })
            .map(|u| SceneItem::Unit(u.id))
            .collect();
        let io = gui::io();
        if !io.KeyShift && !io.KeyCtrl {
            e.selection.clear();
        }
        for i in inside {
            items::select(e, i, if io.KeyCtrl { How::Toggle } else { How::Add });
        }
    }

    /// Over the picture: formations, names, paths, the placing ghost, the box, the right click's menu.
    pub fn overlay(&mut self, e: &mut Editor, ui: &mut SceneUi, create: &mut Create, ground: &Ground, origin: gui::Vec2, size: gui::Vec2) {
        let Some((p, view)) = scene_doc(e).map(|d| (d.project().clone(), d.view.clone())) else { return };
        if e.viewport.game_view {
            return;
        }
        let (w, h) = (e.viewport.width().max(1) as f32, e.viewport.height().max(1) as f32);
        let k = size.x / w;
        let scr = |e: &Editor, m: [f32; 3]| to_image(e, m).map(|(x, y)| v2(origin.x + x * k, origin.y + y * k));
        let _ = h;
        let selected = items::selected(e);
        let dl = gui::window_draw_list();
        dl.push_clip_rect(origin, v2(origin.x + size.x, origin.y + size.y));
        let side = |a: usize| match p.armies.get(a).map_or(0, |x| x.alliance) {
            0 => (90u8, 140u8, 230u8),
            1 => (220, 90, 80),
            _ => (220, 200, 90),
        };
        for u in &p.units {
            if ui.hidden.contains(&u.id) {
                continue;
            }
            let (at, f) = place_of(&view, u);
            let corners = footprint(u, at, f);
            let pts: Vec<gui::Vec2> = corners.iter().filter_map(|c| scr(e, [c[0], ground.height(c[0], c[1]), c[1]])).collect();
            if pts.len() < 4 {
                continue;
            }
            let item = SceneItem::Unit(u.id);
            let sel = selected.contains(&item);
            let hot = self.hovered == Some(item);
            let (r, g, b) = side(u.army);
            let col = if sel { gui::col32(255, 230, 120, 255) } else { gui::col32(r, g, b, if hot { 255 } else { 150 }) };
            dl.polyline(&pts, col, true, if sel || hot { 2.0 } else { 1.0 });
            // The front.
            let (sin, cos) = f.sin_cos();
            let _ = cos;
            let front = [(corners[2][0] + corners[3][0]) * 0.5, (corners[2][1] + corners[3][1]) * 0.5];
            if let (Some(a), Some(b)) = (
                scr(e, [front[0], ground.height(front[0], front[1]), front[1]]),
                scr(e, [front[0] + 6.0 * sin, ground.height(front[0], front[1]), front[1] + 6.0 * cos]),
            ) {
                dl.line(a, b, col, 1.5);
            }
            if (sel || hot)
                && let Some(c) = scr(e, [at[0], ground.height(at[0], at[1]) + 2.0, at[1]])
            {
                let label = format!("{}  ·  {} men", u.name, u.men);
                let tw = gui::calc_text_width(&label);
                dl.rect_filled(v2(c.x - tw * 0.5 - 4.0, c.y - 22.0), v2(c.x + tw * 0.5 + 4.0, c.y - 6.0), 0xC018_1614, 3.0);
                dl.text(v2(c.x - tw * 0.5, c.y - 21.0), 0xFFFF_FFFF, &label);
            }
        }
        // The selected units' paths in the clip: the keys as dots, the playhead's place lit.
        if let Some(clip) = ui.clip.and_then(|c| p.clip(c)) {
            for t in &clip.tracks {
                if let TrackContent::Unit { unit, path, .. } = &t.content
                    && selected.contains(&SceneItem::Unit(*unit))
                {
                    let pts: Vec<gui::Vec2> = path
                        .iter()
                        .filter_map(|k| scr(e, [k.position[0], ground.height(k.position[0], k.position[1]), k.position[1]]))
                        .collect();
                    dl.polyline(&pts, gui::col32(255, 230, 120, 170), false, 1.5);
                    for (q, key) in pts.iter().zip(path) {
                        let on = (key.t - ui.time).abs() < 1e-3;
                        dl.circle_filled(
                            *q,
                            if on { 5.0 } else { 3.5 },
                            if on { gui::col32(255, 120, 80, 255) } else { gui::col32(255, 230, 120, 255) },
                        );
                    }
                }
            }
        }
        // The effects alive at the playhead.
        for (item, at, name) in effects(&p, ui, ground) {
            let Some(c) = scr(e, at) else { continue };
            let sel = selected.contains(&item);
            let col = if sel { gui::col32(255, 230, 120, 255) } else { gui::col32(200, 160, 255, 220) };
            let r = if sel { 7.0 } else { 5.0 };
            dl.triangle_filled(v2(c.x, c.y - r), v2(c.x + r, c.y), v2(c.x, c.y + r), col);
            dl.triangle_filled(v2(c.x, c.y - r), v2(c.x - r, c.y), v2(c.x, c.y + r), col);
            if sel || self.hovered == Some(item) {
                dl.text(v2(c.x + 9.0, c.y - 8.0), 0xFFFF_FFFF, &name);
            }
        }
        // The camera keys (the camera free): where each is, what it looks at, the path between them.
        let cams = camera_keys(&p, ui);
        // The path the camera takes (the keys' curve, sampled).
        if cams.len() > 1
            && let Some(clip) = ui.clip.and_then(|c| p.clip(c))
        {
            let (t0, t1) = (cams.first().map_or(0.0, |c| c.3), cams.last().map_or(0.0, |c| c.3));
            let n = 96;
            // Red where the battle camera can't go (outside 2 to 90 m over the ground: the game holds it at the limit).
            let (lo, hi) = crate::preview::GAME_CAMERA_HEIGHT;
            let pts: Vec<(gui::Vec2, bool)> = (0..=n)
                .filter_map(|i| {
                    let t = t0 + (t1 - t0) * f64::from(i) / f64::from(n);
                    let f = wce_cinematic::eval::evaluate(&p, clip, &wce_cinematic::eval::SpeedTable::default(), t);
                    f.camera.and_then(|c| {
                        // Not where the ground is still on its way (its height there is not known yet).
                        let over = ground.height_known(c.position[0], c.position[2]).map(|g| c.position[1] - g);
                        scr(e, c.position).map(|q| (q, over.is_some_and(|o| o < lo - 0.01 || o > hi + 0.01)))
                    })
                })
                .collect();
            for w in pts.windows(2) {
                let col = if w[0].1 || w[1].1 { gui::col32(255, 90, 80, 230) } else { gui::col32(120, 220, 255, 170) };
                dl.line(w[0].0, w[1].0, col, 2.0);
            }
            if pts.iter().any(|q| q.1)
                && let Some(q) = pts.iter().find(|q| q.1)
            {
                dl.text(v2(q.0.x + 8.0, q.0.y + 6.0), gui::col32(255, 120, 110, 255), "outside the battle camera's 2 to 90 m");
            }
        }
        for (item, at, target, t) in &cams {
            let Some(c) = scr(e, *at) else { continue };
            let sel = selected.contains(item);
            let col = if sel { gui::col32(255, 230, 120, 255) } else { gui::col32(120, 220, 255, 230) };
            if let Some(tg) = scr(e, *target) {
                dl.line(c, tg, col & 0x80FF_FFFF, 1.0);
                let SceneItem::CameraKey { track, index } = *item else { continue };
                let tsel = selected.contains(&SceneItem::CameraTarget { track, index });
                let tc = if tsel { gui::col32(255, 230, 120, 255) } else { gui::col32(255, 150, 120, 220) };
                dl.circle_filled(tg, if tsel { 5.5 } else { 4.0 }, tc);
            }
            dl.rect_filled(v2(c.x - 6.0, c.y - 4.0), v2(c.x + 6.0, c.y + 4.0), col, 2.0);
            if sel || self.hovered == Some(*item) {
                dl.text(v2(c.x + 9.0, c.y - 8.0), 0xFFFF_FFFF, &format!("camera at {t:.2} s"));
            }
        }
        // While placing: where it goes, and how to.
        if let Some(m) = &create.making {
            if let Some(g) = create.ghost {
                match m {
                    Making::Unit(key) => {
                        let army = create.draft.army.min(p.armies.len().saturating_sub(1));
                        let f = p.armies.get(army).map_or(0.0, |a| a.deployment_orientation) + create.draft.turn.to_radians();
                        let men = if create.draft.men > 0 { create.draft.men } else { ui.lists.men.get(key).copied().unwrap_or(160) };
                        let ghost = SceneUnit {
                            id: 0,
                            name: String::new(),
                            unit: key.clone(),
                            army,
                            start: g,
                            facing: f,
                            width: create.draft.width,
                            men,
                            seed: 0,
                            general: false,
                        };
                        let corners = footprint(&ghost, g, f);
                        let pts: Vec<gui::Vec2> = corners.iter().filter_map(|c| scr(e, [c[0], ground.height(c[0], c[1]), c[1]])).collect();
                        if pts.len() == 4 {
                            dl.concave_poly_filled(&pts, gui::col32(255, 230, 120, 60));
                            dl.polyline(&pts, gui::col32(255, 230, 120, 230), true, 2.0);
                        }
                        let (sin, cos) = f.sin_cos();
                        if let (Some(a), Some(b)) = (
                            scr(e, [g[0], ground.height(g[0], g[1]), g[1]]),
                            scr(e, [g[0] + 14.0 * sin, ground.height(g[0], g[1]), g[1] + 14.0 * cos]),
                        ) {
                            dl.line(a, b, gui::col32(255, 230, 120, 230), 2.0);
                        }
                    }
                    Making::CameraPath => {
                        let h = ground.height(g[0], g[1]);
                        if let (Some(a), Some(b)) = (scr(e, [g[0], h, g[1]]), scr(e, [g[0], h + create.draft.cam_height, g[1]])) {
                            dl.line(a, b, gui::col32(120, 220, 255, 220), 1.5);
                            dl.rect_filled(v2(b.x - 6.0, b.y - 4.0), v2(b.x + 6.0, b.y + 4.0), gui::col32(120, 220, 255, 230), 2.0);
                        }
                    }
                    Making::Vfx(_) => {
                        if let Some(c) = scr(e, [g[0], ground.height(g[0], g[1]), g[1]]) {
                            dl.circle_filled(c, 8.0, gui::col32(200, 160, 255, 120));
                        }
                    }
                }
            }
            let what = match m {
                Making::Unit(k) => ui.lists.unit_name(k),
                Making::Vfx(k) => k.clone(),
                Making::CameraPath => "a camera path".to_owned(),
            };
            let hint = format!("Placing {what}  ·  click: put one  ·  R / Shift+R: turn  ·  Esc or right click: stop");
            let tw = gui::calc_text_width(&hint);
            let at = v2(origin.x + (size.x - tw) * 0.5, origin.y + 48.0);
            dl.rect_filled(v2(at.x - 10.0, at.y - 5.0), v2(at.x + tw + 10.0, at.y + gui::text_line_height() + 5.0), 0xD018_1614, 6.0);
            dl.text(at, wce_editor::theme::u32(wce_editor::theme::palette().highlight), &hint);
        }
        if let Some((a, b)) = self.boxed {
            let (pa, pb) =
                (v2(origin.x + a.0 as f32 * k, origin.y + a.1 as f32 * k), v2(origin.x + b.0 as f32 * k, origin.y + b.1 as f32 * k));
            dl.rect_filled(pa, pb, gui::col32(255, 230, 120, 30), 0.0);
            dl.rect(pa, pb, gui::col32(255, 230, 120, 200), 0.0, 1.0);
        }
        // What the movie lays over its pictures, as the playhead has it: the fade, the cinematic bars, the subtitle.
        // Only through the camera (playing, or looking through it): flying free, the scene is not faded nor barred.
        if let Some(f) = ui.frame(&p).filter(|_| ui.follows_keys()) {
            let (lo, hi) = (origin, v2(origin.x + size.x, origin.y + size.y));
            let fade = f.fade.clamp(0.0, 1.0);
            if fade > 0.0 {
                dl.rect_filled(lo, hi, gui::col32(0, 0, 0, (fade * 255.0) as u8), 0.0);
            }
            // As the movie draws them: an eighth of the picture's height each at full strength.
            let bar = f.letterbox.clamp(0.0, 1.0) * size.y / 8.0;
            if bar > 0.5 {
                dl.rect_filled(lo, v2(hi.x, lo.y + bar), 0xFF00_0000, 0.0);
                dl.rect_filled(v2(lo.x, hi.y - bar), hi, 0xFF00_0000, 0.0);
            }
            if let Some(text) = f.subtitle.as_ref().and_then(|id| p.text(id, ui.lang())).filter(|t| !t.is_empty()) {
                self.subtitle(e, &dl, text, origin, size, bar);
            }
        }
        dl.pop_clip_rect();

        // The scene's chips at the top left: play, the free camera, a camera key of this view, the battle preview.
        gui::set_cursor_screen_pos(v2(origin.x + 8.0, origin.y + 6.0));
        gui::push_style_color(gui::COL_CHILD_BG, gui::v4(0.08, 0.08, 0.09, 0.78));
        if gui::begin_pill("##cinematicchips") {
            use wce_editor::chips::chip;
            use wce_editor::icons;
            if chip(
                ui.playing,
                if ui.playing { icons::PAUSE } else { icons::PLAY },
                "cinplay",
                "Play / pause the clip (from its start at the end)",
            ) {
                let length = ui.clip.and_then(|c| p.clip(c)).map_or(0.0, |c| c.length);
                ui.toggle_play(length);
            }
            gui::same_line_touching();
            if chip(
                ui.look_through,
                icons::FOCUS,
                "cinlook",
                "Look through the camera keys while paused (playing always does; off: fly the viewport yourself)",
            ) {
                ui.look_through = !ui.look_through;
            }
            gui::same_line_touching();
            if chip(false, icons::CAMERA, "cinkey", "Key this view: a camera key at the playhead") {
                ui.look_through = false;
                ui.key_camera_view(e);
            }
            gui::same_line_touching();
            if chip(
                ui.recording,
                icons::CIRCLE_DOT_DASHED,
                "cinrecord",
                "Record the camera: the clip plays and your camera moves are keyed every half second (click again to stop)",
            ) {
                ui.toggle_recording();
            }
            gui::same_line_touching();
            if chip(ui.battle_preview, icons::SWORDS, "cinbattle", "Battle preview: units under orders as the battle would play it") {
                ui.battle_preview = !ui.battle_preview;
            }
            gui::same_line_touching();
            if chip(
                false,
                icons::LOG_OUT,
                "cinlaunch",
                "Launch in game: start the game straight into this battle (the mods chosen in the editor)",
            ) {
                ui.launch_request = Some(true);
            }
        }
        gui::end_child();
        gui::pop_style_color(1);

        // The right click's menu.
        if std::mem::take(&mut self.menu_open) {
            gui::open_popup("cinematic_view_menu");
        }
        if gui::begin_popup("cinematic_view_menu") {
            if self.menu.is_some() {
                if gui::menu_item(
                    &format!("{}  Duplicate", wce_editor::icons::COPY),
                    wce_editor::shortcuts::label("cinematic.duplicate").as_deref(),
                    false,
                    true,
                ) {
                    ui.request_duplicate = true;
                }
                if let Some(SceneItem::Unit(id)) = self.menu
                    && gui::menu_item(&format!("{}  Look at it", wce_editor::icons::FOCUS), None, false, true)
                {
                    ui.look_at = Some(id);
                }
                gui::separator();
                if gui::menu_item(
                    &format!("{}  Delete", wce_editor::icons::TRASH_2),
                    wce_editor::shortcuts::label("cinematic.delete").as_deref(),
                    false,
                    true,
                ) {
                    ui.request_delete = true;
                }
                gui::separator();
            }
            if gui::menu_item(
                &format!("{}  Add here...", wce_editor::icons::PLUS),
                wce_editor::shortcuts::label("cinematic.quick_add").as_deref(),
                false,
                true,
            ) && let Some(g) = self.menu_at
            {
                create.ask_quick_add(g);
            }
            gui::end_popup();
        }
    }
}
