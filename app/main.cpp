// Warscape Community Editor (wce.exe): window, D3D11 device, ImGui docking, the tool registry and the frame loop.
// Windows: a Win32 window on the system D3D11. Linux: an SDL3 window on DXVK-native (D3D11 over Vulkan).
//
// Command line (everything is a registry command underneath, see `help` in the Commands panel):
//   --tool <id>             start in a tool (weather, tile, ...)
//   --open <path>           open a file of the game packs (--open-disk for a file on disk)
//   --cam <yaw> <pitch> <dist> [target_y]
//   --cmd "<command line>"  run a command at start (repeatable)
//   --script <file>         run a script (one command per line)
//   --frames <n>            for --shot / --capture: the frame to take (default 20)
//   --shot <file.png>       save the whole window after that frame and exit
//   --capture <WxH> <file.png>   render the scene offscreen, no window involved, and exit
//   --fixed-step <seconds>  simulate that much time per rendered frame (implied by --shot / --capture / --script)
//   --live                  with --script: keep the live clock, the real mouse and background model loading (performance runs)
//   --background / --foreground   scripted and --shot / --capture runs never take the focus (non-activating window behind the others); an interactive run does.
//                           These flags (or WCE_FOREGROUND=1/0) override either default.
#include "foundation/log.h"
#include "foundation/process.h"
#include "foundation/settings.h"
#include "foundation/profile.h"
#include "editor/editor.h"
#include "render/gpu_adapter.h"
#include "render/image_io.h"
#include "tool_factories.h"

#include "imgui.h"
#include "imgui_impl_dx11.h"
#ifdef _WIN32
#include "imgui_impl_win32.h"
#else
#include "imgui_impl_sdl3.h"
#endif

#include <d3d11.h>
#ifdef _WIN32
#include <d3d11sdklayers.h>
#include <windows.h>
#include <dbghelp.h>
#include <shellapi.h>
#else
#include <SDL3/SDL.h>
#endif
#include <wrl/client.h>

#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cwchar>
#include <filesystem>
#include <string>
#include <vector>

using Microsoft::WRL::ComPtr;

#ifdef _WIN32

// Writes %LOCALAPPDATA%\awe\crash.dmp and crash.txt (exception code + module offset) on a crash.
static LONG WINAPI crash_handler(EXCEPTION_POINTERS* ep) {
    wchar_t dir[MAX_PATH];
    DWORD n = GetEnvironmentVariableW(L"LOCALAPPDATA", dir, MAX_PATH);
    std::wstring base = (n ? std::wstring(dir) : std::wstring(L".")) + L"\\awe\\";
    CreateDirectoryW(base.c_str(), nullptr);
    HANDLE f = CreateFileW((base + L"crash.dmp").c_str(), GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS, 0, nullptr);
    if (f != INVALID_HANDLE_VALUE) {
        MINIDUMP_EXCEPTION_INFORMATION mi{GetCurrentThreadId(), ep, FALSE};
        MiniDumpWriteDump(GetCurrentProcess(), GetCurrentProcessId(), f, MiniDumpWithIndirectlyReferencedMemory, &mi, nullptr, nullptr);
        CloseHandle(f);
    }
    HMODULE mod = nullptr;
    GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                       static_cast<LPCWSTR>(ep->ExceptionRecord->ExceptionAddress), &mod);
    wchar_t mname[MAX_PATH] = L"?";
    if (mod) GetModuleFileNameW(mod, mname, MAX_PATH);
    FILE* t = _wfopen((base + L"crash.txt").c_str(), L"w");
    if (t) {
        fwprintf(t, L"code %08lx at %ls+0x%llx\n", ep->ExceptionRecord->ExceptionCode, mname,
                 (unsigned long long)((char*)ep->ExceptionRecord->ExceptionAddress - (char*)mod));
        fclose(t);
    }
    return EXCEPTION_EXECUTE_HANDLER;
}

extern IMGUI_IMPL_API LRESULT ImGui_ImplWin32_WndProcHandler(HWND, UINT, WPARAM, LPARAM);
#endif

namespace {

ComPtr<ID3D11Device> g_device;
ComPtr<ID3D11DeviceContext> g_ctx;
ComPtr<IDXGISwapChain> g_swap;
ComPtr<ID3D11RenderTargetView> g_rtv;
UINT g_resize_w = 0, g_resize_h = 0;
wce::Editor* g_editor = nullptr;

void create_rtv() {
    ComPtr<ID3D11Texture2D> back;
    g_swap->GetBuffer(0, IID_PPV_ARGS(&back));
    g_device->CreateRenderTargetView(back.Get(), nullptr, &g_rtv);
}

#ifdef _WIN32
LRESULT WINAPI wnd_proc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp) {
    // scripted runs ignore the real mouse (Editor::inject_input replaces it)
    const bool real_mouse = msg >= WM_MOUSEFIRST && msg <= WM_MOUSELAST;
    if (!(real_mouse && g_editor && g_editor->scripted_input()) && ImGui_ImplWin32_WndProcHandler(hwnd, msg, wp, lp)) return true;
    switch (msg) {
    case WM_SIZE:
        if (wp != SIZE_MINIMIZED) {
            g_resize_w = LOWORD(lp);
            g_resize_h = HIWORD(lp);
        }
        return 0;
    case WM_SYSCOMMAND:
        if ((wp & 0xfff0) == SC_KEYMENU) return 0;
        break;
    case WM_CLOSE:   // unsaved changes: the editor asks first (it quits itself with the answer)
        if (g_editor && g_editor->window_close_requested()) return 0;
        break;
    case WM_DESTROY:
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcW(hwnd, msg, wp, lp);
}
#else
// One SDL event: ImGui gets it (not the real mouse in a scripted run), and resize / close requests are noted. Returns true to quit.
bool handle_event(SDL_Window* window, const SDL_Event& e) {
    const bool real_mouse = e.type == SDL_EVENT_MOUSE_MOTION || e.type == SDL_EVENT_MOUSE_BUTTON_DOWN || e.type == SDL_EVENT_MOUSE_BUTTON_UP ||
                            e.type == SDL_EVENT_MOUSE_WHEEL;
    if (!(real_mouse && g_editor && g_editor->scripted_input())) ImGui_ImplSDL3_ProcessEvent(&e);
    switch (e.type) {
    case SDL_EVENT_WINDOW_PIXEL_SIZE_CHANGED:
        if (e.window.data1 > 0 && e.window.data2 > 0) {
            g_resize_w = UINT(e.window.data1);
            g_resize_h = UINT(e.window.data2);
        }
        break;
    case SDL_EVENT_WINDOW_CLOSE_REQUESTED:   // unsaved changes: the editor asks first (it quits itself with the answer)
        if (e.window.windowID != SDL_GetWindowID(window)) break;
        [[fallthrough]];
    case SDL_EVENT_QUIT:
        return !(g_editor && g_editor->window_close_requested());
    }
    return false;
}
#endif

struct Options {
    std::string tool, script_text;
    std::wstring shot, capture_path;
    int capture_w = 0, capture_h = 0, frames = 20;
    double fixed_step = 0;
    bool live = false;   // scripts keep the live clock, the real mouse and background loading (performance runs)
    int foreground = -1; // -1: the window takes focus only for an interactive run; --foreground / --background (or WCE_FOREGROUND=1 / 0) force it
};

Options parse_args(const std::vector<std::wstring>& argv) {
    Options o;
    const int argc = int(argv.size());
    auto utf8 = [&](int i) { return wce::narrow(argv[i]); };
    auto to_int = [](const std::wstring& s) { return int(std::wcstol(s.c_str(), nullptr, 10)); };
    auto add = [&](const std::string& line) { o.script_text += line + "\n"; };
    for (int i = 1; i < argc; ++i) {
        const std::wstring a = argv[i];
        const bool more = i + 1 < argc;
        if (a == L"--tool" && more) o.tool = utf8(++i);
        else if (a == L"--open" && more) add("open \"" + utf8(++i) + "\"\nframes 2");
        else if (a == L"--tile" && more) {   // an entry file, a tile name, a folder or a tile.ted, and optionally the variation
            std::string line = "tile \"" + utf8(++i) + "\"";
            if (i + 1 < argc && std::wstring(argv[i + 1]).rfind(L"--", 0) != 0) line += " \"" + utf8(++i) + "\"";
            add(line + "\nframes 2");
        }
        else if (a == L"--open-disk" && more) add("open \"" + utf8(++i) + "\" disk\nframes 2");
        else if (a == L"--cam" && i + 3 < argc) {
            std::string line = "cam " + utf8(i + 1) + " " + utf8(i + 2) + " " + utf8(i + 3);
            i += 3;
            if (i + 1 < argc && argv[i + 1][0] != L'-') line += " " + utf8(++i);
            add(line);
        } else if (a == L"--cmd" && more) add(utf8(++i));
        else if (a == L"--script" && more) add("script \"" + utf8(++i) + "\"");
        else if (a == L"--frames" && more) o.frames = std::max(1, to_int(argv[++i]));
        else if (a == L"--shot" && more) o.shot = argv[++i];
        else if (a == L"--fixed-step" && more) o.fixed_step = std::wcstod(argv[++i].c_str(), nullptr);
        else if (a == L"--live") o.live = true;
        else if (a == L"--foreground") o.foreground = 1;
        else if (a == L"--background") o.foreground = 0;
        else if (a == L"--capture" && i + 2 < argc) {
            const std::wstring dims = argv[++i];
            o.capture_w = to_int(dims);
            const size_t x = dims.find(L'x');
            o.capture_h = x == std::wstring::npos ? 0 : to_int(dims.substr(x + 1));
            o.capture_path = argv[++i];
        } else if (a.rfind(L"--", 0) == 0) {   // any other --name args... runs the command `name args...` (the old weather_editor flags: --view, --blend, --pick, ...)
            std::string line = utf8(i).substr(2);
            // the old Tile Studio flags whose names are taken by generic commands or use hyphens
            static const std::pair<const char*, const char*> kAlias[] = {{"select", "tile_select"}, {"move", "tile_move"}, {"delete", "delete_item"}, {"add", "add_building"},
                {"add-polygon", "add_polygon"}, {"polygon-point", "polygon_point"}, {"new-layer", "new_layer"}, {"move-to-layer", "move_to_layer"}, {"save-to", "save_to"}};
            for (const auto& [from, to] : kAlias)
                if (line == from) line = to;
            while (i + 1 < argc && std::wstring(argv[i + 1]).rfind(L"--", 0) != 0) line += " \"" + utf8(++i) + "\"";
            add(line);
        }
    }
    return o;
}

// Scripted runs echo the log to stderr; an interactive run keeps it in <settings folder>/last_run.log.
void open_log(const Options& opt) {
    if (!opt.shot.empty() || !opt.capture_path.empty() || !opt.script_text.empty()) {
        wce::Log::instance().echo_to_stderr(true);
        return;
    }
    const std::filesystem::path log_path = wce::Settings::default_dir() / "last_run.log";
    std::error_code ec;
    std::filesystem::create_directories(log_path.parent_path(), ec);
    if (ec || !wce::Log::instance().open_file(wce::narrow(log_path.wstring()))) {
        const std::wstring message = L"WCE could not create its interactive log at " + log_path.wstring() + L"\n";
#ifdef _WIN32
        OutputDebugStringW(message.c_str());
#else
        std::fputs(wce::narrow(message).c_str(), stderr);
#endif
    }
}

// Scripted and screenshot runs are unattended: their window must not take the focus from whatever the user is doing (a game, say).
bool window_takes_focus(const Options& opt) {
    bool take_focus = opt.shot.empty() && opt.capture_path.empty() && opt.script_text.empty();
    if (opt.foreground >= 0) take_focus = opt.foreground == 1;
    else if (const char* f = std::getenv("WCE_FOREGROUND"); f && *f) take_focus = *f != '0';
    return take_focus;
}

// The device and swap chain on the chosen adapter. False when D3D11 is not available.
bool create_device(HWND window) {
    DXGI_SWAP_CHAIN_DESC sd{};
    sd.BufferCount = 2;
    sd.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    sd.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    sd.OutputWindow = window;
    sd.SampleDesc.Count = 1;
    sd.Windowed = TRUE;
    sd.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    UINT flags = 0;
    if (const char* d = std::getenv("WCE_D3D_DEBUG"); d && *d == '1') flags |= D3D11_CREATE_DEVICE_DEBUG;   // needs the Windows "Graphics Tools" feature
    Microsoft::WRL::ComPtr<IDXGIAdapter> adapter = wce::choose_adapter();   // the same GPU every run (docs/baseline.md)
    D3D_FEATURE_LEVEL fl = D3D_FEATURE_LEVEL_11_0;
    if (FAILED(D3D11CreateDeviceAndSwapChain(adapter.Get(), adapter ? D3D_DRIVER_TYPE_UNKNOWN : D3D_DRIVER_TYPE_HARDWARE, nullptr, flags, &fl, 1, D3D11_SDK_VERSION, &sd, &g_swap, &g_device, nullptr, &g_ctx)))
        return false;
    create_rtv();
    return true;
}

void init_imgui(float dpi) {
    IMGUI_CHECKVERSION();
    ImGui::CreateContext();
    ImGuiIO& io = ImGui::GetIO();
    io.ConfigFlags |= ImGuiConfigFlags_NavEnableKeyboard | ImGuiConfigFlags_DockingEnable;
    io.IniFilename = nullptr;   // layouts are files per tool, see Editor::save_layout
    ImGui::StyleColorsDark();
    ImGui::GetStyle().ScaleAllSizes(dpi);
    io.FontGlobalScale = dpi;
}

// The editor with its tools, the command line as a script, and the frame loop until the editor or the window quits. `pump_events`
// handles the window's pending events and returns true when the window was closed; `platform_new_frame` is the ImGui platform backend's.
template <typename PumpEvents, typename PlatformNewFrame>
int run_editor(const Options& opt, PumpEvents pump_events, PlatformNewFrame platform_new_frame) {
    ImGuiIO& io = ImGui::GetIO();
    int exit_code = 0;
    wce::Editor editor(g_device.Get(), g_ctx.Get());
    g_editor = &editor;
    editor.add_tool(wce::make_weather_tool());
    editor.add_tool(wce::make_tile_tool());
    editor.add_tool(wce::make_unit_tool());
    editor.add_tool(wce::make_vfx_tool());
    editor.add_tool(wce::make_terrain_tool());
    editor.add_tool(wce::make_cinematic_tool());
    editor.start();

    // Everything the command line asked for becomes a script; `frames` lines give the editor time to render in between.
    std::string script;
    if (!opt.tool.empty()) script += "tool " + opt.tool + "\nframes 2\n";
    script += opt.script_text;
    const bool batch = !opt.shot.empty() || !opt.capture_path.empty();
    if (!opt.capture_path.empty()) script += "frames 2\ncapture " + std::to_string(opt.capture_w) + "x" + std::to_string(opt.capture_h) + " \"" + wce::narrow(opt.capture_path) + "\" " + std::to_string(opt.frames) + "\nquit\n";
    else if (!opt.shot.empty()) script += "frames " + std::to_string(opt.frames) + "\nshot \"" + wce::narrow(opt.shot) + "\"\nquit\n";
    if (!script.empty()) editor.script().load_text(script);
    double step = opt.fixed_step;
    if (step <= 0 && !opt.live && (batch || !opt.script_text.empty())) step = 1.0 / 60.0;
    if (step > 0) editor.set_fixed_step(step);
    if (step > 0) io.KeyRepeatDelay = 1.0e6f;   // scripted runs: a held key must not auto-repeat when a frame happens to take long

    if (const char* v = std::getenv("WCE_VSYNC"); v && *v == '0') editor.vsync = false;
    bool done = false;
    auto last_frame = std::chrono::steady_clock::now();
    while (!done) {
        const auto frame_start = std::chrono::steady_clock::now();
        if (pump_events()) break;
        if (g_resize_w && g_resize_h) {
            g_rtv.Reset();
            g_swap->ResizeBuffers(0, g_resize_w, g_resize_h, DXGI_FORMAT_UNKNOWN, 0);
            g_resize_w = g_resize_h = 0;
            create_rtv();
        }
        editor.pre_frame();
        ImGui_ImplDX11_NewFrame();
        platform_new_frame();
        editor.inject_input();   // scripted runs: the `mouse` commands replace the real cursor (a hovered splitter changes a shot)
        ImGui::NewFrame();
        editor.frame();
        ImGui::Render();
        const float clear[4] = {0.08f, 0.08f, 0.09f, 1.0f};
        g_ctx->OMSetRenderTargets(1, g_rtv.GetAddressOf(), nullptr);
        g_ctx->ClearRenderTargetView(g_rtv.Get(), clear);
        ImGui_ImplDX11_RenderDrawData(ImGui::GetDrawData());
        if (!editor.pending_shot.empty()) {
            ComPtr<ID3D11Texture2D> back;
            g_swap->GetBuffer(0, IID_PPV_ARGS(&back));
            if (!wce::save_texture_png(g_device.Get(), g_ctx.Get(), back.Get(), editor.pending_shot)) wce::Log::error("cannot write " + wce::narrow(editor.pending_shot));
            editor.pending_shot.clear();
        }
        if (editor.wants_quit()) {
            exit_code = editor.exit_code();
            done = true;
        }
        const auto cpu_end = std::chrono::steady_clock::now();
        g_swap->Present(batch || !editor.vsync ? 0 : 1, 0);
        const auto now = std::chrono::steady_clock::now();
        using ms = std::chrono::duration<double, std::milli>;
        editor.perf.record(ms(now - last_frame).count(), ms(cpu_end - frame_start).count());
        last_frame = now;
    }
    if (!batch) editor.save_layout();
    g_editor = nullptr;
    return exit_code;
}

} // namespace

#ifdef _WIN32

int WINAPI wWinMain(HINSTANCE inst, HINSTANCE, PWSTR, int) {
    SetUnhandledExceptionFilter(crash_handler);
    OleInitialize(nullptr);
    ImGui_ImplWin32_EnableDpiAwareness();
    int argc = 0;
    LPWSTR* argv = CommandLineToArgvW(GetCommandLineW(), &argc);
    const Options opt = parse_args(std::vector<std::wstring>(argv, argv + argc));
    LocalFree(argv);
    open_log(opt);

    WNDCLASSEXW wc{sizeof wc, CS_CLASSDC, wnd_proc, 0, 0, inst, nullptr, LoadCursor(nullptr, IDC_ARROW), nullptr, nullptr, L"WarscapeCommunityEditor", nullptr};
    RegisterClassExW(&wc);
    // An unattended run's window is created non-activating, kept out of the taskbar and Alt-Tab, shown without activation and put behind
    // the other windows.
    const bool take_focus = window_takes_focus(opt);
    HWND hwnd = CreateWindowExW(take_focus ? 0 : WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW, wc.lpszClassName, L"Warscape Community Editor", WS_OVERLAPPEDWINDOW, 100, 100, 1600, 960,
                                nullptr, nullptr, inst, nullptr);

    if (!create_device(hwnd)) {
        MessageBoxW(hwnd, L"Could not create a Direct3D 11 device.", L"Warscape Community Editor", MB_ICONERROR);
        return 1;
    }
    if (take_focus) {
        ShowWindow(hwnd, SW_SHOWDEFAULT);
    } else {
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SetWindowPos(hwnd, HWND_BOTTOM, 0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE);
    }
    UpdateWindow(hwnd);

    init_imgui(ImGui_ImplWin32_GetDpiScaleForHwnd(hwnd));
    ImGui_ImplWin32_Init(hwnd);
    ImGui_ImplDX11_Init(g_device.Get(), g_ctx.Get());

    const int exit_code = run_editor(
        opt,
        [] {
            MSG msg;
            bool quit = false;
            while (PeekMessageW(&msg, nullptr, 0, 0, PM_REMOVE)) {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
                if (msg.message == WM_QUIT) quit = true;
            }
            return quit;
        },
        [] { ImGui_ImplWin32_NewFrame(); });

    ImGui_ImplDX11_Shutdown();
    ImGui_ImplWin32_Shutdown();
    ImGui::DestroyContext();
    DestroyWindow(hwnd);
    UnregisterClassW(wc.lpszClassName, inst);
    OleUninitialize();
    return exit_code;
}

#else

int main(int argc, char** argv) {
    std::vector<std::wstring> args;
    for (int i = 0; i < argc; ++i) args.push_back(wce::widen(argv[i]));
    const Options opt = parse_args(args);
    open_log(opt);

    // An unattended run's window cannot take the focus and is not activated when shown.
    const bool take_focus = window_takes_focus(opt);
    if (!take_focus) SDL_SetHint(SDL_HINT_WINDOW_ACTIVATE_WHEN_SHOWN, "0");
    if (!SDL_Init(SDL_INIT_VIDEO)) {
        std::fprintf(stderr, "SDL_Init failed: %s\n", SDL_GetError());
        return 1;
    }
    // Window size in pixels: the DX11 ImGui backend draws at DisplaySize, so the window has no separate high-density back buffer. Under
    // Wayland the compositor scales the window; under X11 the content scale sizes the UI like the Windows DPI scale does.
    const SDL_WindowFlags wflags = SDL_WINDOW_VULKAN | SDL_WINDOW_RESIZABLE | (take_focus ? 0 : SDL_WINDOW_NOT_FOCUSABLE);
    SDL_Window* window = SDL_CreateWindow("Warscape Community Editor", 1600, 960, wflags);
    if (!window) {
        std::fprintf(stderr, "SDL_CreateWindow failed: %s\n", SDL_GetError());
        SDL_Quit();
        return 1;
    }
    if (!create_device(reinterpret_cast<HWND>(window))) {   // DXVK-native takes the SDL window as the HWND
        SDL_ShowSimpleMessageBox(SDL_MESSAGEBOX_ERROR, "Warscape Community Editor", "Could not create a Direct3D 11 device (DXVK needs a Vulkan driver).", window);
        SDL_DestroyWindow(window);
        SDL_Quit();
        return 1;
    }
    const char* video = SDL_GetCurrentVideoDriver();
    const bool wayland = video && std::string(video) == "wayland";
    float dpi = wayland ? 1.0f : SDL_GetWindowDisplayScale(window);
    if (dpi <= 0) dpi = 1.0f;
    init_imgui(dpi);
    ImGui_ImplSDL3_InitForD3D(window);
    ImGui_ImplDX11_Init(g_device.Get(), g_ctx.Get());

    const int exit_code = run_editor(
        opt,
        [window] {
            SDL_Event e;
            bool quit = false;
            while (SDL_PollEvent(&e)) quit = handle_event(window, e) || quit;
            return quit;
        },
        [] { ImGui_ImplSDL3_NewFrame(); });

    ImGui_ImplDX11_Shutdown();
    ImGui_ImplSDL3_Shutdown();
    ImGui::DestroyContext();
    g_rtv.Reset();
    g_swap.Reset();
    g_ctx.Reset();
    g_device.Reset();
    SDL_DestroyWindow(window);
    SDL_Quit();
    return exit_code;
}

#endif
