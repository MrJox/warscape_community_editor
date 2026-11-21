//! Gate of the game's music by mood (docs/plans/cinematic.md): the moods of the banks read from their `music_set_*` events (the
//! Faction group has Hunnic and Roman), and two of them render music (not silence) for 20 seconds; the battle mode alone too.
//!
//!   music_check <game folder>

use std::path::PathBuf;

use wce_audio::library::SoundLibrary;
use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    let Some(game) = std::env::args().nth(1) else {
        eprintln!("usage: music_check <game folder>");
        std::process::exit(2);
    };
    let config = Config::new(PathBuf::from(game), GameProfile::from_env().clone());
    let vfs = Vfs::mount(&config).expect("the game's packs");
    let lib = SoundLibrary::load(&vfs);
    let mut failed = vec![];
    let moods = wce_cinematic::music::moods(&lib);
    println!("{} moods", moods.len());
    for want in ["music_set_Faction_Hunnic", "music_set_Faction_Roman", "music_set_Intensity_Battle"] {
        if !moods.iter().any(|m| m.event == want) {
            failed.push(format!("no mood {want}"));
        }
    }
    let to_stereo = |b: &[u8]| {
        wce_audio::decode::decode_wem(b).ok().map(|p| {
            let ch = p.channels.max(1) as usize;
            p.samples.chunks(ch).map(|c| [c[0], *c.get(1).unwrap_or(&c[0])]).collect::<Vec<_>>()
        })
    };
    for set in [
        vec!["music_set_Faction_Hunnic", "music_set_Intensity_Battle"],
        vec!["music_set_Faction_Roman", "music_set_Intensity_Marching"],
        vec![],
    ] {
        let set: Vec<String> = set.into_iter().map(str::to_owned).collect();
        let pcm = wce_cinematic::music::render(&lib, &vfs, &set, 20.0, &to_stereo);
        let peak = pcm.as_ref().map_or(0.0, |v| v.iter().map(|s| s[0].abs().max(s[1].abs())).fold(0.0f32, f32::max));
        println!(
            "{set:?}: {} frames, peak {peak:.3}, branch {:?}",
            pcm.as_ref().map_or(0, Vec::len),
            wce_cinematic::music::branch(&lib, &set)
        );
        if peak < 0.01 {
            failed.push(format!("{set:?} renders silence"));
        }
    }
    if failed.is_empty() {
        println!("OK");
    } else {
        for f in &failed {
            println!("FAIL {f}");
        }
        std::process::exit(1);
    }
}
