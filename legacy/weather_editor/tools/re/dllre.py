"""Reverse-engineering helpers for empire.retail.dll on top of the attila-modding skill's pedis.PE.

    python dllre.py vtable <rva> [n]                 list virtual functions
    python dllre.py touch <fn_rva> <lo> <hi>         instructions in a function using offsets lo..hi
    python dllre.py vtouch <vtable_rva> <lo> <hi>    which virtual functions use offsets lo..hi
    python dllre.py dis <fn_rva> [max_insns]         disassemble a function until ret/int3 padding
"""
import os, re, struct, sys
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "..", "..", ".claude", "skills", "attila-modding", "scripts"))
from pedis import PE

pe = PE()
TEXT_LO, TEXT_HI = pe.text_rva, pe.text_rva + len(pe.text)

def vtable(rva, n=200):
    out = []
    for i in range(n):
        p = struct.unpack("<I", pe.read(rva + 4 * i, 4))[0] - pe.image_base
        if not (TEXT_LO <= p < TEXT_HI):
            break
        out.append(p)
    return out

def fn_insns(rva, maxn=4000):
    out = []
    for ins in pe.md.disasm(pe.read(rva, 0x6000), pe.image_base + rva):
        out.append(ins)
        if ins.mnemonic == "int3" or len(out) >= maxn:
            break
    return out

def touch(fn, lo, hi):
    hits = []
    for ins in fn_insns(fn):
        for m in re.finditer(r"\+ 0x([0-9a-f]+)\]", ins.op_str):
            off = int(m.group(1), 16)
            if lo <= off < hi:
                hits.append(ins)
                break
    return hits

def fmt(ins):
    return f"  {ins.address - pe.image_base:06x}  {ins.mnemonic:8} {ins.op_str}{pe._note(ins.op_str)}"

if __name__ == "__main__":
    a = sys.argv[1:]
    num = lambda s: int(s, 16)
    if a[0] == "vtable":
        for i, f in enumerate(vtable(num(a[1]), int(a[2]) if len(a) > 2 else 200)):
            print(f"[{i:3}] +{i*4:3x}  {f:06x}")
    elif a[0] == "touch":
        for ins in touch(num(a[1]), num(a[2]), num(a[3])): print(fmt(ins))
    elif a[0] == "vtouch":
        for i, f in enumerate(vtable(num(a[1]))):
            h = touch(f, num(a[2]), num(a[3]))
            if h: print(f"[{i}] {f:06x}: {len(h)} hits")
    elif a[0] == "dis":
        for ins in fn_insns(num(a[1]), int(a[2]) if len(a) > 2 else 4000):
            if ins.mnemonic != "int3": print(fmt(ins))
