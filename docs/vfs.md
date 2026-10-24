# VFS: pack access through RPFM

`libs/vfs` gives the editors one view of the game's packs. It has no pack reader of its own; every read goes through RPFM.

## Layers

| Piece | Where | Role |
|---|---|---|
| `GameProfile` | `libs/game` | Which packs exist and in what order: `used_mods.txt` mods (first listed wins), the DLC `extra_vanilla` packs, then `data/manifest.txt`. Attila is the only profile; engine constants and shader names join it in Phase 3. |
| `IVfs` / `Vfs` | `libs/vfs/vfs.h` | `mount`, `packs`, `list`, `owner(s)`, `fetch`, `fetch_from`, `prefetch`, `read`. Pack priority, the pack index cache (`<cache>/index`, keyed by size and mtime) and the extract cache (`<cache>/x/<pack id>/<path>`) live here, so they are the same for every backend. Loose overlay folders (`Config::overlay_folders`) rank above all packs and are read in place. |
| `PackBackend` | `libs/vfs/pack_backend.h` | `list(pack)` and `extract(pack, paths, root)`. One implementation (tests and `vfs_probe --compare` pass their own to `Vfs`): |
| `rpfm_server` backend | `rpfm_server_backend.cpp`, `rpfm_client.cpp` | JSON-RPC 2.0 over WebSocket (`ws://127.0.0.1:45127/ws`) on WinHTTP (libcurl on Linux); calls `session.set_game`, `pack.open`, `files.list`, `files.extract`, `pack.close`, `session.disconnect`. |

The backend is created on first use, so a warm index cache mounts without touching the server.

## rpfm_server 5.2 (2026-10-13)

The rpfm_cli fallback and the MCP client were removed: RPFM no longer ships `rpfm_cli`, and RPFM 5.2 renamed the MCP tools and gave all MCP clients **one shared
session** (the attila-toolkit bridge would have seen and changed the editor's game and open packs). The editor now uses the WebSocket API, the one the RPFM UI uses
(protocol: `rpfm/docs/server/ws-protocol.md`, methods: `rpfm/docs/server/methods/`):

- **Version.** `GET /version` first: a server older than 5.2 is refused with a message that says so (5.1 answers on `/ws` too, with another protocol).
- **Sessions.** Each WebSocket connection gets a session of its own (game, open packs), isolated from the RPFM UI, other tools and the MCP clients; the server
  announces its id in a `session.connected` notification. A dropped connection reattaches with `/ws?session_id=<id>` (the server keeps a session 5 minutes) and the
  request is sent again; if the server lost the session (restart), the backend starts a new one, sets the game and re-opens the packs. Checked by killing the server
  between two calls: the backend restarted it and carried on.
- **Lifetime.** The client ends its session with `session.disconnect`. The server exits when its last WebSocket session is gone, so a server the editor started
  stops with the editor; one the RPFM UI started stays with the UI.
- **Jobs.** `session.set_game` runs as a job (`{"job": n}`, then `job.wait` until `finished`); it is called with `rebuild_dependencies: false` (the default
  loads all the game's dependencies). Other methods answer directly.
- **Packs.** `pack.open` takes one path and returns a UUID key (the backend keeps path -> key); `lazy_loading: true` keeps file data on disk. `files.list` pages
  (500 by default): the backend asks for everything at once and checks `total`. `files.extract` (`source: {"pack": key}`, `paths`, `destination`) writes
  `<destination>/<path>` and fails with "There were N errors while extracting." when some paths are not in the pack; the others are written.
- **Errors** are `{"code", "message", "data": {"kind"}}`; `RpfmError::kind` keeps the kind.
- `file.read` can return a file's bytes as base64 (the old MCP tool returned a JSON array of numbers): the extract cache is kept anyway, for the warm start and
  the gates.

The port is still fixed (45127, no option). The 2026-10-09 spike (RPFM 4.5, MCP) is in the git history of this file.

## Settings

No machine paths in code. What gets mounted (the game folder, the mod packs, the loose folders) comes from the editor's settings, see
"Settings and first run" in `docs/editor.md`.

## Gates

- `vfs_probe --compare [--reference <cache dir>]`: cold mount of all packs through rpfm_server in an empty cache, checked against a reference cache written earlier
  (default: the configured `cache_dir`, read as it is): same pack order, same file lists and winners, same bytes for the first 2 files per pack and file type (shader,
  model, texture, xml, environment, lua) that the reference has extracted, plus up to 20 paths that several packs provide. Exit 1 on a difference or when the reference
  has nothing to compare. `--per-type` sets the sample size, `--time` adds timings. It replaced the rpfm_server vs rpfm_cli comparison.
- `foundation_check`: JSON and settings.
- `scripts/vfs_timing.sh`: cold and warm start of the weather tool.
