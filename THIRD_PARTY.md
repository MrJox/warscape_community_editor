# Third-party code

Everything here is vendored under `third_party/` with its licence file. Permissive licences only. Ask before adding anything.

| Component | Version | Licence | Where | Used for |
|---|---|---|---|---|
| Dear ImGui | 1.91.9, docking branch (tag `v1.91.9-docking`, commit 126d004) | MIT | `third_party/imgui` (DX11, Win32 and SDL3 backends only; `LICENSE.txt` inside) | all editor UI, docking and multi-panel layouts |
| ImGuizmo | master of 2026-08-08 (commit 18cef5e) | MIT | `third_party/imguizmo` (`ImGuizmo.cpp/.h`, `LICENSE`) | translate / rotate / scale gizmos in the viewport | **Local change (Phase 1 of the TEd parity plan):** `SetAllowedItem(ImGuiID)` and a `CanActivate()` that ignores that item, because the viewport's button is the hovered item over the whole 3D view.
| zlib | 1.3.1 | zlib licence | `third_party/zlib` (deflate and inflate sources only; the licence text is in the header of `zlib.h`) | PNG encode and decode that reproduces the kit's encoder byte for byte (`formats/png_file`) |

## Linux build dependencies (not vendored)

The Linux build keeps the D3D11 renderer and runs it on a D3D11 implementation over Vulkan. `cmake/linux_deps.cmake` fetches and builds these at
configure time into `<build>/_deps` (nothing is committed or redistributed); the executables link them dynamically.

| Component | Version | Licence | Used for |
|---|---|---|---|
| DXVK (native build) | v3.1.1 | zlib licence | `libdxvk_d3d11.so`, `libdxvk_dxgi.so` and the D3D/DXGI headers (from mingw-w64). Built with one change backported from upstream master: `ResizeBuffers` gives the presenter the new size (a maximized window on Wayland kept its first swapchain) |
| vkd3d | vkd3d-2.1 | **LGPL-2.1-or-later** | `libvkd3d-utils.so`: `D3DCompile` (the editor's helper HLSL), `D3DReflect` (game shader bindings) and `D3DDisassemble` (`fxc_dump`). Built with one local change: `WINAPI` / COM methods use the native x86-64 ABI instead of `ms_abi`, to match DXVK's headers |
| SPIRV-Headers | vulkan-sdk-1.4.321.0 | MIT-style (Khronos) | headers needed to build vkd3d |

System libraries: SDL3 (zlib licence) for the window and input, libcurl (curl licence) for the rpfm_server client (HTTP and WebSocket).

Rust port: crates.io dependencies are listed in `Cargo.lock` (all MIT, Apache-2.0 or zlib licensed). Notable: rpfm_lib (MIT) for the packs and
game formats, windows-core / windows-bindgen (MIT or Apache-2.0) for the D3D11 bindings, and libz-sys (MIT or Apache-2.0) building its bundled
classic zlib (zlib licence) statically for the kit's PNG encoder. The UI uses dear-imgui-rs / dear-imgui-sys (MIT or Apache-2.0; they build
Dear ImGui 1.92.9b docking and cimgui, MIT) and sdl3-sys (zlib licence), linked against the system SDL3 (the one DXVK-native presents through);
the SDL3 platform backend is a port of Dear ImGui's `imgui_impl_sdl3.cpp` and the D3D11 renderer one of `imgui_impl_dx11.cpp` (both MIT).
The gizmos come from dear-imguizmo-sys (MIT or Apache-2.0), which builds upstream ImGuizmo (MIT) without the local `SetAllowedItem`
change: the viewport clears its button from Dear ImGui's hovered and active ids around `Manipulate` instead.

No game assets, shaders or binaries are redistributed. The game's `.fxc` shaders are read from the user's own installation at run time.
