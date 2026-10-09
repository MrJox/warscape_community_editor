#pragma once
// Which GPU the renderer runs on. A machine with two GPUs (integrated + discrete) lets Windows pick the default adapter, and the pick can change
// between runs; the two GPUs do not round float math the same way, so shots and captures only reproduce on one adapter.
#include <dxgi1_6.h>
#include <wrl/client.h>

namespace wce {

// The adapter to create the D3D11 device on: the one whose name contains $WCE_GPU (case-insensitive) when that is set, otherwise the
// high-performance GPU. nullptr when DXGI offers nothing (then create the device on the default adapter).
Microsoft::WRL::ComPtr<IDXGIAdapter> choose_adapter();

} // namespace wce
