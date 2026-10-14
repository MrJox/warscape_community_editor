//! Hash of the half-float conversion of every f32 bit pattern (with and without NaNs), for comparing with the C++ `_Float16` cast.

use wce_scene::half::f32_to_f16;

fn main() {
    let (mut all, mut no_nan) = (0xCBF2_9CE4_8422_2325u64, 0xCBF2_9CE4_8422_2325u64);
    for i in 0..=u32::MAX {
        let f = f32::from_bits(i);
        let v = u64::from(f32_to_f16(f));
        all = (all ^ v).wrapping_mul(0x0100_0000_01B3);
        if !f.is_nan() {
            no_nan = (no_nan ^ v).wrapping_mul(0x0100_0000_01B3);
        }
    }

    println!("{all:016x} {no_nan:016x}");
}
