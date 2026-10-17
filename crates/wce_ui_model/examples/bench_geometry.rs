//! Times what the canvas does every frame, per layout (slowest first).
use std::time::Instant;
use wce_ui_model::{DrawOptions, UiModel};

fn main() {
    let game = std::env::args().nth(1).expect("game folder");
    let vfs = wce_vfs::Vfs::mount(&wce_vfs::Config::new(game, wce_vfs::GameProfile::ATTILA)).expect("mount");
    let mut rows = vec![];
    for path in vfs.files().keys() {
        let name = path.rsplit('/').next().unwrap_or(path);
        if !path.starts_with("ui/") || name.contains('.') {
            continue;
        }
        let Ok(data) = vfs.read(path) else { continue };
        if !wce_ui_format::looks_like_ui(&data) {
            continue;
        }
        let t = Instant::now();
        let Ok(m) = UiModel::from_bytes(&data) else { continue };
        let open = t.elapsed();
        let o = DrawOptions::default();
        let t = Instant::now();
        let ents = m.entries();
        let te = t.elapsed();
        let t = Instant::now();
        let _ = m.geometry(&ents, &o);
        let tg = t.elapsed();
        let t = Instant::now();
        let _ = m.draw_list_with(&o);
        let td = t.elapsed();
        rows.push((td + tg + te, open, te, tg, td, ents.len(), path.clone()));
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    for r in rows.iter().take(8) {
        println!("{:>8.1?} frame (entries {:.1?} geometry {:.1?} draw {:.1?}), parse {:.1?}, {} entries  {}", r.0, r.2, r.3, r.4, r.1, r.5, r.6);
    }
}
