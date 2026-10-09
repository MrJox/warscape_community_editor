#!/usr/bin/env python3
"""Which files does TEd touch? Summarises binaries/ted_p4_history (Perforce edit/add/revert/delete lines TEd issued)
and, when --strings is given, the file-name strings embedded in TEd.AssemblyKit.exe.

    python ted_files.py <kit>/binaries/ted_p4_history
    python ted_files.py --strings <kit>/binaries/TEd.AssemblyKit.exe
"""
import collections
import re
import sys


def p4(path):
    verbs = collections.Counter()
    exts = collections.Counter()
    names = collections.Counter()
    for line in open(path, encoding="utf-8", errors="replace"):
        parts = line.strip().split(" ", 1)
        if len(parts) != 2:
            continue
        verb, p = parts[0], parts[1].replace("\\", "/")
        verbs[verb] += 1
        base = p.rsplit("/", 1)[-1].lower()
        exts[base.rsplit(".", 1)[-1] if "." in base else "(none)"] += 1
        names[base] += 1
    print("verbs:", dict(verbs))
    print("extensions:", exts.most_common(30))
    print("most touched file names:")
    for n, c in names.most_common(60):
        print(f"  {c:6} {n}")


def strings(path):
    d = open(path, "rb").read()
    found = set(m.group().decode("latin1") for m in re.finditer(rb"[\x20-\x7e]{4,200}", d))
    found |= set(m.group().decode("utf-16le") for m in re.finditer(rb"(?:[\x20-\x7e]\x00){4,200}", d))
    pat = re.compile(r"\.(xml|tif|tiff|png|bin|dds|txt|script|markers|ted|tga|data|agf|bob|tree_list|outlines|editor|rigid_model_v2|cs2)\b", re.I)
    for s in sorted(x for x in found if pat.search(x) and len(x) < 160):
        print(s)


if __name__ == "__main__":
    if len(sys.argv) >= 3 and sys.argv[1] == "--strings":
        strings(sys.argv[2])
    else:
        p4(sys.argv[1])
