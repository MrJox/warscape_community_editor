//! Tests of environment values. The expected texts are the C++ editor's output for the same inputs (`env_value_dump` compares the
//! two over millions of numbers and every attribute value of the corpus).

use wce_formats::env_value::{NumStyle, ValueKind, format_number, format_value, parse_value};

const STRIPPED: NumStyle = NumStyle::Stripped { keep_one: false };
const SHORTEST: NumStyle = NumStyle::Shortest { keep_one: false };

#[test]
fn numbers_in_every_style() {
    assert_eq!(format_number(111.6, STRIPPED), "111.599998");
    assert_eq!(format_number(111.6, SHORTEST), "111.6");
    assert_eq!(format_number(1.0, NumStyle::Stripped { keep_one: true }), "1.0");
    assert_eq!(format_number(1.0, NumStyle::Fixed { decimals: 6 }), "1.000000");
    assert_eq!(format_number(-0.0, STRIPPED), "0");
    assert_eq!(format_number(-0.0, NumStyle::Fixed { decimals: 2 }), "-0.00");

    // Shortest: scientific when shorter, exact digits for whole numbers, the even digit between two as close (3061734.25 here).
    assert_eq!(format_number(0.0001, SHORTEST), "1e-04");
    assert_eq!(format_number(1e20, SHORTEST), "1e+20");
    assert_eq!(format_number(123_456_792.0, SHORTEST), "123456792");
    assert_eq!(format_number(f32::from_bits(1_245_372_313), SHORTEST), "3061734.2");
    assert_eq!(format_number(f32::from_bits(1_245_372_313), NumStyle::Fixed { decimals: 1 }), "3061734.2");
}

#[test]
fn values_keep_their_style() {
    for (text, kind, style) in [
        ("111.599998", ValueKind::Float, STRIPPED),
        ("244.8", ValueKind::Float, SHORTEST),
        ("1.000000", ValueKind::Float, NumStyle::Fixed { decimals: 6 }),
        ("0.50", ValueKind::Float, NumStyle::Fixed { decimals: 2 }),
        ("rgb(1.0,0.788235,0.615686)", ValueKind::Rgb, NumStyle::Stripped { keep_one: true }),
        ("argb(0.48,0.56,0.83,0.0039)", ValueKind::Argb, STRIPPED),
        ("variance(0.3,40)", ValueKind::Variance, STRIPPED),
    ] {
        let value = parse_value(text);
        assert_eq!((value.kind, value.style), (kind, style), "{text}");
        assert_eq!(format_value(&value), text);
    }

    // Mixed styles in one value: the first that fits every component, or the default.
    assert_eq!(format_value(&parse_value("euler(3.141593,1.000000,0.0)")), "euler(3.141593,1,0)");
    assert_eq!(format_value(&parse_value("1e5")), "100000");
    assert_eq!(format_value(&parse_value("414035.13")), "414035.125");

    let value = parse_value("12");
    assert_eq!((value.kind, value.i, value.f.as_slice()), (ValueKind::Int, 12, &[12.0][..]));
    assert!(parse_value("true").b);
    for text in ["hello", "rgb(1,2)", "rgb(1, 2,3)", "+1", "1e50", ""] {
        assert_eq!(parse_value(text).kind, ValueKind::String, "{text}");
    }
}
