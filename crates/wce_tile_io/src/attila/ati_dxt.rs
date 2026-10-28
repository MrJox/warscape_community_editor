// Portions translated from AMD Compressonator (cmp_compressonatorlib/ati/compressonatorxcodec.cpp, dxtc/codec_dxtc_*.cpp,
// buffer/codecbuffer.h), under this notice:
//
// Copyright (c) 2007-2024  Advanced Micro Devices, Inc. All rights reserved.
// Copyright (c) 2004-2006 ATI Technologies Inc.
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files(the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and / or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions :
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.

//! ATI's DXT5 compressor as BOB runs it (`TOOLDATABUILDER::write_dds_ATI_compressor` -> `ATI_TC_ConvertTexture`, statically linked in
//! ToolDataBuilderDll): the "normal" speed path with zeroed options (no channel weighting, no 3D refinement, one refinement step). A port
//! of AMD Compressonator's legacy codec (`cmp_compressonatorlib/ati/compressonatorxcodec.cpp`, `dxtc/codec_dxtc_*.cpp`, MIT licence),
//! the successor of ATI_Compress. docs/formats/tile_compile.md, stage B.
//!
//! The colour part's arithmetic is the x87's under MSVC's `/fp:precise`: an expression is evaluated in double precision and rounded to
//! `float` where it is stored (a variable, an array element, a function argument); comparisons see the unrounded value. Here the
//! variables are f32 and the expressions f64, rounded by [`s`] at each store. The alpha part ran through the library's SSE2 code: f32.
//!
//! Pixels are BGRA bytes (the codec's dword: B in the low byte, A in the high one).

// The port keeps the C code's indexed loops, to be read side by side with it.
#![allow(clippy::needless_range_loop)]

const MAX_BLOCK: usize = 64;
const MAX_POINTS: usize = 16;
const MAX_ERROR: f32 = 128000.0;
const GBL_SCH_STEP: f32 = 0.018;
const GBL_SCH_EXT: f32 = 0.1;
const LCL_SCH_STEP: f32 = 0.6;
const SCH_STPS: usize = 3;
const S_MV_F: [f32; 17] = [0.0, -1.0, 1.0, -2.0, 2.0, -3.0, 3.0, -4.0, 4.0, -5.0, 5.0, -6.0, 6.0, -7.0, 7.0, -8.0, 8.0];
const PIX_GRID: u32 = 8;
const RC: usize = 2;
const GC: usize = 1;
const BC: usize = 0;
const BITS: [u32; 3] = [5, 6, 5]; // B, G, R (indexed by BC, GC, RC)
const RND_AMOUNT: [f64; 9] = [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0];

type Ramp2 = [[f32; 2]; 4];

/// A store to a `float`.
fn s(x: f64) -> f32 {
    x as f32
}

fn d(x: f32) -> f64 {
    f64::from(x)
}

fn mk_rmp_on_grid(mn_mx: &Ramp2, min: f32, max: f32) -> Ramp2 {
    let mut out = [[0.0f32; 2]; 4];
    for j in 0..3 {
        let f1 = (1u32 << BITS[j]) as f32;
        let f0 = (1u32 << (PIX_GRID - BITS[j])) as f32;
        for k in 0..2 {
            let mut v = s(d(mn_mx[j][k]).floor());
            if v <= min {
                v = min;
            } else {
                v = s(d(v) + (128.0 / d(f1)).floor() - (d(v) / d(f1)).floor());
                v = v.min(max);
            }

            out[j][k] = s((d(v) / d(f0)).floor() * d(f0));
        }
    }

    out
}

/// The endpoints as the decompressor sees them (low bits replicated) and whether the ramp is flat.
fn mk_wk_rmp_pts(inp: &Ramp2) -> (bool, Ramp2) {
    let mut eq = true;
    for row in inp.iter().take(3) {
        eq &= row[0] == row[1];
    }

    let mut out = [[0.0f32; 2]; 4];
    for j in 0..3 {
        let f = (1u32 << BITS[j]) as f32;
        for k in 0..2 {
            let v = s(d(inp[j][k]) + (d(inp[j][k]) / d(f)).floor());
            out[j][k] = v.clamp(0.0, 255.0);
        }
    }

    (eq, out)
}

fn bld_clr_rmp(rmp: &mut [f32; MAX_POINTS], inp: [f32; 2], n: usize) {
    rmp[0] = inp[0];
    rmp[n - 1] = inp[1];
    if n % 2 == 1 {
        rmp[n] = 1_000_000.0;
    }

    for e in 1..n - 1 {
        rmp[e] = s(((d(rmp[0]) * (n - 1 - e) as f64 + d(rmp[n - 1]) * e as f64 + RND_AMOUNT[n]) / d((n - 1) as f32)).floor());
    }
}

fn bld_rmp(rmp: &mut [[f32; MAX_POINTS]; 4], inp: &Ramp2, n: usize) {
    for j in 0..3 {
        bld_clr_rmp(&mut rmp[j], inp[j], n);
    }
}

fn dist3(blk: &[f32; 4], rmp: &[[f32; MAX_POINTS]; 4], r: usize) -> f64 {
    let c = |ch: usize| d(blk[ch]) - d(rmp[ch][r]);
    c(RC) * c(RC) + c(GC) * c(GC) + c(BC) * c(BC)
}

fn clstr_err(blk: &[[f32; 4]], rpt: &[f32], rmp: &[[f32; MAX_POINTS]; 4], n_clrs: usize, blcktp: usize, const_ramp: bool) -> f32 {
    let rmp_l = if const_ramp { 1 } else { blcktp };
    let mut err = 0.0f32;
    for i in 0..n_clrs {
        let mut shortest = 99_999_999_999.0f32;
        for r in 0..rmp_l {
            let dist = s(dist3(&blk[i], rmp, r));
            if dist < shortest {
                shortest = dist;
            }
        }

        err = s(d(err) + d(shortest) * d(rpt[i]));
    }

    err
}

/// The indices of the 16 pixels for the endpoints (DXTC order) and the error.
fn clstr_intnl(blk: &[[f32; 4]], idx: &mut [u8; 16], rmp: &[[f32; MAX_POINTS]; 4], n: usize, const_ramp: bool) -> f32 {
    let rmp_l = if const_ramp { 1 } else { n };
    let mut err = 0.0f32;
    for i in 0..16 {
        let mut shortest = 99_999_999_999.0f32;
        let mut si = 0usize;
        for r in 0..rmp_l {
            let dist = s(dist3(&blk[i], rmp, r));
            if dist < shortest {
                shortest = dist;
                si = r;
            }
        }

        err = s(d(err) + d(shortest));
        idx[i] = if si == n - 1 {
            1
        } else if si != 0 {
            (si + 1) as u8
        } else {
            0
        };
    }

    err
}

fn construct_color(r: u8, g: u8, b: u8) -> u32 {
    ((u32::from(r) & 0xf8) << 8) | ((u32::from(g) & 0xfc) << 3) | ((u32::from(b) & 0xf8) >> 3)
}

/// `Clstr` (the DWORD block): the indices for the integer endpoints, the way the decompressor sees them.
fn clstr(block: &[u32; 16], ends: &[[u8; 2]; 3], idx: &mut [u8; 16], n: usize) -> f32 {
    let c0 = construct_color(ends[RC][0], ends[GC][0], ends[BC][0]);
    let c1 = construct_color(ends[RC][1], ends[GC][1], ends[BC][1]);
    let (i0, i1) = if (n & 1 == 0 && c0 <= c1) || (n & 1 == 1 && c0 > c1) { (1, 0) } else { (0, 1) };
    let mut inp = [[0.0f32; 2]; 4];
    for c in [RC, GC, BC] {
        inp[c] = [f32::from(ends[c][i0]), f32::from(ends[c][i1])];
    }

    let mut blk = [[0.0f32; 4]; MAX_BLOCK];
    for i in 0..16 {
        blk[i][RC] = ((block[i] >> 16) & 0xff) as f32;
        blk[i][GC] = ((block[i] >> 8) & 0xff) as f32;
        blk[i][BC] = (block[i] & 0xff) as f32;
    }

    let (eq, wk) = mk_wk_rmp_pts(&inp);
    let mut rmp = [[0.0f32; MAX_POINTS]; 4];
    bld_rmp(&mut rmp, &wk, n);
    clstr_intnl(&blk, idx, &rmp, n, eq)
}

/// The colour ramp search's error (ToolDataBuilderDll `FUN_101e54a0`, the library's SSE2 `RampSrchW`): like [`rmp_srch1`], with the
/// projection's own error added per value: lane += (count x d) x d + projection error; no early exit.
fn ramp_srch_w(prj: &[f32], prj_err: &[f32], pre_m_rep: &[f32], low: f32, high: f32, n_unique: usize, n: usize) -> f32 {
    let step = s((d(high) - d(low)) / d((n - 1) as f32));
    let rstep = s(1.0 / d(step));
    let mut lanes = [0.0f32; 4];
    for k in (0..n_unique.div_ceil(4) * 4).step_by(4) {
        for (l, lane) in lanes.iter_mut().enumerate() {
            let x = prj[k + l];
            let q = ((x - low) * rstep).round_ties_even();
            let v = (q * step + low).max(low).min(high);
            let dd = x - v;
            *lane = *lane + pre_m_rep[k + l] * dd * dd + prj_err[k + l];
        }
    }

    (lanes[0] + lanes[2]) + (lanes[1] + lanes[3])
}

/// The first approximation of the axis (`FindAxis`): the block shifted to its centre, the direction, the centre, whether it is small.
fn find_axis(blk_uv: &[[f32; 4]], rpt: &[f32], n_unique: usize) -> ([[f32; 4]; MAX_BLOCK], [f32; 4], [f32; 4], bool) {
    let mut blk_sh = [[0.0f32; 4]; MAX_BLOCK];
    let mut line = [0.0f32; 4];
    let mut centre = [0.0f32; 4];
    let mut rgb2 = [0.0f32; 4];
    let mut crrl = [0.0f32; 4];
    let mut n_points = 0.0f32;
    for i in 0..n_unique {
        for c in 0..3 {
            centre[c] = s(d(centre[c]) + d(blk_uv[i][c]) * d(rpt[i]));
        }

        n_points = s(d(n_points) + d(rpt[i]));
    }

    for c in centre.iter_mut().take(3) {
        *c = s(d(*c) / d(n_points));
    }

    for i in 0..n_unique {
        for c in 0..3 {
            blk_sh[i][c] = s(d(blk_uv[i][c]) - d(centre[c]));
        }

        for j in 0..3 {
            rgb2[j] = s(d(rgb2[j]) + d(blk_sh[i][j]) * d(blk_sh[i][j]) * d(rpt[i]));
            crrl[j] = s(d(crrl[j]) + d(blk_sh[i][j]) * d(blk_sh[i][(j + 1) % 3]) * d(rpt[i]));
        }
    }

    let (mut i0, mut mx) = (0usize, 0.0f32);
    let mut k = 0;
    // EPS and EPS2 are macros of float constants: (2.f / 255.f) * (2.f / 255.f), 3.f * (2.f / 255.f) * (2.f / 255.f).
    let c = d(2.0f32 / 255.0f32);
    let f_eps = s(d(n_points) * c * c);
    for j in 0..3 {
        if rgb2[j] >= f_eps {
            k += 1;
        } else {
            rgb2[j] = 0.0;
        }

        if mx < rgb2[j] {
            mx = rgb2[j];
            i0 = j;
        }
    }

    let f_eps2 = s(d(n_points) * 3.0 * c * c);
    let small = (0..3).all(|j| rgb2[j] < f_eps2);
    if small {
        return (blk_sh, line, centre, true);
    }

    if k == 1 {
        line[i0] = 1.0;
    } else if k == 2 {
        let i1 = if rgb2[(i0 + 1) % 3] > 0.0 { (i0 + 1) % 3 } else { (i0 + 2) % 3 };
        let crl = if i1 == (i0 + 1) % 3 { crrl[i0] } else { crrl[(i0 + 2) % 3] };
        line[i1] = s(d(crl) / d(rgb2[i0]));
        line[i0] = 1.0;
    } else {
        let mut max_det = 100_000.0f32;
        for j in 0..3 {
            let det = s(d(rgb2[j]) * d(rgb2[(j + 1) % 3]) - d(crrl[j]) * d(crrl[j]));
            if max_det < det {
                max_det = det;
                i0 = j;
            }
        }

        let vc1 = [crrl[(i0 + 2) % 3], crrl[(i0 + 1) % 3]];
        let m00 = rgb2[(i0 + 1) % 3];
        let m11 = rgb2[i0];
        let m01 = s(-d(crrl[i0]));
        // One x87 expression (the 2008 library's): (m01 vc1[0] + vc1[1] m11) / det + (vc1[1] m01 + vc1[0] m00) / det.
        let vc0 = d(vc1[1]) * d(m01) + d(vc1[0]) * d(m00);
        let vc1b = d(m01) * d(vc1[0]) + d(vc1[1]) * d(m11);
        line[i0] = 1.0;
        line[(i0 + 1) % 3] = 1.0;
        line[(i0 + 2) % 3] = s(vc1b / d(max_det) + vc0 / d(max_det));
    }

    let mut len = s(d(line[0]) * d(line[0]) + d(line[1]) * d(line[1]) + d(line[2]) * d(line[2]));
    len = s(d(len).sqrt());
    for c in line.iter_mut().take(3) {
        *c = if len > 0.0 { s(d(*c) / d(len)) } else { 0.0 };
    }

    (blk_sh, line, centre, false)
}

/// `Refine`: each channel's endpoints moved by a grid step at most, the best total error kept.
fn refine(inp_pnts: &Ramp2, blk: &[[f32; 4]], rpt: &[f32], n_clrs: usize, n: usize, steps: u8) -> Ramp2 {
    let steps = i32::from(steps.clamp(1, 8));
    let fctrs = [(1u32 << (PIX_GRID - BITS[0])) as f32, (1u32 << (PIX_GRID - BITS[1])) as f32, (1u32 << (PIX_GRID - BITS[2])) as f32];
    let inp0 = *inp_pnts;
    let mut inp = *inp_pnts;
    let mut rmp = [[0.0f32; MAX_POINTS]; 4];
    let (eq, wk) = mk_wk_rmp_pts(&inp);
    bld_rmp(&mut rmp, &wk, n);
    let mut best_e = clstr_err(blk, rpt, &rmp, n_clrs, n, eq);
    if best_e == 0.0 {
        return inp;
    }

    let mut rmp_err = [[0.0f32; MAX_BLOCK]; MAX_POINTS];
    // Red, green, blue in turn, the other two channels' errors precomputed.
    for (ch, others) in [(RC, [GC, BC]), (GC, [RC, BC]), (BC, [RC, GC])] {
        if ch != RC {
            let (_, wk) = mk_wk_rmp_pts(&inp);
            bld_rmp(&mut rmp, &wk, n);
        }

        for i in 0..n_clrs {
            for r in 0..n {
                let d0 = s(d(rmp[others[0]][r]) - d(blk[i][others[0]]));
                let d1 = s(d(rmp[others[1]][r]) - d(blk[i][others[1]]));
                rmp_err[r][i] = s(d(d0) * d(d0) * 1.0 + d(d1) * d(d1) * 1.0);
            }
        }

        let (mut c0, mut c1) = (inp0[ch][0], inp0[ch][1]);
        for i in -steps..=steps {
            for j in -steps..=steps {
                inp[ch][0] = s(d(inp0[ch][0]) + f64::from(i) * d(fctrs[ch])).clamp(0.0, 255.0);
                inp[ch][1] = s(d(inp0[ch][1]) + f64::from(j) * d(fctrs[ch])).clamp(0.0, 255.0);
                let (eq, wk) = mk_wk_rmp_pts(&inp);
                bld_clr_rmp(&mut rmp[ch], wk[ch], n);
                let rmp_l = if eq { 1 } else { n };
                let mut mse = 0.0f32;
                for k in 0..n_clrs {
                    let mut min_err = 10_000_000.0f32;
                    for r in 0..rmp_l {
                        let dist = s(d(rmp[ch][r]) - d(blk[k][ch]));
                        let err = s(d(rmp_err[r][k]) + d(dist) * d(dist) * 1.0);
                        min_err = min_err.min(err);
                    }

                    mse = s(d(mse) + d(min_err) * d(rpt[k]));
                }

                if mse < best_e {
                    c0 = inp[ch][0];
                    c1 = inp[ch][1];
                    best_e = mse;
                }
            }
        }

        inp[ch][0] = c0;
        inp[ch][1] = c1;
    }

    inp
}

/// `CompressRGBBlockX`: the endpoints (float, on the 5:6:5 grid) of the unique colours.
fn compress_rgb_block_x(src: &[[f32; 4]], rpt: &[f32], n_unique: usize, n: usize) -> Ramp2 {
    let mut blk_uv = [[0.0f32; 4]; MAX_BLOCK];
    for i in 0..n_unique {
        for j in 0..3 {
            blk_uv[i][j] = s(d(src[i][j]) / 255.0);
        }
    }

    let mut rslt = [[0.0f32; 2]; 4];
    let mut done = false;
    if n_unique <= 2 {
        for j in 0..3 {
            rslt[j] = [src[0][j], src[n_unique - 1][j]];
        }

        done = true;
    }

    let (blk_sh, line0, mdl, small) = if done { ([[0.0f32; 4]; MAX_BLOCK], [0.0; 4], [0.0; 4], false) } else { find_axis(&blk_uv, rpt, n_unique) };
    if !done && small {
        for j in 0..3 {
            rslt[j] = [src[0][j], src[n_unique - 1][j]];
        }

        done = true;
    }

    if !done {
        let mut err_g = 10_000_000.0f32;
        let mut line = line0;
        let mut line_g = [0.0f32; 4];
        let mut pos_g = [0.0f32; 2];
        let mut prj0 = [0.0f32; MAX_BLOCK];
        let mut prj = [0.0f32; MAX_BLOCK];
        let mut prj_err = [0.0f32; MAX_BLOCK];
        let mut pre_m_rep = [0.0f32; MAX_BLOCK];
        let mut rmp_indxs = [0.0f32; MAX_BLOCK];
        loop {
            let mut bnd = [1000.0f32, -1000.0f32];
            prj0.fill(0.0);
            prj.fill(0.0);
            prj_err.fill(0.0);
            pre_m_rep.fill(0.0);
            for i in 0..n_unique {
                let p = s(d(blk_sh[i][0]) * d(line[0]) + d(blk_sh[i][1]) * d(line[1]) + d(blk_sh[i][2]) * d(line[2]));
                prj0[i] = p;
                prj[i] = p;
                let e = |c: usize| d(blk_sh[i][c]) - d(line[c]) * d(p);
                prj_err[i] = s(e(0) * e(0) + e(1) * e(1) + e(2) * e(2));
                bnd[0] = bnd[0].min(p);
                bnd[1] = bnd[1].max(p);
            }

            let scl = [s(d(bnd[0]) - (d(bnd[1]) - d(bnd[0])) * 0.125), s(d(bnd[1]) + (d(bnd[1]) - d(bnd[0])) * 0.125)];
            let scl2 = s((d(scl[1]) - d(scl[0])) * (d(scl[1]) - d(scl[0])));
            let over = s(1.0 / (d(scl[1]) - d(scl[0])));
            for i in 0..n_unique {
                prj[i] = s((d(prj[i]) - d(scl[0])) * d(over));
                pre_m_rep[i] = s(d(rpt[i]) * d(scl2));
            }

            for b in &mut bnd {
                *b = s((d(*b) - d(scl[0])) * d(over));
            }

            let mut step_err = MAX_ERROR;
            const SEARCH_STEP: f32 = 0.025;
            let low_start = if d(bnd[0]) - 2.0 * d(SEARCH_STEP) > 0.0 { s(d(bnd[0]) - 2.0 * d(SEARCH_STEP)) } else { 0.0 };
            let high_start = if d(bnd[1]) + 2.0 * d(SEARCH_STEP) < 1.0 { s(d(bnd[1]) + 2.0 * d(SEARCH_STEP)) } else { 1.0 };
            let mut pos = [0.0f32; 2];
            let mut low = low_start;
            for _ in 0..8 {
                let mut high = high_start;
                for _ in 0..8 {
                    let err = ramp_srch_w(&prj, &prj_err, &pre_m_rep, low, high, n_unique, n);
                    if err < step_err {
                        step_err = err;
                        pos = [low, high];
                    }

                    high = s(d(high) - d(SEARCH_STEP));
                }

                low = s(d(low) + d(SEARCH_STEP));
            }

            for p in &mut pos {
                *p = s(d(*p) * (d(scl[1]) - d(scl[0])) + d(scl[0]));
            }

            if d(step_err) + 0.001 < d(err_g) {
                err_g = step_err;
                line_g = line;
                pos_g = pos;
                // The index vector (SSE): the nearest ramp point (ties to even), clamped, shifted and normalised.
                let nm1 = (n - 1) as f32;
                let rstep = s(1.0 / ((d(pos[1]) - d(pos[0])) / d(nm1)));
                let over_blk_tp = s(1.0 / d(nm1));
                let indx_avrg = s(d(nm1) * 0.5);
                for i in 0..n_unique {
                    let q = ((prj0[i] - pos[0]) * rstep).round_ties_even().max(0.0).min(nm1);
                    rmp_indxs[i] = (q - indx_avrg) * over_blk_tp;
                }

                let (mut crs, mut len) = ([0.0f32; 3], 0.0f32);
                for i in 0..n_unique {
                    let pre = s(d(rmp_indxs[i]) * d(rpt[i]));
                    len = s(d(len) + d(rmp_indxs[i]) * d(pre));
                    for j in 0..3 {
                        crs[j] = s(d(crs[j]) + d(blk_sh[i][j]) * d(pre));
                    }
                }

                line = [0.0; 4];
                if len > 0.0 {
                    for j in 0..3 {
                        line[j] = s(d(crs[j]) / d(len));
                    }

                    let mut len2 = s(d(line[0]) * d(line[0]) + d(line[1]) * d(line[1]) + d(line[2]) * d(line[2]));
                    len2 = s(d(len2).sqrt());
                    for c in line.iter_mut().take(3) {
                        *c = s(d(*c) / d(len2));
                    }
                }
            } else {
                break;
            }
        }

        for k in 0..2 {
            for j in 0..3 {
                rslt[j][k] = s((d(pos_g[k]) * d(line_g[j]) + d(mdl[j])) * 255.0);
            }
        }
    }

    let grid = mk_rmp_on_grid(&rslt, 0.0, 255.0);
    refine(&grid, src, rpt, n_unique, n, 1)
}

/// `CompRGBBlock` (DWORD pixels, four colours) and the packing of `CCodec_DXTC::CompressRGBBlock`: the colour half of a DXT5 block.
fn compress_rgb_block(block: &[u32; 16]) -> [u32; 2] {
    let n = 4;
    // `qsort` with `QSortIntCmp` (the dwords as ints; the alpha byte forced to 0xff).
    let mut blk: Vec<u32> = block.iter().map(|&c| c | 0xff00_0000).collect();
    blk.sort_unstable_by_key(|&c| c as i32);
    let mut unique = vec![blk[0]];
    let mut rpt = vec![1.0f32];
    for &c in &blk[1..] {
        if *unique.last().expect("one") != c {
            unique.push(c);
            rpt.push(1.0);
        } else {
            *rpt.last_mut().expect("one") += 1.0;
        }
    }

    let mut blk_in = [[0.0f32; 4]; MAX_BLOCK];
    for (i, &c) in unique.iter().enumerate() {
        blk_in[i][RC] = ((c >> 16) & 0xff) as f32;
        blk_in[i][GC] = ((c >> 8) & 0xff) as f32;
        blk_in[i][BC] = (c & 0xff) as f32;
        blk_in[i][3] = 255.0;
    }

    let mut rpt_full = [0.0f32; MAX_BLOCK];
    rpt_full[..rpt.len()].copy_from_slice(&rpt);
    let rslt = compress_rgb_block_x(&blk_in, &rpt_full, unique.len(), n);
    let mut ends = [[0u8; 2]; 3];
    for i in 0..3 {
        for j in 0..2 {
            ends[i][j] = rslt[i][j] as i32 as u8;
        }
    }

    let mut idx = [0u8; 16];
    clstr(block, &ends, &mut idx, n);
    let col = |k: usize| (u32::from(ends[RC][k] >> 3) << 11) | (u32::from(ends[GC][k] >> 2) << 5) | u32::from(ends[BC][k] >> 3);
    let (c0, c1) = (col(0), col(1));
    let w0 = if c0 <= c1 { c1 | (c0 << 16) } else { c0 | (c1 << 16) };
    let mut w1 = 0u32;
    for (i, &v) in idx.iter().enumerate() {
        w1 |= u32::from(v) << (2 * i);
    }

    [w0, w1]
}

// ---- the scalar (alpha) case
//
// ATI's library ran this part with `bUseSSE2` (its SSE2 ramp search, [`rmp_srch1`]); the rest of it is single operations on floats,
// where the x87's stored results are the f32 ones.

/// The ramp search's error (ToolDataBuilderDll `FUN_101e5940`, the library's SSE2 `RmpSrch1`, which BOB's compressor runs): the step
/// and its inverse on the x87 (double, stored as float); per value the nearest ramp point (`cvtps2dq`, ties to even) clamped to the
/// ends; the errors (count x d) x d summed in four lanes over the values padded to a multiple of 4, then (l0 + l2) + (l1 + l3); no
/// early exit (the bound is not used).
fn rmp_srch1(blk: &[f32], rpt: &[f32], _max_error: f32, min_ex: f32, max_ex: f32, n_clrs: usize, n: usize) -> f32 {
    let step = s((d(max_ex) - d(min_ex)) / d((n - 1) as f32));
    let rstep = s(1.0 / d(step));
    let mut lanes = [0.0f32; 4];
    for k in (0..n_clrs.div_ceil(4) * 4).step_by(4) {
        for (l, lane) in lanes.iter_mut().enumerate() {
            let (x, r) = (blk[k + l], rpt[k + l]);
            let q = ((x - min_ex) * rstep).round_ties_even();
            let v = (q * step + min_ex).max(min_ex).min(max_ex);
            let dd = x - v;
            *lane += r * dd * dd;
        }
    }

    (lanes[0] + lanes[2]) + (lanes[1] + lanes[3])
}

#[allow(clippy::too_many_arguments)]
fn refine1(blk: &[f32], rpt: &[f32], max_error: f32, min_ex: &mut f32, max_ex: &mut f32, m_step: f32, min_bnd: f32, max_bnd: f32, n_clrs: usize, n: usize) -> f32 {
    let mut maxerror = max_error;
    let (mut lo, mut hi) = (*min_ex, *max_ex);
    loop {
        let (mut best, mut cr0, mut cr1) = (None, lo, hi);
        for mode in 0..SCH_STPS * SCH_STPS {
            let cr_min = (lo + m_step * S_MV_F[mode / SCH_STPS]).max(min_bnd);
            let cr_max = (hi + m_step * S_MV_F[mode % SCH_STPS]).min(max_bnd);
            let error = rmp_srch1(blk, rpt, maxerror, cr_min, cr_max, n_clrs, n);
            if error < maxerror {
                maxerror = error;
                best = Some(mode);
                cr0 = cr_min;
                cr1 = cr_max;
            }
        }

        if best.is_none() {
            break;
        }

        lo = cr0;
        hi = cr1;
    }

    *min_ex = lo;
    *max_ex = hi;
    maxerror
}

/// `CompBlock1` (fixed ramp, integer grid): the two endpoints (0..255) of a scalar block in [0, 1].
fn comp_block1(blk: &[f32; 16], n: usize, fixed_ramp_points: bool) -> [f32; 2] {
    let int_fctr = 256.0f32;
    let mut uniq = [0.0f32; MAX_BLOCK];
    let mut reps = [0.0f32; MAX_BLOCK];
    let mut f = *blk;
    f.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut new_p = -2.0f32;
    let mut nu = 0usize;
    let mut ramp = [0.0f32; 2];
    let mut needs = true;
    let round = |v: f32| (v * (int_fctr - 1.0) + 0.5).floor();
    if fixed_ramp_points {
        for &v in &f {
            if new_p != v {
                new_p = v;
                if f64::from(new_p) <= 1.5 / 255.0 || f64::from(new_p) >= 253.5 / 255.0 {
                } else {
                    uniq[nu] = v;
                    reps[nu] = 1.0;
                    nu += 1;
                }
            } else if nu > 0 && uniq[nu - 1] == new_p {
                reps[nu - 1] += 1.0;
            }
        }

        if nu <= 2 {
            match nu {
                2 => ramp = [round(uniq[0]), round(uniq[1])],
                1 => {
                    ramp[0] = round(uniq[0]);
                    ramp[1] = ramp[0] + 1.0;
                }
                _ => ramp = [128.0, 129.0],
            }

            needs = false;
        }
    } else {
        for &v in &f {
            if new_p != v {
                new_p = v;
                uniq[nu] = v;
                reps[nu] = 1.0;
                nu += 1;
            } else {
                reps[nu - 1] += 1.0;
            }
        }

        if nu <= 2 {
            ramp[0] = round(uniq[0]);
            ramp[1] = if nu == 1 { ramp[0] + 1.0 } else { round(uniq[1]) };
            needs = false;
        }
    }

    if needs {
        let (min_bnd, max_bnd) = (0.0f32, 1.0f32);
        let (mut min_r, mut max_r) = (uniq[0], uniq[nu - 1]);
        let (mut gbl_l, mut gbl_r) = (0.0f32, 0.0f32);
        let cntr = (min_r + max_r) / 2.0;
        let mut gbl_err = MAX_ERROR;
        // The integer grid: no search when the interval is 48 / 256 or less.
        if max_r - min_r > 48.0 / int_fctr {
            let gbl_llb = if min_bnd > min_r - GBL_SCH_EXT { min_bnd } else { min_r - GBL_SCH_EXT };
            let gbl_rrb = if max_bnd < max_r + GBL_SCH_EXT { max_bnd } else { max_r + GBL_SCH_EXT };
            let gbl_lrb = if cntr < min_r + GBL_SCH_EXT { cntr } else { min_r + GBL_SCH_EXT };
            let gbl_rlb = if cntr > max_r - GBL_SCH_EXT { cntr } else { max_r - GBL_SCH_EXT };
            let mut step_l = gbl_llb;
            while step_l < gbl_lrb {
                let mut step_r = gbl_rrb;
                while gbl_rlb <= step_r {
                    let e = rmp_srch1(&uniq, &reps, gbl_err, step_l, step_r, nu, n);
                    if e < gbl_err {
                        gbl_err = e;
                        gbl_l = step_l;
                        gbl_r = step_r;
                    }

                    step_r -= GBL_SCH_STEP;
                }

                step_l += GBL_SCH_STEP;
            }

            min_r = gbl_l;
            max_r = gbl_r;
        }

        let m_step = LCL_SCH_STEP / int_fctr;
        refine1(&uniq, &reps, gbl_err, &mut min_r, &mut max_r, m_step, min_bnd, max_bnd, nu, n);
        let max_ex = max_r * (int_fctr - 1.0);
        let min_ex = min_r * (int_fctr - 1.0);
        ramp = [(min_ex + 0.5).floor(), (max_ex + 0.5).floor()];
    }

    if ramp[0] == ramp[1] {
        if ramp[1] < 255.0 {
            ramp[1] += 1.0;
        } else {
            ramp[1] -= 1.0;
        }
    }

    ramp
}

/// `Clstr1`: the indices of a scalar block for the endpoints and the error; the endpoints are put in the ramp's order in place
/// (`GetRmp1` swaps the caller's array).
fn clstr1(idx: &mut [u8; 16], blk: &[f32; 16], ramp: &mut [f32; 2], n: usize, fixed_ramp_points: bool) -> f32 {
    idx.fill(0);
    if ramp[0] == ramp[1] {
        return 0.0;
    }

    // GetRmp1 (integer precision 8, fraction 0, fixed ramp).
    if (!fixed_ramp_points && ramp[0] <= ramp[1]) || (fixed_ramp_points && ramp[0] > ramp[1]) {
        ramp.swap(0, 1);
    }

    let mut alpha = [100_000.0f32; MAX_POINTS];
    alpha[0] = ramp[0];
    alpha[1] = ramp[1];
    for e in 1..n - 1 {
        alpha[e + 1] = (alpha[0] * (n - 1 - e) as f32 + alpha[1] * e as f32) / (n - 1) as f32;
    }

    let mut np = n;
    if fixed_ramp_points {
        alpha[n] = 0.0;
        alpha[n + 1] = 255.0;
        np += 2;
    }

    for a in alpha.iter_mut().take(n) {
        *a = (*a + 0.5).floor();
    }

    let over = 1.0f32 / (256.0 - 1.0);
    for a in alpha.iter_mut().take(np) {
        *a *= over;
    }

    let mut err = 0.0f32;
    for i in 0..16 {
        let mut shortest = 10_000_000.0f32;
        for (j, &a) in alpha.iter().enumerate().take(np) {
            let mut dist = blk[i] - a;
            dist *= dist;
            if dist < shortest {
                shortest = dist;
                idx[i] = j as u8;
            }
        }

        err += shortest;
    }

    err
}

/// The alpha half of a DXT5 block (`CCodec_DXTC::CompressAlphaBlock`): an 8-point ramp, or a 6-point one with 0 and 255 when that is
/// better.
fn compress_alpha_block(alpha: &[u8; 16]) -> [u32; 2] {
    let f: [f32; 16] = alpha.map(|a| f32::from(a) / 255.0);
    let encode = |mut r: [f32; 2], fixed: bool| {
        let mut idx = [0u8; 16];
        let err = clstr1(&mut idx, &f, &mut r, if fixed { 6 } else { 8 }, fixed);
        // C's (BYTE) of a float: through int, wrapping (an all-255 block's ramp 255, 256 is written 0, 255).
        (err, [r[0] as i32 as u8, r[1] as i32 as u8], idx)
    };
    let (e8, ends8, idx8) = encode(comp_block1(&f, 8, false), false);
    let (ends, idx) = if e8 == 0.0 {
        (ends8, idx8)
    } else {
        let (e6, ends6, idx6) = encode(comp_block1(&f, 6, true), true);
        if e8 <= e6 { (ends8, idx8) } else { (ends6, idx6) }
    };
    let mut w = [u32::from(ends[0]) | (u32::from(ends[1]) << 8), 0];
    for (i, &v) in idx.iter().enumerate() {
        let v = u32::from(v);
        if i < 5 {
            w[0] |= (v & 7) << (16 + i * 3);
        } else if i > 5 {
            w[1] |= (v & 7) << (2 + (i - 6) * 3);
        } else {
            w[0] |= (v & 1) << 31;
            w[1] |= (v & 6) >> 1;
        }
    }

    w
}

/// A 4 x 4 block of the image (BGRA bytes), the codec's way: pixels past the edge repeat the block's own first ones (`PadLine`,
/// `PadBlock`).
fn read_block(bgra: &[u8], w: usize, h: usize, x: usize, y: usize) -> [u32; 16] {
    let mut b = [0u32; 16];
    let bw = 4.min(w - x);
    let bh = 4.min(h - y);
    for j in 0..bh {
        for i in 0..bw {
            let o = ((y + j) * w + x + i) * 4;
            b[j * 4 + i] = u32::from_le_bytes([bgra[o], bgra[o + 1], bgra[o + 2], bgra[o + 3]]);
        }

        pad(&mut b[j * 4..j * 4 + 4], bw, 4);
    }

    if bh < 4 {
        let mut rows = [[0u32; 4]; 4];
        for (j, r) in rows.iter_mut().enumerate() {
            r.copy_from_slice(&b[j * 4..j * 4 + 4]);
        }

        pad(&mut rows, bh, 4);
        for (j, r) in rows.iter().enumerate() {
            b[j * 4..j * 4 + 4].copy_from_slice(r);
        }
    }

    b
}

/// `PadLine` / `PadBlock`: entries `start..end` from the beginning, recursively halving.
fn pad<T: Copy>(v: &mut [T], start: usize, end: usize) {
    let (mut start, mut width) = (start, end - start);
    if width > start {
        pad(v, start, end >> 1);
        start = end >> 1;
        width = end - start;
    }

    for k in 0..width {
        v[start + k] = v[k];
    }
}

/// A BGRA image as DXT5 blocks (row by row, 16 bytes each: alpha, then colour). Each block depends on its own pixels only, so the rows of
/// blocks are compressed on the thread pool and joined in order, and a block met before in the row (a terrain map's are mostly the same
/// few: one texture's full weight) is taken from the row's cache.
pub fn compress_dxt5(bgra: &[u8], w: usize, h: usize) -> Vec<u8> {
    use rayon::prelude::*;
    let row = |by: usize| {
        let mut out = Vec::with_capacity(w.div_ceil(4) * 16);
        let mut seen: std::collections::HashMap<[u32; 16], [u8; 16]> = std::collections::HashMap::new();
        for bx in (0..w).step_by(4) {
            let block = read_block(bgra, w, h, bx, by);
            let bytes = *seen.entry(block).or_insert_with(|| {
                let alpha = block.map(|p| (p >> 24) as u8);
                let a = compress_alpha_block(&alpha);
                let c = compress_rgb_block(&block);
                let mut b = [0u8; 16];
                for (i, v) in [a[0], a[1], c[0], c[1]].into_iter().enumerate() {
                    b[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
                }

                b
            });
            out.extend(bytes);
        }

        out
    };

    let rows: Vec<Vec<u8>> = (0..h.div_ceil(4)).into_par_iter().map(|r| row(r * 4)).collect();
    rows.concat()
}
