#pragma once
// Which GPU the renderer runs on. A machine with two GPUs (integrated + discrete) lets Windows pick the default adapter, and the pick can change
// between runs; the two GPUs do not round float math the same way, so shots and captures only reproduce on one adapter.
#include <dxgi1_6.h>
#include <wrl/client.h>

#ifndef _WIN32
#include <cstdlib>
#endif

namespace wce {

// Call before the first DXGI/D3D11 call. Linux: DXVK-native must be told its window system integration, SDL3 (unless $DXVK_WSI_DRIVER
// says otherwise), and logs to stderr only instead of writing <exe>_d3d11.log files into the working directory (unless $DXVK_LOG_PATH is
// set). Nothing to do on Windows.
inline void prepare_d3d() {
#ifndef _WIN32
    setenv("DXVK_WSI_DRIVER", "SDL3", 0);
    setenv("DXVK_LOG_PATH", "none", 0);
#endif
}

// The adapter to create the D3D11 device on: the one whose name contains $WCE_GPU (case-insensitive) when that is set, otherwise the
// high-performance GPU. nullptr when DXGI offers nothing (then create the device on the default adapter).
Microsoft::WRL::ComPtr<IDXGIAdapter> choose_adapter();

} // namespace wce
