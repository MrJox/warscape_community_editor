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
}
