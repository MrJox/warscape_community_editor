"""Vendor libvpx's VP8 encoder and decoder as plain C (no assembly, no SIMD) into third_party/libvpx.

libvpx's own build needs a POSIX shell, make and gcc to run `configure`; this script does what configure and make would for a
generic C target instead, so that `crates/wce_video/build.rs` can compile the result with the cc crate on every platform:

- writes `vpx_config.h` / `vpx_config.c` from configure's own flag lists (VP8 encoder and decoder, multithreading, nothing else);
- writes the run-time CPU dispatch headers (`vp8_rtcd.h`, `vpx_dsp_rtcd.h`, `vpx_scale_rtcd.h`) with libvpx's `rtcd.pl` (needs perl)
  for the `generic` architecture;
- writes `vpx_version.h` from the tag;
- evaluates the libraries' makefile fragments (`vpx_codec.mk`, `vp8_common.mk`, `vp8cx.mk`, `vp8dx.mk`...) to list the C files
  the library is built from, copies them and every header of those folders (architecture folders left out), and writes
  `sources.txt`.

Usage: python scripts/vendor_libvpx.py <libvpx checkout at a release tag> <tag, e.g. v1.16.0>
The output replaces third_party/libvpx.
"""
import os
import re
import shutil
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(REPO, "third_party", "libvpx")

# What configure would enable for `--target=generic-gnu --disable-vp9 --enable-multithread --disable-postproc` (plus its defaults).
ENABLED = {
    "vp8", "vp8_encoder", "vp8_decoder", "encoders", "decoders", "static", "multithread", "os_support",
    "temporal_denoising", "install_libs", "install_bins", "install_docs", "codec_srcs",
}
# Folders whose makefile fragments make the library (libs.mk), with the variable that lists each one's sources.
FRAGMENTS = [
    ("vpx", "vpx_codec.mk", "API_SRCS"),
    ("vpx_mem", "vpx_mem.mk", "MEM_SRCS"),
    ("vpx_scale", "vpx_scale.mk", "SCALE_SRCS"),
    ("vpx_ports", "vpx_ports.mk", "PORTS_SRCS"),
    ("vpx_dsp", "vpx_dsp.mk", "DSP_SRCS"),
    ("vpx_util", "vpx_util.mk", "UTIL_SRCS"),
    ("vp8", "vp8_common.mk", "VP8_COMMON_SRCS"),
    ("vp8", "vp8cx.mk", "VP8_CX_SRCS"),
    ("vp8", "vp8dx.mk", "VP8_DX_SRCS"),
]
ARCH_DIRS = {"x86", "arm", "mips", "loongarch", "ppc", "neon", "msa", "mmi", "lsx", "aarch64", "riscv"}

LICENCE_HEADER = """/*
 *  Copyright (c) 2010 The WebM project authors. All Rights Reserved.
 *
 *  Use of this source code is governed by a BSD-style license
 *  that can be found in the LICENSE file in the root of the source
 *  tree. An additional intellectual property rights grant can be found
 *  in the file PATENTS.  All contributing project authors may
 *  be found in the AUTHORS file in the root of the source tree.
 */
"""


def shell_list(configure, name):
    """A configure list (NAME=" a b ${OTHER} "), with ${...} references expanded."""
    m = re.search(r'^%s="(.*?)"' % name, configure, re.S | re.M)
    if not m:
        raise SystemExit("configure has no " + name)
    words = []
    for w in m.group(1).split():
        ref = re.fullmatch(r"\$\{(\w+)\}", w)
        words.extend(shell_list(configure, ref.group(1)) if ref else [w])
    return words


def value(name):
    """A make variable's value: CONFIG_X / HAVE_X are 'yes' or ''."""
    key = name.split("_", 1)[1].lower() if "_" in name else name.lower()
    if name.startswith("CONFIG_") or name.startswith("HAVE_"):
        return "yes" if key in ENABLED else ""
    return ""


def subst(text):
    return re.sub(r"\$\((\w+)\)", lambda m: value(m.group(1)), text)


def eval_fragment(path, var):
    """The files a fragment adds to `var`-yes, honouring ifeq/ifneq/ifdef/else/endif."""
    files = []
    stack = []  # each entry: is this branch live
    for raw in open(path, encoding="utf-8").read().replace("\\\n", " ").splitlines():
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        live = all(stack)
        m = re.match(r"(ifeq|ifneq)\s*\((.*?),(.*?)\)\s*$", line)
        if m:
            eq = subst(m.group(2)).strip() == subst(m.group(3)).strip()
            stack.append(eq if m.group(1) == "ifeq" else not eq)
            continue
        m = re.match(r"(ifdef|ifndef)\s+(\w+)", line)
        if m:
            d = bool(value(m.group(2)))
            stack.append(d if m.group(1) == "ifdef" else not d)
            continue
        if line == "else":
            stack[-1] = not stack[-1]
            continue
        if line == "endif":
            stack.pop()
            continue
        if not live:
            continue
        m = re.match(r"(\w+)-(\S+)\s*\+?=\s*(.*)$", line)
        if m and m.group(1) == var and subst(m.group(2)) == "yes":
            files.extend(subst(m.group(3)).split())
    return files


def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)


def main():
    src, tag = sys.argv[1], sys.argv[2]
    configure = open(os.path.join(src, "configure"), encoding="utf-8").read()
    arch = shell_list(configure, "ARCH_LIST")
    have = shell_list(configure, "HAVE_LIST")
    config = shell_list(configure, "CONFIG_LIST")

    if os.path.isdir(OUT):
        shutil.rmtree(OUT)
    os.makedirs(OUT)

    # vpx_config.h: every flag configure knows, 0 or 1; pthread.h and unistd.h by platform (configure probes them).
    h = [LICENCE_HEADER, "/* Written by scripts/vendor_libvpx.py for a generic C build (what configure would write). */\n",
         "#ifndef VPX_CONFIG_H\n#define VPX_CONFIG_H\n", "#if defined(_MSC_VER)\n#define RESTRICT __restrict\n#define INLINE __inline\n",
         "#else\n#define RESTRICT __restrict__\n#define INLINE inline\n#endif\n"]
    for a in arch:
        h.append("#define VPX_ARCH_%s 0\n" % a.upper())
    for x in have:
        if x in ("pthread_h", "unistd_h"):
            h.append("#if defined(_WIN32)\n#define HAVE_%s 0\n#else\n#define HAVE_%s 1\n#endif\n" % (x.upper(), x.upper()))
        else:
            h.append("#define HAVE_%s %d\n" % (x.upper(), 1 if x == "vpx_ports" else 0))
    for c in config:
        h.append("#define CONFIG_%s %d\n" % (c.upper(), 1 if c in ENABLED else 0))
    h.append("#define DECODE_WIDTH_LIMIT 16384\n#define DECODE_HEIGHT_LIMIT 16384\n#endif /* VPX_CONFIG_H */\n")
    write(os.path.join(OUT, "vpx_config.h"), "".join(h))
    write(os.path.join(OUT, "vpx_config.c"), LICENCE_HEADER +
          '#include "vpx/vpx_codec.h"\nstatic const char* const cfg = "--target=generic-gnu --disable-vp9 (wce vendor_libvpx.py)";\n'
          "const char *vpx_codec_build_config(void) {return cfg;}\n")

    # The make config rtcd.pl reads.
    mk = os.path.join(OUT, "_config.mk")
    write(mk, "".join("CONFIG_%s=yes\n" % c.upper() for c in sorted(ENABLED)) + "HAVE_VPX_PORTS=yes\n")
    for sym, defs, out in [("vp8_rtcd", "vp8/common/rtcd_defs.pl", "vp8_rtcd.h"),
                           ("vpx_dsp_rtcd", "vpx_dsp/vpx_dsp_rtcd_defs.pl", "vpx_dsp_rtcd.h"),
                           ("vpx_scale_rtcd", "vpx_scale/vpx_scale_rtcd.pl", "vpx_scale_rtcd.h")]:
        text = subprocess.run(["perl", os.path.join(src, "build/make/rtcd.pl"), "--arch=generic", "--sym=" + sym, "--config=" + mk,
                               os.path.join(src, defs)], check=True, capture_output=True, text=True).stdout
        write(os.path.join(OUT, out), text)
    os.remove(mk)

    major, minor, patch = re.fullmatch(r"v(\d+)\.(\d+)\.(\d+)", tag).groups()
    write(os.path.join(OUT, "vpx_version.h"), LICENCE_HEADER +
          "#define VERSION_MAJOR  %s\n#define VERSION_MINOR  %s\n#define VERSION_PATCH  %s\n#define VERSION_EXTRA  \"\"\n"
          "#define VERSION_PACKED ((VERSION_MAJOR << 16) | (VERSION_MINOR << 8) | (VERSION_PATCH))\n"
          "#define VERSION_STRING_NOSP \"%s\"\n#define VERSION_STRING \" %s\"\n" % (major, minor, patch, tag, tag))

    sources = []
    folders = set()
    for folder, frag, var in FRAGMENTS:
        folders.add(folder)
        for f in eval_fragment(os.path.join(src, folder, frag), var):
            if not f.endswith(".c"):
                continue
            rel = f if os.path.exists(os.path.join(src, f)) and "/" in f and f.split("/")[0] == folder else folder + "/" + f
            if not os.path.exists(os.path.join(src, rel)):
                raise SystemExit("missing source " + rel)
            if set(rel.split("/")) & ARCH_DIRS:
                continue
            if rel not in sources:
                sources.append(rel)
    for rel in sources:
        dst = os.path.join(OUT, rel)
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        shutil.copyfile(os.path.join(src, rel), dst)
    for folder in sorted(folders):
        for root, dirs, files in os.walk(os.path.join(src, folder)):
            dirs[:] = [d for d in dirs if d not in ARCH_DIRS]
            for f in files:
                if f.endswith(".h"):
                    rel = os.path.relpath(os.path.join(root, f), src).replace("\\", "/")
                    dst = os.path.join(OUT, rel)
                    os.makedirs(os.path.dirname(dst), exist_ok=True)
                    shutil.copyfile(os.path.join(src, rel), dst)
    for f in ("LICENSE", "PATENTS", "AUTHORS"):
        shutil.copyfile(os.path.join(src, f), os.path.join(OUT, f))
    sources.append("vpx_config.c")
    write(os.path.join(OUT, "sources.txt"), "\n".join(sorted(sources)) + "\n")
    write(os.path.join(OUT, "README.md"),
          "# libvpx %s (VP8 only, generic C)\n\nFrom https://chromium.googlesource.com/webm/libvpx, tag %s. Generated by "
          "`scripts/vendor_libvpx.py`: the VP8 encoder and decoder and what they use, without assembly or SIMD; the config and "
          "run-time dispatch headers written for a generic C build. Built by `crates/wce_video/build.rs`. BSD-3-Clause "
          "(`LICENSE`) with Google's patent grant (`PATENTS`).\n" % (tag, tag))
    print(len(sources), "sources")


if __name__ == "__main__":
    main()
