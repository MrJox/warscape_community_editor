"""Apply exact-text source edits listed in a spec file (line-ending agnostic, fails if any text is absent).

    python scripts/apply_edits.py <spec.py>

The spec is a Python file defining EDITS = [(path, old, new), ...]. Used for multi-file changes that are
awkward through shell heredocs.
"""
import os
import runpy
import sys

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")


def main():
    edits = runpy.run_path(sys.argv[1])["EDITS"]
    files = {}
    for rel, old, new in edits:
        p = os.path.join(ROOT, rel)
        s = files.get(p) or open(p, "rb").read().decode("utf-8")
        nl = "\r\n" if "\r\n" in s else "\n"
        o, n = old.replace("\r\n", "\n").replace("\n", nl), new.replace("\r\n", "\n").replace("\n", nl)
        if o not in s:
            sys.exit(f"not found in {rel}:\n{old[:200]}")
        files[p] = s.replace(o, n, 1)
    for p, s in files.items():
        open(p, "wb").write(s.encode("utf-8"))
    print(f"applied {len(edits)} edits to {len(files)} files")


if __name__ == "__main__":
    main()
