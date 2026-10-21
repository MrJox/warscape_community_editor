//! Interned names: element classes, property names and flag names. A tile holds millions of properties with a few hundred distinct names,
//! so a name is a `u32` into one process-wide table, and its text lives as long as the process.

use std::collections::HashMap;
use std::fmt;
use std::sync::{OnceLock, RwLock};

/// An interned name. Two `Sym`s are equal when their texts are.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Sym(u32);

#[derive(Default)]
struct Table {
    ids: HashMap<&'static str, u32>,
    names: Vec<&'static str>,
}

fn table() -> &'static RwLock<Table> {
    static TABLE: OnceLock<RwLock<Table>> = OnceLock::new();
    TABLE.get_or_init(Default::default)
}

impl Sym {

    /// The name's symbol, added to the table the first time.
    pub fn new(name: &str) -> Self {
        if let Some(&id) = table().read().expect("sym table").ids.get(name) {
            return Self(id);
        }

        let mut t = table().write().expect("sym table");
        if let Some(&id) = t.ids.get(name) {
            return Self(id);
        }

        let text: &'static str = Box::leak(name.to_owned().into_boxed_str());
        let id = u32::try_from(t.names.len()).expect("fewer than 2^32 names");
        t.names.push(text);
        t.ids.insert(text, id);
        Self(id)
    }

    pub fn as_str(self) -> &'static str {
        table().read().expect("sym table").names[self.0 as usize]
    }
}

impl From<&str> for Sym {
    fn from(name: &str) -> Self {
        Self::new(name)
    }
}

impl PartialEq<str> for Sym {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for Sym {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl fmt::Debug for Sym {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

impl fmt::Display for Sym {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A cache in front of the table for a reader that interns millions of names on one thread (no lock per name).
#[derive(Default)]
pub struct SymCache {
    map: HashMap<Box<str>, Sym>,
}

impl SymCache {
    pub fn get(&mut self, name: &str) -> Sym {
        if let Some(&s) = self.map.get(name) {
            return s;
        }

        let s = Sym::new(name);
        self.map.insert(name.into(), s);
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_text_same_symbol() {
        let a = Sym::new("BUILDING_INSTANCE");
        let b = Sym::from("BUILDING_INSTANCE");
        assert_eq!(a, b);
        assert_ne!(a, Sym::new("VEGETATION_INSTANCE"));
        assert_eq!(a.as_str(), "BUILDING_INSTANCE");
        assert!(a == "BUILDING_INSTANCE");
        assert_eq!(SymCache::default().get("BUILDING_INSTANCE"), a);
    }
}
