//! How a format writes a value as attribute text, and reads it back: the kinds of a schema and their number styles.

use wce_formats::c_num::{printf_f_msvc, printf_g, trim_fraction};
use wce_tile_model::{Sym, SymCache, Value};

/// How a number is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Num {
    /// TEd: float32, `%.6f` rounded half away from zero, trailing zeros and `.` removed (`1`, `0.5`, `-0`, `6451.527832`).
    Ted,

    /// `%g` of the float32 (6 significant digits: `-6.86646e-05`, `261.169`).
    G,

    /// `%.6f` of the double, rounded half away from zero, zeros kept (`-5.000000`).
    Fixed6,

    /// [`Num::Fixed6`] of the float32.
    Fixed6F32,
}

impl Num {
    /// The value this style's text reads back as (what a file of this style can hold).
    pub fn quantise(self, v: f64) -> f64 {
        self.format(v).parse().unwrap_or(v)
    }

    pub fn format(self, v: f64) -> String {
        match self {
            Self::Ted => {
                let d = f64::from(v as f32);
                if d.is_finite() { trim_fraction(&printf_f_msvc(d, 6)).to_owned() } else { printf_g(d) }
            }

            Self::G => printf_g(f64::from(v as f32)),
            Self::Fixed6 => printf_f_msvc(v, 6),
            Self::Fixed6F32 => printf_f_msvc(f64::from(v as f32), 6),
        }
    }
}

/// What a property's text holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Str,

    /// `true` / `false`.
    Bool,
    Int,
    Float(Num),

    /// `<prefix>(x,y)`.
    Vec2(Num, &'static str),

    /// `<prefix>(x,y,z)`, or `x,y,z` with an empty prefix.
    Vec3(Num, &'static str),

    /// `euler(a,b,c)`.
    Euler(Num),

    /// `argb(r,g,b,a)`, each number [`Num::Ted`] with at least one decimal (`1.0`, `0.74902`).
    Colour,

    /// Names joined by `|`, after a space when `leading_space` (TEd: ` OIF_VISIBLE|OIF_ACTIVE`; none set is a single space).
    Flags { leading_space: bool },
}

fn numbers<const N: usize>(text: &str) -> Option<[f64; N]> {
    let mut out = [0.0; N];
    let mut it = text.split(',');
    for v in &mut out {
        *v = it.next()?.parse().ok()?;
    }

    it.next().is_none().then_some(out)
}

fn wrapped<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    if prefix.is_empty() {
        return Some(text);
    }

    text.strip_prefix(prefix)?.strip_prefix('(')?.strip_suffix(')')
}

fn colour_number(v: f64) -> String {
    let s = Num::Ted.format(v);
    if s.contains(['.', 'e', 'n']) { s } else { format!("{s}.0") }
}

impl Kind {

    /// The value of the text (`Str` when the text is not of this kind).
    pub fn parse(self, text: &str, syms: &mut SymCache) -> Value {
        let typed = match self {
            Self::Str => None,
            Self::Bool => match text {
                "true" => Some(Value::Bool(true)),
                "false" => Some(Value::Bool(false)),
                _ => None,
            },

            Self::Int => text.parse().ok().map(Value::Int),
            Self::Float(_) => text.parse().ok().map(Value::Float),
            Self::Vec2(_, prefix) => wrapped(text, prefix).and_then(numbers::<2>).map(Value::Vec2),
            Self::Vec3(_, prefix) => wrapped(text, prefix).and_then(numbers::<3>).map(Value::Vec3),
            Self::Euler(_) => wrapped(text, "euler").and_then(numbers::<3>).map(Value::Euler),
            Self::Colour => wrapped(text, "argb").and_then(numbers::<4>).map(Value::Colour),
            Self::Flags { leading_space } => {
                let body = if leading_space { text.strip_prefix(' ').unwrap_or(text) } else { text };
                if body.is_empty() {
                    Some(Value::Flags(vec![]))
                } else if body.split('|').all(|f| !f.is_empty() && !f.contains(' ')) {
                    Some(Value::Flags(body.split('|').map(|f| syms.get(f)).collect()))
                } else {
                    None
                }
            }
        };

        typed.unwrap_or_else(|| Value::Str(text.to_owned()))
    }

    /// The text of a value. The kind gives the number style and the decoration; a value of another shape (set by an editor) is written in
    /// its own shape with TEd's numbers.
    pub fn format(self, value: &Value) -> String {
        let num = match self {
            Self::Float(n) | Self::Vec2(n, _) | Self::Vec3(n, _) | Self::Euler(n) => n,
            _ => Num::Ted,
        };

        let join = |v: &[f64]| v.iter().map(|&x| num.format(x)).collect::<Vec<_>>().join(",");
        match value {
            Value::Bool(b) => if *b { "true" } else { "false" }.to_owned(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => num.format(*f),
            Value::Vec2(v) => format!("{}({})", if let Self::Vec2(_, p) = self { p } else { "vector" }, join(v)),
            Value::Vec3(v) => match self {
                Self::Vec3(_, "") => join(v),
                Self::Vec3(_, p) => format!("{p}({})", join(v)),
                _ => format!("vector({})", join(v)),
            },

            Value::Euler(v) => format!("euler({})", join(v)),
            Value::Colour(c) => format!("argb({})", c.iter().map(|&x| colour_number(x)).collect::<Vec<_>>().join(",")),
            Value::Flags(f) => {
                let names = f.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("|");
                if matches!(self, Self::Flags { leading_space: false }) { names } else { format!(" {names}") }
            }

            Value::Str(s) => s.clone(),

            // Not in TEd's files; a converted tile can carry them.
            Value::Floats(v) => join(v),
            Value::Ints(v) => v.iter().map(i64::to_string).collect::<Vec<_>>().join(","),
            Value::Bytes(v) => v.iter().map(|b| format!("{b:02x}")).collect(),
        }
    }
}

/// What a format knows about its properties: the kind of each (class, attribute); `None` for one it does not know (read as `Str`,
/// counted as unknown).
pub trait Schema: Sync {
    fn kind(&self, class: Sym, attr: Sym) -> Option<Kind>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round(kind: Kind, text: &str) -> String {
        kind.format(&kind.parse(text, &mut SymCache::default()))
    }

    #[test]
    fn numbers() {
        assert_eq!(Num::Ted.format(1.0), "1");
        assert_eq!(Num::Ted.format(-0.0000001), "-0");
        assert_eq!(Num::Ted.format(6451.527832), "6451.527832");
        assert_eq!(Num::G.format(-6.86646e-05), "-6.86646e-05");
        assert_eq!(Num::G.format(261.169), "261.169");
        assert_eq!(Num::Fixed6.format(-5.0), "-5.000000");
        assert_eq!(Num::Fixed6.format(0.0078125), "0.007813");
    }

    #[test]
    fn kinds_round_trip_teds_texts() {
        for (kind, text) in [
            (Kind::Vec3(Num::Ted, "vector"), "vector(6451.527832,133.062256,4916.137207)"),
            (Kind::Vec3(Num::G, ""), "288.074,19.1262,0.00390625"),
            (Kind::Vec2(Num::Ted, "vector"), "vector(1,2.5)"),
            (Kind::Euler(Num::Ted), "euler(1.353718,3.141593,-3.141593)"),
            (Kind::Colour, "argb(1.0,0.74902,0.545098,1.0)"),
            (Kind::Flags { leading_space: true }, " OIF_VISIBLE|OIF_ACTIVE"),
            (Kind::Flags { leading_space: true }, " "),
            (Kind::Flags { leading_space: false }, "SEASON_BF_WINTER|SEASON_BF_SPRING"),
            (Kind::Float(Num::G), "2.62413e+17"),
            (Kind::Int, "-188233471"),
            (Kind::Bool, "false"),
        ] {
            assert_eq!(round(kind, text), text, "{kind:?}");
        }
    }

    #[test]
    fn text_of_another_kind_is_kept_as_text() {
        let mut syms = SymCache::default();
        assert_eq!(Kind::Vec3(Num::Ted, "vector").parse("euler(0,0,0)", &mut syms), Value::Str("euler(0,0,0)".into()));
        assert_eq!(Kind::Bool.parse("1", &mut syms), Value::Str("1".into()));
        assert_eq!(Kind::Int.parse("1.5", &mut syms), Value::Str("1.5".into()));
        assert_eq!(Kind::Flags { leading_space: true }.parse(" A| B", &mut syms), Value::Str(" A| B".into()));
    }
}
