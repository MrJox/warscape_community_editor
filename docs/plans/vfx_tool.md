# The VFX tool

A tool of its own for `vfx/*.xml` effects, edited visually: the effect plays on the sample map (or the open tile) while it is edited, its
spawn shapes and references are handles in the viewport, its curves and colours are drawn and dragged in the Inspector. Before it, VFX files
were a category of the Weather tool's Files panel, edited as a raw XML tree (docs/plans/weather.md, "Open" item 1).

State: planned and built 2026-11-18 with the user (decisions below) on `feature/vfx_tool`, steps 1 to 7 done; user review pending.
Behind the `vfx` feature of `wce_app`, off by default. What the tool does: [docs/editor.md](../editor.md), "VFX tool".

## Decisions (user, 2026-11-18)

- **Preview**: the Weather tool's sample map (or the open tile's ground), the effect played at a spot the user moves; lit by the editor's
  weather preview (the Tile and Terrain tools' weather chips; else the Weather tool's open environment, taken when the tool
  activates), else the default day. Camera-following effects play around the camera.
- **Visual editing, all four in the first version**: live playback controls (play, pause, restart, speed, loop), viewport gizmos (the
  spawn area, ring and point, the references' offsets and scale, the spawn direction), curve and colour editors in the Inspector, emitter
  solo / mute (preview only, never written to the file).
- **Inspector**: the shared Inspector framework (`wce_editor_core::inspector`, drawn by `wce_editor::panels::ImGuiInspector`), with the
  look of the other tools' Inspectors: one collapsible section per emitter, behaviour chain and reference; typed, labelled fields with
  the property's XML name and meaning in the tooltip. No tree panel (the Outliner was removed on purpose).
- **The Weather tool keeps no VFX**: its Files panel loses the VFX category, and it no longer opens `vfx/` files, whether the `vfx` feature
  is on or not. Weather definitions still name their effects (`vfx1`, `vfx2`) and precipitation still plays them.
- **Byte-identical round trip** as everywhere: edits are span replacements of the lossless `XmlDoc`; a curve edit replaces the text of its
  `ranged_property` only.

## What it builds on

- `XmlFileDocument` (`wce_weather`): a lossless XML document with undo, save to the project's loose folder, autosave. VFX files open as
  one, owned by the VFX tool.
- The particle system (`wce_render::particles`): a scene entity with `Payload::VfxEmitter` plays `vfx/<name>.xml` at its world matrix (the
  path tile VFXInstances use), reading files through a reader that sees the open documents' edits first.
- The weather tool's live overlay (open vfx / weather XML -> the particle reader): moved into the editor (`Editor::particle_overlay`) so
  edits show live whichever tool is active.

## Steps

1. Infrastructure: the editor-level particle overlay (with per-tool preview overrides, for mutes and drags), tool routing by path prefix
   (`Tool::claims_path`), placed effects reloading when an open vfx file changes, a particle time scale, `XmlDoc::with_inner_text`, an
   `edit_curve` Inspector row.
2. The `wce_vfx_tool` crate and the `vfx` feature: Files panel, the document, the preview entity on the sample map, the Weather tool's VFX
   category and placeholder tab removed.
3. The Inspector: file attributes, playback, emitters (grouped properties), behaviour chains and modifiers, render stacks, references;
   duplicate / delete of emitters, chains and references.
4. Playback and solo / mute.
5. Gizmos: spawn area box, ring, spawn point, emission direction cone; reference offsets and scale; drag to edit.
6. Docs (docs/editor.md, weather.md, PLAN.md).
7. The follow-ups of the first version (2026-11-18): new effects (a copy, or one that plays a library), adding a property with
   vanilla's value and a quality variant, the box's height handle, placed effects reloaded only when their files change, scripted
   handle drags (`vfx_handles`, `vfx_drag_handle`).

## Checked (2026-11-18, scripted runs on vanilla Attila files)

- `burning_entity` (a reference file) and `library_burning_entity` (ten emitters) open, play and list their parts; solo and mute change
  what plays without touching the file.
- An attribute edit and a curve edit, undone and redone, saved: the file differs from the pack's in those two spans only (UTF-16,
  CRLF kept). Duplicate emitter (`embers_copy` with its own stack), delete emitter (its stack goes), a refused chain delete (in use),
  duplicate chain, undo / redo of them: the saved file is whole.
- Handle drags by script (`vfx_drag_handle`): the ring's outer radius 0 -> 3.37 m and the box's height 0 -> 4.71 m on `embers`, each
  one undo entry. Found and fixed with it: a size-0 shape's dots sat on the move gizmo's arrows, and the gizmo took the press.
- New effects: `test_fire` (plays `library_burning_entity`) reads and plays as one reference of it, its text the game's layout but the
  reference's name; a copy is the open file byte for byte; taken names are refused.
- Adding properties: vanilla's continuous_mesh emitters all have the same set but the sound events; `SoundEventStart` added to
  `smoke` and a quality 1 `LifeTime`, saved: two lines in the emitter, in the file's indentation and line ends, nothing else.
- A placed effect follows its file: `ParticleLifeTime` 0.1 against 10 on a soloed `fire`: 3 particles against 92.
- Not checked by a script: curve keys dragged in the Inspector (the mouse command reaches the viewport only); what they write is
  unit-tested (`curves.rs`).

## Left elsewhere

- The engine's own defaults for a property (docs/plans/weather_current_state.md item 5): until they are decoded, *Add a property* uses
  vanilla's most common value.
- Curve baking (weather_current_state.md item 4): the curve editor draws keys joined linearly, as the runtime evaluates them.
