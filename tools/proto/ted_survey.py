#!/usr/bin/env python3
"""Survey of tile.ted scene graphs: element names, attribute sets, parent relations, counts.

    python ted_survey.py <manifest.tsv> <kits_parent_dir> [max_mb]
"""
import collections
import os
import sys
import xml.etree.ElementTree as ET

manifest, root = sys.argv[1], sys.argv[2]
max_mb = float(sys.argv[3]) if len(sys.argv) > 3 else 20
elems = collections.Counter()
attrs = collections.defaultdict(collections.Counter)
parents = collections.defaultdict(collections.Counter)
versions = collections.Counter()
bad = skipped = files = 0
for line in list(open(manifest, encoding="utf-8"))[1:]:
    c = line.rstrip("\n").split("\t")
    if c[11] != "in" or os.path.basename(c[2]) != "tile.ted":
        continue
    if int(c[3]) > max_mb * 1e6:
        skipped += 1
        continue
    try:
        tree = ET.parse(os.path.join(root, c[0], c[1], c[2]))
    except ET.ParseError:
        bad += 1
        continue
    files += 1
    r = tree.getroot()
    versions[r.get("version")] += 1

    def walk(e, parent):
        elems[e.tag] += 1
        attrs[e.tag][tuple(sorted(e.attrib))] += 1
        parents[e.tag][parent] += 1
        for ch in e:
            walk(ch, e.tag)

    walk(r, "")
print(files, "files parsed,", bad, "bad,", skipped, "skipped for size;", "versions", dict(versions))
for tag, n in elems.most_common():
    ps = ", ".join(f"{p or '-'}:{k}" for p, k in parents[tag].most_common(3))
    sets = attrs[tag]
    print(f"{n:9} {tag:34} parents[{ps}] attribute sets: {len(sets)}")
    for aset, k in sets.most_common(2):
        print(f"            {k:8} {' '.join(aset)}")
