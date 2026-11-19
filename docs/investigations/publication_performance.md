# Save and Export publication performance

This correction addresses the remaining performance feedback in [PR #74](https://github.com/MrJox/warscape_community_editor/pull/74#issuecomment-6992126283). It changes publication work, leaving format writers, normal calculations and document/Undo semantics intact.

## Save

`Changes::reason` saves a successful initial capture for one following `begin`. A conflict never grants this reuse; explicit Overwrite captures fresh observations. Changing the observed destination or recording an own publication invalidates the saved capture. Reuse also requires the same destination and unchanged in-memory source observations. A direct Save without a precheck captures normally.

Source/destination roots that normalize to the same location share one capture at each boundary. The initial capture also records exact resource spellings, so destination-name validation does not enumerate the directory again. Preflight compares each output exactly once and removes retained outputs; `Staged::prepare_changes` stages the already classified differences without repeating that comparison. It still checks readability, permissions and synchronization.

After all staging completes, `Attempt::recheck` independently captures the relevant roots again and checks stamps and destination spellings. Each changed target is checked once more immediately before its rename. A cached precheck cannot approve a later outside edit, including a case-only destination rename. Size/mtime observations retain their existing limits; this is not a content-hash watcher or filesystem lock.

## Export

`Staged::prepare_with_generated` stages the loose outputs and asks the existing RPFM writer to generate the pack directly at the reserved publication path. The generated file is compared with the destination, receives retained destination permissions when replacing, and is synchronized before publication. Publication renames that same file; it does not copy the completed pack into another staging file.

Generator failure, a synchronization failure or a read-only changed destination discards staging without publishing any output. Duplicate destinations are rejected before running the generator. The owning Export still performs its final source/map/pack freshness check and publishes the pack last. This remains per-file publication, without whole-map rollback after an interrupted series of renames.

## Reproducing the measurement

```text
cargo test --release --locked -p wce_terrain_tool --lib large_synthetic_save_export_measurement -- --ignored --nocapture --test-threads=1
```

The opt-in fixture has a 4096 × 4096 land LF, 128 MiB of carried bulk data and 1,024 small carried resources (about 161 MiB total). It measures changed/unchanged owned Saves, repeat Save As copies, and changed/unchanged Export publication. Three sequential runs per case report elapsed time and logical process read/write bytes. Windows uses process I/O counters; Linux uses `/proc/self/io`. These are logical transfers, including cached I/O, rather than physical disk traffic.

The benchmark uses the real document manager, raw map writer, publication path and RPFM pack writer. It checks retained/copy bytes and every decoded pack resource exactly, then removes its verified temporary tree. It creates no native window or game assets. The Export workload starts from prepared synthetic resources, so its timings measure publication rather than tilemap/normal compilation. It is not a complete game-ready map or a game/runtime performance certification. No wall-time threshold is part of routine CI.

Windows release measurements on 19 November 2026 compare the same workload on the preceding PR implementation and this correction. Medians of three runs:

| Publication case | Before | After | Elapsed reduction |
|---|---:|---:|---:|
| Changed owned Save | 0.790 s | 0.415 s | 47.4% |
| Unchanged owned Save | 0.696 s | 0.332 s | 52.2% |
| Unchanged Save As copy | 0.905 s | 0.704 s | 22.3% |
| Changed Export | 1.687 s | 1.365 s | 19.1% |
| Unchanged Export | 1.250 s | 0.863 s | 30.9% |

Changed Save's logical reads fell from 96 to 64 MiB; its required 32 MiB write remains. Changed Export's median logical writes fell from 450.10 to 289.05 MiB. Unchanged Export's writes fell from 322.10 to 161.05 MiB, eliminating one complete extra pack write. Independent-copy comparisons still read 322 MiB in the unchanged-copy case; the exact comparison remains necessary. These measurements include caching and machine load, and are evidence for this synthetic workload rather than universal timing guarantees. Generated pack metadata can differ between runs, so exact-container comparison costs vary; all decoded payloads remain byte-identical.

Save remains synchronous. Serialization, required staging writes and exact comparisons against independent files still cost time proportional to the data. Exact retained bytes, outside-change protection and recovery behavior take precedence over skipping necessary work.
