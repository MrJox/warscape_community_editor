"""Compares the reference screenshots of two gate runs.

    python scripts/imgcmp.py <run a> <run b> [--tolerance PSNR_DB]

Prints, per PNG present in both runs: identical (bytes, or pixels with another encoder), or mean / max absolute difference (0-255) and PSNR. Exits 1 when an
image is below the PSNR tolerance (default 35 dB). Until the renderer runs on a fixed-step clock (PLAN.md, Phase 3)
the weather shots are not bit-identical between runs: wind and tone-map adaptation follow wall-clock time.
"""
import math
import os
import sys

import numpy as np
from PIL import Image


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    tol = 35.0
    if "--tolerance" in sys.argv:
        tol = float(sys.argv[sys.argv.index("--tolerance") + 1])
        args = [a for a in args if a != str(sys.argv[sys.argv.index("--tolerance") + 1])]
    if len(args) != 2:
        print(__doc__)
        return 2
    a_dir, b_dir = args
    bad = 0
    for name in sorted(os.listdir(a_dir)):
        if not name.endswith(".png") or not os.path.exists(os.path.join(b_dir, name)):
            continue
        pa, pb = os.path.join(a_dir, name), os.path.join(b_dir, name)
        if open(pa, "rb").read() == open(pb, "rb").read():
            print(f"{name:40s} identical")
            continue
        a = np.asarray(Image.open(pa).convert("RGB"), dtype=np.float64)
        b = np.asarray(Image.open(pb).convert("RGB"), dtype=np.float64)
        if a.shape != b.shape:
            print(f"{name:40s} SIZE {a.shape} vs {b.shape}")
            bad += 1
            continue
        d = np.abs(a - b)
        if not d.any():
            print(f"{name:40s} identical pixels (file bytes differ: another PNG encoder)")
            continue
        mse = float((d * d).mean())
        psnr = math.inf if mse == 0 else 10 * math.log10(255 * 255 / mse)
        ok = psnr >= tol
        bad += not ok
        print(f"{name:40s} mean {d.mean():7.3f}  max {d.max():5.0f}  psnr {psnr:6.2f} dB  {'ok' if ok else 'BELOW TOLERANCE'}")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
