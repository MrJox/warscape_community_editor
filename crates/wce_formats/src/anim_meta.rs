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

#[cfg(test)]
mod tests {
    use super::meta_effects;

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
