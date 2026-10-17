//! Every field of every component, state, picture and mouse action of Attila's layouts gets a name and a control:
//! counts the fields the property tables do not know (they would show with the converter's name) and checks that every
//! check box / drop-down value is one the editor can show, and that editing through the tables round-trips.
//!
//!   cargo run --release -p wce_ui_model --example props_check -- --game <game folder> [--list]

use std::collections::BTreeMap;

use wce_ui_model::props::{Control, PropEdit};
use wce_ui_model::{EntryKind, UiModel};

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut game, mut list) = (None, false);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--game" => game = args.next(),
            "--list" => list = true,
            _ => {}
        }
    }

    let game = game.expect("--game <folder>");
    let vfs = wce_vfs::Vfs::mount(&wce_vfs::Config::new(game, wce_vfs::GameProfile::ATTILA)).expect("mount");
    let mut total = 0usize;
    let mut unknown: BTreeMap<String, usize> = BTreeMap::new();
    let mut odd: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut bad_edit = 0usize;
    for path in vfs.files().keys() {
        let name = path.rsplit('/').next().unwrap_or(path);
        if !(path.starts_with("ui/") && !name.contains('.')) {
            continue;
        }

        let Ok(data) = vfs.read(path) else { continue };
        if !wce_ui_format::looks_like_ui(&data) {
            continue;
        }

        let Ok(m) = UiModel::from_bytes(&data) else { continue };
        if !m.is_layout() {
            continue;
        }

        let mut tried = 0;
        for en in m.entries() {
            if en.kind != EntryKind::Entry {
                continue;
            }

            let mut paths = vec![en.path.clone()];
            for (sp, _) in m.state_paths(&en.path) {
                paths.push(sp.clone());
                paths.extend(m.state_children(&sp, "image_use"));
                paths.extend(m.state_children(&sp, "mouse_state"));
            }

            for p in paths {
                let props = m.properties(&p);
                for pr in &props.items {
                    total += 1;
                    if pr.name.starts_with("Unknown") || pr.label.is_empty() && matches!(pr.control, Control::Info) {
                        if list && pr.name.starts_with("Unknown yes") { println!("{path} v{} {:?} after-label {:?}", m.version, p, pr.label); }
                        *unknown.entry(format!("{} / {} / {:?}", props.kind, pr.name, pr.label)).or_default() += 1;
                    }

                    if let Control::Choice(c) = &pr.control {
                        let v: i64 = pr.value.trim().parse().unwrap_or(i64::MIN);
                        if !c.iter().any(|(n, _)| *n == v) {
                            *odd.entry(format!("{} / {}", props.kind, pr.name)).or_default().entry(pr.value.clone()).or_default() += 1;
                        }
                    }
                }

                // an edit that writes the value it already has must not change the tree
                tried += 1;
                if tried > 40 { continue; }
                if let Some(pr) = props.items.iter().find(|x| matches!(x.control, Control::Check | Control::Int | Control::Text | Control::Float | Control::Choice(_)) && !x.value.is_empty()) {
                    let mut m2 = m.clone();
                    let value = if matches!(pr.control, Control::Check) { pr.value.clone() } else { pr.value.clone() };
                    if m2.edit_prop(&p, pr.idx, PropEdit::Value(value)).is_err() || m2.dom != m.dom {
                        bad_edit += 1;
                        if list {
                            println!("edit changed the tree: {path} {:?} {}", p, pr.name);
                        }
                    }
                }
            }
        }
    }

    println!("{total} fields described");
    println!("{} kinds of field without a game name:", unknown.len());
    for (k, n) in &unknown {
        println!("  {n:6}  {k}");
    }

    println!("values outside their drop-down:");
    for (k, vals) in &odd {
        let v: Vec<String> = vals.iter().map(|(v, n)| format!("{v}x{n}")).collect();
        println!("  {k}: {}", v.join(" "));
    }

    println!("{bad_edit} no-op edits that changed the tree");
    if bad_edit > 0 {
        std::process::exit(1);
    }
}
