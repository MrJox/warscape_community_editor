# Terrain work transferred to maintainers

The contributing project is ending major Terrain development. These references preserve useful source, tests and findings for Frodo and Victimized. They are **unfinished or historical development branches**, not proposed replacements for current master. The ready-feature contributions are Reliable Terrain Save/Export and exact land-LF region PNG interchange.

## Source references

| Work | Inspect source | Status and adoption boundary |
|---|---|---|
| Native battle original-baseline persistence | [`handoff/source-native-battle-baseline`](https://github.com/Jordanakoos/warscape_community_editor/tree/handoff/source-native-battle-baseline), `d1ec429ff6c957e24fc8e95112748221e9a7a57c` | First-round corrections are implemented. Further independent review found remaining copy/reference-validation issues. Do not treat as accepted or ready for upstream merge. |
| Existing native campaign prop controls | [`handoff/source-native-props`](https://github.com/Jordanakoos/warscape_community_editor/tree/handoff/source-native-props), `1ac29cd3a678cdd6054d974552732e1658b42909` | Model/I/O is already upstream in #59. Inspector/gizmo/history and horizontal translation remain historical development source. Reconcile with current grounding, coordinate, selection and Save APIs. |
| Shared campaign validation | [`handoff/source-campaign-validation`](https://github.com/Jordanakoos/warscape_community_editor/tree/handoff/source-campaign-validation), `5f6dca2595dda2c75946852e228e541e4f84d4e2` | Accepted at the editor-functional level on its historical base. Contains held height-authoring ancestry. Extract only necessary read-only queries and actual diagnostics with their guard owners. |
| Incremental Terry prop preview | [Fork #8](https://github.com/Jordanakoos/warscape_community_editor/pull/8), `8be7b8c7aad1d89567235b00b16051001834f0f5` | Historical interaction improvement, not current-master validation. Upstream now grounds props/trees/buildings on drawn terrain, tracks loaded tiles and uses newer campaign coordinates. Retain those behaviors when adapting the incremental path. |
| Campaign-height authoring | [#52](https://github.com/MrJox/warscape_community_editor/pull/52), fork [#5](https://github.com/Jordanakoos/warscape_community_editor/pull/5), [#6](https://github.com/Jordanakoos/warscape_community_editor/pull/6), [#7](https://github.com/Jordanakoos/warscape_community_editor/pull/7) | Held preparation and dependent source. No complete native strategic-height export. Header/profile restrictions, consumer integration and native campaign normals remain unresolved. |

Fetch references without replacing an existing worktree:

```text
git remote add jordan-handoff https://github.com/Jordanakoos/warscape_community_editor.git
git fetch jordan-handoff 'refs/heads/handoff/*:refs/remotes/jordan-handoff/handoff/*'
git show jordan-handoff/handoff/source-native-battle-baseline:docs/formats/terrain_authoring.md
```

Use another remote name if it already exists. The reference branches use older upstream foundations; their whole-tree diffs against current master include unrelated upstream history. Compare each contribution against its recorded base before extracting it.

## Native battle persistence

The candidate retains immutable original LF, encoded normals and CHMF outside game-resource directories. This makes continuous, Save-between-edits and fresh-process reopened editing produce identical output on the bounded fixture, including all eight normal mip levels. The original normal algorithm and encoder are unchanged. The candidate records 446 passing tests and eight workflow plus thirteen adversarial application sessions; this does not close issues found by independent probes.

The first independent review rejected five defects involving lost authority during recovery, incomplete completion inventories, missing-original editable recovery, rejected-copy side effects and retained normal snapshots. The candidate's `docs/investigations/fb_phase4_r1_corrections.md` maps those corrections to source and permanent regressions. Recovery metadata becomes version 2; version 1 slots revalidate their owner, while older WCE cannot read new slots.

Further independent probes identified two remaining risks requiring maintainers' attention:

- A redirected destination authoring subdirectory can send copy writes into the source native map or its authoring store. Preflight must validate actual destinations used by materialization, including intermediate junctions, before any writes.
- A missing normal reference whose key is absent from the original inventory can be tolerated as though it were a recognized pinned original. Recovery exceptions must require an exact inventoried key and verified binding; unrelated missing inputs must still fail.

The review outcome and reproduced cases belong with any future adoption. Preserve dirty drafts separately from compilation authority, require complete saved generations, and retain editable recovery while blocking invalid native output. A generic Overwrite choice must not reconcile authoring integrity. No complete directory/power-loss transaction is provided.

## Strategic campaign limits

The raw/kit builder can emit native terrain resources, but a complete playable strategic campaign also needs logical geography, navigation, settlements and startpos consistency. Terrain preview readers for roads, farmland, trees and cities do not imply those authoring/build workflows exist.

The forward CHMF/full/LQ height preparation has bounded source-relative evidence. Native campaign-normal physical scale, registration, slope composition, overlapping chunk updates, ten-mip filtering and compressed feedback remain unresolved. A controlled before/after native bake or an equivalent independently established relationship is needed. The eight-mip battle continuity fixture does not establish that campaign contract.

## Preserve newer upstream behavior

The final ready-feature branches start from `81952bcf117bf676333c708beea0febe8b36bf1a`. Preserve its campaign world-bound coordinates and startpos lookup, Tile source/variation checks, props' grounding/height behavior, camera/rendering fixes, renderer-owned shared buffers and job-pool shutdown. Avoid copying historical whole files over these changes.

The final Save extraction retains the newer `TerryFrame::campaign(..., bounds)` implementation and its regression, adds publication methods alongside it, and leaves upstream Tile I/O and shutdown code unchanged. PNG integration retains current map labels, loader and command registration and adds only the region hooks.

## Small future support

The contributor may help with small reproducible fixes or questions about this handoff. Major development, additional format reconstruction and new feature ownership transfer to the maintainers. Use the existing synthetic tests first; game resources remain locally supplied and must not be committed.
