//! `CA_STD::UNORDERED_MAP` as BOB's generator uses it: what it holds and, above all, the order it gives them back in, which decides every
//! random draw of the generator. Buckets are lists; a new key goes to the end of bucket `hash % buckets`; the map starts with 4 buckets and,
//! when `count / buckets > 1.0` after counting a new key, grows to `2 * buckets + 1` (or `ceil(count)` if more), moving the entries bucket by
//! bucket in order. Iteration is bucket by bucket. Erasing doesn't shrink it.
//!
//! CA's position hash (the bits of x xor those of y) puts a grid's points into few buckets (200 000 candidates: 3 % of the buckets, 42 entries
//! looked at per lookup), so the buckets here only keep the order: each is a list linked through a slab of entries, and a key is found
//! through an index of its own (`index`, by the key's exact [`CaKey::id`]), in constant time. The order is the lists'. A lookup scans the
//! key's bucket only, so a key equal to another but hashed apart (-0 and 0) is found only while both share a bucket: the index gives the
//! equal keys ([`CaKey::equal_ids`]), and those in the key's bucket count.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hash, Hasher};

/// A key with CA's hash.
pub trait CaKey: PartialEq + Clone {
    /// The key exactly, as a hashable value; `None` for a key equal to nothing (a position with a NaN).
    type Id: Hash + Eq + Clone;

    fn ca_hash(&self) -> u32;
    fn id(&self) -> Option<Self::Id>;

    /// The other keys [`PartialEq`] calls equal to this one (their ids).
    fn equal_ids(&self) -> Vec<Self::Id> {
        vec![]
    }
}

impl CaKey for String {
    type Id = String;

    /// `CA_STD::HASH<CA::String>`: 0xf73a51c9 xor each (signed) character from the last, shifted by 0, 8, 16, 24, 0...
    fn ca_hash(&self) -> u32 {
        let mut h = 0xf73a_51c9u32;
        let mut shift = 0u32;
        for &c in self.as_bytes().iter().rev() {
            h ^= ((c as i8) as i32 as u32) << shift;
            shift = (shift + 8) & 31;
        }

        h
    }

    fn id(&self) -> Option<String> {
        Some(self.clone())
    }
}

/// A position as BOB keys it (`UTILITYLIB::VECTOR_2`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vec2Key(pub f32, pub f32);

impl CaKey for Vec2Key {
    type Id = (u32, u32);

    /// `CA_STD::HASH<VECTOR_2>`: the bits of x xor the bits of y xor 0x4a545eed.
    fn ca_hash(&self) -> u32 {
        self.0.to_bits() ^ self.1.to_bits() ^ 0x4a54_5eed
    }

    /// The bits; a NaN equals nothing.
    fn id(&self) -> Option<(u32, u32)> {
        (!self.0.is_nan() && !self.1.is_nan()).then(|| (self.0.to_bits(), self.1.to_bits()))
    }

    /// The same position with a zero's sign flipped (-0 equals 0).
    fn equal_ids(&self) -> Vec<(u32, u32)> {
        let flips = |v: f32| if v == 0.0 { vec![v.to_bits(), (-v).to_bits()] } else { vec![v.to_bits()] };
        let Some(own) = self.id() else { return vec![] };
        let mut out = vec![];
        for x in flips(self.0) {
            for y in flips(self.1) {
                if (x, y) != own {
                    out.push((x, y));
                }
            }
        }

        out
    }
}

/// A multiplicative hasher for the index (its order is never used).
#[derive(Default)]
pub struct FastHasher(u64);

impl Hasher for FastHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0.rotate_left(5) ^ u64::from(b)).wrapping_mul(0x51_7cc1_b727_220a_95);
        }
    }

    fn write_u32(&mut self, v: u32) {
        self.0 = (self.0.rotate_left(5) ^ u64::from(v)).wrapping_mul(0x51_7cc1_b727_220a_95);
    }

    fn write_usize(&mut self, v: usize) {
        self.0 = (self.0.rotate_left(5) ^ v as u64).wrapping_mul(0x51_7cc1_b727_220a_95);
    }
}

const NIL: u32 = u32::MAX;

#[derive(Clone, Debug)]
struct Slot<K, V> {
    entry: Option<(K, V)>,
    next: u32,
    prev: u32,
}

#[derive(Clone, Debug)]
pub struct CaMap<K: CaKey, V> {
    /// Each bucket's first and last entry.
    heads: Vec<u32>,
    tails: Vec<u32>,
    slots: Vec<Slot<K, V>>,
    free: Vec<u32>,
    index: HashMap<K::Id, u32, BuildHasherDefault<FastHasher>>,
    count: usize,
}

impl<K: CaKey, V> Default for CaMap<K, V> {
    fn default() -> Self {
        Self::with_buckets(4)
    }
}

impl<K: CaKey, V> CaMap<K, V> {
    pub fn with_buckets(n: usize) -> Self {
        let n = n.max(1);
        Self { heads: vec![NIL; n], tails: vec![NIL; n], slots: vec![], free: vec![], index: HashMap::default(), count: 0 }
    }

    /// The entry a scan of the key's bucket finds first: the key itself, or an equal one in the same bucket.
    fn slot_of(&self, k: &K) -> Option<u32> {
        let exact = self.index.get(&k.id()?).copied();
        let others = k.equal_ids();
        if others.is_empty() {
            return exact;
        }

        let b = (k.ca_hash() % self.heads.len() as u32) as usize;
        let in_bucket = |s: u32| self.slots[s as usize].entry.as_ref().is_some_and(|(kk, _)| (kk.ca_hash() % self.heads.len() as u32) as usize == b);
        let found: Vec<u32> = exact.into_iter().chain(others.iter().filter_map(|id| self.index.get(id).copied())).filter(|&s| in_bucket(s)).collect();
        if found.len() < 2 {
            return found.first().copied();
        }

        let mut s = self.heads[b];
        while s != NIL {
            if found.contains(&s) {
                return Some(s);
            }

            s = self.slots[s as usize].next;
        }

        None
    }

    pub fn get(&self, k: &K) -> Option<&V> {
        self.slot_of(k).and_then(|s| self.slots[s as usize].entry.as_ref()).map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, k: &K) -> Option<&mut V> {
        let s = self.slot_of(k)?;
        self.slots[s as usize].entry.as_mut().map(|(_, v)| v)
    }

    pub fn contains(&self, k: &K) -> bool {
        self.slot_of(k).is_some()
    }

    /// Appends slot `s` to the end of bucket `b`.
    fn link(&mut self, b: usize, s: u32) {
        let tail = self.tails[b];
        self.slots[s as usize].prev = tail;
        self.slots[s as usize].next = NIL;
        if tail == NIL {
            self.heads[b] = s;
        } else {
            self.slots[tail as usize].next = s;
        }

        self.tails[b] = s;
    }

    /// Inserts if the key is new (an existing value stays); the value stored under the key, and whether it was inserted.
    pub fn insert(&mut self, k: K, v: V) -> (&mut V, bool) {
        if let Some(s) = self.slot_of(&k) {
            return (&mut self.slots[s as usize].entry.as_mut().expect("indexed").1, false);
        }

        self.count += 1;
        let n = self.heads.len();
        if (self.count as f32) / (n as f32) > 1.0 {
            let mut want = n * 2 + 1;
            let need = self.count as f32;
            if (want as f32) < need {
                want = need.ceil() as usize;
            }

            // Every entry moves, bucket by bucket in order, to the end of its new bucket.
            let mut order = Vec::with_capacity(self.count);
            for b in 0..n {
                let mut s = self.heads[b];
                while s != NIL {
                    order.push(s);
                    s = self.slots[s as usize].next;
                }
            }

            self.heads = vec![NIL; want];
            self.tails = vec![NIL; want];
            for s in order {
                let b = (self.slots[s as usize].entry.as_ref().expect("linked").0.ca_hash() % want as u32) as usize;
                self.link(b, s);
            }
        }

        let b = (k.ca_hash() % self.heads.len() as u32) as usize;
        let id = k.id();
        let slot = Slot { entry: Some((k, v)), next: NIL, prev: NIL };
        let s = match self.free.pop() {
            Some(s) => {
                self.slots[s as usize] = slot;
                s
            }

            None => {
                self.slots.push(slot);
                (self.slots.len() - 1) as u32
            }
        };

        self.link(b, s);
        if let Some(id) = id {
            self.index.insert(id, s);
        }

        (&mut self.slots[s as usize].entry.as_mut().expect("just set").1, true)
    }

    pub fn remove(&mut self, k: &K) -> bool {
        let Some(s) = self.slot_of(k) else { return false };
        let stored = self.slots[s as usize].entry.as_ref().expect("indexed").0.clone();
        if let Some(id) = stored.id() {
            self.index.remove(&id);
        }

        let (prev, next) = (self.slots[s as usize].prev, self.slots[s as usize].next);
        let b = (stored.ca_hash() % self.heads.len() as u32) as usize;
        if prev == NIL {
            self.heads[b] = next;
        } else {
            self.slots[prev as usize].next = next;
        }

        if next == NIL {
            self.tails[b] = prev;
        } else {
            self.slots[next as usize].prev = prev;
        }

        self.slots[s as usize].entry = None;
        self.free.push(s);
        self.count -= 1;
        true
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        let mut b = 0;
        let mut s = NIL;
        std::iter::from_fn(move || {
            while s == NIL {
                if b >= self.heads.len() {
                    return None;
                }

                s = self.heads[b];
                b += 1;
            }

            let slot = &self.slots[s as usize];
            s = slot.next;
            slot.entry.as_ref().map(|(k, v)| (k, v))
        })
    }

    pub fn keys(&self) -> Vec<K> {
        self.iter().map(|(k, _)| k.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The map as it was first written (buckets of vectors, lookups by scanning a bucket): the order every change must keep.
    struct Reference<K, V> {
        buckets: Vec<Vec<(K, V)>>,
        count: usize,
    }

    impl<K: CaKey, V> Reference<K, V> {
        fn new() -> Self {
            Self { buckets: (0..4).map(|_| Vec::new()).collect(), count: 0 }
        }

        fn bucket_of(&self, k: &K) -> usize {
            (k.ca_hash() % self.buckets.len() as u32) as usize
        }

        fn contains(&self, k: &K) -> bool {
            self.buckets[self.bucket_of(k)].iter().any(|(kk, _)| kk == k)
        }

        fn insert(&mut self, k: K, v: V) -> bool {
            if self.contains(&k) {
                return false;
            }

            self.count += 1;
            let n = self.buckets.len();
            if (self.count as f32) / (n as f32) > 1.0 {
                let mut want = n * 2 + 1;
                let need = self.count as f32;
                if (want as f32) < need {
                    want = need.ceil() as usize;
                }

                let old = std::mem::take(&mut self.buckets);
                self.buckets = (0..want).map(|_| Vec::new()).collect();
                for (kk, vv) in old.into_iter().flatten() {
                    let b = (kk.ca_hash() % want as u32) as usize;
                    self.buckets[b].push((kk, vv));
                }
            }

            let b = self.bucket_of(&k);
            self.buckets[b].push((k, v));
            true
        }

        fn remove(&mut self, k: &K) -> bool {
            let b = self.bucket_of(k);
            if let Some(i) = self.buckets[b].iter().position(|(kk, _)| kk == k) {
                self.buckets[b].remove(i);
                self.count -= 1;
                return true;
            }

            false
        }
    }

    #[test]
    fn string_hash_is_cas() {
        // h = 0xf73a51c9 ^ 'b' ^ ('a' << 8)
        assert_eq!("ab".to_owned().ca_hash(), 0xf73a_51c9 ^ 0x62 ^ (0x61 << 8));
    }

    #[test]
    fn growth_keeps_bucket_order() {
        let mut m = CaMap::<String, usize>::default();
        for i in 0..20 {
            m.insert(format!("k{i}"), i);
        }

        assert_eq!(m.len(), 20);
        assert_eq!(m.heads.len(), 39);
        // Entries of a new bucket keep the order of the old buckets they came from.
        let mut seen: Vec<usize> = m.iter().map(|(_, v)| *v).collect();
        seen.sort_unstable();
        assert_eq!(seen, (0..20).collect::<Vec<_>>());
        let buckets: Vec<u32> = m.iter().map(|(k, _)| k.ca_hash() % 39).collect();
        assert!(buckets.windows(2).all(|w| w[0] <= w[1]));
    }

    /// Random inserts (grid positions, repeats, -0, NaN) and removes against [`Reference`]: the same answers and the same order.
    #[test]
    fn the_same_order_as_the_reference() {
        let mut state = 0x1234_5678u32;
        let mut next = move || {
            state = state.wrapping_mul(0x343fd).wrapping_add(0x269ec3);
            state >> 16
        };

        for round in 0..20 {
            let (mut m, mut r) = (CaMap::<Vec2Key, u32>::default(), Reference::<Vec2Key, u32>::new());
            for step in 0..20_000u32 {
                let pick = next();
                let key = match pick % 50 {
                    0 => Vec2Key(-0.0, (pick % 7) as f32),
                    1 => Vec2Key(0.0, (pick % 7) as f32),
                    2 => Vec2Key(f32::NAN, 1.0),
                    _ => Vec2Key(((next() % 300) as f32 + 0.5) * 2.0, ((next() % 300) as f32 + 0.5) * 2.0),
                };

                if next() % 4 == 0 {
                    assert_eq!(m.remove(&key), r.remove(&key), "round {round} step {step}: remove {key:?}");
                } else {
                    assert_eq!(m.insert(key, step).1, r.insert(key, step), "round {round} step {step}: insert {key:?}");
                }

                assert_eq!(m.len(), r.count);
                if step % 997 == 0 {
                    let ours: Vec<(u32, u32, u32)> = m.iter().map(|(k, v)| (k.0.to_bits(), k.1.to_bits(), *v)).collect();
                    let theirs: Vec<(u32, u32, u32)> = r.buckets.iter().flatten().map(|(k, v)| (k.0.to_bits(), k.1.to_bits(), *v)).collect();
                    assert_eq!(ours, theirs, "round {round} step {step}: the order");
                    assert_eq!(m.heads.len(), r.buckets.len());
                }
            }
        }
    }
}
