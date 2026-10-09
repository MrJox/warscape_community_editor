#!/usr/bin/env python3
"""Are TEd/BOB's R32F DDS files exactly 'fixed header(width, height) + float32 payload'?

    python dds_template.py <manifest.tsv> <kits_parent_dir>
"""
import collections
import os
import struct
import sys


def header_r32f(w, h):
    hdr = struct.pack("<I7I", 124, 0, h, w, 0, 0, 0, 0) + b"\0" * 40          # size, flags=0, h, w, pitch=0, depth=0, mips=0, reserved[11] (first dword above)
    hdr = struct.pack("<7I", 124, 0, h, w, 0, 0, 0) + b"\0" * 44
    pf = struct.pack("<8I", 32, 4, 114, 32, 0, 0, 0, 0)
    caps = struct.pack("<5I", 0, 0, 0, 0, 0)
    return b"DDS " + hdr + pf + caps


manifest, root = sys.argv[1], sys.argv[2]
res = collections.Counter()
bad = []
for line in list(open(manifest, encoding="utf-8"))[1:]:
    c = line.rstrip("\n").split("\t")
    if c[11] != "in" or not c[2].lower().endswith(".dds"):
        continue
    with open(os.path.join(root, c[0], c[1], c[2]), "rb") as f:
        head = f.read(128)
    if len(head) < 128 or head[:4] != b"DDS ":
        continue
    w, h = struct.unpack_from("<II", head, 16)[::-1]
    h, w = struct.unpack_from("<II", head, 12)
    pf = struct.unpack_from("<8I", head, 76)
    if pf[1] == 4 and pf[2] == 114 and struct.unpack_from("<I", head, 8)[0] == 0:
        ok = head == header_r32f(w, h) and int(c[3]) == 128 + w * h * 4
        res[ok] += 1
        if not ok and len(bad) < 3:
            bad.append((c[2], head.hex(" "), header_r32f(w, h).hex(" ")))
print("R32F flags-0 files: header+size match template:", dict(res))
for b in bad:
    print(b[0]); print(" file", b[1]); print(" tmpl", b[2])
