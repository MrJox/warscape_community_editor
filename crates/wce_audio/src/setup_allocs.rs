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
