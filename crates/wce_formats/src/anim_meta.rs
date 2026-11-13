//! Animation metadata files (`.meta`): what happens during a clip, or plays with a building piece (`<piece file>.meta`), as timed entries.
//! rpfm_lib does not read them. Layout (all the building and unit files of Attila, Rome 2 and the TDD packs parse to the byte and encode
//! back to it; [`Meta`]):
//!
//! - u32 version (2), u32 entry count, then per entry: its kind (u16 length, bytes), u32 entry version, f32 start and end time, for entry
//!   versions >= 2 a tag (string: `ip_fire`, `capture`, `fire_transition`, `fire`, empty), then by kind ([`grammar`]). Some entries, mostly
//!   in old clips, have no version word: start, end, no tag, and the fields of the oldest form.
//! - `EFFECT` (versions 1 to 5) and `BLOOD` (5): the effect (`vfx/<name>.xml`), a string ("true" / "false", not known), the position (3 floats) and
//!   rotation (quaternion x y z w) in the piece's model space, an i32 bone (-1: none), and for version 3 and from 5 an f32 scale (the
//!   engine's reader, fn 0x116a8680; the string is a flag, "true" / "false").
//! - `VOLUMETRIC_EFFECT` (2): the effect, then 20 floats (the engine's reader, fn 0x116a9c00, reads them as 20 dwords), read here as:
//!   a position (3) and a rotation (4) (seen: zero, and the identity or all
//!   zero), the box's centre (3) and size (3) in the piece's model space, the box's rotation (4, the identity in every file seen) and 3
//!   floats (zero in every file seen). ASSUMPTION: centre and size (the walls' boxes span their pieces: 20 x 18 x 12 m around
//!   (0, 6.5, 4.3) for a 20 m wall); the engine was not traced.
//! - `SOUND_BUILDING` (2): 20 bytes (not read).
//! - A clip's entries: `RHAND_POSE`, `LHAND_POSE`, `FACE_POSE` (pose number, weight), `WEAPON_RHAND`, `WEAPON_LHAND`, `WEAPON_HIP`, `WEAPON_ON`
//!   (a number, and from version 3 two floats), `SPLICE` (another clip, its time range and weights, not traced), `PROP`, `ANIMATED_PROP`
//!   (a model, a clip, position, rotation, bone, up to two numbers) and the sounds and positions the game logic reads.

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

/// A value of an entry, in file order.
#[derive(Clone, Debug, PartialEq)]
pub enum MetaField {
    /// Also the signed ones (a bone: -1 for none).
    U32(u32),
    F32(f32),
    Str(String),
}

/// One timed entry of a `.meta`.
#[derive(Clone, Debug, PartialEq)]
pub struct MetaEntry {
    /// `RHAND_POSE`, `WEAPON_ON`, `SPLICE`, `EFFECT`...
    pub kind: String,

    /// `None`: the older form without one, which starts at the start time.
    pub version: Option<u32>,
    pub start: f32,
    pub end: f32,

    /// From version 2.
    pub tag: String,
    pub fields: Vec<MetaField>,
}

impl MetaEntry {
    pub fn u32(&self, i: usize) -> Option<u32> {
        match self.fields.get(i) {
            Some(MetaField::U32(v)) => Some(*v),
            _ => None,
        }
    }

    pub fn f32(&self, i: usize) -> Option<f32> {
        match self.fields.get(i) {
            Some(MetaField::F32(v)) => Some(*v),
            _ => None,
        }
    }

    pub fn str(&self, i: usize) -> Option<&str> {
        match self.fields.get(i) {
            Some(MetaField::Str(v)) => Some(v),
            _ => None,
        }
    }
}

/// A whole `.meta`. [`Meta::encode`] gives back the file's bytes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Meta {
    pub version: u32,
    pub entries: Vec<MetaEntry>,
}

/// An entry's fields by kind and version (0 for the form without one): `u` a u32, `f` an f32, `s` a string. `None`: not a known kind.
fn grammar(kind: &str, version: u32) -> Option<String> {
    Some(match kind {
        "RHAND_POSE" | "LHAND_POSE" | "FACE_POSE" => "uf".to_owned(),
        "WEAPON_RHAND" | "WEAPON_LHAND" | "WEAPON_HIP" | "WEAPON_ON" | "DOCK_EQPT_BACK" | "DOCK_EQPT_LHAND" => {
            if version >= 3 { "uff" } else { "u" }.to_owned()
        }
        "SOUND_TRIGGER" | "SOUND_IMPACT" => "uu".to_owned(),
        "SOUND_ATTACK_TYPE" | "SOUND_DEFEND_TYPE" => if version >= 3 { "uuu" } else { "u" }.to_owned(),
        "IMPACT_POS" | "FIRE_POS" | "TARGET_POS" => "fff".to_owned(),
        "PULL_ROPE" => String::new(),
        "DISTANCE" => "f".to_owned(),
        "DISMEMBER" => "ufffffff".to_owned(),
        "WOUNDED_POSE" => "s".to_owned(),
        "FACEFX" => "sssfff".to_owned(),
        "CAMPAIGN_DISMEMBER" => "us".to_owned(),
        "EFFECT" | "BLOOD" => format!("ssfffffffu{}", if version == 3 || version >= 5 { "f" } else { "" }),
        "PROP" | "ANIMATED_PROP" => format!("ssfffffffu{}{}", if version >= 3 { "u" } else { "" }, if version >= 4 { "u" } else { "" }),
        "SPLICE" => match version {
            0..=2 => "sffffs",
            3 => "ssffffffss",
            _ => "ssffffffsss",
        }
        .to_owned(),
        "VOLUMETRIC_EFFECT" => format!("s{}", "f".repeat(20)),
        "SOUND_BUILDING" => "uuuuu".to_owned(),
        _ => return None,
    })
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
        self.take(n).and_then(|s| String::from_utf8(s.to_vec()).ok())
    }
}

fn put_string(out: &mut Vec<u8>, s: &str) {
    out.extend((s.len() as u16).to_le_bytes());
    out.extend(s.as_bytes());
}

impl Meta {
    /// Decodes a `.meta`; `None` when an entry isn't a known kind or the file doesn't end with the last entry.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let mut r = Reader { b: bytes, p: 0 };
        let version = r.u32()?;
        let count = r.u32()?;
        let mut entries = vec![];
        for _ in 0..count {
            let kind = r.string()?;

            // The word after the name is the entry's version, or already the start time: the two forms are told apart by its value (the
            // bits of any time in the files are far from these, and a start of 0 reads the same either way).
            let word = r.u32()?;
            let version = (1..=5).contains(&word).then_some(word);
            let start = if version.is_some() { r.f32()? } else { f32::from_bits(word) };
            let end = r.f32()?;
            let v = version.unwrap_or(0);
            let tag = if v >= 2 { r.string()? } else { String::new() };
            let fields = grammar(&kind, v)?
                .chars()
                .map(|c| match c {
                    'u' => r.u32().map(MetaField::U32),
                    'f' => r.f32().map(MetaField::F32),
                    _ => r.string().map(MetaField::Str),
                })
                .collect::<Option<Vec<_>>>()?;
            entries.push(MetaEntry { kind, version, start, end, tag, fields });
        }

        (r.p == bytes.len()).then_some(Self { version, entries })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![];
        out.extend(self.version.to_le_bytes());
        out.extend((self.entries.len() as u32).to_le_bytes());
        for e in &self.entries {
            put_string(&mut out, &e.kind);
            if let Some(v) = e.version {
                out.extend(v.to_le_bytes());
            }

            out.extend(e.start.to_bits().to_le_bytes());
            out.extend(e.end.to_bits().to_le_bytes());
            if e.version.is_some_and(|v| v >= 2) {
                put_string(&mut out, &e.tag);
            }

            for f in &e.fields {
                match f {
                    MetaField::U32(v) => out.extend(v.to_le_bytes()),
                    MetaField::F32(v) => out.extend(v.to_bits().to_le_bytes()),
                    MetaField::Str(v) => put_string(&mut out, v),
                }
            }
        }

        out
    }
}

/// The effects of a `.meta`; `None` when it isn't one [`Meta::decode`] reads (nothing of it is used then).
pub fn meta_effects(bytes: &[u8]) -> Option<Vec<MetaEffect>> {
    if bytes.len() < 8 {
        return Some(vec![]);
    }

    let floats = |e: &MetaEntry, from: usize, n: usize| (from..from + n).map(|i| e.f32(i)).collect::<Option<Vec<_>>>();
    Meta::decode(bytes)?
        .entries
        .iter()
        .filter_map(|e| match e.kind.as_str() {
            "EFFECT" => Some((|| {
                let f = floats(e, 2, 7)?;
                Some(MetaEffect {
                    tag: e.tag.clone(),
                    effect: e.str(0)?.to_owned(),
                    start: e.start,
                    end: e.end,
                    position: [f[0], f[1], f[2]],
                    rotation: [f[3], f[4], f[5], f[6]],
                    bone: e.u32(9)? as i32,
                    scale: e.f32(10).unwrap_or(1.0),
                    volume: None,
                })
            })()),
            "VOLUMETRIC_EFFECT" => Some((|| {
                let f = floats(e, 1, 20)?;
                Some(MetaEffect {
                    tag: e.tag.clone(),
                    effect: e.str(0)?.to_owned(),
                    start: e.start,
                    end: e.end,
                    position: [f[0] + f[7], f[1] + f[8], f[2] + f[9]],
                    rotation: [f[13], f[14], f[15], f[16]],
                    bone: -1,
                    scale: 1.0,
                    volume: Some([f[10], f[11], f[12]]),
                })
            })()),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Meta, meta_effects};

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

    /// One entry of each form: with a version and tag, with a version and no tag, and without a version.
    #[test]
    fn entries_encode_back() {
        let mut b = vec![];
        b.extend(2u32.to_le_bytes());
        b.extend(3u32.to_le_bytes());
        let string = |b: &mut Vec<u8>, s: &str| {
            b.extend((s.len() as u16).to_le_bytes());
            b.extend(s.bytes());
        };

        string(&mut b, "RHAND_POSE");
        b.extend(2u32.to_le_bytes());
        b.extend(0.5f32.to_le_bytes());
        b.extend(0.5f32.to_le_bytes());
        string(&mut b, "t");
        b.extend(7u32.to_le_bytes());
        b.extend(1f32.to_le_bytes());
        string(&mut b, "LHAND_POSE");
        b.extend(1u32.to_le_bytes());
        b.extend(0f32.to_le_bytes());
        b.extend(0f32.to_le_bytes());
        b.extend(4u32.to_le_bytes());
        b.extend(1f32.to_le_bytes());
        string(&mut b, "WEAPON_ON");
        b.extend(0.25f32.to_le_bytes());
        b.extend(0.25f32.to_le_bytes());
        b.extend(3u32.to_le_bytes());
        let m = Meta::decode(&b).expect("parses");
        assert_eq!(
            (m.entries[0].version, m.entries[0].tag.as_str(), m.entries[0].u32(0), m.entries[0].f32(1)),
            (Some(2), "t", Some(7), Some(1.0))
        );
        assert_eq!((m.entries[1].version, m.entries[1].u32(0)), (Some(1), Some(4)));
        assert_eq!((m.entries[2].version, m.entries[2].start, m.entries[2].u32(0)), (None, 0.25, Some(3)));
        assert_eq!(m.encode(), b);
        b.push(0);
        assert!(Meta::decode(&b).is_none(), "bytes after the last entry");
    }
}
