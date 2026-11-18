//! A campaign's fog of war at its start: the shroud texture (`s_shroud`) the game shows a chosen faction on turn 1, built from the
//! startpos and `map_data.esf` (docs/formats/campaign_shroud.md).
//!
//! The frame: an A8 texture of [`CampaignShroud::width`] x [`CampaignShroud::height`] texels (1485 x 1240 on `main_attila_map`): the
//! content, 2 texels a world unit (1357 x 1112 = ceil(2 x 678.5) x ceil(2 x 555.625), the size of the startpos's `QUAD_TREE_BIT_ARRAY`s),
//! with 64 texels of padding on every side. Row 0 is the map's south edge (z = 0), x runs east: content texel (x, y) covers world x in
//! [x / 2, (x + 1) / 2), z in [y / 2, (y + 1) / 2). Values: 64 hidden, 128 explored, 255 visible (0 where the faction's third bitmap is
//! set), then a separable 20-tap blur. The game's shaders read it with `g_uv_offset` = (64 / 1485, 64 / 1240) and
//! `g_campaign_world_bounds` (0, 0, 678.5, 555.625).
//!
//! The startpos records read (RPFM's names): `FACTION/CAMPAIGN_SHROUD` (three `QUAD_TREE_BIT_ARRAY`s: explored, visible, the third),
//! `LINE_OF_SIGHT` (a flag, a bounding box, a `QUAD_TREE_BIT_ARRAY`) under `CHARACTER`, `REGION`, `GARRISON_RESIDENCE` and
//! `FACTION_SEA_REGION_MANAGER`, `CAMPAIGN_PREOPEN_MAP_INFO/REGION_OWNERSHIPS_BY_THEATRE/REGION_OWNERSHIPS`, the diplomacy
//! ([`super::diplomacy`]); from `map_data.esf` the hex cells' regions ([`super::roads::HexMap`]) and the regions' neighbours
//! (`REGION_DATA/CONNECTIVITY_DIRECT_*`).

use std::collections::{HashMap, HashSet};
use std::io::Cursor;

use rpfm_lib::files::Decodeable;
use rpfm_lib::files::esf::{ESF, NodeType, RecordNode};
use rpfm_lib::schema::Schema;

use super::cities::{as_record, child, str_of, u32_of};
use super::diplomacy::{Diplomacy, Stance, default_faction, faction_infos};
use super::roads::HexMap;
use super::start;

/// Texels of padding on every side of the content.
pub const PADDING: usize = 64;

/// The fill values before the blur.
pub const HIDDEN: u8 = 64;
pub const EXPLORED: u8 = 128;
pub const VISIBLE: u8 = 255;

/// The game's `g_campaign_world_bounds` on `main_attila_map` (x, z of the far corner; the near one is (0, 0)).
pub const MAIN_ATTILA_WORLD: (f32, f32) = (678.5, 555.625);

/// The blur's half-width: taps at offsets -10 to +9.
const BLUR_K: i32 = 10;

/// Regions left out of the explored area (not land a faction explores).
const SKIPPED_REGIONS: [&str; 2] = ["att_reg_terra_incognita", "att_sea_lake"];

/// The shroud texture of a faction at the start.
#[derive(Clone, Debug, Default)]
pub struct CampaignShroud {
    /// The campaign whose startpos was read ([`read`]; empty from [`build`]), and the faction.
    pub campaign: String,
    pub faction: String,
    /// The texture's size: the content plus [`PADDING`] on each side.
    pub width: usize,
    pub height: usize,
    /// A8 texels, `width` a row, row 0 the south edge.
    pub pixels: Vec<u8>,
    /// The content's size in texels (2 a world unit).
    pub content: (usize, usize),
    pub padding: usize,
    /// The world bounds' far corner the shaders map the content to (x east, z north; the near corner (0, 0)).
    pub world: (f32, f32),
    /// What was read, missing or left out.
    pub notes: Vec<String>,
}

impl CampaignShroud {
    /// The texture with the shroud disabled (fully revealed): 255 everywhere, no blur.
    #[must_use]
    pub fn revealed(content: (usize, usize), world: (f32, f32)) -> Self {
        let (width, height) = (content.0 + 2 * PADDING, content.1 + 2 * PADDING);
        Self { width, height, pixels: vec![VISIBLE; width * height], content, padding: PADDING, world, ..Self::default() }
    }

    /// The `g_uv_offset` the game's shaders take: the padding as a fraction of the texture.
    #[must_use]
    pub fn uv_offset(&self) -> (f32, f32) {
        (self.padding as f32 / self.width as f32, self.padding as f32 / self.height as f32)
    }
}

/// A bitmap over the content, row 0 the south edge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bitmap {
    pub width: usize,
    pub height: usize,
    pub bits: Vec<bool>,
}

impl Bitmap {
    #[must_use]
    pub fn new(width: usize, height: usize) -> Self {
        Self { width, height, bits: vec![false; width * height] }
    }

    #[must_use]
    pub fn get(&self, x: usize, y: usize) -> bool {
        self.bits[y * self.width + x]
    }

    fn fill(&mut self, x: usize, y: usize, w: usize, h: usize) {
        for row in y..(y + h).min(self.height) {
            let r = row * self.width;
            for col in x..(x + w).min(self.width) {
                self.bits[r + col] = true;
            }
        }
    }
}

/// Sets in `out` the bits a quadtree blob (`QUAD_TREE_BIT_ARRAY`'s bytes) sets over a square of `size` texels whose corner (0, 0) is
/// the bitmap's: bits beyond the bitmap are dropped.
///
/// The blob, depth first: a node byte holds four 2-bit child codes from its low bits up, the children NW, NE, SW, SE (row 0 south:
/// (0, half), (half, half), (0, 0), (half, 0)): `0b10` empty, `0b11` full, `0b01` subdivided (its node follows); `0b00` unused. A
/// node byte 0 makes the node a leaf instead: 8 bytes (two LE u32 = 64 bits), bit i the cell (i % 8, i / 8) of an 8 x 8 grid over the
/// node's square, cell row 0 its south edge.
///
/// # Errors
///
/// The blob ends early or has bytes left over.
pub fn decode_quadtree_into(blob: &[u8], size: usize, out: &mut Bitmap) -> Result<(), String> {
    fn node(blob: &[u8], pos: &mut usize, x: usize, y: usize, s: usize, out: &mut Bitmap) -> Result<(), String> {
        let b = *blob.get(*pos).ok_or("quadtree: the blob ends early")?;
        *pos += 1;
        if b == 0 {
            let bytes: [u8; 8] = blob.get(*pos..*pos + 8).and_then(|v| v.try_into().ok()).ok_or("quadtree: a leaf ends early")?;
            *pos += 8;
            let m = u64::from_le_bytes(bytes);
            let c = s / 8;
            for i in 0..64 {
                if m >> i & 1 == 1 {
                    out.fill(x + (i % 8) * c, y + (i / 8) * c, c, c);
                }
            }
            return Ok(());
        }

        let h = s / 2;
        for k in 0..4 {
            let (qx, qy) = [(0, 1), (1, 1), (0, 0), (1, 0)][k];
            match b >> (2 * k) & 3 {
                1 => node(blob, pos, x + qx * h, y + qy * h, h, out)?,
                3 => out.fill(x + qx * h, y + qy * h, h, h),
                _ => {}
            }
        }
        Ok(())
    }

    let mut pos = 0;
    node(blob, &mut pos, 0, 0, size, out)?;
    if pos != blob.len() {
        return Err(format!("quadtree: {} bytes left over", blob.len() - pos));
    }
    Ok(())
}

/// A quadtree blob decoded into a new bitmap of `width` x `height` ([`decode_quadtree_into`]).
///
/// # Errors
///
/// As [`decode_quadtree_into`].
pub fn decode_quadtree(blob: &[u8], width: usize, height: usize, size: usize) -> Result<Bitmap, String> {
    let mut b = Bitmap::new(width, height);
    decode_quadtree_into(blob, size, &mut b)?;
    Ok(b)
}

/// A `QUAD_TREE_BIT_ARRAY` record: width, height, size, blob.
#[derive(Clone, Debug)]
struct QuadTree<'a> {
    width: usize,
    height: usize,
    size: usize,
    blob: &'a [u8],
}

impl QuadTree<'_> {
    fn or_into(&self, out: &mut Bitmap) -> Result<(), String> {
        decode_quadtree_into(self.blob, self.size, out)
    }
}

fn quadtree(r: &RecordNode) -> Option<QuadTree<'_>> {
    let g = r.children().first()?;
    let dims: Vec<usize> = g.iter().filter_map(u32_of).take(3).map(|v| v as usize).collect();
    let blob = g.iter().find_map(|n| match n {
        NodeType::U8Array(v) => Some(v.as_slice()),
        _ => None,
    })?;
    let [width, height, size] = dims[..] else { return None };
    Some(QuadTree { width, height, size, blob })
}

/// Where a `LINE_OF_SIGHT` sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LosOwner {
    Character,
    Region,
    Garrison,
    SeaRegion,
}

struct Los<'a> {
    owner: LosOwner,
    /// The innermost `FACTION` (by key), `REGION` (by key) and `FACTION_SEA_REGION_MANAGER` (its faction id) around it.
    faction: Option<&'a str>,
    region: Option<&'a str>,
    sea_faction: Option<u32>,
    tree: QuadTree<'a>,
}

#[derive(Default)]
struct Gathered<'a> {
    /// Region key -> owner's faction key.
    owners: HashMap<&'a str, &'a str>,
    /// Faction key -> its three shroud bitmaps.
    shrouds: HashMap<&'a str, Vec<QuadTree<'a>>>,
    los: Vec<Los<'a>>,
}

#[derive(Clone, Copy, Default)]
struct Ctx<'a> {
    faction: Option<&'a str>,
    region: Option<&'a str>,
    sea_faction: Option<u32>,
}

fn ascii(n: Option<&NodeType>) -> Option<&str> {
    match n {
        Some(NodeType::Ascii(s)) => Some(s.as_str()),
        _ => None,
    }
}

fn gather<'a>(r: &'a RecordNode, parent: &str, ctx: Ctx<'a>, out: &mut Gathered<'a>) {
    let name = r.name().as_str();
    for g in r.children() {
        let mut c = ctx;
        match name {
            "FACTION" => c.faction = ascii(g.get(1)).or(c.faction),
            "REGION" => c.region = ascii(g.get(1)).or(c.region),
            "FACTION_SEA_REGION_MANAGER" => c.sea_faction = g.get(1).and_then(u32_of).or(c.sea_faction),
            "REGION_OWNERSHIPS" => {
                let s: Vec<&str> = g.iter().filter_map(str_of).take(2).collect();
                if let [region, owner] = s[..] {
                    out.owners.insert(region, owner);
                }
            }
            "CAMPAIGN_SHROUD" => {
                let trees: Vec<QuadTree> =
                    g.iter().filter_map(as_record).filter(|q| q.name() == "QUAD_TREE_BIT_ARRAY").filter_map(quadtree).collect();
                if let Some(f) = c.faction {
                    out.shrouds.insert(f, trees);
                }
            }
            "LINE_OF_SIGHT" => {
                let on = matches!(g.first(), Some(NodeType::Bool(b)) if *b.value());
                let owner = match parent {
                    "CHARACTER" => Some(LosOwner::Character),
                    "REGION" => Some(LosOwner::Region),
                    "GARRISON_RESIDENCE" => Some(LosOwner::Garrison),
                    "FACTION_SEA_REGION_MANAGER" => Some(LosOwner::SeaRegion),
                    _ => None,
                };
                let tree = g.iter().filter_map(as_record).find(|q| q.name() == "QUAD_TREE_BIT_ARRAY").and_then(quadtree);
                if let (true, Some(owner), Some(tree)) = (on, owner, tree) {
                    out.los.push(Los { owner, faction: c.faction, region: c.region, sea_faction: c.sea_faction, tree });
                }
            }
            _ => {}
        }
        for sub in g.iter().filter_map(as_record) {
            gather(sub, name, c, out);
        }
    }
}

/// `map_data.esf`'s regions (`REGIONS_BLOCK/REGION_DATA`, in order) with their direct neighbours' indices (`CONNECTIVITY_DIRECT_*`:
/// traversable or not, same or other kind).
fn neighbours(map_data: &[u8]) -> Result<Vec<Vec<usize>>, String> {
    let md = ESF::decode(&mut Cursor::new(map_data), &None).map_err(|e| format!("map_data.esf: {e}"))?;
    let root = as_record(md.root_node()).ok_or("map_data.esf: no root record")?;
    let block = child(root, "REGIONS_DATA").and_then(|r| child(r, "REGIONS_BLOCK")).ok_or("map_data.esf: no REGIONS_BLOCK")?;
    Ok(block
        .children()
        .iter()
        .flatten()
        .filter_map(as_record)
        .filter(|r| r.name() == "REGION_DATA")
        .map(|r| {
            r.children()
                .iter()
                .flatten()
                .filter_map(as_record)
                .filter(|c| c.name().starts_with("CONNECTIVITY_DIRECT"))
                .flat_map(|c| c.children().iter().filter_map(|g| g.first().and_then(u32_of)))
                .map(|v| v as usize)
                .collect()
        })
        .collect())
}

/// The region (index) of every content texel: the hex cell whose centre is nearest the texel's centre (x = (tx + 0.5) / 2, z = (ty +
/// 0.5) / 2), cells as docs/formats/campaign_borders.md has them; ties to the first of columns c0 - 1 .. c0 + 2, rows r0 - 1 .. r0 + 1.
fn texel_regions(hex: &HexMap, (tw, th): (usize, usize)) -> Vec<u8> {
    let (hw, hh) = (i64::from(hex.w), i64::from(hex.h));
    let colw = hex.world.0 / (hw - 1) as f64;
    let rowh = 3f64.sqrt() * colw / 1.5;
    let mut out = vec![0; tw * th];
    for ty in 0..th {
        let z = (ty as f64 + 0.5) / 2.0;
        for tx in 0..tw {
            let x = (tx as f64 + 0.5) / 2.0;
            let c0 = (x / colw).floor() as i64;
            let mut best = 1e9;
            let mut reg = 0;
            for dc in -1..=2 {
                let c = (c0 + dc).clamp(0, hw - 1);
                let off = if c % 2 == 1 { rowh / 2.0 } else { 0.0 };
                let r0 = ((z - off) / rowh).floor() as i64;
                for dr in -1..=1 {
                    let r = (r0 + dr).clamp(0, hh - 1);
                    let dx = x - c as f64 * colw;
                    let dz = z - r as f64 * rowh - off;
                    let d = dx * dx + dz * dz;
                    if d < best {
                        best = d;
                        reg = hex.region((c as u32, r as u32));
                    }
                }
            }
            out[ty * tw + tx] = reg;
        }
    }
    out
}

/// The blur's 20 weights (offsets -10 to +9): e^(-x² / 40) x 0.19947114, normalised to sum 1, in single precision (the sum as numpy's
/// pairwise float32 sum, so the reference's bytes are reproduced exactly).
fn blur_weights() -> [f32; 20] {
    let scale = (-0.25 / f64::from(BLUR_K)) as f32;
    let w: [f32; 20] = std::array::from_fn(|i| {
        let t = (i as i32 - BLUR_K) as f32;
        std::f32::consts::E.powf(t * t * scale) * 0.199_471_14
    });
    let mut r: [f32; 8] = std::array::from_fn(|j| w[j]);
    for (j, v) in r.iter_mut().enumerate() {
        *v += w[8 + j];
    }
    let mut sum = ((r[0] + r[1]) + (r[2] + r[3])) + ((r[4] + r[5]) + (r[6] + r[7]));
    for v in &w[16..] {
        sum += v;
    }
    w.map(|v| v / sum)
}

/// The separable blur: horizontal, then vertical, each pass summed in single precision (taps in offset order) and floored to a byte;
/// the texture wraps at its edges (inside the padding, so it never reaches the content).
#[must_use]
pub fn blur(pixels: &[u8], width: usize, height: usize) -> Vec<u8> {
    let w = blur_weights();
    let k = BLUR_K as isize;
    let pass = |src: &[f32], horizontal: bool| -> Vec<f32> {
        let mut out = vec![0f32; src.len()];
        for y in 0..height {
            for x in 0..width {
                let mut s = 0f32;
                for (i, wt) in w.iter().enumerate() {
                    let o = i as isize - k;
                    let at = if horizontal {
                        y * width + (x as isize + o).rem_euclid(width as isize) as usize
                    } else {
                        (y as isize + o).rem_euclid(height as isize) as usize * width + x
                    };
                    s += wt * src[at];
                }
                out[y * width + x] = s.floor();
            }
        }
        out
    };
    let a: Vec<f32> = pixels.iter().map(|&v| f32::from(v)).collect();
    let h = pass(&a, true);
    pass(&h, false).into_iter().map(|v| v as u8).collect()
}

/// The filled texture before the blur: [`HIDDEN`] everywhere, [`EXPLORED`], [`VISIBLE`] over the content, then 0 where `cleared`.
#[must_use]
pub fn fill(explored: &Bitmap, visible: &Bitmap, cleared: Option<&Bitmap>) -> Vec<u8> {
    let (cw, ch) = (explored.width, explored.height);
    let width = cw + 2 * PADDING;
    let mut raw = vec![HIDDEN; width * (ch + 2 * PADDING)];
    for y in 0..ch {
        for x in 0..cw {
            let t = &mut raw[(y + PADDING) * width + x + PADDING];
            if explored.get(x, y) {
                *t = EXPLORED;
            }
            if visible.get(x, y) {
                *t = VISIBLE;
            }
            if cleared.is_some_and(|c| c.get(x, y)) {
                *t = 0;
            }
        }
    }
    raw
}

/// The shroud of `faction` (none: [`default_faction`]) at the start, from `map_data.esf` and the startpos.
///
/// The turn-1 model (fitted to the game's captures; docs/formats/campaign_shroud.md): visible = the faction's own characters' lines of
/// sight + the region and settlement lines of sight of the regions owned by the faction or its military allies + the faction's own
/// sea-region lines of sight; explored = the first shroud bitmap of the faction and its allies + for the regions owned by the faction,
/// its allies and the allies' vassals, and their neighbours (not `att_reg_terra_incognita`, `att_sea_lake`): their hex cells and their
/// region lines of sight; the faction's third bitmap cleared to 0.
///
/// # Errors
///
/// A file does not decode, the startpos has no shroud bitmaps, or the faction is not in the startpos.
pub fn build(map_data: &[u8], startpos: &[u8], faction: Option<&str>) -> Result<CampaignShroud, String> {
    let hex = HexMap::read(map_data)?;
    let nbs = neighbours(map_data)?;
    let esf = ESF::decode(&mut Cursor::new(startpos), &None).map_err(|e| format!("startpos.esf: {e}"))?;
    let root = as_record(esf.root_node()).ok_or("startpos.esf: no root record")?;
    let diplomacy = Diplomacy::read(root)?;
    let faction = match faction {
        Some(f) => f.to_owned(),
        None => default_faction(&faction_infos(root)).map(|i| i.key.clone()).ok_or("startpos.esf: no factions")?,
    };
    let fid = diplomacy.faction_id(&faction).ok_or_else(|| format!("{faction} is not a faction of the startpos"))?;

    let mut g = Gathered::default();
    gather(root, "", Ctx::default(), &mut g);
    let content = g
        .shrouds
        .values()
        .flatten()
        .chain(g.los.iter().map(|l| &l.tree))
        .map(|t| (t.width, t.height))
        .next()
        .ok_or("startpos.esf: no shroud bitmaps")?;
    let mut notes = vec![];
    let world = if content == (1357, 1112) {
        MAIN_ATTILA_WORLD
    } else {
        notes.push("ASSUMPTION: the world bounds are the content's size / 2 (only main_attila_map's are known)".to_owned());
        (content.0 as f32 / 2.0, content.1 as f32 / 2.0)
    };

    // The faction's group (itself and its military allies) and the group's vassals, by key.
    let stances_of = |key: &str| -> Vec<(u32, Stance)> {
        let ids: HashSet<u32> = diplomacy.factions.iter().filter(|f| f.key == key).map(|f| f.id).collect();
        diplomacy.stances.iter().filter(|((from, _), _)| ids.contains(from)).map(|(&(_, to), &s)| (to, s)).collect()
    };
    let key_of = |id: u32| diplomacy.factions.iter().find(|f| f.id == id && !f.key.is_empty()).map(|f| f.key.clone());
    let mut group: HashSet<String> = HashSet::from([faction.clone()]);
    group.extend(stances_of(&faction).into_iter().filter(|(_, s)| *s == Stance::MilitaryAllies).filter_map(|(j, _)| key_of(j)));
    let vassals: HashSet<String> =
        group.iter().flat_map(|k| stances_of(k)).filter(|(_, s)| *s == Stance::Master).filter_map(|(j, _)| key_of(j)).collect();
    let mut allies: Vec<&String> = group.iter().filter(|k| **k != faction).collect();
    allies.sort();
    let mut vs: Vec<&String> = vassals.iter().collect();
    vs.sort();
    notes.push(format!("allies: {allies:?}; vassals: {vs:?}"));

    let owned_by = |region: &str, set: &dyn Fn(&str) -> bool| g.owners.get(region).is_some_and(|o| set(o));
    let in_group = |k: &str| group.contains(k);
    let mut errors = 0;
    let mut or = |t: &QuadTree, b: &mut Bitmap| {
        if t.or_into(b).is_err() {
            errors += 1;
        }
    };

    // Visible.
    let mut visible = Bitmap::new(content.0, content.1);
    let mut counts = [0usize; 3];
    for l in &g.los {
        let on = match l.owner {
            LosOwner::Character => l.faction == Some(faction.as_str()),
            LosOwner::Region | LosOwner::Garrison => l.region.is_some_and(|r| owned_by(r, &in_group)),
            LosOwner::SeaRegion => l.sea_faction == Some(fid),
        };
        if on {
            or(&l.tree, &mut visible);
            counts[match l.owner {
                LosOwner::Character => 0,
                LosOwner::SeaRegion => 2,
                _ => 1,
            }] += 1;
        }
    }
    notes.push(format!("visible: {} characters', {} region and settlement, {} sea-region lines of sight", counts[0], counts[1], counts[2]));

    // Explored.
    let mut explored = Bitmap::new(content.0, content.1);
    for k in &group {
        if let Some(first) = g.shrouds.get(k.as_str()).and_then(|t| t.first()) {
            or(first, &mut explored);
        }
    }
    let index: HashMap<&str, usize> = hex.regions.iter().enumerate().rev().map(|(i, k)| (k.as_str(), i)).collect();
    let owners = |o: &str| group.contains(o) || vassals.contains(o);
    let owned: HashSet<usize> = g.owners.iter().filter(|(_, o)| owners(o)).filter_map(|(r, _)| index.get(r).copied()).collect();
    let skip: HashSet<usize> = SKIPPED_REGIONS.iter().filter_map(|k| index.get(k).copied()).collect();
    let area: HashSet<usize> = owned
        .iter()
        .copied()
        .chain(owned.iter().flat_map(|&i| nbs.get(i).into_iter().flatten().copied()))
        .filter(|i| !skip.contains(i))
        .collect();
    let regions = texel_regions(&hex, content);
    for (b, r) in explored.bits.iter_mut().zip(&regions) {
        if area.contains(&usize::from(*r)) {
            *b = true;
        }
    }
    let names: HashSet<&str> = area.iter().filter_map(|&i| hex.regions.get(i).map(String::as_str)).collect();
    for l in g.los.iter().filter(|l| l.owner == LosOwner::Region && l.region.is_some_and(|r| names.contains(r))) {
        or(&l.tree, &mut explored);
    }
    notes.push(format!("explored: {} owned regions, {} with their neighbours", owned.len(), area.len()));

    let mut cleared = Bitmap::new(content.0, content.1);
    let third = g.shrouds.get(faction.as_str()).and_then(|t| t.get(2));
    if let Some(t) = third {
        or(t, &mut cleared);
    }
    if !g.shrouds.contains_key(faction.as_str()) {
        notes.push(format!("{faction} has no CAMPAIGN_SHROUD"));
    }
    if errors > 0 {
        notes.push(format!("{errors} quadtrees did not decode (their bits so far kept)"));
    }

    let raw = fill(&explored, &visible, third.map(|_| &cleared));
    let (width, height) = (content.0 + 2 * PADDING, content.1 + 2 * PADDING);
    Ok(CampaignShroud {
        campaign: String::new(),
        faction,
        width,
        height,
        pixels: blur(&raw, width, height),
        content,
        padding: PADDING,
        world,
        notes,
    })
}

/// The shroud of `map` for `faction` (none: the default) at the start, its startpos found as [`start::find`] does: `list` the paths
/// under a prefix, `read` a path's bytes (the packs' files), `startpos` a startpos path's bytes.
///
/// # Errors
///
/// No startpos or `map_data.esf`, or as [`build`].
pub fn read(
    schema: &Schema,
    map: &str,
    faction: Option<&str>,
    list: &dyn Fn(&str) -> Vec<String>,
    read: &dyn Fn(&str) -> Option<Vec<u8>>,
    startpos: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<CampaignShroud, String> {
    let start = start::find(schema, map, list, read, startpos)?;
    let map_data = read(&format!("campaign_maps/{map}/map_data.esf")).ok_or("no map_data.esf")?;
    let mut s = build(&map_data, &start.startpos, faction)?;
    s.campaign = start.campaign;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(b: &Bitmap) -> Vec<(usize, usize)> {
        (0..b.height).flat_map(|y| (0..b.width).map(move |x| (x, y))).filter(|&(x, y)| b.get(x, y)).collect()
    }

    #[test]
    fn empty_and_full() {
        // All four children empty, then all full.
        assert!(set(&decode_quadtree(&[0b1010_1010], 4, 4, 4).unwrap()).is_empty());
        assert_eq!(set(&decode_quadtree(&[0xff], 4, 4, 4).unwrap()).len(), 16);
    }

    #[test]
    fn child_order() {
        // Child k full alone: NW (0, half), NE (half, half), SW (0, 0), SE (half, 0); row 0 south.
        for (k, corner) in [(0, (0, 2)), (1, (2, 2)), (2, (0, 0)), (3, (2, 0))] {
            let byte = (0..4).fold(0u8, |b, j| b | (if j == k { 0b11 } else { 0b10 }) << (2 * j));
            let b = decode_quadtree(&[byte], 4, 4, 4).unwrap();
            assert_eq!(set(&b), vec![corner, (corner.0 + 1, corner.1), (corner.0, corner.1 + 1), (corner.0 + 1, corner.1 + 1)]);
        }
    }

    #[test]
    fn subdivided_and_clipped() {
        // Root: NW subdivided, the rest empty; the NW node: its SE full. Size 8, so the NW quadrant is x 0..4, y 4..8 and its SE
        // quadrant x 2..4, y 4..6. Clipped to 3 x 5.
        let root = 0b10_10_10_01;
        let nw = 0b11_10_10_10;
        let b = decode_quadtree(&[root, nw], 3, 5, 8).unwrap();
        assert_eq!(set(&b), vec![(2, 4)]);
    }

    #[test]
    fn leaf_bit_order() {
        // A leaf at the root of a 16 square: 2 x 2 texels a bit; bit 0 the south-west cell, bit 9 the cell (1, 1), bit 63 (7, 7).
        let m: u64 = 1 | 1 << 9 | 1 << 63;
        let mut blob = vec![0];
        blob.extend_from_slice(&m.to_le_bytes());
        let b = decode_quadtree(&blob, 16, 16, 16).unwrap();
        let s = set(&b);
        assert_eq!(s.len(), 12);
        for p in [(0, 0), (1, 1), (2, 2), (3, 3), (14, 14), (15, 15)] {
            assert!(s.contains(&p), "{p:?}");
        }
        assert!(!b.get(2, 0));
    }

    #[test]
    fn bad_blobs() {
        assert!(decode_quadtree(&[0b01], 4, 4, 4).is_err());
        assert!(decode_quadtree(&[0, 1, 2], 4, 4, 4).is_err());
        assert!(decode_quadtree(&[0xff, 0], 4, 4, 4).is_err());
    }

    #[test]
    fn blur_keeps_constants_near() {
        let w = blur_weights();
        assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        let out = blur(&[128; 40 * 30], 40, 30);
        assert!(out.iter().all(|&v| (126..=128).contains(&v)));
    }
}
