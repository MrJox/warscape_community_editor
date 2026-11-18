# The VFX tool

A tool of its own for `vfx/*.xml` effects, edited visually: the effect plays on the sample map (or the open tile) while it is edited, its
spawn shapes and references are handles in the viewport, its curves and colours are drawn and dragged in the Inspector. Before it, VFX files
were a category of the Weather tool's Files panel, edited as a raw XML tree (docs/plans/weather.md, "Open" item 1).

State: planned 2026-11-18 with the user (decisions below), built on `feature/vfx_tool`. Behind the `vfx` feature of `wce_app`, off by
default.

## Decisions (user, 2026-11-18)

- **Preview**: the Weather tool's sample map (or the open tile's ground), the effect played at a spot the user moves; lit by the previewed
  weather (View > Weather preview / the editor's weather preview), else the default day. Camera-following effects play around the camera.
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

## Follow-ups

- New effect from a template; adding a property the file lacks with the engine's default (needs the schema's defaults:
  docs/plans/weather_current_state.md item 5).
- Curve baking is not decoded (weather_current_state.md item 4): the curve editor draws keys joined linearly, as the runtime evaluates them.
