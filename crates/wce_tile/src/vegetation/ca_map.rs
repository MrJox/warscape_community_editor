//! `CA_STD::UNORDERED_MAP` as BOB's generator uses it: what it holds and, above all, the order it gives them back in, which decides every
//! random draw of the generator. Buckets are lists; a new key goes to the end of bucket `hash % buckets`; the map starts with 4 buckets and,
//! when `count / buckets > 1.0` after counting a new key, grows to `2 * buckets + 1` (or `ceil(count)` if more), moving the entries bucket by
//! bucket in order. Iteration is bucket by bucket. Erasing doesn't shrink it.

/// A key with CA's hash.
pub trait CaKey: PartialEq + Clone {
    fn ca_hash(&self) -> u32;
}

impl CaKey for String {
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
}

/// A position as BOB keys it (`UTILITYLIB::VECTOR_2`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vec2Key(pub f32, pub f32);

impl CaKey for Vec2Key {
    /// `CA_STD::HASH<VECTOR_2>`: the bits of x xor the bits of y xor 0x4a545eed.
    fn ca_hash(&self) -> u32 {
        self.0.to_bits() ^ self.1.to_bits() ^ 0x4a54_5eed
    }
}

#[derive(Clone, Debug)]
pub struct CaMap<K, V> {
    buckets: Vec<Vec<(K, V)>>,
    count: usize,
}

impl<K: CaKey, V> Default for CaMap<K, V> {
    fn default() -> Self {
        Self::with_buckets(4)
    }
}

impl<K: CaKey, V> CaMap<K, V> {
    pub fn with_buckets(n: usize) -> Self {
        Self { buckets: (0..n.max(1)).map(|_| Vec::new()).collect(), count: 0 }
    }

    fn bucket_of(&self, k: &K) -> usize {
        (k.ca_hash() % self.buckets.len() as u32) as usize
    }

    pub fn get(&self, k: &K) -> Option<&V> {
        self.buckets[self.bucket_of(k)].iter().find(|(kk, _)| kk == k).map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, k: &K) -> Option<&mut V> {
        let b = self.bucket_of(k);
        self.buckets[b].iter_mut().find(|(kk, _)| kk == k).map(|(_, v)| v)
    }

    pub fn contains(&self, k: &K) -> bool {
        self.get(k).is_some()
    }

    /// Inserts if the key is new (an existing value stays); the value stored under the key, and whether it was inserted.
    pub fn insert(&mut self, k: K, v: V) -> (&mut V, bool) {
        if self.contains(&k) {
            return (self.get_mut(&k).expect("present"), false);
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
        (&mut self.buckets[b].last_mut().expect("just pushed").1, true)
    }

    pub fn remove(&mut self, k: &K) -> bool {
        let b = self.bucket_of(k);
        if let Some(i) = self.buckets[b].iter().position(|(kk, _)| kk == k) {
            self.buckets[b].remove(i);
            self.count -= 1;
            return true;
        }

        false
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.buckets.iter().flatten().map(|(k, v)| (k, v))
    }

    pub fn keys(&self) -> Vec<K> {
        self.iter().map(|(k, _)| k.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(m.buckets.len(), 39);
        // Entries of a new bucket keep the order of the old buckets they came from.
        let mut seen: Vec<usize> = m.iter().map(|(_, v)| *v).collect();
        seen.sort_unstable();
        assert_eq!(seen, (0..20).collect::<Vec<_>>());
        let buckets: Vec<u32> = m.iter().map(|(k, _)| k.ca_hash() % 39).collect();
        assert!(buckets.windows(2).all(|w| w[0] <= w[1]));
    }
}
