#!/usr/bin/env python3
"""The editor's river shader for Attila battles: the game's own `rigidriver` `ps30_main_battle_river` (its 10 dx11 variants, shadows
and shader models), patched to draw what the vanilla one reads and drops (docs/re/water.md, "The editor's river shader"):

- flow speed: the ripples run down the river at the speed of the river's `flow_speed` (0.5 = vanilla's; 0 still, negative upstream),
  as two layers half a cycle apart (a flow map's way, so a speed changing along the river doesn't stretch the ripples over time);
- alpha fade: the water fades into what is under it as `alpha_fade` goes to 0 (a river's ends);
- foam: the vertex's foam (`foam_amount` times the editor's foam map: where the water meets the bank or a rock, and in a trail behind
  what stands in it) as white foam on the ripples.

Only on rivers of the editor's: their vertices carry 2 + that foam where BOB writes the place across (0 to 1, which the vanilla shader
passes on and never reads; `wce_tile_io::attila::river::MARKED_FOAM_BASE`). The others are drawn as the game draws them.

The game reads shaders when it makes its graphics device, before the mod packs are mounted: they load from a movie-type pack only
(docs/re/game_command_line.md).

Usage: python3 -I river_shader.py <vanilla fxc folder> <out folder>
  The folder holds the vanilla `fxc/dx11*rigidriver.hlsl_ps_ps30_main_battle_river.fxc` (`vfs_ls <game> fxc/ --vanilla --extract-all`);
  the patched ones are written to <out folder> under the same names. Needs vkd3d's `vkd3d-compiler` (the disassembly the patch finds
  its places in) and `vkd3d-dxbc` (the checksum): $VKD3D_BIN, else the build's `build/_deps/prefix/bin`.
"""
import os, re, struct, subprocess, sys, tempfile

# Flow: the cycle of the two layers (seconds), and vanilla's speed down the river (uv per second, the shader's own `0.05 * time`).
FLOW_CYCLE_S = 3.0
VANILLA_SCROLL = 0.05

# Foam only where the water meets something, and in a trail behind what stands in it (user 2026-11-28): worked out by the editor from
# the ground and the buildings (wce_tile::river_foam), each vertex of the dense river mesh carrying 2 + the river's foam times its map;
# the shader draws what the vertex says. (Working it out here from the screen's depth was tried: from a low view the face of a rock
# the point is in and a rock in front of it can't be told apart, so the trail came and went with the camera.)

# Foam: metres per repeat of its texture, across and along alike, from the vertex's metres (v4 is them times (0.01, 0.001)); it moves
# down the river alone, at the ripples' speed (vanilla's 0.05 uv a second on their third layer, 0.177 uv a metre: 0.28 m/s) plus the
# flow's offsets. Not on the ripples' own uv: that layer also drifts with the wind at 0.12 uv a second, mostly across the river once
# made even (user 2026-11-28: the foam went sideways).
FOAM_REPEAT_M = 9.0
# The texture's ripples are longer across than along (it is laid on the river stretched to 17 m across by 5.6 m along):
# across, it repeats this many times as often, for rounder flecks (3 showed its repeats as rows of rings).
FOAM_ACROSS_SQUEEZE = 1.6
FOAM_UV = (100.0 * FOAM_ACROSS_SQUEEZE / FOAM_REPEAT_M, 1000.0 / FOAM_REPEAT_M)
# A second look at the texture under the foam, at this scale and offset (uv), averaged with the first: one repeat alone showed as a
# regular pattern of flecks.
FOAM_SECOND = (0.41, (0.37, 0.71))
RIPPLE_UV_PER_M = 0.177

# Foam: the ripples' steepness (|g - 0.5| + |a - 0.5| of `river_normal.dds`: median
# 0.22, 90% under 0.36) over which a river of foam 0 and of foam 1 shows it, and how sharp its edge is.
FOAM_FROM = (0.3, 0.04)
FOAM_EDGE = 8.0

# Foam: how much of the colour under it it hides at most, and its light: the sun's colour (half of it grey), times this plus the sun's
# height times the next.
FOAM_OPACITY = 0.85
FOAM_LIGHT = (0.25, 0.6)

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.environ.get('VKD3D_BIN', os.path.join(ROOT, 'build', '_deps', 'prefix', 'bin'))
LIB = os.path.join(os.path.dirname(BIN), 'lib')


def tool(name, *args, out=None):
    env = dict(os.environ, LD_LIBRARY_PATH=LIB + ':' + os.environ.get('LD_LIBRARY_PATH', ''))
    r = subprocess.run([os.path.join(BIN, name), *args], env=env, capture_output=True)
    if r.returncode != 0:
        sys.exit(f'{name} failed: {r.stderr.decode(errors="replace")}')
    return r.stdout


# ---- SM4/5 tokens ----

ADD, DP3, FRC, LT, MAD, MAX, MOV, MOVC, MUL, RSQ, SAMPLE, SAMPLE_L, RET = 0, 16, 26, 49, 50, 52, 54, 55, 56, 68, 69, 72, 62
DCL_INPUT_PS, DCL_TEMPS, CUSTOMDATA = 98, 104, 53
SAT = 1 << 13
T_TEMP, T_INPUT, T_OUTPUT, T_IMM, T_CB = 0, 1, 2, 4, 8
SW = {'x': 0, 'y': 1, 'z': 2, 'w': 3}


def instructions(toks):
    out, i = [], 0
    while i < len(toks):
        n = toks[i + 1] if toks[i] & 0x7ff == CUSTOMDATA else (toks[i] >> 24) & 0x7f
        out.append(list(toks[i:i + n]))
        i += n
    return out


def operand_len(t, i):
    """The length of the operand at t[i]."""
    tok, j = t[i], i + 1
    ext = tok & (1 << 31)
    while ext:
        ext = t[j] & (1 << 31)
        j += 1
    kind = (tok >> 12) & 0xff
    if kind in (T_IMM, 5):
        j += (1 if tok & 3 == 1 else 4) * (2 if kind == 5 else 1)
    for d in range((tok >> 20) & 3):
        rep = (tok >> (22 + 3 * d)) & 7
        j += {0: 1, 1: 2, 2: 0, 3: 1, 4: 2}[rep]
        if rep in (2, 3, 4):
            j += operand_len(t, j)
    return j - i


def split(ins):
    """An instruction's opcode tokens (with its extended ones) and its operands."""
    j = 1
    while ins[j - 1] & (1 << 31):
        j += 1
    head, ops = ins[:j], []
    while j < len(ins):
        n = operand_len(ins, j)
        ops.append(ins[j:j + n])
        j += n
    return head, ops


def join(head, ops, sat=False):
    body = [t for o in ops for t in o]
    h = list(head)
    h[0] = (h[0] & ~(0x7f << 24) & ~SAT) | ((len(h) + len(body)) << 24) | (SAT if sat else 0)
    return h + body


def op(code, *ops, sat=False):
    return join([code], ops, sat)


def mask(m):
    return sum(1 << SW[c] for c in m)


def swz(s):
    s = (s + s[-1] * 4)[:4]
    return sum(SW[c] << (2 * k) for k, c in enumerate(s))


def reg(kind, idx, sel, dst=False, neg=False, abs_=False):
    t = {'r': T_TEMP, 'v': T_INPUT, 'o': T_OUTPUT}[kind]
    tok = 2 | (t << 12) | (1 << 20) | ((mask(sel) << 4) if dst else ((1 << 2) | (swz(sel) << 4)))
    mod = (1 if neg else 0) | (2 if abs_ else 0)
    return ([tok | (1 << 31), 1 | (mod << 6)] if mod else [tok]) + [idx]


def cb(buf, idx, sel, neg=False):
    tok = 2 | (1 << 2) | (swz(sel) << 4) | (T_CB << 12) | (2 << 20)
    return ([tok | (1 << 31), 1 | (1 << 6)] if neg else [tok]) + [buf, idx]


def imm(*v):
    v = list(v) + [v[-1]] * (4 - len(v))
    return [2 | (1 << 2) | (swz('xyzw') << 4) | (T_IMM << 12)] + [struct.unpack('<I', struct.pack('<f', x))[0] for x in v]


def imm1(x):
    return [1 | (T_IMM << 12), struct.unpack('<I', struct.pack('<f', x))[0]]


def with_dst(ins, dst, addr=None, lod=None, identity=False):
    """A sample instruction like `ins`, writing `dst` from `addr` (the resource's swizzle made xyzw when `identity`)."""
    head, ops = split(ins)
    ops = list(ops)
    ops[0] = dst
    if addr is not None:
        ops[1] = addr
    if identity:
        ops[2] = [(ops[2][0] & ~(0xff << 4)) | (swz('xyzw') << 4)] + ops[2][1:]
    if lod is not None:
        ops[4] = lod
    return join(head, ops)


# ---- the patch ----

def find(lines, pattern, start=0, nth=0):
    hits = [k for k in range(start, len(lines)) if re.search(pattern, lines[k])]
    if len(hits) <= nth:
        raise SystemExit(f'not found: {pattern}')
    return hits[nth]


def patch_shader(toks, asm):
    """The patched token stream of a shader (its version and length tokens first) and its disassembly (one line per instruction)."""
    body = instructions(toks[2:])
    lines = [l.strip() for l in asm.splitlines() if l.strip()][1:]
    assert len(lines) == len(body), (len(lines), len(body))

    temps = next(i[1] for i in body if i[0] & 0x7ff == DCL_TEMPS)
    N = [temps + k for k in range(8)]
    # Where: the screen uv (the line before the ripples), the ripples' block (three samples of the normal map, to the normalised
    # normal), the water's depth, the lighting buffer, the refraction (the scene behind), the colour before the fog, the colour.
    s0 = find(lines, r'^mul r1\.xy, r2\.xyxx, l\(7\.5')
    e0 = find(lines, r'^mul r1\.xyz, r0\.x, r1\.xyzx$', s0)
    assert re.match(r'^mul r0\.zw, r0\.zzzw, cb0\[27\]\.zzzw$', lines[s0 - 1]), lines[s0 - 1]
    time = split(body[find(lines, r'^mul r1\.xyz, cb0\[26\]\.x, l\(-8')])[1][1]
    depth = find(lines, r'^add (r\d+)\.([xyzw]), -\1\.\2, v1\.y$')
    dreg, dcomp = re.match(r'^add r(\d+)\.([xyzw])', lines[depth]).groups()
    assert re.match(rf'^max r{dreg}\.{dcomp}, r{dreg}\.{dcomp}, l\(0', lines[depth + 1]), lines[depth + 1]
    light = int(re.search(r'cb(\d+)\[11\]', lines[find(lines, r'^lt r\d+\.w, l\(0\.0+e\+00\), cb\d+\[11\]\.x$')]).group(1))
    refraction = body[find(lines, r'^sample_l\S* r\d+\.\w+, r0\.zwzz, t\d+\.\w+, s\d+, r0\.w$', nth=1)]
    noise = body[find(lines, r'^sample\S* r1\.\w+, r1\.xyxx, t\d+\.', s0)]
    tail = find(lines, r'^mad o3\.xyz, r0\.x, r1\.xyzx, r0\.yzwy$')
    assert lines[tail - 1] == 'add r1.xyz, -r0.yzwy, r1.xyzx', lines[tail - 1]
    block = body[s0:e0 + 1]

    r = lambda k, sel, **kw: reg('r', N[k], sel, **kw)

    def steepness(phase, into):
        """The ripples' steepness under the foam for a flow layer (`phase`: N2's z or w, its offset) into `into` (N's register, comp):
        the mean of the texture at the foam's scale and at FOAM_SECOND times it (its repeats then don't line up)."""
        return [
            op(MUL, r(5, 'xy', dst=True), reg('v', 4, 'xyxx'), imm(*FOAM_UV)),
            op(MAD, r(5, 'y', dst=True), time, imm(-VANILLA_SCROLL / RIPPLE_UV_PER_M / FOAM_REPEAT_M), r(5, 'y')),
            op(MAD, r(5, 'y', dst=True), r(2, phase), imm(-1.0 / (RIPPLE_UV_PER_M * FOAM_REPEAT_M)), r(5, 'y')),
            with_dst(noise, r(6, 'xyzw', dst=True), r(5, 'xyxx'), identity=True),
            op(ADD, r(6, 'xy', dst=True), r(6, 'ywyy'), imm(-0.5)),
            op(ADD, r(3, 'x', dst=True), r(6, 'x', abs_=True), r(6, 'y', abs_=True)),
            op(MAD, r(5, 'xy', dst=True), r(5, 'xyxx'), imm(FOAM_SECOND[0]), imm(*FOAM_SECOND[1])),
            with_dst(noise, r(6, 'xyzw', dst=True), r(5, 'xyxx'), identity=True),
            op(ADD, r(6, 'xy', dst=True), r(6, 'ywyy'), imm(-0.5)),
            op(ADD, r(6, 'x', dst=True), r(6, 'x', abs_=True), r(6, 'y', abs_=True)),
            op(ADD, r(3, 'x', dst=True), r(3, 'x'), r(6, 'x')),
            op(MUL, r(into[0], into[1], dst=True), r(3, 'x'), imm(0.5)),
        ]

    out = []
    for k, ins in enumerate(body):
        code = ins[0] & 0x7ff
        if code == DCL_TEMPS:
            out.append([ins[0], temps + len(N)])
            continue
        if code == DCL_INPUT_PS and (ins[1] >> 12) & 0xff == T_INPUT and ins[2] == 4:
            v4 = list(ins)
            v4[1] = (v4[1] & ~(0xf << 4)) | (0xf << 4)
            v7 = list(ins)
            v7[1] = (v7[1] & ~(0xf << 4)) | (0x3 << 4)
            v7[2] = 7
            out += [v4, v7]
            continue

        if k == s0:
            # Kept: the third layer's uv and the screen's (N0), the first two layers' (N1). N2 = (river of the editor's, the flow's
            # speed past vanilla's, layer 0's offset, layer 1's); N4.w = layer 0's weight.
            out += [
                op(MOV, r(0, 'xy', dst=True), reg('r', 0, 'xyxx')),
                op(MOV, r(0, 'zw', dst=True), reg('r', 0, 'xxzw')),
                op(MOV, r(1, 'xyzw', dst=True), reg('r', 2, 'xyzw')),
                op(ADD, r(2, 'x', dst=True), reg('v', 4, 'z'), imm(-1.0), sat=True),
                op(MAD, r(2, 'y', dst=True), reg('v', 7, 'y'), imm(2.0), imm(-1.0)),
                op(MUL, r(2, 'y', dst=True), r(2, 'y'), r(2, 'x')),
                op(MUL, r(5, 'x', dst=True), time, imm(1.0 / FLOW_CYCLE_S)),
                op(FRC, r(5, 'x', dst=True), r(5, 'x')),
                op(ADD, r(5, 'y', dst=True), r(5, 'x'), imm(0.5)),
                op(FRC, r(5, 'y', dst=True), r(5, 'y')),
                op(MAD, r(4, 'w', dst=True), r(5, 'x'), imm(2.0), imm(-1.0)),
                op(ADD, r(4, 'w', dst=True), r(4, 'w', abs_=True, neg=True), imm(1.0)),
                op(ADD, r(5, 'xy', dst=True), r(5, 'xyxx'), imm(-0.5)),
                op(MUL, r(5, 'xy', dst=True), r(5, 'xyxx'), r(2, 'yyyy')),
                op(MUL, r(2, 'zw', dst=True), r(5, 'xxxy'), imm(VANILLA_SCROLL * FLOW_CYCLE_S)),
                # Layer 0: the uvs moved down the river by its offset.
                op(ADD, reg('r', 2, 'yw', dst=True), reg('r', 2, 'xyzw'), r(2, 'zzzz', neg=True)),
                op(ADD, reg('r', 0, 'y', dst=True), reg('r', 0, 'y'), r(2, 'z', neg=True)),
            ]
            out.append(ins)
            continue

        if k == e0:
            out.append(ins)
            # Layer 1: the block again from the kept uvs moved by its offset; the two normals weighed and normalised.
            out += [
                op(MOV, r(3, 'xyz', dst=True), reg('r', 1, 'xyzx')),
                op(MOV, reg('r', 0, 'xy', dst=True), r(0, 'xyxx')),
                op(MOV, reg('r', 2, 'xyzw', dst=True), r(1, 'xyzw')),
                op(ADD, reg('r', 2, 'yw', dst=True), reg('r', 2, 'xyzw'), r(2, 'wwww', neg=True)),
                op(ADD, reg('r', 0, 'y', dst=True), reg('r', 0, 'y'), r(2, 'w', neg=True)),
            ]
            out += [list(b) for b in block]
            out += [
                op(ADD, r(5, 'xyz', dst=True), r(3, 'xyzx'), reg('r', 1, 'xyzx', neg=True)),
                op(MAD, reg('r', 1, 'xyz', dst=True), r(5, 'xyzx'), r(4, 'wwww'), reg('r', 1, 'xyzx')),
                op(DP3, r(5, 'x', dst=True), reg('r', 1, 'xyzx'), reg('r', 1, 'xyzx')),
                op(RSQ, r(5, 'x', dst=True), r(5, 'x')),
                op(MUL, reg('r', 1, 'xyz', dst=True), reg('r', 1, 'xyzx'), r(5, 'xxxx')),
                op(MOV, reg('r', 0, 'zw', dst=True), r(0, 'xxzw')),
            ]
            continue

        if k == depth + 1:
            out.append(ins)
            out.append(op(MOV, r(4, 'x', dst=True), reg('r', int(dreg), dcomp)))
            continue

        if k == tail - 1:
            # Foam over the colour (before the fog): f = the foam carried (0 on others); where the ripples of both layers are steep;
            # lit by the sun, in the HDR scale.
            out += [
                op(ADD, r(4, 'y', dst=True), reg('v', 4, 'z'), imm(-2.0), sat=True),
            ]
            out += steepness('z', (4, 'z'))
            out += steepness('w', (6, 'x'))
            out += [
                op(ADD, r(4, 'z', dst=True), r(4, 'z'), r(6, 'x', neg=True)),
                op(MAD, r(4, 'z', dst=True), r(4, 'z'), r(4, 'w'), r(6, 'x')),
                op(MAD, r(5, 'x', dst=True), r(4, 'y'), imm(FOAM_FROM[1] - FOAM_FROM[0]), imm(FOAM_FROM[0])),
                op(ADD, r(4, 'z', dst=True), r(4, 'z'), r(5, 'x', neg=True)),
                op(MUL, r(4, 'z', dst=True), r(4, 'z'), imm(FOAM_EDGE), sat=True),
                op(MUL, r(5, 'x', dst=True), r(4, 'y'), imm(3.0), sat=True),
                op(MUL, r(4, 'z', dst=True), r(4, 'z'), r(5, 'x')),
                op(MUL, r(4, 'z', dst=True), r(4, 'z'), imm(FOAM_OPACITY)),
                op(LT, r(5, 'x', dst=True), imm(0.0), cb(light, 11, 'x')),
                op(MOVC, r(5, 'x', dst=True), r(5, 'x'), imm(0.004), imm(1.0)),
                op(MAX, r(5, 'y', dst=True), cb(light, 0, 'z', neg=True), imm(0.0)),
                op(MAD, r(5, 'y', dst=True), r(5, 'y'), imm(FOAM_LIGHT[1]), imm(FOAM_LIGHT[0])),
                op(MUL, r(5, 'x', dst=True), r(5, 'x'), r(5, 'y')),
                op(DP3, r(5, 'y', dst=True), cb(light, 1, 'xyzx'), imm(1.0 / 6.0, 1.0 / 6.0, 1.0 / 6.0, 0.0)),
                op(MAD, r(6, 'xyz', dst=True), cb(light, 1, 'xyzx'), imm(0.5), r(5, 'yyyy')),
                op(MUL, r(6, 'xyz', dst=True), r(6, 'xyzx'), r(5, 'xxxx')),
                op(ADD, r(6, 'xyz', dst=True), r(6, 'xyzx'), reg('r', 0, 'yzwy', neg=True)),
                op(MAD, reg('r', 0, 'yzw', dst=True), r(6, 'xxyz'), r(4, 'zzzz'), reg('r', 0, 'yyzw')),
            ]
            out.append(ins)
            continue

        if k == tail:
            head, ops = split(ins)
            ops = list(ops)
            ops[0] = r(7, 'xyz', dst=True)
            out.append(join(head, ops))
            continue

        if code == RET:
            # Alpha fade (a river of the editor's): towards the scene behind the water, undistorted.
            out += [
                op(ADD, r(5, 'x', dst=True), reg('v', 7, 'x'), imm(-1.0)),
                op(MAD, r(5, 'x', dst=True), r(5, 'x'), r(2, 'x'), imm(1.0), sat=True),
                with_dst(refraction, r(6, 'xyzw', dst=True), r(0, 'zwzz'), lod=imm1(0.0), identity=True),
                op(ADD, r(7, 'xyz', dst=True), r(7, 'xyzx'), r(6, 'xyzx', neg=True)),
                op(MAD, reg('o', 3, 'xyz', dst=True), r(7, 'xyzx'), r(5, 'xxxx'), r(6, 'xyzx')),
            ]

        out.append(ins)

    flat = [t for i in out for t in i]
    return toks[:1] + [len(flat) + 2] + flat


def patch_fxc(data, work):
    at = data.find(b'DXBC')
    head, dxbc = data[:at], data[at:]
    n = struct.unpack_from('<I', dxbc, 28)[0]
    chunks = []
    for o in struct.unpack_from(f'<{n}I', dxbc, 32):
        size = struct.unpack_from('<I', dxbc, o + 4)[0]
        chunks.append([dxbc[o:o + 4], bytearray(dxbc[o + 8:o + 8 + size])])

    src = os.path.join(work, 'in.dxbc')
    open(src, 'wb').write(dxbc)
    asm = tool('vkd3d-compiler', '-x', 'dxbc-tpf', '-b', 'd3d-asm', src).decode()
    for c in chunks:
        if c[0] == b'ISGN':
            # TEXCOORD4.zw and TEXCOORD7.xy read now.
            d = c[1]
            for k in range(struct.unpack_from('<I', d, 0)[0]):
                e = 8 + k * 24
                r = struct.unpack_from('<I', d, e + 16)[0]
                if r in (4, 7):
                    d[e + 21] = d[e + 20]
        elif c[0] in (b'SHDR', b'SHEX'):
            toks = list(struct.unpack_from(f'<{len(c[1]) // 4}I', c[1], 0))
            new = patch_shader(toks, asm)
            c[1] = bytearray(struct.pack(f'<{len(new)}I', *new))

    hdr = 32 + 4 * len(chunks)
    body, offs = bytearray(), []
    for tag, d in chunks:
        offs.append(hdr + len(body))
        body += tag + struct.pack('<I', len(d)) + d
    blob = b'DXBC' + b'\0' * 16 + struct.pack('<III', 1, hdr + len(body), len(chunks)) + struct.pack(f'<{len(chunks)}I', *offs) + body
    raw, fixed = os.path.join(work, 'raw.dxbc'), os.path.join(work, 'fixed.dxbc')
    open(raw, 'wb').write(blob)
    tool('vkd3d-dxbc', '--ignore-checksum', '-o', fixed, '--emit', raw)
    tool('vkd3d-compiler', '-x', 'dxbc-tpf', '-b', 'd3d-asm', fixed)  # it reads back
    return head + open(fixed, 'rb').read()


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    src, dst = sys.argv[1], sys.argv[2]
    names = sorted(n for root, _, files in os.walk(src) for n in files
                   if re.match(r'^dx11.*rigidriver\.hlsl_ps_ps30_main_battle_river\.fxc$', n))
    if len(names) != 10:
        sys.exit(f'expected the 10 dx11 battle river pixel shaders in {src}, found {len(names)}')
    os.makedirs(dst, exist_ok=True)
    with tempfile.TemporaryDirectory() as work:
        for name in names:
            path = next(os.path.join(root, name) for root, _, files in os.walk(src) if name in files)
            out = patch_fxc(open(path, 'rb').read(), work)
            open(os.path.join(dst, name), 'wb').write(out)
            print(name)


if __name__ == '__main__':
    main()
