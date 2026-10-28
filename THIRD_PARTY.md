# Third-party code

Permissive licences only. Ask before adding anything. Nothing is vendored: the code comes from crates.io (see the Rust dependencies
below), and `third_party/` only keeps the licence of a project whose format knowledge was ported.

## Format knowledge from other projects

| Project | Licence | Where | Used for |
|---|---|---|---|
| etwng by Tomasz Wegrzanowski (taw), `ui/` converter; research credits in its README: alpaca and Cpecific | MIT | `third_party/etwng/LICENSE`; ported to Rust in `crates/wce_ui_format` (the version-by-version layout of the UI file formats, the XML conventions and the float formatting) | UI layouts (`Version0xx`, `.cml`, `.fc`, `.twui.images`) to XML and back |

## Fonts (vendored)

The one exception to "nothing is vendored": the UI's fonts are files in `crates/wce_editor/assets/fonts`, built into the binary
(`include_bytes!`), each with its licence next to it.

| Font | Version | Licence | Used for |
|---|---|---|---|
| Inter (Rasmus Andersson), `Inter-Regular.ttf` | 4.1 | SIL Open Font License 1.1 (`Inter-LICENSE.txt`) | the UI text |
| Lucide icons, `lucide.ttf` (from the `lucide-static` npm package) | 1.61.0 | ISC (`lucide-LICENSE.txt`) | the icons merged into the UI font (`wce_editor::icons`) |

## Linux build dependencies (not vendored)

The Linux build keeps the D3D11 renderer and runs it on a D3D11 implementation over Vulkan. `cmake/linux_deps.cmake` fetches and builds these at
configure time into `<build>/_deps` (nothing is committed or redistributed); `wce` links them dynamically.

| Component | Version | Licence | Used for |
|---|---|---|---|
| DXVK (native build) | v3.1.1 | zlib licence | `libdxvk_d3d11.so`, `libdxvk_dxgi.so` and the D3D/DXGI headers (from mingw-w64). Built with one change backported from upstream master: `ResizeBuffers` gives the presenter the new size (a maximized window on Wayland kept its first swapchain) |
| vkd3d | vkd3d-2.1 | **LGPL-2.1-or-later** | `libvkd3d-utils.so`: `D3DCompile` (the editor's helper HLSL), `D3DReflect` (game shader bindings) and `D3DDisassemble` (`fxc_dump`). Built with one local change: `WINAPI` / COM methods use the native x86-64 ABI instead of `ms_abi`, to match DXVK's headers |
| SPIRV-Headers | vulkan-sdk-1.4.321.0 | MIT-style (Khronos) | headers needed to build vkd3d |

## Rust dependencies

The crates.io dependencies are listed in `Cargo.lock` (MIT, Apache-2.0 or zlib licensed, except a few of the updater's, below). Notable: serde, toml and directories (MIT or
Apache-2.0) for the settings file and the system's folders; rpfm_lib (MIT) for the packs and
game formats, windows-core / windows-bindgen (MIT or Apache-2.0) for the D3D11 bindings, and libz-sys (MIT or Apache-2.0) building its bundled
classic zlib (zlib licence) statically for the kit's PNG encoder. The UI uses dear-imgui-rs / dear-imgui-sys (MIT or Apache-2.0; they build
Dear ImGui 1.92.9b docking and cimgui, MIT) and sdl3-sys (zlib licence), linked against the system SDL3 (the one DXVK-native presents through);
the SDL3 platform backend is a port of Dear ImGui's `imgui_impl_sdl3.cpp` and the D3D11 renderer one of `imgui_impl_dx11.cpp` (both MIT).
The gizmos come from dear-imguizmo-sys (MIT or Apache-2.0), which builds upstream ImGuizmo (MIT) without the local `SetAllowedItem`
change: the viewport clears its button from Dear ImGui's hovered and active ids around `Manipulate` instead.

No game assets, shaders or binaries are redistributed. The game's `.fxc` shaders are read from the user's own installation at run time.

The self-updater (`crates/wce_updater`) uses ureq (MIT or Apache-2.0) over rustls (Apache-2.0, ISC or MIT) with ring (Apache-2.0 and ISC),
rustls-webpki and untrusted (ISC), subtle (BSD-3-Clause) and webpki-roots (CDLA-Permissive-2.0: Mozilla's list of root certificates); zip
(MIT) with zlib-rs (zlib licence) to unpack a release, and semver (MIT or Apache-2.0) to compare versions.

## Windows release

The release zip (`.github/workflows/release.yml`) ships `wce.exe` and the `SDL3.dll` that sdl3-sys builds from SDL's source (zlib licence).
