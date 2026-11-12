# Editor: to do later

Editor-wide ideas, noted for later (user, 2026-10-24). Status (2026-10-26): all three open.

## Minimap with views

Done (2026-10-25): the Tile tool's Minimap panel (docs/editor.md, "Minimap"): the colour, height or no base, with the siege AI, the deployment
zones and the camera as layers. Done (2026-10-28): the ground views unified (a chip on the viewport and one on the minimap, docs/editor.md,
"Ground"), every view on the minimap as its own base, the tile's cells and the automatic no-go areas as layers.

## Save: log the siege AI's problems

When a tile is saved or exported, write a short error to the log for each broken part of its siege AI. The checks exist in
`wce_siege::check` (`check`, `check_captures`), but today they run only inside the siege AI generation
(`wce_tile_editor` `tile_document/siege.rs`), not on save. They would run on `Tile::current_siege_layout()`:
- no fort perimeter;
- an edge's end in no node's rectangle;
- a node without a rectangle or Boundary;
- the street graph in parts;
- the main capture point in no area;
- walls, separators and no-go regions not closing the ring.

These are only log lines: the save still goes ahead. The natural place is next to the vegetation problems, which a save already puts at
the front of its message (docs/editor.md, "Problems").

## Autosave

Autosave of modified documents, from the old tile plan's cross-cutting items; nothing exists yet. To decide: where the copies go (never
over the user's file) and how they are offered back after a crash.

## Share / Import shared project: visible errors

User, 2026-11-12: share and import work, but when one fails it is hard to see why. Today a failure is only the status line's text
(`share failed: ...`, `import failed: ...`, `share_prompt.rs`), easy to miss and gone at the next message, and the reasons a share can
refuse (an unsaved tile, no database entry, the tile not in a kit's layout, a file that does not read) or an import can (not a shared
zip, another game's, a path outside the projects folder, the shared tile open, a file that does not write) never reach the log. To do:
show a failure in a dialog (or a lasting notice) with the reason and what to do about it, write it to the log, and list there the
prefabs a share left out and an import could not find.

Done (2026-11-12, `share_prompt.rs`): a failure (menu, prompt or console) opens a dialog with the reason (red) and what to do about it, and
is a red line in the log; a share that leaves out prefabs found nowhere, and an import that lacks prefabs, open it too with the list (a
warning in the log).

## The Inspector comes to the foreground when it changes

User, 2026-11-12: when something changes what the Inspector shows (a selection, Flip mode's panel, a brush's settings, a placing's
Inspector), and the Inspector is a tab that is not the one shown in its dock (behind Weather or Tile, say), it is brought to the
foreground, so that what changed is seen. Only on a change of what it shows, not on every frame: a tab the user picked must stay picked
while they work in it.

Done (2026-11-12): docs/editor.md, "Layouts".
