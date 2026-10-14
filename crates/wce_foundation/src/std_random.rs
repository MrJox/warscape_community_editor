//! `std::mt19937` and the libstdc++ `std::shuffle` / `std::uniform_int_distribution` on it, draw for draw: what the C++ code picked from a
//! seed (the sample map's buildings), the port picks too. The algorithms are those of libstdc++ 16 for a 32-bit generator: Lemire's
//! nearly-divisionless downscaling, and a shuffle that takes two swap positions from one draw while the range is small.

/// The 32-bit Mersenne twister, as `std::mt19937`.
#[derive(Clone)]
pub struct Mt19937 {
    mt: [u32; 624],
    i: usize,
}

impl Mt19937 {
    pub fn new(seed: u32) -> Self {
        let mut mt = [0u32; 624];
        mt[0] = seed;
        for i in 1..624 {
            mt[i] = 1_812_433_253u32.wrapping_mul(mt[i - 1] ^ (mt[i - 1] >> 30)).wrapping_add(i as u32);
        }

        Self { mt, i: 624 }
    }

    pub fn next_u32(&mut self) -> u32 {
        if self.i >= 624 {
            for k in 0..624 {
                let y = (self.mt[k] & 0x8000_0000) | (self.mt[(k + 1) % 624] & 0x7fff_ffff);
                self.mt[k] = self.mt[(k + 397) % 624] ^ (y >> 1) ^ if y & 1 != 0 { 0x9908_b0df } else { 0 };
            }

            self.i = 0;
        }

        let mut y = self.mt[self.i];
        self.i += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^ (y >> 18)
    }

    /// `std::uniform_int_distribution<uint64_t>{0, count - 1}(*this)` for `count` up to 2^32.
    pub fn below(&mut self, count: u64) -> u64 {
        assert!(count > 0 && count <= 1 << 32, "the range must fit the generator's 32 bits");
        if count == 1 << 32 {
            return u64::from(self.next_u32());
        }

        // Lemire's method with a 64-bit product.
        let range = count as u32;
        let mut product = u64::from(self.next_u32()) * u64::from(range);
        let mut low = product as u32;
        if low < range {
            let threshold = range.wrapping_neg() % range;
            while low < threshold {
                product = u64::from(self.next_u32()) * u64::from(range);
                low = product as u32;
            }
        }

        product >> 32
    }
}

/// `std::shuffle(v.begin(), v.end(), g)`.
pub fn shuffle<T>(v: &mut [T], g: &mut Mt19937) {
    if v.is_empty() {
        return;
    }

    let n = v.len() as u64;
    if u64::from(u32::MAX) / n >= n {
        let mut i = 1usize;

        // An even number of elements is an odd number of swaps: the first one up front.
        if n.is_multiple_of(2) {
            let j = g.below(2) as usize;
            v.swap(i, j);
            i += 1;
        }

        // The rest in pairs, two swap positions from one draw.
        while i < v.len() {
            let range = i as u64 + 1;
            let x = g.below(range * (range + 1));
            v.swap(i, (x / (range + 1)) as usize);
            v.swap(i + 1, (x % (range + 1)) as usize);
            i += 2;
        }

        return;
    }

    for i in 1..v.len() {
        let j = g.below(i as u64 + 1) as usize;
        v.swap(i, j);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mt19937_reference() {
        // The C++ standard requires the 10000th value of a default-seeded mt19937 to be 4123659995.
        let mut g = Mt19937::new(5489);
        let v = (0..10000).map(|_| g.next_u32()).last();
        assert_eq!(v, Some(4_123_659_995));
    }
}
