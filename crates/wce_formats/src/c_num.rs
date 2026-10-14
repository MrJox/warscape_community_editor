//! Numbers read the way C's `strtod` and `strtol` read them: leading whitespace, then the longest prefix that is a number,
//! ignoring the rest (`"12.5abc"` is 12.5 as a float, 12 as an integer). The editor's typed views of XML documents read their
//! attributes like this, so a value the game's tools accept never fails to read.
//!
//! Not covered: hexadecimal floats (`0x1p3` reads as 0), which no game file uses.

/// C's `isspace` in the C locale.
fn is_c_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C)
}

/// The number of bytes of `s` from `at` that are ASCII digits.
fn digits(s: &[u8], at: usize) -> usize {
    s[at.min(s.len())..].iter().take_while(|b| b.is_ascii_digit()).count()
}

/// The float at the start of `s` (after whitespace), or `None` if there is none (where `strtod` sets its end pointer to `s`).
pub fn strtod(s: &str) -> Option<f64> {
    strtod_prefix(s).map(|(value, _)| value)
}

/// The float at the start of `s` (after whitespace) and the number of bytes `strtod` consumes for it (its end pointer), or `None` if there
/// is none.
pub fn strtod_prefix(s: &str) -> Option<(f64, usize)> {
    let bytes = s.as_bytes();
    let start = bytes.iter().take_while(|b| is_c_space(**b)).count();
    let mut p = start;
    if matches!(bytes.get(p), Some(b'+' | b'-')) {
        p += 1;
    }

    let rest = &bytes[p..];
    let starts_with = |word: &[u8]| rest.len() >= word.len() && rest[..word.len()].eq_ignore_ascii_case(word);
    let sign = if bytes.get(start) == Some(&b'-') { -1.0 } else { 1.0 };
    if starts_with(b"inf") {
        let len = if starts_with(b"infinity") { 8 } else { 3 };
        return Some((sign * f64::INFINITY, p + len));
    }

    if starts_with(b"nan") {
        // "nan(n-char-sequence)" takes the parentheses too when they close.
        let mut end = p + 3;
        if bytes.get(end) == Some(&b'(') {
            let chars = bytes[end + 1..].iter().take_while(|b| b.is_ascii_alphanumeric() || **b == b'_').count();
            if bytes.get(end + 1 + chars) == Some(&b')') {
                end += chars + 2;
            }
        }

        return Some((sign * f64::NAN, end));
    }

    let int_digits = digits(bytes, p);
    let mut end = p + int_digits;
    let mut frac_digits = 0;
    if bytes.get(end) == Some(&b'.') {
        frac_digits = digits(bytes, end + 1);
        if int_digits + frac_digits > 0 {
            end += 1 + frac_digits;
        }
    }

    if int_digits + frac_digits == 0 {
        return None;
    }

    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let mut q = end + 1;
        if matches!(bytes.get(q), Some(b'+' | b'-')) {
            q += 1;
        }

        let exp_digits = digits(bytes, q);
        if exp_digits > 0 {
            end = q + exp_digits;
        }
    }

    Some((s[start..end].parse().ok()?, end))
}

/// `printf("%g", v)`: 6 significant digits, fixed or exponent notation by the exponent, trailing zeros removed.
pub fn printf_g(v: f64) -> String {
    if v.is_nan() {
        return if v.is_sign_negative() { "-nan" } else { "nan" }.to_owned();
    }

    if v.is_infinite() {
        return if v < 0.0 { "-inf" } else { "inf" }.to_owned();
    }

    if v == 0.0 {
        return if v.is_sign_negative() { "-0" } else { "0" }.to_owned();
    }

    let strip = |t: String| if t.contains('.') { t.trim_end_matches('0').trim_end_matches('.').to_owned() } else { t };

    // The exponent after rounding to 6 significant digits decides the notation.
    let e = format!("{v:.5e}");
    let (mantissa, exp) = e.split_once('e').expect("exponent notation");
    let x = exp.parse::<i32>().expect("an exponent");
    if (-4..6).contains(&x) {
        strip(format!("{v:.*}", (5 - x) as usize))
    } else {
        format!("{}e{}{:02}", strip(mantissa.to_owned()), if x < 0 { '-' } else { '+' }, x.abs())
    }
}

/// The integer at the start of `s` (after whitespace) in base 10, or `None` if there is none. Out of range values saturate, as in `strtoll`.
pub fn strtol(s: &str) -> Option<i64> {
    let bytes = s.as_bytes();
    let mut p = bytes.iter().take_while(|b| is_c_space(**b)).count();
    let negative = bytes.get(p) == Some(&b'-');
    if matches!(bytes.get(p), Some(b'+' | b'-')) {
        p += 1;
    }

    let count = digits(bytes, p);
    if count == 0 {
        return None;
    }

    let value = bytes[p..p + count].iter().try_fold(0i64, |value, digit| {
        let digit = i64::from(digit - b'0');
        value.checked_mul(10).and_then(|value| if negative { value.checked_sub(digit) } else { value.checked_add(digit) })
    });

    Some(value.unwrap_or(if negative { i64::MIN } else { i64::MAX }))
}
