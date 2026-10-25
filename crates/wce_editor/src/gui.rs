//! Immediate-mode calls on the current Dear ImGui context, named and used like the C++ editor's ImGui calls (so the panels port line by line),
//! with Rust strings. Every function needs an open frame on the current context: the shell only calls them between `Context::frame` and
//! `Context::render`.

#![allow(clippy::missing_safety_doc, reason = "the functions are safe on an open frame, which the shell guarantees")]

use std::ffi::CString;

use dear_imgui_sys as sys;
pub use sys::{ImGuiID, ImVec2_c as Vec2};

pub fn v2(x: f32, y: f32) -> Vec2 {
    Vec2 { x, y }
}

pub fn v4(x: f32, y: f32, z: f32, w: f32) -> sys::ImVec4_c {
    sys::ImVec4_c { x, y, z, w }
}

/// A C string of `s` (an interior NUL ends it).
pub fn cs(s: &str) -> CString {
    CString::new(s.split('\0').next().unwrap_or_default()).unwrap_or_default()
}

macro_rules! call {
    ($e:expr) => {
        // SAFETY: Dear ImGui calls on the current context, between NewFrame and Render (see the module).
        unsafe { $e }
    };
}

// ---- windows

pub fn begin(name: &str, open: Option<&mut bool>, flags: i32) -> bool {
    let n = cs(name);
    call!(sys::igBegin(n.as_ptr(), open.map_or(std::ptr::null_mut(), |o| o as *mut bool), flags))
}

pub fn end() {
    call!(sys::igEnd())
}

/// Dear ImGui's style editor, on the live style.
pub fn show_style_editor() {
    call!(sys::igShowStyleEditor(std::ptr::null_mut()))
}

pub fn begin_child(id: &str, size: Vec2, child_flags: i32, window_flags: i32) -> bool {
    let n = cs(id);
    call!(sys::igBeginChild_Str(n.as_ptr(), size, child_flags, window_flags))
}

pub fn end_child() {
    call!(sys::igEndChild())
}

pub fn set_next_window_pos(pos: Vec2, cond: i32, pivot: Vec2) {
    call!(sys::igSetNextWindowPos(pos, cond, pivot))
}

/// Docks the next window into a dock node (`cond`: when, like COND_FIRST_USE_EVER: a window the layout does not know yet).
pub fn set_next_window_dock_id(dock: ImGuiID, cond: i32) {
    call!(sys::igSetNextWindowDockID(dock, cond))
}

/// The dock node the current window is in (0: none).
pub fn window_dock_id() -> ImGuiID {
    call!(sys::igGetWindowDockID())
}

/// Opens or closes the next tree node.
pub fn set_next_item_open(open: bool, cond: i32) {
    call!(sys::igSetNextItemOpen(open, cond))
}

/// Brings a window to the front by its name (a docked one: its tab is selected).
pub fn set_window_focus_named(name: &str) {
    let n = cs(name);
    call!(sys::igSetWindowFocus_Str(n.as_ptr()))
}

/// Brings the next window to the front (a docked one: its tab is selected).
pub fn set_next_window_focus() {
    call!(sys::igSetNextWindowFocus())
}

pub fn set_next_window_size(size: Vec2, cond: i32) {
    call!(sys::igSetNextWindowSize(size, cond))
}

pub fn main_viewport() -> &'static sys::ImGuiViewport {
    call!(&*sys::igGetMainViewport())
}

pub fn set_window_focus(name: &str) {
    let n = cs(name);
    call!(sys::igSetWindowFocus_Str(n.as_ptr()))
}

pub fn window_exists(name: &str) -> bool {
    let n = cs(name);
    call!(!sys::igFindWindowByName(n.as_ptr()).is_null())
}

pub fn window_width() -> f32 {
    call!(sys::igGetWindowWidth())
}

// ---- text

pub fn text(s: &str) {
    let b = s.as_bytes();
    call!(sys::igTextUnformatted(b.as_ptr().cast(), b.as_ptr().add(b.len()).cast()))
}

pub fn text_disabled(s: &str) {
    let c = cs(s);
    call!(sys::igTextDisabled(c"%s".as_ptr(), c.as_ptr()))
}

pub fn text_colored(col: [f32; 4], s: &str) {
    let c = cs(s);
    call!(sys::igTextColored(v4(col[0], col[1], col[2], col[3]), c"%s".as_ptr(), c.as_ptr()))
}

pub fn text_wrapped(s: &str) {
    let c = cs(s);
    call!(sys::igTextWrapped(c"%s".as_ptr(), c.as_ptr()))
}

pub fn bullet_text(s: &str) {
    let c = cs(s);
    call!(sys::igBulletText(c"%s".as_ptr(), c.as_ptr()))
}

pub fn set_tooltip(s: &str) {
    let c = cs(s);
    call!(sys::igSetTooltip(c"%s".as_ptr(), c.as_ptr()))
}

pub fn calc_text_width(s: &str) -> f32 {
    let c = cs(s);
    call!(sys::igCalcTextSize(c.as_ptr(), std::ptr::null(), false, -1.0).x)
}

pub fn push_text_wrap_pos(x: f32) {
    call!(sys::igPushTextWrapPos(x))
}

pub fn pop_text_wrap_pos() {
    call!(sys::igPopTextWrapPos())
}

// ---- layout

pub fn same_line() {
    call!(sys::igSameLine(0.0, -1.0))
}

pub fn same_line_at(x: f32) {
    call!(sys::igSameLine(x, -1.0))
}

pub fn separator() {
    call!(sys::igSeparator())
}

pub fn separator_text(s: &str) {
    let c = cs(s);
    call!(sys::igSeparatorText(c.as_ptr()))
}

/// The style's padding inside a frame (x, y).
pub fn style_frame_padding() -> Vec2 {
    call!((*sys::igGetStyle()).FramePadding)
}

/// The style's spacing between items (x, y).
pub fn item_spacing() -> Vec2 {
    call!((*sys::igGetStyle()).ItemSpacing)
}

/// An empty item of a size (it moves the cursor the way an item does).
pub fn dummy(size: Vec2) {
    call!(sys::igDummy(size))
}

pub fn new_line() {
    call!(sys::igNewLine())
}

pub fn set_next_window_bg_alpha(alpha: f32) {
    call!(sys::igSetNextWindowBgAlpha(alpha))
}

pub fn spacing() {
    call!(sys::igSpacing())
}

pub fn align_text_to_frame_padding() {
    call!(sys::igAlignTextToFramePadding())
}

pub fn cursor_pos_x() -> f32 {
    call!(sys::igGetCursorPosX())
}

pub fn set_cursor_pos_x(x: f32) {
    call!(sys::igSetCursorPosX(x))
}

pub fn cursor_screen_pos() -> Vec2 {
    call!(sys::igGetCursorScreenPos())
}

pub fn set_cursor_screen_pos(p: Vec2) {
    call!(sys::igSetCursorScreenPos(p))
}

pub fn content_region_avail() -> Vec2 {
    call!(sys::igGetContentRegionAvail())
}

pub fn set_next_item_width(w: f32) {
    call!(sys::igSetNextItemWidth(w))
}

pub fn push_id_int(id: i32) {
    call!(sys::igPushID_Int(id))
}

pub fn push_id(id: &str) {
    let c = cs(id);
    call!(sys::igPushID_Str(c.as_ptr()))
}

pub fn pop_id() {
    call!(sys::igPopID())
}

pub fn get_id(s: &str) -> ImGuiID {
    let c = cs(s);
    call!(sys::igGetID_Str(c.as_ptr()))
}

pub fn push_style_color(idx: i32, col: sys::ImVec4_c) {
    call!(sys::igPushStyleColor_Vec4(idx, col))
}

pub fn pop_style_color(n: i32) {
    call!(sys::igPopStyleColor(n))
}

pub fn style_color(idx: i32) -> sys::ImVec4_c {
    call!(*sys::igGetStyleColorVec4(idx))
}

pub fn push_style_var_f(idx: i32, v: f32) {
    call!(sys::igPushStyleVar_Float(idx, v))
}

pub fn push_style_var_v(idx: i32, v: Vec2) {
    call!(sys::igPushStyleVar_Vec2(idx, v))
}

pub fn pop_style_var(n: i32) {
    call!(sys::igPopStyleVar(n))
}

pub fn begin_disabled(disabled: bool) {
    call!(sys::igBeginDisabled(disabled))
}

pub fn end_disabled() {
    call!(sys::igEndDisabled())
}

pub fn set_scroll_here_y(r: f32) {
    call!(sys::igSetScrollHereY(r))
}

pub fn scroll_y() -> f32 {
    call!(sys::igGetScrollY())
}

pub fn scroll_max_y() -> f32 {
    call!(sys::igGetScrollMaxY())
}

pub fn set_scroll_y(y: f32) {
    call!(sys::igSetScrollY_Float(y))
}

pub fn window_height() -> f32 {
    call!(sys::igGetWindowHeight())
}

pub fn set_keyboard_focus_here(offset: i32) {
    call!(sys::igSetKeyboardFocusHere(offset))
}

// ---- widgets

pub fn button(label: &str) -> bool {
    let c = cs(label);
    call!(sys::igButton(c.as_ptr(), v2(0.0, 0.0)))
}

pub fn small_button(label: &str) -> bool {
    let c = cs(label);
    call!(sys::igSmallButton(c.as_ptr()))
}

pub fn invisible_button(id: &str, size: Vec2, flags: i32) -> bool {
    let c = cs(id);
    call!(sys::igInvisibleButton(c.as_ptr(), size, flags))
}

pub fn checkbox(label: &str, v: &mut bool) -> bool {
    let c = cs(label);
    call!(sys::igCheckbox(c.as_ptr(), v))
}

pub fn selectable(label: &str, selected: bool, flags: i32) -> bool {
    let c = cs(label);
    call!(sys::igSelectable_Bool(c.as_ptr(), selected, flags, v2(0.0, 0.0)))
}

pub fn menu_item(label: &str, shortcut: Option<&str>, selected: bool, enabled: bool) -> bool {
    let (l, s) = (cs(label), shortcut.map(cs));
    call!(sys::igMenuItem_Bool(l.as_ptr(), s.as_ref().map_or(std::ptr::null(), |s| s.as_ptr()), selected, enabled))
}

pub fn menu_item_toggle(label: &str, v: &mut bool) -> bool {
    let l = cs(label);
    call!(sys::igMenuItem_BoolPtr(l.as_ptr(), std::ptr::null(), v, true))
}

pub fn begin_menu_bar() -> bool {
    call!(sys::igBeginMenuBar())
}

pub fn end_menu_bar() {
    call!(sys::igEndMenuBar())
}

pub fn begin_menu(label: &str) -> bool {
    let c = cs(label);
    call!(sys::igBeginMenu(c.as_ptr(), true))
}

pub fn end_menu() {
    call!(sys::igEndMenu())
}

pub fn drag_float(label: &str, v: &mut f32, speed: f32, lo: f32, hi: f32, format: &str) -> bool {
    let (l, f) = (cs(label), cs(format));
    call!(sys::igDragFloat(l.as_ptr(), v, speed, lo, hi, f.as_ptr(), 0))
}

pub fn drag_float2(label: &str, v: &mut [f32; 2], speed: f32, format: &str) -> bool {
    let (l, f) = (cs(label), cs(format));
    call!(sys::igDragFloat2(l.as_ptr(), v.as_mut_ptr(), speed, 0.0, 0.0, f.as_ptr(), 0))
}

pub fn drag_float3(label: &str, v: &mut [f32; 3], speed: f32, format: &str) -> bool {
    let (l, f) = (cs(label), cs(format));
    call!(sys::igDragFloat3(l.as_ptr(), v.as_mut_ptr(), speed, 0.0, 0.0, f.as_ptr(), 0))
}

pub fn drag_i64(label: &str, v: &mut i64, speed: f32) -> bool {
    let l = cs(label);
    call!(sys::igDragScalar(l.as_ptr(), sys::ImGuiDataType_S64, (v as *mut i64).cast(), speed, std::ptr::null(), std::ptr::null(), std::ptr::null(), 0))
}

pub fn drag_f32(label: &str, v: &mut f32, speed: f32) -> bool {
    let l = cs(label);
    call!(sys::igDragFloat(l.as_ptr(), v, speed, 0.0, 0.0, c"%.3f".as_ptr(), 0))
}

pub fn input_int(label: &str, v: &mut i32) -> bool {
    let l = cs(label);
    call!(sys::igInputInt(l.as_ptr(), v, 1, 100, 0))
}

pub fn slider_float(label: &str, v: &mut f32, lo: f32, hi: f32, format: &str) -> bool {
    let (l, f) = (cs(label), cs(format));
    call!(sys::igSliderFloat(l.as_ptr(), v, lo, hi, f.as_ptr(), 0))
}

pub fn slider_int(label: &str, v: &mut i32, lo: i32, hi: i32) -> bool {
    let l = cs(label);
    call!(sys::igSliderInt(l.as_ptr(), v, lo, hi, c"%d".as_ptr(), 0))
}

/// A combo over `items`; true when the choice changed.
pub fn combo(label: &str, current: &mut i32, items: &[&str]) -> bool {
    let shown = usize::try_from(*current).ok().and_then(|i| items.get(i)).copied().unwrap_or("");
    let mut changed = false;
    if begin_combo(label, shown) {
        for (i, item) in items.iter().enumerate() {
            if selectable(item, i as i32 == *current, 0) {
                *current = i as i32;
                changed = true;
            }
        }

        end_combo();
    }

    changed
}

pub fn color_edit3(label: &str, v: &mut [f32; 3], flags: i32) -> bool {
    let l = cs(label);
    call!(sys::igColorEdit3(l.as_ptr(), v.as_mut_ptr(), flags))
}

pub fn color_edit4(label: &str, v: &mut [f32; 4], flags: i32) -> bool {
    let l = cs(label);
    call!(sys::igColorEdit4(l.as_ptr(), v.as_mut_ptr(), flags))
}

/// A text field over a String; true on every change.
pub fn input_text(label: &str, hint: Option<&str>, value: &mut String, capacity: usize, flags: i32) -> bool {
    let mut buf = value.as_bytes().to_vec();
    buf.resize(capacity.max(buf.len() + 1), 0);
    let l = cs(label);
    let h = hint.map(cs);
    let changed = match &h {
        Some(h) => call!(sys::igInputTextWithHint(l.as_ptr(), h.as_ptr(), buf.as_mut_ptr().cast(), buf.len(), flags, None, std::ptr::null_mut())),
        None => call!(sys::igInputText(l.as_ptr(), buf.as_mut_ptr().cast(), buf.len(), flags, None, std::ptr::null_mut())),
    };

    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let text = String::from_utf8_lossy(&buf[..end]).into_owned();
    if *value != text {
        *value = text;
    }

    changed
}

pub fn begin_combo(label: &str, preview: &str) -> bool {
    let (l, p) = (cs(label), cs(preview));
    call!(sys::igBeginCombo(l.as_ptr(), p.as_ptr(), 0))
}

pub fn end_combo() {
    call!(sys::igEndCombo())
}

pub fn tree_node_ex(label: &str, flags: i32) -> bool {
    let c = cs(label);
    call!(sys::igTreeNodeEx_Str(c.as_ptr(), flags))
}

/// A tree node whose id is `id` and whose shown text is `label` (the id stays when the text changes).
pub fn tree_node_ex_id(id: &str, flags: i32, label: &str) -> bool {
    let (i, l) = (cs(id), cs(label));
    call!(sys::igTreeNodeEx_StrStr(i.as_ptr(), flags, c"%s".as_ptr(), l.as_ptr()))
}

pub fn tree_node(label: &str) -> bool {
    let c = cs(label);
    call!(sys::igTreeNode_Str(c.as_ptr()))
}

/// A tree node with an id and a shown text.
pub fn tree_node_id(id: &str, label: &str) -> bool {
    let (i, l) = (cs(id), cs(label));
    call!(sys::igTreeNode_StrStr(i.as_ptr(), c"%s".as_ptr(), l.as_ptr()))
}

pub fn tree_pop() {
    call!(sys::igTreePop())
}

pub fn collapsing_header(label: &str, flags: i32) -> bool {
    let c = cs(label);
    call!(sys::igCollapsingHeader_TreeNodeFlags(c.as_ptr(), flags))
}

pub fn image(id: u64, size: Vec2) {
    let tex = sys::ImTextureRef_c { _TexData: std::ptr::null_mut(), _TexID: id };
    call!(sys::igImage(tex, size, v2(0.0, 0.0), v2(1.0, 1.0)))
}

// ---- tabs and tables

pub fn begin_tab_bar(id: &str) -> bool {
    let c = cs(id);
    call!(sys::igBeginTabBar(c.as_ptr(), 0))
}

pub fn end_tab_bar() {
    call!(sys::igEndTabBar())
}

pub fn begin_tab_item(label: &str, flags: i32) -> bool {
    let c = cs(label);
    call!(sys::igBeginTabItem(c.as_ptr(), std::ptr::null_mut(), flags))
}

pub fn end_tab_item() {
    call!(sys::igEndTabItem())
}

pub fn begin_table(id: &str, columns: i32, flags: i32, outer_size: Vec2) -> bool {
    let c = cs(id);
    call!(sys::igBeginTable(c.as_ptr(), columns, flags, outer_size, 0.0))
}

pub fn end_table() {
    call!(sys::igEndTable())
}

pub fn table_setup_column(label: &str, flags: i32, width_or_weight: f32) {
    let c = cs(label);
    call!(sys::igTableSetupColumn(c.as_ptr(), flags, width_or_weight, 0))
}

pub fn table_setup_scroll_freeze(cols: i32, rows: i32) {
    call!(sys::igTableSetupScrollFreeze(cols, rows))
}

pub fn table_headers_row() {
    call!(sys::igTableHeadersRow())
}

pub fn table_next_row() {
    call!(sys::igTableNextRow(0, 0.0))
}

pub fn table_next_column() -> bool {
    call!(sys::igTableNextColumn())
}

pub fn table_set_column_index(column: i32) -> bool {
    call!(sys::igTableSetColumnIndex(column))
}

// ---- drawing

/// The current window's draw list.
#[derive(Clone, Copy, Debug)]
pub struct DrawList(*mut sys::ImDrawList);

/// IM_COL32.
pub const fn col32(r: u8, g: u8, b: u8, a: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16) | ((a as u32) << 24)
}

pub fn window_draw_list() -> DrawList {
    DrawList(call!(sys::igGetWindowDrawList()))
}

impl DrawList {
    pub fn ellipse(&self, c: Vec2, radius: Vec2, col: u32, segments: i32, thickness: f32) {
        call!(sys::ImDrawList_AddEllipse(self.0, c, radius, col, 0.0, segments, thickness))
    }

    pub fn circle_filled(&self, c: Vec2, radius: f32, col: u32) {
        call!(sys::ImDrawList_AddCircleFilled(self.0, c, radius, col, 0))
    }

    pub fn line(&self, a: Vec2, b: Vec2, col: u32, thickness: f32) {
        call!(sys::ImDrawList_AddLine(self.0, a, b, col, thickness))
    }

    pub fn triangle_filled(&self, a: Vec2, b: Vec2, c: Vec2, col: u32) {
        call!(sys::ImDrawList_AddTriangleFilled(self.0, a, b, c, col))
    }

    pub fn rect_filled(&self, min: Vec2, max: Vec2, col: u32, rounding: f32) {
        call!(sys::ImDrawList_AddRectFilled(self.0, min, max, col, rounding, 0))
    }

    pub fn rect(&self, min: Vec2, max: Vec2, col: u32, rounding: f32, thickness: f32) {
        call!(sys::ImDrawList_AddRect(self.0, min, max, col, rounding, thickness, 0))
    }

    pub fn text(&self, pos: Vec2, col: u32, text: &str) {
        let r = text.as_bytes().as_ptr_range();
        call!(sys::ImDrawList_AddText_Vec2(self.0, pos, col, r.start.cast(), r.end.cast()))
    }

    /// A picture (a texture id from `Editor::images`) between two corners, with texture coordinates and a tint (IM_COL32).
    pub fn image(&self, id: u64, min: Vec2, max: Vec2, uv0: Vec2, uv1: Vec2, tint: u32) {
        let tex = sys::ImTextureRef_c { _TexData: std::ptr::null_mut(), _TexID: id };
        call!(sys::ImDrawList_AddImage(self.0, tex, min, max, uv0, uv1, tint))
    }

    /// Text at an explicit size in pixels (the font's own size scaled).
    pub fn text_sized(&self, size: f32, pos: Vec2, col: u32, text: &str) {
        let r = text.as_bytes().as_ptr_range();
        call!(sys::ImDrawList_AddText_FontPtr(self.0, std::ptr::null_mut(), size, pos, col, r.start.cast(), r.end.cast(), 0.0, std::ptr::null()))
    }

    pub fn push_clip_rect(&self, min: Vec2, max: Vec2) {
        call!(sys::ImDrawList_PushClipRect(self.0, min, max, true))
    }

    pub fn pop_clip_rect(&self) {
        call!(sys::ImDrawList_PopClipRect(self.0))
    }
}

// ---- drag and drop (payloads of element indices)

pub fn begin_drag_drop_source() -> bool {
    call!(sys::igBeginDragDropSource(sys::ImGuiDragDropFlags_SourceAllowNullID))
}

pub fn set_drag_drop_payload(kind: &str, data: &[i32]) {
    let k = cs(kind);
    call!({ sys::igSetDragDropPayload(k.as_ptr(), data.as_ptr().cast(), std::mem::size_of_val(data), 0); })
}

pub fn end_drag_drop_source() {
    call!(sys::igEndDragDropSource())
}

pub fn begin_drag_drop_target() -> bool {
    call!(sys::igBeginDragDropTarget())
}

/// A payload of `kind` hovering this target (before delivery, without the default rectangle): its data and whether it is delivered now.
pub fn accept_drag_drop_payload(kind: &str) -> Option<(Vec<i32>, bool)> {
    let k = cs(kind);
    call!({
        let p = sys::igAcceptDragDropPayload(k.as_ptr(), sys::ImGuiDragDropFlags_AcceptBeforeDelivery | sys::ImGuiDragDropFlags_AcceptNoDrawDefaultRect);
        if p.is_null() {
            None
        } else {
            let n = usize::try_from((*p).DataSize).unwrap_or(0) / std::mem::size_of::<i32>();
            let data = if n == 0 { vec![] } else { std::slice::from_raw_parts((*p).Data as *const i32, n).to_vec() };
            Some((data, sys::ImGuiPayload_IsDelivery(p.cast_mut())))
        }
    })
}

pub fn end_drag_drop_target() {
    call!(sys::igEndDragDropTarget())
}

// ---- metrics and misc

pub fn set_next_item_allow_overlap() {
    call!(sys::igSetNextItemAllowOverlap())
}

/// The window (or a child of it) is hovered.
pub fn is_window_hovered_with_children() -> bool {
    call!(sys::igIsWindowHovered(sys::ImGuiHoveredFlags_ChildWindows | sys::ImGuiHoveredFlags_RootAndChildWindows))
}

/// The last item's rectangle on screen: (top left, bottom right).
pub fn item_rect() -> (Vec2, Vec2) {
    (call!(sys::igGetItemRectMin()), call!(sys::igGetItemRectMax()))
}

pub fn mouse_pos() -> Vec2 {
    call!(sys::igGetMousePos())
}

pub fn frame_height_with_spacing() -> f32 {
    call!(sys::igGetFrameHeightWithSpacing())
}

pub fn calc_text_size(s: &str) -> Vec2 {
    let r = s.as_bytes().as_ptr_range();
    call!(sys::igCalcTextSize(r.start.cast(), r.end.cast(), false, -1.0))
}

pub fn text_line_height() -> f32 {
    call!(sys::igGetTextLineHeight())
}

pub fn text_line_height_with_spacing() -> f32 {
    call!(sys::igGetTextLineHeightWithSpacing())
}

pub fn time() -> f64 {
    call!(sys::igGetTime())
}

pub fn radio_button(label: &str, active: bool) -> bool {
    let c = cs(label);
    call!(sys::igRadioButton_Bool(c.as_ptr(), active))
}

/// A swatch that does nothing on click.
pub fn color_swatch(id: &str, col: [f32; 4], size: Vec2) -> bool {
    let c = cs(id);
    let flags = sys::ImGuiColorEditFlags_NoTooltip | sys::ImGuiColorEditFlags_NoDragDrop | sys::ImGuiColorEditFlags_NoPicker;
    call!(sys::igColorButton(c.as_ptr(), v4(col[0], col[1], col[2], col[3]), flags, size))
}

pub fn selectable_sized(label: &str, selected: bool, flags: i32, size: Vec2) -> bool {
    let c = cs(label);
    call!(sys::igSelectable_Bool(c.as_ptr(), selected, flags, size))
}

pub fn is_item_clicked_with(button: i32) -> bool {
    call!(sys::igIsItemClicked(button))
}

// ---- popups

pub fn open_popup(id: &str) {
    let c = cs(id);
    call!({ sys::igOpenPopup_Str(c.as_ptr(), 0); })
}

pub fn begin_popup_modal(name: &str, open: Option<&mut bool>, flags: i32) -> bool {
    let c = cs(name);
    call!(sys::igBeginPopupModal(c.as_ptr(), open.map_or(std::ptr::null_mut(), |o| o as *mut bool), flags))
}

/// A popup opened with `open_popup`.
pub fn begin_popup(id: &str) -> bool {
    let c = cs(id);
    call!(sys::igBeginPopup(c.as_ptr(), 0))
}

pub fn begin_popup_context_item(id: &str) -> bool {
    let c = cs(id);
    call!(sys::igBeginPopupContextItem(c.as_ptr(), sys::ImGuiPopupFlags_MouseButtonRight))
}

pub fn end_popup() {
    call!(sys::igEndPopup())
}

pub fn close_current_popup() {
    call!(sys::igCloseCurrentPopup())
}

// ---- item state

/// Hovered, also when the item is disabled (to explain why).
pub fn is_item_hovered_allow_disabled() -> bool {
    call!(sys::igIsItemHovered(sys::ImGuiHoveredFlags_AllowWhenDisabled))
}

pub fn is_item_hovered() -> bool {
    call!(sys::igIsItemHovered(0))
}

pub fn is_item_active() -> bool {
    call!(sys::igIsItemActive())
}

pub fn is_item_clicked() -> bool {
    call!(sys::igIsItemClicked(0))
}

pub fn is_item_toggled_open() -> bool {
    call!(sys::igIsItemToggledOpen())
}

pub fn is_item_deactivated_after_edit() -> bool {
    call!(sys::igIsItemDeactivatedAfterEdit())
}

pub fn is_item_activated() -> bool {
    call!(sys::igIsItemActivated())
}

pub fn is_item_deactivated() -> bool {
    call!(sys::igIsItemDeactivated())
}

pub fn is_window_appearing() -> bool {
    call!(sys::igIsWindowAppearing())
}

pub fn item_id() -> ImGuiID {
    call!(sys::igGetItemID())
}

// ---- input

pub fn io() -> &'static mut sys::ImGuiIO {
    call!(&mut *sys::igGetIO_Nil())
}

pub fn is_mouse_clicked(button: i32) -> bool {
    call!(sys::igIsMouseClicked_Bool(button, false))
}

pub fn is_mouse_double_clicked(button: i32) -> bool {
    call!(sys::igIsMouseDoubleClicked_Nil(button))
}

pub fn is_mouse_down(button: i32) -> bool {
    call!(sys::igIsMouseDown_Nil(button))
}

pub fn is_mouse_released(button: i32) -> bool {
    call!(sys::igIsMouseReleased_Nil(button))
}

pub fn is_key_pressed(key: i32, repeat: bool) -> bool {
    call!(sys::igIsKeyPressed_Bool(key, repeat))
}

pub fn is_key_down(key: i32) -> bool {
    call!(sys::igIsKeyDown_Nil(key))
}

/// Queues key events (the `key` command; also outside a frame).
pub fn add_key_event(key: i32, down: bool) {
    call!(sys::ImGuiIO_AddKeyEvent(sys::igGetIO_Nil(), key, down))
}

pub fn add_mouse_pos_event(x: f32, y: f32) {
    call!(sys::ImGuiIO_AddMousePosEvent(sys::igGetIO_Nil(), x, y))
}

pub fn add_mouse_button_event(button: i32, down: bool) {
    call!(sys::ImGuiIO_AddMouseButtonEvent(sys::igGetIO_Nil(), button, down))
}

pub fn add_mouse_wheel_event(x: f32, y: f32) {
    call!(sys::ImGuiIO_AddMouseWheelEvent(sys::igGetIO_Nil(), x, y))
}

/// Gives the mouse wheel (both axes) to `owner`, or back to everyone (None): while a window's item owns it, Dear ImGui scrolls no window
/// with it (it would otherwise bubble a wheel over a window that does not scroll up to a parent that does; Shift makes it horizontal).
pub fn own_mouse_wheel(owner: Option<ImGuiID>) {
    let id = owner.unwrap_or(ImGuiID::MAX);
    call!({
        sys::igSetKeyOwner(sys::ImGuiKey_MouseWheelX, id, 0);
        sys::igSetKeyOwner(sys::ImGuiKey_MouseWheelY, id, 0);
    })
}

// ---- docking and settings

pub fn dock_space(id: ImGuiID, size: Vec2) {
    call!(sys::igDockSpace(id, size, 0, std::ptr::null()));
}

pub fn dock_builder_reset(id: ImGuiID, size: Vec2) {
    call!({
        sys::igDockBuilderRemoveNode(id);
        sys::igDockBuilderAddNode(id, sys::ImGuiDockNodeFlags_DockSpace);
        sys::igDockBuilderSetNodeSize(id, size);
    })
}

/// Splits `node` and returns (the new node on `dir`, what is left of `node`).
pub fn dock_builder_split(node: ImGuiID, dir: i32, ratio: f32) -> (ImGuiID, ImGuiID) {
    let mut rest = 0;
    let at = call!(sys::igDockBuilderSplitNode(node, dir, ratio, std::ptr::null_mut(), &mut rest));
    (at, rest)
}

pub fn dock_builder_dock_window(name: &str, node: ImGuiID) {
    let c = cs(name);
    call!(sys::igDockBuilderDockWindow(c.as_ptr(), node))
}

pub fn dock_builder_finish(id: ImGuiID) {
    call!(sys::igDockBuilderFinish(id))
}

pub fn save_ini(path: &std::path::Path) {
    let c = cs(&path.to_string_lossy());
    call!(sys::igSaveIniSettingsToDisk(c.as_ptr()))
}

pub fn load_ini(path: &std::path::Path) {
    let c = cs(&path.to_string_lossy());
    call!(sys::igLoadIniSettingsFromDisk(c.as_ptr()))
}

pub fn load_ini_empty() {
    call!(sys::igLoadIniSettingsFromMemory(c"".as_ptr(), 0))
}

/// A clipped list: calls `row` for the rows on screen.
pub fn clipped(count: usize, row: impl FnMut(usize)) {
    clipped_rows(count, -1.0, row)
}

/// A clipped list of rows of a known height (-1: measured from the first row).
pub fn clipped_rows(count: usize, height: f32, mut row: impl FnMut(usize)) {
    call!({
        let clipper = sys::ImGuiListClipper_ImGuiListClipper();
        sys::ImGuiListClipper_Begin(clipper, count as i32, height);
        while sys::ImGuiListClipper_Step(clipper) {
            for i in (*clipper).DisplayStart..(*clipper).DisplayEnd {
                row(i as usize);
            }
        }

        sys::ImGuiListClipper_destroy(clipper);
    })
}

/// The hovered and active ids of the current context, for the gizmo's view of the viewport button.
pub fn context_ids() -> (ImGuiID, ImGuiID, ImGuiID) {
    call!({
        let g = &*sys::igGetCurrentContext();
        (g.HoveredId, g.HoveredIdPreviousFrame, g.ActiveId)
    })
}

pub fn set_context_ids(ids: (ImGuiID, ImGuiID, ImGuiID)) {
    call!({
        let g = &mut *sys::igGetCurrentContext();
        (g.HoveredId, g.HoveredIdPreviousFrame, g.ActiveId) = ids;
    })
}

// ---- constants with the C++ names the panels use

pub use sys::{
    ImGuiButtonFlags_MouseButtonLeft as BUTTON_LEFT, ImGuiButtonFlags_MouseButtonMiddle as BUTTON_MIDDLE, ImGuiButtonFlags_MouseButtonRight as BUTTON_RIGHT,
    ImGuiChildFlags_Borders as CHILD_BORDERS, ImGuiCol_Button as COL_BUTTON, ImGuiInputTextFlags_AutoSelectAll as INPUT_AUTO_SELECT_ALL, ImGuiTableFlags_SizingFixedFit as TABLE_SIZING_FIXED_FIT,
    ImGuiTableFlags_BordersInnerV as TABLE_BORDERS_INNER_V, ImGuiKey_Delete as KEY_DELETE, ImGuiKey_G as KEY_G, ImGuiKey_Enter as KEY_ENTER, ImGuiKey_KeypadEnter as KEY_KEYPAD_ENTER,
    ImGuiKey_F2 as KEY_F2, ImGuiCol_FrameBg as COL_FRAME_BG, ImGuiCol_ButtonActive as COL_BUTTON_ACTIVE, ImGuiCol_CheckMark as COL_CHECK_MARK,
    ImGuiCol_Text as COL_TEXT, ImGuiCol_TextDisabled as COL_TEXT_DISABLED, ImGuiColorEditFlags_AlphaBar as COLOR_ALPHA_BAR, ImGuiColorEditFlags_Float as COLOR_FLOAT,
    ImGuiColorEditFlags_HDR as COLOR_HDR, ImGuiCond_Always as COND_ALWAYS, ImGuiCond_Appearing as COND_APPEARING, ImGuiCond_FirstUseEver as COND_FIRST_USE_EVER, ImGuiDir_Down as DIR_DOWN,
    ImGuiDir_Left as DIR_LEFT, ImGuiDir_Right as DIR_RIGHT, ImGuiInputTextFlags_EnterReturnsTrue as INPUT_ENTER_RETURNS_TRUE, ImGuiKey_1 as KEY_1,
    ImGuiKey_A as KEY_A, ImGuiKey_D as KEY_D, ImGuiKey_U as KEY_U, ImGuiKey_E as KEY_E, ImGuiKey_Escape as KEY_ESCAPE, ImGuiKey_F as KEY_F, ImGuiKey_Q as KEY_Q, ImGuiKey_R as KEY_R,
    ImGuiKey_S as KEY_S, ImGuiKey_W as KEY_W, ImGuiKey_Y as KEY_Y, ImGuiKey_Z as KEY_Z, ImGuiSelectableFlags_AllowDoubleClick as SELECTABLE_ALLOW_DOUBLE_CLICK,
    ImGuiStyleVar_WindowBorderSize as VAR_WINDOW_BORDER_SIZE, ImGuiStyleVar_WindowPadding as VAR_WINDOW_PADDING, ImGuiStyleVar_WindowRounding as VAR_WINDOW_ROUNDING,
    ImGuiTreeNodeFlags_DefaultOpen as TREE_DEFAULT_OPEN, ImGuiTreeNodeFlags_Leaf as TREE_LEAF, ImGuiTreeNodeFlags_NoTreePushOnOpen as TREE_NO_PUSH_ON_OPEN, ImGuiTreeNodeFlags_OpenOnArrow as TREE_OPEN_ON_ARROW, ImGuiTreeNodeFlags_Selected as TREE_SELECTED,
    ImGuiTreeNodeFlags_SpanAvailWidth as TREE_SPAN_AVAIL_WIDTH, ImGuiWindowFlags_AlwaysAutoResize as WINDOW_ALWAYS_AUTO_RESIZE,
    ImGuiWindowFlags_HorizontalScrollbar as WINDOW_HORIZONTAL_SCROLLBAR, ImGuiWindowFlags_MenuBar as WINDOW_MENU_BAR,
    ImGuiWindowFlags_NoBringToFrontOnFocus as WINDOW_NO_BRING_TO_FRONT, ImGuiWindowFlags_NoCollapse as WINDOW_NO_COLLAPSE, ImGuiWindowFlags_NoDocking as WINDOW_NO_DOCKING,
    ImGuiWindowFlags_NoMove as WINDOW_NO_MOVE, ImGuiWindowFlags_NoNavFocus as WINDOW_NO_NAV_FOCUS, ImGuiWindowFlags_NoResize as WINDOW_NO_RESIZE,
    ImGuiWindowFlags_NoSavedSettings as WINDOW_NO_SAVED_SETTINGS, ImGuiWindowFlags_NoScrollWithMouse as WINDOW_NO_SCROLL_WITH_MOUSE,
    ImGuiWindowFlags_NoScrollbar as WINDOW_NO_SCROLLBAR, ImGuiWindowFlags_NoTitleBar as WINDOW_NO_TITLE_BAR, ImGuiTableFlags_Resizable as TABLE_RESIZABLE, ImGuiTableFlags_RowBg as TABLE_ROW_BG,
    ImGuiTableFlags_Borders as TABLE_BORDERS, ImGuiTableFlags_SizingStretchProp as TABLE_SIZING_STRETCH_PROP, ImGuiTableFlags_ScrollY as TABLE_SCROLL_Y,
    ImGuiTableColumnFlags_WidthStretch as COLUMN_WIDTH_STRETCH, ImGuiTableColumnFlags_WidthFixed as COLUMN_WIDTH_FIXED, ImGuiTabItemFlags_SetSelected as TAB_SET_SELECTED,
};
pub const CHILD_AUTO_RESIZE: i32 = sys::ImGuiChildFlags_AutoResizeX | sys::ImGuiChildFlags_AutoResizeY | sys::ImGuiChildFlags_AlwaysUseWindowPadding;
pub const COL_CHILD_BG: i32 = sys::ImGuiCol_ChildBg;
pub const MOD_CTRL: i32 = sys::ImGuiMod_Ctrl;
pub const MOD_SHIFT: i32 = sys::ImGuiMod_Shift;
pub const MOD_ALT: i32 = sys::ImGuiMod_Alt;


// ---- multi-line text

extern "C" fn resize_callback(data: *mut sys::ImGuiInputTextCallbackData) -> i32 {
    // SAFETY: Dear ImGui passes its callback data; UserData is the `Vec<u8>` of `input_text_multiline`, which outlives the call.
    unsafe {
        let d = &mut *data;
        if d.EventFlag == sys::ImGuiInputTextFlags_CallbackResize as i32 {
            let v = &mut *d.UserData.cast::<Vec<u8>>();
            v.resize(d.BufSize.max(1) as usize, 0);
            d.Buf = v.as_mut_ptr().cast();
        }
    }

    0
}

/// A multi-line text box that grows with its text; true on every change.
pub fn input_text_multiline(label: &str, value: &mut String, size: Vec2, flags: i32) -> bool {
    let mut buf = value.as_bytes().to_vec();
    buf.push(0);
    let l = cs(label);
    let changed = call!(sys::igInputTextMultiline(
        l.as_ptr(), buf.as_mut_ptr().cast(), buf.len(), size, flags | sys::ImGuiInputTextFlags_CallbackResize as i32, Some(resize_callback),
        (&raw mut buf).cast(),
    ));
    if changed {
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        *value = String::from_utf8_lossy(&buf[..end]).into_owned();
    }

    changed
}
