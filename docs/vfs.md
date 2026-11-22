# VFS: the game's files

`wce_vfs` gives the editor and its tools one view of a game's files. It has no pack reader of its own: packs are read in-process with
rpfm_lib, lazily (a pack's file data stays on disk until a file is read). There is no server, no index cache and no extract cache.

## Pieces

| Piece | Where | Role |
|---|---|---|
| `GameProfile` | `wce_foundation::game` (re-exported by `wce_vfs`) | the game's packs and their order (`pack_order`, `pack_order_with`): the mods first (first listed wins), then the vanilla packs: the patch packs, then the rest, each group in the order of `data/manifest.txt` (after the DLC packs it does not list) or of the profile's list. The engine's exact order between mods and between vanilla packs of one kind is not verified. |
| `Config`, `Vfs` | `wce_vfs` (`lib.rs`) | `Vfs::mount(&Config)`: the game folder, its profile, the mod packs (by file name in the data folder; `None`: the launcher's `used_mods.txt` when `use_mods`), and loose overlay folders mounted above every pack, highest priority first. `packs`, `files`, `list`, `owner`, `owners`, `read`, `read_from`, `missing_mods`, `refresh_loose` (a loose file written: that path indexed again, no remount). Paths compare lower-case with forward slashes (`normalize`); each pack keeps its own spelling for reading. An empty file in a pack removes the path from the packs below it (the patch packs remove files that way), unless a pack above has it again. |
| `Files` | `wce_vfs::files` | what the tools read through, from any thread: `read`, `exists`, `list`, `game`, `mount_id` (another mount has another id, for caches of what was found). `Vfs` implements it; `FolderFiles` reads an extracted copy of the packs (tools and comparisons). |
| generated files | `wce_vfs::generated` | files made in memory and read before the packs and folders by every `Files` here (the procedural walls' preview pieces), process-wide. |
| `mod_pack` | `wce_vfs::mod_pack` | writing files into a mod pack of a game (`write_file`, `write_files`, `write_new`). |
| `steam_detect` | `wce_vfs::steam_detect` | first-run helpers: the game found through Steam's libraries; the file and folder dialogs (zenity or kdialog on Linux, PowerShell on Windows). |

## What the editor mounts

`Editor::vfs_config`: the game folder of the settings, the mod packs chosen in the editor (the game's `mod_packs`; the launcher's list is
not used), and as overlay folders the project's `loose/` first (what Save writes, [editor.md](editor.md), "Saving and exporting"), then the
game's `overlay_folders`. No machine paths in code: the settings and their environment overrides are in [editor.md](editor.md),
"Settings and first run".

## Examples

- `vfs_probe` (`WCE_GAME=<game folder> [WCE_GAME_KEY=<key>] cargo run -p wce_vfs --example vfs_probe [--find <substring>]...
  [--extract <prefix> <folder>]... [path...]`): mounts the packs and prints what the editor would see.
- `vfs_ls <game dir> <prefix> [--vanilla] [--extract <path> <out file>]`: the pack files under a prefix with their sizes, or one written out.
- `vfs_exists <list file>`: which paths of a list the packs lack.

## History

2026-10-09: a C++ VFS over RPFM 4.5 (MCP, with an rpfm_cli fallback); 2026-10-13: over rpfm_server 5.2's WebSocket API with its own index
and extract caches, checked by `vfs_compare` (`docs/baseline.md`). The Rust port reads the packs with rpfm_lib in-process, which retired the
server, the caches and that gate. The detail is in the git history of this file.
