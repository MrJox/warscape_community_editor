//! Wwise's IDs: an event, state, switch, bus or game parameter is known by the 32-bit FNV-1 hash of its lower-case name.

/// The ID Wwise gives a name: FNV-1 (multiply, then xor) over the name's lower-case ASCII bytes.
pub fn id(name: &str) -> u32 {
    let mut hash: u32 = 2_166_136_261;
    for byte in name.bytes() {
        hash = hash.wrapping_mul(16_777_619) ^ u32::from(byte.to_ascii_lowercase());
    }

    hash
}

/// FNV-1 over raw bytes, as is: the Wwise Vorbis header's codebook hash is this over the setup packet.
pub fn bytes(data: &[u8]) -> u32 {
    data.iter().fold(2_166_136_261u32, |hash, &byte| hash.wrapping_mul(16_777_619) ^ u32::from(byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_ids() {
        // IDs that event_data.dat's dialogue events list next to these names (Attila's sound.pack).
        assert_eq!(id("vo_battle_subculture"), 720_515_004);
        assert_eq!(id("vo_battle_remark"), 2_851_864_456);
        assert_eq!(id("battle_vo_about_to_disembark"), 3_427_422_450);
        assert_eq!(id("Barbarian"), 1_787_064_993);
        assert_eq!(id("conversation_03"), 3_206_234_756);
    }
}
