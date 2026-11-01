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

## How the engine reads the quality options (Attila; work in progress)

The `gfx_*` option objects (registered by fn 0x100be850 into an array at 0x11db3ec0 by id: texture 0x14, sky 0x16, unit 0x17,
building 0x18, tree 0x1a, grass 0x1b, terrain 0x1c, effects 0x1e) are only read by the preferences file's code. The renderer reads the
graphics system's live settings struct, at `DAT_12a21d18 + 0x7164c` (the screen's layout above), through getters of its vtable
(0x11c9bb70): +0x1b4 shadows (0x71674: ShadowMapView, fn 0x117665e0: map 1024 / 2048 / 2048 and cascades), +0x1b8 unit detail
(0x7166c), +0x1bc grass (0x7167c), +0x1c0 building (0x71670); trees (0x71678), terrain (0x71694), texture (0x71654) and effects
(0x71690) are read by offset.

- Per model (fn 0x11841280, model type, battle): the level that applies to a model by a type code at its node's +0x50: building detail
  by default; 0x40 grass's; 0x49 .. 0x4b, 0x6c unit detail's; 0x5b, 0x5c, 0x6e, 0x6f trees'; 0x69 terrain's (battle). The type codes
  are not RMV2 material ids (the trees' meshes are 74 / 75, the units' 65, the grass 69, the buildings' 68): which enum is not known.
- Battle LODs (fn 0x1180c370): at a low level a model's first LODs are switched off (enabled byte +0x44 and distance +0x38 zeroed):
  types 0x47 / 0x48: level 0 keeps from LOD 2, level 1 from LOD 1; 0x49 .. 0x4b, 0x5b, 0x5c, 0x6c, 0x6e, 0x6f: level 0 from LOD 1; 0x6b:
  level 0 from LOD 1, else LOD 1 off; 0x41 (with +0x124 off): the last LOD only; the others: all LODs.
- Campaign LOD distances (fn 0x117fbd00, the campaign levels): scale 0.25 / 0.5 / 0.75 / 1 by level (type 0x40: 0.15 / 0.25 / 0.5 / 1).
- Effects (fns 0x118f6290, 0x1191a170): fn 0x118f0a30 maps the level to 2 (levels 0, 1), 1 (2), 0 (3+) for the particle systems.
- Texture quality also sets the tree impostors' step (fn 0x11924370: 0.1 / 0.1 / 0.05 / 0.025, size 1024).

Not found yet: the grass density per level (the kit's `max_grass.xml` holds one density; the engine's `grass_low / med / hi_max_threshold`
tweakables, 0.4 / 0.65 / 1.0, are not read by any code found), what the effects' 2 / 1 / 0 drive, the texture quality's mip drop,
and the type codes' enum.

**Resolved:** the type codes are the RMV2 files' material ids. The v6+ loader (fn 0x1185d050) reads each mesh's type as the first u16 of
its header (the file's material id) and passes it to the mesh's creation (fn 0x1185b0c0); the campaign path reads LOD 0's first mesh's
type. Read so, the battle pass gives: Decal (71) and DecalDirtmap (72) models the building level (level 0 keeps from LOD 2, level 1
from LOD 1), Dirtmap (73), Tree (74) and TreeLeaf (75) models the unit detail level (level 0 drops LOD 0), 91, 92, 110 and 111 the tree
level (level 0 drops LOD 0); the others (DefaultMaterial 68 among them) none. The engine's own name table (0x11f5ab44) lists the same
materials (index = id - 29). The LOD flags are the loaded model's, so every instance of a model takes them; that the model's type is its
LOD 0 first mesh's (the node's +0x50 was not seen written) is the campaign path's reading. Thrones of Britannia has the same two
functions (0x118806e0, 0x118b5d70); Rome 2 has the per-model one (0x1151ab60) but its battle pass is not found.

Earlier leads, for the record: the type code is a model's (its node's +0x50; the campaign path takes its first LOD's first mesh's +0x10): the battle LOD pass
(fn 0x1180c370) runs over a rigid node's children (fn 0x1186a3b0, node class fn 0x117c9580). Ruled out as its enum: the RMV2 material
ids (the files' 74 / 75 trees, 65 units, 69 grass, 68 buildings) and the engine's material name table (0x11f5ab44: index = file id -
29), the shader techniques' ids (`FUN_118c8440`, numbered per shader file: `water_low` .. `water_ultra` 0x27 .. 0x2a) and a type-id
getter beside the classes' name getters (`WATER_MESH`, fn 0x11751d70). Next: what writes the node's +0x50 (the rigid node's load), or
a frame of the game (the LOD each model draws at each level).

## The other options (Attila; Thrones of Britannia has the same functions)

Readers of the live struct (by offset, or the vtable's getters: +0x114 texture as 3 - level, +0x1bc grass, +0x16c / +0x15c / +0x160 /
+0x178 the campaign's):

- Sky (fn 0x11905050): the sky cube ("Sky Large CubeMap") 1024 at levels 1-3, 512 ("Sky Small CubeMap") at 0, where the sky is drawn
  through the 256 cube of the environment map ("Sky Cube EnvMap RT") instead (fn 0x11923ad0). Applied: the editor's sky cube is 1024,
  256 at 0 (it was 256 at every level).
- Screen space reflections = the environment specular pass (environment_specular_light, reflections_blur): on above 0 (fn 0x118c2530),
  its buffers at 0, 0.5, 1 of the screen (table 0x11cca8fc, fn 0x11903ec0); at 0 `g_apply_environment_specular` is 1 (fn 0x11904130: the
  directional light adds the specular). The camera's screen constants stay the full screen's: fn 0x118e0b90 writes the table's value to
  the reflection's and the blurs' `g_buffers_scale` each frame, and the reflection and the x blur divide their pixel's position by it
  (their uv = (position + offset) * g_screen_size.zw / g_buffers_scale, the depth texel = position / g_buffers_scale); fn 0x118ec730
  takes the `_full_res` entries when it is 1. The y blur draws the full screen; its half resolution entry also reads G-buffer channels
  2 (.w weighs a Fresnel term) and 4 (the depth, for the view's direction). Applied: off at 0, half resolution at 1.
- Particle effects: the level is mapped to a variant (fn 0x118f0a30: 0 at 3, 1 at 2, 2 at 0 and 1) and each effect takes that variant
  (fn 0x11894bc0, else variant 0): the vfx files' `quality="1"` / `"2"` property overrides. Applied (`vfx_def::set_quality`). The same
  value sizes the particle system (fn 0x118f96a0 -> its init, fn 0x1184d6e0): 0x40000 (512^2) at 0 when the graphics system's flag
  +0x715c8 is set (the dumps at maximum settings: width 512), else 0x10000 (256^2; 0x4000 only on GPUs below shader model 4, fn
  0x1192e670). Applied as the most particles alive: 512^2 at the highest level, 256^2 below.
- Texture quality (fn 0x1188a3c0): a texture's first mip = min(drop - (3 - bias), size class) when drop (3 - level) > 3 - bias; size
  class 0 below 128, 1 below 256, 2 below 512, else 3; bias a u16 table at 0x11c88670 indexed by the texture's +0x108 (unknown: not the
  RMV2 texture type, whose indices are all 0). +0x108 is the texture's category, the first argument of its creation (Attila's
  fn 0x11700930: category, path, flags). Categories with a bias, by creation site: 0x55 terrain detail maps (2), 0x58 river normals and
  ice (2), 0x53 cloud sets (2), 0x43 the LF normal maps (3), 0x5d-0x5f the LF height and sea maps (1), 0x50 flag normals (3), 0x54 and
  0x40 dissolve (2), 0x5a lens flares (1), 0x62 snow normals (1), 0x2e fire and blood maps, 0x2b vfx turbulence (2); the models'
  `_diffuse` / `_normal` made with a constant category are 0 (never dropped). The meshes' textures (fn 0x1186c880, from the RMV2 mesh
  creation fn 0x1185b0c0) take their category from the mesh factory's +0xc, a value set at run time: not known. Not applied.
- Terrain (+0x48, the graphics system's +0x71694). Its readers, none with a difference the editor can reproduce:
  - fn 0x118e1300: per-frame work budgets for the terrain's streaming (10 at 0, else 25; 1000 at 0 / 1, 2000 at 2 / 3; a config
    variable overrides both). How fast tiles arrive, not how they look.
  - fn 0x118eb8e0: the terrain virtual texture's cache (`virtualtexture.hlsl`, docs/re/terrain.md): atlas 2048 at 0 else 4096; outside the
    campaign extent 0x80000 / 2 levels / 256 pages at 0, 0x100000 / 3 levels / 512 pages above (0x200000 at 3 on some hardware). The
    battle terrain in the game's dumps is the screen-space path (`ps_height_map_cb`), the editor's; the editor has no virtual texture.
  - fn 0x11902d30 (fn 0x1190b5a0: 0.6 at 0, else 1): a range = `terrain_visible_tiles` (config, 600) * the terrain's tile scale * 128,
    used by fns 0x118e06f0 / 0x1192c5a0 to mark the quadtree nodes around the camera into a pool of slots (+0x94, 0x104 bytes each).
    What the slots hold is not traced.
  - fn 0x117f5b00 (vtable 0x11cc19d0 + 0x10c, the meshes of material 67 `ProjectedDecal`, fn 0x1185b0c0's case 0x43): such a decal is
    queued when the level is above 0, or at 0 only while one of two views of the scene (+0x468 / +0x46c, flagged for a redraw at +0x5c
    whenever the terrain changes: fns 0x118eeda0, 0x11902900, 0x11905c30) asks for one. Those views are not identified.
  - fn 0x118debf0 (the terrain node's per-view draw, by the view's type at +0x70): view types 0x3b / 0x3c at 0 also flush a batch
    (fn 0x11811730 -> 0x1189b9a0); view type 0x15 at 0 also draws fn 0x118da4e0's set. The view types are not mapped to names.
  Not applied.
- Grass, trees: no battle reader besides the per-model levels above (the grass getter's only caller is fn 0x11841280's type 0x40; the
  vegetation's own tree LOD uses `tree_lod_scaler` 1.35, no level). In battle they likely change nothing; left as not applied.
- Film grain (ToB; fns 0x117d7da0, 0x1179af90): `commontextures/grain.dds` through `grain_sampler` (linear, wrap) in hdr_to_screen's
  ENABLE_GRAIN variants; `grain_tex_offset` moves when 1000 / grain_fps (24) ms have passed: two numbers of the MSVC generator
  (seed * 0x343fd + 0x269ec3), x the second, y the first, top 16 bits * 1 / 65535. Applied. The texture is UNORM: its creation flags
  (0x280) lack 0x2000, the sRGB bit of the texture creation (fn 0x117700d0: `_diffuse` 0x2180 / 0x2100, `_normal` 0x180,
  `no_texture_srgb` 0x2002180 against `no_texture` 0x2000180, the sun and moon disks 0x2100).
- Cloud shadows (ToB; fn 0x117c3680): off (the graphics system's +0x747fe clear): g_cloud_shade_params 0, the shade never applies. On,
  in battle: `terrain/textures/common/cloud_shade.dds` through `cloud_shade_sampler`, params (offset x, y, cloud_shade_battle_scale
  0.0005, 1.0); the offset moves by the change of the battle's seconds (its +0x378, vtable 0x108) * cloud_shade_battle_speed (50) /
  4096 along the wind's direction, wrapped into 0..1 (the campaign: cloud_shade_campaign_scale 0.01, w 0.998). The direction is fn
  0x118089e0 on the battle's weather (+0x21f8): (cos, sin) of its degrees (+0x30) * 0.0174533, the battle's vtable 0x78 that also
  fills the shaders' wind direction (fn 0x1199a770); fn 0x1196ebd0 scales it by the gusting strength for the wind's velocity. The
  texture is UNORM (creation flags 0x180, no sRGB bit 0x2000; see the grain). Applied.
- God rays (ToB; the views of fn 0x117a88a0, before hdr_to_screen, which adds 0.1 x their red through `god_rays_sampler`):
  HDR_GOD_RAYS (fn 0x1174a6c0, half the view's size) draws hdr_god_rays `ps30_god_rays`: 1 where the depth a step of min(d, 0.1) from
  the sun towards the pixel is the sky's (>= 0.99999), faded by (1 - min(1.7 d, 1))^2, d the pixel's distance to the sun in uv;
  `sun_position` (fn 0x11798900) = the view projection of the camera plus the direction to the sun ((1, 0, 0) under 0.01 long):
  ((x / w + 1) / 2, 0.5 - y / w / 2, z / w), with no test for a sun behind the camera. HDR_GOD_RAYS_BLURX / _BLURY (fns 0x1174a190 /
  0x1174a2c0, scale 1) blur its red with gaussianblur `ps30_gaussian_horizontal_blur_R_channel_wide` / `vertical_..._wide` (the blur
  shader table's 10 and 11, fn 0x11745a90), stepping a texel of their input (`g_input_size_ps` = its size, fn 0x11798830). Applied
  (`Renderer::pass_god_rays`). The targets (fn 0x1197b480): "Screen Texture"s of their scale (fn 0x117701b0; the size is the screen's
  * the scale, truncated, fn 0x11808810), in the first of TTF_L8 / TTF_A8R8G8B8 the device takes (fn 0x117ca2f0), TTF_L8 being
  R8_UNORM (fn 0x118163e0; the TTF_* enum counts from TTF_FROM_FILE 0, names at 0x11d83d84). The samplers are D3D11_SAMPLER_DESCs
  from their parameter's +8 (fn 0x117d9760: filter, address u, v, w, ...; 16 copies with MinLOD 0..15): `depth_sampler` point
  clamp, the blurs' `s_input_sampler` point wrap (fn 0x11745a90; the y blur's taps past the bottom edge read the top's: a faint strip
  along the bottom under a sun near the top), hdr_to_screen's `god_rays_sampler` linear clamp (fn 0x1174b440).
- Anti-aliasing 1, MLAA (Attila, ToB; the views of fn 0x117377d0, after the lens flare and vignette, on the final image: the image view
  is fn 0x116dd4e0's): MLAA_PASS1_EDGES (fn 0x116df850) draws `mlaa_pixelshader_seperatinglines` with `g_LuminanceThreshold` 0.1 (the
  view's +0x68; the cbuffer's default 0.0625, fn 0x116ce880); MLAA_PASS2_PREPASS (fn 0x116e0240) clears; MLAA_PASS2_BLEND_WEIGHTS (fn
  0x116e0100), registered three times, draws `mlaa_computelinelength_ps` with `g_IterationDepth` = its draw count mod 3 (fn 0x11728db0),
  reading the line lengths written last and writing the other pair (fn 0x11749530 swaps them); MLAA_PASS3_FINAL_BLEND (fn 0x116e0750)
  draws `mlaa_blend_ps` into the frame. The edge mask and the four line length targets are R8_UINT at the view's size on DX11 (fns
  0x11765d60, 0x11765e60: TTF_R8_UINT, else TTF_R16F / TTF_G16R16F). The sm_4_0 shaders (mlaa_pass*_sm4). Same in ToB (its pass 1
  0.1, pass 2 3 iterations). Applied (`Renderer::pass_mlaa`; the editor blends into a target of its own, copied back). The MSAA
  levels (2..4) are not: drawn without anti-aliasing. Rome 2 has the same shaders; its engine is not checked: not applied.
- Alpha blending (+0x70; 1 standard OIT, fn 0x11763cc0: with feature level above 2; 2 Intel's AOIT, fn 0x11725c20; the automatic
  downgrade "Disabled Alpha-blending" sets 0, fn 0x1173da30):
  - Which geometry (fn 0x117f3780 the grass, fn 0x117f3a74 the leaves): a renderable whose squared distance (+0xbc) is within
    OIT_GRASS_MIN / MAX_DISTANCE (or the TREE ones; AOIT_* for AOIT; tweakers, defaults 0 and 25) goes to the OIT pass (render list
    0x10) only; the rest to lists 0x14 and 0x15 (the G-buffer's).
  - Textures: the material resolver (fn 0x1186c880) loads `<name>_ab.dds` for the diffuse (slots 0 and 0x11) of mesh categories 0x5c
    and 0x40 with either on, for every pass. The packs have them for the leaves, billboards and grass, not the bark.
  - Buffers (fn 0x11766000): per pixel a head index (raw, TTF_R32_TYPELESS), and 10 fragments per pixel of 12 bytes with a counter.
  - OIT_PARENT (fn 0x116e24e0, filed after the deferred lights, before the DEFERRED_ALPHA views, fn 0x11736a90):
    OIT_STORE_ALPHA_FRAGMENTS (fn 0x116e2910) clears the heads to 0xffffffff (fn 0x11729000) and draws `ps30_forward_lit_OIT`
    (rigidleaf_v3) / `ps30_forward_lit_OIT_grass` (rigidgrass): lit forward, each fragment (colour in 6.4/6.5 bits a channel, alpha,
    depth) appended with an atomic exchange of the head (0xfffffffe there when the list is full).
  - The depth-stencil states "first pass" / "post first pass" (fns 0x116e2470, 0x116e2580): LESS_EQUAL without depth writes; stencil
    ALWAYS / REPLACE, then EQUAL (`oit_multi_pass` off).
  - OIT_SORT_ALPHA_FRAGMENTS_RESOLVE_COLOUR (fns 0x116e25f0, 0x116e2690) draws `ps50_sort_alpha_fragments_resolve_colour`: sorts up to
    64 fragments by depth, composites them front to back (stopping at 0.99), blended ONE / INV_SRC_ALPHA over the light buffer;
    pixels without fragments (or full lists) discarded.
  - AOIT's (`ps30_forward_lit_AOIT*`, aoit.hlsl) orders its fragments through `g_IntelExt` (Intel's pixel synchronisation).
  - Applied (Attila, ToB; `Renderer::pass_oit`, after the fog, before the faded meshes; the near leaves are split off in the draw set's
    cull, `cull_split`, by the item's bounding sphere centre; the grass by the patch). AOIT is drawn as the standard OIT.
  - Not done: the stencil marks (the editor's stencil bits mean other things; the resolve discards empty pixels anyway); the shadow
    pass's diffuse stays the plain one.
  - Open: the forward-lit leaves come out lighter than the same leaves through the G-buffer (not SSAO; shadows reach them). Which one
    the game's look is was not checked in game.
- Tessellation: displacement tessellation of rigid meshes (`rigid_tesselation_factor`, `t_tesselation_displacement_map`); needs hull and
  domain shaders. Not applied.

## What the editor applies

- Shadows: the cascade split row (Attila, ToB; Rome 2's renderer draws its four-cascade row whatever the level).
- Building detail, unit detail (Attila, ToB): the battle pass's LODs switched off (`draw_set::LodQuality`): a switched off LOD is passed
  over by the LOD selection (its distance 0, not enabled), in every view (camera, shadows, picking). Trees: the level only reaches the
  materials above (none of Attila's trees use them; its vegetation's density and LOD per level are not found): not applied.
- Water: the water planes' and the sea's pixel shaders, `ps30_battle_<level>_water_quality` / `ps30_main_<level>_water_quality`
  (the engine registers the techniques `water_low` .. `water_ultra`, fn 0x116ec510); only ultra has the variant with shadows, the
  lower levels draw the plain one. Not Rome 2's (its levels' mapping is not known): ultra.
- Texture filtering: the "quality" samplers, trilinear at 0, anisotropic 2^n (fn 0x1177f410; docs/re/terrain.md). Not Rome 2's (not
  checked in its engine): anisotropic 8x.
- Sky, screen space reflections (0 off, 1 half resolution), particle effects, MLAA, alpha blending (Attila, ToB): above.
- ToB's film grain, cloud shadows and god rays: above.
- SSAO, depth of field, distortion, vignette, gamma, brightness: as before.

Not applied yet (drawn at the highest level, or off for the effects the editor does not draw): MSAA (and Rome 2's MLAA), texture
quality, trees, grass, terrain, tessellation, Rome 2's alpha blending, grain, cloud shadows, god rays, lightning and foliage hiding.

## Editor only: the render scale

Not a game option: the scene at 50 .. 100 % of the viewport's width and height, enlarged with NVIDIA Image Scaling
(`renderer/nis.rs`, `third_party/nis`). The editor's overlays are drawn at the scene's size before the enlargement (they test its
depth). At 100 % the frame is drawn as before.
