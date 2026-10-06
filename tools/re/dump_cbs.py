"""Read the game's live shader constants from a full memory dump of Attila.exe.

    python dump_cbs.py <Attila.dmp> [regex]       e.g. regex "sky|sun|ambient|fog|luma|colour_matrix"

How to make the dump: during a battle (or on the campaign map), Task Manager > Details > Attila.exe >
right-click > Create dump file. It is written to %LOCALAPPDATA%\\Temp\\Attila.DMP.

Engine layout (empire.retail.dll 1.6.0, verified by disassembly 2026-10-06, see docs/re/renderer.md):
  shader manager = *( *(dll + 0x2a21d18) + 0x71558 )           (fn 0x178c460)
  named cbuffer object:   +0x28 -> char* cbuffer name         (e.g. "lighting_VS_PS" at manager+0x634)
  named variable object:  +0x08 -> char* variable name, +0x0c -> owning cbuffer object,
                          value pointer at +0x2c (vector vars) or +0x24 (scalar vars)  (fns 0x1923b50, 0x178a230)
The script scans the manager for variable objects (name pointer into the DLL + owner pointer that
names a cbuffer) and prints the 16 bytes behind both value-pointer candidates.
"""
import os
import re
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "..", "..", ".claude", "skills", "attila-modding", "scripts"))
from minidump import Dump  # noqa: E402

GLOBAL_RVA = 0x2A21D18
MANAGER_FIELD = 0x71558
SCAN_BYTES = 0x200000  # the shader manager is ~1.4 MB (skydome shader at +0xd31ec)


def main():
    path = sys.argv[1]
    pat = re.compile(sys.argv[2]) if len(sys.argv) > 2 else None
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
        if not cb:
            continue
        key = f"{cb}.{name}"
        if pat and not pat.search(key):
            continue
        vals = []
        for vo in (0x2C, 0x24):
            vp = struct.unpack_from("<I", blob, off + vo)[0]
            raw = D.read(vp, 16) if vp else b""
            vals.append(" ".join(f"{x:.6g}" for x in struct.unpack("<4f", raw)) if len(raw) == 16 else "-")
        rows.append((key, manager + off, vals))
    for key, addr, (v2c, v24) in sorted(rows):
        print(f"{key:60} obj {addr:#010x}  [+2c] {v2c:40}  [+24] {v24}")
    print(f"{len(rows)} variables")


if __name__ == "__main__":
    main()
