#!/usr/bin/env python3
"""Survey of the BATTLE_MAP_DEFINITION_DATA xml files (stage-A export, sibling of *_procedural_bmd_data.bin and
bmd_data_escalation*.bin): which element/attribute paths occur, how often, in how many files.

    python bmd_survey.py <manifest.tsv> <kits_parent_dir> [--paths] [--attrs ELEMENT]
"""
import collections
import os
import sys
import xml.etree.ElementTree as ET


def main():
    manifest, root = sys.argv[1], sys.argv[2]
    show_paths = "--paths" in sys.argv
    attr_of = sys.argv[sys.argv.index("--attrs") + 1] if "--attrs" in sys.argv else None
    files = 0
    bad = []
    count = collections.Counter()
    infiles = collections.Counter()
    attrs = collections.defaultdict(collections.Counter)
    versions = collections.Counter()
    for line in list(open(manifest, encoding="utf-8"))[1:]:
        c = line.rstrip("\n").split("\t")
        if c[11] != "in" or not (c[2].endswith("_procedural_bmd_data.xml") or "bmd_data_escalation" in c[2] and c[2].endswith(".xml")):
            continue
        files += 1
        try:
            tree = ET.parse(os.path.join(root, c[0], c[1], c[2]))
        except ET.ParseError as e:
            bad.append(f"{c[2]}: {e}")
            continue
        seen = set()

        def walk(e, path):
            p = path + "/" + e.tag
            count[p] += 1
            seen.add(p)
            attrs[e.tag][tuple(e.attrib.keys())] += 1
            if "serialise_version" in e.attrib:
                versions[(e.tag, e.attrib["serialise_version"])] += 1
            for ch in e:
                walk(ch, p)

        walk(tree.getroot(), "")
        for p in seen:
            infiles[p] += 1
    print(files, "xml files,", len(bad), "unparseable:", bad[:3])
    print("serialise_version values:", dict(versions))
    if attr_of:
        for k, n in attrs[attr_of].most_common():
            print(n, k)
        return
    for p in sorted(count):
        if show_paths or count[p] != infiles[p] or True:
            print(f"{count[p]:8} in {infiles[p]:5} files  {p}")


if __name__ == "__main__":
    main()
