//! Float text the way taw/etwng writes it: Ruby's `Float#to_s` of a value rounded to five decimals when that
//! still is the same f32.

/// `Float#pretty_single`: round to 5 decimals if that gives the same f32, else the exact f32 value.
pub fn pretty_single(f: f32) -> f64 {
    let v = f as f64;
    if !v.is_finite() {
        return v;
    }
    let rv = (100_000.0 * v).round() / 100_000.0;
    if (rv as f32).to_bits() == f.to_bits() { rv } else { v }
}

/// Ruby's `Float#to_s` for finite values.
pub fn ruby_float_to_s(v: f64) -> String {
    if v == 0.0 {
        return if v.is_sign_negative() { "-0.0".into() } else { "0.0".into() };
    }
    // Shortest round-trip digits via the scientific formatter: "-d.ddde-x".
    let sci = format!("{:e}", v);
    let (mant, exp) = sci.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let (neg, mant) = match mant.strip_prefix('-') {
        Some(m) => (true, m),
        None => (false, mant),
    };
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let decpt = exp + 1;
    let mut s = String::new();
    if neg {
        s.push('-');
    }
    if decpt > 0 && decpt <= 16 {
        let dp = decpt as usize;
        if digits.len() <= dp {
            s.push_str(&digits);
            s.push_str(&"0".repeat(dp - digits.len()));
            s.push_str(".0");
        } else {
            s.push_str(&digits[..dp]);
            s.push('.');
            s.push_str(&digits[dp..]);
        }
    } else if decpt <= 0 && decpt > -4 {
        s.push_str("0.");
        s.push_str(&"0".repeat((-decpt) as usize));
        s.push_str(&digits);
    } else {
        s.push_str(&digits[..1]);
        s.push('.');
        if digits.len() > 1 {
            s.push_str(&digits[1..]);
        } else {
            s.push('0');
        }
        let e = decpt - 1;
        s.push('e');
        s.push(if e < 0 { '-' } else { '+' });
        s.push_str(&format!("{:02}", e.abs()));
    }
    s
}

/// `Float#round(2)` followed by `to_s`, for the degree comments.
pub fn round2_to_s(v: f64) -> String {
    ruby_float_to_s((v * 100.0).round() / 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ruby_formats() {
        assert_eq!(ruby_float_to_s(1.0), "1.0");
        assert_eq!(ruby_float_to_s(0.5), "0.5");
        assert_eq!(ruby_float_to_s(-12.25), "-12.25");
        assert_eq!(ruby_float_to_s(100000.0), "100000.0");
        assert_eq!(ruby_float_to_s(0.0001), "0.0001");
        assert_eq!(ruby_float_to_s(0.00001), "1.0e-05");
        assert_eq!(ruby_float_to_s(1e16), "1.0e+16");
        assert_eq!(ruby_float_to_s(1e15), "1000000000000000.0");
        assert_eq!(ruby_float_to_s(1.5e-7), "1.5e-07");
    }

    #[test]
    fn pretty() {
        assert_eq!(pretty_single(0.1f32), 0.1);
        assert_eq!(ruby_float_to_s(pretty_single(1.0)), "1.0");
        // an f32 that is not a five decimal value keeps its exact value
        let f = 1.0000001f32;
        assert_eq!(pretty_single(f), f as f64);
    }

    fn text_of(f: f32) -> String {
        ruby_float_to_s(pretty_single(f))
    }

    #[test]
    fn edge_values() {
        assert_eq!(text_of(0.0), "0.0");
        assert_eq!(text_of(-0.0), "-0.0");
        assert_eq!(text_of(0.1), "0.1");
        assert_eq!(text_of(-0.1), "-0.1");
        assert_eq!(text_of(1e-7), "1.0000000116860974e-07");
        assert_eq!(text_of(1e-45), "1.401298464324817e-45");
        assert_eq!(text_of(3.0), "3.0");
        assert_eq!(text_of(-7.0), "-7.0");
        assert_eq!(text_of(16_777_216.0), "16777216.0");
        assert_eq!(text_of(123_456.79), "123456.78906");
        // A small value that rounds to zero at five decimals keeps its own value, sign included.
        assert_eq!(text_of(-1e-10), "-1.000000013351432e-10");
        assert_eq!(text_of(0.000_01), "1.0e-05");
        assert_eq!(text_of(0.000_1), "0.0001");
        assert_eq!(text_of(0.001), "0.001");
    }

    #[test]
    fn exponent_boundaries_follow_ruby() {
        assert_eq!(ruby_float_to_s(9_999_999_999_999_998.0), "9999999999999998.0");
        assert_eq!(ruby_float_to_s(1.5e16), "1.5e+16");
        assert_eq!(ruby_float_to_s(1e100), "1.0e+100");
        assert_eq!(ruby_float_to_s(-2.5e-300), "-2.5e-300");
        assert_eq!(ruby_float_to_s(f64::MIN_POSITIVE), "2.2250738585072014e-308");
        assert_eq!(ruby_float_to_s(0.000_123), "0.000123");
        assert_eq!(ruby_float_to_s(0.000_012_3), "1.23e-05");
        assert_eq!(ruby_float_to_s(1.0 / 3.0), "0.3333333333333333");
        assert_eq!(ruby_float_to_s(123.456), "123.456");
    }

    #[test]
    fn every_text_reads_back_to_the_same_f32() {
        let mut state = 1u64;
        let mut fs = vec![f32::MAX, f32::MIN, f32::MIN_POSITIVE, -f32::MIN_POSITIVE, f32::EPSILON, 1e-45, -1e-45, 0.0, -0.0];
        while fs.len() < 200_000 {
            state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            fs.push(f32::from_bits((state >> 32) as u32));
        }
        for f in fs.into_iter().filter(|f| f.is_finite()) {
            let s = text_of(f);
            let back = s.parse::<f64>().unwrap() as f32;
            assert_eq!(back.to_bits(), f.to_bits(), "{f:e} written as {s}");
        }
    }

    #[test]
    fn degrees_round_to_two_decimals() {
        assert_eq!(round2_to_s(0.0), "0.0");
        assert_eq!(round2_to_s(90.000_002_5), "90.0");
        assert_eq!(round2_to_s(-57.295_779_5), "-57.3");
        assert_eq!(round2_to_s(12.345_6), "12.35");
    }
}
