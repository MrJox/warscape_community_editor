# Handoff: campaign view leftovers and RPFM startpos generation

State on 2026-11-05, written for the agent that takes this work over. Two independent pieces: RPFM's startpos generator
(`../rpfm`) and the campaign side of the Terrain tool (this repo). Neither blocks the other.

## Rules that apply to both

- Feature branches only; never merge or push (the user merges by hand).
- Builds: `cargo --release`, one package at a time (`-p ...`); never `--workspace --all-targets` (C: is short of space, about 26 GB
  free). Scratch data goes on I: (`I:\wce_scratch\...`).
- Never write into the game or kit folders (read them; copy to I: what has to change).
- RPFM has its own rules (`../rpfm/AGENTS.md`): tests in separate `*_test.rs` files, no `cargo fmt`, one logical change per commit
  with a `Startpos:` prefix, doc comments, no `unwrap`, leave the local changes to `3rdparty/src/ritual` and `schemas` alone.
- Ghidra: `I:\Ghidra`, analysed projects `attila`, `thrones`, `rome2` (each game's `empire.retail.dll`) in `C:\Users\frodo\ghidra_work`,
  headless scripts in its `scripts` folder (`DecompAt.java <out> <rva>...`, `RefsTo`, `Disasm`, `DumpData`...). The engine references
  strings by absolute 32-bit address: a byte scan of `.text` for a string's VA finds its users.

## 1. RPFM startpos generation (`../rpfm`, branch `startpos-thrones-rome2`)

The generator builds a campaign's `startpos.esf` from the Assembly Kit's tables. Work log, evidence and DLL addresses:
`I:\wce_scratch\rpfm_startpos\NOTES.md` (read it first). Test data and how to run the comparison harnesses: same folder (`thrones/`,
`attila/`, `rome2/`, `tdd/`, env vars `RPFM_STARTPOS_TEST_DATA_*`). The harness tests themselves are not committed: they are in
`git stash` on that branch ("startpos scratch harness tests"); `git stash apply` them to measure, don't commit them.

**User policy:** a game goes into `SUPPORTED_GAMES` for startpos generation only when every known gap is fixed. Don't add Thrones (or
Rome 2) before that, and don't offer to.

### Where it stands

| Campaign | Fields matching the game's startpos |
|---|---|
| Thrones `vik_attila` | 0.8492 (852 007), from 0.6777 at the start |
| Attila `main_attila` | 0.9029 |
| Attila `pro_attila` | 0.9105 |
| TDD victory objectives | 4/4 tests: all 124 objectives equal TDD's game-built startpos |

Done: TDD's objective types; Thrones' record layouts, objectives, estates, posts and heirs, loyalty, skills / rank / xp, art sets,
followers, GDP, unit stat bonuses, the game's DB key hash and hash-set orders (posts, mercenaries, provinces, managers, sea managers),
trade routes (all 1 184 ids). CAI_HISTORY is left out on purpose (written only by CA's internal builds).

### Thrones: what is left (all must close before Thrones is listed)

1. **Garrison unit order**: the code that builds garrisons at a new game isn't found. The unit constructor `FUN_106d98e0` has 10
   callers; the world builder `FUN_10781ec0` calls none directly. Next: trace from the code that writes GARRISON_RESIDENCE (the
   function containing 0x80f389) and the settlement constructor. If it orders by object pointer hash (`p >> 2` XOR 0x4a545eed), it
   can't be reproduced: prove it and document it.
2. **Trade route path tie-break**: 111 Thrones paths (and 2 Attila ones) of equal step count take another path in the game. The
   pathfinder call is `FUN_107f7420(&out, &capital_a, &capital_b, -1.0f)` with a static filter (a generic graph search): decompile it
   and reproduce its expansion order.
3. **Route resource amounts**: 406 game routes carry an amount (index 26 of the resource array); we write zeros.
4. **Welsh garrisons**: the game picks `wel_swordsmen` from the `wel_armoured_swordsmen` group, we pick the group's own unit (9 units).
5. **Smaller fields** (NOTES.md, rounds 7 to 9): CAI_REGION ^+14, GBD+16..18 (`FUN_107ce210`), the AI's character mirror
   (CAI_CHARACTER), a CHARACTER_ARRAY missing group, legacy skill sets / dates / names / emblems, CAI resource mobiles, REGION
   LINE_OF_SIGHT+6, CAI faction provinces, CAI_FACTION ^+10, BUILDING_BASE ^+3/^+4, the province and trait-category strings after the
   skills list, character effect bundles, missing traits, the 7 characters whose skill list the game leaves empty.
6. **Names and the main RNG**: the game seeds its campaign RNG with 0xCACA (51914) when it builds a startpos, so names, colonels'
   birth dates and CEB+1 are deterministic. Reproducing them means modelling the name picking (with its retries) and every main-RNG
   draw in order (each faction's name pools are LCG copies seeded from the main RNG). The largest item; do it last.
7. **Gravitas base** (header ^+6, Alfred 4 where his traits give 8 and estates 3): not settled from the DLL. Waits for the user's
   in-game check (below).
8. **AI analyses** (CAI beliefs / desires): retail computes them at startpos build; whether the game rebuilds missing ones is the
   user's load test (below). If it does, they can stay empty; if not, they are a real gap.

Known and documented, not gaps: CEB+2 (a draw at load), the art set hash (recomputed at load), region line of sight (recomputed).

### User tests waiting (instructions in `I:\wce_scratch\rpfm_startpos\loadtest\README.md`)

- Attila: `zz_test_empty_beliefs.pack` (then `zz_test_rpfm_startpos.pack`): start a Grand Campaign, 3 or 4 turns, does the AI act,
  send the save (the beliefs pool should be back to about 1 500 records).
- Thrones: West Seaxe, Alfred's influence tooltip on turn 1 (screenshot) and a turn-1 save.

### Rome 2 (on hold until Thrones' gaps are closed: user's decision)

Groundwork is in (objective ids by game, optional tables and columns, the comparison harness). The generator stops at
CHARACTER_DETAILS: Rome 2 reorganises characters, diplomacy (no OLD_DIPLOMACY_MANAGER), region slots and buildings (no
REGION_BUILDING_MANAGER), religion, fortifications and the CAI (706 differing paths): a record-by-record port. Five objective types
used only by main_3c / main_invasion have their ids (NOTES.md: 32, 42, 49, 55, 56) but no known record layout (no game startpos uses
them): read them from the DLL's save functions. Kit: `D:\Juegos\Steam\steamapps\common\Total War Rome II\assembly_kit`.

## 2. Terrain tool, campaign view (this repo, branch `terrain-editor-plan`)

The plan of record is docs/plans/terrain_editor.md (Phase 8's "Left" and Phase 9). Open items:

- **Settlement buildings' facing**: an integer on the runtime display slot (+8) times 60°, not stored in the startpos and not found
  in the DLL yet (docs/re/terry.md, "Settlement models"). Buildings are drawn unturned.
- **Assumptions to check against the game** (each marked ASSUMPTION in the code and docs/re/terry.md): the farmland rule (chain level
  >= area level), sprawl levels (tier.div_ceil(2) clamped 1..3), walls around zones without a building, the road level 3 capped to
  `road2`, the trees' size and lean.
- **Thrones' campaign look**: `vik_attila_map` opens with its settlements; not checked against the game (the user will send turn-1
  screenshots).
- **Crash**: deferred by the user; ask before working on it.
- **Performance, optional**: the Alps view is 13.7 ms (CPU and GPU about even, 16 M triangles). Next cuts: a level of detail within a
  campaign tile by distance (tiles are one 256-cell patch now), and the blend boxes' per-material binds (`terrain.from_blend`,
  1.8 ms). Measure with `I:\wce_scratch\wce_run\run_fps1.ps1` (near ring 24) with no other build running.
- **A campaign launch**: noted, not for now.
