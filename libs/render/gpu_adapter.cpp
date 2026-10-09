#include "render/gpu_adapter.h"

#include "foundation/log.h"
#include "foundation/process.h"

#include <algorithm>
#include <cctype>
#include <cstdlib>
#include <string>

namespace wce {

namespace {

std::string lower(std::string s) {
    for (char& c : s) c = char(std::tolower(static_cast<unsigned char>(c)));
    return s;
}

} // namespace

Microsoft::WRL::ComPtr<IDXGIAdapter> choose_adapter() {
    Microsoft::WRL::ComPtr<IDXGIFactory1> factory;
    if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory)))) return nullptr;
    const char* want = std::getenv("WCE_GPU");
    if (want && *want) {
        const std::string needle = lower(want);
        Microsoft::WRL::ComPtr<IDXGIAdapter1> a;
        for (UINT i = 0; factory->EnumAdapters1(i, &a) == S_OK; ++i, a.Reset()) {
            DXGI_ADAPTER_DESC1 d{};
            a->GetDesc1(&d);
            if (lower(narrow(d.Description)).find(needle) != std::string::npos && !(d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE)) return a;
        }
        Log::warn(std::string("WCE_GPU=") + want + " matches no adapter, using the high-performance one");
    }
    Microsoft::WRL::ComPtr<IDXGIFactory6> f6;
    if (SUCCEEDED(factory.As(&f6))) {
        Microsoft::WRL::ComPtr<IDXGIAdapter1> a;
        if (SUCCEEDED(f6->EnumAdapterByGpuPreference(0, DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE, IID_PPV_ARGS(&a)))) return a;
    }
    return nullptr;
}

} // namespace wce
