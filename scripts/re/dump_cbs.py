"""Read the game's live shader constants from a full memory dump of Attila.exe.

    python dump_cbs.py <Attila.dmp> [regex]       e.g. regex "sky|sun|ambient|fog|luma|colour_matrix"

How to make the dump: during a battle (or on the campaign map), Task Manager > Details > Attila.exe >
right-click > Create dump file. It is written to %LOCALAPPDATA%\\Temp\\Attila.DMP.

Engine layout (empire.retail.dll 1.6.0, verified by disassembly 2026-10-06, see docs/re/renderer.md):
  shader manager = *( *(dll + 0x2a21d18) + 0x71558 )           (fn 0x178c460)
  named cbuffer object:   +0x28 -> char* cbuffer name         (e.g. "lighting_VS_PS" at manager+0x634)
  named variable object:  +0x08 -> char* variable name, +0x0c -> owning cbuffer object,
                          +0x14 -> the variable's slot in the cbuffer's CPU copy (every type; verified on
                          the 2026-10-06 dump: sun_colour 0x0db596c0, ambient_cube_lr 0x0db596d0)
The script scans the manager for variable objects (name pointer into the DLL + owner pointer that
names a cbuffer) and prints the floats in the variable's slot (16 for matrices/arrays, else 4).
"""
import os
import re
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "..", "..", ".claude", "skills", "attila-modding", "scripts"))
sys.path.insert(0, HERE)  # minidump.py beside this script
from minidump import Dump  # noqa: E402

GLOBAL_RVA = 0x2A21D18
MANAGER_FIELD = 0x71558
SCAN_BYTES = 0x200000  # the shader manager is ~1.4 MB (skydome shader at +0xd31ec)


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--array=")]
    # --array=N: print the first N float4 rows of every matching variable (non-zero rows only, with their index),
    # for the [512] per-emitter arrays of the cb_vfx_emitter_constants_* buffers.
    array_rows = next((int(a.split("=")[1]) for a in sys.argv[1:] if a.startswith("--array=")), 0)
    path = args[0]
    pat = re.compile(args[1]) if len(args) > 1 else None
    D = Dump(path)
    mod = D.module("empire.retail.dll")
    if not mod:
        sys.exit("empire.retail.dll not in the dump's module list")
    base = mod[0] if isinstance(mod, tuple) else mod.base
    print(f"empire.retail.dll at {base:#x}")
    g = D.u32(base + GLOBAL_RVA)
    manager = D.u32(g + MANAGER_FIELD) if g else 0
    print(f"global {g:#x} -> shader manager {manager:#x}")
    if not manager:
        sys.exit("shader manager pointer is null (dump taken before rendering started?)")
    # Read page by page: Dump.read returns None across memory-region boundaries.
    blob = bytearray()
    for p in range(0, SCAN_BYTES, 0x1000):
        blob += D.read(manager + p, 0x1000) or bytes(0x1000)
    blob = bytes(blob)
    lo, hi = base, base + 0x2B9C000
    rows = []
    for off in range(0, len(blob) - 0x30, 4):
        name_p, owner = struct.unpack_from("<II", blob, off + 8)
        if not (lo <= name_p < hi) or not owner:
            continue
        name = D.cstr(name_p, 64)
        if not name or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]{2,63}", name):
            continue
        cb_name_p = D.u32(owner + 0x28)
        cb = D.cstr(cb_name_p, 64) if cb_name_p and lo <= cb_name_p < hi else None
        if not cb or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]{2,63}", cb):
            continue
        key = f"{cb}.{name}"
        if pat and not pat.search(key):
            continue
        vp = struct.unpack_from("<I", blob, off + 0x14)[0]
        if array_rows:
            for r in range(array_rows if vp else 0):
                raw = D.read(vp + 16 * r, 16)  # per row: Dump.read fails across memory-region boundaries
                if not raw:
                    continue
                v = struct.unpack("<4f", raw)
                u = struct.unpack("<4I", raw)
                if any(u):
                    txt = " ".join(f"{x:.6g}" if (w == 0 or 1e-30 < abs(x) < 1e30) else f"#{w}" for x, w in zip(v, u))
                    rows.append((f"{key}[{r}]", manager + off, txt))
            continue
        n = 16 if re.search(r"matrix|cube_|split|bounds|projection|world_view|view|sample_data", name) else 4
        raw = (D.read(vp, 4 * n) if vp else None) or b""
        val = " ".join(f"{x:.6g}" for x in struct.unpack(f"<{n}f", raw)) if len(raw) == 4 * n else "-"
        rows.append((key, manager + off, val))
    for key, addr, val in sorted(set(rows)):
        print(f"{key:58} {val}")
    print(f"{len(rows)} variables")


if __name__ == "__main__":
    main()
