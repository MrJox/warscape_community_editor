// Attila Weather Editor: window, D3D11 device and the ImGui frame loop.
#include "core/process.h"
#include "editor/app.h"
#include "render/image_io.h"

#include "imgui.h"
#include "imgui_impl_dx11.h"
#include "imgui_impl_win32.h"

#include <d3d11.h>
#include <windows.h>
#include <dbghelp.h>
#include <shellapi.h>

#include <cstdio>
#include <cstdlib>
#include <string>
#include <wrl/client.h>

using Microsoft::WRL::ComPtr;

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

namespace {

ComPtr<ID3D11Device> g_device;
ComPtr<ID3D11DeviceContext> g_ctx;
ComPtr<IDXGISwapChain> g_swap;
ComPtr<ID3D11RenderTargetView> g_rtv;
UINT g_resize_w = 0, g_resize_h = 0;
we::App* g_app = nullptr;

void create_rtv() {
    ComPtr<ID3D11Texture2D> back;
    g_swap->GetBuffer(0, IID_PPV_ARGS(&back));
    g_device->CreateRenderTargetView(back.Get(), nullptr, &g_rtv);
}

LRESULT WINAPI wnd_proc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp) {
    if (ImGui_ImplWin32_WndProcHandler(hwnd, msg, wp, lp)) return true;
    switch (msg) {
    case WM_SIZE:
        if (wp != SIZE_MINIMIZED) {
            g_resize_w = LOWORD(lp);
            g_resize_h = HIWORD(lp);
        }
        return 0;
    case WM_CLOSE:
        if (g_app && !g_app->confirm_close()) return 0;
        break;
    case WM_SYSCOMMAND:
        if ((wp & 0xfff0) == SC_KEYMENU) return 0;
        break;
    case WM_DESTROY:
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcW(hwnd, msg, wp, lp);
}

} // namespace

int WINAPI wWinMain(HINSTANCE inst, HINSTANCE, PWSTR, int) {
    SetUnhandledExceptionFilter(crash_handler);
    ImGui_ImplWin32_EnableDpiAwareness();
    WNDCLASSEXW wc{sizeof wc, CS_CLASSDC, wnd_proc, 0, 0, inst, nullptr, LoadCursor(nullptr, IDC_ARROW), nullptr, nullptr, L"AttilaWeatherEditor", nullptr};
    RegisterClassExW(&wc);
    HWND hwnd = CreateWindowW(wc.lpszClassName, L"Attila Weather Editor", WS_OVERLAPPEDWINDOW, 100, 100, 1600, 960, nullptr, nullptr, inst, nullptr);

    DXGI_SWAP_CHAIN_DESC sd{};
    sd.BufferCount = 2;
    sd.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    sd.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    sd.OutputWindow = hwnd;
    sd.SampleDesc.Count = 1;
    sd.Windowed = TRUE;
    sd.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    UINT flags = 0;
#ifdef _DEBUG
    flags |= D3D11_CREATE_DEVICE_DEBUG;
#endif
    D3D_FEATURE_LEVEL fl = D3D_FEATURE_LEVEL_11_0;
    if (FAILED(D3D11CreateDeviceAndSwapChain(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, flags, &fl, 1, D3D11_SDK_VERSION, &sd, &g_swap, &g_device, nullptr, &g_ctx))) {
        MessageBoxW(hwnd, L"Could not create a Direct3D 11 device.", L"Attila Weather Editor", MB_ICONERROR);
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

    we::App app(g_device.Get(), g_ctx.Get());
    g_app = &app;
    std::wstring shot_path; // --shot <file.png>: save the window after a few frames and exit
    {   // --open <path> (repeatable)
        int argc = 0;
        LPWSTR* argv = CommandLineToArgvW(GetCommandLineW(), &argc);
        for (int i = 1; i + 1 < argc; ++i) {
            std::wstring a = argv[i];
            if (a == L"--open") app.open_path(we::narrow(argv[++i]));
            else if (a == L"--shot") shot_path = argv[++i];
            else if (a == L"--view") app.set_view(_wtoi(argv[++i]));
            else if (a == L"--blend") app.set_blend(0, float(_wtof(argv[++i]))); // first --open document, blended into the active one
            else if (a == L"--cam" && i + 3 < argc) { // --cam <yaw> <pitch> <distance> (radians, metres)
                float yaw = float(_wtof(argv[i + 1])), pitch = float(_wtof(argv[i + 2])), dist = float(_wtof(argv[i + 3]));
                float ty = 5;
                if (i + 4 < argc && argv[i + 4][0] != L'-') ty = float(_wtof(argv[i + 4]));
                app.set_orbit(yaw, pitch, dist, ty);
                i += (ty != 5 ? 4 : 3);
            }
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
        ImGui_ImplDX11_NewFrame();
        ImGui_ImplWin32_NewFrame();
        ImGui::NewFrame();
        app.frame();
        ImGui::Render();
        const float clear[4] = {0.08f, 0.08f, 0.09f, 1.0f};
        g_ctx->OMSetRenderTargets(1, g_rtv.GetAddressOf(), nullptr);
        g_ctx->ClearRenderTargetView(g_rtv.Get(), clear);
        ImGui_ImplDX11_RenderDrawData(ImGui::GetDrawData());
        if (!shot_path.empty() && ++frame_no == 20) {
            Microsoft::WRL::ComPtr<ID3D11Texture2D> back;
            g_swap->GetBuffer(0, IID_PPV_ARGS(&back));
            we::save_texture_png(g_device.Get(), g_ctx.Get(), back.Get(), shot_path);
            if (FILE* t = _wfopen((shot_path + L".txt").c_str(), L"w")) {
                fputs(app.diagnostics().c_str(), t);
                fclose(t);
            }
            done = true;
        }
        g_swap->Present(1, 0);
    }

    g_app = nullptr;
    ImGui_ImplDX11_Shutdown();
    ImGui_ImplWin32_Shutdown();
    ImGui::DestroyContext();
    DestroyWindow(hwnd);
    UnregisterClassW(wc.lpszClassName, inst);
    return 0;
}
