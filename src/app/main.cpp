// Tile Studio: window, D3D11 device and the ImGui frame loop (same shell as weather_editor/src/editor/main.cpp).
#include "app/studio.h"
#include "render/image_io.h"

#include "imgui.h"
#include "imgui_impl_dx11.h"
#include "imgui_impl_win32.h"

#include <d3d11.h>
#include <windows.h>
#include <shellapi.h>
#include <wrl/client.h>

#include <cstdio>
#include <cstdlib>
#include <string>
#include <vector>

using Microsoft::WRL::ComPtr;

extern IMGUI_IMPL_API LRESULT ImGui_ImplWin32_WndProcHandler(HWND, UINT, WPARAM, LPARAM);

namespace {

ComPtr<ID3D11Device> g_device;
ComPtr<ID3D11DeviceContext> g_ctx;
ComPtr<IDXGISwapChain> g_swap;
ComPtr<ID3D11RenderTargetView> g_rtv;
UINT g_resize_w = 0, g_resize_h = 0;

void create_rtv() {
    ComPtr<ID3D11Texture2D> back;
    g_swap->GetBuffer(0, IID_PPV_ARGS(&back));
    g_device->CreateRenderTargetView(back.Get(), nullptr, &g_rtv);
}

LRESULT WINAPI wnd_proc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp) {
    if (ImGui_ImplWin32_WndProcHandler(hwnd, msg, wp, lp)) return true;
    switch (msg) {
    case WM_SIZE:
        if (wp != SIZE_MINIMIZED) { g_resize_w = LOWORD(lp); g_resize_h = HIWORD(lp); }
        return 0;
    case WM_SYSCOMMAND:
        if ((wp & 0xfff0) == SC_KEYMENU) return 0;
        break;
    case WM_DESTROY:
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcW(hwnd, msg, wp, lp);
}

std::string narrow(const std::wstring& w) {
    if (w.empty()) return {};
    int n = WideCharToMultiByte(CP_UTF8, 0, w.c_str(), int(w.size()), nullptr, 0, nullptr, nullptr);
    std::string s(size_t(n), 0);
    WideCharToMultiByte(CP_UTF8, 0, w.c_str(), int(w.size()), s.data(), n, nullptr, nullptr);
    return s;
}

} // namespace

int WINAPI wWinMain(HINSTANCE inst, HINSTANCE, PWSTR, int) {
    ImGui_ImplWin32_EnableDpiAwareness();
    WNDCLASSEXW wc{sizeof wc, CS_CLASSDC, wnd_proc, 0, 0, inst, nullptr, LoadCursor(nullptr, IDC_ARROW), nullptr, nullptr, L"TileStudio", nullptr};
    RegisterClassExW(&wc);
    HWND hwnd = CreateWindowW(wc.lpszClassName, L"Tile Studio", WS_OVERLAPPEDWINDOW, 100, 100, 1600, 960, nullptr, nullptr, inst, nullptr);

    DXGI_SWAP_CHAIN_DESC sd{};
    sd.BufferCount = 2;
    sd.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    sd.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    sd.OutputWindow = hwnd;
    sd.SampleDesc.Count = 1;
    sd.Windowed = TRUE;
    sd.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    D3D_FEATURE_LEVEL fl = D3D_FEATURE_LEVEL_11_0;
    if (FAILED(D3D11CreateDeviceAndSwapChain(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, 0, &fl, 1, D3D11_SDK_VERSION, &sd, &g_swap, &g_device, nullptr, &g_ctx))) {
        MessageBoxW(hwnd, L"Could not create a Direct3D 11 device.", L"Tile Studio", MB_ICONERROR);
        return 1;
    }
    create_rtv();
    ShowWindow(hwnd, SW_SHOWDEFAULT);
    UpdateWindow(hwnd);

    IMGUI_CHECKVERSION();
    ImGui::CreateContext();
    ImGuiIO& io = ImGui::GetIO();
    io.ConfigFlags |= ImGuiConfigFlags_NavEnableKeyboard;
    io.IniFilename = nullptr;
    ImGui::StyleColorsDark();
    float dpi = ImGui_ImplWin32_GetDpiScaleForHwnd(hwnd);
    ImGui::GetStyle().ScaleAllSizes(dpi);
    io.FontGlobalScale = dpi;
    ImGui_ImplWin32_Init(hwnd);
    ImGui_ImplDX11_Init(g_device.Get(), g_ctx.Get());

    ts::Studio studio(g_device.Get(), g_ctx.Get());
    std::wstring shot_path;
    int shot_frame = 8;
    {
        int argc = 0;
        LPWSTR* argv = CommandLineToArgvW(GetCommandLineW(), &argc);
        for (int i = 1; i < argc; ++i) {
            std::wstring a = argv[i];
            if (a == L"--tile" && i + 1 < argc) { std::string err; if (!studio.open_tile(narrow(argv[++i]), &err)) std::fprintf(stderr, "open failed: %s\n", err.c_str()); }
            else if (a == L"--shot" && i + 1 < argc) shot_path = argv[++i];
            else if (a == L"--frames" && i + 1 < argc) shot_frame = std::max(1, _wtoi(argv[++i]));
            else if (a == L"--paint" && i + 4 < argc) { std::string err; if (!studio.script_paint(narrow(argv[i + 1]), float(_wtof(argv[i + 2])), float(_wtof(argv[i + 3])), _wtoi(argv[i + 4]), &err)) std::fprintf(stderr, "paint: %s\n", err.c_str()); i += 4; }
            else if (a == L"--select" && i + 1 < argc) { if (!studio.script_select(narrow(argv[++i]))) std::fprintf(stderr, "select: nothing found\n"); }
            else if (a == L"--move" && i + 2 < argc) { studio.script_move(float(_wtof(argv[i + 1])), float(_wtof(argv[i + 2]))); i += 2; }
            else if (a == L"--undo" && i + 1 < argc) studio.script_undo(_wtoi(argv[++i]));
            else if (a == L"--redo" && i + 1 < argc) studio.script_redo(_wtoi(argv[++i]));
            else if (a == L"--duplicate") studio.script_duplicate();
            else if (a == L"--delete") studio.script_delete();
            else if (a == L"--rotate" && i + 1 < argc) studio.script_rotate(float(_wtof(argv[++i])));
            else if (a == L"--add" && i + 1 < argc) studio.script_add(narrow(argv[++i]));
            else if (a == L"--new-layer" && i + 1 < argc) studio.script_new_layer(narrow(argv[++i]));
            else if (a == L"--move-to-layer" && i + 1 < argc) studio.script_move_selected_to_layer(narrow(argv[++i]));
            else if (a == L"--save-to" && i + 1 < argc) { std::string msg; bool ok = studio.save_to(narrow(argv[++i]), &msg); std::fprintf(stderr, "save: %s (%s)\n", ok ? "ok" : "failed", msg.c_str()); }
            else if (a == L"--cam" && i + 3 < argc) { float y = float(_wtof(argv[i + 1])), p = float(_wtof(argv[i + 2])), dd = float(_wtof(argv[i + 3])); studio.set_camera(y, p, dd); i += 3; }
        }
        LocalFree(argv);
    }

    int frame_no = 0;
    bool done = false;
    while (!done) {
        MSG msg;
        while (PeekMessageW(&msg, nullptr, 0, 0, PM_REMOVE)) {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
            if (msg.message == WM_QUIT) done = true;
        }
        if (done) break;
        if (g_resize_w && g_resize_h) {
            g_rtv.Reset();
            g_swap->ResizeBuffers(0, g_resize_w, g_resize_h, DXGI_FORMAT_UNKNOWN, 0);
            g_resize_w = g_resize_h = 0;
            create_rtv();
        }
        ComPtr<ID3D11Texture2D> back;
        g_swap->GetBuffer(0, IID_PPV_ARGS(&back));
        D3D11_TEXTURE2D_DESC bd;
        back->GetDesc(&bd);
        studio.set_surface_size(int(bd.Width), int(bd.Height));

        ImGui_ImplDX11_NewFrame();
        ImGui_ImplWin32_NewFrame();
        ImGui::NewFrame();
        studio.frame();
        ImGui::Render();
        const float clear[4] = {0.10f, 0.12f, 0.16f, 1.0f};
        g_ctx->OMSetRenderTargets(1, g_rtv.GetAddressOf(), nullptr);
        g_ctx->ClearRenderTargetView(g_rtv.Get(), clear);
        studio.render_scene(g_rtv.Get());
        g_ctx->OMSetRenderTargets(1, g_rtv.GetAddressOf(), nullptr);
        ImGui_ImplDX11_RenderDrawData(ImGui::GetDrawData());
        if (!shot_path.empty() && ++frame_no == shot_frame) {
            we::save_texture_png(g_device.Get(), g_ctx.Get(), back.Get(), shot_path);
            if (FILE* t = _wfopen((shot_path + L".txt").c_str(), L"w")) { std::fputs(studio.diagnostics().c_str(), t); std::fclose(t); }
            done = true;
        }
        g_swap->Present(1, 0);
    }

    ImGui_ImplDX11_Shutdown();
    ImGui_ImplWin32_Shutdown();
    ImGui::DestroyContext();
    DestroyWindow(hwnd);
    UnregisterClassW(wc.lpszClassName, inst);
    return 0;
}
