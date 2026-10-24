//! The two line lists the game keeps in a building's `models_building` row (columns `EFLines` and `DockingLines`, byte sequences), under
//! a warp. Read from the game's rows (2026-10-24):
//!
//! - `EFLines`: u32 count, per line a name (u16 length, UTF-16), u32 action, start, end and direction (3 f32 each, the direction a unit
//!   vector). The firing lines of the logic file again, all levels (`EFline_piece01_destruct01_line01`).
//! - `DockingLines`: u32 count, per line a name (`DockingLine_piece01_destruct01_line01`), start, end and direction in x / z (2 f32 each,
//!   the direction facing out). Where ladders and towers dock: one straight line along a straight's outer face; CA's curved pieces
//!   have a polyline of short lines (6 of about 5 m on `barbarian_fort_curve_20m_radius`); the step pieces none.

use crate::warp::{Warp, turn};

/// A firing line of the row.
#[derive(Clone, Debug, PartialEq)]
pub struct EfLine {
    pub name: String,
    pub action: u32,
    pub start: [f32; 3],
    pub end: [f32; 3],
    pub direction: [f32; 3],
}

/// A docking line of the row (x / z).
#[derive(Clone, Debug, PartialEq)]
pub struct DockingLine {
    pub name: String,
    pub start: [f32; 2],
    pub end: [f32; 2],
    pub direction: [f32; 2],
}

/// The firing lines of an `EFLines` cell.
///
/// # Errors
///
/// The bytes end early.
pub fn read_ef_lines(b: &[u8]) -> Result<Vec<EfLine>, String> {
    let mut r = Reader { b, at: 0 };
    let n = r.u32()?;
    (0..n).map(|_| Ok(EfLine { name: r.name()?, action: r.u32()?, start: r.v3()?, end: r.v3()?, direction: r.v3()? })).collect()
}

/// An `EFLines` cell.
pub fn write_ef_lines(lines: &[EfLine]) -> Vec<u8> {
    let mut out = (lines.len() as u32).to_le_bytes().to_vec();
    for l in lines {
        name(&mut out, &l.name);
        out.extend_from_slice(&l.action.to_le_bytes());
        floats(&mut out, l.start.iter().chain(&l.end).chain(&l.direction));
    }

    out
}

/// The docking lines of a `DockingLines` cell.
///
/// # Errors
///
/// The bytes end early.
pub fn read_docking_lines(b: &[u8]) -> Result<Vec<DockingLine>, String> {
    let mut r = Reader { b, at: 0 };
    let n = r.u32()?;
    (0..n).map(|_| Ok(DockingLine { name: r.name()?, start: r.v2()?, end: r.v2()?, direction: r.v2()? })).collect()
}

/// A `DockingLines` cell.
pub fn write_docking_lines(lines: &[DockingLine]) -> Vec<u8> {
    let mut out = (lines.len() as u32).to_le_bytes().to_vec();
    for l in lines {
        name(&mut out, &l.name);
        floats(&mut out, l.start.iter().chain(&l.end).chain(&l.direction));
    }

    out
}

/// Firing lines warped: moved, their direction turned.
pub fn warp_ef_lines(lines: &[EfLine], warp: &dyn Warp) -> Vec<EfLine> {
    lines.iter().map(|l| {
        let mid = [(l.start[0] + l.end[0]) / 2.0, (l.start[1] + l.end[1]) / 2.0, (l.start[2] + l.end[2]) / 2.0];
        let t = turn([l.direction[0], l.direction[2]], warp.yaw(mid));
        EfLine { start: warp.point(l.start), end: warp.point(l.end), direction: [t[0], l.direction[1], t[1]], ..l.clone() }
    }).collect()
}

/// Docking lines warped: each cut into pieces of at most `step` metres, each a straight line between its warped ends facing out from
/// its middle, named after the original (`..._line01`, `..._line02`, ... per piece and level, in order).
pub fn warp_docking_lines(lines: &[DockingLine], warp: &dyn Warp, step: f32) -> Vec<DockingLine> {
    warp_docking_lines_where(lines, warp, step, &|_| true)
}

/// [`warp_docking_lines`] keeping only the pieces whose middle (x of the base's frame) `keep` takes (no docking on a step section, as
/// on CA's step pieces).
pub fn warp_docking_lines_where(lines: &[DockingLine], warp: &dyn Warp, step: f32, keep: &dyn Fn(f32) -> bool) -> Vec<DockingLine> {
    let mut out: Vec<DockingLine> = vec![];
    for l in lines {
        let len = ((l.end[0] - l.start[0]).powi(2) + (l.end[1] - l.start[1]).powi(2)).sqrt();
        let parts = (len / step).ceil().max(1.0) as usize;
        let at = |t: f32| [l.start[0] + (l.end[0] - l.start[0]) * t, 0.0, l.start[1] + (l.end[1] - l.start[1]) * t];
        let stem = l.name.rsplit_once("_line").map_or(l.name.as_str(), |(s, _)| s).to_owned();
        for k in 0..parts {
            let (a, b) = (at(k as f32 / parts as f32), at((k + 1) as f32 / parts as f32));
            let mid = at((k as f32 + 0.5) / parts as f32);
            if !keep(mid[0]) {
                continue;
            }

            let (wa, wb) = (warp.point(a), warp.point(b));
            let number = out.iter().filter(|o| o.name.starts_with(&stem)).count() + 1;
            out.push(DockingLine {
                name: format!("{stem}_line{number:02}"),
                start: [wa[0], wa[2]],
                end: [wb[0], wb[2]],
                direction: turn(l.direction, warp.yaw(mid)),
            });
        }
    }

    out
}

/// The action codes of a building's firing lines (the row's numbers by the logic file's action names), pairing the row's `EFLines` and
/// the logic file's `ef_lines` by name.
///
/// # Errors
///
/// The row's bytes or the logic file don't read.
pub fn action_codes(row: &[u8], logic: &[u8], codes: &mut std::collections::HashMap<String, u32>) -> Result<(), String> {
    let by_name = read_ef_lines(row)?.into_iter().map(|l| (l.name, l.action)).collect::<std::collections::HashMap<_, _>>();
    for (name, action, ..) in logic_ef_lines(logic)? {
        if let Some(&code) = by_name.get(&name) {
            codes.entry(action).or_insert(code);
        }
    }

    Ok(())
}

/// The firing lines of a logic file, every level: name, action name, start, end, direction.
fn logic_ef_lines(logic: &[u8]) -> Result<Vec<(String, String, [f32; 3], [f32; 3], [f32; 3])>, String> {
    use rpfm_lib::files::{Decodeable, cs2_parsed::Cs2Parsed};
    let parsed = Cs2Parsed::decode(&mut std::io::Cursor::new(logic), &None).map_err(|e| format!("cs2.parsed: {e}"))?;
    let v = serde_json::to_value(&parsed).map_err(|e| e.to_string())?;
    let p3 = |v: &serde_json::Value| ["x", "y", "z"].map(|k| v[k].as_f64().unwrap_or(0.0) as f32);
    Ok(v["pieces"].as_array().into_iter().flatten()
        .flat_map(|p| p["destructs"].as_array().into_iter().flatten())
        .flat_map(|d| d["ef_lines"].as_array().into_iter().flatten())
        .map(|l| (l["name"].as_str().unwrap_or_default().to_owned(), l["action"].as_str().unwrap_or_default().to_owned(), p3(&l["start"]), p3(&l["end"]), p3(&l["direction"])))
        .collect())
}

/// The `EFLines` of a logic file's firing lines (every level), their actions by `codes`, their directions unit vectors; with the count of
/// lines left out for an action without a code.
///
/// # Errors
///
/// The logic file doesn't read.
pub fn ef_lines_of(logic: &[u8], codes: &std::collections::HashMap<String, u32>) -> Result<(Vec<EfLine>, usize), String> {
    let (mut out, mut unknown) = (vec![], 0);
    for (name, action, start, end, d) in logic_ef_lines(logic)? {
        let Some(&code) = codes.get(&action) else {
            unknown += 1;
            continue;
        };

        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-9);
        out.push(EfLine { name, action: code, start, end, direction: [d[0] / l, d[1] / l, d[2] / l] });
    }

    Ok((out, unknown))
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let s = self.b.get(self.at..self.at + n).ok_or("line list ends early")?;
        self.at += n;
        Ok(s)
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }

    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }

    fn v2(&mut self) -> Result<[f32; 2], String> {
        Ok([self.f32()?, self.f32()?])
    }

    fn v3(&mut self) -> Result<[f32; 3], String> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }

    fn name(&mut self) -> Result<String, String> {
        let n = u16::from_le_bytes(self.take(2)?.try_into().expect("2 bytes")) as usize;
        let units = self.take(n * 2)?.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>();
        String::from_utf16(&units).map_err(|e| e.to_string())
    }
}

fn name(out: &mut Vec<u8>, s: &str) {
    let units = s.encode_utf16().collect::<Vec<_>>();
    out.extend_from_slice(&(units.len() as u16).to_le_bytes());
    units.iter().for_each(|u| out.extend_from_slice(&u.to_le_bytes()));
}

fn floats<'a>(out: &mut Vec<u8>, v: impl Iterator<Item = &'a f32>) {
    v.for_each(|f| out.extend_from_slice(&f.to_le_bytes()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::warp::Bend;

    #[test]
    fn docking_lines_round_trip_and_bend_into_short_lines() {
        let line = DockingLine { name: "DockingLine_piece01_destruct01_line01".into(), start: [19.931, -2.5], end: [0.068, -2.499], direction: [0.0, -1.0] };
        let bytes = write_docking_lines(std::slice::from_ref(&line));
        assert_eq!(bytes.len(), 104);
        assert_eq!(read_docking_lines(&bytes).unwrap(), vec![line.clone()]);

        let bent = warp_docking_lines(&[line], &Bend::turning(0.0, 20.0, 10f32.to_radians()), 5.0);
        assert_eq!(bent.len(), 4);
        assert_eq!(bent[3].name, "DockingLine_piece01_destruct01_line04");
        // Consecutive lines meet; the first faces out turned by most of the bend (it is at the far end).
        assert!((bent[0].end[0] - bent[1].start[0]).abs() < 1e-5 && (bent[0].end[1] - bent[1].start[1]).abs() < 1e-5);
        assert!(bent[0].direction[0] > 0.1);
    }
}
