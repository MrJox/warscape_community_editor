//! The `models_naval` DB table (`db/models_naval_tables/*`, Rome II and Attila; rpfm's schema doesn't cover it): per ship model its plain
//! columns and its compiled ship logic (the kit's `art/battle/naval/models/ships/<ship>/logic.xml`): cameras, buoyancy bobbies, decks, rope
//! pipes, damage panels, collision triangles, entity formation lines and custom positions. See `docs/formats/models_naval.md`.
//!
//! Every position is in the ship model's space, as its `.rigid_model_v2` parts are authored: x right (starboard), y up, z forward (the bow).
//! logic.xml writes the same points with y and z swapped (z up, y forward): the table's `[x, y, z]` is the XML's `x, z, y`, no sign flips.
//!
//! The table decodes byte for byte and [`encode`] writes it back identically; fields whose meaning isn't known keep their raw values, and
//! the counts that are always zero in the games (cannons, the trailing list) fail the decode when they aren't.

/// The whole table.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelsNaval {
    /// The header's table GUID (`FD FE FC FF` block), if it has one.
    pub guid: Option<String>,

    /// The table version (`FC FD FE FF` block), if it has one: 11 in Rome II and Attila.
    pub version: Option<u32>,

    /// The byte after the header, before the row count (1 in every DB table of these games).
    pub marker: u8,
    pub rows: Vec<ShipLogic>,
}

/// A row: one ship model.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShipLogic {
    pub key: String,

    /// `VariantMeshes\_VariantModels\ships\<ship>\logic.xml.rigging`.
    pub rigging_file: String,

    /// A destruction animation's rigid model (Rome II's `RigidModels\Naval\Destruction\*.rigid_model_v2`); `Some("")` in Attila's rows,
    /// `None` in most of Rome II's.
    pub destruction_file: Option<String>,

    /// The folder of the ship's rigid models (`VariantMeshes\_VariantModels\ships\<ship>`). The kit's `logic_folder` column has no
    /// field of its own: the logic below is what it was compiled from.
    pub display_folder: String,

    /// `models_oars` keys: upper, middle, lower.
    pub oars: [String; 3],

    /// `capsule` or `ellipse`.
    pub selection_indicator_shape: String,
    pub cameras: Vec<Camera>,
    pub bobbies: Vec<Bobby>,
    pub decks: Vec<Deck>,
    pub pipes: Vec<Pipe>,
    pub damage_parts: Vec<DamagePart>,
    pub collision: Vec<CollisionFace>,
    pub ef_lines: Vec<EfLine>,
    pub custom_positions: Vec<CustomPosition>,
}

/// A ship camera (`navalcam_01`, `navalcam_02`; from the kit's `ship_cameras.cs2`, not logic.xml).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Camera {
    pub name: String,

    /// Row major: rows 0 to 2 the camera's x, y and z axes (w 0), row 3 its position and, in place of w, the vertical field of view in
    /// radians (0.817 in every row; **assumption**).
    pub matrix: [f32; 16],
}

/// A buoyancy sample sphere.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bobby {
    pub id: u32,
    pub mass: f32,
    pub radius: f32,
    pub position: [f32; 3],

    /// The breach sphere (a hole letting water in, **assumption**): radius 0 and offset 0 when the XML has none. The offset is the XML's,
    /// relative to the bobby (**assumption**).
    pub breach_radius: f32,
    pub breach_offset: [f32; 3],

    /// The ids of the bobbies it is joined to.
    pub connections: Vec<u32>,
}

/// A walkable deck: a mesh of convex polygons.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Deck {
    /// `deck01`, `deck_01_main`, ...
    pub name: String,

    /// 1, 2, 3 in file order.
    pub index: u32,
    pub pieces: Vec<DeckPiece>,
}

/// A deck polygon (3 corners, a few with 4).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeckPiece {
    pub id: u32,
    pub corners: Vec<DeckCorner>,

    /// The polygon's plane: unit normal (up) and d, with `normal . p + d = 0` for its corners.
    pub plane: [f32; 4],
}

/// A deck polygon's corner and the edge from it to the next corner.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DeckCorner {
    pub position: [f32; 3],

    /// The edge to the next corner is the deck's border (XML `type="hard"`); otherwise (`clear`) it is shared with another piece.
    pub hard: bool,
}

/// A rope (Rome II only; Attila's ships have none): a polyline, its ends tied to deck pieces.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pipe {
    /// The XML id's number less 100 (`mpipe_rope_131` is 31; **assumption** from the one kit ship with pipes).
    pub id: u32,

    /// 2 for the four-point ropes, 1 for the two-point ones; meaning unknown.
    pub unknown: u32,

    /// The deck pieces the first and last points are tied to (-1: none).
    pub start_deck_piece: i32,
    pub end_deck_piece: i32,

    /// At least two.
    pub points: Vec<[f32; 3]>,
}

/// A damage model part: the panels of one of the ship's `<part>_dm1` / `_dm2` models.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DamagePart {
    /// `bow_01`, `port_02`, `misc_12`, ...
    pub name: String,

    /// 0 in every row; meaning unknown.
    pub unknown: u32,

    /// Polygons (3 or 4 vertices, a few more).
    pub panels: Vec<Vec<[f32; 3]>>,
}

/// A collision triangle.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CollisionFace {
    /// `logic_collision3d` (some ships `logic_collision3D`).
    pub name: String,

    /// From 1.
    pub index: u32,
    pub vertices: [[f32; 3]; 3],
}

/// An entity formation line: where men of a role stand (or a naval weapon is placed), spread from one end to the other.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EfLine {
    /// `EFLine001`, ...
    pub name: String,

    /// The purpose, an index of [`EF_PURPOSES`] (XML `action`).
    pub purpose: u32,
    pub ends: [[f32; 3]; 2],

    /// The way the men face: the XML's direction, normalised.
    pub direction: [f32; 3],
}

impl EfLine {
    /// The purpose's name as logic.xml writes it (`Marines`, `Engine_placement`, ...).
    pub fn purpose_name(&self) -> Option<&'static str> {
        EF_PURPOSES.get(self.purpose as usize).copied()
    }
}

/// The entity formation purposes, by index (the game's enum; the XML names its lines with these).
pub const EF_PURPOSES: [&str; 26] = [
    "Low wall", "High wall", "Window", "Overflow", "Marines", "Seamen", "Gunners_overflow", "Captain", "Officer1", "Boarding",
    "Naval_firing_position_stand", "Naval_firing_position_crouch", "Naval_firing_position_stand_360", "Naval_perimeter_position", "Tree",
    "Entrance Defence", "Officer2", "Officer3", "Crenel left outer", "Crenel left inner", "Crenel right inner", "Crenel right outer",
    "Engine_placement", "Secondary_engine_placement", "Disembark_left", "Disembark_right",
];

/// A named point (oar sockets, the flag).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CustomPosition {
    /// `flag`, `oar_left_l1` ... `oar_right_l3`.
    pub name: String,

    /// The kind the game reads from the name: see [`custom_position_kind`].
    pub kind: u32,

    /// 1 in every row; meaning unknown.
    pub unknown: u32,
    pub position: [f32; 3],

    /// (0, 1, 0) in every row: an up axis (**assumption**).
    pub up: [f32; 3],
}

/// A custom position kind's name: 0 (the flag and any other name), then the oar sockets 6 to 11. 1 to 5 are likely the steamships'
/// (`wheelsplash`, `steamfunnel`, `wheel`, `paddle`, `propellor`; **assumption**, no row has them).
pub fn custom_position_kind(kind: u32) -> Option<&'static str> {
    match kind {
        0 => Some("other"),
        6 => Some("oar_left_l1"),
        7 => Some("oar_left_l2"),
        8 => Some("oar_left_l3"),
        9 => Some("oar_right_l1"),
        10 => Some("oar_right_l2"),
        11 => Some("oar_right_l3"),
        _ => None,
    }
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

type Result<T> = std::result::Result<T, String>;

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let s = self.p.checked_add(n).and_then(|end| self.b.get(self.p..end)).ok_or_else(|| format!("{n} bytes past the end at {:#x}", self.p))?;
        self.p += n;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16> {
        let s = self.take(2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }

    fn u32(&mut self) -> Result<u32> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }

    fn i32(&mut self) -> Result<i32> {
        self.u32().map(|v| v as i32)
    }

    fn f32(&mut self) -> Result<f32> {
        self.u32().map(f32::from_bits)
    }

    fn floats<const N: usize>(&mut self) -> Result<[f32; N]> {
        let mut out = [0.0; N];
        for v in &mut out {
            *v = self.f32()?;
        }
        Ok(out)
    }

    /// A list's count, checked against what is left so a bad count fails instead of allocating.
    fn count(&mut self, min_item: usize) -> Result<usize> {
        let at = self.p;
        let n = self.u32()? as usize;
        if n.saturating_mul(min_item) > self.b.len() - self.p {
            return Err(format!("count {n} at {at:#x} is more than the bytes left"));
        }
        Ok(n)
    }

    fn list<T>(&mut self, min_item: usize, mut f: impl FnMut(&mut Self) -> Result<T>) -> Result<Vec<T>> {
        let n = self.count(min_item)?;
        (0..n).map(|_| f(self)).collect()
    }

    /// A u16 length and UTF-8 bytes.
    fn string(&mut self) -> Result<String> {
        let at = self.p;
        let n = self.u16()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| format!("string at {at:#x} isn't UTF-8"))
    }

    /// A u16 length in characters and UTF-16 code units.
    fn wide(&mut self) -> Result<String> {
        let at = self.p;
        let n = self.u16()? as usize;
        let units: Vec<u16> = self.take(n * 2)?.as_chunks::<2>().0.iter().map(|&c| u16::from_le_bytes(c)).collect();
        String::from_utf16(&units).map_err(|_| format!("UTF-16 string at {at:#x} is malformed"))
    }

    fn flag(&mut self) -> Result<bool> {
        let at = self.p;
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            v => Err(format!("byte {v} at {at:#x} isn't a bool")),
        }
    }

    /// A list count that is zero in every known row.
    fn zero(&mut self, what: &str) -> Result<()> {
        let at = self.p;
        match self.u32()? {
            0 => Ok(()),
            n => Err(format!("{what} at {at:#x} is {n}; its layout is unknown")),
        }
    }
}

const GUID_MARK: [u8; 4] = [0xFD, 0xFE, 0xFC, 0xFF];
const VERSION_MARK: [u8; 4] = [0xFC, 0xFD, 0xFE, 0xFF];

/// Decodes a `models_naval` table file; every byte must be accounted for.
///
/// # Errors
///
/// The bytes end early, a count or flag is out of range, a list that is always empty isn't, or bytes are left over.
pub fn decode(bytes: &[u8]) -> Result<ModelsNaval> {
    let mut r = Reader { b: bytes, p: 0 };
    let mut out = ModelsNaval::default();
    if bytes.starts_with(&GUID_MARK) {
        r.p = 4;
        out.guid = Some(r.wide()?);
    }

    if bytes[r.p..].starts_with(&VERSION_MARK) {
        r.p += 4;
        out.version = Some(r.u32()?);
    }

    out.marker = r.u8()?;
    let rows = r.count(32)?;
    for i in 0..rows {
        let row = ship(&mut r).map_err(|e| format!("row {i}: {e}"))?;
        out.rows.push(row);
    }

    if r.p != bytes.len() {
        return Err(format!("{} bytes left over at {:#x}", bytes.len() - r.p, r.p));
    }

    Ok(out)
}

fn ship(r: &mut Reader) -> Result<ShipLogic> {
    let key = r.string()?;
    let rigging_file = r.string()?;
    let destruction_file = if r.flag()? { Some(r.string()?) } else { None };
    let display_folder = r.string()?;
    let oars = [r.string()?, r.string()?, r.string()?];
    let selection_indicator_shape = r.string()?;
    let cameras = r.list(66, |r| Ok(Camera { name: r.wide()?, matrix: r.floats()? }))?;
    let bobbies = r.list(44, |r| {
        Ok(Bobby {
            id: r.u32()?,
            mass: r.f32()?,
            radius: r.f32()?,
            position: r.floats()?,
            breach_radius: r.f32()?,
            breach_offset: r.floats()?,
            connections: r.list(4, Reader::u32)?,
        })
    })?;

    let decks = r.list(10, |r| {
        Ok(Deck {
            name: r.wide()?,
            index: r.u32()?,
            pieces: r.list(24, |r| {
                let id = r.u32()?;
                let corners = r.list(13, |r| Ok(DeckCorner { position: r.floats()?, hard: r.flag()? }))?;
                Ok(DeckPiece { id, corners, plane: r.floats()? })
            })?,
        })
    })?;

    let pipes = r.list(40, |r| {
        let at = r.p;
        let n = r.u32()? as usize;
        if n < 2 || n > r.b.len() / 12 {
            return Err(format!("pipe at {at:#x} has {n} points"));
        }

        let id = r.u32()?;
        let unknown = r.u32()?;
        let start_deck_piece = r.i32()?;
        let mut points = (0..n - 1).map(|_| r.floats()).collect::<Result<Vec<_>>>()?;
        let end_deck_piece = r.i32()?;
        points.push(r.floats()?);
        Ok(Pipe { id, unknown, start_deck_piece, end_deck_piece, points })
    })?;

    r.zero("cannon count")?;
    let damage_parts = r.list(10, |r| Ok(DamagePart { name: r.wide()?, unknown: r.u32()?, panels: r.list(4, |r| r.list(12, Reader::floats))? }))?;
    let collision = r.list(42, |r| Ok(CollisionFace { name: r.wide()?, index: r.u32()?, vertices: [r.floats()?, r.floats()?, r.floats()?] }))?;
    let ef_lines = r.list(42, |r| Ok(EfLine { name: r.wide()?, purpose: r.u32()?, ends: [r.floats()?, r.floats()?], direction: r.floats()? }))?;
    let custom_positions = r.list(34, |r| Ok(CustomPosition { name: r.wide()?, kind: r.u32()?, unknown: r.u32()?, position: r.floats()?, up: r.floats()? }))?;
    r.zero("trailing count")?;
    Ok(ShipLogic {
        key,
        rigging_file,
        destruction_file,
        display_folder,
        oars,
        selection_indicator_shape,
        cameras,
        bobbies,
        decks,
        pipes,
        damage_parts,
        collision,
        ef_lines,
        custom_positions,
    })
}

struct Writer(Vec<u8>);

impl Writer {
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    fn count(&mut self, n: usize) -> Result<()> {
        self.u32(u32::try_from(n).map_err(|_| format!("{n} items don't fit a count"))?);
        Ok(())
    }

    fn floats(&mut self, v: &[f32]) {
        for f in v {
            self.u32(f.to_bits());
        }
    }

    fn len16(&mut self, n: usize, what: &str) -> Result<()> {
        let n = u16::try_from(n).map_err(|_| format!("string {what:?} is too long"))?;
        self.0.extend_from_slice(&n.to_le_bytes());
        Ok(())
    }

    fn string(&mut self, s: &str) -> Result<()> {
        self.len16(s.len(), s)?;
        self.0.extend_from_slice(s.as_bytes());
        Ok(())
    }

    fn wide(&mut self, s: &str) -> Result<()> {
        let units: Vec<u16> = s.encode_utf16().collect();
        self.len16(units.len(), s)?;
        for u in units {
            self.0.extend_from_slice(&u.to_le_bytes());
        }
        Ok(())
    }
}

/// Encodes a table as [`decode`] reads it: a decoded file comes back byte for byte.
///
/// # Errors
///
/// A string is longer than a u16 length, or a pipe has fewer than two points.
pub fn encode(table: &ModelsNaval) -> Result<Vec<u8>> {
    let mut w = Writer(vec![]);
    if let Some(guid) = &table.guid {
        w.0.extend_from_slice(&GUID_MARK);
        w.wide(guid)?;
    }

    if let Some(version) = table.version {
        w.0.extend_from_slice(&VERSION_MARK);
        w.u32(version);
    }

    w.0.push(table.marker);
    w.count(table.rows.len())?;
    for s in &table.rows {
        w.string(&s.key)?;
        w.string(&s.rigging_file)?;
        w.0.push(u8::from(s.destruction_file.is_some()));
        if let Some(file) = &s.destruction_file {
            w.string(file)?;
        }

        w.string(&s.display_folder)?;
        for oar in &s.oars {
            w.string(oar)?;
        }

        w.string(&s.selection_indicator_shape)?;
        w.count(s.cameras.len())?;
        for c in &s.cameras {
            w.wide(&c.name)?;
            w.floats(&c.matrix);
        }

        w.count(s.bobbies.len())?;
        for b in &s.bobbies {
            w.u32(b.id);
            w.floats(&[b.mass, b.radius]);
            w.floats(&b.position);
            w.floats(&[b.breach_radius]);
            w.floats(&b.breach_offset);
            w.count(b.connections.len())?;
            b.connections.iter().for_each(|&c| w.u32(c));
        }

        w.count(s.decks.len())?;
        for d in &s.decks {
            w.wide(&d.name)?;
            w.u32(d.index);
            w.count(d.pieces.len())?;
            for p in &d.pieces {
                w.u32(p.id);
                w.count(p.corners.len())?;
                for c in &p.corners {
                    w.floats(&c.position);
                    w.0.push(u8::from(c.hard));
                }
                w.floats(&p.plane);
            }
        }

        w.count(s.pipes.len())?;
        for p in &s.pipes {
            let Some((last, rest)) = p.points.split_last().filter(|(_, rest)| !rest.is_empty()) else {
                return Err(format!("pipe {} has fewer than two points", p.id));
            };

            w.count(p.points.len())?;
            w.u32(p.id);
            w.u32(p.unknown);
            w.u32(p.start_deck_piece as u32);
            rest.iter().for_each(|v| w.floats(v));
            w.u32(p.end_deck_piece as u32);
            w.floats(last);
        }

        w.u32(0);
        w.count(s.damage_parts.len())?;
        for part in &s.damage_parts {
            w.wide(&part.name)?;
            w.u32(part.unknown);
            w.count(part.panels.len())?;
            for panel in &part.panels {
                w.count(panel.len())?;
                panel.iter().for_each(|v| w.floats(v));
            }
        }

        w.count(s.collision.len())?;
        for f in &s.collision {
            w.wide(&f.name)?;
            w.u32(f.index);
            f.vertices.iter().for_each(|v| w.floats(v));
        }

        w.count(s.ef_lines.len())?;
        for e in &s.ef_lines {
            w.wide(&e.name)?;
            w.u32(e.purpose);
            e.ends.iter().for_each(|v| w.floats(v));
            w.floats(&e.direction);
        }

        w.count(s.custom_positions.len())?;
        for c in &s.custom_positions {
            w.wide(&c.name)?;
            w.u32(c.kind);
            w.u32(c.unknown);
            w.floats(&c.position);
            w.floats(&c.up);
        }

        w.u32(0);
    }

    Ok(w.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ModelsNaval {
        let ship = ShipLogic {
            key: "att_shp_test".into(),
            rigging_file: r"VariantMeshes\_VariantModels\ships\test\logic.xml.rigging".into(),
            destruction_file: Some(String::new()),
            display_folder: r"VariantMeshes\_VariantModels\ships\test".into(),
            oars: ["four_l3".into(), "four_l2".into(), "four_l1".into()],
            selection_indicator_shape: "capsule".into(),
            cameras: vec![Camera { name: "navalcam_01".into(), matrix: [0.5; 16] }],
            bobbies: vec![Bobby { id: 0, mass: 440.5, radius: 1.0, position: [1.0, 2.0, 3.0], breach_radius: 0.8, breach_offset: [0.0, 0.83, 0.0], connections: vec![1, 2] }],
            decks: vec![Deck {
                name: "deck01".into(),
                index: 1,
                pieces: vec![DeckPiece {
                    id: 1,
                    corners: vec![
                        DeckCorner { position: [0.0, 1.8, 0.0], hard: false },
                        DeckCorner { position: [1.0, 1.8, 0.0], hard: true },
                        DeckCorner { position: [0.0, 1.8, 1.0], hard: false },
                    ],
                    plane: [0.0, 1.0, 0.0, -1.8],
                }],
            }],
            pipes: vec![Pipe { id: 31, unknown: 2, start_deck_piece: -1, end_deck_piece: 12, points: vec![[1.0; 3], [2.0; 3], [3.0; 3]] }],
            damage_parts: vec![DamagePart { name: "bow_01".into(), unknown: 0, panels: vec![vec![[1.0; 3]; 3], vec![[2.0; 3]; 4]] }],
            collision: vec![CollisionFace { name: "logic_collision3D".into(), index: 1, vertices: [[0.0; 3], [1.0; 3], [2.0; 3]] }],
            ef_lines: vec![EfLine { name: "EFLine001".into(), purpose: 23, ends: [[0.0; 3], [1.0; 3]], direction: [-1.0, 0.0, 0.0] }],
            custom_positions: vec![CustomPosition { name: "oar_left_l3".into(), kind: 8, unknown: 1, position: [4.5, 1.3, -7.4], up: [0.0, 1.0, 0.0] }],
        };

        let mut other = ship.clone();
        other.destruction_file = None;
        other.pipes.clear();
        ModelsNaval { guid: Some("3b7807d8-bb14-42c4-91a1-6112beb7abb2".into()), version: Some(11), marker: 1, rows: vec![ship, other] }
    }

    #[test]
    fn round_trip() {
        let table = sample();
        let bytes = encode(&table).unwrap();
        let back = decode(&bytes).unwrap();
        assert_eq!(back, table);
        assert_eq!(encode(&back).unwrap(), bytes);
        assert_eq!(back.rows[0].ef_lines[0].purpose_name(), Some("Secondary_engine_placement"));
    }

    #[test]
    fn header_layout() {
        let bytes = encode(&ModelsNaval { guid: Some("ab".into()), version: Some(11), marker: 1, rows: vec![] }).unwrap();
        assert_eq!(bytes, [0xFD, 0xFE, 0xFC, 0xFF, 2, 0, b'a', 0, b'b', 0, 0xFC, 0xFD, 0xFE, 0xFF, 11, 0, 0, 0, 1, 0, 0, 0, 0]);
    }

    #[test]
    fn rejects_leftovers_and_unknown_lists() {
        let mut bytes = encode(&sample()).unwrap();
        bytes.push(0);
        assert!(decode(&bytes).unwrap_err().contains("left over"));

        // The last row's trailing count, made non-zero.
        bytes.pop();
        let n = bytes.len();
        bytes[n - 4] = 1;
        assert!(decode(&bytes).unwrap_err().contains("trailing count"));
        assert!(decode(&bytes[..n - 2]).is_err());
    }
}
