//! How the editor looks: the fonts (Inter, with Lucide's icons merged in, see `icons`; Dear ImGui's own monospace font for the log and the
//! console), the themes (colours and sizes, `[appearance]` in the settings) and the named colours panels use instead of fixed ones, so that
//! every theme keeps them readable.
//!
//! The fonts go into the atlas once, before the first frame (`load_fonts`); a theme or a text size is applied between frames (`apply`).

use std::sync::RwLock;
use std::sync::atomic::{AtomicPtr, Ordering};

use dear_imgui_sys as sys;
use sys::ImVec4_c as V4;
use wce_foundation::settings::Appearance;

const INTER: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
const LUCIDE: &[u8] = include_bytes!("../assets/fonts/lucide.ttf");

/// The size the fonts are added at: the glyph offsets below are for it, other sizes scale them.
const REFERENCE_SIZE: f32 = 16.0;

/// The private use area, Lucide's: Inter has glyphs of its own there, which must not hide the icons (a zero-ended list of ranges).
static PRIVATE_USE: [sys::ImWchar; 3] = [0xE000, 0xF8FF, 0];

/// The text sizes the settings allow, in points before the display's scale.
pub const FONT_SIZES: std::ops::RangeInclusive<f32> = 11.0..=22.0;

/// A theme the settings can name.
pub struct ThemeInfo {
    pub id: &'static str,
    pub title: &'static str,
}

pub const THEMES: &[ThemeInfo] = &[
    ThemeInfo { id: "dark", title: "Dark" },
    ThemeInfo { id: "light", title: "Light" },
    ThemeInfo { id: "imgui", title: "Dear ImGui (classic)" },
];

/// The colours with a meaning, for text and marks in the panels (not over the 3D view, which has its own).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub error: [f32; 4],
    pub warning: [f32; 4],

    /// Unsaved changes.
    pub modified: [f32; 4],
    pub success: [f32; 4],

    /// Drop targets and markers.
    pub highlight: [f32; 4],

    /// The background of a field with a bad value.
    pub error_bg: [f32; 4],
}

const DARK_PALETTE: Palette = Palette {
    error: [1.0, 0.45, 0.4, 1.0], warning: [1.0, 0.76, 0.35, 1.0], modified: [1.0, 0.75, 0.3, 1.0], success: [0.45, 0.85, 0.5, 1.0],
    highlight: [1.0, 0.82, 0.24, 1.0], error_bg: [0.45, 0.15, 0.15, 1.0],
};

const LIGHT_PALETTE: Palette = Palette {
    error: [0.8, 0.13, 0.1, 1.0], warning: [0.68, 0.42, 0.0, 1.0], modified: [0.75, 0.42, 0.0, 1.0], success: [0.1, 0.52, 0.2, 1.0],
    highlight: [0.9, 0.6, 0.0, 1.0], error_bg: [1.0, 0.8, 0.78, 1.0],
};

static PALETTE: RwLock<Palette> = RwLock::new(DARK_PALETTE);
static MONO: AtomicPtr<sys::ImFont> = AtomicPtr::new(std::ptr::null_mut());

/// The current theme's named colours.
pub fn palette() -> Palette {
    *PALETTE.read().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn error() -> [f32; 4] {
    palette().error
}

pub fn warning() -> [f32; 4] {
    palette().warning
}

pub fn modified() -> [f32; 4] {
    palette().modified
}

pub fn success() -> [f32; 4] {
    palette().success
}

/// Secondary text: the theme's disabled text colour.
pub fn dim() -> [f32; 4] {
    let c = crate::gui::style_color(crate::gui::COL_TEXT_DISABLED);
    [c.x, c.y, c.z, c.w]
}

/// The windows' background.
pub fn palette_background() -> [f32; 4] {
    let c = crate::gui::style_color(sys::ImGuiCol_WindowBg);
    [c.x, c.y, c.z, c.w]
}

pub fn text() -> [f32; 4] {
    let c = crate::gui::style_color(crate::gui::COL_TEXT);
    [c.x, c.y, c.z, c.w]
}

pub fn v4(c: [f32; 4]) -> V4 {
    V4 { x: c[0], y: c[1], z: c[2], w: c[3] }
}

/// A colour for the draw lists (IM_COL32).
pub fn u32(c: [f32; 4]) -> u32 {
    // SAFETY: a pure conversion.
    unsafe { sys::igColorConvertFloat4ToU32(v4(c)) }
}

/// `a` mixed with `b` (0: a, 1: b).
pub fn mix(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

/// Adds the fonts to the current context's atlas: Inter with Lucide merged in (the default font) and the monospace one.
///
/// # Safety
///
/// A current Dear ImGui context, before its first frame.
pub unsafe fn load_fonts() {
    // SAFETY: the caller's contract; the font data is static, so the atlas must not own (free) it.
    unsafe {
        let atlas = (*sys::igGetIO_Nil()).Fonts;
        let add = |data: &'static [u8], merge: bool, offset_y: f32, min_advance: f32, scale: f32| {
            let cfg = sys::ImFontConfig_ImFontConfig();
            (*cfg).FontDataOwnedByAtlas = false;
            if !merge {
                (*cfg).GlyphExcludeRanges = PRIVATE_USE.as_ptr();
            }

            (*cfg).MergeMode = merge;
            (*cfg).GlyphOffset.y = offset_y;
            (*cfg).GlyphMinAdvanceX = min_advance;
            let font = sys::ImFontAtlas_AddFontFromMemoryTTF(atlas, data.as_ptr().cast_mut().cast(), data.len() as i32, REFERENCE_SIZE * scale, cfg, std::ptr::null());
            sys::ImFontConfig_destroy(cfg);
            font
        };

        add(INTER, false, 0.0, 0.0, 1.0);

        // The icons, a little smaller than the text and centred on its x-height, as wide as the text is high so that they line up.
        add(LUCIDE, true, 2.5, REFERENCE_SIZE, 0.95);
        MONO.store(sys::ImFontAtlas_AddFontDefaultVector(atlas, std::ptr::null()), Ordering::Relaxed);
    }
}

/// The monospace font until `pop_font` (the log and the console). Nothing happens without `load_fonts`; `pop_font` is still due.
pub fn push_mono() {
    let mono = MONO.load(Ordering::Relaxed);
    // SAFETY: an open frame (see `gui`); a null font pushes the current one again.
    unsafe { sys::igPushFont(mono, 0.0) }
}

pub fn pop_font() {
    // SAFETY: after a push.
    unsafe { sys::igPopFont() }
}

fn rgb(r: u8, g: u8, b: u8) -> V4 {
    rgba(r, g, b, 1.0)
}

fn rgba(r: u8, g: u8, b: u8, a: f32) -> V4 {
    V4 { x: f32::from(r) / 255.0, y: f32::from(g) / 255.0, z: f32::from(b) / 255.0, w: a }
}

/// The accent of the editor's own themes.
const ACCENT: (u8, u8, u8) = (74, 138, 226);

fn accent(a: f32) -> V4 {
    rgba(ACCENT.0, ACCENT.1, ACCENT.2, a)
}

/// Sizes of the editor's own themes: a little more air than Dear ImGui's and soft corners.
fn editor_sizes(s: &mut sys::ImGuiStyle) {
    s.WindowPadding = sys::ImVec2_c { x: 8.0, y: 8.0 };
    s.FramePadding = sys::ImVec2_c { x: 6.0, y: 4.0 };
    s.CellPadding = sys::ImVec2_c { x: 6.0, y: 3.0 };
    s.ItemSpacing = sys::ImVec2_c { x: 8.0, y: 5.0 };
    s.ItemInnerSpacing = sys::ImVec2_c { x: 5.0, y: 4.0 };
    s.IndentSpacing = 16.0;
    s.ScrollbarSize = 12.0;
    s.GrabMinSize = 10.0;
    s.WindowRounding = 4.0;
    s.ChildRounding = 3.0;
    s.FrameRounding = 3.0;
    s.PopupRounding = 4.0;
    s.ScrollbarRounding = 6.0;
    s.GrabRounding = 3.0;
    s.TabRounding = 3.0;
    s.WindowBorderSize = 1.0;
    s.ChildBorderSize = 1.0;
    s.PopupBorderSize = 1.0;
    s.FrameBorderSize = 0.0;
    s.TabBorderSize = 0.0;
    s.TabBarOverlineSize = 2.0;
    s.SeparatorTextBorderSize = 2.0;
    s.DockingSeparatorSize = 2.0;
    s.WindowMenuButtonPosition = sys::ImGuiDir_None;
}

/// The editor's dark theme: neutral greys, one blue accent.
fn dark_colors(c: &mut [V4]) {
    use sys::*;

    let set = |c: &mut [V4], i: i32, v: V4| c[i as usize] = v;
    set(c, ImGuiCol_Text, rgb(222, 224, 228));
    set(c, ImGuiCol_TextDisabled, rgb(128, 132, 140));
    set(c, ImGuiCol_WindowBg, rgb(30, 31, 34));
    set(c, ImGuiCol_ChildBg, rgba(0, 0, 0, 0.0));
    set(c, ImGuiCol_PopupBg, rgba(36, 37, 41, 0.98));
    set(c, ImGuiCol_Border, rgb(52, 54, 60));
    set(c, ImGuiCol_BorderShadow, rgba(0, 0, 0, 0.0));
    set(c, ImGuiCol_FrameBg, rgb(44, 46, 51));
    set(c, ImGuiCol_FrameBgHovered, rgb(54, 57, 64));
    set(c, ImGuiCol_FrameBgActive, rgb(62, 66, 75));
    set(c, ImGuiCol_TitleBg, rgb(24, 25, 28));
    set(c, ImGuiCol_TitleBgActive, rgb(28, 29, 33));
    set(c, ImGuiCol_TitleBgCollapsed, rgb(24, 25, 28));
    set(c, ImGuiCol_MenuBarBg, rgb(26, 27, 30));
    set(c, ImGuiCol_ScrollbarBg, rgba(0, 0, 0, 0.0));
    set(c, ImGuiCol_ScrollbarGrab, rgb(62, 64, 70));
    set(c, ImGuiCol_ScrollbarGrabHovered, rgb(78, 81, 89));
    set(c, ImGuiCol_ScrollbarGrabActive, rgb(96, 100, 110));
    set(c, ImGuiCol_CheckMark, rgb(110, 168, 245));
    set(c, ImGuiCol_CheckboxSelectedBg, rgb(44, 46, 51));
    set(c, ImGuiCol_SliderGrab, accent(1.0));
    set(c, ImGuiCol_SliderGrabActive, rgb(110, 168, 245));
    set(c, ImGuiCol_Button, rgb(50, 53, 59));
    set(c, ImGuiCol_ButtonHovered, rgb(62, 66, 74));
    set(c, ImGuiCol_ButtonActive, rgb(46, 96, 170));
    set(c, ImGuiCol_Header, accent(0.28));
    set(c, ImGuiCol_HeaderHovered, accent(0.42));
    set(c, ImGuiCol_HeaderActive, accent(0.58));
    set(c, ImGuiCol_Separator, rgb(52, 54, 60));
    set(c, ImGuiCol_SeparatorHovered, accent(0.6));
    set(c, ImGuiCol_SeparatorActive, accent(1.0));
    set(c, ImGuiCol_ResizeGrip, accent(0.18));
    set(c, ImGuiCol_ResizeGripHovered, accent(0.5));
    set(c, ImGuiCol_ResizeGripActive, accent(0.8));
    set(c, ImGuiCol_InputTextCursor, rgb(222, 224, 228));
    set(c, ImGuiCol_Tab, rgb(34, 35, 39));
    set(c, ImGuiCol_TabHovered, rgb(58, 62, 71));
    set(c, ImGuiCol_TabSelected, rgb(44, 46, 52));
    set(c, ImGuiCol_TabSelectedOverline, accent(1.0));
    set(c, ImGuiCol_TabDimmed, rgb(28, 29, 33));
    set(c, ImGuiCol_TabDimmedSelected, rgb(38, 40, 45));
    set(c, ImGuiCol_TabDimmedSelectedOverline, rgba(0, 0, 0, 0.0));
    set(c, ImGuiCol_DockingPreview, accent(0.6));
    set(c, ImGuiCol_DockingEmptyBg, rgb(20, 21, 23));
    set(c, ImGuiCol_TableHeaderBg, rgb(40, 42, 47));
    set(c, ImGuiCol_TableBorderStrong, rgb(58, 60, 66));
    set(c, ImGuiCol_TableBorderLight, rgb(46, 48, 54));
    set(c, ImGuiCol_TableRowBg, rgba(0, 0, 0, 0.0));
    set(c, ImGuiCol_TableRowBgAlt, rgba(255, 255, 255, 0.03));
    set(c, ImGuiCol_TextLink, rgb(110, 168, 245));
    set(c, ImGuiCol_TextSelectedBg, accent(0.35));
    set(c, ImGuiCol_TreeLines, rgb(70, 73, 80));
    set(c, ImGuiCol_DragDropTarget, rgb(255, 210, 60));
    set(c, ImGuiCol_NavCursor, accent(1.0));
    set(c, ImGuiCol_ModalWindowDimBg, rgba(0, 0, 0, 0.5));
}

/// The editor's light theme: Dear ImGui's light colours with the dark theme's accent.
fn light_colors(c: &mut [V4]) {
    use sys::*;

    let set = |c: &mut [V4], i: i32, v: V4| c[i as usize] = v;
    set(c, ImGuiCol_Text, rgb(28, 30, 34));
    set(c, ImGuiCol_TextDisabled, rgb(120, 124, 132));
    set(c, ImGuiCol_WindowBg, rgb(240, 241, 243));
    set(c, ImGuiCol_PopupBg, rgba(250, 250, 251, 0.98));
    set(c, ImGuiCol_Border, rgb(200, 202, 208));
    set(c, ImGuiCol_FrameBg, rgb(255, 255, 255));
    set(c, ImGuiCol_FrameBgHovered, rgb(232, 238, 248));
    set(c, ImGuiCol_FrameBgActive, rgb(218, 228, 245));
    set(c, ImGuiCol_TitleBg, rgb(226, 228, 232));
    set(c, ImGuiCol_TitleBgActive, rgb(214, 217, 222));
    set(c, ImGuiCol_MenuBarBg, rgb(230, 232, 236));
    set(c, ImGuiCol_CheckMark, accent(1.0));
    set(c, ImGuiCol_SliderGrab, accent(1.0));
    set(c, ImGuiCol_SliderGrabActive, rgb(46, 96, 170));
    set(c, ImGuiCol_Button, rgb(222, 225, 230));
    set(c, ImGuiCol_ButtonHovered, rgb(208, 216, 230));
    set(c, ImGuiCol_ButtonActive, rgb(180, 202, 236));
    set(c, ImGuiCol_Header, accent(0.22));
    set(c, ImGuiCol_HeaderHovered, accent(0.34));
    set(c, ImGuiCol_HeaderActive, accent(0.48));
    set(c, ImGuiCol_Separator, rgb(200, 202, 208));
    set(c, ImGuiCol_SeparatorHovered, accent(0.6));
    set(c, ImGuiCol_SeparatorActive, accent(1.0));
    set(c, ImGuiCol_Tab, rgb(226, 228, 232));
    set(c, ImGuiCol_TabHovered, rgb(208, 216, 230));
    set(c, ImGuiCol_TabSelected, rgb(246, 247, 248));
    set(c, ImGuiCol_TabSelectedOverline, accent(1.0));
    set(c, ImGuiCol_TabDimmed, rgb(226, 228, 232));
    set(c, ImGuiCol_TabDimmedSelected, rgb(238, 239, 241));
    set(c, ImGuiCol_DockingPreview, accent(0.5));
    set(c, ImGuiCol_DockingEmptyBg, rgb(214, 216, 220));
    set(c, ImGuiCol_TextSelectedBg, accent(0.3));
    set(c, ImGuiCol_TextLink, rgb(46, 96, 170));
    set(c, ImGuiCol_DragDropTarget, rgb(230, 150, 0));
    set(c, ImGuiCol_NavCursor, accent(1.0));
}

/// Applies a theme and a text size at a display scale: the style starts from Dear ImGui's defaults each time.
///
/// # Safety
///
/// A current Dear ImGui context, outside a frame.
pub unsafe fn apply(appearance: &Appearance, dpi: f32) {
    // SAFETY: the caller's contract.
    unsafe {
        let style = &mut *sys::igGetStyle();
        let fresh = sys::ImGuiStyle_ImGuiStyle();
        *style = std::ptr::read(fresh);
        sys::ImGuiStyle_destroy(fresh);

        let palette = match appearance.theme.as_str() {
            "light" => {
                sys::igStyleColorsLight(style);
                editor_sizes(style);
                light_colors(&mut style.Colors);
                LIGHT_PALETTE
            }

            "imgui" => {
                sys::igStyleColorsDark(style);
                DARK_PALETTE
            }

            _ => {
                sys::igStyleColorsDark(style);
                editor_sizes(style);
                dark_colors(&mut style.Colors);
                DARK_PALETTE
            }
        };

        sys::ImGuiStyle_ScaleAllSizes(style, dpi);
        style.FontSizeBase = appearance.font_size.clamp(*FONT_SIZES.start(), *FONT_SIZES.end());
        style.FontScaleMain = 1.0;
        style.FontScaleDpi = dpi;
        *PALETTE.write().unwrap_or_else(std::sync::PoisonError::into_inner) = palette;
    }
}
