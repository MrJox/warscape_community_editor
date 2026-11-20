//! Decoder memory sizes of the Wwise Vorbis setups, by codebook hash (docs/formats/wem.md): every setup of Attila's WEMs and every
//! setup Wwise 2014.1.3 makes for qualities 0 to 10 with 1, 2, 4, 5 and 6 channels (one setup per quality and channel layout, at
//! any sample rate). Measured with the dev-only Wwise oracle (scratch tools/setup_grid.py, 2026-11-19); the runtime reads them from
//! the WEM header, so an encoded WEM must carry its setup's.

/// (codebook hash, decoder alloc size, x64 decoder alloc size).
pub const SETUP_ALLOCS: &[(u32, u32, u32)] = &[
    (103219657, 18240, 18736),
    (127720195, 16872, 17296),
    (233383710, 19956, 20436),
    (471264238, 20128, 20608),
    (550434184, 17692, 18116),
    (578293492, 17892, 18292),
    (873610789, 18240, 18736),
    (899852801, 15492, 15892),
    (997642109, 17892, 18292),
    (1069932774, 19488, 19984),
    (1191182305, 18436, 18916),
    (1266951288, 16596, 17076),
    (1288147548, 16872, 17296),
    (1495390512, 20904, 21400),
    (1539623572, 21480, 21976),
    (1743332505, 9200, 9436),
    (2028618466, 13788, 14032),
    (2134899758, 14540, 14988),
    (2498220993, 14540, 14988),
    (2786080167, 19776, 20272),
    (2840645231, 13300, 13716),
    (2847957762, 16192, 16616),
    (2848244989, 16872, 17296),
    (2914718126, 14540, 14988),
    (3017712712, 18180, 18636),
    (3071128302, 14540, 14988),
    (3145595791, 14316, 14764),
    (3268482500, 17892, 18292),
    (3366068561, 16452, 16848),
    (3378998832, 16872, 17296),
    (3551530976, 14540, 14988),
    (3578505448, 16080, 16560),
    (3726503625, 15116, 15532),
    (3787059486, 13076, 13492),
    (4235668595, 16192, 16616),
];

/// The decoder alloc sizes of a setup, when known.
pub fn allocs(codebook_hash: u32) -> Option<(u32, u32)> {
    SETUP_ALLOCS.iter().find(|(h, _, _)| *h == codebook_hash).map(|(_, a, b)| (*a, *b))
}

/// What the Vorbis setups are (the Wwise oracle grid, scratch `grid/report.txt`, 2026-11-19: Wwise 2014 at qualities 0 to 10, 1 to 6
/// channels, 8 rates; and aoTuV at each rate itself, as the game's older files were encoded): (codebook hash, the lowest and highest
/// Wwise quality made with it, stereo (coupled), the lowest and highest sample rate it is made for: 0 for Wwise 2014's own, the same
/// at any rate, made as for 44.1 kHz).
pub const SETUP_KINDS: &[(u32, u8, u8, bool, u32, u32)] = &[
    (0x3b76cf7d, 0, 0, false, 0, 0),
    (0xde1deec9, 0, 0, true, 0, 0),
    (0x35a2aa01, 1, 1, false, 0, 0),
    (0xa950ce6f, 1, 1, true, 0, 0),
    (0xfc772c73, 2, 2, false, 0, 0),
    (0xbb7dfb8f, 2, 2, true, 0, 0),
    (0xa9c4c4fd, 3, 3, false, 0, 0),
    (0x7f3ffc2e, 3, 3, true, 0, 0),
    (0xb3dea448, 4, 4, false, 0, 0),
    (0xd54ba8e8, 4, 4, true, 0, 0),
    (0x0de9271e, 5, 5, false, 0, 0),
    (0x4b842478, 5, 5, true, 0, 0),
    (0x1c16ebee, 6, 6, false, 0, 0),
    (0x062701c9, 6, 6, true, 0, 0),
    (0x46ffffe1, 7, 7, false, 0, 0),
    (0x34123e25, 7, 7, true, 0, 0),
    (0x5921d930, 8, 8, false, 0, 0),
    (0x3fc5e0e6, 8, 8, true, 0, 0),
    (0x5bc4ca94, 9, 10, false, 0, 0),
    (0xa61035a7, 9, 10, true, 0, 0),
    (0x78ea42e2, 0, 10, false, 11025, 11025),
    (0x67e92499, 0, 10, true, 11025, 11025),
    (0xc8a22151, 0, 0, false, 16000, 16000),
    (0x64f2dc80, 0, 0, true, 16000, 16000),
    (0xa06d09cb, 0, 0, false, 22050, 22050),
    (0xd0ab3020, 0, 0, true, 22050, 22050),
    (0x38012f7f, 0, 0, false, 24000, 24000),
    (0x0b1334c0, 0, 0, true, 24000, 24000),
    (0x2b2f4fa4, 0, 0, false, 28000, 28000),
    (0x5a43db0a, 0, 0, true, 28000, 28000),
    (0x22780ef4, 0, 0, false, 32000, 32000),
    (0x6aaa8a59, 0, 0, true, 32000, 32000),
    (0x9839088d, 0, 0, false, 48000, 48000),
    (0xc9922278, 0, 0, true, 48000, 48000),
    (0x20cef588, 1, 4, false, 16000, 24000),
    (0xe1b9f11e, 1, 4, true, 16000, 24000),
    (0x72680778, 1, 1, false, 28000, 28000),
    (0xbe49c81b, 1, 1, true, 28000, 28000),
    (0x847c9b48, 1, 1, false, 32000, 32000),
    (0xf79b8360, 1, 1, true, 32000, 32000),
    (0xcdd8c229, 1, 1, false, 44100, 44100),
    (0xf7cf38e9, 1, 1, false, 48000, 48000),
    (0x6376b9c0, 1, 1, true, 48000, 48000),
    (0xde336d6c, 2, 2, false, 28000, 28000),
    (0xc1fdb1ca, 2, 2, true, 28000, 28000),
    (0xccdf0543, 2, 2, false, 32000, 32000),
    (0x9166e947, 2, 2, true, 32000, 32000),
    (0x87334b8f, 2, 2, false, 48000, 48000),
    (0xc51f52d2, 2, 2, true, 48000, 48000),
    (0xc9676e30, 3, 3, false, 28000, 28000),
    (0xb70db2ee, 3, 3, true, 28000, 28000),
    (0x4cc7925c, 3, 3, false, 32000, 32000),
    (0x94e7d3c1, 3, 3, true, 32000, 32000),
    (0x4293dc30, 3, 3, false, 48000, 48000),
    (0x80ab7560, 3, 3, true, 48000, 48000),
    (0xc28e8c38, 4, 4, false, 28000, 32000),
    (0xc279396b, 4, 4, true, 28000, 32000),
    (0x081e7c2f, 4, 4, false, 48000, 48000),
    (0xec69cb18, 4, 4, true, 48000, 48000),
    (0xc574e4b8, 5, 10, false, 16000, 24000),
    (0x7fa9dd1f, 5, 10, true, 16000, 24000),
    (0x3b862393, 5, 5, false, 28000, 32000),
    (0x5d778271, 5, 5, true, 28000, 32000),
    (0x2f9cbc1b, 5, 5, false, 48000, 48000),
    (0xb6c0a30e, 5, 5, true, 48000, 48000),
];

/// A setup's quality (range), stereo or not, and the rates it was made for (`None`: Wwise 2014's own, any rate).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetupKind {
    pub qualities: (u8, u8),
    pub stereo: bool,
    pub rates: Option<(u32, u32)>,
}

pub fn setup_kind(codebook_hash: u32) -> Option<SetupKind> {
    SETUP_KINDS.iter().find(|k| k.0 == codebook_hash).map(|&(_, lo, hi, stereo, r0, r1)| SetupKind {
        qualities: (lo, hi),
        stereo,
        rates: (r0 != 0).then_some((r0, r1)),
    })
}

#[cfg(test)]
mod kind_tests {
    use super::*;

    #[test]
    fn known_setups() {
        // Wwise's Voice preset (quality 4, mono) and a game file's setup tuned for 32 kHz at quality 3.
        assert_eq!(setup_kind(3_017_712_712), Some(SetupKind { qualities: (4, 4), stereo: false, rates: None }));
        assert_eq!(setup_kind(0x4cc7_925c), Some(SetupKind { qualities: (3, 3), stereo: false, rates: Some((32_000, 32_000)) }));
    }
}
