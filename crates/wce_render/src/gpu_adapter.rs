//! Which GPU the renderer runs on. A machine with two GPUs (integrated + discrete) lets the system pick the default adapter, and the
//! pick can change between runs; the two GPUs don't round float math the same way, so shots and captures only reproduce on one adapter.

use wce_d3d11::*;

/// The adapter to create the D3D11 device on: the one whose name contains `$WCE_GPU` (case-insensitive) when that is set, otherwise
/// the high-performance GPU. `None` when DXGI offers nothing (then create the device on the default adapter).
///
/// # Safety
///
/// Call it after [`wce_d3d11::init_platform`].
pub unsafe fn choose_adapter() -> Option<IDXGIAdapter> {
    // SAFETY: DXGI calls on a factory this function owns.
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
        if let Ok(want) = std::env::var("WCE_GPU")
            && !want.is_empty()
        {
            let needle = want.to_lowercase();
            let mut i = 0;
            while let Ok(adapter) = factory.EnumAdapters1(i) {
                i += 1;
                let mut d = DXGI_ADAPTER_DESC1::default();
                if adapter.GetDesc1(&mut d).is_err() {
                    continue;
                }

                let len = d.Description.iter().position(|&c| c == 0).unwrap_or(d.Description.len());
                let name = String::from_utf16_lossy(&d.Description[..len]).to_lowercase();
                if name.contains(&needle) && d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE as u32 == 0 {
                    return windows_core::Interface::cast(&adapter).ok();
                }
            }

            wce_foundation::log::warn(format!("WCE_GPU={want} matches no adapter, using the high-performance one"));
        }

        let factory6: IDXGIFactory6 = windows_core::Interface::cast(&factory).ok()?;
        let adapter: IDXGIAdapter1 = factory6.EnumAdapterByGpuPreference(0, DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE).ok()?;
        windows_core::Interface::cast(&adapter).ok()
    }
}
