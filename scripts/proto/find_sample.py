#!/usr/bin/env python3
"""Find the smallest BMD xml files containing a given marker string.
    python find_sample.py <manifest.tsv> <kits_parent_dir> '<PROP key' [count]
"""
import os
import sys

manifest, root, needle = sys.argv[1], sys.argv[2], sys.argv[3]
count = int(sys.argv[4]) if len(sys.argv) > 4 else 3
hits = []
for line in list(open(manifest, encoding="utf-8"))[1:]:
    c = line.rstrip("\n").split("\t")
    if c[11] == "in" and (c[2].endswith("_procedural_bmd_data.xml") or ("bmd_data_escalation" in c[2] and c[2].endswith(".xml"))):
        p = os.path.join(root, c[0], c[1], c[2])
        if needle in open(p, encoding="utf-8", errors="replace").read():
            hits.append((os.path.getsize(p), p))
for size, p in sorted(hits)[:count]:
    print(size, os.path.getsize(p[:-3] + "bin"), p)
