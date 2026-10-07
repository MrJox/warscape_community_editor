"""Dump a property-schema builder function of empire.retail.dll: name, type vtable and default stores per property.

    python schema.py <fn_rva> [max_insns]      e.g. 0x17ee510 (VFX "Emitter Properties")
"""
import re, struct, sys
from dllre import pe, fn_insns

fn = int(sys.argv[1], 16)
insns = fn_insns(fn, int(sys.argv[2]) if len(sys.argv) > 2 else 40000)
rows, cur = [], None
for ins in insns:
    s = ins.op_str
    m = re.match(r"0x([0-9a-f]+)$", s)
    if ins.mnemonic == "push" and m:
        a = int(m.group(1), 16) - pe.image_base
        try:
            txt = pe.read(a, 64).split(b"\0")[0].decode("ascii")
        except Exception:
            txt = ""
        if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_ ]{1,60}", txt) and "Properties" not in txt:
            cur = {"name": txt, "vt": None, "def": []}
            rows.append(cur)
        continue
    if cur is None:
        continue
    m = re.match(r"dword ptr \[esi\], 0x([0-9a-f]+)", s)
    if ins.mnemonic == "mov" and m:
        cur["vt"] = int(m.group(1), 16) - pe.image_base
        continue
    m = re.match(r"(byte|dword) ptr \[esi \+ 0x([0-9a-f]+)\], (0x[0-9a-f]+|\d+)$", s)
    if ins.mnemonic == "mov" and m:
        off, v = int(m.group(2), 16), int(m.group(3), 0)
        val = v if m.group(1) == "byte" else struct.unpack("<f", struct.pack("<I", v))[0]
        if m.group(1) == "dword" and abs(val) < 1e-30 and v: val = v
        cur["def"].append((off, round(val, 6) if isinstance(val, float) else val))
    if ins.mnemonic == "ret":
        break
for i, r in enumerate(rows):
    print(f"{i:3} {r['name']:32} vt {r['vt'] or 0:#x}  " + " ".join(f"+{o:x}={v}" for o, v in r["def"]))
