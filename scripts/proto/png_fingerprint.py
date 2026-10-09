#!/usr/bin/env python3
"""Does the kit's PNG writer equal 'zlib at level N over the same filtered scanlines, IDAT chunks of K bytes'?

For every in-scope PNG of the manifest (or a sample): split into chunks, inflate IDAT, recompress the inflated
(filtered) stream with zlib levels 0-9 and several memLevels / strategies, cut it into IDAT chunks of the original
first chunk size, rebuild the file and compare with the original bytes. Also tallies the filter types used per file
and the metadata chunks. Prints the best match per file kind.

    python png_fingerprint.py <manifest.tsv> <kits_parent_dir> [limit]
"""
import collections
import os
import struct
import sys
import zlib


def chunks(b):
    if b[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("not PNG")
    i = 8
    while i < len(b):
        n, t = struct.unpack_from(">I4s", b, i)
        yield t, b[i + 8:i + 8 + n], i
        i += 12 + n


def rebuild(b, new_idat_stream, chunk_size):
    out = [b[:8]]
    done = False
    for t, d, _ in chunks(b):
        if t == b"IDAT":
            if done:
                continue
            done = True
            for k in range(0, len(new_idat_stream), chunk_size):
                piece = new_idat_stream[k:k + chunk_size]
                out.append(struct.pack(">I4s", len(piece), b"IDAT") + piece + struct.pack(">I", zlib.crc32(b"IDAT" + piece)))
        else:
            out.append(struct.pack(">I4s", len(d), t) + d + struct.pack(">I", zlib.crc32(t + d)))
    return b"".join(out)


def main():
    manifest, root = sys.argv[1], sys.argv[2]
    limit = int(sys.argv[3]) if len(sys.argv) > 3 else 40
    results = collections.defaultdict(collections.Counter)
    seen = collections.Counter()
    for line in list(open(manifest, encoding="utf-8"))[1:]:
        c = line.rstrip("\n").split("\t")
        if c[11] != "in" or not c[2].endswith(".png") or int(c[3]) > 40_000_000:
            continue
        name = os.path.basename(c[2])
        if "--all" not in sys.argv and name not in ("height_map_0.png", "ground_type_map.png", "nogo.png", "lf_influence_map.png"):
            continue
        if "--all" in sys.argv:
            name = "/".join(c[2].split("/")[-2:-1]) if "/raw/" in c[2] or "/wm/" in c[2] else name
        seen[name] += 1
        if seen[name] > limit:
            continue
        b = open(os.path.join(root, c[0], c[1], c[2]), "rb").read()
        try:
            idats = [d for t, d, _ in chunks(b) if t == b"IDAT"]
            types = [t.decode() for t, _, _ in chunks(b)]
            zlib.decompress(b"".join(idats))
        except Exception as e:  # noqa
            results["(unreadable: " + str(e)[:30] + ")"][(None, (), (c[2],))] += 1
            continue
        sizes = {len(d) for d in idats[:-1]} or {len(idats[0])}
        raw = zlib.decompress(b"".join(idats))
        best = None
        for level in (6, 9, 1, 5, 7, 8, 4, 3, 2, 0):
            for mem in (8, 9):
                for strat in (0, 1, 2):
                    co = zlib.compressobj(level, zlib.DEFLATED, 15, mem, strat)
                    z = co.compress(raw) + co.flush()
                    if rebuild(b, z, max(sizes)) == b:
                        best = (level, mem, strat)
                        break
                if best:
                    break
            if best:
                break
        results[name][(best, tuple(sorted(sizes))[:2], tuple(types))] += 1
    for name, cnt in results.items():
        print("==", name, "files sampled", sum(cnt.values()))
        for k, n in cnt.most_common():
            print("  ", n, "match(level,mem,strategy)=", k[0], "idat chunk sizes", k[1], "chunks", k[2]) if "--all" not in sys.argv else None
        if "--all" in sys.argv:
            hit = sum(n for k, n in cnt.items() if k[0]); print("   matched", hit, "of", sum(cnt.values()))


if __name__ == "__main__":
    main()
