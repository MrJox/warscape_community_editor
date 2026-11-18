# Save/Export: prompted and emergency Save contracts

The final Save/Export contribution includes corrections for the four maintainer findings reported on 18 November 2026. The supported workflows and remaining native-normal limitations are in [Terrain Save/Export](../plans/terrain_save_export.md).

## Emergency saving without an editor frame

`Editor::save_all_documents` is the graphics-device-loss path. It calls `DocumentManager::save_all_without_prompt`, which attempts every modified, saveable document in opening order. Outside changes are refused without creating a pending modal, and an error in one document does not stop later documents. Failures remain dirty; successful saves are recorded normally.

If ordinary saving leaves failures, the existing autosave system writes recovery copies of the remaining edits, even if periodic autosave is disabled. The native error dialog reports the ordinary Save failures and recovery-copy outcome. The device-loss exit retains the session, so the next launch can offer its copies. An inability to write a recovery copy is reported rather than presented as Save success. No outside file is implicitly approved for overwrite during emergency saving.

## Save As retains the original's outside-change warning

`Changes::finish_copy` records the completed copy destination without replacing the document's source observations. Per-file publication already records the files this document itself wrote. Overwrite for a separate copy therefore leaves unreviewed TEd edits to the original visible to a later plain Save. The owned Save destination, dirty state and history remain unchanged by the copy.

## One modal and a resumable action

When closing, replacing or quitting has already opened the unsaved-changes modal, the disk-change choice is rendered inside that modal. A second root modal is not opened. After a successful Overwrite, the shell resumes the pending action's Save phase. Documents already saved are skipped; another conflict asks about the next document without repeating completed Saves. Opening, closing or quitting occurs only after all required Saves succeed.

Standalone Save All keeps its remaining work across each disk-change choice as well. Cancel stops its continuation and keeps the remaining edits unsaved.

Both modal renderers handle a false `begin_popup_modal` result with a cleared `open` flag as cancellation before returning. X, Cancel and Escape clear the pending Save and cancel the enclosing close/open/quit action. A failed Overwrite leaves the document open and retains the enclosing prompt for retry, discard or cancel. Cancellation does not undo documents already saved earlier in a multi-document action.

## Regression coverage

Nine added tests exercise emergency best-effort saving, multiple Save All conflicts and cancellation, source-change retention after an overwrite-copy, close/open/quit continuation, sequential quit conflicts, failed overwrite, actual modal X clicks and emergency recovery with periodic autosave disabled.

```text
cargo test --locked -p wce_editor_core --test editor_check save_routing
cargo test --locked -p wce_terrain_editor --lib save_as_overwrite_keeps_outside_source_changes
cargo test --locked -p wce_editor --lib save_prompt_tests
```

The shell modal tests run on Windows using a software D3D11 WARP device only to construct the shell. They create real Dear ImGui frames, feed in-memory mouse events to the real Overwrite/X buttons, and use isolated settings and synthetic documents. They create no native window, need no game assets and do not simulate a real driver crash. Core save routing and the real Terrain copy regression run on every platform.
