//! Typed view of environment attribute values.
//!
//! Syntax seen in every vanilla and TDD file:
//!
//! - `1`, `0.5`, `111.599998`, `-7.2`: float or int;
//! - `true`, `false`: bool;
//! - `rgb(1.0,0.788235,0.615686)`: colour;
//! - `argb(0.48,0.56,0.83,0.0039)`: colour, in the order r, g, b, a despite the name (verified);
//! - `variance(0.3,40)`: two floats;
//! - `euler(3.141593,1.000000,0.0)`: legacy direction.
//!
//! Number styles differ by file and tool generation (`%f` stripped: `111.599998`, shortest: `244.8`, fixed: `0.50` and `100.00`
//! in vfx files, `1.000000` in old files). [`parse_value`] detects the style that reproduces the original text, and
//! [`format_value`] writes new numbers in that same style.

/// The kind of a value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ValueKind {
    Float,
    Int,
    Bool,
    Rgb,
    Argb,
    Variance,
    Euler,
    #[default]
    String,
}

/// How a value writes its numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumStyle {
    /// C's `%f` without trailing zeros. With `keep_one`, whole numbers keep `.0`.
    Stripped { keep_one: bool },

    /// The shortest text that reads back as the same float (C++'s `std::to_chars`). With `keep_one`, whole numbers keep `.0`.
    Shortest { keep_one: bool },

    /// A fixed number of decimals.
    Fixed { decimals: usize },
}

impl Default for NumStyle {
    fn default() -> Self {
        Self::Stripped { keep_one: false }
    }
}

/// A parsed attribute value.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EnvValue {
    pub kind: ValueKind,

    /// Float and Int: 1; Rgb: r, g, b; Argb: r, g, b, a; Variance: 2; Euler: 3.
    pub f: Vec<f32>,

    /// Int.
    pub i: i64,

    /// Bool.
    pub b: bool,

    /// The original text.
    pub s: String,

    pub style: NumStyle,
}

/// Reads a float the way C++'s `std::from_chars` does: the whole text, no sign `+`, no out-of-range values.
fn parse_float(s: &str) -> Option<f32> {
    if s.is_empty() || s.starts_with('+') || s.starts_with("-+") {
        return None;
    }

    // `nan(chars)`.
    let unsigned = s.strip_prefix('-').unwrap_or(s);
    if unsigned.len() > 4 && unsigned.as_bytes()[..4].eq_ignore_ascii_case(b"nan(") && unsigned.ends_with(')')
        && unsigned.as_bytes()[4..unsigned.len() - 1].iter().all(|b| b.is_ascii_alphanumeric() || *b == b'_')
    {
        return Some(if s.starts_with('-') { -f32::NAN } else { f32::NAN });
    }

    let value = s.parse::<f32>().ok()?;
    let mantissa = s.split(['e', 'E']).next().unwrap_or_default();
    let named = mantissa.trim_start_matches('-').chars().all(|c| c.is_ascii_alphabetic());
    if !named && (value.is_infinite() || (value == 0.0 && mantissa.bytes().any(|b| (b'1'..=b'9').contains(&b)))) {
        return None;
    }

    Some(value)
}

/// Reads an integer the way C++'s `std::from_chars` does: the whole text, no sign `+`.
fn parse_int(s: &str) -> Option<i64> {
    if s.starts_with('+') {
        return None;
    }

    s.parse().ok()
}

/// The styles to try for a number's text, in order.
fn candidate_styles(sample: &str) -> Vec<NumStyle> {
    let mut styles = vec![
        NumStyle::Stripped { keep_one: false },
        NumStyle::Stripped { keep_one: true },
        NumStyle::Shortest { keep_one: false },
        NumStyle::Shortest { keep_one: true },
    ];

    // Fixed, with the sample's own decimal count.
    if let Some(dot) = sample.find('.') {
        let decimals = sample.len() - dot - 1;
        if (1..=9).contains(&decimals) {
            styles.push(NumStyle::Fixed { decimals });
        }
    }

    styles
}

/// The first style that reproduces the text of every component exactly.
fn detect_style(texts: &[&str], values: &[f32]) -> NumStyle {
    texts.iter()
        .flat_map(|sample| candidate_styles(sample))
        .find(|style| values.iter().zip(texts).all(|(value, text)| format_number(*value, *style) == *text))
        .unwrap_or_default()
}

/// Non-finite values as C's `printf` and C++'s `to_chars` write them.
fn non_finite(v: f32) -> String {
    let sign = if v.is_sign_negative() { "-" } else { "" };
    if v.is_nan() { format!("{sign}nan") } else { format!("{sign}inf") }
}

/// The shortest text that reads back as `v`, as C++'s `std::to_chars(first, last, v)`: fixed or scientific notation, whichever
/// is shorter, fixed on a tie.
fn shortest(v: f32) -> String {
    if !v.is_finite() {
        return non_finite(v);
    }

    if v == 0.0 {
        return if v.is_sign_negative() { "-0".to_owned() } else { "0".to_owned() };
    }

    // The shortest digits and their exponent. Between two candidates as close to the value, the even one (as Ryu does).
    let mut sci = format!("{:e}", v.abs());
    let count = sci.split('e').next().unwrap_or_default().replace('.', "").len();
    let nearest = format!("{:.*e}", count - 1, f64::from(v.abs()));
    if nearest.parse::<f32>().ok() == Some(v.abs()) {
        sci = nearest;
    }

    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let digits = mantissa.replace('.', "");
    let exp = exp.parse::<i32>().unwrap_or_default();
    let sign = if v < 0.0 { "-" } else { "" };

    let scientific = format!("{sign}{}{}{}e{}{:02}", &digits[..1], if digits.len() > 1 { "." } else { "" }, &digits[1..], if exp < 0 { '-' } else { '+' }, exp.unsigned_abs());
    let count = digits.len() as i32;
    let fixed = if exp >= count - 1 {

        // A whole number: its exact digits (every whole f32 that's shorter in fixed notation is exact in f64).
        format!("{sign}{}", f64::from(v.abs()))
    } else if exp >= 0 {
        let point = exp as usize + 1;
        format!("{sign}{}.{}", &digits[..point], &digits[point..])
    } else {
        format!("{sign}0.{}{digits}", "0".repeat((-exp - 1) as usize))
    };

    if fixed.len() <= scientific.len() { fixed } else { scientific }
}

/// Writes a number in a style.
pub fn format_number(v: f32, style: NumStyle) -> String {
    let (mut s, keep_one) = match style {
        NumStyle::Fixed { decimals } => {
            return if v.is_finite() { format!("{:.*}", decimals, f64::from(v)) } else { non_finite(v) };
        }

        NumStyle::Stripped { keep_one } => {
            let mut s = if v.is_finite() { format!("{:.6}", f64::from(v)) } else { non_finite(v) };
            if s.contains('.') {
                s.truncate(s.trim_end_matches('0').trim_end_matches('.').len());
            }

            (s, keep_one)
        }

        NumStyle::Shortest { keep_one } => (shortest(v), keep_one),
    };

    if s == "-0" {
        s = "0".to_owned();
    }

    if keep_one && !s.contains(['.', 'e']) {
        s.push_str(".0");
    }

    s
}

/// Reads a value, and the style its numbers are written in.
pub fn parse_value(t: &str) -> EnvValue {
    let mut v = EnvValue { s: t.to_owned(), ..Default::default() };
    if t == "true" || t == "false" {
        v.kind = ValueKind::Bool;
        v.b = t == "true";
        return v;
    }

    let plain = !t.contains(['.', 'e', 'E']);
    if plain && let Some(n) = parse_int(t) {
        v.kind = ValueKind::Int;
        v.i = n;
        v.f = vec![n as f32];
        return v;
    }

    if let Some(x) = parse_float(t) {
        v.kind = if plain { ValueKind::Int } else { ValueKind::Float };
        v.f = vec![x];
        v.style = detect_style(&[t], &v.f);
        return v;
    }

    let Some((name, args)) = t.strip_suffix(')').and_then(|t| t.split_once('(')) else { return v };
    let texts = args.split(',').collect::<Vec<_>>();
    let Some(comps) = texts.iter().map(|text| parse_float(text)).collect::<Option<Vec<_>>>() else { return v };
    let kind = match (name, comps.len()) {
        ("rgb", 3) => ValueKind::Rgb,
        ("argb", 4) => ValueKind::Argb,
        ("variance", 2) => ValueKind::Variance,
        ("euler", 3) => ValueKind::Euler,
        _ => return v,
    };

    v.kind = kind;
    v.style = detect_style(&texts, &comps);
    v.f = comps;
    v
}

/// Writes a value: numbers in its style, any other kind as its original text.
pub fn format_value(v: &EnvValue) -> String {
    let list = |name: &str| {
        let numbers = v.f.iter().map(|x| format_number(*x, v.style)).collect::<Vec<_>>();
        format!("{name}({})", numbers.join(","))
    };

    match v.kind {
        ValueKind::Bool => if v.b { "true" } else { "false" }.to_owned(),
        ValueKind::Int => v.i.to_string(),
        ValueKind::Float => v.f.first().map_or_else(|| v.s.clone(), |x| format_number(*x, v.style)),
        ValueKind::Rgb => list("rgb"),
        ValueKind::Argb => list("argb"),
        ValueKind::Variance => list("variance"),
        ValueKind::Euler => list("euler"),
        ValueKind::String => v.s.clone(),
    }
}
