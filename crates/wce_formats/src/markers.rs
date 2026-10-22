//! `.markers`: TEd's stage-A exports of point-like things (props and decals, particle emitters, point lights, AI hints, ...), written by
//! `EMPIREUTILITY::BASE_MARKER_REPOSITORY::write` (`EmpireUtility.AssemblyKit.dll`). docs/formats/tile_binary.md, "Markers".
//!
//! Layout (little endian; `wstr` = u16 count + UTF-16 code units, `str` = u16 count + bytes):
//! - wstr `BASE_MARKER_REPOSITORY`, the repository's id (16 bytes: 12 zero, u32 0x9D9BB), u32 1, u16 1;
//! - the table's class id (16 bytes: 12 zero, u32), wstr class name (`PROP_MARKER`), the class id again, u32 version, u32 count;
//! - the records, `count` of them, in the class's layout for that version ([`layout`]);
//! - the groups (omitted when the file ends after the records): u16 count, per group u16 key part count, the parts (a wstr, a str or a
//!   u32 by class: the TEd layer, the model, the hint type), u32 index count, the u32 indices of the group's records.
//!
//! A record is kept as its bytes; [`MarkersFile::fields`] reads them as named values. Classes or versions without a layout read only when
//! they hold no record.

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct MarkersError(pub String);

fn fail<T>(what: impl Into<String>) -> Result<T, MarkersError> {
    Err(MarkersError(what.into()))
}

/// One piece of a record layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    /// 16 f32, row-major, the translation in the last row.
    Matrix(&'static str),
    F32(&'static str),
    U32(&'static str),
    U8(&'static str),

    /// u16 count + bytes.
    Str(&'static str),

    /// u32 count + (x, y) f32 pairs.
    Points(&'static str),

    /// u32 count + [`Field::Points`] each.
    PointLists(&'static str),

    /// Bytes whose meaning is not known.
    Bytes(&'static str, usize),

    /// The fields that follow up to the end of the layout are only there when the record's byte at this offset is 1 (the decal block of a
    /// prop).
    IfByte(usize),
}

/// What a group key part is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyKind {
    Wide,
    Narrow,
    U32,
}

use Field::*;

const MATRIX: Field = Matrix("transform");
const DECAL: [Field; 6] = [IfByte(65), F32("parallax_scale"), U8("tiled"), F32("scale_x"), F32("scale_y"), F32("scale_z")];
const PROP_HEAD: [Field; 7] = [MATRIX, U8("unknown_64"), U8("is_decal"), U8("unknown_66"), U8("unknown_67"), U8("unknown_68"), U8("unknown_69")];
const VISIBILITY: [Field; 4] = [U8("visible_inside_snow_zone"), U8("visible_outside_snow_zone"), U8("visible_inside_destruction_zone"), U8("visible_outside_destruction_zone")];

macro_rules! cat {
    ($($part:expr),* $(,)?) => {{
        const N: usize = 0 $(+ $part.len())*;
        const OUT: [Field; N] = {
            let mut out = [Field::U8(""); N];
            let mut i = 0;
            $(
                let part = $part;
                let mut j = 0;
                while j < part.len() {
                    out[i] = part[j];
                    i += 1;
                    j += 1;
                }
            )*
            out
        };
        &OUT
    }};
}

/// The record layout of a class and version, and the kind of its group keys; `None` for what has not been decoded.
pub fn layout(class: &str, version: u32) -> Option<(&'static [Field], KeyKind)> {
    Some(match (class, version) {
        ("PROP_MARKER", 5) => (cat!(PROP_HEAD, DECAL), KeyKind::Wide),
        ("PROP_MARKER", 6) => (cat!(PROP_HEAD, [U32("seasons")], DECAL), KeyKind::Wide),
        ("PROP_MARKER", 8) => (cat!(PROP_HEAD, [U32("seasons"), Bytes("unknown_74", 3)], DECAL), KeyKind::Wide),
        ("PROP_MARKER", 9) => (cat!(PROP_HEAD, [U32("seasons"), U8("unknown_74")], VISIBILITY, DECAL), KeyKind::Wide),
        ("PARTICLE_EMITTER_MARKER", 4) => (cat!([MATRIX, Bytes("unknown_64", 11)]), KeyKind::Wide),
        ("PARTICLE_EMITTER_MARKER", 5) => (cat!([MATRIX, Bytes("unknown_64", 15)]), KeyKind::Wide),
        ("PARTICLE_EMITTER_MARKER", 7) => (cat!([MATRIX, Bytes("unknown_64", 16)]), KeyKind::Wide),
        ("PARTICLE_EMITTER_MARKER", 8) => (cat!([MATRIX, Bytes("unknown_64", 18)]), KeyKind::Wide),
        ("PARTICLE_EMITTER_MARKER", 9) => (
            cat!([MATRIX, Bytes("unknown_64", 6), U8("autoplay"), U8("use_in_outfield")], VISIBILITY, [F32("scale"), U32("seasons")]),
            KeyKind::Wide,
        ),
        ("POINT_LIGHT_MARKER", 2) => (
            cat!([
                F32("x"), F32("y"), F32("z"), F32("radius"), F32("red"), F32("green"), F32("blue"), F32("colour_scale"), Str("animation_type"),
                F32("animation_speed_x"), F32("animation_speed_y"), F32("colour_min"), F32("random_offset"), Str("falloff_type"),
            ]),
            KeyKind::Wide,
        ),
        ("POINT_LIGHT_MARKER", 3) => (
            cat!([
                F32("x"), F32("y"), F32("z"), F32("radius"), F32("red"), F32("green"), F32("blue"), F32("colour_scale"), U8("unknown_32"),
                Str("animation_type"), F32("animation_speed_x"), F32("animation_speed_y"), F32("colour_min"), F32("random_offset"),
                Str("falloff_type"),
            ]),
            KeyKind::Wide,
        ),
        ("AI_HINT_POLYLINE_MARKER", 1) => (&[Points("points")], KeyKind::U32),
        ("AI_HINT_POLYLINE_LIST_MARKER", 1) => (&[PointLists("polylines")], KeyKind::U32),
        ("AI_SEPARATOR_MARKER", 1) => (&[Points("points")], KeyKind::Narrow),
        _ => return None,
    })
}

/// A value of a record field.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Matrix([f32; 16]),
    F32(f32),
    U32(u32),
    U8(u8),
    Str(Vec<u8>),
    Points(Vec<[f32; 2]>),
    PointLists(Vec<Vec<[f32; 2]>>),
    Bytes(Vec<u8>),
}

/// A part of a group key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyPart {
    Wide(Vec<u16>),
    Narrow(Vec<u8>),
    U32(u32),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MarkerGroup {
    pub key: Vec<KeyPart>,
    pub indices: Vec<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MarkersFile {
    pub repository: String,
    pub repository_id: u32,

    /// The u32 and u16 after the repository id (1 and 1 in every file).
    pub repository_u32: u32,
    pub repository_u16: u16,
    pub class_id: u32,
    pub class_name: String,
    pub version: u32,

    /// The records' bytes.
    pub records: Vec<Vec<u8>>,

    /// `None` when the file ends after the records.
    pub groups: Option<Vec<MarkerGroup>>,
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], MarkersError> {
        let r = self.b.get(self.p..self.p + n).ok_or_else(|| MarkersError(format!("truncated at {}", self.p)))?;
        self.p += n;
        Ok(r)
    }

    fn u32(&mut self) -> Result<u32, MarkersError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }

    fn u16(&mut self) -> Result<u16, MarkersError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().expect("2 bytes")))
    }

    fn f32(&mut self) -> Result<f32, MarkersError> {
        Ok(f32::from_bits(self.u32()?))
    }

    fn wide(&mut self) -> Result<Vec<u16>, MarkersError> {
        let n = usize::from(self.u16()?);
        Ok(self.take(2 * n)?.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect())
    }

    fn narrow(&mut self) -> Result<Vec<u8>, MarkersError> {
        let n = usize::from(self.u16()?);
        Ok(self.take(n)?.to_vec())
    }

    /// 16 bytes: 12 zero bytes and a u32.
    fn id(&mut self) -> Result<u32, MarkersError> {
        if self.take(12)?.iter().any(|&b| b != 0) {
            return fail(format!("an id at {} does not start with 12 zero bytes", self.p - 12));
        }

        self.u32()
    }

    fn points(&mut self) -> Result<Vec<[f32; 2]>, MarkersError> {
        let n = self.u32()? as usize;
        if n > self.b.len() / 8 {
            return fail("point count");
        }

        (0..n).map(|_| Ok([self.f32()?, self.f32()?])).collect()
    }

    /// Reads (or, without `out`, skips) one record's fields; `start` is where the record began.
    fn fields(&mut self, layout: &[Field], start: usize, mut out: Option<&mut Vec<(&'static str, Value)>>) -> Result<(), MarkersError> {
        for f in layout {
            let v = match *f {
                IfByte(at) => {
                    if self.b.get(start + at) != Some(&1) {
                        return Ok(());
                    }

                    continue;
                }

                Matrix(n) => {
                    let mut m = [0.0; 16];
                    for x in &mut m {
                        *x = self.f32()?;
                    }

                    (n, Value::Matrix(m))
                }
                F32(n) => (n, Value::F32(self.f32()?)),
                U32(n) => (n, Value::U32(self.u32()?)),
                U8(n) => (n, Value::U8(self.take(1)?[0])),
                Str(n) => (n, Value::Str(self.narrow()?)),
                Points(n) => (n, Value::Points(self.points()?)),
                PointLists(n) => {
                    let count = self.u32()? as usize;
                    if count > self.b.len() / 4 {
                        return fail("polyline count");
                    }

                    (n, Value::PointLists((0..count).map(|_| self.points()).collect::<Result<_, _>>()?))
                }

                Bytes(n, len) => (n, Value::Bytes(self.take(len)?.to_vec())),
            };

            if let Some(out) = out.as_deref_mut() {
                out.push(v);
            }
        }

        Ok(())
    }
}

fn put_wide(o: &mut Vec<u8>, units: &[u16]) {
    o.extend_from_slice(&(units.len() as u16).to_le_bytes());
    for u in units {
        o.extend_from_slice(&u.to_le_bytes());
    }
}

fn put_id(o: &mut Vec<u8>, id: u32) {
    o.extend_from_slice(&[0; 12]);
    o.extend_from_slice(&id.to_le_bytes());
}

impl MarkersFile {

    /// # Errors
    ///
    /// Not a markers file, a truncated one, or records of a class / version without a layout.
    pub fn parse(bytes: &[u8]) -> Result<Self, MarkersError> {
        let mut r = Reader { b: bytes, p: 0 };
        let repository = String::from_utf16_lossy(&r.wide()?);
        if repository != "BASE_MARKER_REPOSITORY" {
            return fail("not a markers file");
        }

        let mut f = Self { repository, ..Default::default() };
        f.repository_id = r.id()?;
        f.repository_u32 = r.u32()?;
        f.repository_u16 = r.u16()?;
        f.class_id = r.id()?;
        f.class_name = String::from_utf16_lossy(&r.wide()?);
        if r.id()? != f.class_id {
            return fail("class id mismatch");
        }

        f.version = r.u32()?;
        let count = r.u32()? as usize;
        let (fields, key) = match layout(&f.class_name, f.version) {
            Some(l) => l,
            None if count == 0 => (&[][..], KeyKind::Wide),
            None => return fail(format!("{} version {} is not decoded", f.class_name, f.version)),
        };

        if count > bytes.len() {
            return fail("record count");
        }

        for _ in 0..count {
            let start = r.p;
            r.fields(fields, start, None)?;
            f.records.push(bytes[start..r.p].to_vec());
        }

        if r.p < bytes.len() {
            let n = r.u16()?;
            let mut groups = Vec::with_capacity(usize::from(n));
            for _ in 0..n {
                let parts = r.u16()?;
                let key = (0..parts)
                    .map(|_| {
                        Ok(match key {
                            KeyKind::Wide => KeyPart::Wide(r.wide()?),
                            KeyKind::Narrow => KeyPart::Narrow(r.narrow()?),
                            KeyKind::U32 => KeyPart::U32(r.u32()?),
                        })
                    })
                    .collect::<Result<Vec<_>, MarkersError>>()?;

                let k = r.u32()? as usize;
                if k > bytes.len() / 4 {
                    return fail("index count");
                }

                groups.push(MarkerGroup { key, indices: (0..k).map(|_| r.u32()).collect::<Result<_, _>>()? });
            }

            f.groups = Some(groups);
        }

        if r.p != bytes.len() {
            return fail(format!("{} bytes after the groups", bytes.len() - r.p));
        }

        Ok(f)
    }

    pub fn write(&self) -> Vec<u8> {
        let mut o = vec![];
        put_wide(&mut o, &self.repository.encode_utf16().collect::<Vec<_>>());
        put_id(&mut o, self.repository_id);
        o.extend_from_slice(&self.repository_u32.to_le_bytes());
        o.extend_from_slice(&self.repository_u16.to_le_bytes());
        put_id(&mut o, self.class_id);
        put_wide(&mut o, &self.class_name.encode_utf16().collect::<Vec<_>>());
        put_id(&mut o, self.class_id);
        o.extend_from_slice(&self.version.to_le_bytes());
        o.extend_from_slice(&(self.records.len() as u32).to_le_bytes());
        for r in &self.records {
            o.extend_from_slice(r);
        }

        if let Some(groups) = &self.groups {
            o.extend_from_slice(&(groups.len() as u16).to_le_bytes());
            for g in groups {
                o.extend_from_slice(&(g.key.len() as u16).to_le_bytes());
                for k in &g.key {
                    match k {
                        KeyPart::Wide(w) => put_wide(&mut o, w),
                        KeyPart::Narrow(s) => {
                            o.extend_from_slice(&(s.len() as u16).to_le_bytes());
                            o.extend_from_slice(s);
                        }

                        KeyPart::U32(v) => o.extend_from_slice(&v.to_le_bytes()),
                    }
                }

                o.extend_from_slice(&(g.indices.len() as u32).to_le_bytes());
                for i in &g.indices {
                    o.extend_from_slice(&i.to_le_bytes());
                }
            }
        }

        o
    }

    /// The fields of record `i`, named by the layout.
    ///
    /// # Errors
    ///
    /// The class / version has no layout (never for a parsed file with records).
    pub fn fields(&self, i: usize) -> Result<Vec<(&'static str, Value)>, MarkersError> {
        let Some((layout, _)) = layout(&self.class_name, self.version) else { return fail("no layout") };
        let mut out = vec![];
        let mut r = Reader { b: &self.records[i], p: 0 };
        r.fields(layout, 0, Some(&mut out))?;
        Ok(out)
    }
}

/// The bytes of a record from its named fields (as [`MarkersFile::fields`] gives them); the fields of an [`Field::IfByte`] block are
/// written when its byte is 1.
///
/// # Errors
///
/// No layout for the class / version, a field missing or of another kind.
pub fn record_from_fields(class: &str, version: u32, fields: &[(&str, Value)]) -> Result<Vec<u8>, MarkersError> {
    let Some((layout, _)) = layout(class, version) else { return fail(format!("{class} version {version} has no layout")) };
    let mut out = vec![];
    let get = |name: &str| fields.iter().find(|(n, _)| *n == name).map(|(_, v)| v).ok_or_else(|| MarkersError(format!("no field {name}")));
    let points = |out: &mut Vec<u8>, p: &[[f32; 2]]| {
        out.extend_from_slice(&(p.len() as u32).to_le_bytes());
        for [x, y] in p {
            out.extend_from_slice(&x.to_le_bytes());
            out.extend_from_slice(&y.to_le_bytes());
        }
    };

    for f in layout {
        let name = match *f {
            IfByte(at) => {
                if out.get(at) != Some(&1) {
                    break;
                }

                continue;
            }

            Matrix(n) | F32(n) | U32(n) | U8(n) | Str(n) | Points(n) | PointLists(n) | Bytes(n, _) => n,
        };

        match (*f, get(name)?) {
            (Matrix(_), Value::Matrix(m)) => m.iter().for_each(|x| out.extend_from_slice(&x.to_le_bytes())),
            (F32(_), Value::F32(v)) => out.extend_from_slice(&v.to_le_bytes()),
            (U32(_), Value::U32(v)) => out.extend_from_slice(&v.to_le_bytes()),
            (U8(_), Value::U8(v)) => out.push(*v),
            (Str(_), Value::Str(s)) => {
                out.extend_from_slice(&(s.len() as u16).to_le_bytes());
                out.extend_from_slice(s);
            }

            (Points(_), Value::Points(p)) => points(&mut out, p),
            (PointLists(_), Value::PointLists(l)) => {
                out.extend_from_slice(&(l.len() as u32).to_le_bytes());
                for p in l {
                    points(&mut out, p);
                }
            }

            (Bytes(_, len), Value::Bytes(b)) if b.len() == len => out.extend_from_slice(b),
            (f, v) => return fail(format!("field {name}: {v:?} for {f:?}")),
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(class: &str, version: u32, records: &[Vec<u8>], groups: Option<Vec<MarkerGroup>>) -> MarkersFile {
        MarkersFile {
            repository: "BASE_MARKER_REPOSITORY".into(), repository_id: 0x9D9BB, repository_u32: 1, repository_u16: 1, class_id: 0x00B7_E42E,
            class_name: class.into(), version, records: records.to_vec(), groups,
        }
    }

    #[test]
    fn point_light_round_trip_and_fields() {
        let mut rec = vec![];
        for v in [10.0f32, 2.0, 30.0, 4.0, 1.0, 0.5, 0.25, 1.5] {
            rec.extend_from_slice(&v.to_le_bytes());
        }

        rec.push(1);
        rec.extend_from_slice(&8u16.to_le_bytes());
        rec.extend_from_slice(b"LAT_NONE");
        rec.extend_from_slice(&[0; 16]);
        rec.extend_from_slice(&12u16.to_le_bytes());
        rec.extend_from_slice(b"WPLFT_SMOOTH");
        let groups = vec![MarkerGroup { key: vec![KeyPart::Wide("Point light".encode_utf16().collect())], indices: vec![0, 1] }];
        let f = file("POINT_LIGHT_MARKER", 3, &[rec.clone(), rec], Some(groups));
        let bytes = f.write();
        let back = MarkersFile::parse(&bytes).unwrap();
        assert_eq!(back, f);
        let fields = back.fields(1).unwrap();
        assert_eq!(record_from_fields("POINT_LIGHT_MARKER", 3, &fields).unwrap(), back.records[1]);
        assert_eq!(fields[3], ("radius", Value::F32(4.0)));
        assert_eq!(fields[9], ("animation_type", Value::Str(b"LAT_NONE".to_vec())));
        assert_eq!(fields.last().unwrap(), &("falloff_type", Value::Str(b"WPLFT_SMOOTH".to_vec())));
    }

    #[test]
    fn prop_decal_block_follows_the_flag() {
        let mut model = vec![0u8; 64];
        model.extend_from_slice(&[0, 0, 0, 0, 0, 1]);
        model.extend_from_slice(&15u32.to_le_bytes());
        model.extend_from_slice(&[0, 1, 1, 1, 1]);
        let mut decal = model.clone();
        decal[65] = 1;
        decal.extend_from_slice(&0.1f32.to_le_bytes());
        decal.push(0);
        for v in [2.0f32, 3.0, -2.0] {
            decal.extend_from_slice(&v.to_le_bytes());
        }

        let f = file("PROP_MARKER", 9, &[model, decal], None);
        let back = MarkersFile::parse(&f.write()).unwrap();
        assert_eq!(back.records.iter().map(Vec::len).collect::<Vec<_>>(), [79, 96]);
        assert_eq!(back.fields(0).unwrap().len(), 13);
        assert_eq!(back.fields(1).unwrap().last().unwrap(), &("scale_z", Value::F32(-2.0)));
        for i in 0..2 {
            assert_eq!(record_from_fields("PROP_MARKER", 9, &back.fields(i).unwrap()).unwrap(), back.records[i]);
        }
    }

    #[test]
    fn records_of_an_unknown_class_are_refused() {
        let f = file("TREE_MARKER", 3, &[vec![0; 8]], None);
        assert!(MarkersFile::parse(&f.write()).is_err());
        assert!(MarkersFile::parse(&file("TREE_MARKER", 3, &[], None).write()).is_ok());
    }
}
