//! Exploration: the campaign markers and environment collection made from a `.terry` against BOB's: the first differing records' fields.
//!
//!   cargo run --release -p wce_terrain_io --example campaign_markers_diff -- <.terry> <BOB's output folder>
use wce_formats::markers::MarkersFile;
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let p = wce_terrain::terry::TerryProject::parse(std::fs::read(&a[0]).expect("terry")).expect("xml");
    let out = std::path::PathBuf::from(&a[1]);
    let env = wce_terrain_io::attila::campaign::environment_collection(&p);
    let want = std::fs::read(out.join("environment_collection.xml")).unwrap_or_default();
    for (x, y) in String::from_utf8_lossy(&env).lines().zip(String::from_utf8_lossy(&want).lines()) {
        if x != y {
            println!("env ours  {x}\nenv BOB's {y}");
            break;
        }
    }
    for (name, ours) in [
        ("prop_marker.markers", wce_terrain_io::attila::campaign::prop_markers(&p).expect("props")),
        ("particle_emitter_marker.markers", wce_terrain_io::attila::campaign::particle_markers(&p).expect("vfx")),
    ] {
        let a = MarkersFile::parse(&ours).expect("ours");
        let b = MarkersFile::parse(&std::fs::read(out.join(name)).expect("BOB's")).expect("BOB's");
        let diff: Vec<usize> = (0..a.records.len().min(b.records.len())).filter(|&i| a.records[i] != b.records[i]).collect();
        println!("{name}: {} of {} records differ; groups the same: {}", diff.len(), a.records.len(), a.groups == b.groups);
        for &i in diff.iter().take(2) {
            println!("  #{i} ours  {:?}\n  #{i} BOB's {:?}", a.fields(i), b.fields(i));
        }

        // `WCE_DUMP`: the first differing prop's entity and the one before it.
        if std::env::var("WCE_DUMP").is_ok()
            && name.starts_with("prop")
            && let Some(&i) = diff.first()
        {
            let props: Vec<_> = p.entities.iter().filter(|e| matches!(p.kind(e), wce_terrain::terry::EntityKind::Prop(_))).collect();
            let mut models = std::collections::BTreeMap::new();
            for (k, e) in props.iter().enumerate() {
                let wce_terrain::terry::EntityKind::Prop(m) = p.kind(e) else { continue };
                let c = models.entry(m.rsplit_once('/').map_or(m.clone(), |x| x.0.to_owned())).or_insert([0, 0]);
                c[usize::from(diff.binary_search(&k).is_ok())] += 1;
            }

            println!("folders (same, differing): {models:?}");
            for k in [i - 1, i] {
                for (_, t) in p.entity_text(props[k].id) {
                    println!("--- prop {k}\n{t}");
                }
            }
        }
    }
}
