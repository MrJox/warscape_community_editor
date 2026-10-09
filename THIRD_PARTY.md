# Third-party code

Everything here is vendored under `third_party/` with its licence file. Permissive licences only. Ask before adding anything.

| Component | Version | Licence | Where | Used for |
|---|---|---|---|---|
| Dear ImGui | 1.91.9, docking branch (tag `v1.91.9-docking`, commit 126d004) | MIT | `third_party/imgui` (DX11 and Win32 backends only; `LICENSE.txt` inside) | all editor UI, docking and multi-panel layouts |
| ImGuizmo | master of 2026-08-08 (commit 18cef5e) | MIT | `third_party/imguizmo` (`ImGuizmo.cpp/.h`, `LICENSE`) | translate / rotate / scale gizmos in the viewport |
| zlib | 1.3.1 | zlib licence | `third_party/zlib` (deflate and inflate sources only; the licence text is in the header of `zlib.h`) | PNG encode and decode that reproduces the kit's encoder byte for byte (`formats/png_file`) |

No game assets, shaders or binaries are redistributed. The game's `.fxc` shaders are read from the user's own installation at run time.
