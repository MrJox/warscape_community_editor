//! Animation metadata files (`.meta`, next to a building piece's mesh: `<piece file>.meta`): what plays with the piece, as timed entries.
//! rpfm_lib does not read them. Layout (all the standing pieces' files of the game and the TDD packs parse to the byte, 1495 files):
//!
//! - u32 version (2), u32 entry count, then per entry: its type name (u16 length, bytes), u32 entry version, f32 start and end time, and
//!   for entry versions >= 2 a tag (string: `ip_fire`, `capture`, `fire_transition`, `fire`, empty), then by type:
//! - `EFFECT` (versions 1, 2, 3, 5): the effect (`vfx/<name>.xml`), a string ("true" / "false", not known), the position (3 floats) and
//!   rotation (quaternion x y z w) in the piece's model space, an i32 bone (-1: none), and for version 3 and from 5 an f32 scale (the
//!   engine's reader, fn 0x116a8680; the string is a flag, "true" / "false").
//! - `VOLUMETRIC_EFFECT` (2): the effect, then 20 floats (the engine's reader, fn 0x116a9c00, reads them as 20 dwords), read here as:
//!   a position (3) and a rotation (4) (seen: zero, and the identity or all
//!   zero), the box's centre (3) and size (3) in the piece's model space, the box's rotation (4, the identity in every file seen) and 3
//!   floats (zero in every file seen). ASSUMPTION: centre and size (the walls' boxes span their pieces: 20 x 18 x 12 m around
//!   (0, 6.5, 4.3) for a 20 m wall); the engine was not traced.
//! - `SOUND_BUILDING` (2): 20 bytes (not read).

/// An effect a piece plays (an `EFFECT`, or a `VOLUMETRIC_EFFECT`: then `volume` is its box's size and `position` / `rotation` its centre
/// and rotation).
#[derive(Clone, Debug, PartialEq)]
pub struct MetaEffect {
    /// Empty: always; otherwise the event it belongs to (`ip_fire` while the building burns, `capture`, `fire_transition`, `fire`).
    pub tag: String,
    pub effect: String,
    pub start: f32,
    pub end: f32,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub bone: i32,
    pub scale: f32,
    pub volume: Option<[f32; 3]>,
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.b.get(self.p..self.p.checked_add(n)?)?;
        self.p += n;
        Some(s)
    }

    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }

    fn f32(&mut self) -> Option<f32> {
        self.u32().map(f32::from_bits)
    }

    fn string(&mut self) -> Option<String> {
        let n = self.take(2).map(|s| u16::from_le_bytes([s[0], s[1]]) as usize)?;
        self.take(n).map(|s| String::from_utf8_lossy(s).into_owned())
    }
}

/// The effects of a `.meta`; `None` when the file has an entry this doesn't know (nothing of it is used then).
pub fn meta_effects(bytes: &[u8]) -> Option<Vec<MetaEffect>> {
    let mut r = Reader { b: bytes, p: 0 };
    if bytes.len() < 8 {
        return Some(vec![]);
    }

    r.u32()?;
    let count = r.u32()?;
    let mut out = vec![];
    for _ in 0..count {
        let kind = r.string()?;
        let version = r.u32()?;
        let (start, end) = (r.f32()?, r.f32()?);
        let tag = if version >= 2 { r.string()? } else { String::new() };
        match kind.as_str() {
            "EFFECT" => {
                let effect = r.string()?;
                r.string()?;
                let position = [r.f32()?, r.f32()?, r.f32()?];
                let rotation = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
                let bone = r.u32()? as i32;
                // The engine's reader (fn 0x116a8680) reads it for versions 3 and from 5 (4 has none).
                let scale = if version == 3 || version >= 5 { r.f32()? } else { 1.0 };
                out.push(MetaEffect { tag, effect, start, end, position, rotation, bone, scale, volume: None });
            }

            "VOLUMETRIC_EFFECT" => {
                let effect = r.string()?;
                let f = (0..20).map(|_| r.f32()).collect::<Option<Vec<_>>>()?;
                let position = [f[0] + f[7], f[1] + f[8], f[2] + f[9]];
                let rotation = [f[13], f[14], f[15], f[16]];
                out.push(MetaEffect { tag, effect, start, end, position, rotation, bone: -1, scale: 1.0, volume: Some([f[10], f[11], f[12]]) });
            }

            "SOUND_BUILDING" => {
                r.take(20)?;
            }

            _ => return None,
        }
    }

    (r.p == bytes.len()).then_some(out)
}

/// A `.meta` as stored, every field kept: what is written back to the byte, and what generated buildings' effects are made as.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MetaFile {
    pub version: u32,
    pub entries: Vec<MetaEntry>,
}

/// An entry: its type name, version, times, tag (from version 2) and contents.
#[derive(Clone, Debug, PartialEq)]
pub struct MetaEntry {
    pub kind: String,
    pub version: u32,
    pub start: f32,
    pub end: f32,
    pub tag: Option<String>,
    pub body: MetaBody,
}

/// An entry's contents by type.
#[derive(Clone, Debug, PartialEq)]
pub enum MetaBody {
    /// `EFFECT`: the effect, the flag string, position, rotation, bone, and the scale of versions 3 and from 5.
    Effect { effect: String, flag: String, position: [f32; 3], rotation: [f32; 4], bone: i32, scale: Option<f32> },

    /// `VOLUMETRIC_EFFECT`: the effect and its 20 floats (the layout in this module's notes).
    Volumetric { effect: String, floats: [f32; 20] },

    /// `SOUND_BUILDING`: 20 bytes, not decoded.
    Sound([u8; 20]),
}

impl MetaFile {
    /// Reads a `.meta`; `None` when an entry's type is unknown or the file doesn't end where its entries do.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let mut r = Reader { b: bytes, p: 0 };
        let version = r.u32()?;
        let count = r.u32()?;
        let mut entries = vec![];
        for _ in 0..count {
            let kind = r.string_exact()?;
            let v = r.u32()?;
            let (start, end) = (r.f32()?, r.f32()?);
            let tag = if v >= 2 { Some(r.string_exact()?) } else { None };
            let body = match kind.as_str() {
                "EFFECT" => {
                    let (effect, flag) = (r.string_exact()?, r.string_exact()?);
                    let position = [r.f32()?, r.f32()?, r.f32()?];
                    let rotation = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
                    let bone = r.u32()? as i32;
                    let scale = if v == 3 || v >= 5 { Some(r.f32()?) } else { None };
                    MetaBody::Effect { effect, flag, position, rotation, bone, scale }
                }

                "VOLUMETRIC_EFFECT" => {
                    let effect = r.string_exact()?;
                    let mut floats = [0.0; 20];
                    for f in &mut floats {
                        *f = r.f32()?;
                    }

                    MetaBody::Volumetric { effect, floats }
                }

                "SOUND_BUILDING" => MetaBody::Sound(r.take(20)?.try_into().ok()?),
                _ => return None,
            };

            entries.push(MetaEntry { kind, version: v, start, end, tag, body });
        }

        (r.p == bytes.len()).then_some(Self { version, entries })
    }

    /// The bytes of the file.
    pub fn encode(&self) -> Vec<u8> {
        let mut b = vec![];
        let text = |b: &mut Vec<u8>, s: &str| {
            b.extend((s.len() as u16).to_le_bytes());
            b.extend(s.bytes());
        };

        b.extend(self.version.to_le_bytes());
        b.extend((self.entries.len() as u32).to_le_bytes());
        for e in &self.entries {
            text(&mut b, &e.kind);
            b.extend(e.version.to_le_bytes());
            b.extend(e.start.to_le_bytes());
            b.extend(e.end.to_le_bytes());
            if let Some(t) = &e.tag {
                text(&mut b, t);
            }

            match &e.body {
                MetaBody::Effect { effect, flag, position, rotation, bone, scale } => {
                    text(&mut b, effect);
                    text(&mut b, flag);
                    position.iter().chain(rotation).for_each(|v| b.extend(v.to_le_bytes()));
                    b.extend(bone.to_le_bytes());
                    if let Some(s) = scale {
                        b.extend(s.to_le_bytes());
                    }
                }

                MetaBody::Volumetric { effect, floats } => {
                    text(&mut b, effect);
                    floats.iter().for_each(|v| b.extend(v.to_le_bytes()));
                }

                MetaBody::Sound(raw) => b.extend(raw),
            }
        }

        b
    }

    /// A volumetric effect over a box (centre, size, in the piece's model space) at `start` seconds, untagged, as the game's collapses
    /// have their dust.
    pub fn volumetric(effect: &str, start: f32, centre: [f32; 3], size: [f32; 3]) -> MetaEntry {
        let mut floats = [0.0; 20];
        floats[6] = 1.0;
        floats[7..10].copy_from_slice(&centre);
        floats[10..13].copy_from_slice(&size);
        floats[16] = 1.0;
        MetaEntry { kind: "VOLUMETRIC_EFFECT".into(), version: 2, start, end: start, tag: Some(String::new()), body: MetaBody::Volumetric { effect: effect.into(), floats } }
    }

    /// A point effect at `start` seconds, untagged, version 5 (with a scale).
    pub fn effect(effect: &str, start: f32, position: [f32; 3], scale: f32) -> MetaEntry {
        MetaEntry {
            kind: "EFFECT".into(),
            version: 5,
            start,
            end: start,
            tag: Some(String::new()),
            body: MetaBody::Effect { effect: effect.into(), flag: "false".into(), position, rotation: [0.0, 0.0, 0.0, 1.0], bone: -1, scale: Some(scale) },
        }
    }
}

impl Reader<'_> {
    /// A string that must be UTF-8 (so that it writes back to the same bytes).
    fn string_exact(&mut self) -> Option<String> {
        let n = self.take(2).map(|s| u16::from_le_bytes([s[0], s[1]]) as usize)?;
        self.take(n).and_then(|s| String::from_utf8(s.to_vec()).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::{MetaFile, meta_effects};

    #[test]
    fn writes_back() {
        let b = (0..TORCH.len()).step_by(2).map(|i| u8::from_str_radix(&TORCH[i..i + 2], 16).unwrap()).collect::<Vec<_>>();
        assert_eq!(MetaFile::parse(&b).expect("parses").encode(), b);

        let made = MetaFile { version: 2, entries: vec![MetaFile::volumetric("destruction_vfx_area_dust_light", 0.01, [0.0, 6.5, 4.3], [20.0, 18.0, 12.0])] };
        let e = meta_effects(&made.encode()).expect("reads");
        assert_eq!((e[0].position, e[0].volume), ([0.0, 6.5, 4.3], Some([20.0, 18.0, 12.0])));
        assert_eq!(MetaFile::parse(&made.encode()), Some(made));
    }

    /// torch_sconce_piece01_destruct01.meta (models2.pack).
    const TORCH: &str = "0200000001000000060045464645435403000000000000000000000000001b00616374696f6e5f7666785f746f7263685f666972655f736d616c6c050066616c7365d4d2383fb2d7054000000000000000000000000058da12be925a7d3fffffffff0000803f";

    #[test]
    fn torch() {
        let b = (0..TORCH.len()).step_by(2).map(|i| u8::from_str_radix(&TORCH[i..i + 2], 16).unwrap()).collect::<Vec<_>>();
        let e = meta_effects(&b).expect("parses");
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].effect, "action_vfx_torch_fire_small");
        assert!(e[0].tag.is_empty());
        assert!((e[0].position[0] - 0.7220).abs() < 1e-3 && (e[0].position[1] - 2.0913).abs() < 1e-3);
        assert_eq!(e[0].bone, -1);
        assert!((e[0].scale - 1.0).abs() < 1e-6);
    }

    /// The dust of roman_fort_straight_piece01_destruct01_anim.meta (its first entry).
    #[test]
    fn volume() {
        let mut b = vec![];
        b.extend(2u32.to_le_bytes());
        b.extend(1u32.to_le_bytes());
        let string = |b: &mut Vec<u8>, s: &str| {
            b.extend((s.len() as u16).to_le_bytes());
            b.extend(s.bytes());
        };

        string(&mut b, "VOLUMETRIC_EFFECT");
        b.extend(2u32.to_le_bytes());
        b.extend(0.011f32.to_le_bytes());
        b.extend(0.011f32.to_le_bytes());
        string(&mut b, "");
        string(&mut b, "destruction_vfx_area_dust_light");
        for v in [0.0f32, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 6.487, 4.32, 20.0, 18.0, 12.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0] {
            b.extend(v.to_le_bytes());
        }

        let e = meta_effects(&b).expect("parses");
        assert_eq!(e.len(), 1);
        assert_eq!((e[0].effect.as_str(), e[0].volume), ("destruction_vfx_area_dust_light", Some([20.0, 18.0, 12.0])));
        assert_eq!((e[0].position, e[0].rotation), ([0.0, 6.487, 4.32], [0.0, 0.0, 0.0, 1.0]));
    }
}
