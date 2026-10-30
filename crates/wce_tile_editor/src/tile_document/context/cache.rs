//! The map context's templates on disk (`<settings>/map_context/<key>.bin`): opening a tile takes a tenth of a second to seconds, reading what
//! it made back a few milliseconds. The key hashes the tile folder's files (path, size, time), the terrain look, the trees switch, the
//! game's kit the trees are generated with when the tiles' own kit has no parameters, and the format version; a changed tile gets a new key. The game files the tiles name (models, effects) are not in the key: `clear` empties it.
//!
//! Layout: "WCECTX" + u16 version, then lz4 (size prepended) of: the extent, the near grid, the material, the entities. Little endian; a
//! string is u32 length + UTF-8.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use wce_foundation::math::Float4x4;
use wce_scene::render_scene::{Decal, MeshInstance, Payload, PointLight, TerrainImage, TerrainMaterial, VfxEmitter, Water, WaterVertex};

use super::super::{MATERIAL_IDS, NORMAL_IDS, next_id};
use super::template::{Grid, Template};
use crate::terrain_bake::TerrainLook;

const MAGIC: &[u8; 6] = b"WCECTX";
const VERSION: u16 = 3;

/// The folder of the cache.
pub fn cache_dir() -> PathBuf {
    wce_foundation::settings::Paths::standard().cache_dir().join("map_context")
}

/// Removes every cached template; returns how many there were.
pub fn clear(dir: &Path) -> usize {
    let mut n = 0;
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if e.path().extension().is_some_and(|x| x == "bin") && std::fs::remove_file(e.path()).is_ok() {
            n += 1;
        }
    }

    n
}

/// Removes the cached templates of another format (an older version's: no reader of this one opens them again). How many and their bytes.
pub fn prune(dir: &Path) -> (usize, u64) {
    let (mut n, mut bytes) = (0, 0);
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = e.path();
        if path.extension().is_none_or(|x| x != "bin") {
            continue;
        }

        let mut head = [0u8; 8];
        let current = std::fs::File::open(&path).and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head)).is_ok()
            && &head[..6] == MAGIC
            && u16::from_le_bytes([head[6], head[7]]) == VERSION;
        if !current {
            let size = e.metadata().map_or(0, |m| m.len());
            if std::fs::remove_file(&path).is_ok() {
                (n, bytes) = (n + 1, bytes + size);
            }
        }
    }

    (n, bytes)
}

fn fnv(h: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(h, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01B3))
}

/// The cache key of a tile folder for a look, its trees generated with `game_kit`'s parameters when there is one.
pub fn key(dir: &Path, look: &TerrainLook, trees: bool, game_kit: Option<&Path>) -> u64 {
    let mut files = vec![];
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let Ok(md) = e.metadata() else { continue };
            if md.is_dir() {
                stack.push(e.path());
            } else {
                let time = md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
                files.push((e.path().to_string_lossy().to_lowercase(), md.len(), time));
            }
        }
    }

    files.sort();
    let mut h = fnv(0xCBF2_9CE4_8422_2325, &VERSION.to_le_bytes());
    h = fnv(h, dir.to_string_lossy().to_lowercase().as_bytes());
    for (p, len, time) in &files {
        h = fnv(h, p.as_bytes());
        h = fnv(h, &len.to_le_bytes());
        h = fnv(h, &time.to_le_bytes());
    }

    // Only then: the keys of the tiles under a kit with parameters stay the same.
    if let Some(kit) = game_kit {
        h = fnv(h, kit.to_string_lossy().to_lowercase().as_bytes());
    }

    fnv(h, format!("{look:?} {trees}").as_bytes())
}

pub fn path(dir: &Path, key: u64) -> PathBuf {
    dir.join(format!("{key:016x}.bin"))
}

// ---- writing

#[derive(Default)]
struct W(Vec<u8>);

impl W {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }

    fn bool(&mut self, v: bool) {
        self.u8(u8::from(v));
    }

    fn u32(&mut self, v: u32) {
        self.0.extend(v.to_le_bytes());
    }

    fn u64(&mut self, v: u64) {
        self.0.extend(v.to_le_bytes());
    }

    fn i32(&mut self, v: i32) {
        self.0.extend(v.to_le_bytes());
    }

    fn f32(&mut self, v: f32) {
        self.0.extend(v.to_le_bytes());
    }

    fn f64(&mut self, v: f64) {
        self.0.extend(v.to_le_bytes());
    }

    fn f32s(&mut self, v: &[f32]) {
        self.u32(v.len() as u32);
        for &x in v {
            self.f32(x);
        }
    }

    fn str(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.0.extend(s.as_bytes());
    }

    fn image(&mut self, img: &TerrainImage) {
        self.u32(img.w);
        self.u32(img.h);
        match &img.rgba {
            Some(px) => {
                self.bool(true);
                self.u32(px.len() as u32);
                self.0.extend(px.iter());
            }

            None => self.bool(false),
        }
    }

    fn matrix(&mut self, m: &Float4x4) {
        for row in &m.m {
            for &v in row {
                self.f32(v);
            }
        }
    }
}

fn write_material(w: &mut W, m: &TerrainMaterial) {
    let TerrainMaterial {
        x0, z0, x1, z1, y0, y1, blend, index, normal, alpha, normal_id: _, normal_dirty: _, blend_id: _, blend_dirty: _, diffuse, normal_tex, pixel_scale, outfield_scale, mid_strength, smoothness, reflectivity,
        detail_colour, detail_normal, campaign: _, id: _,
    } = m;
    for v in [x0, z0, x1, z1, y0, y1] {
        w.f64(*v);
    }

    w.image(blend);
    w.image(index);
    w.image(normal);
    w.image(alpha);
    for s in diffuse.iter().chain(normal_tex) {
        w.str(s);
    }

    for v in pixel_scale.iter().chain(outfield_scale).chain(mid_strength).chain(smoothness).chain(reflectivity) {
        w.f32(*v);
    }

    w.str(detail_colour);
    w.str(detail_normal);
}

fn write_payload(w: &mut W, p: &Payload) -> bool {
    match p {
        Payload::MeshInstance(MeshInstance { model, casts_shadow, lod, p4, p5, p6, p7, burning: _ }) => {
            w.u8(1);
            w.str(model);
            w.bool(*casts_shadow);
            w.bool(*lod);
            for v in p4.iter().chain(p5).chain(p6).chain(p7) {
                w.f32(*v);
            }
        }

        Payload::Decal(Decal { model, tiled, opacity, parallax }) => {
            w.u8(2);
            w.str(model);
            w.bool(*tiled);
            w.f32(*opacity);
            w.f32(*parallax);
        }

        Payload::PointLight(PointLight { colour, colour_scale, colour_min, radius, smooth, animation, speed, phase }) => {
            w.u8(3);
            for v in colour {
                w.f32(*v);
            }

            w.f32(*colour_scale);
            w.f32(*colour_min);
            w.f32(*radius);
            w.bool(*smooth);
            w.u8(*animation);
            w.f32(speed[0]);
            w.f32(speed[1]);
            w.f32(*phase);
        }

        Payload::VfxEmitter(VfxEmitter { effect, scale, mesh }) => {
            w.u8(4);
            w.str(effect);
            w.f32(*scale);
            match mesh {
                Some(m) => {
                    w.bool(true);
                    w.f32s(m.as_flattened());
                }

                None => w.bool(false),
            }
        }

        Payload::Water(Water { verts, indices, river, foam, version }) => {
            w.u8(5);
            w.u32(verts.len() as u32);
            for WaterVertex { x, y, z, across, u, v, flow, fade } in verts {
                for f in [x, y, z, across, u, v, flow, fade] {
                    w.f32(*f);
                }
            }

            w.u32(indices.len() as u32);
            for &i in indices {
                w.u32(i);
            }

            w.bool(*river);
            w.f32(*foam);
            w.u64(*version);
        }

        _ => return false,
    }

    true
}

/// The template's bytes.
pub fn encode(t: &Template) -> Vec<u8> {
    let mut w = W::default();
    w.f64(t.extent.0);
    w.f64(t.extent.1);
    w.i32(t.own.nx);
    w.i32(t.own.nz);
    w.f32(t.own.ox);
    w.f32(t.own.oz);
    w.f32(t.own.step);
    w.f32s(&t.own.heights);
    match &t.material {
        Some(m) => {
            w.bool(true);
            write_material(&mut w, m);
        }

        None => w.bool(false),
    }

    let kept: Vec<usize> = (0..t.entities.len()).filter(|&i| matches!(t.entities[i].1, Payload::MeshInstance(_) | Payload::Decal(_) | Payload::PointLight(_) | Payload::VfxEmitter(_) | Payload::Water(_))).collect();
    w.u32(kept.iter().filter(|&&i| i < t.mid).count() as u32);
    w.u32(kept.len() as u32);
    for &i in &kept {
        let (m, p) = &t.entities[i];
        w.matrix(m);
        write_payload(&mut w, p);
    }

    let mut out = MAGIC.to_vec();
    out.extend(VERSION.to_le_bytes());
    out.extend(lz4_flex::compress_prepend_size(&w.0));
    out
}

// ---- reading

struct R<'a> {
    b: &'a [u8],
    at: usize,
}

impl R<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let s = self.b.get(self.at..self.at + n).ok_or("map context cache: truncated")?;
        self.at += n;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn bool(&mut self) -> Result<bool, String> {
        Ok(self.u8()? != 0)
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }

    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().expect("8 bytes")))
    }

    fn i32(&mut self) -> Result<i32, String> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }

    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }

    fn f64(&mut self) -> Result<f64, String> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().expect("8 bytes")))
    }

    fn f32s(&mut self) -> Result<Vec<f32>, String> {
        let n = self.u32()? as usize;
        Ok(self.take(n * 4)?.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect())
    }

    fn str(&mut self) -> Result<String, String> {
        let n = self.u32()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|e| e.to_string())
    }

    fn image(&mut self) -> Result<TerrainImage, String> {
        let (w, h) = (self.u32()?, self.u32()?);
        let rgba = if self.bool()? {
            let n = self.u32()? as usize;
            Some(Arc::new(self.take(n)?.to_vec()))
        } else {
            None
        };

        Ok(TerrainImage { w, h, rgba })
    }

    fn matrix(&mut self) -> Result<Float4x4, String> {
        let mut m = Float4x4::IDENTITY;
        for r in 0..4 {
            for c in 0..4 {
                m.m[r][c] = self.f32()?;
            }
        }

        Ok(m)
    }

    fn f32_array<const N: usize>(&mut self) -> Result<[f32; N], String> {
        let mut a = [0.0; N];
        for v in &mut a {
            *v = self.f32()?;
        }

        Ok(a)
    }
}

fn read_material(r: &mut R) -> Result<TerrainMaterial, String> {
    let mut m = TerrainMaterial { x0: r.f64()?, z0: r.f64()?, x1: r.f64()?, z1: r.f64()?, y0: r.f64()?, y1: r.f64()?, ..Default::default() };
    m.blend = r.image()?;
    m.index = r.image()?;
    m.normal = r.image()?;
    m.alpha = r.image()?;
    for k in 0..32 {
        let s = r.str()?;
        if k < 16 { m.diffuse[k] = s } else { m.normal_tex[k - 16] = s }
    }

    m.pixel_scale = r.f32_array()?;
    m.outfield_scale = r.f32_array()?;
    m.mid_strength = r.f32_array()?;
    m.smoothness = r.f32_array()?;
    m.reflectivity = r.f32_array()?;
    m.detail_colour = r.str()?;
    m.detail_normal = r.str()?;
    m.id = next_id(&MATERIAL_IDS);
    m.normal_id = next_id(&NORMAL_IDS);
    Ok(m)
}

fn read_payload(r: &mut R) -> Result<Payload, String> {
    Ok(match r.u8()? {
        1 => {
            let (model, casts_shadow, lod) = (r.str()?, r.bool()?, r.bool()?);
            Payload::MeshInstance(MeshInstance { model, casts_shadow, lod, p4: r.f32_array()?, p5: r.f32_array()?, p6: r.f32_array()?, p7: r.f32_array()?, burning: false })
        }

        2 => Payload::Decal(Decal { model: r.str()?, tiled: r.bool()?, opacity: r.f32()?, parallax: r.f32()? }),
        3 => Payload::PointLight(PointLight {
            colour: r.f32_array()?, colour_scale: r.f32()?, colour_min: r.f32()?, radius: r.f32()?, smooth: r.bool()?, animation: r.u8()?, speed: r.f32_array()?, phase: r.f32()?,
        }),
        4 => {
            let (effect, scale) = (r.str()?, r.f32()?);
            let mesh = if r.bool()? { Some(Arc::new(r.f32s()?.as_chunks::<9>().0.to_vec())) } else { None };
            Payload::VfxEmitter(VfxEmitter { effect, scale, mesh })
        }

        5 => {
            let n = r.u32()? as usize;
            let mut verts = Vec::with_capacity(n);
            for _ in 0..n {
                let [x, y, z, across, u, v, flow, fade] = r.f32_array()?;
                verts.push(WaterVertex { x, y, z, across, u, v, flow, fade });
            }

            let n = r.u32()? as usize;
            let indices = (0..n).map(|_| r.u32()).collect::<Result<Vec<_>, _>>()?;
            Payload::Water(Water { verts, indices, river: r.bool()?, foam: r.f32()?, version: r.u64()? })
        }

        k => return Err(format!("map context cache: unknown entity kind {k}")),
    })
}

/// A template from its bytes (`finish`ed).
pub fn decode(bytes: &[u8]) -> Result<Template, String> {
    if bytes.len() < 8 || &bytes[..6] != MAGIC || u16::from_le_bytes([bytes[6], bytes[7]]) != VERSION {
        return Err("map context cache: another format".to_owned());
    }

    let body = lz4_flex::decompress_size_prepended(&bytes[8..]).map_err(|e| format!("map context cache: {e}"))?;
    let r = &mut R { b: &body, at: 0 };
    let extent = (r.f64()?, r.f64()?);
    let (nx, nz, ox, oz, step) = (r.i32()?, r.i32()?, r.f32()?, r.f32()?, r.f32()?);
    let heights = r.f32s()?;
    if heights.len() != (nx.max(0) * nz.max(0)) as usize {
        return Err("map context cache: bad heights".to_owned());
    }

    let material = if r.bool()? { Some(Arc::new(read_material(r)?)) } else { None };
    let (mid, n) = (r.u32()? as usize, r.u32()? as usize);
    let mut entities = Vec::with_capacity(n);
    for _ in 0..n {
        let m = r.matrix()?;
        entities.push((m, read_payload(r)?));
    }

    let mut t = Template { extent, own: Grid { nx, nz, ox, oz, step, heights: Arc::new(heights) }, material, entities, mid, ..Default::default() };
    t.finish();
    Ok(t)
}

/// Reads a cached template.
pub fn read(dir: &Path, key: u64) -> Option<Template> {
    let bytes = std::fs::read(path(dir, key)).ok()?;
    decode(&bytes).ok()
}

/// Writes a template (through a temporary file, so a reader never sees half of it).
pub fn write(dir: &Path, key: u64, t: &Template) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let (to, tmp) = (path(dir, key), dir.join(format!("{key:016x}.{}.tmp", std::process::id())));
    std::fs::write(&tmp, encode(t)).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &to).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_removes_only_other_formats() {
        let dir = std::env::temp_dir().join(format!("wce_cache_prune_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a scratch folder");
        let mut t = Template { extent: (8.0, 8.0), own: Grid { nx: 2, nz: 2, ox: 0.0, oz: 0.0, step: 2.0, heights: Arc::new(vec![0.0; 4]) }, ..Default::default() };
        t.finish();
        write(&dir, 1, &t).expect("written");
        let mut old = MAGIC.to_vec();
        old.extend((VERSION - 1).to_le_bytes());
        std::fs::write(path(&dir, 2), &old).expect("an old one");
        std::fs::write(dir.join("other.txt"), b"kept").expect("another file");
        assert_eq!(prune(&dir).0, 1);
        assert!(read(&dir, 1).is_some() && !path(&dir, 2).exists() && dir.join("other.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_template_reads_back_as_written() {
        let mut m = TerrainMaterial { x0: 1.0, z1: 7.0, y0: -5.0, detail_colour: "a.dds".to_owned(), ..Default::default() };
        m.blend = TerrainImage { w: 2, h: 1, rgba: Some(Arc::new(vec![1, 2, 3, 4, 5, 6, 7, 8])) };
        m.diffuse[3] = "d3.dds".to_owned();
        m.smoothness[15] = 0.5;
        let mut t = Template {
            extent: (128.0, 256.0), own: Grid { nx: 2, nz: 3, ox: -8.0, oz: -8.0, step: 2.0, heights: Arc::new(vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0]) }, material: Some(Arc::new(m)),
            entities: vec![
                (Float4x4::IDENTITY, Payload::MeshInstance(MeshInstance { model: "b.rigid_model_v2".to_owned(), ..Default::default() })),
                (Float4x4::IDENTITY, Payload::Water(Water { verts: vec![WaterVertex { x: 1.0, fade: 0.5, ..Default::default() }], indices: vec![0, 0, 0], ..Default::default() })),
                (Float4x4::IDENTITY, Payload::PointLight(PointLight { colour: [1.0, 0.5, 0.25], colour_scale: 2.0, colour_min: 0.0, radius: 9.0, smooth: true, animation: 2, speed: [1.0, 2.0], phase: 0.3 })),
                (Float4x4::IDENTITY, Payload::VfxEmitter(VfxEmitter { effect: "fire".to_owned(), scale: 1.5, mesh: Some(Arc::new(vec![[1.0; 9]])) })),
                (Float4x4::IDENTITY, Payload::Decal(Decal { model: "d".to_owned(), tiled: true, opacity: 0.7, parallax: 0.1 })),
            ],
            mid: 2,
            ..Default::default()
        };

        t.finish();
        let back = decode(&encode(&t)).expect("decodes");
        assert_eq!(back.extent, t.extent);
        assert_eq!((*back.own.heights).clone(), (*t.own.heights).clone());
        assert_eq!((back.own.ox, back.near.nx), (t.own.ox, t.near.nx));
        assert_eq!(back.mid, 2);
        assert_eq!(back.entities.iter().map(|e| &e.1).collect::<Vec<_>>(), t.entities.iter().map(|e| &e.1).collect::<Vec<_>>());
        let (a, b) = (back.material.as_ref().unwrap(), t.material.as_ref().unwrap());
        assert_eq!((a.x0, a.z1, a.y0, &a.blend, &a.diffuse, a.smoothness, &a.detail_colour), (b.x0, b.z1, b.y0, &b.blend, &b.diffuse, b.smoothness, &b.detail_colour));
    }
}
