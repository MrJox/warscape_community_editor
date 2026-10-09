#!/usr/bin/env python3
"""span_diff.py <original> <edited> [--expect OLD NEW]: the two files differ in one contiguous span (common prefix and suffix cut off).

Prints the span (offsets, the old and the new bytes). Exit 0 when the files differ in one span that sits inside a single XML tag (no '<' or '>' in it, no
line break) and, with --expect, whose old and new text are exactly OLD and NEW; 1 otherwise. Line endings are compared as they are (the editor keeps them)."""
import sys


def main():
    args = sys.argv[1:]
    expect = None
    if '--expect' in args:
        i = args.index('--expect')
        expect = (args[i + 1].encode(), args[i + 2].encode())
        del args[i:i + 3]
    a, b = open(args[0], 'rb').read(), open(args[1], 'rb').read()
    n = min(len(a), len(b))
    p = 0
    while p < n and a[p] == b[p]:
        p += 1
    s = 0
    while s < n - p and a[len(a) - 1 - s] == b[len(b) - 1 - s]:
        s += 1
    old, new = a[p:len(a) - s], b[p:len(b) - s]
    if not old and not new:
        print('identical')
        return 1 if expect else 0
    print('one span at byte %d: %r -> %r (file %d -> %d bytes)' % (p, old[:80], new[:80], len(a), len(b)))
    ok = not any(c in old + new for c in b'<>\r\n')
    if not ok:
        print('the span crosses a tag or a line')
    if expect and (old, new) != expect:
        print('expected %r -> %r' % expect)
        ok = False
    return 0 if ok else 1


sys.exit(main())
