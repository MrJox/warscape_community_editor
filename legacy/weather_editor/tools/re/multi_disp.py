"""Functions in .text that use ALL the given displacements (hex), e.g. fields of one object."""
import re, sys, struct
from dllre import pe
targets = [int(a, 16) for a in sys.argv[1:]]
sets = []
for t in targets:
    fns = set()
    for pat in (struct.pack("<I", t),):
        for m in re.finditer(re.escape(pat), pe.text):
            fns.add(pe.func_start(pe.text_rva + m.start()))
    sets.append(fns)
common = set.intersection(*sets)
print(sorted(hex(f) for f in common if f))
