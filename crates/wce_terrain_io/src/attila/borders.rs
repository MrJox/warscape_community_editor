//! A campaign map's borders as the game draws them: `campaign_maps/<map>/display/borders/borders.pbd`'s hex-cell lines, joined into
//! chains by the regions' owners and provinces (empire.retail.dll fns 0x10afda10, 0x10b22e10, 0x10ad5750), each chain fitted to cubic
//! Béziers ([`super::border_fit`]); docs/formats/campaign_borders.md.
//!
//! `borders.pbd` (little-endian): u16 1, u16 0x12a; u32 n + n region keys (u32 length + ASCII); u32 n entries of { u32 region (an index
//! into the keys), u32 n segments of { u32 neighbour (an index into the keys), u32 n lines of { u32 n + n x (u16 column, u16 row) } } };
//! u32 CRC-32 of everything before it.

use std::collections::HashMap;

use super::border_fit::{self, Cell, Cubic, Grid, hexdist};
use super::diplomacy::{ColourClass, Diplomacy};

/// One region's line of cells towards a neighbour.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PbdSegment {
    /// The neighbour (an index into the keys).
    pub neighbour: u32,
    pub lines: Vec<Vec<(u16, u16)>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PbdEntry {
    /// The region (an index into the keys).
    pub region: u32,
    pub segments: Vec<PbdSegment>,
}

/// A decoded `borders.pbd`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pbd {
    pub version: (u16, u16),
    pub keys: Vec<String>,
    pub entries: Vec<PbdEntry>,
    /// The trailer, and whether it is the CRC-32 of the bytes before it.
    pub crc: u32,
    pub crc_ok: bool,
}

/// CRC-32 (IEEE, reflected, as zlib's).
pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in bytes {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xedb8_8320 } else { c >> 1 };
        }
    }
    !c
}

/// A little-endian reader of the overlay files.
pub(crate) struct Reader<'a> {
    pub(crate) d: &'a [u8],
    pub(crate) o: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn bytes(&mut self, n: usize) -> Result<&'a [u8], String> {
        let s = self.d.get(self.o..self.o + n).ok_or_else(|| format!("truncated at {}", self.o))?;
        self.o += n;
        Ok(s)
    }

    pub(crate) fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap_or_default()))
    }

    pub(crate) fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap_or_default()))
    }

    /// A count, checked against what is left (`each` bytes an item at least).
    pub(crate) fn count(&mut self, each: usize) -> Result<usize, String> {
        let n = self.u32()? as usize;
        if n.saturating_mul(each) > self.d.len() - self.o {
            return Err(format!("count {n} at {} past the end", self.o - 4));
        }
        Ok(n)
    }

    pub(crate) fn string(&mut self) -> Result<String, String> {
        let n = self.count(1)?;
        Ok(String::from_utf8_lossy(self.bytes(n)?).into_owned())
    }
}

/// Decodes a `borders.pbd`.
///
/// # Errors
///
/// The file is truncated or has bytes past its trailer.
pub fn decode_pbd(d: &[u8]) -> Result<Pbd, String> {
    let mut r = Reader { d, o: 0 };
    let version = (r.u16()?, r.u16()?);
    let n = r.count(4)?;
    let keys = (0..n).map(|_| r.string()).collect::<Result<Vec<_>, _>>()?;
    let n = r.count(8)?;
    let mut entries = Vec::with_capacity(n);
    for _ in 0..n {
        let region = r.u32()?;
        let ns = r.count(8)?;
        let mut segments = Vec::with_capacity(ns);
        for _ in 0..ns {
            let neighbour = r.u32()?;
            let nl = r.count(4)?;
            let mut lines = Vec::with_capacity(nl);
            for _ in 0..nl {
                let nc = r.count(4)?;
                lines.push((0..nc).map(|_| Ok((r.u16()?, r.u16()?))).collect::<Result<Vec<_>, String>>()?);
            }
            segments.push(PbdSegment { neighbour, lines });
        }
        entries.push(PbdEntry { region, segments });
    }

    let end = r.o;
    let crc = r.u32()?;
    if r.o != d.len() {
        return Err(format!("{} bytes past the trailer", d.len() - r.o));
    }

    Ok(Pbd { version, keys, entries, crc, crc_ok: crc == crc32(&d[..end]) })
}

/// A chain of cells being built: the region whose list holds it, internal (between provinces of one owner) or external, the region pairs
/// it borders.
#[derive(Clone, Debug)]
struct Chain {
    cells: Vec<Cell>,
    internal: bool,
    pairs: Vec<(String, String)>,
}

fn pair(a: &str, b: &str) -> (String, String) {
    if a <= b { (a.to_owned(), b.to_owned()) } else { (b.to_owned(), a.to_owned()) }
}

impl Chain {
    /// An end of one within a cell of an end of the other (fn 0x10b2b8d0).
    fn touch(&self, o: &[Cell]) -> bool {
        let (a0, a1, b0, b1) = (self.cells[0], self.cells[self.cells.len() - 1], o[0], o[o.len() - 1]);
        [(a0, b0), (a0, b1), (a1, b0), (a1, b1)].iter().any(|&(x, y)| hexdist(x, y) <= 1)
    }

    /// A region pair in common (fn 0x10af92b0).
    fn shares_pair(&self, o: &Self) -> bool {
        self.pairs.iter().any(|p| o.pairs.contains(p))
    }

    /// Joins a line at the ends it meets (fns 0x10acad50 / 0x10acac50): a shared end cell first (the duplicate dropped), then
    /// neighbouring ends.
    fn join(&mut self, line: &[Cell]) -> bool {
        let (s0, s1) = (self.cells[0], self.cells[self.cells.len() - 1]);
        let n = line.len();
        let d = [hexdist(s0, line[0]), hexdist(s0, line[n - 1]), hexdist(s1, line[0]), hexdist(s1, line[n - 1])];
        for k in [0usize, 1] {
            let ki = k as i32;
            if d[0] == ki {
                let mut v: Vec<Cell> = line[1 - k..].iter().rev().copied().collect();
                v.extend_from_slice(&self.cells);
                self.cells = v;
                return true;
            }
            if d[1] == ki {
                let mut v: Vec<Cell> = line[..n - 1 + k].to_vec();
                v.extend_from_slice(&self.cells);
                self.cells = v;
                return true;
            }
            if d[2] == ki {
                self.cells.extend_from_slice(&line[1 - k..]);
                return true;
            }
            if d[3] == ki {
                self.cells.extend(line[..n - 1 + k].iter().rev());
                return true;
            }
        }
        false
    }

    fn absorb_pairs(&mut self, o: &[(String, String)]) {
        for p in o {
            if !self.pairs.contains(p) {
                self.pairs.push(p.clone());
            }
        }
    }
}

/// The chains of each land region, in `order` (fn 0x10b22e10, then fn 0x10ad5750 per region until nothing merges).
///
/// `owner` and `province`: a region key's owner and province (a region of `order` has both); `order`: the regions in the startpos's
/// order. A region's line towards a neighbour is kept when the owners differ (or the neighbour has none: the sea, off the map),
/// external, or the owner is one and the provinces differ, internal; it joins the region's first chain of its kind it touches.
fn build_chains(
    pbd: &Pbd,
    owner: &HashMap<String, String>,
    province: &HashMap<String, String>,
    order: &[String],
) -> Vec<(String, Vec<Chain>)> {
    let mut lines: HashMap<&str, Vec<(&str, Vec<Cell>)>> = HashMap::new();
    let key = |i: u32| pbd.keys.get(i as usize).map_or("", String::as_str);
    for e in &pbd.entries {
        for s in &e.segments {
            for l in &s.lines {
                let cells = l.iter().map(|&(c, r)| (i32::from(c), i32::from(r))).collect();
                lines.entry(key(e.region)).or_default().push((key(s.neighbour), cells));
            }
        }
    }

    let mut chains: Vec<(String, Vec<Chain>)> = vec![];
    for rk in order {
        let Some(ls) = lines.get(rk.as_str()) else { continue };
        let mut lst: Vec<Chain> = vec![];
        for (nk, cells) in ls {
            if cells.is_empty() {
                continue;
            }
            let same_owner = owner.get(*nk).is_some_and(|o| Some(o) == owner.get(rk));
            let internal = same_owner && province.get(*nk) != province.get(rk);
            let external = !owner.contains_key(*nk) || !same_owner;
            if !(internal || external) {
                continue;
            }

            let p = pair(rk, nk);
            if let Some(ch) = lst.iter_mut().find(|ch| ch.internal == internal && ch.touch(cells)) {
                ch.join(cells);
                ch.pairs.push(p);
            } else {
                lst.push(Chain { cells: cells.clone(), internal, pairs: vec![p] });
            }
        }
        chains.push((rk.clone(), lst));
    }

    let land: Vec<&String> = order.iter().collect();
    for ri in 0..chains.len() {
        while merge_pass(ri, &mut chains, owner, province, &land) {}
    }

    chains
}

/// One pass of fn 0x10ad5750 for a region: its chains take the touching chains of the owner's other regions (external with external,
/// internal with internal of the same province), then its external chains take each other.
fn merge_pass(
    ri: usize,
    chains: &mut [(String, Vec<Chain>)],
    owner: &HashMap<String, String>,
    province: &HashMap<String, String>,
    land: &[&String],
) -> bool {
    if chains[ri].1.is_empty() {
        return false;
    }

    let rk = chains[ri].0.clone();
    let index: HashMap<String, usize> = chains.iter().enumerate().map(|(i, c)| (c.0.clone(), i)).collect();
    let mut mine = std::mem::take(&mut chains[ri].1);
    let mut merged = false;
    for ok in land {
        if **ok == rk || owner.get(*ok) != owner.get(&rk) {
            continue;
        }
        let Some(&oi) = index.get(*ok) else { continue };
        for ch in &mut mine {
            let others = &mut chains[oi].1;
            let mut i = 0;
            while i < others.len() {
                let o = &others[i];
                let ok_flag = (!ch.internal && !o.internal) || (ch.internal && o.internal && province.get(*ok) == province.get(&rk));
                if ok_flag && !ch.shares_pair(o) && ch.touch(&o.cells) {
                    let o = others.remove(i);
                    ch.join(&o.cells);
                    ch.absorb_pairs(&o.pairs);
                    merged = true;
                    continue;
                }
                i += 1;
            }
        }
    }

    'again: loop {
        for a in 0..mine.len() {
            for b in 0..mine.len() {
                if a == b || mine[a].internal || mine[b].internal {
                    continue;
                }
                if !mine[a].shares_pair(&mine[b]) && mine[a].touch(&mine[b].cells) {
                    let o = mine[b].clone();
                    mine[a].join(&o.cells);
                    mine[a].absorb_pairs(&o.pairs);
                    mine.remove(b);
                    merged = true;
                    continue 'again;
                }
            }
        }
        break;
    }

    chains[ri].1 = mine;
    merged
}

/// A curve of a border chain.
#[derive(Clone, Debug, PartialEq)]
pub struct BorderCurve {
    /// The control points (x, z) in the campaign world.
    pub cp: [[f32; 2]; 4],
    /// Its 16 vertices' parameters (equal steps of its arc length); the chain's last curve draws a 17th at t = 1.
    pub ts: [f32; 16],
    /// Its start along the chain and its share of the chain's length (the texture's u = offset + t' · scale, t' its arc-length share).
    pub uv_offset: f32,
    pub uv_scale: f32,
}

/// A border chain: one region's border towards other owners (external) or its owner's other provinces (internal), as the game draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct BorderChain {
    /// The region whose list holds the chain (its owner colours it).
    pub region: String,
    /// The owner's faction key (empty: the rebels).
    pub owner: String,
    pub internal: bool,
    pub colour: ColourClass,
    /// The colour, 0 to 1.
    pub rgb: [f32; 3],
    pub curves: Vec<BorderCurve>,
    /// The chain's length in world units.
    pub length: f32,
    /// Its cells (column, row), as joined.
    pub cells: Vec<(i32, i32)>,
}

/// The border chains of a campaign for a chosen faction, in the game's order (the startpos's regions, each region's chains).
///
/// `grid`: the map's hex grid; `provinces`: region key -> province key (the db's `region_to_province_junctions`). A chain that does not
/// fit (cells not neighbours) is left out and counted in the returned notes.
#[must_use]
pub fn chains(
    pbd: &Pbd,
    grid: &Grid,
    diplomacy: &Diplomacy,
    provinces: &HashMap<String, String>,
    player: u32,
) -> (Vec<BorderChain>, Vec<String>) {
    // Owners by key, as the game compares them: the rebels one owner, an unknown id another.
    let mut owner = HashMap::new();
    let mut owner_id = HashMap::new();
    let mut province = HashMap::new();
    let mut order = vec![];
    for r in &diplomacy.regions {
        let known = diplomacy.factions.iter().any(|f| f.id == r.owner);
        let key = diplomacy.faction_key(r.owner);
        let ok = if !known {
            "?"
        } else if key.is_empty() {
            "(rebels)"
        } else {
            key
        };
        owner.insert(r.key.clone(), ok.to_owned());
        owner_id.insert(r.key.clone(), r.owner);
        province.insert(r.key.clone(), provinces.get(&r.key).cloned().unwrap_or_else(|| "?".to_owned()));
        order.push(r.key.clone());
    }

    let mut out = vec![];
    let mut failed = vec![];
    for (rk, list) in build_chains(pbd, &owner, &province, &order) {
        let oid = owner_id.get(&rk).copied().unwrap_or(0);
        let colour = diplomacy.colour_class(player, oid);
        for ch in list {
            let curves = match border_fit::fit(grid, &ch.cells) {
                Ok(c) => c,
                Err(e) => {
                    failed.push(format!("{rk}: {e}"));
                    continue;
                }
            };
            let (drawn, total) = border_fit::chain_curves(&curves);
            if drawn.is_empty() {
                continue;
            }

            out.push(BorderChain {
                region: rk.clone(),
                owner: diplomacy.faction_key(oid).to_owned(),
                internal: ch.internal,
                colour,
                rgb: colour.rgb(),
                curves: drawn.into_iter().map(|(cp, uo, us, ts)| curve(&cp, ts, uo, us)).collect(),
                length: total as f32,
                cells: ch.cells,
            });
        }
    }

    let mut notes = vec![format!("{} border chains", out.len())];
    if !failed.is_empty() {
        notes.push(format!("{} chains not fitted: {}", failed.len(), failed.join("; ")));
    }
    (out, notes)
}

pub(crate) fn curve(cp: &Cubic, ts: [f32; 16], uv_offset: f32, uv_scale: f32) -> BorderCurve {
    BorderCurve { cp: cp.map(|p| [p.0 as f32, p.1 as f32]), ts, uv_offset, uv_scale }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file of two regions bordering each other, written as the format says.
    fn sample() -> Vec<u8> {
        let mut d = vec![];
        d.extend(1u16.to_le_bytes());
        d.extend(0x12au16.to_le_bytes());
        d.extend(2u32.to_le_bytes());
        for k in ["a", "bb"] {
            d.extend((k.len() as u32).to_le_bytes());
            d.extend(k.as_bytes());
        }
        d.extend(2u32.to_le_bytes());
        for (reg, nb, cells) in [(0u32, 1u32, [(10u16, 10u16), (11, 10)]), (1, 0, [(10, 11), (11, 11)])] {
            d.extend(reg.to_le_bytes());
            d.extend(1u32.to_le_bytes());
            d.extend(nb.to_le_bytes());
            d.extend(1u32.to_le_bytes());
            d.extend(2u32.to_le_bytes());
            for (c, r) in cells {
                d.extend(c.to_le_bytes());
                d.extend(r.to_le_bytes());
            }
        }
        let crc = crc32(&d);
        d.extend(crc.to_le_bytes());
        d
    }

    #[test]
    fn crc32_is_zlibs() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn decodes_the_layout() {
        let p = decode_pbd(&sample()).expect("pbd");
        assert_eq!(p.version, (1, 0x12a));
        assert_eq!(p.keys, ["a", "bb"]);
        assert_eq!(p.entries.len(), 2);
        assert_eq!(p.entries[1].segments[0].neighbour, 0);
        assert_eq!(p.entries[0].segments[0].lines[0], [(10, 10), (11, 10)]);
        assert!(p.crc_ok);
        let mut bad = sample();
        bad[4] ^= 1;
        assert!(decode_pbd(&bad).is_err() || !decode_pbd(&bad).expect("pbd").crc_ok);
        assert!(decode_pbd(&sample()[..20]).is_err());
    }
}
