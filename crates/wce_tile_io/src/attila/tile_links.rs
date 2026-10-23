//! BOB's offline blend of a tile's heights into its linking tiles' (ToolDataBuilderDll `FUN_10015fd0`, after the final heights): for
//! each blending link (its tile set has a linking tile, `TLT_EQUALS`, no `no_offline_blend`; sea links first), in each of the four
//! rotations, a strip of `blend_size` along the cell edge the link touches eases the heights into the linking tile's
//! `final_heights.dds`, rotated, tiled from the cell's corner. docs/formats/tile_compile.md has the RE.

use std::collections::HashMap;
use std::path::Path;

use wce_tile_model::{Node, Tile};

use super::bob_tile::CELL;

/// A region triangle (`FUN_100148a0`): three points and the linking tile's weight at each.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Tri {
    p: [[f32; 2]; 3],
    w: [f32; 3],
}

/// The barycentric weights of `(px, py)` on p1 and p2 (`FUN_100140b0`): the areas as the lengths of 3D cross products (z 0), in f32,
/// their square roots and products on the x87 in double.
fn barycentric(t: &Tri, px: f32, py: f32) -> (f32, f32) {
    let [[x0, y0], [x1, y1], [x2, y2]] = t.p;
    let (ay, ax, bx, by) = (y1 - y0, x1 - x0, x2 - x0, y2 - y0);
    let cz = ay * bx - by * ax;
    let total = f64::from(cz * cz).sqrt() * 0.5;
    let inv = (1.0 / total) as f32;
    let (dy, dx) = (py - y0, px - x0);
    let cu = dy * bx - by * dx;
    let cv = dy * ax - ay * dx;
    let area = |c: f32| (f64::from(c * c).sqrt() * 0.5 * f64::from(inv)) as f32;
    (area(cu), area(cv))
}

/// The linking tile's weight at `(px, py)` (`FUN_10014520`): the first triangle the point is inside (the three edge signs agree) gives
/// it, when its barycentric weights are all within [-0.0001, 1.0001]; otherwise none.
fn weight(tris: &[Tri], px: f32, py: f32) -> Option<f32> {
    let (lo, hi) = (f32::from_bits(0xb8d1_b717), f32::from_bits(0x3f80_0347));
    for t in tris {
        let [[x0, y0], [x1, y1], [x2, y2]] = t.p;
        let s1 = (y0 - y1) * (px - x1) - (x0 - x1) * (py - y1) < 0.0;
        let s2 = (y1 - y2) * (px - x2) - (x1 - x2) * (py - y2) < 0.0;
        let s3 = (px - x0) * (y2 - y0) - (py - y0) * (x2 - x0) < 0.0;
        if s1 != s2 || s2 != s3 {
            continue;
        }

        let (u, v) = barycentric(t, px, py);
        let r = 1.0 - u - v;
        if [u, v, r].iter().any(|&c| lo > c || c > hi) {
            return None;
        }

        return Some(t.w[2] * v + t.w[1] * u + t.w[0] * r);
    }

    None
}

/// The link's region (`FUN_100148a0`): for each side of cell `(cx, cy)` (rows down) the inverted link point `(lx, ly)` lies past, two
/// triangles over the `blend` strip along that side, weight 1 on the edge and 0 inside (or, with a 4-point `blend_quad`, its two
/// triangles). For the right and bottom sides the cell's corner first moves back `max((blend / size - 1) x size, 0)`.
fn region(link: (i32, i32), corner: (i32, i32), cell: (i32, i32), size: f32, blend: f32, quad: &[[f32; 3]]) -> Vec<Tri> {
    let ((lx, ly), (mut x0, mut y0), (cx, cy)) = (link, corner, cell);
    let mut out = vec![];
    let mut push = |a: Tri, b: Tri| {
        if let [q0, q1, q2, q3] = quad {
            let tri = |a: &[f32; 3], b: &[f32; 3], c: &[f32; 3]| Tri { p: [[a[0], a[1]], [b[0], b[1]], [c[0], c[1]]], w: [a[2], b[2], c[2]] };
            out.push(tri(q0, q1, q2));
            out.push(tri(q0, q2, q3));
        } else {
            out.push(a);
            out.push(b);
        }
    };
    let back = || ((((blend / size) as i32 - 1) as f32) * size).max(0.0);
    if lx < cx {
        let (x, y) = (x0 as f32, y0 as f32);
        let (xb, y1) = (x + blend, y + size);
        push(Tri { p: [[x, y], [xb, y], [xb, y1]], w: [1.0, 0.0, 0.0] }, Tri { p: [[x, y], [xb, y1], [x, y1]], w: [1.0, 0.0, 1.0] });
    }
    if lx > cx {
        x0 = (x0 as f32 - back()) as i32;
        let (x, y) = (x0 as f32, y0 as f32);
        let (xr, y1) = (x + size, y + size);
        let xb = xr - blend;
        push(Tri { p: [[xb, y], [xr, y], [xr, y1]], w: [0.0, 1.0, 1.0] }, Tri { p: [[xb, y], [xr, y1], [xb, y1]], w: [0.0, 1.0, 0.0] });
    }
    if ly < cy {
        let (x, y) = (x0 as f32, y0 as f32);
        let (xr, yb) = (x + size, y + blend);
        push(Tri { p: [[x, y], [xr, y], [xr, yb]], w: [1.0, 1.0, 0.0] }, Tri { p: [[x, y], [xr, yb], [x, yb]], w: [1.0, 0.0, 0.0] });
    }
    if ly > cy {
        y0 = (y0 as f32 - back()) as i32;
        let (x, y) = (x0 as f32, y0 as f32);
        let (xr, y1) = (x + size, y + size);
        let yb = y1 - blend;
        push(Tri { p: [[x, yb], [xr, yb], [xr, y1]], w: [0.0, 0.0, 1.0] }, Tri { p: [[x, yb], [xr, y1], [x, y1]], w: [0.0, 1.0, 1.0] });
    }

    out
}

/// `TILE_MAP::rotate_in_tile_space`: a point of a `w` x `h` tile turned by rotation `r` (0 to 3: none, 90, 180, 270 degrees).
fn rotate(w: i32, h: i32, r: usize, x: i32, y: i32) -> (i32, i32) {
    match r {
        0 => (x, y),
        1 => (h - 1 - y, x),
        2 => (w - 1 - x, h - 1 - y),
        _ => (y, w - 1 - x),
    }
}

/// A scalar field: width, height, values row-major.
type Field = (usize, usize, Vec<f32>);

/// `get_scalar_field_from_file` (`FUN_100109b0`): `<path>.png` (16-bit grey, / 65535) or else `<path>.dds` (R32F), turned by rotation
/// `r` (1: source (x, y) to (h - 1 - y, x) as column, row; 2: (w - 1 - x, h - 1 - y); 3: (y, w - 1 - x)).
fn load_field(path: &Path, r: usize) -> Option<Field> {
    let (w, h, src) = if let Ok(b) = std::fs::read(path.with_extension("png")) {
        let png = wce_formats::png_file::PngFile::parse(&b).ok()?;
        let (w, h) = (png.width as usize, png.height as usize);
        if png.bytes_per_pixel() != 2 {
            return None;
        }

        let v: Vec<f32> = png.pixels.as_chunks::<2>().0.iter().map(|p| f32::from(u16::from_be_bytes(*p)) * (1.0 / 65535.0)).collect();
        (w, h, v)
    } else {
        let b = std::fs::read(path.with_extension("dds")).ok()?;
        let u32_at = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().expect("4 bytes")) as usize);
        let (h, w) = (u32_at(12)?, u32_at(16)?);
        if u32_at(84)? != 0x72 {
            return None;
        }

        (w, h, b.get(128..128 + w * h * 4)?.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect())
    };
    let (dw, dh) = if r % 2 == 1 { (h, w) } else { (w, h) };
    let mut out = vec![0.0f32; dw * dh];
    for y in 0..h {
        for x in 0..w {
            let (col, row) = match r {
                0 => (x, y),
                1 => (h - 1 - y, x),
                2 => (w - 1 - x, h - 1 - y),
                _ => (y, w - 1 - x),
            };
            out[row * dw + col] = src[y * w + x];
        }
    }

    Some((dw, dh, out))
}

/// The width and height of the database tile at `location` (`TILE_DATABASE::tile_from_location`: the variation dropped).
fn tile_size_of(raw_data: &Path, location: &str) -> Option<(i32, i32)> {
    let tile = location.trim_end_matches('\\');
    let tile = tile.rsplit_once('\\').map_or(tile, |(t, _)| t).to_lowercase();
    let dir = raw_data.join("terrain/tiles/battle/_tile_database/TILES");
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let text = std::fs::read_to_string(e.path()).unwrap_or_default();
        let head = text.split('>').next().unwrap_or_default();
        let attr = |k: &str| head.split(&format!(" {k}='")).nth(1).and_then(|v| v.split('\'').next());
        if attr("location").is_some_and(|l| l.to_lowercase() == tile) {
            return Some((attr("width")?.parse().ok()?, attr("height")?.parse().ok()?));
        }
    }

    None
}

fn int(n: &Node, k: &str) -> i32 {
    n.get(k).and_then(|v| v.as_int()).unwrap_or(0) as i32
}

fn text<'a>(n: &'a Node, k: &str) -> &'a str {
    n.get(k).and_then(|v| v.as_str()).unwrap_or_default()
}

/// `TILE_RAW_DATA::apply_straight_self_tiling` (`FUN_10016410`), for a tile that tiles with itself
/// ([`super::bob_tile::self_tiling`]): its two blending links (the first two non-sea `TLT_EQUALS` links without `no_offline_blend`)
/// on opposite sides make the tile repeat seamlessly that way. Counting the non-sea `TLT_EQUALS` links (with `no_offline_blend` too)
/// and noting a sea one; per column i (links left and right) or row i (above and below), `t = clamp(i x (1 / blend), 0, 1)` (x87,
/// rounded to f32), the line eased into its mirror (`size - i`):
/// - left and right, exactly two such links and no sea link: from a copy of the source, `(1 - t) c[j][i] + c[j][mirror] t`; then
///   each row averaged with its mirror row, `(c[mirror][i] + c[j][i]) x 0.5`, from a copy of the result;
/// - left and right otherwise: `f[j][mirror] t + (1 - t) s[j][i]`, the field read as it changes;
/// - above and below: with two links (in all), two such and no sea link, first each row averaged with its mirror column
///   (`(f[j][mirror] + f[j][i]) x 0.5` into both); then `(1 - t) s[i][j] + f[mirror][j] t`.
///
/// The source is the tile's own field when both links' linking tiles end with its variation's location, else the first link's
/// linking tile's `final_heights` (not seen in the kit). Returns whether it applied (BOB then blends no other link).
fn straight_self_tiling(raw: &Tile, entry: &Node, raw_data: &Path, linking_tiles: &HashMap<String, String>, field: &mut [f32], w: usize, h: usize) -> Result<bool, String> {
    let links = entry.wrapped("TILE_LINKS");
    let (mut count, mut sea, mut pair) = (0, None, vec![]);
    for (i, l) in links.iter().enumerate() {
        if text(l, "test") != "TLT_EQUALS" {
            continue;
        }

        if text(l, "link_set") == "sea" {
            sea = Some(i);
            continue;
        }

        count += 1;
        if !l.get("no_offline_blend").and_then(|v| v.as_bool()).unwrap_or(false) {
            if pair.len() == 2 {
                return Ok(false);
            }

            pair.push(i);
        }
    }

    let [first, second] = pair[..] else { return Ok(false) };
    let location = text(entry, "location").to_lowercase();
    let linking = |i: usize| linking_tiles.get(text(&links[i], "link_set")).map(|t| t.to_lowercase()).unwrap_or_default();
    let (one, two) = (linking(first), linking(second));
    if !one.starts_with(&location) || !two.starts_with(&location) {
        return Ok(false);
    }

    let th = int(entry, "height");
    let inverted = |i: usize| (int(&links[i], "x"), th - 1 - int(&links[i], "y"));
    let (a, b) = (inverted(first), inverted(second));
    if a.0 + b.0 != 0 || a.1 + b.1 != 0 {
        return Ok(false);
    }

    let variation = raw.variation.as_deref().unwrap_or_default().to_lowercase();
    let own = one.ends_with(&variation) && two.ends_with(&variation);
    let source = if own {
        None
    } else {
        let file = raw_data.join(one.replace('\\', "/")).join("final_heights");
        Some(load_field(&file, 0).ok_or_else(|| format!("You need to convert {one} first."))?)
    };
    let s = |field: &[f32], row: usize, col: usize| source.as_ref().map_or(field[row * w + col], |(sw, _, v)| v[row * sw + col]);
    let size = super::bob_tile::HeightNumbers::of(raw).tri_density;
    let inv = 1.0f32 / int(&links[first], "blend_size") as u32 as f32;
    let t_of = |i: usize| ((f64::from(i as u32) * f64::from(inv)) as f32).clamp(0.0, 1.0);
    let two_clean = count == 2 && sea.is_none();
    if a.1 == 0 {
        if two_clean {
            let copy = (0..h).flat_map(|r| (0..w).map(move |c| (r, c))).map(|(r, c)| s(field, r, c)).collect::<Vec<_>>();
            for i in 0..=size {
                let t = t_of(i);
                for j in 0..=size {
                    field[j * w + i] = (1.0 - t) * copy[j * w + i] + copy[j * w + (w - i - 1)] * t;
                }
            }

            let copy = field.to_vec();
            for c in 0..=size {
                for r in 0..=size {
                    field[r * w + c] = (copy[(h - r - 1) * w + c] + copy[r * w + c]) * 0.5;
                }
            }
        } else {
            for i in 0..=size {
                let t = t_of(i);
                for j in 0..=size {
                    let v = s(field, j, i);
                    field[j * w + i] = field[j * w + (w - i - 1)] * t + (1.0 - t) * v;
                }
            }
        }
    } else {
        if links.len() == 2 && two_clean {
            for r in 0..=size {
                for c in 0..size / 2 {
                    let v = (field[r * w + (w - c - 1)] + field[r * w + c]) * 0.5;
                    field[r * w + c] = v;
                    field[r * w + (w - c - 1)] = v;
                }
            }
        }

        for i in 0..=size {
            let t = t_of(i);
            for j in 0..=size {
                let v = s(field, i, j);
                field[i * w + j] = (1.0 - t) * v + field[(h - i - 1) * w + j] * t;
            }
        }
    }

    Ok(true)
}

/// Blends `field` (the final heights, `w` x `h`, rows as `final_heights.dds`) into the project's linking tiles, as BOB does before it
/// builds the meshes. `raw_data` is the kit's, where the linking tiles' `final_heights.dds` are; `linking_tiles` maps a tile set to its
/// linking tile. A tile that tiles with itself (BOB's `apply_straight_self_tiling`) is left as it is: not ported.
///
/// # Errors
///
/// A linking tile that is not in the database, or whose heights are missing ("You need to convert ... first").
pub fn blend_links(raw: &Tile, raw_data: &Path, linking_tiles: &HashMap<String, String>, field: &mut [f32], w: usize, h: usize) -> Result<(), String> {
    let Some(entry) = raw.entry.as_ref().map(|e| &e.root) else { return Ok(()) };
    if super::bob_tile::self_tiling(entry, linking_tiles) && straight_self_tiling(raw, entry, raw_data, linking_tiles, field, w, h)? {
        return Ok(());
    }

    let (tw, th) = (int(entry, "width"), int(entry, "height"));
    let location = text(entry, "location");
    let variation = entry.wrapped("VARIATIONS").iter().find(|v| v.get("location").and_then(|l| l.as_str()) == raw.variation.as_deref());
    let density = variation.map_or(CELL as i32, |v| int(v, "raw_data_tri_density")).max(1);
    let k = (CELL as i32 / density) as u32;
    // BOB's tile size: the subtile in samples.
    let size = density;
    let mask = super::bob_tile::entry_mask(raw);
    let targets = entry.wrapped("TILE_LINK_TARGETS").iter().map(|t| (int(t, "x"), int(t, "y"))).collect::<Vec<_>>();
    let links = entry.wrapped("TILE_LINKS");
    for sea in [true, false] {
        for l in links {
            let set = text(l, "link_set");
            let Some(linking) = linking_tiles.get(set).filter(|t| !t.is_empty()) else { continue };
            let no_blend = l.get("no_offline_blend").and_then(|v| v.as_bool()).unwrap_or(false);
            if (set == "sea") != sea || text(l, "test") != "TLT_EQUALS" || no_blend {
                continue;
            }

            let (lw, lh) = tile_size_of(raw_data, linking).ok_or_else(|| format!("Failed to find link tile {linking} in database."))?;
            let is_self = linking.trim_end_matches('\\').rsplit_once('\\').is_some_and(|(t, _)| t.eq_ignore_ascii_case(location));
            let point = (int(l, "x"), int(l, "y"));
            let base = (int(l, "base_x"), int(l, "base_y"));
            let inverted = (point.0, th - 1 - point.1);
            let blend = int(l, "blend_size") as f32;
            let quad = l.wrapped("blend_quad").iter().map(|p| [0, 1, 2].map(|i| p.get(["x", "y", "z"][i]).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32)).collect::<Vec<_>>();
            // With a target cell next to the link point, only the rotations that put the target there count.
            let adjacent = targets.iter().any(|t| (t.0 - point.0).abs() + (t.1 - point.1).abs() == 1 && (t.0 == point.0 || t.1 == point.1));
            for r in 0..4 {
                let (rx, ry) = rotate(lw, lh, r, base.0, base.1);
                if adjacent && !targets.iter().any(|&t| t == (point.0 + rx, point.1 - ry)) {
                    continue;
                }

                let cell = (rx + inverted.0, ry + inverted.1);
                if cell.0 < 0 || cell.0 >= tw || cell.1 < 0 || cell.1 >= th {
                    continue;
                }

                // A masked tile's link into an invalid subtile (the cell is rows down, the mask's rows from the bottom).
                if !mask.is_empty() && !super::bob_tile::subtile_valid(&mask, tw as usize, th as usize, cell.0, th - cell.1 - 1) {
                    continue;
                }

                let corner = (size * cell.0.clamp(0, tw - 1), size * cell.1.clamp(0, th - 1));
                let file = raw_data.join(linking.replace('\\', "/")).join(if is_self { "height_map_0" } else { "final_heights" });
                let (fw, fh, link_field) = load_field(&file, r).ok_or_else(|| format!("You need to convert {linking} first."))?;
                let tris = region(inverted, corner, cell, size as f32, blend, &quad);
                if tris.is_empty() {
                    continue;
                }

                let mut b = [f32::MAX, f32::MAX, -f32::MAX, -f32::MAX];
                for p in tris.iter().flat_map(|t| t.p) {
                    b = [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])];
                }

                let mut y = (b[1] - 1.0) as i32;
                while (y as f32) < b[3] + 1.0 {
                    let mut x = (b[0] - 1.0) as i32;
                    while (x as f32) < b[2] + 1.0 {
                        if let Some(wt) = weight(&tris, x as f32, y as f32)
                            && x >= 0
                            && y >= 0
                            && (x as usize) < w
                            && (y as usize) < h
                        {
                            let i = y as usize * w + x as usize;
                            let lx = ((x - corner.0) as u32).wrapping_mul(k) % fw as u32;
                            let ly = ((y - corner.1) as u32).wrapping_mul(k) % fh as u32;
                            field[i] = (1.0 - wt) * field[i] + link_field[ly as usize * fw + lx as usize] * wt;
                        }

                        x += 1;
                    }

                    y += 1;
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_strip_eases_from_the_edge() {
        // A link above cell (0, 0), a 64 m blend: weight 1 on row 0, half way at row 32, none from row 64.
        let tris = region((0, -1), (0, 0), (0, 0), 128.0, 64.0, &[]);
        assert_eq!(tris.len(), 2);
        assert_eq!(weight(&tris, 40.0, 0.0), Some(1.0));
        assert_eq!(weight(&tris, 40.0, 32.0), Some(0.5));
        assert_eq!(weight(&tris, 40.0, 64.0), Some(0.0));
        assert_eq!(weight(&tris, 40.0, 70.0), None);
        // A link right of cell (1, 0) moves nothing back with a blend under a cell.
        let tris = region((2, 0), (128, 0), (1, 0), 128.0, 24.0, &[]);
        assert_eq!(weight(&tris, 256.0, 50.0), Some(1.0));
        assert_eq!(weight(&tris, 232.0, 50.0), Some(0.0));
    }

    #[test]
    fn rotations_in_tile_space() {
        assert_eq!([0, 1, 2, 3].map(|r| rotate(2, 3, r, 0, 0)), [(0, 0), (2, 0), (1, 2), (0, 1)]);
    }
}
