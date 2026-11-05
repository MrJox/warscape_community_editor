# Project guidance

Warscape Community Editor is an all-in-one Total War editor for weather, battle and campaign tiles and terrain tilemaps, units, cinematics, and related content. Use Rust (a Cargo workspace, one crate per layer under `crates/`) with VS Code; the editor must run natively on Windows, macOS, and Linux. It is currently developed for Total War ATTILA but the architecture must be designed in a way that would allow to easily support other Total War games (starting from Rome 2 and ending with Pharaoh Dynasties).

Use RPFM to read game and mod data from packs. Use only the game's shaders. Keep visuals accurate and faithful to the game's rendering; deviations require explicit human permission.

For Total War development and modding in Claude Code, it is recommended to install the [Attila Modding Marketplace](https://github.com/MrJox/attila-modding-marketplace).

Build with `cargo build -p wce_app` (on Linux, run the CMake configure step from the README once first: it builds DXVK-native and vkd3d-utils). Run tests on demand with `cargo test --workspace`; do not run tests after every change. Leave routine test runs to a human, or run them occasionally after major changes. The gates are Cargo examples (`scripts/gates.sh` runs them). Launch with `cargo run -p wce_app`, or use VS Code's WCE launch configuration.

Use "grill me" technique when a feature implementation request is submitted.

## Working rules

- **Byte-identical round trips are absolute**, unless the user explicitly says otherwise for a case (for example float precision in
  TEd files): a round trip and a minimal edit (only the edited spans change) never regress, BOB equivalence never regresses, unknown
  bytes are preserved, and no parser drops data to pass a gate.
- **Focus:** the user plays games on this machine. Unattended runs (`--script`, `--shot`, `--capture`) are non-activating: keep them
  so, never pass `--foreground` unless the user asks in that session, and end every scripted run with `quit`. Never bring TEd or any
  window in front of a running game on your own; ask. Capture only the editor's own window, never the full screen.
- **TEd:** `assembly_kit/binaries/TEd.AssemblyKit.exe`, run from `assembly_kit/binaries`. Work on scratch copies of kit tiles (the
  tile folder and, if TEd needs it, its `TILES/*.xml` entry in a scratch kit), never on the user's files. Ask the user to run TEd when
  you need clicks or pixels you cannot get safely. Close a TEd you started when done (about 800 MB); never close one the user opened.
- **Do not kill the user's running `wce.exe`** (a link error LNK1168 means one is running): ask them to close it.
- **Where data is read from:** kit-layout data (tiles, the tile database, battle maps, prefabs, vegetation parameters) comes from the first
  source that has it: the projects' kit layout, then the Assembly Kits in the settings' order, then the game's packs as the fallback
  (`wce_tile::sources`). Models and textures keep the game's order (loose folders, then packs). New loaders follow this order.
- **Third-party code:** nothing new (a crate included) without asking; permissive licences only; keep `THIRD_PARTY.md` current.
- **Game content:** never commit game assets or shader binaries. Reverse-engineering notes record formats, behaviour and constants
  (in `docs/re/` and `docs/formats/`), never copied code. Reverse engineering is targeted: black box first (the tools on scratch
  copies, diffs, their outputs as oracles), short spikes only where that does not settle a question.
- **No machine paths in code:** defaults come from the settings and the first-run dialog; test scripts may default to this machine but
  accept environment-variable overrides.
- **Shell:** Bash heredocs mangle backslash escapes, so write code and scripts containing `\n` or `\\` with the file tools.
