//! Source-relative height preparation for the witnessed built campaign profile (CHMF v5, literal single-block regions,
//! parameters 1/8, full/LQ grids 4x/2x). This forward delta contract is independently witnessed by localized Rome II edits;
//! it does not reconstruct the original authoring TIFF or infer nodes from arbitrary built DDS edits.
//!
//! The result contains heights only. Campaign normals are not regenerated; complete campaign output requires verified
//! companion-resource handling. This preparation module leaves the built reader and writer behavior unchanged.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use wce_formats::chmf::{ChmfFile, decode_block, encode_region};
use wce_terrain::paged::PagedU16;
use wce_tile_model::Blob;

use crate::{MapFiles, Result, file_error};

const CHMF: &str = "lf_height_map.data";
const FULL: &str = "lf_height_map.dds";
const LQ: &str = "lf_height_map_lq.dds";

/// Explicit raw-height changes on a bounded, inclusive CHMF nodal rectangle. The zero border bounds bilinear support. This is an
/// adapter input, not a new brush or an inferred inverse of the built full DDS. Fractional nodes remain unrounded until sampling.
pub struct NodalDelta {
    bounds: [u32; 4],
    values: Vec<f64>,
}

impl NodalDelta {
    /// Construct a complete row-major rectangle. Reject non-finite values, invalid bounds, and nonzero borders.
    ///
    /// # Errors
    ///
    /// The explicit field is invalid or cannot fit a raw u16 height difference.
    pub fn new(bounds: [u32; 4], values: Vec<f64>) -> Result<Self> {
        let [x0, y0, x1, y1] = bounds;
        let n = x1.checked_sub(x0).and_then(|w| w.checked_add(1)).zip(y1.checked_sub(y0).and_then(|h| h.checked_add(1)))
            .and_then(|(w, h)| (w as usize).checked_mul(h as usize));
        if x0 >= x1 || y0 >= y1 || n != Some(values.len()) {
            return Err(file_error(CHMF, "delta field must be a complete nonempty inclusive rectangle"));
        }
        let w = (x1 - x0 + 1) as usize;
        for (i, &v) in values.iter().enumerate() {
            let (x, y) = (x0 + (i % w) as u32, y0 + (i / w) as u32);
            if !v.is_finite() || v.round_ties_even().abs() > 65535.0 || ((x == x0 || x == x1 || y == y0 || y == y1) && v != 0.0) {
                return Err(file_error(CHMF, "delta nodes must be finite raw differences with a zero border"));
            }
        }
        Ok(Self { bounds, values })
    }

    fn at(&self, x: f64, y: f64) -> f64 {
        let [x0, y0, x1, y1] = self.bounds;
        if x < f64::from(x0) || y < f64::from(y0) || x >= f64::from(x1) || y >= f64::from(y1) {
            return 0.0;
        }
        let (ix, iy) = (x.floor() as u32, y.floor() as u32);
        let (fx, fy) = (x - f64::from(ix), y - f64::from(iy));
        let node = |x: u32, y: u32| self.values[((y - y0) as usize * (x1 - x0 + 1) as usize) + (x - x0) as usize];
        (1.0 - fx) * (1.0 - fy) * node(ix, iy) + fx * (1.0 - fy) * node(ix + 1, iy)
            + (1.0 - fx) * fy * node(ix, iy + 1) + fx * fy * node(ix + 1, iy + 1)
    }

    fn full(&self, x: u32, y: u32) -> f64 {
        let mut sum = 0.0;
        for a in 0..2 {
            for b in 0..2 {
                sum += self.at((f64::from(x) * 2.0 + f64::from(a)) / 8.0, (f64::from(y) * 2.0 + f64::from(b)) / 8.0);
            }
        }
        sum / 4.0
    }

    fn lq(&self, x: u32, y: u32) -> f64 {
        let mut sum = 0.0;
        for a in 0..2 {
            for b in 0..2 {
                sum += self.full(2 * x + a, 2 * y + b);
            }
        }
        sum / 4.0
    }
}

/// Counts concern stored/quantized changes, not the unrounded field support or a claim about normals/runtime rendering.
#[derive(Debug, Default)]
pub struct HeightDeltaStats {
    pub chmf_samples: usize,
    pub chmf_regions: usize,
    pub full_samples: usize,
    pub lq_samples: usize,
    pub full_lq_delta_error: f64,
}

/// Three prepared height resources only. This deliberately carries no normal files and is not a save/export authorization.
pub struct PreparedHeights {
    pub height_files: MapFiles,
    pub stats: HeightDeltaStats,
}

fn add(original: u16, delta: f64, path: &str) -> Result<u16> {
    // The independently witnessed delta author rounds nearest/ties-to-even once per representation, then adds to the stored base.
    // Do not run the raw TIFF builder's normalized-f32 truncation on already-built values, and never silently clamp/wrap.
    let value = i64::from(original) + delta.round_ties_even() as i64;
    u16::try_from(value).map_err(|_| file_error(path, "height delta would overflow or underflow a native u16 sample"))
}

fn block_len(block: &[u8]) -> Result<usize> {
    let n = match block.first().copied() {
        Some(0) => 3,
        Some(k @ 1..=127) => 1 + 2 * (usize::from(k) + 1) + (u8::BITS - k.leading_zeros()) as usize * 32,
        Some(k @ 128..=142) => 3 + usize::from(k - 127) * 32,
        Some(143) => 513,
        _ => return Err(file_error(CHMF, "unsupported CHMF block mode")),
    };
    if block.len() < n {
        return Err(file_error(CHMF, "truncated CHMF block"));
    }
    Ok(n)
}

fn dds(bytes: &[u8], path: &str, size: (u32, u32)) -> Result<PagedU16> {
    let u = |o: usize| bytes.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().expect("four bytes")));
    let n = (size.0 as usize).checked_mul(size.1 as usize).and_then(|n| n.checked_mul(2)).and_then(|n| n.checked_add(128));
    if bytes.get(..4) != Some(b"DDS ") || u(4) != Some(124) || u(76) != Some(32)
        || u(16) != Some(size.0) || u(12) != Some(size.1) || !matches!(u(28), Some(0 | 1))
        || u(80) != Some(0x20000) || u(84) != Some(0) || u(88) != Some(16) || u(92) != Some(65535)
        || [96, 100, 104].iter().any(|&o| u(o) != Some(0)) || n != Some(bytes.len())
    {
        return Err(file_error(path, "unsupported campaign DDS: expected the witnessed single-image L16 grid"));
    }
    Ok(PagedU16::from_le_rows(size.0, size.1, &bytes[128..]))
}

fn patch_dds(original: &[u8], field: &mut PagedU16, delta: &NodalDelta, scale: u32, path: &str) -> Result<(Vec<u8>, BTreeMap<(u32, u32), i32>)> {
    let [x0, y0, x1, y1] = delta.bounds;
    let mut changes = BTreeMap::new();
    for y in (scale * y0).saturating_sub(2)..(scale * y1 + 2).min(field.height()) {
        for x in (scale * x0).saturating_sub(2)..(scale * x1 + 2).min(field.width()) {
            let d = if scale == 4 { delta.full(x, y) } else { delta.lq(x, y) };
            let before = field.at(x, y);
            let after = add(before, d, path)?;
            if after != before {
                field.set(x, y, after);
                changes.insert((x, y), i32::from(after) - i32::from(before));
            }
        }
    }
    let mut out = original[..128].to_vec();
    out.extend(field.to_le_rows());
    Ok((out, changes))
}

/// Prepare the empirically validated forward height change without modifying its inputs. Only the witnessed interior field/profile
/// is enabled; map-edge fields, other sampling layouts, mips, and unsafe u16 changes fail explicitly. Unknown originals can still be
/// kept for a quantized-no-op field without decoding them. Arbitrary full-DDS brush edits are not inverted into a guessed field.
///
/// # Errors
///
/// Missing resources, unsupported profile, invalid/intersecting regions, undecodable data, or an out-of-range result.
pub fn prepare_delta(files: &MapFiles, delta: &NodalDelta) -> Result<PreparedHeights> {
    let mut out = MapFiles::default();
    for path in [CHMF, FULL, LQ] {
        let (key, blob) = files.folder.iter().find(|(p, _)| p.eq_ignore_ascii_case(path)).ok_or_else(|| file_error(path, "missing campaign height resource"))?;
        out.folder.insert(key.clone(), blob.clone());
    }
    let mut stats = HeightDeltaStats::default();
    // Bilinear/area weights are convex: if every node rounds to zero, every derived sample does too.
    if delta.values.iter().all(|&v| v.round_ties_even() == 0.0) {
        return Ok(PreparedHeights { height_files: out, stats });
    }
    let key = |path: &str| files.folder.keys().find(|p| p.eq_ignore_ascii_case(path)).expect("checked above").clone();
    let original = files.folder[&key(CHMF)].bytes()?;
    let mut c = ChmfFile::parse(&original).map_err(|e| file_error(CHMF, e))?;
    let [x0, y0, x1, y1] = delta.bounds;
    if c.version != 5 || c.width < 4 || c.height < 4 || c.cells_x != 1 || c.cells_y != 8 || c.spacing != 1
        || c.min.to_bits() != 0 || c.max != 65535.0 || x0 <= 1 || y0 <= 1 || x1 >= c.width - 1 || y1 >= c.height - 1
    {
        return Err(file_error(CHMF, "unsupported campaign height profile or non-interior delta bounds"));
    }
    // Checking the larger derived dimensions also bounds rectangle and support arithmetic below.
    let full_size = c.width.checked_mul(4).zip(c.height.checked_mul(4)).ok_or_else(|| file_error(CHMF, "height grid dimensions overflow"))?;
    let lq_size = (full_size.0 / 2, full_size.1 / 2);
    let full_bytes = files.folder[&key(FULL)].bytes()?;
    let lq_bytes = files.folder[&key(LQ)].bytes()?;
    let mut full = dds(&full_bytes, FULL, full_size)?;
    let mut lq = dds(&lq_bytes, LQ, lq_size)?;
    let expected = u64::from(c.width.div_ceil(16)) * u64::from(c.height.div_ceil(16));
    if c.regions.len() as u64 != expected {
        return Err(file_error(CHMF, "incomplete campaign region coverage"));
    }
    let mut seen = BTreeSet::new();
    for r in &mut c.regions {
        let [rx, ry, ex, ey] = r.rect;
        if rx >= c.width || ry >= c.height || rx % 16 != 0 || ry % 16 != 0 || rx.checked_add(16) != Some(ex) || ry.checked_add(16) != Some(ey)
            || r.offsets.len() != 1 || !seen.insert((rx, ry))
        {
            return Err(file_error(CHMF, "unsupported or duplicate campaign region rectangle/block table"));
        }
        let start = r.offsets[0] as usize;
        let block = r.data.get(start..).ok_or_else(|| file_error(CHMF, "CHMF block offset outside region"))?;
        let encoded = block_len(block)?;
        let mut samples = vec![];
        decode_block(block, 256, &mut samples).map_err(|e| file_error(CHMF, e))?;
        if r.tag != [*samples.iter().min().expect("256 samples"), 65535] {
            return Err(file_error(CHMF, "unsupported campaign minimum/65535 region tags"));
        }
        let mut changed = false;
        for (i, v) in samples.iter_mut().enumerate() {
            let (x, y) = (rx + (i % 16) as u32, ry + (i / 16) as u32);
            if x >= c.width || y >= c.height { continue; } // Stored padding is never synthesized from a clamped edge.
            let after = add(*v, delta.at(f64::from(x), f64::from(y)), CHMF)?;
            if after != *v {
                *v = after;
                changed = true;
                stats.chmf_samples += 1;
            }
        }
        if changed {
            let (tag, offsets, data) = encode_region(16, 16, &samples);
            let mut payload = r.data[..start].to_vec();
            payload.extend(data);
            payload.extend_from_slice(&block[encoded..]);
            r.tag[0] = tag[0];
            r.offsets = offsets.into_iter().map(|o| o + start as u32).collect();
            r.data = payload;
            stats.chmf_regions += 1;
        }
    }
    let (full_out, full_changes) = patch_dds(&full_bytes, &mut full, delta, 4, FULL)?;
    let (lq_out, lq_changes) = patch_dds(&lq_bytes, &mut lq, delta, 2, LQ)?;
    let points: BTreeSet<_> = lq_changes.keys().copied().chain(full_changes.keys().map(|&(x, y)| (x / 2, y / 2))).collect();
    for (x, y) in points {
        let mut sum = 0;
        for a in 0..2 { for b in 0..2 { sum += full_changes.get(&(2 * x + a, 2 * y + b)).copied().unwrap_or(0); } }
        let difference = f64::from(lq_changes.get(&(x, y)).copied().unwrap_or(0)) - f64::from(sum) / 4.0;
        stats.full_lq_delta_error = stats.full_lq_delta_error.max(difference.abs());
    }
    if stats.full_lq_delta_error > 1.0 {
        return Err(file_error(LQ, "prepared full/LQ deltas exceed the witnessed rounding bound"));
    }
    stats.full_samples = full_changes.len();
    stats.lq_samples = lq_changes.len();
    if stats.chmf_regions != 0 { out.folder.insert(key(CHMF), Blob::Bytes(Arc::from(c.write()))); }
    if stats.full_samples != 0 { out.folder.insert(key(FULL), Blob::Bytes(Arc::from(full_out))); }
    if stats.lq_samples != 0 { out.folder.insert(key(LQ), Blob::Bytes(Arc::from(lq_out))); }
    Ok(PreparedHeights { height_files: out, stats })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wce_formats::chmf::ChmfRegion;

    fn files(base: u16) -> MapFiles {
        let (w, h) = (48, 35); // Includes a partial final region row whose padding must survive.
        let mut c = ChmfFile { version: 5, width: w, height: h, cells_x: 1, cells_y: 8, min: 0.0, max: 65535.0, spacing: 1, regions: vec![] };
        for y in (0..h).step_by(16) {
            for x in (0..w).step_by(16) {
                let mut samples = [base; 256];
                for (i, v) in samples.iter_mut().enumerate() {
                    if y + i as u32 / 16 >= h { *v = 1; }
                }
                let (tag, _, data) = encode_region(16, 16, &samples);
                let mut blob = vec![0xaa, 0xbb];
                blob.extend(data);
                blob.extend([0xcc, 0xdd, 0xee]);
                c.regions.push(ChmfRegion { rect: [x, y, x + 16, y + 16], tag: [tag[0], 65535], offsets: vec![2], data: blob });
            }
        }
        MapFiles::from_bytes([
            (CHMF.into(), c.write()),
            (FULL.into(), crate::attila::heights::lf_height_dds(&PagedU16::filled(4 * w, 4 * h, base))),
            (LQ.into(), crate::attila::heights::lf_height_dds(&PagedU16::filled(2 * w, 2 * h, base))),
            ("normals/normal_0.dds".into(), b"must not be claimed regenerated".to_vec()),
        ])
    }

    fn impulse(x: u32, y: u32, amount: f64) -> NodalDelta {
        NodalDelta::new([x - 1, y - 1, x + 1, y + 1], vec![0.0, 0.0, 0.0, 0.0, amount, 0.0, 0.0, 0.0, 0.0]).unwrap()
    }

    fn coarse(file: &[u8]) -> (ChmfFile, BTreeMap<(u32, u32), u16>) {
        let c = ChmfFile::parse(file).unwrap();
        let mut values = BTreeMap::new();
        for r in &c.regions {
            let mut v = vec![];
            decode_block(&r.data[r.offsets[0] as usize..], 256, &mut v).unwrap();
            for (i, s) in v.into_iter().enumerate() {
                values.insert((r.rect[0] + (i % 16) as u32, r.rect[1] + (i / 16) as u32), s);
            }
        }
        (c, values)
    }

    #[test]
    fn explicit_impulse_preserves_header_regions_prefix_suffix_and_padding() {
        let f = files(10000);
        let out = prepare_delta(&f, &impulse(16, 16, -64.0)).unwrap();
        assert_eq!(out.height_files.folder.len(), 3, "normals are deliberately not included as corrected output");
        assert_eq!((out.stats.chmf_samples, out.stats.chmf_regions), (1, 1));
        let old = f.folder[CHMF].bytes().unwrap();
        let new = out.height_files.folder[CHMF].bytes().unwrap();
        assert_eq!(&old[..40], &new[..40]);
        let (a, av) = coarse(&old);
        let (b, bv) = coarse(&new);
        for (i, (r, s)) in a.regions.iter().zip(&b.regions).enumerate() {
            if i != 4 { assert_eq!(r, s); }
            else {
                assert_eq!(s.rect, r.rect);
                assert_eq!(s.tag, [9936, 65535]);
                assert_eq!(s.offsets, [2]);
                assert_eq!(&s.data[..2], &r.data[..2]);
                assert_eq!(&s.data[s.data.len() - 3..], &r.data[r.data.len() - 3..]);
            }
        }
        for (&p, &v) in &av {
            assert_eq!(bv[&p], if p == (16, 16) { v - 64 } else { v });
        }
        // Analytic tensor weights: full at (64,64): 64*(15/16)^2=56.25; LQ at (32,32):64*(13/16)^2=42.25.
        let full = dds(&out.height_files.folder[FULL].bytes().unwrap(), FULL, (192, 140)).unwrap();
        let low = dds(&out.height_files.folder[LQ].bytes().unwrap(), LQ, (96, 70)).unwrap();
        assert_eq!(full.at(64, 64), 9944);
        assert_eq!(low.at(32, 32), 9958);
        assert!(out.stats.full_lq_delta_error <= 1.0);
        // Edit a real sample in the last, partial region row. Its low-valued stored padding is neither clamped nor regenerated.
        let edge_region = prepare_delta(&f, &impulse(16, 32, 64.0)).unwrap();
        let (last, lv) = coarse(&edge_region.height_files.folder[CHMF].bytes().unwrap());
        assert_eq!(last.regions[7].tag, [1, 65535]);
        assert_eq!(lv[&(16, 32)], 10064);
        for y in 35..48 { for x in 16..32 { assert_eq!(lv[&(x, y)], 1); } }
    }

    #[test]
    fn ties_even_and_fractional_nodes_are_sampled_before_rounding() {
        let f = files(10000);
        for (amount, expected) in [(2.5, 10002), (3.5, 10004), (-2.5, 9998), (-3.5, 9996)] {
            let mut v = vec![amount; 9 * 9];
            for y in 0..9 { for x in 0..9 { if x == 0 || y == 0 || x == 8 || y == 8 { v[y * 9 + x] = 0.0; } } }
            let d = NodalDelta::new([12, 12, 20, 20], v).unwrap();
            let out = prepare_delta(&f, &d).unwrap();
            assert_eq!(coarse(&out.height_files.folder[CHMF].bytes().unwrap()).1[&(16, 16)], expected);
            assert_eq!(dds(&out.height_files.folder[FULL].bytes().unwrap(), FULL, (192, 140)).unwrap().at(64, 64), expected);
            assert_eq!(dds(&out.height_files.folder[LQ].bytes().unwrap(), LQ, (96, 70)).unwrap().at(32, 32), expected);
        }
        let d = impulse(16, 16, 2.5);
        let out = prepare_delta(&f, &d).unwrap();
        // At (63,63), average first then round:2.5*(13/16)^2=1.650390625 ->2. Rounding the node first would yield1.
        assert_eq!(dds(&out.height_files.folder[FULL].bytes().unwrap(), FULL, (192, 140)).unwrap().at(63, 63), 10002);
    }

    #[test]
    fn invalid_fields_unknown_profiles_and_overflow_fail_without_mutating_inputs() {
        for bad in [vec![0.0; 8], vec![f64::NAN; 9], vec![f64::INFINITY; 9], vec![1.0; 9]] {
            assert!(NodalDelta::new([3, 3, 5, 5], bad).is_err());
        }
        assert!(NodalDelta::new([4, 4, 3, 5], vec![]).is_err());
        assert!(NodalDelta::new([u32::MAX, 3, u32::MAX, 5], vec![]).is_err());
        for (base, amount) in [(0, -1.0), (65535, 1.0)] {
            let f = files(base);
            let before = f.clone();
            assert!(prepare_delta(&f, &impulse(16, 16, amount)).is_err());
            assert_eq!(f, before);
        }
        let f = files(10000);
        for edit in 0..5 {
            let mut unknown = f.clone();
            let mut c = ChmfFile::parse(&f.folder[CHMF].bytes().unwrap()).unwrap();
            match edit {
                0 => c.cells_y = 4,
                1 => c.version = 4,
                2 => c.regions[1].rect = c.regions[0].rect,
                3 => { c.regions.pop(); },
                _ => c.regions[0].tag[1] = 123,
            }
            unknown.folder.insert(CHMF.into(), Blob::Bytes(Arc::from(c.write())));
            assert!(prepare_delta(&unknown, &impulse(16, 16, 10.0)).is_err());
        }
        for path in [FULL, LQ] {
            let mut unknown = f.clone();
            let mut bytes = f.folder[path].bytes().unwrap().to_vec();
            bytes[28..32].copy_from_slice(&2u32.to_le_bytes());
            unknown.folder.insert(path.into(), Blob::Bytes(Arc::from(bytes)));
            assert!(prepare_delta(&unknown, &impulse(16, 16, 10.0)).is_err());
            // Failure in a DDS after a valid CHMF update must not mutate any input or return a partial result.
            let mut overflow = f.clone();
            let mut bytes = f.folder[path].bytes().unwrap().to_vec();
            let (w, x, y) = if path == FULL { (192, 64, 64) } else { (96, 32, 32) };
            let pos = 128 + 2 * (y * w + x);
            bytes[pos..pos + 2].copy_from_slice(&65535u16.to_le_bytes());
            overflow.folder.insert(path.into(), Blob::Bytes(Arc::from(bytes)));
            let before = overflow.clone();
            assert!(prepare_delta(&overflow, &impulse(16, 16, 64.0)).is_err());
            assert_eq!(overflow, before);
        }
        assert!(prepare_delta(&f, &impulse(2, 2, 10.0)).is_err(), "edge support is not part of the witnessed interior algorithm");
    }

    #[test]
    fn quantized_noop_keeps_unknown_originals_without_reading_them() {
        let f = MapFiles { folder: [CHMF, FULL, LQ].into_iter().map(|p| (p.into(), Blob::File(format!("missing-{p}").into()))).collect() };
        for amount in [0.0, 0.25, -0.5, 0.5] {
            let out = prepare_delta(&f, &impulse(16, 16, amount)).unwrap();
            assert_eq!(out.height_files, f);
            assert_eq!(out.stats.chmf_regions, 0);
            assert_eq!(out.stats.full_samples, 0);
        }
    }

    #[test]
    fn edits_cross_regions_and_keep_existing_headers_on_both_dds_grids() {
        let f = files(10000);
        let mut v = vec![0.0; 5 * 5];
        for y in 1..4 { for x in 1..4 { v[y * 5 + x] = if x == y { 100.0 } else { -20.0 }; } }
        let out = prepare_delta(&f, &NodalDelta::new([14, 14, 18, 18], v).unwrap()).unwrap();
        assert_eq!((out.stats.chmf_regions, out.stats.chmf_samples), (4, 9));
        for path in [FULL, LQ] {
            let a = f.folder[path].bytes().unwrap();
            let b = out.height_files.folder[path].bytes().unwrap();
            assert_eq!(&a[..128], &b[..128]);
            assert_eq!(a.len(), b.len());
        }
    }
}
