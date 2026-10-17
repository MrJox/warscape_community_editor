//! Dear ImGui on the editor's window and device. The platform backend is a port of Dear ImGui's `imgui_impl_sdl3.cpp` (the system SDL3 is
//! the one DXVK presents through, so the editor links that one instead of a second SDL3); the renderer backend is the D3D11 one of
//! `imgui_impl_dx11.cpp`, with the managed textures of Dear ImGui 1.92 and user textures for render targets shown in panels.

pub mod platform;
pub mod renderer;
