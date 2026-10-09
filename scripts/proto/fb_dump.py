#!/usr/bin/env python3
"""Annotated dump of a FASTBIN0 file: u16-length ASCII strings are decoded inline, everything else printed as
hex bytes in groups. Used to reverse-engineer tile_database.bin against _settings.xml / TILES/*.xml.

    python fb_dump.py <file.bin> [start] [length]
"""
import struct
import sys

b = open(sys.argv[1], "rb").read()
start = int(sys.argv[2], 0) if len(sys.argv) > 2 else 0
length = int(sys.argv[3], 0) if len(sys.argv) > 3 else 600
i, end = start, min(len(b), start + length)
line = []


def flush():
    global line
    if line:
        print(" ".join(line))
        line = []


while i < end:
    if i + 2 <= len(b):
        n = struct.unpack_from("<H", b, i)[0]
        if 2 <= n <= 120 and i + 2 + n <= len(b):
            c = b[i + 2:i + 2 + n]
            if all(32 <= x < 127 for x in c):
                flush()
                print(f"[{i:7}] STR({n}) {c.decode()!r}")
                i += 2 + n
                continue
    line.append(f"{b[i]:02x}")
    i += 1
    if len(line) == 32:
        flush()
flush()
