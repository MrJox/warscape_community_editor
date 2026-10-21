//! The value of a property: what it means, not how a file writes it. Numbers are `f64` (every game's float32 and double fit), vectors and
//! colours are arrays, flags are names. A value a reader could not type is kept as its text (`Str`), so nothing is lost.

use crate::sym::Sym;

/// Equality is the values' meaning: two NaNs are the same value (TEd writes `lf_offset="nan"` in some tiles), unlike IEEE comparison.
#[derive(Clone, Debug)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Float(f64),
    Vec2([f64; 2]),
    Vec3([f64; 3]),

    /// Three angles in radians (TEd: `euler(a,b,c)`, the rotation `Rz(c) * Ry(a) * Rx(-b)`).
    Euler([f64; 3]),

    /// Red, green, blue, alpha in `[0, 1]`.
    Colour([f64; 4]),

    /// The flags that are set, in the source's order (TEd: `OIF_VISIBLE|OIF_ACTIVE`, `SEASON_BF_WINTER|...`).
    Flags(Vec<Sym>),
    Str(String),
}

fn same(a: f64, b: f64) -> bool {
    a == b || (a.is_nan() && b.is_nan())
}

fn same_all(a: &[f64], b: &[f64]) -> bool {
    a.iter().zip(b).all(|(&x, &y)| same(x, y))
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => same(*a, *b),
            (Self::Vec2(a), Self::Vec2(b)) => same_all(a, b),
            (Self::Vec3(a), Self::Vec3(b)) | (Self::Euler(a), Self::Euler(b)) => same_all(a, b),
            (Self::Colour(a), Self::Colour(b)) => same_all(a, b),
            (Self::Flags(a), Self::Flags(b)) => a == b,
            (Self::Str(a), Self::Str(b)) => a == b,
            _ => false,
        }
    }
}

impl Value {
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(v) => Some(*v),
            _ => None,
        }
    }

    /// A float, or an int read as one.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Float(v) => Some(*v),
            Self::Int(v) => Some(*v as f64),
            _ => None,
        }
    }

    pub fn as_vec3(&self) -> Option<[f64; 3]> {
        match self {
            Self::Vec3(v) | Self::Euler(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_flags(&self) -> Option<&[Sym]> {
        match self {
            Self::Flags(v) => Some(v),
            _ => None,
        }
    }

    /// Whether the flag is set (`false` when this is not a flag value).
    pub fn has_flag(&self, flag: &str) -> bool {
        self.as_flags().is_some_and(|f| f.iter().any(|s| *s == flag))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nan_equals_nan() {
        assert_eq!(Value::Float(f64::NAN), Value::Float(f64::NAN));
        assert_eq!(Value::Vec3([0.0, f64::NAN, 1.0]), Value::Vec3([0.0, f64::NAN, 1.0]));
        assert_ne!(Value::Float(f64::NAN), Value::Float(0.0));
        assert_ne!(Value::Vec3([1.0; 3]), Value::Euler([1.0; 3]));
    }
}
