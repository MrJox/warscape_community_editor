# VFS: pack access through RPFM

`libs/vfs` gives the editors one view of the game's packs. It has no pack reader of its own; every read goes through RPFM.

## Layers

| Piece | Where | Role |
|---|---|---|
| `GameProfile` | `libs/game` | Which packs exist and in what order: `used_mods.txt` mods (first listed wins), the DLC `extra_vanilla` packs, then `data/manifest.txt`. Attila is the only profile; engine constants and shader names join it in Phase 3. |
| `IVfs` / `Vfs` | `libs/vfs/vfs.h` | `mount`, `packs`, `list`, `owner(s)`, `fetch`, `fetch_from`, `prefetch`, `read`. Pack priority, the pack index cache (`<cache>/index`, keyed by size and mtime) and the extract cache (`<cache>/x/<pack id>/<path>`) live here, so they are the same for every backend. Loose overlay folders (`Config::overlay_folders`) rank above all packs and are read in place. |
| `PackBackend` | `libs/vfs/pack_backend.h` | `list(pack)` and `extract(pack, paths, root)`. Two implementations: |
| `rpfm_server` backend | `rpfm_server_backend.cpp`, `mcp_client.cpp` | Primary. MCP over HTTP (`127.0.0.1:45127/mcp`) on WinHTTP; calls `set_game_selected`, `open_packfiles`, `open_pack_info`, `extract_packed_files`, `close_pack`. |
| `rpfm_cli` backend | `rpfm_cli_backend.cpp` | Fallback: one `rpfm_cli pack list` per pack, `pack extract` in batches under the command-line limit. |

`BackendMode::Auto` (the default) tries the server and falls back to the cli when it cannot be used; `Vfs::backend_note()` says why. The backend is created
on first use, so a warm index cache mounts without touching either.

## What the spike found (2026-10-09, RPFM 4.5, rmcp 3.1.0)

- The server keeps the selected game and the open packs **per MCP session**. A session of this editor and the attila-toolkit bridge used by Claude sessions
  do not see each other's state (checked: setting the game and opening packs in one session left the other's `get_game_selected` and `list_open_packs`
  untouched). So sharing the one server on its fixed port is safe, and the editor never closes or changes anything it did not open itself.
- The port cannot be changed (`rpfm_server` has no option; a second instance fails to bind). The editor uses the running server, or starts `rpfm_server.exe`
  (next to `rpfm_cli.exe`) when none listens and autostart is on. It never stops a server it started, because a Claude session may be using it by then.
- `open_packfiles` takes pack paths and returns the key (the path as given); `open_pack_info` lists a pack (50,773 files of `data.pack` in 0.11 s, 8 MB of JSON, no sizes);
  `get_packed_file_raw_data` returns bytes as a JSON array of numbers (3.5 characters per byte), so the backend extracts to disk instead; `extract_packed_files`
  with `{"PackFile":[{"File":...}]}` writes `<destination>/<path>`, 2,000 files per call. In-pack paths are case-sensitive for the server: `Vfs` keeps the
  pack's own spelling per pack.
- Errors: a missing path gives `{"Error": ...}` with `isError`; a session the server forgot gives HTTP 404, and the client reconnects and re-opens (restart recovery is implemented but untested).

## Settings

No machine paths in code. `Vfs::default_config()` reads `%LOCALAPPDATA%\awe\wce.ini` (or `$WCE_SETTINGS_DIR`), `key=value` lines; an environment variable wins over the file:

| Key | Environment | Meaning |
|---|---|---|
| `game_dir` | `WCE_GAME`, `ATTILA_DIR` | game folder (required) |
| `rpfm_dir` | `WCE_RPFM_DIR` | folder with `rpfm_cli.exe` and `rpfm_server.exe` |
| `rpfm_cli`, `rpfm_server_exe` | `RPFM_CLI`, `WCE_RPFM_SERVER` | explicit exe paths |
| `rpfm_backend` | `WCE_RPFM_BACKEND` | `auto` (default), `server`, `cli` |
| `rpfm_server_port`, `rpfm_server_autostart` | `WCE_RPFM_PORT`, `WCE_RPFM_AUTOSTART` | 45127, on |
| `cache_dir` | `WCE_CACHE_DIR`, `AWE_CACHE` | index and extract caches; keep it short (rpfm writes nothing past MAX_PATH) |

The Phase 4 first-run dialog writes this file.

## Gates

- `vfs_probe --compare`: cold mount of all packs through each backend in empty caches; same pack order, same file lists and winners, same bytes for 2 files per pack and
  file type (shader, model, texture, xml, environment, lua) from every pack including the mod packs, plus 20 paths that several packs provide. `--time` adds timings.
- `foundation_check`: JSON and settings.
- `scripts/vfs_timing.sh`: cold and warm start of the weather tool per backend.
