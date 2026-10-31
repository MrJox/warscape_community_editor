# The games' graphics options and presets

The editor mirrors each game's graphics options (`wce_foundation::graphics`: the `gfx_*` lines of
`%APPDATA%/The Creative Assembly/<game>/scripts/preferences.script.txt`, the options screen's levels, the presets) and applies the
ones its renderer reproduces. Settings > Game > Graphics: the game's own file (the default), or the editor's copy per game
(`[games.<id>.graphics]`), which never writes the game's file. Addresses are RVAs of this machine's `empire.retail.dll`
(image base 0x10000000; Attila's project `attila`, Thrones of Britannia's `thrones`, Rome 2's `rome2` in `ghidra_work`).

## The options screen's settings struct (Attila, Thrones of Britannia)

The options screen and the graphics system keep the options in one 0x78-byte struct (copied by fn 0x115f0f80 in Attila). Its fields,
from the screen's code (the "downgraded" icons compare two structs field by field per dropdown, fn ~0x115dc600; the check boxes read
the panel's struct at panel + 0x80, fn ~0x115dcaa0; Thrones of Britannia's at 0x1646800 .. 0x1647e00):

| Offset | Field | Offset | Field |
|---|---|---|---|
| 0x00 | anti-aliasing (`dropdown_aliasing`) | 0x30 | grass |
| 0x04 | texture filtering | 0x34 | water |
| 0x08 | texture quality | 0x38 | unit size |
| 0x0c | (3 in every preset) | 0x3c | fleet size |
| 0x10 | SSAO (byte) | 0x40 | sky |
| 0x14 | depth of field | 0x44 | particle effects |
| 0x18 | distortion (byte) | 0x48 | terrain |
| 0x19 / 0x1a / 0x1b | ToB: grain, cloud shadows, god rays (bytes) | 0x4c / 0x4d | windowed, v-sync (bytes) |
| 0x1c | screen space reflections | 0x50, 0x5c | strings (the resolution, kept by a preset) |
| 0x20 | unit detail | 0x6c | vignette (byte) |
| 0x24 | building detail | 0x75 | unlimited video memory (byte) |
| 0x28 | shadows | | |
| 0x2c | trees | | |

## Presets

The slider (`gfx_preset_slider`, fn 0x115dc260) picks one of five structs the graphics system returns (virtual +0x48 .. +0x58: low,
medium, high, ultra, extreme; the slider's position 2 is high, or custom when the settings match none) and copies it over the panel's,
keeping the window, the resolution and +0x68 (fn 0x116165e0). The structs are filled by fn 0x1175c2f0 (Attila) / 0x117ce2f0 (ToB):

| | Low | Medium | High | Ultra | Extreme |
|---|---|---|---|---|---|
| Anti-aliasing | 0 | 0 | 0 | 1 (MLAA) | 3 (MSAA 4x) |
| Texture filtering | 0 | 0 | 0 | 1 | 2 |
| Shadows, water, trees, grass, terrain, sky, building, unit detail, particle effects | 0 | 1 | 2 | 3 | 3 |
| SSAO | off | off | off | on | on |
| Depth of field | 0 | 0 | 0 | 0 | 1 |
| Distortion | off | on | on | on | on |
| Screen space reflections | 0 | 0 | 1 | 2 | 2 |
| Vignette, v-sync | off | off | off | off | off |
| ToB: grain / cloud shadows / god rays | off / off / off | off / on / off | off / on / on | off / on / on | off / on / on |

Extreme is Ultra copied (fn 0x1175c2f0: `LEA ESI, ultra` + copy into extreme) with these changes; its anti-aliasing is 3 unless the
device's class (virtual +0x370) is 2. Every preset's texture quality is by the card's video memory (virtual +0x194, MB): up to 800: 0,
up to 1600: 1 (0 when the device class is 2), up to 2600: 2, above: 3. Unit and fleet sizes come from fns 0x11730f60 / 0x11730f20
(not the renderer's).

## Rome 2

The same struct (0x7c bytes; the options screen's panel holds it at +0xc8: the code at 0x11292400 .. 0x11293a80): +0x1c .. +0x1f are its fade near units, hide
dead bodies, hide foliage and lightning check boxes (no screen space reflections), +0x70 alpha blending (`dropdown_aoit`). Its quality
options have six levels (Low, Medium, High, Very High, Ultra, Extreme: the options screen's layout) and the slider six presets, filled
by fn 0x1144c3b0:

| | Low | Medium | High | Very High | Ultra | Extreme |
|---|---|---|---|---|---|---|
| Anti-aliasing | 0 | 0 | 0 | 0 | 0 | 3 |
| Texture filtering | 0 | 0 | 0 | 0 | 0 | 1 |
| Shadows | 0 | 0 | 2 | 3 | 4 | 5 |
| Water, trees, grass, terrain, sky, building, unit detail, particle effects | 0 | 1 | 2 | 3 | 4 | 5 |
| SSAO | off | off | off | off | off | on |
| Distortion | off | off | off | on | on | on |
| Cloud shadows / god rays | off / off | on / off | on / on | on / on | on / on | on / on |
| Depth of field, grain, hide foliage, vignette, alpha blending | off | off | off | off | off | off |
| Texture quality | memory | memory | memory | 2 | memory | memory |

By video memory (virtual +0x19c, MB): above 1200: 3, above 800: 2, above 600: 1, else 0. Its water techniques are registered in the
other order (`water_ultra` 0x1f .. `water_low` 0x22, fn 0x113ded70) and how its six levels pick one of the four is not found yet.

## What the editor applies

- Shadows: the cascade split row (Attila, ToB; Rome 2's renderer draws its four-cascade row whatever the level).
- Water: the water planes' and the sea's pixel shaders, `ps30_battle_<level>_water_quality` / `ps30_main_<level>_water_quality`
  (the engine registers the techniques `water_low` .. `water_ultra`, fn 0x116ec510); only ultra has the variant with shadows, the
  lower levels draw the plain one. Not Rome 2's (its levels' mapping is not known): ultra.
- Texture filtering: the "quality" samplers, trilinear at 0, anisotropic 2^n (fn 0x1177f410; docs/re/terrain.md). Not Rome 2's (not
  checked in its engine): anisotropic 8x.
- SSAO, depth of field, distortion, vignette, gamma, brightness: as before.

Not applied yet (drawn at the highest level, or off for the effects the editor does not draw): anti-aliasing, texture quality,
trees, grass, terrain, sky, building detail, particle effects, screen space reflections, alpha blending, tessellation, ToB's and
Rome 2's grain, cloud shadows, god rays, Rome 2's lightning and foliage hiding.

## Editor only: the render scale

Not a game option: the scene at 50 .. 100 % of the viewport's width and height, enlarged with NVIDIA Image Scaling
(`renderer/nis.rs`, `third_party/nis`). The editor's overlays are drawn at the scene's size before the enlargement (they test its
depth). At 100 % the frame is drawn as before.
