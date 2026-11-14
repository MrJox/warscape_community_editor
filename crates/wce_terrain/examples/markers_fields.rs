//! Exploration: a markers file's header and its first records' fields.
//!
//!   cargo run --release -p wce_terrain --example markers_fields -- <file.markers> [records]
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let m = wce_formats::markers::MarkersFile::parse(&std::fs::read(&a[0]).expect("file")).expect("markers");
    println!(
        "{} id {} u32 {} u16 {} class {} id {} v{} records {} groups {:?}",
        m.repository,
        m.repository_id,
        m.repository_u32,
        m.repository_u16,
        m.class_name,
        m.class_id,
        m.version,
        m.records.len(),
        m.groups.as_ref().map(Vec::len)
    );
    let n: usize = a.get(1).and_then(|v| v.parse().ok()).unwrap_or(2);
    for i in 0..n.min(m.records.len()) {
        println!("#{i} {:?}", m.fields(i));
    }
    if let Some(g) = &m.groups {
        for x in g.iter().take(3) {
            println!(
                "group {:?} {:?}",
                x.key
                    .iter()
                    .map(|k| match k {
                        wce_formats::markers::KeyPart::Wide(w) => String::from_utf16_lossy(w),
                        other => format!("{other:?}"),
                    })
                    .collect::<Vec<_>>(),
                &x.indices[..x.indices.len().min(5)]
            );
        }
    }
}
