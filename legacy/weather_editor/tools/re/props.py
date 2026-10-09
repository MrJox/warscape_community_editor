"""Find data references (pointers in .rdata/.data) to strings, and dump the surrounding dwords."""
import re, struct, sys
from dllre import pe

def data_refs(rva):
    pat = re.escape(struct.pack("<I", rva + pe.image_base))
    out = []
    for m in re.finditer(pat, pe.data):
        r = pe.off2rva(m.start())
        if r is not None and not (pe.text_rva <= r < pe.text_rva + len(pe.text)):
            out.append(r)
    return out

def show(rva, before=4, after=8):
    for k in range(-before, after):
        a = rva + 4 * k
        v = struct.unpack("<I", pe.read(a, 4))[0]
        r = v - pe.image_base
        note = ""
        if 0 < r < pe.size_of_image:
            s = pe.cstr(r)
            note = f'"{s}"' if s else f"rva {r:x}"
        else:
            f = struct.unpack("<f", struct.pack("<I", v))[0]
            note = f"{v} / {f:g}"
        print(f"   {'>' if k == 0 else ' '} {a:07x}: {v:08x}  {note}")

for name in sys.argv[1:]:
    for s in pe.find_string(name):
        for d in data_refs(s):
            print(f"{name} @ {s:x}: data ref {d:x}")
            show(d)
