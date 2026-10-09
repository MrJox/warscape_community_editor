# Third-party code

Everything here is vendored under `third_party/` with its licence file. Permissive licences only. Ask before adding anything.

| Component | Version | Licence | Where | Used for |
|---|---|---|---|---|
| Dear ImGui | 1.91.9 (release branch; the `1.91.9-docking` branch replaces it in Phase 4) | MIT | `third_party/imgui` (DX11 and Win32 backends only; `LICENSE.txt` inside) | all editor UI |
| zlib | 1.3.1 | zlib licence | `third_party/zlib` (deflate and inflate sources only; the licence text is in the header of `zlib.h`) | PNG encode and decode that reproduces the kit's encoder byte for byte (`formats/png_file`) |

Approved for later, not added yet: ImGuizmo (MIT), for the viewport gizmos (Phase 4).

No game assets, shaders or binaries are redistributed. The game's `.fxc` shaders are read from the user's own installation at run time.
