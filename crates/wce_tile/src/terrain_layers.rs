//! Terrain layers (docs/plans/procedural_roads.md), as the newer Terry's: height, blend and ground type maps made by composition. Each type
//! has a base and an ordered list of layers, applied in order over it.
//!
//! The tile's `HeightField`, `BlendField` and `GroundTypeField` keep holding the terrain as it shows, the composite, so that nothing reading
//! them has to know about layers: not the renderer, nor a kit save, which is how TEd gets the terrain the layers make. A [`Stack`] sits beside
//! each field. Nothing edits a layer directly either: every brush goes on editing the field, and [`Stack::absorb`] then finds where the field
//! differs from what the stack composes and takes the difference into the working layer, or the base without one. That holds for undo too.
//!
//! The rules are read from Warhammer III's kit (`nuln_map_reduced_wh1`). ASSUMPTION: they are inferred from the layers' data; nothing compared
//! them with what Terry builds. A height layer adds (0 is nothing there), a blend layer goes over what is below by its weights (all 0 is
//! nothing), and a ground type layer replaces (Terry's 16 is nothing there, an alpha of 0 here: the field keeps colours, not numbers).
//!
//! A stack lives in `terrain_layers.wcl` beside the tile: TEd never sees it, and a tile without layers has no such file.

use wce_formats::png_file::{deflate_bytes, inflate_bytes};

use crate::blend_field::BlendField;
use crate::ground_type_field::GroundTypeField;
use crate::height_field::HeightField;

/// The file of a tile's layers.
pub const FILE: &str = "terrain_layers.wcl";

const MAGIC: &[u8; 4] = b"WCTL";
const VERSION: u32 = 1;

/// What a layer of a type does to a pixel, and how a pixel of a field is read and written.
pub trait Kind {
    type Sample: Copy + Default + PartialEq;

    /// Samples a pixel, in a layer and in the base.
    const CHANNELS: usize;
    type Field;

    fn read(field: &Self::Field, x: u32, y: u32, out: &mut [Self::Sample]);

    /// Writes the composite: not an edit, and no one is told of it as one.
    fn write(field: &mut Self::Field, x: u32, y: u32, pixel: &[Self::Sample]);
    fn size(field: &Self::Field) -> (u32, u32);

    /// Applies a layer's pixel over `out`.
    fn over(out: &mut [Self::Sample], layer: &[Self::Sample], opacity: f32);

    /// Sets a layer's pixel so that it turns `below` into `new`. `shown` is what the stack composed before the edit.
    fn absorb(layer: &mut [Self::Sample], below: &[Self::Sample], shown: &[Self::Sample], new: &[Self::Sample], opacity: f32);

    /// Changes a pixel of the base after an edit with no working layer.
    fn rebase(base: &mut [Self::Sample], shown: &[Self::Sample], new: &[Self::Sample]);

    /// Whether two pixels are the same, within what the field's own rounding leaves.
    fn same(a: &[Self::Sample], b: &[Self::Sample]) -> bool;
    fn put(out: &mut Vec<u8>, s: Self::Sample);
    fn get(bytes: &[u8]) -> Self::Sample;
    const BYTES: usize;
}

pub struct Height;

impl Kind for Height {
    type Sample = f32;
    type Field = HeightField;
    const CHANNELS: usize = 1;
    const BYTES: usize = 4;

    fn read(field: &HeightField, x: u32, y: u32, out: &mut [f32]) {
        out[0] = field.metres(x, y) as f32;
    }

    fn write(field: &mut HeightField, x: u32, y: u32, pixel: &[f32]) {
        field.compose_metres(x, y, f64::from(pixel[0]));
    }

    fn size(field: &HeightField) -> (u32, u32) {
        (field.width(), field.height())
    }

    fn over(out: &mut [f32], layer: &[f32], opacity: f32) {
        out[0] += layer[0] * opacity;
    }

    fn absorb(layer: &mut [f32], _: &[f32], shown: &[f32], new: &[f32], opacity: f32) {
        layer[0] += (new[0] - shown[0]) / opacity.max(0.01);
    }

    fn rebase(base: &mut [f32], shown: &[f32], new: &[f32]) {
        base[0] += new[0] - shown[0];
    }

    fn same(a: &[f32], b: &[f32]) -> bool {
        (a[0] - b[0]).abs() < 0.001
    }

    fn put(out: &mut Vec<u8>, s: f32) {
        out.extend(s.to_le_bytes());
    }

    fn get(bytes: &[u8]) -> f32 {
        f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }
}

pub struct Blend;

impl Kind for Blend {
    type Sample = u16;
    type Field = BlendField;
    const CHANNELS: usize = 16;
    const BYTES: usize = 2;

    fn read(field: &BlendField, x: u32, y: u32, out: &mut [u16]) {
        out.copy_from_slice(field.pixel(x, y));
    }

    fn write(field: &mut BlendField, x: u32, y: u32, pixel: &[u16]) {
        field.compose_pixel(x, y, pixel);
    }

    fn size(field: &BlendField) -> (u32, u32) {
        (field.width(), field.height())
    }

    fn over(out: &mut [u16], layer: &[u16], opacity: f32) {
        let cover = (layer.iter().map(|&v| f32::from(v)).sum::<f32>() / 65535.0).min(1.0) * opacity;
        if cover <= 0.0 {
            return;
        }

        let mut sum = 0u32;
        let (mut top, mut most) = (0, 0);
        for (k, (o, &l)) in out.iter_mut().zip(layer).enumerate() {
            *o = (f32::from(*o) * (1.0 - cover) + f32::from(l) * opacity).round().min(65535.0) as u16;
            sum += u32::from(*o);
            if *o >= most {
                (top, most) = (k, *o);
            }
        }

        // A pixel sums to 65535 (`blend_field`): what the rounding left goes on its strongest channel.
        out[top] = (i64::from(most) + 65535 - i64::from(sum)).clamp(0, 65535) as u16;
    }

    fn absorb(layer: &mut [u16], below: &[u16], _: &[u16], new: &[u16], opacity: f32) {
        // The least cover that gives `new`: what a channel lost can only come from covering it.
        let mut cover = 0.0f32;
        for (&b, &n) in below.iter().zip(new) {
            if b > n {
                cover = cover.max(f32::from(b - n) / f32::from(b));
            }
        }

        for ((l, &b), &n) in layer.iter_mut().zip(below).zip(new) {
            let v = (f32::from(n) - f32::from(b) * (1.0 - cover)) / opacity.max(0.01);
            *l = v.round().clamp(0.0, 65535.0) as u16;
        }
    }

    fn rebase(base: &mut [u16], _: &[u16], new: &[u16]) {
        base.copy_from_slice(new);
    }

    fn same(a: &[u16], b: &[u16]) -> bool {
        a.iter().zip(b).all(|(&x, &y)| x.abs_diff(y) <= 2)
    }

    fn put(out: &mut Vec<u8>, s: u16) {
        out.extend(s.to_le_bytes());
    }

    fn get(bytes: &[u8]) -> u16 {
        u16::from_le_bytes([bytes[0], bytes[1]])
    }
}

pub struct Ground;

impl Kind for Ground {
    type Sample = u8;
    type Field = GroundTypeField;
    const CHANNELS: usize = 4;
    const BYTES: usize = 1;

    fn read(field: &GroundTypeField, x: u32, y: u32, out: &mut [u8]) {
        let [r, g, b] = field.rgb(x, y);
        out.copy_from_slice(&[r, g, b, 255]);
    }

    fn write(field: &mut GroundTypeField, x: u32, y: u32, pixel: &[u8]) {
        field.compose_rgb(x, y, [pixel[0], pixel[1], pixel[2]]);
    }

    fn size(field: &GroundTypeField) -> (u32, u32) {
        (field.width(), field.height())
    }

    fn over(out: &mut [u8], layer: &[u8], opacity: f32) {
        if layer[3] > 0 && opacity >= 0.5 {
            out[..3].copy_from_slice(&layer[..3]);
        }
    }

    fn absorb(layer: &mut [u8], below: &[u8], _: &[u8], new: &[u8], _: f32) {
        layer[..3].copy_from_slice(&new[..3]);
        layer[3] = if below[..3] == new[..3] { 0 } else { 255 };
    }

    fn rebase(base: &mut [u8], _: &[u8], new: &[u8]) {
        base.copy_from_slice(new);
    }

    fn same(a: &[u8], b: &[u8]) -> bool {
        a[..3] == b[..3]
    }

    fn put(out: &mut Vec<u8>, s: u8) {
        out.push(s);
    }

    fn get(bytes: &[u8]) -> u8 {
        bytes[0]
    }
}

/// A layer: its samples over a rectangle that grows as it is painted. Most of a layer is empty, and a blend layer of a whole tile is 50 MB.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer<T> {
    pub id: u64,
    pub name: String,
    pub visible: bool,
    pub opacity: f32,

    /// The procedural feature that writes it (a road), empty for a layer of the user's. Such a layer is not edited by hand.
    pub owner: String,
    rect: [u32; 4],
    samples: Vec<T>,
}

impl<T: Copy + Default> Layer<T> {
    fn pixel(&self, x: u32, y: u32, channels: usize) -> Option<&[T]> {
        let [x0, y0, x1, y1] = self.rect;
        if x < x0 || x >= x1 || y < y0 || y >= y1 {
            return None;
        }

        let i = ((y - y0) as usize * (x1 - x0) as usize + (x - x0) as usize) * channels;
        Some(&self.samples[i..i + channels])
    }

    /// Makes room for a rectangle.
    fn grow(&mut self, [a, b, c, d]: [u32; 4], channels: usize) {
        let [x0, y0, x1, y1] = self.rect;
        let new = if x1 == x0 { [a, b, c, d] } else { [x0.min(a), y0.min(b), x1.max(c), y1.max(d)] };
        if new == self.rect {
            return;
        }

        let (w, h) = ((new[2] - new[0]) as usize, (new[3] - new[1]) as usize);
        let mut samples = vec![T::default(); w * h * channels];
        for y in y0..y1 {
            let from = (y - y0) as usize * (x1 - x0) as usize * channels;
            let to = ((y - new[1]) as usize * w + (x0 - new[0]) as usize) * channels;
            let n = (x1 - x0) as usize * channels;
            samples[to..to + n].copy_from_slice(&self.samples[from..from + n]);
        }

        (self.rect, self.samples) = (new, samples);
    }

    fn pixel_mut(&mut self, x: u32, y: u32, channels: usize) -> &mut [T] {
        let [x0, y0, x1, _] = self.rect;
        let i = ((y - y0) as usize * (x1 - x0) as usize + (x - x0) as usize) * channels;
        &mut self.samples[i..i + channels]
    }

    /// The bytes in memory of its samples.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.samples.len() * size_of::<T>()
    }
}

/// A map's base and layers.
pub struct Stack<K: Kind> {
    w: u32,
    h: u32,
    base: Vec<K::Sample>,
    pub layers: Vec<Layer<K::Sample>>,

    /// The layer edits go to, by id; none: the base.
    pub working: Option<u64>,
}

impl<K: Kind> Stack<K> {
    /// A stack with no layers over what a field holds.
    pub fn new(field: &K::Field) -> Self {
        let (w, h) = K::size(field);
        let mut base = vec![K::Sample::default(); w as usize * h as usize * K::CHANNELS];
        for (i, pixel) in base.chunks_exact_mut(K::CHANNELS).enumerate() {
            K::read(field, i as u32 % w, i as u32 / w, pixel);
        }

        Self { w, h, base, layers: vec![], working: None }
    }

    /// A new layer on top; its id.
    pub fn add(&mut self, id: u64, name: &str, owner: &str) -> u64 {
        self.layers.push(Layer { id, name: name.to_owned(), visible: true, opacity: 1.0, owner: owner.to_owned(), rect: [0; 4], samples: vec![] });
        id
    }

    #[must_use]
    pub fn index_of(&self, id: u64) -> Option<usize> {
        self.layers.iter().position(|l| l.id == id)
    }

    /// The composite at a pixel from the base up to, not including, layer `end`.
    fn compose(&self, x: u32, y: u32, end: usize, out: &mut [K::Sample]) {
        let i = (y as usize * self.w as usize + x as usize) * K::CHANNELS;
        out.copy_from_slice(&self.base[i..i + K::CHANNELS]);
        for layer in self.layers[..end].iter().filter(|l| l.visible && l.opacity > 0.0) {
            if let Some(px) = layer.pixel(x, y, K::CHANNELS) {
                K::over(out, px, layer.opacity);
            }
        }
    }

    /// Writes the composite over a rectangle into the field.
    pub fn apply(&self, field: &mut K::Field, rect: [u32; 4]) {
        let mut out = vec![K::Sample::default(); K::CHANNELS];
        for y in rect[1]..rect[3].min(self.h) {
            for x in rect[0]..rect[2].min(self.w) {
                self.compose(x, y, self.layers.len(), &mut out);
                K::write(field, x, y, &out);
            }
        }
    }

    /// Writes the composite into the whole field.
    pub fn apply_all(&self, field: &mut K::Field) {
        self.apply(field, [0, 0, self.w, self.h]);
    }

    /// Takes an edit of the field over a rectangle into the working layer (a hidden or empty-handed one is left out: the base takes it). Whether
    /// anything differed. Where layers above cover the edit, the field goes back to what they make.
    pub fn absorb(&mut self, field: &mut K::Field, rect: [u32; 4]) -> bool {
        let target = self.working.and_then(|id| self.index_of(id)).filter(|&i| self.layers[i].visible && self.layers[i].opacity > 0.0);
        let (x1, y1) = (rect[2].min(self.w), rect[3].min(self.h));
        let (mut shown, mut new, mut below) = (vec![K::Sample::default(); K::CHANNELS], vec![K::Sample::default(); K::CHANNELS], vec![K::Sample::default(); K::CHANNELS]);

        // The rectangle of what differs, so that a layer grows no further than it was touched.
        let mut edits = vec![];
        for y in rect[1]..y1 {
            for x in rect[0]..x1 {
                self.compose(x, y, self.layers.len(), &mut shown);
                K::read(field, x, y, &mut new);
                if !K::same(&shown, &new) {
                    edits.push((x, y));
                }
            }
        }

        let Some(bounds) = edits.iter().fold(None, |b: Option<[u32; 4]>, &(x, y)| {
            Some(b.map_or([x, y, x + 1, y + 1], |b| [b[0].min(x), b[1].min(y), b[2].max(x + 1), b[3].max(y + 1)]))
        }) else { return false };

        if let Some(k) = target {
            self.layers[k].grow(bounds, K::CHANNELS);
        }

        for (x, y) in edits {
            self.compose(x, y, self.layers.len(), &mut shown);
            K::read(field, x, y, &mut new);
            match target {
                Some(k) => {
                    self.compose(x, y, k, &mut below);
                    let opacity = self.layers[k].opacity;
                    K::absorb(self.layers[k].pixel_mut(x, y, K::CHANNELS), &below, &shown, &new, opacity);
                }

                None => {
                    let i = (y as usize * self.w as usize + x as usize) * K::CHANNELS;
                    K::rebase(&mut self.base[i..i + K::CHANNELS], &shown, &new);
                }
            }
        }

        // What the layers above leave of it, and the rounding of the rest.
        self.apply(field, bounds);
        true
    }

    /// Merges a layer into the one below it, or into the base. The composite stays as it was only for a visible, fully opaque stack: a
    /// merged layer applies its opacity for good.
    pub fn merge_down(&mut self, index: usize) {
        if index >= self.layers.len() {
            return;
        }

        let top = self.layers.remove(index);
        if self.working == Some(top.id) {
            self.working = None;
        }

        if !top.visible {
            return;
        }

        let [x0, y0, x1, y1] = top.rect;
        let mut px = vec![K::Sample::default(); K::CHANNELS];
        for y in y0..y1 {
            for x in x0..x1 {
                let Some(l) = top.pixel(x, y, K::CHANNELS) else { continue };
                match index.checked_sub(1) {
                    Some(under) => {
                        self.layers[under].grow(top.rect, K::CHANNELS);
                        let o = self.layers[under].pixel_mut(x, y, K::CHANNELS);
                        K::over(o, l, top.opacity);
                    }

                    None => {
                        let i = (y as usize * self.w as usize + x as usize) * K::CHANNELS;
                        px.copy_from_slice(l);
                        K::over(&mut self.base[i..i + K::CHANNELS], &px, top.opacity);
                    }
                }
            }
        }
    }

    fn write(&self, out: &mut Vec<u8>) {
        for v in [self.w, self.h, self.layers.len() as u32] {
            out.extend(v.to_le_bytes());
        }

        out.extend(self.working.unwrap_or(0).to_le_bytes());
        self.base.iter().for_each(|&s| K::put(out, s));
        for l in &self.layers {
            out.extend(l.id.to_le_bytes());
            put_text(out, &l.name);
            put_text(out, &l.owner);
            out.push(u8::from(l.visible));
            out.extend(l.opacity.to_le_bytes());
            l.rect.iter().for_each(|v| out.extend(v.to_le_bytes()));
            l.samples.iter().for_each(|&s| K::put(out, s));
        }
    }

    fn read(r: &mut Reader) -> Result<Self, String> {
        let (w, h, n) = (r.u32()?, r.u32()?, r.u32()?);
        let working = Some(r.u64()?).filter(|&id| id != 0);
        let samples = |r: &mut Reader, count: usize| -> Result<Vec<K::Sample>, String> {
            Ok(r.take(count * K::BYTES)?.chunks_exact(K::BYTES).map(K::get).collect())
        };

        let base = samples(r, w as usize * h as usize * K::CHANNELS)?;
        let mut layers = vec![];
        for _ in 0..n {
            let (id, name, owner, visible, opacity) = (r.u64()?, r.text()?, r.text()?, r.take(1)?[0] != 0, r.f32()?);
            let rect = [r.u32()?, r.u32()?, r.u32()?, r.u32()?];
            let count = rect[2].saturating_sub(rect[0]) as usize * rect[3].saturating_sub(rect[1]) as usize * K::CHANNELS;
            let samples = samples(r, count)?;
            layers.push(Layer { id, name, visible, opacity, owner, rect, samples });
        }

        Ok(Self { w, h, base, layers, working })
    }
}

fn put_text(out: &mut Vec<u8>, s: &str) {
    out.extend((s.len() as u32).to_le_bytes());
    out.extend(s.as_bytes());
}

struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if n > self.bytes.len() {
            return Err("terrain layers: the file ends early".to_owned());
        }

        let (head, tail) = self.bytes.split_at(n);
        self.bytes = tail;
        Ok(head)
    }

    fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_bits(self.u32()?))
    }

    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from(self.u32()?) | u64::from(self.u32()?) << 32)
    }

    fn text(&mut self) -> Result<String, String> {
        let n = self.u32()? as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
}

/// A tile's layers: the types it has any of.
#[derive(Default)]
pub struct TerrainLayers {
    pub height: Option<Stack<Height>>,
    pub blend: Option<Stack<Blend>>,
    pub ground: Option<Stack<Ground>>,

    /// Changed since it was last saved.
    pub dirty: bool,
    next_id: u64,
}

impl std::fmt::Debug for TerrainLayers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let n = (self.height.as_ref().map_or(0, |s| s.layers.len()), self.blend.as_ref().map_or(0, |s| s.layers.len()), self.ground.as_ref().map_or(0, |s| s.layers.len()));
        write!(f, "TerrainLayers {n:?}")
    }
}

impl TerrainLayers {
    /// An id for a new layer.
    pub fn new_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// Whether there is nothing to keep.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.height.as_ref().is_none_or(|s| s.layers.is_empty()) && self.blend.as_ref().is_none_or(|s| s.layers.is_empty()) && self.ground.as_ref().is_none_or(|s| s.layers.is_empty())
    }

    /// The file's bytes.
    ///
    /// # Errors
    ///
    /// It could not be compressed.
    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        let mut raw = vec![];
        raw.extend(self.next_id.to_le_bytes());
        raw.push(u8::from(self.height.is_some()));
        if let Some(s) = &self.height {
            s.write(&mut raw);
        }

        raw.push(u8::from(self.blend.is_some()));
        if let Some(s) = &self.blend {
            s.write(&mut raw);
        }

        raw.push(u8::from(self.ground.is_some()));
        if let Some(s) = &self.ground {
            s.write(&mut raw);
        }

        let mut out = MAGIC.to_vec();
        out.extend(VERSION.to_le_bytes());
        out.extend((raw.len() as u64).to_le_bytes());
        out.extend(deflate_bytes(&raw).map_err(|e| e.to_string())?);
        Ok(out)
    }

    /// Reads the file's bytes.
    ///
    /// # Errors
    ///
    /// They are not such a file, or of a newer version.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let mut head = Reader { bytes };
        if head.take(4)? != MAGIC {
            return Err("not a terrain layers file".to_owned());
        }

        let version = head.u32()?;
        if version > VERSION {
            return Err(format!("terrain layers: version {version} is newer than this editor"));
        }

        let size = head.u64()? as usize;
        let mut raw = vec![0u8; size];
        inflate_bytes(head.bytes, &mut raw).map_err(|e| e.to_string())?;
        let mut r = Reader { bytes: &raw };
        let next_id = r.u64()?;
        let mut out = Self { next_id, ..Self::default() };
        if r.take(1)?[0] != 0 {
            out.height = Some(Stack::read(&mut r)?);
        }

        if r.take(1)?[0] != 0 {
            out.blend = Some(Stack::read(&mut r)?);
        }

        if r.take(1)?[0] != 0 {
            out.ground = Some(Stack::read(&mut r)?);
        }

        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack() -> (HeightField, Stack<Height>) {
        let field = HeightField::flat(4, 4, 100.0);
        let s = Stack::new(&field);
        (field, s)
    }

    fn edit(field: &mut HeightField, x: u32, y: u32, metres: f64) {
        field.begin_edit().unwrap();
        field.set_metres(x, y, metres).unwrap();
        field.end_edit();
    }

    #[test]
    fn a_height_layer_takes_the_edit_and_adds() {
        let (mut f, mut s) = stack();
        s.working = Some(s.add(1, "hills", ""));
        edit(&mut f, 1, 1, 110.0);
        assert!(s.absorb(&mut f, [0, 0, 4, 4]));
        assert_eq!(f.metres(1, 1), 110.0);

        // Hidden, the base shows: it never had the edit.
        s.layers[0].visible = false;
        s.apply_all(&mut f);
        assert_eq!(f.metres(1, 1), 100.0);
        s.layers[0].visible = true;
        s.layers[0].opacity = 0.5;
        s.apply_all(&mut f);
        assert_eq!(f.metres(1, 1), 105.0);
    }

    #[test]
    fn an_edit_at_half_opacity_lands_where_it_was_made() {
        let (mut f, mut s) = stack();
        s.working = Some(s.add(1, "hills", ""));
        s.layers[0].opacity = 0.5;
        edit(&mut f, 0, 0, 104.0);
        s.absorb(&mut f, [0, 0, 4, 4]);
        assert!((f.metres(0, 0) - 104.0).abs() < 0.001);
    }

    #[test]
    fn with_no_working_layer_the_base_takes_it() {
        let (mut f, mut s) = stack();
        s.add(1, "hills", "");
        edit(&mut f, 2, 2, 90.0);
        s.absorb(&mut f, [0, 0, 4, 4]);
        assert!(s.layers[0].samples.is_empty());
        s.layers[0].visible = false;
        s.apply_all(&mut f);
        assert_eq!(f.metres(2, 2), 90.0);
    }

    #[test]
    fn nothing_differs_nothing_grows() {
        let (mut f, mut s) = stack();
        s.working = Some(s.add(1, "hills", ""));
        assert!(!s.absorb(&mut f, [0, 0, 4, 4]));
        assert!(s.layers[0].samples.is_empty());
    }

    #[test]
    fn a_layer_only_grows_where_it_is_touched_and_keeps_what_it_had() {
        let (mut f, mut s) = stack();
        s.working = Some(s.add(1, "hills", ""));
        edit(&mut f, 3, 3, 103.0);
        s.absorb(&mut f, [0, 0, 4, 4]);
        assert_eq!(s.layers[0].rect, [3, 3, 4, 4]);
        edit(&mut f, 1, 2, 101.0);
        s.absorb(&mut f, [0, 0, 4, 4]);
        assert_eq!(s.layers[0].rect, [1, 2, 4, 4]);
        assert_eq!(s.layers[0].pixel(3, 3, 1), Some(&[3.0f32][..]));
        assert_eq!(s.layers[0].pixel(1, 2, 1), Some(&[1.0f32][..]));
    }

    #[test]
    fn merging_down_keeps_the_terrain() {
        let (mut f, mut s) = stack();
        s.working = Some(s.add(1, "a", ""));
        edit(&mut f, 1, 1, 105.0);
        s.absorb(&mut f, [0, 0, 4, 4]);
        s.working = Some(s.add(2, "b", ""));
        edit(&mut f, 1, 1, 107.0);
        s.absorb(&mut f, [0, 0, 4, 4]);
        s.merge_down(1);
        assert_eq!(s.layers.len(), 1);
        s.apply_all(&mut f);
        assert_eq!(f.metres(1, 1), 107.0);
        s.merge_down(0);
        assert!(s.layers.is_empty());
        s.apply_all(&mut f);
        assert_eq!(f.metres(1, 1), 107.0);
    }

    #[test]
    fn a_blend_layer_goes_over_by_its_weights_and_keeps_the_sum() {
        let mut out = [65535u16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

        // A quarter of channel 2 over all of channel 0.
        let mut layer = [0u16; 16];
        layer[2] = 16384;
        Blend::over(&mut out, &layer, 1.0);
        assert_eq!(out.iter().map(|&v| u32::from(v)).sum::<u32>(), 65535);
        assert!(out[0].abs_diff(49151) <= 1 && out[2].abs_diff(16384) <= 1, "{out:?}");

        // Nothing there: nothing happens.
        let before = out;
        Blend::over(&mut out, &[0; 16], 1.0);
        assert_eq!(out, before);
    }

    #[test]
    fn a_blend_edit_comes_back_from_its_layer() {
        let mut below = [0u16; 16];
        below[0] = 65535;
        let mut new = [0u16; 16];
        (new[0], new[3]) = (40000, 25535);
        let mut layer = [0u16; 16];
        Blend::absorb(&mut layer, &below, &below, &new, 1.0);
        let mut out = below;
        Blend::over(&mut out, &layer, 1.0);
        assert!(Blend::same(&out, &new), "{out:?}");
    }

    #[test]
    fn a_ground_layer_replaces_where_it_has_a_pixel() {
        let mut out = [1u8, 2, 3, 255];
        Ground::over(&mut out, &[9, 9, 9, 0], 1.0);
        assert_eq!(out, [1, 2, 3, 255]);
        Ground::over(&mut out, &[9, 8, 7, 255], 1.0);
        assert_eq!(out, [9, 8, 7, 255]);

        // The same as below: nothing there, so that the layers under it still show.
        let mut layer = [0u8; 4];
        Ground::absorb(&mut layer, &[4, 4, 4, 255], &[4, 4, 4, 255], &[4, 4, 4, 255], 1.0);
        assert_eq!(layer[3], 0);
    }

    #[test]
    fn the_file_gives_back_what_it_was_given() {
        let (mut f, mut s) = stack();
        let mut l = TerrainLayers::default();
        let id = l.new_id();
        s.working = Some(s.add(id, "Hills ñ", "road:7"));
        edit(&mut f, 1, 2, 104.0);
        s.absorb(&mut f, [0, 0, 4, 4]);
        s.layers[0].opacity = 0.75;
        l.height = Some(s);
        let back = TerrainLayers::from_bytes(&l.to_bytes().unwrap()).unwrap();
        let (a, b) = (l.height.as_ref().unwrap(), back.height.as_ref().unwrap());
        assert_eq!(a.layers, b.layers);
        assert_eq!(a.base, b.base);
        assert_eq!(b.working, Some(id));
        assert!(back.blend.is_none() && back.ground.is_none());
        let mut back = back;
        assert_eq!(back.new_id(), id + 1);
    }

    #[test]
    fn a_bad_file_is_an_error() {
        assert!(TerrainLayers::from_bytes(b"nope").is_err());
        assert!(TerrainLayers::from_bytes(b"WCTL").is_err());
    }
}
