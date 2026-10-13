//! Differential dump of `env_value`, for comparing with the C++ code fed the same inputs.
//!
//!   env_value_dump numbers <file of little-endian u32 float bits>   every style of every number
//!   env_value_dump values <file of lines>                           the parse and the re-format of every line

use std::io::{BufWriter, Write};

use wce_formats::env_value::{NumStyle, format_number, format_value, parse_value};

fn style_name(style: NumStyle) -> String {
    match style {
        NumStyle::Stripped { keep_one } => format!("S{}", u8::from(keep_one)),
        NumStyle::Shortest { keep_one } => format!("H{}", u8::from(keep_one)),
        NumStyle::Fixed { decimals } => format!("F{decimals}"),
    }
}

fn main() -> std::io::Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    let data = std::fs::read(&args[2])?;
    let mut out = BufWriter::new(std::io::stdout().lock());
    if args[1] == "numbers" {
        for bits in data.as_chunks::<4>().0 {
            let bits = u32::from_le_bytes(*bits);
            let v = f32::from_bits(bits);
            let mut line = bits.to_string();
            for keep_one in [false, true] {
                line += &format!(" {} {}", format_number(v, NumStyle::Stripped { keep_one }), format_number(v, NumStyle::Shortest { keep_one }));
            }

            for decimals in 1..=9 {
                line += &format!(" {}", format_number(v, NumStyle::Fixed { decimals }));
            }

            writeln!(out, "{line}")?;
        }
    } else {
        for t in String::from_utf8_lossy(&data).lines() {
            let v = parse_value(t);
            let floats = v.f.iter().map(|f| format!("{},", f.to_bits())).collect::<String>();
            writeln!(out, "{} {} {} {} [{floats}] {} | {t}", v.kind as i32, style_name(v.style), v.i, i32::from(v.b), format_value(&v))?;
        }
    }

    Ok(())
}
