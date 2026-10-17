//! Differential dump of the math, for comparing with the C++ code fed the same inputs (bit for bit).
//!
//!   math_dump <file of little-endian f32, 40 per case>

use std::io::{BufWriter, Write};

use wce_foundation::math::*;

fn main() -> std::io::Result<()> {
    let data = std::fs::read(std::env::args().nth(1).unwrap_or_default())?;
    let f = data.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect::<Vec<_>>();
    let mut out = BufWriter::new(std::io::stdout().lock());
    let p = |out: &mut BufWriter<_>, m: Float4x4| -> std::io::Result<()> {
        for v in m.data() {
            write!(out, "{:08x} ", v.to_bits())?;
        }

        writeln!(out)
    };

    for v in f.as_chunks::<40>().0 {
        let matrix = |at: usize| Float4x4 { m: std::array::from_fn(|i| std::array::from_fn(|j| v[at + i * 4 + j])) };
        let (a, b) = (matrix(0), matrix(16));
        let (e, t, up) = (Float3::new(v[32], v[33], v[34]), Float3::new(v[35], v[36], v[37]), Float3::new(0.0, 1.0, 0.0));
        for m in [a * b, inverse(&a), translation(e), rotation_x(v[38]), rotation_y(v[38]), rotation_z(v[39]), scaling(v[39]), look_at_lh(e, t, up),
            perspective_lh(v[38].abs() + 0.1, v[39].abs() + 0.5, 0.5, 5000.0), ortho_lh(v[38], v[39], -1.0, 100.0)] {
            p(&mut out, m)?;
        }

        for q in [a.transform_point(e), normalize(cross(e, t))] {
            writeln!(out, "{:08x} {:08x} {:08x}", q.x.to_bits(), q.y.to_bits(), q.z.to_bits())?;
        }

        writeln!(out, "{:08x} {:08x}", dot(e, t).to_bits(), length(e).to_bits())?;
    }

    Ok(())
}
