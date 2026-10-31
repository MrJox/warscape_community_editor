//! `CS2`: the raw models 3ds Max's Cas2 exporter writes (`raw_data/art/...`), as far as BOB reads them for the buildings' height-map
//! meshes (ToolDataBuilderDll's CAS2 loader, `FUN_10274120`). Little endian: the magic `01 32 53 43`, then chunks of u32 size (the
//! 8-byte head included), u32 type, the payload. Type 0 is the header (f32 version, two strings), 7 a rigid mesh, 12 the scene; the
//! others are skipped here. Strings are a u16 count of UTF-16 units. Fields come and go with the version (an f32, compared as such).
//!
//! - A **mesh** (type 7): three strings (the name first), a u32; from 1.04 the metadata; from 1.13 an optional property block (u32
//!   present, u32 type 11, u32, f32, an f32 list, a list of six-float boxes); two u32 lists; the vertices (position, normal, an f32
//!   colour of four, a list of vec3 texture coordinates (one per channel), from 1.03 a list of extras, always empty in the files read);
//!   the face groups (a list of u32 triples, then a u32 material); a u32.
//! - The **scene** (type 12): a list of nodes: name, i32 parent (-1 for the root), a position track (vec3), a rotation track
//!   (quaternions), from 1.15 a scale track (vec3), up to 1.05 a 4 x 3 matrix, a string, from 1.04 the metadata.
//! - **Metadata**: string attributes (a key, a list of strings), then integer, float, vec3 and vec4 attributes (a key and a track).
//! - A **track**: u32 present; then u32 type, u32, f32, a list of f32 keys, a list of values of the type (8 integers, 0 floats, 3
//!   vec3, 5 and 6 quaternions, 9 vec4).

use std::fmt;

#[derive(Debug)]
pub struct Cas2Error(pub String);

impl fmt::Display for Cas2Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Cas2Error {}

type Result<T> = std::result::Result<T, Cas2Error>;

/// A rigid mesh: its name, string attributes, vertices (position, normal, colour, the texture coordinate channels) and triangles (the face groups'
/// in order), and each face group's triangle count and material number.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cas2Mesh {
    pub name: String,
    pub strings: Vec<(String, Vec<String>)>,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colours: Vec<[f32; 4]>,
    pub uvs: Vec<Vec<[f32; 3]>>,
    pub faces: Vec<[u32; 3]>,
    pub groups: Vec<(usize, u32)>,
}

/// A scene node: its name, parent, string attributes and its transform's first keys (position, rotation as i j k w, scale).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cas2Node {
    pub name: String,
    pub parent: i32,
    pub strings: Vec<(String, Vec<String>)>,
    pub position: Option<[f32; 3]>,
    pub rotation: Option<[f32; 4]>,
    pub scale: Option<[f32; 3]>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cas2File {
    pub version: f32,
    pub meshes: Vec<Cas2Mesh>,
    pub nodes: Vec<Cas2Node>,
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let r = self.b.get(self.p..self.p + n).ok_or_else(|| Cas2Error(format!("truncated at {}", self.p)))?;
        self.p += n;
        Ok(r)
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }

    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }

    fn floats<const N: usize>(&mut self) -> Result<[f32; N]> {
        let mut out = [0.0; N];
        for v in &mut out {
            *v = self.f32()?;
        }

        Ok(out)
    }

    fn string(&mut self) -> Result<String> {
        let n = u16::from_le_bytes(self.take(2)?.try_into().expect("2 bytes")) as usize;
        let units = self.take(n * 2)?.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect::<Vec<_>>();
        Ok(String::from_utf16_lossy(&units))
    }

    fn list<T>(&mut self, mut f: impl FnMut(&mut Self) -> Result<T>) -> Result<Vec<T>> {
        let n = self.u32()? as usize;
        if n > self.b.len() {
            return Err(Cas2Error(format!("a list of {n} at {}", self.p)));
        }

        (0..n).map(|_| f(self)).collect()
    }

    /// A track: the values of its first key (an empty list gives none).
    fn track(&mut self, types: &[u32]) -> Result<Option<Vec<f32>>> {
        if self.u32()? == 0 {
            return Ok(None);
        }

        let t = self.u32()?;
        if !types.contains(&t) {
            return Err(Cas2Error(format!("track type {t} at {}", self.p)));
        }

        self.u32()?;
        self.f32()?;
        self.list(Self::f32)?;
        let width = match t {
            8 | 0 => 1,
            3 => 3,
            _ => 4,
        };
        let values = self.list(|r| (0..width).map(|_| r.f32()).collect::<Result<Vec<_>>>())?;
        Ok(values.into_iter().next())
    }

    fn metadata(&mut self) -> Result<Vec<(String, Vec<String>)>> {
        let strings = self.list(|r| Ok((r.string()?, r.list(Self::string)?)))?;
        for t in [8, 0, 3, 9] {
            self.list(|r| {
                r.string()?;
                r.track(&[t])
            })?;
        }

        Ok(strings)
    }
}

/// `v >= threshold` as the loader compares them (both f32).
fn from(version: f32, threshold: f32) -> bool {
    version >= threshold
}

impl Cas2File {
    /// # Errors
    ///
    /// Not a CS2, or a mesh or scene chunk that does not read to its end.
    pub fn parse(b: &[u8]) -> Result<Self> {
        if b.get(..4) != Some(&[0x01, 0x32, 0x53, 0x43]) {
            return Err(Cas2Error("not a CS2".into()));
        }

        let mut out = Self::default();
        let mut p = 4;
        while p + 8 <= b.len() {
            let size = u32::from_le_bytes(b[p..p + 4].try_into().expect("4 bytes")) as usize;
            let kind = u32::from_le_bytes(b[p + 4..p + 8].try_into().expect("4 bytes"));
            if size < 8 || p + size > b.len() {
                return Err(Cas2Error(format!("chunk at {p} of size {size}")));
            }

            let mut r = Reader { b: &b[..p + size], p: p + 8 };
            match kind {
                0 => {
                    out.version = r.f32()?;
                    r.p = p + size;
                }
                7 => out.meshes.push(Self::mesh(&mut r, out.version)?),
                12 => out.nodes = Self::scene(&mut r, out.version)?,
                _ => r.p = p + size,
            }

            if r.p != p + size {
                return Err(Cas2Error(format!("chunk {kind} at {p} read to {} of {}", r.p, p + size)));
            }

            p += size;
        }

        Ok(out)
    }

    fn mesh(r: &mut Reader, v: f32) -> Result<Cas2Mesh> {
        let name = r.string()?;
        r.string()?;
        r.string()?;
        r.u32()?;
        let strings = if from(v, 1.04) { r.metadata()? } else { vec![] };
        if from(v, 1.13) && r.u32()? != 0 {
            let t = r.u32()?;
            if t != 11 {
                return Err(Cas2Error(format!("property block type {t}")));
            }

            r.u32()?;
            r.f32()?;
            r.list(Reader::f32)?;
            r.list(Reader::floats::<6>)?;
        }

        r.list(Reader::u32)?;
        r.list(Reader::u32)?;
        let vertices = r.list(|r| {
            let p = r.floats::<3>()?;
            let n = r.floats::<3>()?;
            let c = r.floats::<4>()?;
            let uv = r.list(Reader::floats::<3>)?;
            if from(v, 1.03) && r.u32()? != 0 {
                return Err(Cas2Error("vertex extras".into()));
            }

            Ok((p, n, c, uv))
        })?;
        let groups = r.list(|r| {
            let faces = r.list(|r| Ok([r.u32()?, r.u32()?, r.u32()?]))?;
            let material = r.u32()?;
            Ok((faces, material))
        })?;
        r.u32()?;
        let (positions, normals, colours) = (vertices.iter().map(|v| v.0).collect(), vertices.iter().map(|v| v.1).collect(), vertices.iter().map(|v| v.2).collect());
        let uvs = vertices.into_iter().map(|v| v.3).collect();
        let counts = groups.iter().map(|(f, m)| (f.len(), *m)).collect();
        Ok(Cas2Mesh { name, strings, positions, normals, colours, uvs, faces: groups.into_iter().flat_map(|(f, _)| f).collect(), groups: counts })
    }

    fn scene(r: &mut Reader, v: f32) -> Result<Vec<Cas2Node>> {
        r.list(|r| {
            let name = r.string()?;
            let parent = r.u32()? as i32;
            let position = r.track(&[3])?.map(|p| [p[0], p[1], p[2]]);
            let rotation = r.track(&[5, 6])?.map(|q| [q[0], q[1], q[2], q[3]]);
            let scale = if from(v, 1.15) { r.track(&[3])?.map(|s| [s[0], s[1], s[2]]) } else { None };
            if v <= 1.05 {
                r.floats::<16>()?;
            }

            r.string()?;
            let strings = if from(v, 1.04) { r.metadata()? } else { vec![] };
            Ok(Cas2Node { name, parent, strings, position, rotation, scale })
        })
    }
}
