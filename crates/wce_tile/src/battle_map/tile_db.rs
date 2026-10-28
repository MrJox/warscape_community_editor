//! The battle tile database as BOB's Terrain processor sees it (WARSCAPE::TILE_DATABASE, Warscape.AssemblyKit.dll): the tile sets and climates
//! of `_tile_database/_settings.xml` and the tiles of `_tile_database/TILES/*.xml`.
//!
//! Ids (`post_load_fixup`, 0x1029bc50): tile set `i` is `0x80000000 | (i + 1)`; the tiles are sorted (`sort`, the comparator fn 0x1038ed10:
//! larger width * height first, then more link targets, then the location ascending) and tile `i` is `i`. A variation is
//! `0x40000000 | index << 16 | tile id`. The comparator is a strict total order (locations are unique), so the sort algorithm doesn't matter.

use std::path::Path;

use crate::vegetation::ca_xml::{self, Element};

/// The tile id bit of a tile set.
pub const SET_BIT: u32 = 0x8000_0000;

/// The id bit of a tile's variation.
pub const VARIATION_BIT: u32 = 0x4000_0000;

/// `TILE_LINK_TEST`: TLT_EQUALS is 0, anything else 1 (`string_to_enum`, 0x100012f0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkTest {
    Equals,
    NotEquals,
}

#[derive(Clone, Debug)]
pub struct TileSet {
    pub name: String,
    pub colour: [f32; 3],
    pub link_as_set: String,
    pub linking_tile: String,
    pub shared_geometry: String,
    pub also_place_tile_set: String,
    pub id: u32,
}

impl TileSet {
    /// The name links compare (fn 0x10299970): `link_as_set`, or the set's own name.
    pub fn link_name(&self) -> &str {
        if self.link_as_set.is_empty() { &self.name } else { &self.link_as_set }
    }
}

#[derive(Clone, Debug)]
pub struct Climate {
    pub name: String,
    pub colour: [f32; 3],
}

#[derive(Clone, Debug)]
pub struct Variation {
    pub location: String,
    pub colour: [f32; 3],
    pub barbarian: bool,
}

#[derive(Clone, Debug)]
pub struct Link {
    /// The set named by `link_set` (`None`: not a set of the database).
    pub set: Option<usize>,
    pub x: i32,
    pub y: i32,
    pub test: LinkTest,
    pub is_entry: bool,
}

#[derive(Clone, Debug)]
pub struct LinkTarget {
    pub set: Option<usize>,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug)]
pub struct Tile {
    /// `terrain\tiles\battle\<set>\<size>`, as the XML writes it.
    pub location: String,
    pub set: Option<usize>,
    pub width: i32,
    pub height: i32,
    pub colour: [f32; 3],

    /// One flag per cell (`string_to_mask`: '1' is part of the tile), row by row; empty: not masked.
    pub mask: Vec<bool>,
    pub random_rotatable: bool,
    pub variations: Vec<Variation>,
    pub links: Vec<Link>,
    pub link_targets: Vec<LinkTarget>,
    pub id: u32,
}

impl Tile {
    pub fn masked(&self) -> bool {
        !self.mask.is_empty()
    }

    /// `subtile_masked_valid` (0x10299ff0): a cell inside the tile, and in the mask when there is one of the tile's size.
    pub fn subtile_masked_valid(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return false;
        }

        if self.mask.is_empty() {
            return true;
        }

        if self.mask.len() != (self.width * self.height) as usize {
            return false;
        }

        self.mask[(self.width * y + x) as usize]
    }

    pub fn variation_id(&self, index: usize) -> u32 {
        (index as u32) << 16 | VARIATION_BIT | (self.id & 0xffff)
    }
}

#[derive(Clone, Debug, Default)]
pub struct TileDatabase {
    pub sets: Vec<TileSet>,
    pub climates: Vec<Climate>,
    pub tiles: Vec<Tile>,
}

fn attr<'a>(e: &'a Element, name: &str) -> &'a str {
    e.attribute(name).unwrap_or_default()
}

fn num(e: &Element, name: &str) -> f32 {
    attr(e, name).trim().parse().unwrap_or(0.0)
}

fn colour(e: &Element) -> [f32; 3] {
    [num(e, "red"), num(e, "green"), num(e, "blue")]
}

fn flag(e: &Element, name: &str) -> bool {
    attr(e, name) == "true"
}

/// The root element of a document.
fn root(bytes: &[u8]) -> Element {
    ca_xml::parse(bytes).children.into_iter().next().unwrap_or_default()
}

impl TileDatabase {

    /// Reads `<folder>/_settings.xml` and `<folder>/TILES/*.xml` (`folder`: the kit's `terrain/tiles/battle/_tile_database`).
    ///
    /// # Errors
    ///
    /// Fails when `_settings.xml` can't be read.
    pub fn load(folder: &Path) -> Result<Self, String> {
        let settings = std::fs::read(folder.join("_settings.xml")).map_err(|e| format!("{}: {e}", folder.join("_settings.xml").display()))?;
        let mut db = Self::default();
        let settings = root(&settings);
        for c in settings.child("CLIMATES").map(|c| c.children.as_slice()).unwrap_or_default().iter().filter(|c| c.name == "CLIMATE") {
            db.climates.push(Climate { name: attr(c, "name").to_owned(), colour: colour(c) });
        }

        for s in settings.child("TILE_SETS").map(|c| c.children.as_slice()).unwrap_or_default().iter().filter(|c| c.name == "TILE_SET") {
            db.sets.push(TileSet {
                name: attr(s, "name").to_owned(),
                colour: colour(s),
                link_as_set: attr(s, "link_as_set").to_owned(),
                linking_tile: attr(s, "linking_tile").to_owned(),
                shared_geometry: attr(s, "shared_geometry").to_owned(),
                also_place_tile_set: attr(s, "also_place_tile_set").to_owned(),
                id: 0,
            });
        }

        // merge_all_from_folder (0x1029e3a0) adds these two sets when the database lacks them.
        for name in ["user", "assembly_kit"] {
            if !db.sets.iter().any(|s| s.name == name) {
                db.sets.push(TileSet {
                    name: name.to_owned(), colour: [0.0; 3], link_as_set: String::new(), linking_tile: String::new(), shared_geometry: String::new(),
                    also_place_tile_set: String::new(), id: 0,
                });
            }
        }

        let mut files = std::fs::read_dir(folder.join("TILES")).map(|d| d.filter_map(Result::ok).map(|e| e.path()).collect::<Vec<_>>()).unwrap_or_default();
        files.retain(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("xml")));
        files.sort();
        for path in files {
            let Ok(bytes) = std::fs::read(&path) else { continue };
            let t = root(&bytes);
            if t.name != "TILE" {
                continue;
            }

            let set_of = |name: &str| db.sets.iter().position(|s| s.name == name);
            let list = |parent: &str, child: &str| t.child(parent).map(|c| c.children.iter().filter(|e| e.name == child).cloned().collect::<Vec<_>>()).unwrap_or_default();
            let tile = Tile {
                location: attr(&t, "location").to_owned(),
                set: set_of(attr(&t, "tile_set")),
                width: attr(&t, "width").trim().parse().unwrap_or(1),
                height: attr(&t, "height").trim().parse().unwrap_or(1),
                colour: colour(&t),
                mask: attr(&t, "mask").bytes().map(|c| c == b'1').collect(),
                random_rotatable: flag(&t, "random_rotatable"),
                variations: list("VARIATIONS", "VARIATION").iter().map(|v| Variation { location: attr(v, "location").to_owned(), colour: colour(v), barbarian: flag(v, "barbarian") }).collect(),
                links: list("TILE_LINKS", "TILE_LINK").iter().map(|l| Link {
                    set: set_of(attr(l, "link_set")),
                    x: attr(l, "x").trim().parse().unwrap_or(0),
                    y: attr(l, "y").trim().parse().unwrap_or(0),
                    test: if attr(l, "test") == "TLT_EQUALS" { LinkTest::Equals } else { LinkTest::NotEquals },
                    is_entry: flag(l, "is_entry"),
                }).collect(),
                link_targets: list("TILE_LINK_TARGETS", "TILE_LINK_TARGET").iter().map(|l| LinkTarget {
                    set: set_of(attr(l, "tile_set")),
                    x: attr(l, "x").trim().parse().unwrap_or(0),
                    y: attr(l, "y").trim().parse().unwrap_or(0),
                }).collect(),
                id: 0,
            };

            db.tiles.push(tile);
        }

        db.fix_up();
        Ok(db)
    }

    /// A tile as `load` reads it, from an entry (the open tile's, with its unsaved edits); its id is not one of the database's.
    pub fn tile_from_entry(&self, e: &wce_formats::tile_entry::TileEntry) -> Tile {
        let set_of = |name: &str| self.sets.iter().position(|s| s.name == name);
        Tile {
            location: e.location(),
            set: set_of(&e.tile_set()),
            width: e.width().max(1) as i32,
            height: e.height().max(1) as i32,
            colour: [e.red() as f32, e.green() as f32, e.blue() as f32],
            mask: e.mask().bytes().map(|c| c == b'1').collect(),
            random_rotatable: e.random_rotatable(),
            variations: e.variations().iter().map(|v| Variation { location: v.location.clone(), colour: [0.0; 3], barbarian: false }).collect(),
            links: e.links().iter().map(|l| Link {
                set: set_of(&l.link_set),
                x: l.x,
                y: l.y,
                test: if l.test == "TLT_EQUALS" { LinkTest::Equals } else { LinkTest::NotEquals },
                is_entry: l.is_entry,
            }).collect(),
            link_targets: e.link_targets().iter().map(|t| LinkTarget { set: set_of(&t.tile_set), x: t.x, y: t.y }).collect(),
            id: u32::MAX,
        }
    }

    /// `post_load_fixup`: the sort and the ids.
    fn fix_up(&mut self) {
        self.tiles.sort_by(|a, b| {
            let (aa, ba) = (a.width * a.height, b.width * b.height);
            ba.cmp(&aa).then(b.link_targets.len().cmp(&a.link_targets.len())).then_with(|| a.location.as_bytes().cmp(b.location.as_bytes()))
        });

        for (i, t) in self.tiles.iter_mut().enumerate() {
            t.id = i as u32;
        }

        for (i, s) in self.sets.iter_mut().enumerate() {
            s.id = SET_BIT | (i as u32 + 1);
        }
    }

    /// `tile_from_id` (0x1029d150): any id without the set bit is `tiles[id & 0xffff]` (0 is the first tile).
    pub fn tile_from_id(&self, id: u32) -> Option<usize> {
        if id & SET_BIT != 0 {
            return None;
        }

        let i = (id & 0xffff) as usize;
        (i < self.tiles.len()).then_some(i)
    }

    /// `tile_set_from_id` (0x1029d230).
    pub fn set_from_id(&self, id: u32) -> Option<usize> {
        self.sets.iter().position(|s| s.id == id)
    }

    pub fn set_from_name(&self, name: &str) -> Option<usize> {
        self.sets.iter().position(|s| s.name == name)
    }

    /// `tiles_match` (0x1029d550).
    pub fn tiles_match(&self, a: u32, b: u32) -> bool {
        match (self.tile_from_id(a), self.tile_from_id(b)) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        }
    }

    /// `tile_sets_match` (0x1029d300) without transition sets (no tile of the XML database has one).
    pub fn tile_sets_match(&self, a: u32, b: u32) -> bool {
        if a == 0 || b == 0 {
            return false;
        }

        let set_of = |id: u32| if id & SET_BIT != 0 { self.set_from_id(id) } else { self.tile_from_id(id).and_then(|t| self.tiles[t].set) };
        let (sa, sb) = (set_of(a), set_of(b));
        sa.is_some() && sa == sb
    }

    pub fn tile_location_of(&self, tile: usize, variation: usize) -> String {
        format!("{}\\{}", self.tiles[tile].location, self.tiles[tile].variations[variation].location)
    }
}
