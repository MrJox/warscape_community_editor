//! Half floats.

/// A float as a half float (IEEE binary16), rounded to the nearest, ties to even, as a C `_Float16` cast and DirectXMath's
/// `XMConvertFloatToHalf` round. Too big values become infinities; NaNs stay quiet NaNs with the top bits of their payload.
pub fn f32_to_f16(v: f32) -> u16 {
    let x = v.to_bits();
    let sign = ((x >> 16) & 0x8000) as u16;
    let exp = ((x >> 23) & 0xFF) as i32;
    let mant = x & 0x7F_FFFF;
    if exp == 0xFF {
        return sign | 0x7C00 | if mant != 0 { 0x200 | (mant >> 13) as u16 } else { 0 };
    }

    let e = exp - 127 + 15;
    if e >= 31 {
        return sign | 0x7C00;
    }

    // A subnormal half, or zero: the value in units of 2^-24.
    if e <= 0 {
        if e < -10 {
            return sign;
        }

        let m = mant | 0x80_0000;
        let shift = (14 - e) as u32;
        let (h, rem, halfway) = (m >> shift, m & ((1 << shift) - 1), 1 << (shift - 1));
        let rounded = if rem > halfway || (rem == halfway && h & 1 == 1) { h + 1 } else { h };
        return sign | rounded as u16;
    }

    // A carry out of the mantissa moves into the exponent, up to infinity.
    let h = ((e as u32) << 10) | (mant >> 13);
    let rem = mant & 0x1FFF;
    let rounded = if rem > 0x1000 || (rem == 0x1000 && h & 1 == 1) { h + 1 } else { h };
    sign | rounded as u16
}
