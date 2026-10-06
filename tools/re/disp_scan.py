"""Find instructions anywhere in .text whose memory displacement falls in [lo, hi). Slow (full linear sweep)."""
import re, sys, struct
from dllre import pe
lo, hi = int(sys.argv[1], 16), int(sys.argv[2], 16)
# Fast prefilter: look for the displacement bytes as imm32 in .text.
cands = set()
for d in range(lo, hi):
    for m in re.finditer(re.escape(struct.pack("<I", d)), pe.text):
        cands.add(pe.text_rva + m.start())
fns = {}
for c in sorted(cands):
    for back in range(1, 8):
        for ins in pe.md.disasm(pe.read(c - back, 16), pe.image_base + c - back):
            m = re.search(r"\+ 0x([0-9a-f]+)\]", ins.op_str)
            if m and lo <= int(m.group(1), 16) < hi and ins.address - pe.image_base <= c < ins.address - pe.image_base + ins.size:
                f = pe.func_start(c)
                fns.setdefault(f, []).append(f"{ins.address - pe.image_base:x} {ins.mnemonic} {ins.op_str}")
            break
for f, l in sorted(fns.items()):
    print(f"fn {f:x}: {len(l)}")
    for x in sorted(set(l))[:12]: print("   ", x)
