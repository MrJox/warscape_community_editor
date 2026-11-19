//! Campaign map ambience (`audio/<campaign map>.amb`, Attila and Rome 2): the map's ambience emitters, each a sound event along a
//! polyline (a coast, a river, a forest's edge). A u32 count, then per emitter: the event's ID (FNV-1 of names such as
//! `campaign_spot_ambience_coastal_waves_cliff`), a u32 point count and the points (x, y, z f32; y is 0 in every file seen). The
//! `.xml` of the same name holds the listener scaling and the climate blend distances (plain XML, read elsewhere).
//! docs/formats/sound_data.md.

use crate::io::{Put, Reader};
use crate::{FormatError, Result};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ambience {
    pub emitters: Vec<Emitter>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Emitter {
    /// The sound event it plays.
    pub event_id: u32,
    pub points: Vec<[f32; 3]>,
}

impl Ambience {
    /// Reads the file.
    ///
    /// # Errors
    ///
    /// Fails when an emitter runs past the end or bytes are left after the last one.
    pub fn read(bytes: &[u8]) -> Result<Self> {
        let mut r = Reader::new(bytes);
        let n = r.count(8)?;
        let emitters = (0..n)
            .map(|_| {
                let event_id = r.u32()?;
                let count = r.count(12)?;
                let points = (0..count).map(|_| Ok([r.f32()?, r.f32()?, r.f32()?])).collect::<Result<_>>()?;
                Ok(Emitter { event_id, points })
            })
            .collect::<Result<_>>()?;
        if r.left() != 0 {
            return Err(FormatError::Invalid(format!("{} bytes left after the last emitter", r.left())));
        }

        Ok(Self { emitters })
    }

    /// Writes the file. What [`Ambience::read`] read comes back byte for byte.
    pub fn write(&self) -> Vec<u8> {
        let mut out = vec![];
        out.put_u32(self.emitters.len() as u32);
        for e in &self.emitters {
            out.put_u32(e.event_id);
            out.put_u32(e.points.len() as u32);
            for p in &e.points {
                p.iter().for_each(|v| out.put_f32(*v));
            }
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let amb = Ambience { emitters: vec![Emitter { event_id: 0xb8de_034d, points: vec![[54.5, 0.0, 449.8], [51.4, 0.0, 441.0]] }] };
        let bytes = amb.write();
        assert_eq!(bytes.len(), 4 + 8 + 24);
        assert_eq!(Ambience::read(&bytes).unwrap(), amb);
        assert!(Ambience::read(&bytes[..bytes.len() - 4]).is_err());
    }
}
