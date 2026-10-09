#!/usr/bin/env python3
"""Annotated dump of a .markers file: UTF-16 length-prefixed strings are decoded inline, the rest is hex words.

    python markers_dump.py <file.markers> [max_bytes]
"""
import re
import struct
import sys

b = open(sys.argv[1], "rb").read()
limit = int(sys.argv[2]) if len(sys.argv) > 2 else len(b)
i = 0
out = []
while i < min(len(b), limit):
    # try a u16-length UTF-16 string of printable ASCII (len >= 3)
    if i + 2 <= len(b):
        n = struct.unpack_from("<H", b, i)[0]
        if 3 <= n <= 80 and i + 2 + 2 * n <= len(b):
            chunk = b[i + 2:i + 2 + 2 * n]
            if all(chunk[k + 1] == 0 and 32 <= chunk[k] < 127 for k in range(0, len(chunk), 2)):
                out.append(f"\n[{i:5}] STR16({n}) {chunk[::2].decode()!r}")
                i += 2 + 2 * n
                continue
    out.append(f"{b[i]:02x}")
    i += 1
print(" ".join(out))
print("\nsize", len(b))
