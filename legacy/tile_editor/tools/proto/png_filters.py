#!/usr/bin/env python3
"""Reproduce the kit's PNG row filter choice: libpng 'minimum sum of absolute differences' heuristic.

For sample PNGs: unfilter the scanlines, re-run the heuristic (try None, Sub, Up, Avg, Paeth; pick the filter with the
smallest sum of |signed byte| of the filtered row; ties keep the earlier filter), and compare the chosen filter and
bytes with the original. Reports per file how many rows agree.

    python png_filters.py <manifest.tsv> <kits_parent_dir> [files_per_type]
"""
import os
import struct
import sys
import zlib

import numpy as np


def parse(b):
    i = 8
    ihdr = None
    idat = []
    while i < len(b):
        n, t = struct.unpack_from(">I4s", b, i)
        d = b[i + 8:i + 8 + n]
        if t == b"IHDR":
            ihdr = struct.unpack(">IIBBBBB", d)
        elif t == b"IDAT":
            idat.append(d)
        i += 12 + n
    return ihdr, zlib.decompress(b"".join(idat))


def paeth(a, b, c):
    p = a.astype(np.int32) + b - c
    pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
    return np.where((pa <= pb) & (pa <= pc), a, np.where(pb <= pc, b, c)).astype(np.uint8)


def filter_row(ft, row, prev, bpp):
    r = row.astype(np.int32)
    left = np.concatenate([np.zeros(bpp, np.uint8), row[:-bpp]]).astype(np.int32)
    up = prev.astype(np.int32)
    ul = np.concatenate([np.zeros(bpp, np.uint8), prev[:-bpp]]).astype(np.int32)
    if ft == 0:
        out = r
    elif ft == 1:
        out = r - left
    elif ft == 2:
        out = r - up
    elif ft == 3:
        out = r - ((left + up) >> 1)
    else:
        out = r - paeth(left.astype(np.uint8), up.astype(np.uint8), ul.astype(np.uint8)).astype(np.int32)
    return (out & 255).astype(np.uint8)


def unfilter(raw, w, h, bpp):
    stride = w * bpp
    rows = []
    prev = np.zeros(stride, np.uint8)
    types = []
    for y in range(h):
        ft = raw[y * (stride + 1)]
        f = np.frombuffer(raw, np.uint8, stride, y * (stride + 1) + 1).astype(np.int32)
        cur = np.zeros(stride, np.int32)
        for x in range(stride):
            a = cur[x - bpp] if x >= bpp else 0
            b = int(prev[x])
            c = int(prev[x - bpp]) if x >= bpp else 0
            if ft == 0:
                p = 0
            elif ft == 1:
                p = a
            elif ft == 2:
                p = b
            elif ft == 3:
                p = (a + b) >> 1
            else:
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                p = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
            cur[x] = (f[x] + p) & 255
        prev = cur.astype(np.uint8)
        rows.append(prev)
        types.append(ft)
    return rows, types


def choose(row, prev, bpp):
    best, best_sum = 0, None
    for ft in (0, 1, 2, 3, 4):
        f = filter_row(ft, row, prev, bpp)
        s = int(np.abs(f.astype(np.int8).astype(np.int32)).sum())
        if best_sum is None or s < best_sum:
            best, best_sum = ft, s
    return best


def main():
    manifest, root = sys.argv[1], sys.argv[2]
    per = int(sys.argv[3]) if len(sys.argv) > 3 else 5
    seen = {}
    for line in list(open(manifest, encoding="utf-8"))[1:]:
        c = line.rstrip("\n").split("\t")
        name = os.path.basename(c[2])
        if c[11] != "in" or name not in ("height_map_0.png", "ground_type_map.png", "nogo.png") or int(c[3]) > 3_000_000:
            continue
        seen[name] = seen.get(name, 0) + 1
        if seen[name] > per:
            continue
        b = open(os.path.join(root, c[0], c[1], c[2]), "rb").read()
        (w, h, depth, ctype, *_), raw = parse(b)
        bpp = {0: 1, 2: 3, 4: 2, 6: 4}[ctype] * (2 if depth == 16 else 1)
        rows, types = unfilter(raw, w, h, bpp)
        agree = 0
        prev = np.zeros(w * bpp, np.uint8)
        for y in range(h):
            if choose(rows[y], prev, bpp) == types[y]:
                agree += 1
            prev = rows[y]
        print(f"{name:20} {w}x{h} depth {depth} type {ctype}: heuristic agrees on {agree}/{h} rows; original filters {sorted(set(types))}")


if __name__ == "__main__":
    main()
