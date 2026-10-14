//! The backends on a window: SDL3, DXVK's D3D11 on it, Dear ImGui's demo window with docking.
//!
//!   cargo run -p wce_imgui --example demo -- [frames shot.png]
//!
//! With a frame count and a file it saves the back buffer after that frame and exits (no input needed).

use std::path::PathBuf;

use dear_imgui_rs::{ConfigFlags, Context};
use sdl3_sys::events::{SDL_EVENT_QUIT, SDL_EVENT_WINDOW_CLOSE_REQUESTED, SDL_Event, SDL_EventType, SDL_PollEvent};
use sdl3_sys::init::{SDL_INIT_VIDEO, SDL_Init, SDL_Quit};
use sdl3_sys::video::{SDL_CreateWindow, SDL_DestroyWindow, SDL_WINDOW_RESIZABLE, SDL_WINDOW_VULKAN};
use wce_d3d11::*;
use wce_imgui::platform::Sdl3Platform;
use wce_imgui::renderer::D3D11Renderer;

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let shot = (args.len() >= 2).then(|| (args[0].parse::<u32>().unwrap_or(20), PathBuf::from(&args[1])));

    // SAFETY: no other thread runs yet; SDL and D3D calls with valid arguments; the window outlives the device.
    unsafe {
        init_platform();
        assert!(SDL_Init(SDL_INIT_VIDEO), "SDL_Init failed");
        let window = SDL_CreateWindow(c"wce_imgui demo".as_ptr(), 1280, 800, SDL_WINDOW_VULKAN | SDL_WINDOW_RESIZABLE);
        assert!(!window.is_null(), "SDL_CreateWindow failed");
        let sd = DXGI_SWAP_CHAIN_DESC {
            BufferDesc: DXGI_MODE_DESC { Format: DXGI_FORMAT_R8G8B8A8_UNORM, ..Default::default() }, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            BufferUsage: DXGI_USAGE(DXGI_USAGE_RENDER_TARGET_OUTPUT), BufferCount: 2, OutputWindow: HWND(window.cast()), Windowed: true.into(), SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD, Flags: 0,
        };

        let (mut swap, mut dev, mut ctx) = (None, None, None);
        D3D11CreateDeviceAndSwapChain(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), 0, Some(&[D3D_FEATURE_LEVEL_11_0]), D3D11_SDK_VERSION as u32, Some(&sd),
            Some(&mut swap), Some(&mut dev), None, Some(&mut ctx)).ok().expect("D3D11CreateDeviceAndSwapChain");
        let (swap, dev, ctx): (IDXGISwapChain, ID3D11Device, ID3D11DeviceContext) = (swap.unwrap(), dev.unwrap(), ctx.unwrap());

        let mut imgui = Context::create();
        let flags = imgui.io().config_flags() | ConfigFlags::DOCKING_ENABLE | ConfigFlags::NAV_ENABLE_KEYBOARD;
        imgui.io_mut().set_config_flags(flags);
        imgui.set_ini_filename(None::<PathBuf>).ok();
        let mut platform = Sdl3Platform::new(&mut imgui, window);
        let mut renderer = D3D11Renderer::new(&dev, &ctx, &mut imgui).expect("renderer");
        let back = |swap: &IDXGISwapChain| -> ID3D11Texture2D { swap.GetBuffer(0).expect("back buffer") };
        let mut frame = 0u32;
        'main: loop {
            let mut e = SDL_Event::default();
            while SDL_PollEvent(&mut e) {
                let ty = SDL_EventType(e.r#type);
                if ty == SDL_EVENT_QUIT || ty == SDL_EVENT_WINDOW_CLOSE_REQUESTED {
                    break 'main;
                }

                platform.process_event(&mut imgui, &e, true);
            }

            platform.new_frame(&mut imgui);
            let ui = imgui.frame();
            ui.dock_space_over_main_viewport_raw(dear_imgui_rs::Id::from(0u32), dear_imgui_rs::DockNodeFlags::PASSTHRU_CENTRAL_NODE);
            let mut open = true;
            ui.show_demo_window(&mut open);
            let pending = imgui.render(renderer.consumer());
            let bb = back(&swap);
            let mut rtv = None;
            dev.CreateRenderTargetView(&bb, None, Some(&mut rtv)).ok().expect("rtv");
            ctx.OMSetRenderTargets(Some(&[rtv.clone()]), None);
            ctx.ClearRenderTargetView(rtv.as_ref().unwrap(), &[0.08, 0.08, 0.09, 1.0]);
            renderer.render(pending).expect("render");
            frame += 1;
            if let Some((n, path)) = &shot
                && frame == *n
            {
                assert!(wce_render::image_io::save_texture_png(&dev, &ctx, &bb, path), "png");
                break;
            }

            let _ = swap.Present(1, 0);
        }

        // The C++ app's order: the backends, the context, then the swap chain and the device (they present to the window), then the window.
        drop(renderer);
        drop(platform);
        drop(imgui);
        drop(swap);
        drop(ctx);
        drop(dev);
        SDL_DestroyWindow(window);
        SDL_Quit();
    }
}
