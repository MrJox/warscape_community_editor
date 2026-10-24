//! Warscape Community Editor (wce): window, D3D11 device, Dear ImGui docking, the tool registry and the frame loop. An SDL3 window, on the
//! system D3D11 on Windows and on DXVK-native (D3D11 over Vulkan) on Linux.
//!
//! Command line (everything is a registry command underneath, see `help` in the Commands panel):
//!   --tool <id>             start in a tool (weather, tile, ...)
//!   --open <path>           open a file of the game packs (--open-disk for a file on disk)
//!   --cam <yaw> <pitch> <dist> [target_y]
//!   --cmd "<command line>"  run a command at start (repeatable)
//!   --script <file>         run a script (one command per line)
//!   --frames <n>            for --shot / --capture: the frame to take (default 20)
//!   --shot <file.png>       save the whole window after that frame and exit
//!   --capture <WxH> <file.png>   render the scene offscreen, no window involved, and exit
//!   --fixed-step <seconds>  simulate that much time per rendered frame (implied by --shot / --capture / --script)
//!   --live                  with --script: keep the live clock, the real mouse and background model loading (performance runs)
//!   --background / --foreground   scripted and --shot / --capture runs never take the focus (a window that cannot be focused); an
//!                           interactive run does. These flags (or WCE_FOREGROUND=1/0) override either default.
//!   --<name> args...        any other flag runs the command `name args...` (the old weather_editor and Tile Studio flags)

mod tools;

use std::path::PathBuf;
use std::time::Instant;

use dear_imgui_rs::{ConfigFlags, Context};
use sdl3_sys::events::*;
use sdl3_sys::hints::{SDL_HINT_WINDOW_ACTIVATE_WHEN_SHOWN, SDL_SetHint};
use sdl3_sys::init::{SDL_INIT_VIDEO, SDL_Init, SDL_Quit};
use sdl3_sys::messagebox::{SDL_MESSAGEBOX_ERROR, SDL_ShowSimpleMessageBox};
use sdl3_sys::video::*;
use wce_d3d11::*;
use wce_editor::Editor;
use wce_foundation::log;
use wce_foundation::settings::Paths;
use wce_imgui::platform::Sdl3Platform;
use wce_imgui::renderer::D3D11Renderer;
use wce_render::gpu_adapter::choose_adapter;
use wce_render::image_io::save_texture_png;

#[derive(Debug, Default)]
struct Options {
    tool: String,
    script_text: String,
    shot: String,
    capture_path: String,
    capture_w: i32,
    capture_h: i32,
    frames: i32,
    fixed_step: f64,

    /// Scripts keep the live clock, the real mouse and background loading (performance runs).
    live: bool,

    /// None: the window takes focus only for an interactive run; --foreground / --background (or WCE_FOREGROUND=1 / 0) force it.
    foreground: Option<bool>,
}

/// C's strtol / atoi on a flag's value: the leading integer, 0 when there is none.
fn to_int(s: &str) -> i32 {
    let end = s.char_indices().take_while(|&(i, c)| c.is_ascii_digit() || (i == 0 && (c == '-' || c == '+'))).count();
    s[..end].parse().unwrap_or(0)
}

fn parse_args(argv: &[String]) -> Options {
    let mut o = Options { frames: 20, ..Default::default() };
    let mut script = String::new();
    let mut add = |line: &str| {
        script += line;
        script.push('\n');
    };

    let argc = argv.len();
    let mut i = 1;
    let is_flag = |s: &str| s.starts_with("--");
    while i < argc {
        let a = argv[i].as_str();
        let more = i + 1 < argc;
        match a {
            "--tool" if more => {
                i += 1;
                o.tool = argv[i].clone();
            }

            "--open" if more => {
                i += 1;
                add(&format!("open \"{}\"\nframes 2", argv[i]));
            }

            // An entry file, a tile name, a folder or a tile.ted, and optionally the variation.
            "--tile" if more => {
                i += 1;
                let mut line = format!("tile \"{}\"", argv[i]);
                if i + 1 < argc && !is_flag(&argv[i + 1]) {
                    i += 1;
                    line += &format!(" \"{}\"", argv[i]);
                }

                add(&format!("{line}\nframes 2"));
            }

            "--open-disk" if more => {
                i += 1;
                add(&format!("open \"{}\" disk\nframes 2", argv[i]));
            }

            "--cam" if i + 3 < argc => {
                let mut line = format!("cam {} {} {}", argv[i + 1], argv[i + 2], argv[i + 3]);
                i += 3;
                if i + 1 < argc && !argv[i + 1].starts_with('-') {
                    i += 1;
                    line += &format!(" {}", argv[i]);
                }

                add(&line);
            }

            "--cmd" if more => {
                i += 1;
                add(&argv[i]);
            }

            "--script" if more => {
                i += 1;
                add(&format!("script \"{}\"", argv[i]));
            }

            "--frames" if more => {
                i += 1;
                o.frames = to_int(&argv[i]).max(1);
            }

            "--shot" if more => {
                i += 1;
                o.shot = argv[i].clone();
            }

            "--fixed-step" if more => {
                i += 1;
                o.fixed_step = argv[i].parse().unwrap_or(0.0);
            }

            "--live" => o.live = true,
            "--foreground" => o.foreground = Some(true),
            "--background" => o.foreground = Some(false),
            "--capture" if i + 2 < argc => {
                let dims = &argv[i + 1];
                o.capture_w = to_int(dims);
                o.capture_h = dims.find('x').map_or(0, |x| to_int(&dims[x + 1..]));
                o.capture_path = argv[i + 2].clone();
                i += 2;
            }

            // Any other --name args... runs the command `name args...` (the old weather_editor flags: --view, --blend, --pick, ...).
            _ if is_flag(a) => {
                let mut line = a[2..].to_owned();

                // The old Tile Studio flags whose names are taken by generic commands or use hyphens.
                const ALIAS: [(&str, &str); 9] = [
                    ("select", "tile_select"), ("move", "tile_move"), ("delete", "delete_item"), ("add", "add_building"), ("add-polygon", "add_polygon"),
                    ("polygon-point", "polygon_point"), ("new-layer", "new_layer"), ("move-to-layer", "move_to_layer"), ("save-to", "save_to"),
                ];
                if let Some((_, to)) = ALIAS.iter().find(|(from, _)| *from == line) {
                    line = (*to).to_owned();
                }

                while i + 1 < argc && !is_flag(&argv[i + 1]) {
                    i += 1;
                    line += &format!(" \"{}\"", argv[i]);
                }

                add(&line);
            }

            _ => {}
        }

        i += 1;
    }

    o.script_text = script;
    o
}

/// Scripted runs echo the log to stderr; an interactive run keeps it in <log folder>/last_run.log.
fn open_log(opt: &Options) {
    if !opt.shot.is_empty() || !opt.capture_path.is_empty() || !opt.script_text.is_empty() {
        log::echo_to_stderr(true);
        return;
    }

    let path = Paths::standard().log_dir().join("last_run.log");
    let opened = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| log::open_file(&path));
    if opened.is_err() {
        eprintln!("WCE could not create its interactive log at {}", path.display());
    }
}

/// Scripted and screenshot runs are unattended: their window must not take the focus from whatever the user is doing (a game, say).
fn window_takes_focus(opt: &Options) -> bool {
    if let Some(f) = opt.foreground {
        return f;
    }

    match std::env::var("WCE_FOREGROUND") {
        Ok(f) if !f.is_empty() => !f.starts_with('0'),
        _ => opt.shot.is_empty() && opt.capture_path.is_empty() && opt.script_text.is_empty(),
    }
}

/// What D3D11 presents to: the SDL window itself for DXVK-native, the Win32 window on Windows.
///
/// # Safety
///
/// `window` is a live SDL window.
unsafe fn output_window(window: *mut SDL_Window) -> HWND {
    #[cfg(windows)]
    {
        use sdl3_sys::properties::SDL_GetPointerProperty;
        // SAFETY: a live window's property set.
        unsafe { HWND(SDL_GetPointerProperty(SDL_GetWindowProperties(window), SDL_PROP_WINDOW_WIN32_HWND_POINTER, std::ptr::null_mut())) }
    }

    #[cfg(not(windows))]
    HWND(window.cast())
}

/// The device and swap chain on the chosen adapter. None when D3D11 is not available.
///
/// # Safety
///
/// After `init_platform`; `window` is live.
unsafe fn create_device(window: HWND) -> Option<(IDXGISwapChain, ID3D11Device, ID3D11DeviceContext)> {
    let sd = DXGI_SWAP_CHAIN_DESC {
        BufferDesc: DXGI_MODE_DESC { Format: DXGI_FORMAT_R8G8B8A8_UNORM, ..Default::default() }, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        BufferUsage: DXGI_USAGE(DXGI_USAGE_RENDER_TARGET_OUTPUT), BufferCount: 2, OutputWindow: window, Windowed: true.into(), SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
        Flags: 0,
    };

    // Needs the Windows "Graphics Tools" feature.
    let flags = if std::env::var("WCE_D3D_DEBUG").is_ok_and(|d| d.starts_with('1')) { D3D11_CREATE_DEVICE_DEBUG as u32 } else { 0 };

    // The same GPU every run (docs/baseline.md).
    // SAFETY: after init_platform (the caller's contract).
    let adapter = unsafe { choose_adapter() };
    let driver = if adapter.is_some() { D3D_DRIVER_TYPE_UNKNOWN } else { D3D_DRIVER_TYPE_HARDWARE };
    let (mut swap, mut dev, mut ctx) = (None, None, None);
    // SAFETY: valid descriptions and out pointers.
    unsafe {
        D3D11CreateDeviceAndSwapChain(
            adapter.as_ref(), driver, HMODULE::default(), flags, Some(&[D3D_FEATURE_LEVEL_11_0]), D3D11_SDK_VERSION as u32, Some(&sd), Some(&mut swap),
            Some(&mut dev), None, Some(&mut ctx),
        )
        .ok()
        .ok()?;
    }

    Some((swap?, dev?, ctx?))
}

/// Docking, keyboard navigation and the fonts; the editor applies the theme at the display's scale. No ini file: layouts are files per tool
/// (see `Editor::save_layout`).
fn init_imgui() -> Context {
    let mut imgui = Context::create();
    let flags = imgui.io().config_flags() | ConfigFlags::DOCKING_ENABLE | ConfigFlags::NAV_ENABLE_KEYBOARD;
    imgui.io_mut().set_config_flags(flags);
    imgui.set_ini_filename(None::<PathBuf>).ok();
    // SAFETY: the context was just created and is the current one.
    unsafe { wce_editor::theme::load_fonts() };

    imgui
}

/// The window's state the event pump updates.
#[derive(Default)]
struct WindowEvents {
    resize: Option<(u32, u32)>,
}

/// One SDL event: Dear ImGui gets it (not the real mouse in a scripted run), and resize / close requests are noted. True to quit.
fn handle_event(window: *mut SDL_Window, e: &SDL_Event, editor: &mut Editor, imgui: &mut Context, platform: &mut Sdl3Platform, state: &mut WindowEvents) -> bool {
    platform.process_event(imgui, e, !editor.scripted_input());
    // SAFETY: the event type selects the union member read.
    unsafe {
        let ty = SDL_EventType(e.r#type);
        if ty == SDL_EVENT_WINDOW_PIXEL_SIZE_CHANGED {
            if e.window.data1 > 0 && e.window.data2 > 0 {
                state.resize = Some((e.window.data1 as u32, e.window.data2 as u32));
            }
        } else if (ty == SDL_EVENT_WINDOW_CLOSE_REQUESTED && e.window.windowID == SDL_GetWindowID(window)) || ty == SDL_EVENT_QUIT {
            // Unsaved changes: the editor asks first (it quits itself with the answer).
            return !editor.window_close_requested();
        }
    }

    false
}

/// The editor with its tools, the command line as a script, and the frame loop until the editor or the window quits.
///
/// # Safety
///
/// `window` is live; the device presents to it.
unsafe fn run_editor(opt: &Options, window: *mut SDL_Window, swap: &IDXGISwapChain, dev: &ID3D11Device, ctx: &ID3D11DeviceContext, imgui: &mut Context, dpi: f32) -> i32 {
    let mut exit_code = 0;
    // SAFETY: the window outlives the backend (the caller's contract).
    let mut platform = unsafe { Sdl3Platform::new(imgui, window) };
    let mut renderer = match D3D11Renderer::new(dev, ctx, imgui) {
        Ok(r) => r,
        Err(e) => {
            log::error(format!("cannot create the Dear ImGui renderer: {e}"));
            return 1;
        }
    };

    let mut editor = Editor::new(dev, ctx);
    editor.set_display_scale(dpi);
    for tool in tools::all_tools() {
        editor.add_tool(tool);
    }

    editor.start();

    // Everything the command line asked for becomes a script; `frames` lines give the editor time to render in between.
    let mut script = String::new();
    if !opt.tool.is_empty() {
        script += &format!("tool {}\nframes 2\n", opt.tool);
    }

    script += &opt.script_text;
    let batch = !opt.shot.is_empty() || !opt.capture_path.is_empty();

    // A plain interactive start opens the tool of the last session; --tool, or anything scripted, starts as it says.
    if opt.tool.is_empty() && opt.script_text.is_empty() && !batch {
        editor.restore_last_tool();
    }
    if !opt.capture_path.is_empty() {
        script += &format!("frames 2\ncapture {}x{} \"{}\" {}\nquit\n", opt.capture_w, opt.capture_h, opt.capture_path, opt.frames);
    } else if !opt.shot.is_empty() {
        script += &format!("frames {}\nshot \"{}\"\nquit\n", opt.frames, opt.shot);
    }

    if !script.is_empty() {
        editor.script().load_text(&script);
    }

    let mut step = opt.fixed_step;
    if step <= 0.0 && !opt.live && (batch || !opt.script_text.is_empty()) {
        step = 1.0 / 60.0;
    }

    if step > 0.0 {
        editor.set_fixed_step(step);

        // Scripted runs: a held key must not auto-repeat when a frame happens to take long.
        imgui.io_mut().set_key_repeat_delay(1.0e6);
    }

    if std::env::var("WCE_VSYNC").is_ok_and(|v| v.starts_with('0')) {
        editor.vsync = false;
    }

    let mut state = WindowEvents::default();
    let mut last_frame = Instant::now();
    // SAFETY: D3D calls on live objects; SDL calls on the live window.
    unsafe {
        let mut rtv = back_buffer_view(swap, dev);
        loop {
            let frame_start = Instant::now();
            let mut quit = false;
            let mut e = SDL_Event::default();
            while SDL_PollEvent(&mut e) {
                quit = handle_event(window, &e, &mut editor, imgui, &mut platform, &mut state) || quit;
            }

            if quit {
                break;
            }

            if let Some((w, h)) = state.resize.take() {
                rtv = None;
                let _ = swap.ResizeBuffers(0, w, h, DXGI_FORMAT_UNKNOWN, 0);
                rtv = back_buffer_view(swap, dev).or(rtv);
            }

            editor.pre_frame();
            platform.new_frame(imgui);

            // Scripted runs: the `mouse` commands replace the real cursor (a hovered splitter changes a shot).
            editor.inject_input();
            let _ui = imgui.frame();
            editor.frame(&mut renderer);
            let pending = imgui.render(renderer.consumer());
            ctx.OMSetRenderTargets(Some(&[rtv.clone()]), None);
            if let Some(rtv) = &rtv {
                ctx.ClearRenderTargetView(rtv, &[0.08, 0.08, 0.09, 1.0]);
            }

            if let Err(e) = renderer.render(pending) {
                log::error(format!("Dear ImGui render: {e}"));
            }

            if let Some(path) = editor.pending_shot.take() {
                let saved = swap.GetBuffer::<ID3D11Texture2D>(0).ok().is_some_and(|back| save_texture_png(dev, ctx, &back, &path));
                if !saved {
                    log::error(format!("cannot write {}", path.display()));
                }
            }

            if editor.wants_quit() {
                exit_code = editor.exit_code();
                break;
            }

            let cpu_end = Instant::now();
            let _ = swap.Present(if batch || !editor.vsync { 0 } else { 1 }, 0);

            // Interactive frames wait out the rest of their share of the cap; shots, captures and frames of a running script (the startup
            // commands, perf measurements) run free.
            if !batch && editor.script().done() && editor.frame_cap() > 0 {
                let budget = std::time::Duration::from_secs_f64(1.0 / f64::from(editor.frame_cap()));
                if let Some(rest) = budget.checked_sub(frame_start.elapsed()) {
                    std::thread::sleep(rest);
                }
            }

            let now = Instant::now();
            editor.perf.record(now.duration_since(last_frame).as_secs_f64() * 1e3, cpu_end.duration_since(frame_start).as_secs_f64() * 1e3);
            last_frame = now;
        }
    }

    if !batch {
        editor.save_layout();
    }

    // The editor's GPU objects before the backends, the backends before the context.
    drop(editor);
    drop(renderer);
    drop(platform);
    exit_code
}

/// A render target view of the swap chain's back buffer.
///
/// # Safety
///
/// Live swap chain and device.
unsafe fn back_buffer_view(swap: &IDXGISwapChain, dev: &ID3D11Device) -> Option<ID3D11RenderTargetView> {
    // SAFETY: the caller's contract.
    unsafe {
        let back: ID3D11Texture2D = swap.GetBuffer(0).ok()?;
        let mut rtv = None;
        dev.CreateRenderTargetView(&back, None, Some(&mut rtv)).ok().ok()?;
        rtv
    }
}

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let opt = parse_args(&args);
    open_log(&opt);

    // SAFETY: no other thread runs yet; SDL and D3D calls with valid arguments; the window outlives the device.
    let code = unsafe {
        init_platform();

        // An unattended run's window cannot take the focus and is not activated when shown.
        let take_focus = window_takes_focus(&opt);
        if !take_focus {
            SDL_SetHint(SDL_HINT_WINDOW_ACTIVATE_WHEN_SHOWN, c"0".as_ptr());
        }

        if !SDL_Init(SDL_INIT_VIDEO) {
            eprintln!("SDL_Init failed: {}", std::ffi::CStr::from_ptr(sdl3_sys::error::SDL_GetError()).to_string_lossy());
            std::process::exit(1);
        }

        // A high pixel density window (interactive runs): on a scaled Wayland output the back buffer has the output's pixels (Dear ImGui works in logical units
        // with a framebuffer scale, and rasterizes its fonts at that density), instead of the compositor stretching a logical-size image.
        // Under X11 the content scale sizes the UI like the Windows DPI scale does.
        // Scripted runs keep one pixel per unit: their mouse positions and shots must not depend on the display's scale.
        let scripted = opt.fixed_step > 0.0 || (!opt.live && (!opt.shot.is_empty() || !opt.capture_path.is_empty() || !opt.script_text.is_empty()));
        let density = if scripted { SDL_WindowFlags(0) } else { SDL_WINDOW_HIGH_PIXEL_DENSITY };
        let wflags = SDL_WINDOW_VULKAN | SDL_WINDOW_RESIZABLE | density | if take_focus { SDL_WindowFlags(0) } else { SDL_WINDOW_NOT_FOCUSABLE };
        let window = SDL_CreateWindow(c"Warscape Community Editor".as_ptr(), 1600, 960, wflags);
        if window.is_null() {
            eprintln!("SDL_CreateWindow failed: {}", std::ffi::CStr::from_ptr(sdl3_sys::error::SDL_GetError()).to_string_lossy());
            SDL_Quit();
            std::process::exit(1);
        }

        let Some((swap, dev, ctx)) = create_device(output_window(window)) else {
            SDL_ShowSimpleMessageBox(SDL_MESSAGEBOX_ERROR, c"Warscape Community Editor".as_ptr(), c"Could not create a Direct3D 11 device (DXVK needs a Vulkan driver).".as_ptr(), window);
            SDL_DestroyWindow(window);
            SDL_Quit();
            std::process::exit(1);
        };

        let video = SDL_GetCurrentVideoDriver();
        let wayland = !video.is_null() && std::ffi::CStr::from_ptr(video).to_bytes() == b"wayland";
        let mut dpi = if wayland { 1.0 } else { SDL_GetWindowDisplayScale(window) };
        if dpi <= 0.0 {
            dpi = 1.0;
        }

        let mut imgui = init_imgui();
        let code = run_editor(&opt, window, &swap, &dev, &ctx, &mut imgui, dpi);

        // The C++ app's order: the context, then the swap chain and the device (they present to the window), then the window.
        drop(imgui);
        drop(swap);
        drop(ctx);
        drop(dev);
        SDL_DestroyWindow(window);
        SDL_Quit();
        code
    };

    std::process::exit(code);
}
