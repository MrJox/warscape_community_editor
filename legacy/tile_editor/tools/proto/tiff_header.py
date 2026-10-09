#!/usr/bin/env python3
"""blend_map.tif header analysis: which of the first 242 bytes vary between files, and is the file exactly
header(width, height) + raw samples?

    python tiff_header.py <manifest.tsv> <kits_parent_dir>
"""
import collections
import os
import struct
import sys

manifest, root = sys.argv[1], sys.argv[2]
heads = {}
shape = collections.Counter()
exact = bad = 0
for line in list(open(manifest, encoding="utf-8"))[1:]:
    c = line.rstrip("\n").split("\t")
    if c[11] != "in" or os.path.basename(c[2]) != "blend_map.tif":
        continue
    p = os.path.join(root, c[0], c[1], c[2])
    with open(p, "rb") as f:
        h = f.read(4096)
    size = int(c[3])
    if h[:4] != b"II*\x00":
        bad += 1
        continue
    n = struct.unpack_from("<H", h, 8)[0]
    ents = {}
    for i in range(n):
        tag, typ, cnt, val = struct.unpack_from("<HHII", h, 10 + 12 * i)
        ents[tag] = (typ, cnt, val)
    w, ht = ents[256][2], ents[257][2]
    strip_off, strip_len = ents[273][2], ents[279][2]
    spp = ents[277][2]
    shape[(n, spp, strip_off, ents[259][2], ents[262][2])] += 1
    heads[c[2]] = h[:strip_off]
    if strip_off + strip_len == size and strip_len == w * ht * spp * 2:
        exact += 1
print("tif headers:", len(heads), "not II*:", bad, "| IFD entries, samples/px, strip offset, compression, photometric:", dict(shape))
print("size == 242 + w*h*spp*2 for", exact)
hs = [h for h in heads.values() if len(h) == 242]
varying = [i for i in range(min(len(h) for h in hs)) if len({h[i] for h in hs}) > 1]
print("varying header byte offsets:", varying)
