//! Text encodings of the game's XML files: 8-bit (ASCII or UTF-8, optionally with a BOM) or UTF-16 LE with a BOM (vfx files).
//!
//! Text is kept as bytes: 8-bit files may not be valid UTF-8, and UTF-16 files are decoded to UTF-8 with lone surrogates kept
//! as WTF-8. `encode_text(decode_text(x))` gives `x` back for every valid input.

/// Encoding of a text file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextEncoding {
    #[default]
    Utf8,
    Utf8Bom,
    Utf16LeBom,
}

/// Error of a UTF-16 file with an odd number of bytes.
#[derive(Debug, thiserror::Error)]
#[error("odd byte count in UTF-16 file")]
pub struct OddUtf16;

/// Decodes a text file to UTF-8 (WTF-8 for lone surrogates), without its BOM.
///
/// # Errors
///
/// Returns [`OddUtf16`] if the file starts with a UTF-16 LE BOM but has an odd number of bytes.
pub fn decode_text(bytes: &[u8]) -> Result<(Vec<u8>, TextEncoding), OddUtf16> {
    if let [0xFF, 0xFE, rest @ ..] = bytes {
        if rest.len() % 2 != 0 {
            return Err(OddUtf16);
        }

        let mut out = Vec::with_capacity(rest.len() / 2);
        let mut units = rest.as_chunks::<2>().0.iter().map(|&[low, high]| u32::from(low) | (u32::from(high) << 8)).peekable();
        while let Some(unit) = units.next() {
            if (0xD800..0xDC00).contains(&unit)
                && let Some(&low) = units.peek()
                && (0xDC00..0xE000).contains(&low)
            {
                units.next();
                append_utf8(&mut out, 0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00));
                continue;
            }

            append_utf8(&mut out, unit);
        }

        return Ok((out, TextEncoding::Utf16LeBom));
    }

    match bytes {
        [0xEF, 0xBB, 0xBF, rest @ ..] => Ok((rest.to_vec(), TextEncoding::Utf8Bom)),
        _ => Ok((bytes.to_vec(), TextEncoding::Utf8)),
    }
}

/// Encodes UTF-8 (or WTF-8) text in an encoding, with its BOM.
pub fn encode_text(text: &[u8], encoding: TextEncoding) -> Vec<u8> {
    match encoding {
        TextEncoding::Utf8 => return text.to_vec(),
        TextEncoding::Utf8Bom => return [&[0xEF, 0xBB, 0xBF], text].concat(),
        TextEncoding::Utf16LeBom => {}
    }

    let mut out = Vec::with_capacity(2 + text.len() * 2);
    out.extend_from_slice(&[0xFF, 0xFE]);
    let mut put = |unit: u32| out.extend_from_slice(&[unit as u8, (unit >> 8) as u8]);
    let mut i = 0;
    while i < text.len() {
        let lead = text[i];
        let (mut code_point, len) = match lead {
            _ if lead < 0x80 => (u32::from(lead), 1),
            _ if lead & 0xE0 == 0xC0 => (u32::from(lead & 0x1F), 2),
            _ if lead & 0xF0 == 0xE0 => (u32::from(lead & 0x0F), 3),
            _ => (u32::from(lead & 0x07), 4),
        };

        for byte in text.iter().skip(i + 1).take(len - 1) {
            code_point = (code_point << 6) | u32::from(byte & 0x3F);
        }

        i += len;
        if code_point >= 0x10000 {
            code_point -= 0x10000;
            put(0xD800 + (code_point >> 10));
            put(0xDC00 + (code_point & 0x3FF));
        } else {
            put(code_point);
        }
    }

    out
}

/// Appends a code point as UTF-8; surrogates are written like any other code point (WTF-8).
fn append_utf8(out: &mut Vec<u8>, code_point: u32) {
    if code_point < 0x80 {
        out.push(code_point as u8);
    } else if code_point < 0x800 {
        out.extend_from_slice(&[0xC0 | (code_point >> 6) as u8, 0x80 | (code_point & 0x3F) as u8]);
    } else if code_point < 0x10000 {
        out.extend_from_slice(&[0xE0 | (code_point >> 12) as u8, 0x80 | ((code_point >> 6) & 0x3F) as u8, 0x80 | (code_point & 0x3F) as u8]);
    } else {
        out.extend_from_slice(&[
            0xF0 | (code_point >> 18) as u8,
            0x80 | ((code_point >> 12) & 0x3F) as u8,
            0x80 | ((code_point >> 6) & 0x3F) as u8,
            0x80 | (code_point & 0x3F) as u8,
        ]);
    }
}
