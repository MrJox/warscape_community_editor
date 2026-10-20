//! Building animations (`.anim`, version 5; rpfm_lib reads the header only): the siege engines' working animations and the buildings'
//! collapses between two destruction levels.
//!
//! Layout: u32 version (5), u32 (0), f32 frame rate, string skeleton name (`building` / `generic`), f32 end time, u32 bone count; per bone
//! its name (string) and parent (i32, -1 for a root); per bone the index of its translation track, then per bone the index of its rotation
//! track (i32, -1: none); u32 translation track count, u32 rotation track count, u32 frame count; per frame the translations (3 f32 each)
//! then the rotations (4 i16 each, x y z w over 32767). Checked on battering_ram_light and roman_fort_straight: the data sizes match the files
//! to the byte.

/// A bone's local transform: translation, rotation quaternion (x y z w, normalised).
pub type BoneLocal = ([f32; 3], [f32; 4]);

/// A decoded animation: per frame, per bone its local transform (relative to its parent).
#[derive(Clone, Debug, Default)]
pub struct Animation {
    pub fps: f32,
    pub end: f32,
    pub parents: Vec<i32>,
    pub frames: Vec<Vec<BoneLocal>>,
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

    fn i32(&mut self) -> Option<i32> {
        self.u32().map(|v| v as i32)
    }

    fn f32(&mut self) -> Option<f32> {
        self.u32().map(f32::from_bits)
    }

    fn i16(&mut self) -> Option<i16> {
        self.take(2).map(|s| i16::from_le_bytes([s[0], s[1]]))
    }

    fn string(&mut self) -> Option<()> {
        let n = self.take(2).map(|s| u16::from_le_bytes([s[0], s[1]]) as usize)?;
        self.take(n).map(|_| ())
    }
}

fn normalised(q: [f32; 4]) -> [f32; 4] {
    let len = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if len < 1e-6 { [0.0, 0.0, 0.0, 1.0] } else { q.map(|v| v / len) }
}

impl Animation {
    /// Decodes a `.anim`; `None` when it isn't one this reads. `max_frames` limits how many frames are read (1: the first frame only).
    pub fn decode(bytes: &[u8], max_frames: usize) -> Option<Self> {
        let mut r = Reader { b: bytes, p: 0 };
        if r.u32()? != 5 {
            return None;
        }

        r.u32()?;
        let fps = r.f32()?;
        r.string()?;
        let end = r.f32()?;
        let n = r.u32()? as usize;
        let mut parents = Vec::with_capacity(n.min(4096));
        for _ in 0..n {
            r.string()?;
            parents.push(r.i32()?);
        }

        let tmap = (0..n).map(|_| r.i32()).collect::<Option<Vec<_>>>()?;
        let rmap = (0..n).map(|_| r.i32()).collect::<Option<Vec<_>>>()?;
        let (nt, nr, count) = (r.u32()? as usize, r.u32()? as usize, r.u32()? as usize);
        if count == 0 {
            return None;
        }

        let mut frames = Vec::with_capacity(count.min(max_frames));
        for _ in 0..count.min(max_frames) {
            let ts = (0..nt).map(|_| Some([r.f32()?, r.f32()?, r.f32()?])).collect::<Option<Vec<_>>>()?;
            let qs = (0..nr).map(|_| Some(normalised([r.i16()?, r.i16()?, r.i16()?, r.i16()?].map(|v| f32::from(v) / 32767.0)))).collect::<Option<Vec<_>>>()?;
            frames.push((0..n).map(|i| {
                let t = usize::try_from(tmap[i]).ok().and_then(|k| ts.get(k)).copied().unwrap_or([0.0; 3]);
                let q = usize::try_from(rmap[i]).ok().and_then(|k| qs.get(k)).copied().unwrap_or([0.0, 0.0, 0.0, 1.0]);
                (t, q)
            }).collect());
        }

        Some(Self { fps, end, parents, frames })
    }

    /// How long it plays, in seconds: its end time, else its frames at its rate.
    pub fn duration(&self) -> f32 {
        if self.end > 0.0 {
            self.end
        } else {
            self.frames.len().saturating_sub(1) as f32 / self.fps.max(1e-3)
        }
    }

    /// The bones' local transforms at time `t` (seconds), between the two nearest frames (translations lerped, rotations nlerped along the
    /// shorter arc); held at the last frame after the end.
    pub fn sample(&self, t: f32) -> Vec<BoneLocal> {
        let Some(last) = self.frames.len().checked_sub(1) else { return vec![] };
        let f = (t.max(0.0) * self.fps).min(last as f32);
        let (i, w) = (f.floor() as usize, f.fract());
        let (a, b) = (&self.frames[i], &self.frames[(i + 1).min(last)]);
        a.iter().zip(b).map(|((ta, qa), (tb, qb))| {
            let t = std::array::from_fn(|c| ta[c] + (tb[c] - ta[c]) * w);
            let sign = if qa.iter().zip(qb).map(|(x, y)| x * y).sum::<f32>() < 0.0 { -1.0 } else { 1.0 };
            (t, normalised(std::array::from_fn(|c| qa[c] + (sign * qb[c] - qa[c]) * w)))
        }).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::Animation;

    /// A one-bone animation: two frames, translation (0, 0, 0) -> (2, 4, 0), rotation identity -> 180 degrees about y.
    fn two_frames() -> Vec<u8> {
        let mut b = vec![];
        b.extend(5u32.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        b.extend(10f32.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.push(b'b');
        b.extend(0.1f32.to_le_bytes());
        b.extend(1u32.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.push(b'x');
        b.extend((-1i32).to_le_bytes());
        b.extend(0i32.to_le_bytes());
        b.extend(0i32.to_le_bytes());
        b.extend(1u32.to_le_bytes());
        b.extend(1u32.to_le_bytes());
        b.extend(2u32.to_le_bytes());
        for (t, q) in [([0f32, 0.0, 0.0], [0i16, 0, 0, 32767]), ([2.0, 4.0, 0.0], [0, 32767, 0, 0])] {
            t.iter().for_each(|v| b.extend(v.to_le_bytes()));
            q.iter().for_each(|v| b.extend(v.to_le_bytes()));
        }

        b
    }

    #[test]
    fn samples_between_frames() {
        let a = Animation::decode(&two_frames(), usize::MAX).expect("decodes");
        assert_eq!((a.parents.clone(), a.frames.len()), (vec![-1], 2));
        let (t, q) = a.sample(0.05)[0];
        assert!((t[0] - 1.0).abs() < 1e-5 && (t[1] - 2.0).abs() < 1e-5, "{t:?}");
        let s = std::f32::consts::FRAC_1_SQRT_2;
        assert!((q[1] - s).abs() < 1e-3 && (q[3] - s).abs() < 1e-3, "{q:?}");
        assert_eq!(a.sample(5.0)[0].0, [2.0, 4.0, 0.0], "held at the end");
        assert_eq!(Animation::decode(&two_frames(), 1).map(|a| a.frames.len()), Some(1));
    }
}
