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
        return Some(sign * f64::INFINITY);
    }

    if starts_with(b"nan") {
        return Some(sign * f64::NAN);
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

    s[start..end].parse().ok()
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
