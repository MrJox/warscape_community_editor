# The VFX tool

A tool of its own for `vfx/*.xml` effects, edited visually: the effect plays on the sample map (or the open tile) while it is edited, its
spawn shapes and references are handles in the viewport, its curves and colours are drawn and dragged in the Inspector. Before it, VFX files
were a category of the Weather tool's Files panel, edited as a raw XML tree (docs/plans/weather.md, "Open" item 1).

State: planned and built 2026-11-18 with the user (decisions below), in master; user review pending. Behind the `vfx` feature of
`wce_app`: in the default features, not in the `release` set. What the tool does: [docs/editor.md](../editor.md), "VFX tool".

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

## As built (2026-11-18)

- Infrastructure: the editor-level particle overlay (`wce_editor::particle_overlay`, with per-tool preview overrides for mutes and
  drags), routing of `vfx/` files to the tool (`Tool::opens_under`), placed effects reloaded only when their open file changes, a
  particle time scale (`particle_time_scale`), `XmlDoc::with_inner_text`, an `edit_curve` Inspector row.
- The `wce_vfx_tool` crate: Files panel, the document, the preview entity on the sample map; the Inspector (file attributes, playback,
  emitters, behaviour chains and modifiers, render stacks, references; duplicate / delete of emitters, chains and references);
  playback and solo / mute; gizmos (spawn box with its height handle, ring, spawn point, emission cone, reference offsets and scale,
  dragged to edit); new effects (a copy, or one that plays a library); adding a property with vanilla's value and a quality variant.
  Script commands `vfx_handles`, `vfx_drag_handle`.
- Checked by scripted runs on vanilla Attila files (2026-11-18): `burning_entity` and `library_burning_entity` open, play and list
  their parts; attribute, curve and structural edits with undo / redo save with only their spans changed (UTF-16, CRLF kept); handle
  drags are one undo entry each; new effects and added properties write the game's layout; a placed effect follows its file. Curve
  keys dragged in the Inspector are unit-tested only (`curves.rs`): the scripted mouse reaches the viewport alone.

## Left elsewhere

- The engine's own defaults for a property (docs/plans/weather_current_state.md item 5): until they are decoded, *Add a property* uses
  vanilla's most common value.
- Curve baking (weather_current_state.md item 4): the curve editor draws keys joined linearly, as the runtime evaluates them.
