//! Gate of WEM decoding: decodes every WEM of a game and the given mod packs (the copy the game loads) and checks that each gives
//! its header's frame count; prints the totals, the slowest files and the failures. Exits non-zero on a failure.
//!
//!   audio_decode_check <game folder> [mod pack...]

use std::path::PathBuf;
use std::time::Instant;

use wce_audio::decode::decode_wem;
use wce_audio_format::bank::Bank;
use wce_audio_format::wem::Wem;
use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(game) = args.next() else {
        eprintln!("audio_decode_check <game folder> [mod pack...]");
        std::process::exit(2);
    };

    let mut config = Config::new(PathBuf::from(game), GameProfile::from_env().clone());
    config.use_mods = true;
    config.mod_packs = Some(args.collect());
    let vfs = Vfs::mount(&config).expect("the game's packs");

    let started = Instant::now();
    let (mut ok, mut failed, mut seconds, mut slowest) = (0usize, vec![], 0.0f64, vec![]);
    for path in vfs.list("", ".wem") {
        let bytes = vfs.read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        if bytes.is_empty() {
            continue;
        }

        let t = Instant::now();
        let expected = Wem::read(&bytes).ok().and_then(|w| w.vorbis()).map(|h| h.total_frames as usize);
        match decode_wem(&bytes) {
            Ok(pcm) if Some(pcm.frames()) == expected => {
                ok += 1;
                seconds += pcm.seconds();
                slowest.push((t.elapsed().as_secs_f64(), path.to_owned(), pcm.seconds()));
            }
            Ok(pcm) => failed.push(format!("{path}: {} frames, the header says {expected:?}", pcm.frames())),
            Err(e) => failed.push(format!("{path}: {e}")),
        }
    }

    // Media held in banks (DIDX + DATA). A prefetched sound keeps only its start in the bank (the whole file is the loose
    // audio/<id>.wem); an effect plug-in's media is not RIFF.
    let (mut in_banks, mut prefetch, mut not_riff) = (0usize, 0usize, 0usize);
    for path in vfs.list("", ".bnk") {
        let bytes = vfs.read(path).unwrap_or_default();
        let Ok(bank) = Bank::read(&bytes) else { continue };
        for entry in bank.media_index() {
            let Some(media) = bank.media(entry.id) else {
                failed.push(format!("{path}: media {} is outside DATA", entry.id));
                continue;
            };
            if !media.starts_with(b"RIFF") {
                not_riff += 1;
                continue;
            }
            let riff_size = u32::from_le_bytes([media[4], media[5], media[6], media[7]]) as usize;
            if riff_size + 8 > media.len() && vfs.owner(&format!("audio/{}.wem", entry.id)).is_some() {
                prefetch += 1;
                continue;
            }
            let expected = Wem::read(media).ok().and_then(|w| w.vorbis()).map(|h| h.total_frames as usize);
            match decode_wem(media) {
                Ok(pcm) if Some(pcm.frames()) == expected => {
                    in_banks += 1;
                    seconds += pcm.seconds();
                }
                Ok(pcm) => failed.push(format!("{path}: media {}: {} frames, the header says {expected:?}", entry.id, pcm.frames())),
                Err(e) => failed.push(format!("{path}: media {}: {e}", entry.id)),
            }
        }
    }
    println!("{in_banks} media inside banks decoded; {prefetch} prefetched starts of loose files, {not_riff} not RIFF (plug-in media)");

    slowest.sort_by(|a, b| b.0.total_cmp(&a.0));
    println!("{ok} decoded ({:.1} hours of sound) in {:.0} s, {} failed", seconds / 3600.0, started.elapsed().as_secs_f64(), failed.len());
    for (t, path, s) in slowest.iter().take(3) {
        println!("  slowest: {path}: {s:.1} s of sound in {:.0} ms", t * 1000.0);
    }
    for f in failed.iter().take(20) {
        println!("  {f}");
    }

    std::process::exit(if failed.is_empty() { 0 } else { 1 });
}
