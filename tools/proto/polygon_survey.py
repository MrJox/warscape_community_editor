#!/usr/bin/env python3
"""Polygon-type instances of tile.ted: the most common attribute set per type (for creating new polygons) and the hint types.

    python polygon_survey.py <manifest.tsv> <kits_parent_dir> [max_mb]
"""
import collections
import os
import sys
import xml.etree.ElementTree as ET

manifest, root = sys.argv[1], sys.argv[2]
max_mb = float(sys.argv[3]) if len(sys.argv) > 3 else 8
types = collections.defaultdict(lambda: collections.defaultdict(collections.Counter))
hints = collections.Counter()
counts = collections.Counter()
for line in list(open(manifest, encoding="utf-8"))[1:]:
    c = line.rstrip("\n").split("\t")
    if c[11] != "in" or os.path.basename(c[2]) != "tile.ted" or int(c[3]) > max_mb * 1e6:
        continue
    try:
        r = ET.parse(os.path.join(root, c[0], c[1], c[2])).getroot()
    except ET.ParseError:
        continue
    for e in r.iter():
        if e.find("points") is None or not e.get("position"):
            continue
        counts[e.tag] += 1
        for k, v in e.attrib.items():
            if k in ("position", "area", "name"):
                continue
            types[e.tag][k][v] += 1
        if e.tag == "AI_HINT_POLYGON_INSTANCE":
            hints[e.get("hint_type")] += 1
print("hint types:", dict(hints))
for t, n in counts.most_common():
    print(f"== {t} ({n})")
    for k, vals in sorted(types[t].items()):
        top = vals.most_common(3)
        print(f"   {k}: " + " | ".join(f"{v!r} x{m}" for v, m in top))
