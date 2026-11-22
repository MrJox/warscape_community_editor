//! Hash maps keyed by ids or addresses, hashed by a multiply instead of SipHash (no untrusted keys: the editor's own ids).

/// A map keyed by an id or an address, hashed by a multiply: the frame's walk looks its 20 000 terrains up in several of them, and
/// with SipHash the walk took 6.5 ms, 4.6 with this.
pub type IdMap<K, V> = std::collections::HashMap<K, V, std::hash::BuildHasherDefault<IdHasher>>;

/// A set of ids or addresses ([`IdMap`]'s hash).
pub type IdSet<K> = std::collections::HashSet<K, std::hash::BuildHasherDefault<IdHasher>>;

/// [`IdMap`]'s hash: each integer multiplied in, the high half folded into the low (the table picks buckets by the low bits).
#[derive(Default)]
pub struct IdHasher(u64);

impl std::hash::Hasher for IdHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(u64::from(b));
        }
    }

    fn write_u32(&mut self, v: u32) {
        self.write_u64(u64::from(v));
    }

    fn write_u64(&mut self, v: u64) {
        let h = (self.0.rotate_left(5) ^ v).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        self.0 = h ^ (h >> 32);
    }

    fn write_usize(&mut self, v: usize) {
        self.write_u64(v as u64);
    }
}
