//! The SDL3 platform backend: a port of Dear ImGui's `imgui_impl_sdl3.cpp` for one window (no platform viewports): events into the IO, the
//! display size and frame time, the mouse cursor shape, text input and the clipboard.

use std::ffi::{CStr, CString};

use dear_imgui_rs::input::MouseButton;
use dear_imgui_rs::{Context, Key};
use sdl3_sys::clipboard::{SDL_GetClipboardText, SDL_SetClipboardText};
use sdl3_sys::events::*;
use sdl3_sys::keyboard::{SDL_StartTextInput, SDL_StopTextInput, SDL_TextInputActive};
use sdl3_sys::keycode::*;
use sdl3_sys::mouse::*;
use sdl3_sys::scancode::*;
use sdl3_sys::stdinc::SDL_free;
use sdl3_sys::timer::{SDL_GetPerformanceCounter, SDL_GetPerformanceFrequency};
use sdl3_sys::video::{SDL_GetWindowID, SDL_GetWindowSize, SDL_GetWindowSizeInPixels, SDL_Window};

/// The ImGui key of a key event; keypad keys by scancode first, then the key code, then the scancode for the punctuation SDL's key codes do
/// not name the same on every layout.
fn imgui_key(keycode: SDL_Keycode, scancode: SDL_Scancode) -> Option<Key> {
    let by_scancode = match scancode {
        SDL_SCANCODE_KP_0 => Some(Key::Keypad0),
        SDL_SCANCODE_KP_1 => Some(Key::Keypad1),
        SDL_SCANCODE_KP_2 => Some(Key::Keypad2),
        SDL_SCANCODE_KP_3 => Some(Key::Keypad3),
        SDL_SCANCODE_KP_4 => Some(Key::Keypad4),
        SDL_SCANCODE_KP_5 => Some(Key::Keypad5),
        SDL_SCANCODE_KP_6 => Some(Key::Keypad6),
        SDL_SCANCODE_KP_7 => Some(Key::Keypad7),
        SDL_SCANCODE_KP_8 => Some(Key::Keypad8),
        SDL_SCANCODE_KP_9 => Some(Key::Keypad9),
        SDL_SCANCODE_KP_PERIOD => Some(Key::KeypadDecimal),
        SDL_SCANCODE_KP_DIVIDE => Some(Key::KeypadDivide),
        SDL_SCANCODE_KP_MULTIPLY => Some(Key::KeypadMultiply),
        SDL_SCANCODE_KP_MINUS => Some(Key::KeypadSubtract),
        SDL_SCANCODE_KP_PLUS => Some(Key::KeypadAdd),
        SDL_SCANCODE_KP_ENTER => Some(Key::KeypadEnter),
        SDL_SCANCODE_KP_EQUALS => Some(Key::KeypadEqual),
        _ => None,
    };

    if by_scancode.is_some() {
        return by_scancode;
    }

    const LETTERS: [Key; 26] = [
        Key::A, Key::B, Key::C, Key::D, Key::E, Key::F, Key::G, Key::H, Key::I, Key::J, Key::K, Key::L, Key::M, Key::N, Key::O, Key::P, Key::Q, Key::R, Key::S, Key::T,
        Key::U, Key::V, Key::W, Key::X, Key::Y, Key::Z,
    ];
    const DIGITS: [Key; 10] = [Key::Key0, Key::Key1, Key::Key2, Key::Key3, Key::Key4, Key::Key5, Key::Key6, Key::Key7, Key::Key8, Key::Key9];
    const FUNCTIONS: [Key; 24] = [
        Key::F1, Key::F2, Key::F3, Key::F4, Key::F5, Key::F6, Key::F7, Key::F8, Key::F9, Key::F10, Key::F11, Key::F12, Key::F13, Key::F14, Key::F15, Key::F16, Key::F17,
        Key::F18, Key::F19, Key::F20, Key::F21, Key::F22, Key::F23, Key::F24,
    ];

    let k = keycode.0;
    if (SDLK_A.0..=SDLK_Z.0).contains(&k) {
        return Some(LETTERS[(k - SDLK_A.0) as usize]);
    }

    if (SDLK_0.0..=SDLK_9.0).contains(&k) {
        return Some(DIGITS[(k - SDLK_0.0) as usize]);
    }

    if (SDLK_F1.0..=SDLK_F12.0).contains(&k) {
        return Some(FUNCTIONS[(k - SDLK_F1.0) as usize]);
    }

    if (SDLK_F13.0..=SDLK_F24.0).contains(&k) {
        return Some(FUNCTIONS[12 + (k - SDLK_F13.0) as usize]);
    }

    let by_keycode = match keycode {
        SDLK_TAB => Some(Key::Tab),
        SDLK_LEFT => Some(Key::LeftArrow),
        SDLK_RIGHT => Some(Key::RightArrow),
        SDLK_UP => Some(Key::UpArrow),
        SDLK_DOWN => Some(Key::DownArrow),
        SDLK_PAGEUP => Some(Key::PageUp),
        SDLK_PAGEDOWN => Some(Key::PageDown),
        SDLK_HOME => Some(Key::Home),
        SDLK_END => Some(Key::End),
        SDLK_INSERT => Some(Key::Insert),
        SDLK_DELETE => Some(Key::Delete),
        SDLK_BACKSPACE => Some(Key::Backspace),
        SDLK_SPACE => Some(Key::Space),
        SDLK_RETURN => Some(Key::Enter),
        SDLK_ESCAPE => Some(Key::Escape),
        SDLK_COMMA => Some(Key::Comma),
        SDLK_PERIOD => Some(Key::Period),
        SDLK_SEMICOLON => Some(Key::Semicolon),
        SDLK_CAPSLOCK => Some(Key::CapsLock),
        SDLK_SCROLLLOCK => Some(Key::ScrollLock),
        SDLK_NUMLOCKCLEAR => Some(Key::NumLock),
        SDLK_PRINTSCREEN => Some(Key::PrintScreen),
        SDLK_PAUSE => Some(Key::Pause),
        SDLK_LCTRL => Some(Key::LeftCtrl),
        SDLK_LSHIFT => Some(Key::LeftShift),
        SDLK_LALT => Some(Key::LeftAlt),
        SDLK_LGUI => Some(Key::LeftSuper),
        SDLK_RCTRL => Some(Key::RightCtrl),
        SDLK_RSHIFT => Some(Key::RightShift),
        SDLK_RALT => Some(Key::RightAlt),
        SDLK_RGUI => Some(Key::RightSuper),
        SDLK_APPLICATION => Some(Key::Menu),
        SDLK_AC_BACK => Some(Key::AppBack),
        SDLK_AC_FORWARD => Some(Key::AppForward),
        _ => None,
    };

    if by_keycode.is_some() {
        return by_keycode;
    }

    match scancode {
        SDL_SCANCODE_GRAVE => Some(Key::GraveAccent),
        SDL_SCANCODE_MINUS => Some(Key::Minus),
        SDL_SCANCODE_EQUALS => Some(Key::Equal),
        SDL_SCANCODE_LEFTBRACKET => Some(Key::LeftBracket),
        SDL_SCANCODE_RIGHTBRACKET => Some(Key::RightBracket),
        SDL_SCANCODE_NONUSBACKSLASH => Some(Key::Oem102),
        SDL_SCANCODE_BACKSLASH => Some(Key::Backslash),
        SDL_SCANCODE_SEMICOLON => Some(Key::Semicolon),
        SDL_SCANCODE_APOSTROPHE => Some(Key::Apostrophe),
        SDL_SCANCODE_COMMA => Some(Key::Comma),
        SDL_SCANCODE_PERIOD => Some(Key::Period),
        SDL_SCANCODE_SLASH => Some(Key::Slash),
        _ => None,
    }
}

/// The system clipboard through SDL.
struct SdlClipboard;

impl dear_imgui_rs::ClipboardBackend for SdlClipboard {
    fn get(&mut self) -> Option<String> {
        // SAFETY: SDL returns an owned string (or null) that is freed here.
        unsafe {
            let text = SDL_GetClipboardText();
            if text.is_null() {
                return None;
            }

            let s = CStr::from_ptr(text).to_string_lossy().into_owned();
            SDL_free(text.cast());
            Some(s)
        }
    }

    fn set(&mut self, value: &str) {
        if let Ok(c) = CString::new(value) {
            // SAFETY: SDL copies the string.
            unsafe { SDL_SetClipboardText(c.as_ptr()) };
        }
    }
}

pub struct Sdl3Platform {
    window: *mut SDL_Window,
    time: u64,
    cursors: Vec<*mut SDL_Cursor>,
    last_cursor: Option<i32>,
    mouse_pending_leave_frame: i32,
    frame: i32,
}

impl Sdl3Platform {

    /// The backend for one window.
    ///
    /// # Safety
    ///
    /// `window` is a live SDL window that outlives the backend.
    pub unsafe fn new(imgui: &mut Context, window: *mut SDL_Window) -> Self {
        let io = imgui.io_mut();
        io.set_backend_flags(io.backend_flags() | dear_imgui_rs::BackendFlags::HAS_MOUSE_CURSORS);
        let _ = imgui.set_platform_name(Some("wce_sdl3"));
        imgui.set_clipboard_backend(SdlClipboard);
        let shapes = [
            SDL_SYSTEM_CURSOR_DEFAULT, SDL_SYSTEM_CURSOR_TEXT, SDL_SYSTEM_CURSOR_MOVE, SDL_SYSTEM_CURSOR_NS_RESIZE, SDL_SYSTEM_CURSOR_EW_RESIZE, SDL_SYSTEM_CURSOR_NESW_RESIZE,
            SDL_SYSTEM_CURSOR_NWSE_RESIZE, SDL_SYSTEM_CURSOR_POINTER, SDL_SYSTEM_CURSOR_WAIT, SDL_SYSTEM_CURSOR_PROGRESS, SDL_SYSTEM_CURSOR_NOT_ALLOWED,
        ];

        // SAFETY: plain SDL calls.
        let cursors = shapes.iter().map(|&s| unsafe { SDL_CreateSystemCursor(s) }).collect();
        Self { window, time: 0, cursors, last_cursor: Some(-2), mouse_pending_leave_frame: 0, frame: 0 }
    }

    /// Feeds one event to the IO; false when it is not this window's. `real_mouse` false drops the mouse events (scripted runs move a mouse of
    /// their own).
    pub fn process_event(&mut self, imgui: &mut Context, e: &SDL_Event, real_mouse: bool) -> bool {
        // SAFETY: the union member read is the one the event type names; the window outlives the backend.
        unsafe {
            let ty = SDL_EventType(e.r#type);
            let mine = |id: sdl3_sys::video::SDL_WindowID| id == SDL_GetWindowID(self.window);
            let io = imgui.io_mut();
            match ty {
                SDL_EVENT_MOUSE_MOTION if real_mouse && mine(e.motion.windowID) => {
                    io.add_mouse_pos_event([e.motion.x, e.motion.y]);
                    true
                }

                SDL_EVENT_MOUSE_WHEEL if real_mouse && mine(e.wheel.windowID) => {
                    io.add_mouse_wheel_event([-e.wheel.x, e.wheel.y]);
                    true
                }

                SDL_EVENT_MOUSE_BUTTON_DOWN | SDL_EVENT_MOUSE_BUTTON_UP if real_mouse && mine(e.button.windowID) => {
                    let button = match i32::from(e.button.button) {
                        SDL_BUTTON_LEFT => MouseButton::Left,
                        SDL_BUTTON_RIGHT => MouseButton::Right,
                        SDL_BUTTON_MIDDLE => MouseButton::Middle,
                        SDL_BUTTON_X1 => MouseButton::Extra1,
                        SDL_BUTTON_X2 => MouseButton::Extra2,
                        _ => return false,
                    };

                    io.add_mouse_button_event(button, ty == SDL_EVENT_MOUSE_BUTTON_DOWN);
                    true
                }

                SDL_EVENT_TEXT_INPUT if mine(e.text.windowID) => {
                    if !e.text.text.is_null() {
                        io.add_input_characters_utf8(CStr::from_ptr(e.text.text).to_string_lossy());
                    }

                    true
                }

                SDL_EVENT_KEY_DOWN | SDL_EVENT_KEY_UP if mine(e.key.windowID) => {
                    let m = e.key.r#mod;
                    io.add_key_event(Key::ModCtrl, (m & SDL_KMOD_CTRL) != 0);
                    io.add_key_event(Key::ModShift, (m & SDL_KMOD_SHIFT) != 0);
                    io.add_key_event(Key::ModAlt, (m & SDL_KMOD_ALT) != 0);
                    io.add_key_event(Key::ModSuper, (m & SDL_KMOD_GUI) != 0);
                    if let Some(key) = imgui_key(e.key.key, e.key.scancode) {
                        io.add_key_event(key, ty == SDL_EVENT_KEY_DOWN);
                    }

                    true
                }

                SDL_EVENT_WINDOW_MOUSE_ENTER if mine(e.window.windowID) => {
                    self.mouse_pending_leave_frame = 0;
                    true
                }

                // Delayed by one frame: SDL may send the enter of another window one frame late.
                SDL_EVENT_WINDOW_MOUSE_LEAVE if mine(e.window.windowID) => {
                    self.mouse_pending_leave_frame = self.frame + 1;
                    true
                }

                SDL_EVENT_WINDOW_FOCUS_GAINED | SDL_EVENT_WINDOW_FOCUS_LOST if mine(e.window.windowID) => {
                    io.add_focus_event(ty == SDL_EVENT_WINDOW_FOCUS_GAINED);
                    true
                }

                _ => false,
            }
        }
    }

    /// Before a frame: display size, frame time, the mouse leaving, the cursor shape and text input.
    pub fn new_frame(&mut self, imgui: &mut Context) {
        self.frame += 1;
        let (mut w, mut h, mut pw, mut ph) = (0, 0, 0, 0);
        // SAFETY: plain SDL calls on the live window.
        let (now, freq) = unsafe {
            SDL_GetWindowSize(self.window, &mut w, &mut h);
            SDL_GetWindowSizeInPixels(self.window, &mut pw, &mut ph);
            (SDL_GetPerformanceCounter(), SDL_GetPerformanceFrequency())
        };

        let dt = if self.time > 0 { (now - self.time) as f64 / freq as f64 } else { 1.0 / 60.0 };
        self.time = now;
        let io = imgui.io_mut();
        io.set_display_size([w as f32, h as f32]);
        if w > 0 && h > 0 {
            io.set_display_framebuffer_scale([pw as f32 / w as f32, ph as f32 / h as f32]);
        }

        io.set_delta_time(if dt > 0.0 { dt as f32 } else { 1.0 / 60.0 });
        if self.mouse_pending_leave_frame != 0 && self.mouse_pending_leave_frame >= self.frame {
            io.add_mouse_pos_event([-f32::MAX, -f32::MAX]);
            self.mouse_pending_leave_frame = 0;
        }

        // SAFETY: plain SDL calls on the live window.
        unsafe {
            let wants_text = io.want_text_input();
            if wants_text && !SDL_TextInputActive(self.window) {
                SDL_StartTextInput(self.window);
            } else if !wants_text && SDL_TextInputActive(self.window) {
                SDL_StopTextInput(self.window);
            }
        }

        // SAFETY: the context is current (the caller's frames run on it).
        let cursor = match unsafe { dear_imgui_sys::igGetMouseCursor() } {
            c if c < 0 => None,
            c => Some(c),
        };
        if cursor != self.last_cursor {
            self.last_cursor = cursor;
            // SAFETY: plain SDL calls with the cursors made in new().
            unsafe {
                match cursor {
                    None => {
                        SDL_HideCursor();
                    }

                    Some(c) => {
                        let i = c.clamp(0, self.cursors.len() as i32 - 1) as usize;
                        SDL_SetCursor(self.cursors[i]);
                        SDL_ShowCursor();
                    }
                }
            }
        }
    }
}

impl Drop for Sdl3Platform {
    fn drop(&mut self) {
        for &c in &self.cursors {
            // SAFETY: the cursors were made by SDL_CreateSystemCursor.
            unsafe { SDL_DestroyCursor(c) };
        }
    }
}
