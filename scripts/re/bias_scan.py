"""Functions in a range that read float fields at the given small displacements (disp8 aware)."""
import re, struct, sys
from dllre import pe, fn_insns
lo, hi = int(sys.argv[1], 16), int(sys.argv[2], 16)
need = [int(x, 16) for x in sys.argv[3].split(',')]
# function starts = targets of E8 calls into the range
t = pe.text
starts = set()
for m in re.finditer(b"\xe8", t):
    i = m.start()
    if i + 5 > len(t): continue
    tgt = pe.text_rva + i + 5 + struct.unpack_from("<i", t, i + 1)[0]
    if lo <= tgt < hi: starts.add(tgt)
print(len(starts), "functions")
for f in sorted(starts):
    seen = set()
    for ins in fn_insns(f, 3000):
        for m2 in re.finditer(r"\[e[a-z]{2} \+ 0x([0-9a-f]+)\]", ins.op_str):
            seen.add(int(m2.group(1), 16))
    if all(n in seen for n in need):
        print(hex(f))
