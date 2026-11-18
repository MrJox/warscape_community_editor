//! The curves of a `ranged_property`, as text: `alpha_min,16711680;0.0,0.000212;0.204703,0.801828|alpha_max,65280;...`. Each curve is a
//! head (its name, then the colour the game's editor drew it in) and its keys (t, value). Read and written back losslessly: an edit
//! rewrites only the keys it changed, every other byte (number style, separators, trailing ones) stays.

/// One key, with the text it was read from (None for a key an edit made or moved).
#[derive(Clone, Debug, PartialEq)]
pub struct Key {
    pub t: f32,
    pub v: f32,
    raw: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    /// `alpha_min,16711680`.
    head: String,
    pub keys: Vec<Key>,

    /// The text ended with `;` after the last key.
    trailing_semicolon: bool,
}

impl Curve {
    /// The curve's name (`alpha_min`, `outer_radius_max`).
    pub fn name(&self) -> &str {
        self.head.split(',').next().unwrap_or_default()
    }

    /// The colour the game's editor drew the curve in (`0x00BBGGRR` as the files have it, red in the low byte), as RGBA.
    pub fn colour(&self) -> Option<[f32; 4]> {
        let c = self.head.split(',').nth(1)?.trim().parse::<u32>().ok()?;
        Some([(c & 0xff) as f32 / 255.0, ((c >> 8) & 0xff) as f32 / 255.0, ((c >> 16) & 0xff) as f32 / 255.0, 1.0])
    }

    /// The keys in t order (the runtime sorts them; files have them in any order).
    pub fn points(&self) -> Vec<(f32, f32)> {
        let mut pts = self.keys.iter().map(|k| (k.t, k.v)).collect::<Vec<_>>();
        pts.sort_by(|a, b| a.0.total_cmp(&b.0));
        pts
    }

    /// The keys set to `points`: a key whose numbers did not change keeps its text.
    pub fn set_points(&mut self, points: &[(f32, f32)]) {
        let old = std::mem::take(&mut self.keys);
        self.keys = points
            .iter()
            .map(|&(t, v)| {
                let raw = old.iter().find(|k| k.t == t && k.v == v).and_then(|k| k.raw.clone());
                Key { t, v, raw }
            })
            .collect();
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Curves {
    pub curves: Vec<Curve>,

    /// The text ended with `|`.
    trailing_bar: bool,
}

/// C's `atof`: the longest numeric prefix, 0 for none.
fn atof(s: &str) -> f32 {
    wce_formats::c_num::strtod(s).unwrap_or(0.0) as f32
}

/// A number as the files write them: up to six decimals, at least one.
pub fn format_number(v: f32) -> String {
    let mut s = format!("{v:.6}");
    while s.ends_with('0') && !s.ends_with(".0") {
        s.pop();
    }

    if s == "-0.0" { "0.0".to_owned() } else { s }
}

impl Curves {
    pub fn parse(text: &str) -> Self {
        let mut out = Curves { trailing_bar: text.ends_with('|'), ..Default::default() };
        let body = text.strip_suffix('|').unwrap_or(text);
        if body.is_empty() {
            return out;
        }

        for piece in body.split('|') {
            let trailing_semicolon = piece.ends_with(';');
            let piece = piece.strip_suffix(';').unwrap_or(piece);
            let mut parts = piece.split(';');
            let head = parts.next().unwrap_or_default().to_owned();
            let keys = parts
                .filter_map(|key| {
                    let (t, v) = key.split_once(',')?;
                    Some(Key { t: atof(t), v: atof(v), raw: Some(key.to_owned()) })
                })
                .collect();
            out.curves.push(Curve { head, keys, trailing_semicolon });
        }

        out
    }

    pub fn find(&self, name: &str) -> Option<&Curve> {
        self.curves.iter().find(|c| c.name() == name)
    }

    pub fn find_mut(&mut self, name: &str) -> Option<&mut Curve> {
        self.curves.iter_mut().find(|c| c.name() == name)
    }

    pub fn write(&self) -> String {
        let mut out = String::new();
        for (i, c) in self.curves.iter().enumerate() {
            if i > 0 {
                out.push('|');
            }

            out += &c.head;
            for k in &c.keys {
                out.push(';');
                match &k.raw {
                    Some(raw) => out += raw,
                    None => out += &format!("{},{}", format_number(k.t), format_number(k.v)),
                }
            }

            if c.trailing_semicolon {
                out.push(';');
            }
        }

        if self.trailing_bar {
            out.push('|');
        }

        out
    }
}

/// `n` values of keys joined linearly over 0..1 (flat outside them), clamped to 0..1: a read-only plot.
pub fn sample(keys: &[(f32, f32)], n: usize) -> Vec<f32> {
    let eval = |t: f32| {
        let Some(first) = keys.first() else { return 0.0 };
        if t <= first.0 {
            return first.1;
        }

        for pair in keys.windows(2) {
            let ((t0, v0), (t1, v1)) = (pair[0], pair[1]);
            if t <= t1 {
                let a = if t1 > t0 { (t - t0) / (t1 - t0) } else { 1.0 };
                return v0 + (v1 - v0) * a;
            }
        }

        keys.last().map_or(0.0, |k| k.1)
    };

    (0..n).map(|i| eval(i as f32 / (n.max(2) - 1) as f32).clamp(0.0, 1.0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "alpha_min,16711680;0.0,0.000212;0.204703,0.801828|alpha_max,65280;0.0,1;1.0,0.50;|";

    #[test]
    fn reads_and_writes_back_unchanged() {
        let c = Curves::parse(TEXT);
        assert_eq!(c.curves.len(), 2);
        assert_eq!(c.curves[0].name(), "alpha_min");
        assert_eq!(c.curves[0].points(), vec![(0.0, 0.000212), (0.204703, 0.801828)]);
        assert_eq!(c.curves[0].colour(), Some([0.0, 0.0, 1.0, 1.0]));
        assert_eq!(c.write(), TEXT);
        assert_eq!(Curves::parse("").write(), "");
    }

    #[test]
    fn an_edit_rewrites_only_the_keys_it_changed() {
        let mut c = Curves::parse(TEXT);
        c.find_mut("alpha_max").unwrap().set_points(&[(0.0, 1.0), (0.5, 0.25), (1.0, 0.5)]);
        assert_eq!(c.write(), "alpha_min,16711680;0.0,0.000212;0.204703,0.801828|alpha_max,65280;0.0,1;0.5,0.25;1.0,0.50;|");
    }

    #[test]
    fn keys_out_of_order_are_read_in_order_and_kept_until_edited() {
        let text = "emission_min,255;1.0,0;0.0,0;0.5,12.0";
        let mut c = Curves::parse(text);
        assert_eq!(c.curves[0].points(), vec![(0.0, 0.0), (0.5, 12.0), (1.0, 0.0)]);
        assert_eq!(c.write(), text);
        let mut pts = c.curves[0].points();
        pts[1].1 = 10.0;
        c.curves[0].set_points(&pts);
        assert_eq!(c.write(), "emission_min,255;0.0,0;0.5,10.0;1.0,0");
    }

    #[test]
    fn numbers_are_written_as_the_files_write_them() {
        assert_eq!(format_number(0.5), "0.5");
        assert_eq!(format_number(2.0), "2.0");
        assert_eq!(format_number(-0.0), "0.0");
        assert_eq!(format_number(0.1234567), "0.123457");
    }
}
