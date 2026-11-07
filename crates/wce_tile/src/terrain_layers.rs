//! Terrain layers (docs/plans/procedural_roads.md), as the newer Terry's: height, blend and ground type maps made by composition. Each type
//! has a base and an ordered list of layers, applied in order over it.
//!
//! The tile's `HeightField`, `BlendField` and `GroundTypeField` keep holding the terrain as it shows, the composite, so that nothing reading
//! them has to know about layers: not the renderer, nor a kit save, which is how TEd gets the terrain the layers make. A [`Stack`] sits beside
//! each field. Nothing edits a layer directly either: every brush goes on editing the field, and [`Stack::absorb`] then finds where the field
//! differs from what the stack composes and takes the difference into the working layer, or the base without one.
//!
//! Undo goes by the stack's state, not the field's records: a stroke into a hidden layer leaves the field as it was, so they could not say
//! what it did. Each edit's record has a serial (`top_id`); [`Stack::sync`] keeps the state after it under that serial, and when the field
//! comes back to a serial it knows (an undo, a redo) it puts that state back. Samples are in blocks shared between states, so a state costs
//! what the edit touched. A change of the stack itself is its own undo entry ([`AnyState`]).
//!
//! The rules are read from Warhammer III's kit (`nuln_map_reduced_wh1`). ASSUMPTION: they are inferred from the layers' data; nothing compared
//! them with what Terry builds. A height layer adds (0 is nothing there), a blend layer goes over what is below by its weights (all 0 is
//! nothing), and a ground type layer replaces (Terry's 16 is nothing there, an alpha of 0 here: the field keeps colours, not numbers).
//!
//! A stack lives in `terrain_layers.wcl` beside the tile: TEd never sees it, and a tile without layers has no such file.

use std::collections::HashMap;
use std::sync::Arc;

use wce_formats::png_file::{deflate_bytes, inflate_bytes};

use crate::blend_field::BlendField;
use crate::ground_type_field::GroundTypeField;
use crate::height_field::HeightField;

/// The file of a tile's layers.
pub const FILE: &str = "terrain_layers.wcl";

const MAGIC: &[u8; 4] = b"WCTL";
const VERSION: u32 = 2;

/// Pixels a side of a block of samples.
const BLOCK: u32 = 64;

/// The states a stack keeps, past which the oldest go (their edits then undo only as far as the field's records say).
const STATES: usize = 512;

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

    /// The rectangle edited since the last call.
    fn take_edited(field: &mut Self::Field) -> Option<(u32, u32, u32, u32)>;

    /// The serial of the last edit undo would take back, 0 with none.
    fn top_id(field: &Self::Field) -> u64;

    /// Applies a layer's pixel over `out`.
    fn over(out: &mut [Self::Sample], layer: &[Self::Sample], opacity: f32);

    /// Sets a layer's pixel so that it turns `below` into `new`. `shown` is what the stack composed before the edit.
    fn absorb(layer: &mut [Self::Sample], below: &[Self::Sample], shown: &[Self::Sample], new: &[Self::Sample], opacity: f32);

    /// Adds an edit to a pixel that is not what shows (a hidden layer, or one with layers over it): what the edit did to what is shown, on top of what
    /// the pixel holds. Taken from `new` itself, the pixel would keep what those layers over it put there, and a road's texture stayed after the road.
    fn onto(layer: &mut [Self::Sample], shown: &[Self::Sample], new: &[Self::Sample], opacity: f32);

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

    fn take_edited(field: &mut HeightField) -> Option<(u32, u32, u32, u32)> {
        field.take_edited()
    }

    fn top_id(field: &HeightField) -> u64 {
        field.top_id()
    }

    fn over(out: &mut [f32], layer: &[f32], opacity: f32) {
        out[0] += layer[0] * opacity;
    }

    fn absorb(layer: &mut [f32], _: &[f32], shown: &[f32], new: &[f32], opacity: f32) {
        layer[0] += (new[0] - shown[0]) / opacity.max(0.01);
    }

    fn onto(layer: &mut [f32], shown: &[f32], new: &[f32], opacity: f32) {
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

    fn take_edited(field: &mut BlendField) -> Option<(u32, u32, u32, u32)> {
        field.take_edited()
    }

    fn top_id(field: &BlendField) -> u64 {
        field.top_id()
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

    fn onto(layer: &mut [u16], shown: &[u16], new: &[u16], opacity: f32) {
        let mut edit = [0u16; 16];
        Self::absorb(&mut edit, shown, shown, new, opacity);
        let cover = (edit.iter().map(|&v| f32::from(v)).sum::<f32>() / 65535.0).min(1.0);
        let full = layer.iter().map(|&v| u32::from(v)).sum::<u32>() > 65000;
        for (l, &e) in layer.iter_mut().zip(&edit) {
            *l = (f32::from(*l) * (1.0 - cover) + f32::from(e)).round().min(65535.0) as u16;
        }

        // A whole pixel (the base's) stays one: what the rounding left goes on its strongest channel.
        if full {
            let sum = layer.iter().map(|&v| i64::from(v)).sum::<i64>();
            if let Some(top) = layer.iter_mut().max() {
                *top = (i64::from(*top) + 65535 - sum).clamp(0, 65535) as u16;
            }
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

    fn take_edited(field: &mut GroundTypeField) -> Option<(u32, u32, u32, u32)> {
        field.take_edited()
    }

    fn top_id(field: &GroundTypeField) -> u64 {
        field.top_id()
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

    fn onto(layer: &mut [u8], shown: &[u8], new: &[u8], _: f32) {
        if shown[..3] != new[..3] {
            layer[..3].copy_from_slice(&new[..3]);
            layer[3] = 255;
        }
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

/// Samples by block, those of a block nobody touched left out. Most of a layer is empty, and a whole blend layer is 50 MB.
#[derive(Clone, Debug, PartialEq)]
struct Blocks<T> {
    across: u32,
    blocks: Vec<Option<Arc<Vec<T>>>>,
}

impl<T: Copy + Default> Blocks<T> {
    fn new(w: u32, h: u32) -> Self {
        let (across, down) = (w.div_ceil(BLOCK), h.div_ceil(BLOCK));
        Self { across, blocks: vec![None; across as usize * down as usize] }
    }

    fn at(x: u32, y: u32, across: u32, channels: usize) -> (usize, usize) {
        (((y / BLOCK) * across + x / BLOCK) as usize, ((y % BLOCK) * BLOCK + x % BLOCK) as usize * channels)
    }

    fn pixel(&self, x: u32, y: u32, channels: usize) -> Option<&[T]> {
        let (b, i) = Self::at(x, y, self.across, channels);
        self.blocks[b].as_ref().map(|s| &s[i..i + channels])
    }

    /// A pixel to change: its block made if it was not there, copied if a state shares it.
    fn pixel_mut(&mut self, x: u32, y: u32, channels: usize) -> &mut [T] {
        let (b, i) = Self::at(x, y, self.across, channels);
        let block = self.blocks[b].get_or_insert_with(|| Arc::new(vec![T::default(); (BLOCK * BLOCK) as usize * channels]));
        &mut Arc::make_mut(block)[i..i + channels]
    }

    fn bytes(&self) -> usize {
        self.blocks.iter().flatten().map(|b| b.len() * size_of::<T>()).sum()
    }
}

/// A layer.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer<T> {
    pub id: u64,
    pub name: String,
    pub visible: bool,
    pub opacity: f32,

    /// The procedural feature that writes it (a road), empty for a layer of the user's. Such a layer is not edited by hand.
    pub owner: String,
    samples: Blocks<T>,
}

impl<T: Copy + Default> Layer<T> {
    /// The bytes in memory of its samples.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.samples.bytes()
    }
}

/// What a stack holds at a moment. Cheap: the samples are shared.
#[derive(Clone, Debug, PartialEq)]
pub struct State<T> {
    base: Blocks<T>,
    layers: Vec<Layer<T>>,
    working: Option<u64>,
}

/// A map's base and layers.
pub struct Stack<K: Kind> {
    w: u32,
    h: u32,
    base: Blocks<K::Sample>,
    pub layers: Vec<Layer<K::Sample>>,

    /// The layer edits go to, by id; none: the base.
    pub working: Option<u64>,

    /// The state after each edit, by the serial of its record in the field, and the serial last seen.
    states: HashMap<u64, State<K::Sample>>,
    seen: u64,
}

impl<K: Kind> Stack<K> {
    /// A stack with no layers over what a field holds.
    pub fn new(field: &K::Field) -> Self {
        let (w, h) = K::size(field);
        let mut base = Blocks::new(w, h);
        let mut px = vec![K::Sample::default(); K::CHANNELS];
        for y in 0..h {
            for x in 0..w {
                K::read(field, x, y, &mut px);
                base.pixel_mut(x, y, K::CHANNELS).copy_from_slice(&px);
            }
        }

        let seen = K::top_id(field);
        let mut s = Self { w, h, base, layers: vec![], working: None, states: HashMap::new(), seen };
        s.states.insert(seen, s.state());
        s
    }

    /// A new layer; its id. A layer of the user's goes on top of theirs, under what procedural features own: those apply last.
    pub fn add(&mut self, id: u64, name: &str, owner: &str) -> u64 {
        let samples = Blocks::new(self.w, self.h);
        let at = if owner.is_empty() { self.layers.iter().position(|l| !l.owner.is_empty()).unwrap_or(self.layers.len()) } else { self.layers.len() };
        self.layers.insert(at, Layer { id, name: name.to_owned(), visible: true, opacity: 1.0, owner: owner.to_owned(), samples });
        id
    }

    /// A layer of the user's, by id: what a procedural feature owns is not theirs to touch.
    #[must_use]
    pub fn user_index(&self, id: u64) -> Option<usize> {
        self.index_of(id).filter(|&i| self.layers[i].owner.is_empty())
    }

    /// Whether what procedural features own applies after everything else.
    #[must_use]
    pub fn owned_on_top(&self) -> bool {
        !self.layers.windows(2).any(|w| !w[0].owner.is_empty() && w[1].owner.is_empty())
    }

    /// Makes it so.
    pub fn put_owned_on_top(&mut self) {
        self.layers.sort_by_key(|l| !l.owner.is_empty());
    }

    #[must_use]
    pub fn index_of(&self, id: u64) -> Option<usize> {
        self.layers.iter().position(|l| l.id == id)
    }

    #[must_use]
    pub fn state(&self) -> State<K::Sample> {
        State { base: self.base.clone(), layers: self.layers.clone(), working: self.working }
    }

    /// Puts a change of the stack itself in or takes it out (an undo entry of its own): the state, its composite in the field.
    pub fn put_state(&mut self, state: &State<K::Sample>, field: &mut K::Field) {
        (self.base, self.layers, self.working) = (state.base.clone(), state.layers.clone(), state.working);
        self.remember();
        self.apply_all(field);
    }

    /// Keeps the state under the serial last seen: a later undo comes back to it.
    pub fn remember(&mut self) {
        self.states.insert(self.seen, self.state());
        if self.states.len() > STATES {
            let oldest = self.states.keys().copied().min().unwrap_or(0);
            self.states.remove(&oldest);
        }
    }

    /// Follows the field: a new edit goes into the stack, and coming back to an edit it knows puts that state back (an undo, a redo; the
    /// working layer stays the one chosen). Whether anything changed. Not while an edit is open: its record does not exist yet.
    pub fn sync(&mut self, field: &mut K::Field) -> bool {
        let Some(r) = K::take_edited(field) else { return false };
        let id = K::top_id(field);
        if id != self.seen
            && let Some(state) = self.states.get(&id).cloned()
        {
            self.seen = id;
            let working = self.working;
            (self.base, self.layers) = (state.base, state.layers);
            self.working = working.filter(|&w| self.index_of(w).is_some());
            self.apply_all(field);
            return true;
        }

        self.seen = id;
        let changed = self.absorb(field, [r.0, r.1, r.2, r.3]);
        self.remember();
        changed
    }

    /// What lies under a layer at a pixel: what a procedural feature works from.
    pub fn below(&self, id: u64, x: u32, y: u32, out: &mut [K::Sample]) {
        self.compose(x, y, self.index_of(id).unwrap_or(self.layers.len()), out);
    }

    /// Whether any visible layer whose owner starts with `owner` has a pixel at a place (`empty`: what a layer's pixel holds where it has nothing).
    pub fn owned_at(&self, owner: &str, x: u32, y: u32, empty: impl Fn(&[K::Sample]) -> bool) -> bool {
        x < self.w && y < self.h && self.layers.iter().any(|l| {
            l.visible && l.owner.starts_with(owner) && l.samples.pixel(x, y, K::CHANNELS).is_some_and(|p| !empty(p))
        })
    }

    /// Empties a layer, for a procedural feature to fill.
    pub fn clear_layer(&mut self, id: u64) {
        if let Some(i) = self.index_of(id) {
            self.layers[i].samples = Blocks::new(self.w, self.h);
        }
    }

    /// Sets a pixel of a layer. Not an edit: [`Self::apply_all`] then shows it.
    pub fn put(&mut self, id: u64, x: u32, y: u32, pixel: &[K::Sample]) {
        if let Some(i) = self.index_of(id) {
            self.layers[i].samples.pixel_mut(x, y, K::CHANNELS).copy_from_slice(pixel);
        }
    }

    /// The composite at a pixel from the base up to, not including, layer `end`.
    fn compose(&self, x: u32, y: u32, end: usize, out: &mut [K::Sample]) {
        match self.base.pixel(x, y, K::CHANNELS) {
            Some(px) => out.copy_from_slice(px),
            None => out.fill(K::Sample::default()),
        }

        for layer in self.layers[..end].iter().filter(|l| l.visible && l.opacity > 0.0) {
            if let Some(px) = layer.samples.pixel(x, y, K::CHANNELS) {
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

    /// Takes an edit of the field over a rectangle into the working layer, or the base with none. Whether anything differed. A hidden layer
    /// takes it too and it does not show, which the panel warns of: paint going elsewhere would be worse. The field then goes back to what
    /// the stack composes: what layers above cover, what a hidden layer holds.
    pub fn absorb(&mut self, field: &mut K::Field, rect: [u32; 4]) -> bool {
        let target = self.working.and_then(|id| self.index_of(id));
        let (x1, y1) = (rect[2].min(self.w), rect[3].min(self.h));
        let (mut shown, mut new, mut below) = (vec![K::Sample::default(); K::CHANNELS], vec![K::Sample::default(); K::CHANNELS], vec![K::Sample::default(); K::CHANNELS]);
        let mut bounds: Option<[u32; 4]> = None;
        for y in rect[1]..y1 {
            for x in rect[0]..x1 {
                self.compose(x, y, self.layers.len(), &mut shown);
                K::read(field, x, y, &mut new);
                if K::same(&shown, &new) {
                    continue;
                }

                bounds = Some(bounds.map_or([x, y, x + 1, y + 1], |b| [b[0].min(x), b[1].min(y), b[2].max(x + 1), b[3].max(y + 1)]));
                let shows = |l: &Layer<K::Sample>| l.visible && l.opacity > 0.0;
                match target {
                    Some(k) if shows(&self.layers[k]) && self.layers[k + 1..].iter().any(shows) => {
                        let opacity = self.layers[k].opacity;
                        K::onto(self.layers[k].samples.pixel_mut(x, y, K::CHANNELS), &shown, &new, opacity);
                    }

                    Some(k) if shows(&self.layers[k]) => {
                        self.compose(x, y, k, &mut below);
                        let opacity = self.layers[k].opacity;
                        K::absorb(self.layers[k].samples.pixel_mut(x, y, K::CHANNELS), &below, &shown, &new, opacity);
                    }

                    Some(k) => K::onto(self.layers[k].samples.pixel_mut(x, y, K::CHANNELS), &shown, &new, 1.0),
                    None if self.layers.iter().any(shows) => K::onto(self.base.pixel_mut(x, y, K::CHANNELS), &shown, &new, 1.0),
                    None => K::rebase(self.base.pixel_mut(x, y, K::CHANNELS), &shown, &new),
                }
            }
        }

        let Some(bounds) = bounds else { return false };
        self.apply(field, bounds);
        true
    }

    /// Merges a layer into the one below it, or into the base. A merged layer applies its opacity for good; a hidden one goes.
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

        let mut px = vec![K::Sample::default(); K::CHANNELS];
        for y in 0..self.h {
            for x in 0..self.w {
                let Some(l) = top.samples.pixel(x, y, K::CHANNELS) else { continue };
                px.copy_from_slice(l);
                let target = match index.checked_sub(1) {
                    Some(under) => &mut self.layers[under].samples,
                    None => &mut self.base,
                };

                K::over(target.pixel_mut(x, y, K::CHANNELS), &px, top.opacity);
            }
        }
    }

    fn write(&self, out: &mut Vec<u8>) {
        for v in [self.w, self.h, self.layers.len() as u32] {
            out.extend(v.to_le_bytes());
        }

        out.extend(self.working.unwrap_or(0).to_le_bytes());
        write_blocks::<K>(out, &self.base);
        for l in &self.layers {
            out.extend(l.id.to_le_bytes());
            put_text(out, &l.name);
            put_text(out, &l.owner);
            out.push(u8::from(l.visible));
            out.extend(l.opacity.to_le_bytes());
            write_blocks::<K>(out, &l.samples);
        }
    }

    fn read(r: &mut Reader) -> Result<Self, String> {
        let (w, h, n) = (r.u32()?, r.u32()?, r.u32()?);
        let working = Some(r.u64()?).filter(|&id| id != 0);
        let base = read_blocks::<K>(r, w, h)?;
        let mut layers = vec![];
        for _ in 0..n {
            let (id, name, owner, visible, opacity) = (r.u64()?, r.text()?, r.text()?, r.take(1)?[0] != 0, r.f32()?);
            let samples = read_blocks::<K>(r, w, h)?;
            layers.push(Layer { id, name, visible, opacity, owner, samples });
        }

        Ok(Self { w, h, base, layers, working, states: HashMap::new(), seen: 0 })
    }
}

fn write_blocks<K: Kind>(out: &mut Vec<u8>, b: &Blocks<K::Sample>) {
    out.extend((b.blocks.iter().flatten().count() as u32).to_le_bytes());
    for (i, block) in b.blocks.iter().enumerate() {
        if let Some(block) = block {
            out.extend((i as u32).to_le_bytes());
            block.iter().for_each(|&s| K::put(out, s));
        }
    }
}

fn read_blocks<K: Kind>(r: &mut Reader, w: u32, h: u32) -> Result<Blocks<K::Sample>, String> {
    let mut b = Blocks::new(w, h);
    for _ in 0..r.u32()? {
        let i = r.u32()? as usize;
        let bytes = r.take((BLOCK * BLOCK) as usize * K::CHANNELS * K::BYTES)?;
        let slot = b.blocks.get_mut(i).ok_or("terrain layers: a block out of its map")?;
        *slot = Some(Arc::new(bytes.chunks_exact(K::BYTES).map(K::get).collect()));
    }

    Ok(b)
}

/// A stack's state, of whichever type: what an undo entry of a change of the stack keeps.
#[derive(Clone)]
pub enum AnyState {
    Height(State<f32>),
    Blend(State<u16>),
    Ground(State<u8>),
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

    /// The state before a change still in progress (an opacity being dragged), which its undo entry goes back to.
    pub pending: Option<AnyState>,
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
        assert_eq!(s.layers[0].bytes(), 0);
        s.layers[0].visible = false;
        s.apply_all(&mut f);
        assert_eq!(f.metres(2, 2), 90.0);
    }

    #[test]
    fn nothing_differs_nothing_grows() {
        let (mut f, mut s) = stack();
        s.working = Some(s.add(1, "hills", ""));
        assert!(!s.absorb(&mut f, [0, 0, 4, 4]));
        assert_eq!(s.layers[0].bytes(), 0);
    }

    #[test]
    fn a_layer_keeps_only_the_blocks_it_was_touched_in() {
        let (mut f, mut s) = (HeightField::flat(200, 200, 0.0), Stack::<Height>::new(&HeightField::flat(200, 200, 0.0)));
        s.working = Some(s.add(1, "hills", ""));
        edit(&mut f, 190, 190, 3.0);
        s.absorb(&mut f, [0, 0, 200, 200]);
        assert_eq!(s.layers[0].samples.blocks.iter().flatten().count(), 1);
        edit(&mut f, 1, 2, 1.0);
        s.absorb(&mut f, [0, 0, 200, 200]);
        assert_eq!(s.layers[0].samples.blocks.iter().flatten().count(), 2);
        assert_eq!(s.layers[0].samples.pixel(190, 190, 1), Some(&[3.0f32][..]));
    }

    #[test]
    fn a_hidden_layer_takes_the_paint_and_it_does_not_show() {
        let (mut f, mut s) = stack();
        s.working = Some(s.add(1, "hills", ""));
        edit(&mut f, 1, 1, 104.0);
        s.sync(&mut f);
        s.layers[0].visible = false;
        s.apply_all(&mut f);
        edit(&mut f, 1, 1, 102.0);
        s.sync(&mut f);
        assert_eq!(f.metres(1, 1), 100.0);
        s.layers[0].visible = true;
        s.apply_all(&mut f);
        assert_eq!(f.metres(1, 1), 106.0);
    }

    #[test]
    fn what_a_feature_owns_stays_on_top_of_the_users_layers() {
        let (_, mut s) = stack();
        s.add(1, "road", "road:1#0");
        s.add(2, "mine", "");
        s.add(3, "mine too", "");
        assert_eq!(s.layers.iter().map(|l| l.id).collect::<Vec<_>>(), [2, 3, 1]);
        assert!(s.owned_on_top());
        assert_eq!(s.user_index(1), None);
        assert_eq!(s.user_index(3), Some(1));

        // A tile saved before they had to be.
        s.layers.swap(1, 2);
        assert!(!s.owned_on_top());
        s.put_owned_on_top();
        assert!(s.owned_on_top());
    }

    #[test]
    fn a_height_edit_under_a_layer_lands_where_it_was_made() {
        let (mut f, mut s) = stack();
        s.working = Some(s.add(1, "mine", ""));
        s.add(2, "road", "road:1#0");
        s.layers[0].opacity = 0.5;
        edit(&mut f, 1, 1, 104.0);
        s.absorb(&mut f, [0, 0, 4, 4]);
        assert!((f.metres(1, 1) - 104.0).abs() < 0.001);
    }

    #[test]
    fn paint_over_a_layer_does_not_keep_that_layer_when_it_goes() {
        // The base all of channel 0, a road's channel 5 over half of it.
        let mut base = [0u16; 16];
        base[0] = 65535;
        let mut road = [0u16; 16];
        road[5] = 32768;
        let mut shown = base;
        Blend::over(&mut shown, &road, 1.0);

        // A stroke turns what shows half into channel 2.
        let mut new = shown.map(|v| v / 2);
        new[2] = 65535 - new.iter().map(|&v| u32::from(v)).sum::<u32>() as u16;
        Blend::onto(&mut base, &shown, &new, 1.0);
        assert!(base[5] <= 2, "{base:?}");
        assert!(base[2] > 30000, "{base:?}");
        assert_eq!(base.iter().map(|&v| u32::from(v)).sum::<u32>(), 65535, "{base:?}");
    }

    #[test]
    fn a_hidden_blend_layer_keeps_what_it_held() {
        let mut layer = [0u16; 16];
        layer[1] = 65535;
        let mut shown = [0u16; 16];
        shown[0] = 65535;

        // A quarter of channel 2 painted over what shows, none of which is this layer's.
        let mut new = [0u16; 16];
        (new[0], new[2]) = (49151, 16384);
        Blend::onto(&mut layer, &shown, &new, 1.0);
        assert!(layer[1].abs_diff(49151) <= 2 && layer[2].abs_diff(16384) <= 2, "{layer:?}");

        // Ground types: the same, and what was not painted stays.
        let mut layer = [7u8, 7, 7, 255];
        Ground::onto(&mut layer, &[1, 1, 1, 255], &[1, 1, 1, 255], 1.0);
        assert_eq!(layer, [7, 7, 7, 255]);
    }

    #[test]
    fn undo_and_redo_follow_the_stack_even_into_a_hidden_layer() {
        let (mut f, mut s) = stack();
        s.working = Some(s.add(1, "hills", ""));
        s.remember();
        s.layers[0].visible = false;
        s.apply_all(&mut f);
        s.remember();
        edit(&mut f, 1, 1, 105.0);
        s.sync(&mut f);
        assert_eq!(s.layers[0].samples.pixel(1, 1, 1), Some(&[5.0f32][..]));

        f.undo();
        s.sync(&mut f);
        assert!(s.layers[0].samples.pixel(1, 1, 1).is_none());
        f.redo();
        s.sync(&mut f);
        assert_eq!(s.layers[0].samples.pixel(1, 1, 1), Some(&[5.0f32][..]));
        assert_eq!(f.metres(1, 1), 100.0);
    }

    #[test]
    fn a_new_edit_after_an_undo_is_a_new_edit() {
        let (mut f, mut s) = stack();
        s.working = Some(s.add(1, "hills", ""));
        s.remember();
        edit(&mut f, 1, 1, 105.0);
        s.sync(&mut f);
        f.undo();
        s.sync(&mut f);
        edit(&mut f, 2, 2, 103.0);
        s.sync(&mut f);
        assert!(s.layers[0].samples.pixel(1, 1, 1).is_none_or(|p| p[0] == 0.0));
        assert_eq!(s.layers[0].samples.pixel(2, 2, 1), Some(&[3.0f32][..]));
    }

    #[test]
    fn a_change_of_the_stack_goes_back_and_a_later_undo_finds_it() {
        let (mut f, mut s) = stack();
        s.working = Some(s.add(1, "hills", ""));
        s.remember();
        edit(&mut f, 1, 1, 105.0);
        s.sync(&mut f);

        // Hidden as an undo entry of its own, then a stroke on top.
        let before = s.state();
        s.layers[0].visible = false;
        s.apply_all(&mut f);
        let after = s.state();
        s.put_state(&after, &mut f);
        s.working = None;
        edit(&mut f, 2, 2, 90.0);
        s.sync(&mut f);

        // The stroke goes: the layer is still hidden.
        f.undo();
        s.sync(&mut f);
        assert!(!s.layers[0].visible);
        assert_eq!(f.metres(2, 2), 100.0);
        s.put_state(&before, &mut f);
        assert!(s.layers[0].visible);
        assert_eq!(f.metres(1, 1), 105.0);
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
