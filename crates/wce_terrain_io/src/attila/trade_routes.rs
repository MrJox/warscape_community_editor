//! A campaign map's trade routes as the game draws them (the dashed yellow lines): `campaign_maps/<map>/trade_routes.ptd`'s Bézier
//! segments, picked by the chosen faction's international trade routes in the startpos (empire.retail.dll loader fn 0x1084eb70;
//! docs/formats/campaign_borders.md, "Trade routes").
//!
//! `trade_routes.ptd` (little-endian): the magic `89 'PTD' 0d 0a 1a 0a`, u32 version (1); u32 n + n region keys (u32 length + ASCII);
//! u32 n segments of { u8 x 4 flags, u32 n curves of 16 bytes: u32 P0 (u16 X, u16 Z), u32 H1 (i16 dx, i16 dz), u32 H2 (i16, i16),
//! u32 P3 }; two route tables (0 land legs, 1 sea legs), each u32 n records of { u32 region A, u32 region B, u32 n + n segment refs
//! (2 · segment + reversed) }; u32 CRC-32 of everything before it (not read by the loader).
//!
//! A point (fns 0x108445f0, 0x1083a780, 0x1083a740, single precision): x = X · a · 0.25 - a, z = Z · b · 0.25 - b, a = width /
//! (1.5 (columns - 1)), b = height / (2 rows + 1) (the hex grid's +0x64, +0x68); a handle: P1 = 0.01 · (dx · a · 0.25, dz · b · 0.25)
//! + P0, P2 = the same of H2 + P3. A segment whose flag byte 0 is 1 gets no draw object (fn 0x10aba960).

use std::collections::{BTreeMap, HashMap};

use rpfm_lib::files::esf::{NodeType, RecordNode};

use super::border_fit::{Cubic, Grid, curve_length, f, vertex_ts};
use super::borders::{BorderCurve, Reader, crc32, curve};
use super::cities::{as_record, child, leaf, u32_of};
use super::diplomacy::Diplomacy;

const MAGIC: &[u8; 8] = b"\x89PTD\r\n\x1a\n";

/// A segment of `trade_routes.ptd`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PtdSegment {
    pub flags: [u8; 4],
    /// Per curve: X0, Z0, H1 dx, dz, H2 dx, dz, X3, Z3 (the handles signed).
    pub curves: Vec<[i32; 8]>,
}

/// A route record: the legs' regions (indices into the keys) and its segment refs (2 · segment + reversed).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PtdRoute {
    pub a: u32,
    pub b: u32,
    pub refs: Vec<u32>,
}

/// A decoded `trade_routes.ptd`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ptd {
    pub version: u32,
    pub keys: Vec<String>,
    pub segments: Vec<PtdSegment>,
    /// Table 0 the land legs, 1 the sea legs.
    pub tables: [Vec<PtdRoute>; 2],
    pub crc: u32,
    pub crc_ok: bool,
}

/// Decodes a `trade_routes.ptd`.
///
/// # Errors
///
/// Not the magic, truncated, or bytes past the trailer.
pub fn decode_ptd(d: &[u8]) -> Result<Ptd, String> {
    if d.get(..8) != Some(MAGIC.as_slice()) {
        return Err("not a PTD file".to_owned());
    }

    let mut r = Reader { d, o: 8 };
    let version = r.u32()?;
    let n = r.count(4)?;
    let keys = (0..n).map(|_| r.string()).collect::<Result<Vec<_>, _>>()?;
    let n = r.count(8)?;
    let mut segments = Vec::with_capacity(n);
    for _ in 0..n {
        let flags: [u8; 4] = r.bytes(4)?.try_into().unwrap_or_default();
        let nc = r.count(16)?;
        let mut curves = Vec::with_capacity(nc);
        for _ in 0..nc {
            let v: Vec<u16> = (0..8).map(|_| r.u16()).collect::<Result<_, _>>()?;
            let s = |i: usize| i32::from(v[i] as i16);
            let u = |i: usize| i32::from(v[i]);
            curves.push([u(0), u(1), s(2), s(3), s(4), s(5), u(6), u(7)]);
        }
        segments.push(PtdSegment { flags, curves });
    }

    let mut table = || -> Result<Vec<PtdRoute>, String> {
        let n = r.count(12)?;
        (0..n)
            .map(|_| {
                let (a, b) = (r.u32()?, r.u32()?);
                let k = r.count(4)?;
                Ok(PtdRoute { a, b, refs: (0..k).map(|_| r.u32()).collect::<Result<_, _>>()? })
            })
            .collect()
    };
    let tables = [table()?, table()?];
    let end = r.o;
    let crc = r.u32()?;
    if r.o != d.len() {
        return Err(format!("{} bytes past the trailer", d.len() - r.o));
    }

    Ok(Ptd { version, keys, segments, tables, crc, crc_ok: crc == crc32(&d[..end]) })
}

impl Ptd {
    /// A segment's curves in the world, as the game decodes them (`a`, `b`: the grid's [`Grid::trade_x`], [`Grid::trade_z`]).
    #[must_use]
    pub fn curves(&self, segment: usize, a: f64, b: f64) -> Vec<Cubic> {
        let q = f(0.25);
        let c01 = f(0.01);
        let Some(s) = self.segments.get(segment) else { return vec![] };
        s.curves
            .iter()
            .map(|c| {
                let p = |x: i32, z: i32| (f(f(f(f64::from(x) * a) * q) - a), f(f(f(f64::from(z) * b) * q) - b));
                let (p0, p3) = (p(c[0], c[1]), p(c[6], c[7]));
                let h = |o: (f64, f64), dx: i32, dz: i32| {
                    let (hx, hz) = (f(f(f64::from(dx) * a) * q), f(f(f64::from(dz) * b) * q));
                    (f(f(hx * c01) + o.0), f(f(hz * c01) + o.1))
                };
                [p0, h(p0, c[2], c[3]), h(p3, c[4], c[5]), p3]
            })
            .collect()
    }

    /// The segment refs of a leg between two regions (by key) in a table, either way round.
    #[must_use]
    pub fn leg(&self, table: usize, a: &str, b: &str) -> Option<&[u32]> {
        let ia = self.keys.iter().position(|k| k == a)? as u32;
        let ib = self.keys.iter().position(|k| k == b)? as u32;
        self.tables.get(table)?.iter().find(|r| (r.a, r.b) == (ia, ib) || (r.a, r.b) == (ib, ia)).map(|r| r.refs.as_slice())
    }
}

/// An international trade route of the startpos.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartRoute {
    /// The faction whose list holds it and its partner (runtime ids).
    pub owner: u32,
    pub partner: u32,
    /// The regions it passes (runtime ids), from the owner's.
    pub path: Vec<u32>,
    /// Its sea leg (indices into the path), if any.
    pub sea: Option<(usize, usize)>,
}

/// The startpos's international trade routes (`CAMPAIGN_ENV/CAMPAIGN_MODEL/CAMPAIGN_TRADE_MANAGER/INTERNATIONAL_TRADE_ROUTES`: per
/// group the owner's runtime id and its `FACTION_INTERNATIONAL_TRADE_ROUTES_ARRAY/INTERNATIONAL_TRADE_ROUTE`s: `^+1` the partner,
/// `^+2` the path, `^+3` whether a leg is by sea, then that leg's two path indices).
#[must_use]
pub fn start_routes(root: &RecordNode) -> Vec<StartRoute> {
    let mut out = vec![];
    let Some(tm) = ["CAMPAIGN_ENV", "CAMPAIGN_MODEL", "CAMPAIGN_TRADE_MANAGER"].iter().try_fold(root, |r, n| child(r, n)) else {
        return out;
    };
    let Some(itr) = child(tm, "INTERNATIONAL_TRADE_ROUTES") else { return out };
    for g in itr.children() {
        let Some(owner) = leaf(g, None, 1).and_then(u32_of) else { continue };
        for arr in g.iter().filter_map(as_record).filter(|r| r.name() == "FACTION_INTERNATIONAL_TRADE_ROUTES_ARRAY") {
            for route in arr.children().iter().flatten().filter_map(as_record).filter(|r| r.name() == "INTERNATIONAL_TRADE_ROUTE") {
                for rg in route.children() {
                    let leaves: Vec<&NodeType> = rg.iter().take_while(|n| as_record(n).is_none()).collect();
                    let Some(partner) = leaves.first().copied().and_then(u32_of) else { continue };
                    let Some(NodeType::U32Array(path)) = leaves.get(1) else { continue };
                    let by_sea = matches!(leaves.get(2), Some(NodeType::Bool(b)) if *b.value());
                    let sea = if by_sea {
                        match (leaves.get(3).copied().and_then(u32_of), leaves.get(4).copied().and_then(u32_of)) {
                            (Some(a), Some(b)) => Some((a as usize, b as usize)),
                            _ => None,
                        }
                    } else {
                        None
                    };
                    out.push(StartRoute { owner, partner, path: path.value().clone(), sea });
                }
            }
        }
    }

    out
}

/// A drawn trade-route segment.
#[derive(Clone, Debug, PartialEq)]
pub struct TradeSegment {
    /// Its index in the file.
    pub segment: usize,
    /// From the sea legs' table (the route's sea leg).
    pub sea_leg: bool,
    /// The game draws it: its flag byte 0 is 0 (fn 0x10aba960 makes no draw object for 1).
    pub drawn: bool,
    pub flags: [u8; 4],
    /// The curves in the file's direction; uv in world units rounded up to a whole number per segment (offset + t' · scale).
    pub curves: Vec<BorderCurve>,
    /// The segment's length in world units.
    pub length: f32,
}

/// The trade-route segments of the routes `player` holds in the startpos (each leg's table: the route's sea leg the sea table, the
/// others the land table), each once, by index; drawn or not ([`TradeSegment::drawn`]). Also the legs not found in the file.
#[must_use]
pub fn segments(ptd: &Ptd, routes: &[StartRoute], diplomacy: &Diplomacy, player: u32, grid: &Grid) -> (Vec<TradeSegment>, Vec<String>) {
    let region: HashMap<u32, &str> = diplomacy.regions.iter().map(|r| (r.id, r.key.as_str())).collect();
    let mut used: BTreeMap<usize, bool> = BTreeMap::new();
    let mut missing = vec![];
    for route in routes.iter().filter(|r| r.owner == player) {
        for k in 0..route.path.len().saturating_sub(1) {
            let sea = route.sea == Some((k, k + 1));
            let (a, b) = (region.get(&route.path[k]).copied().unwrap_or("?"), region.get(&route.path[k + 1]).copied().unwrap_or("?"));
            match ptd.leg(usize::from(sea), a, b) {
                Some(refs) => {
                    for r in refs {
                        *used.entry((r >> 1) as usize).or_default() |= sea;
                    }
                }
                None => missing.push(format!("{a} -> {b}{}", if sea { " (sea)" } else { "" })),
            }
        }
    }

    let out = used
        .into_iter()
        .filter(|&(s, _)| s < ptd.segments.len())
        .map(|(s, sea_leg)| {
            let cps = ptd.curves(s, grid.trade_x, grid.trade_z);
            let lens: Vec<f64> = cps.iter().map(|c| curve_length(c, 0.0, 1.0)).collect();
            let total = lens.iter().fold(0.0, |a, l| f(a + l));
            let scale = if total > 0.0 { total.ceil() / total } else { 1.0 };
            let mut acc = 0.0;
            let curves = cps
                .iter()
                .zip(&lens)
                .map(|(cp, l)| {
                    let c = curve(cp, vertex_ts(cp), (acc * scale) as f32, (l * scale) as f32);
                    acc = f(acc + l);
                    c
                })
                .collect();
            let flags = ptd.segments[s].flags;
            TradeSegment { segment: s, sea_leg, drawn: flags[0] == 0, flags, curves, length: total as f32 }
        })
        .collect();
    (out, missing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_the_layout() {
        let mut d = MAGIC.to_vec();
        d.extend(1u32.to_le_bytes());
        d.extend(2u32.to_le_bytes());
        for k in ["r0", "r1"] {
            d.extend((k.len() as u32).to_le_bytes());
            d.extend(k.as_bytes());
        }
        d.extend(1u32.to_le_bytes());
        d.extend([1, 0, 0, 1]);
        d.extend(1u32.to_le_bytes());
        for v in [8u16, 8, (-100i16) as u16, 0, 100, 0, 16, 8] {
            d.extend(v.to_le_bytes());
        }
        for t in 0..2u32 {
            d.extend(1u32.to_le_bytes());
            d.extend([0u32, 1, 1, t].iter().flat_map(|v| v.to_le_bytes()));
        }
        let crc = crc32(&d);
        d.extend(crc.to_le_bytes());
        let p = decode_ptd(&d).expect("ptd");
        assert_eq!((p.version, p.keys.len(), p.segments.len()), (1, 2, 1));
        assert_eq!(p.segments[0].curves[0][2], -100);
        assert_eq!(p.tables[1][0].refs, [1]);
        assert!(p.crc_ok);
        assert_eq!(p.leg(0, "r1", "r0"), Some([0u32].as_slice()));
        let c = p.curves(0, 1.0, 2.0);
        assert_eq!((c[0][0], c[0][1].0), ((1.0, 2.0), f(1.0 - 0.25)));
        assert!(decode_ptd(&d[..d.len() - 1]).is_err());
    }
}
