//! Round-trip gate of the lossless XML layer, as the C++ `xml_roundtrip`.
//!
//!   cargo run --release -p wce_formats --example xml_roundtrip -- <dir|file>...
//!
//! For every .environment / .lighting / .xml file: reading and saving must give the same bytes; editing every attribute must
//! read back as the edits; reverting them must give the same bytes; and the structural edits on a sample of elements must read
//! back as the expected tree and, with their inverse, give the same bytes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use wce_formats::{XmlDoc, XmlError};

fn diff_at(a: &[u8], b: &[u8]) -> String {
    let i = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    format!(" (first difference at byte {i}, sizes {}/{})", a.len(), b.len())
}

fn check(path: &Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let doc = match XmlDoc::parse(bytes.clone()) {
        Ok(doc) => doc,
        Err(XmlError::OddUtf16(_)) => return Err("CORRUPT (UTF-16 with text-mode CRLF, broken in game data too)".to_owned()),
        Err(error) => return Err(error.to_string()),
    };

    if doc.save() != bytes {
        return Err("save differs".to_owned());
    }

    let els = doc.elements();
    let with_x = |e: usize, a: usize| [doc.attr_raw(e, a), b"_x"].concat();

    let mut edited = doc.clone();
    for (e, el) in els.iter().enumerate() {
        for a in 0..el.attrs.len() {
            edited.set_attr_raw(e, a, with_x(e, a));
        }
    }

    let re = XmlDoc::parse(edited.save()).map_err(|error| error.to_string())?;
    if re.elements().len() != els.len() {
        return Err("edit changed element count".to_owned());
    }

    for (e, el) in els.iter().enumerate() {
        if re.elements()[e].name != el.name || re.elements()[e].attrs.len() != el.attrs.len() {
            return Err(format!("edit changed tree at element {e}"));
        }

        for a in 0..el.attrs.len() {
            if re.attr_raw(e, a) != with_x(e, a) {
                return Err(format!("edit value mismatch at {}@{}", doc.path_of(e), el.attrs[a].name));
            }
        }
    }

    let mut app = doc.clone();
    for e in 0..els.len() {
        app.set(e, "we_test", "a&b'c");
    }

    let re2 = XmlDoc::parse(app.save()).map_err(|error| error.to_string())?;
    if (0..els.len()).any(|e| re2.get(e, "we_test").as_deref() != Some("a&b'c")) {
        return Err("append failed".to_owned());
    }

    app.revert_all();
    if app.save() != bytes {
        return Err("revert after append differs".to_owned());
    }

    for (e, el) in els.iter().enumerate() {
        for a in 0..el.attrs.len() {
            edited.set_attr_raw(e, a, doc.attr_raw(e, a).to_vec());
        }
    }

    if edited.modified() || edited.save() != bytes {
        return Err("revert differs".to_owned());
    }

    let n = els.len();
    let stride = (n / 60).max(1);
    for e in (0..n).step_by(stride) {
        let at = doc.path_of(e);
        structural(&doc, &bytes, e, &at).map_err(|error| match error {
            Fail::Gate(message) => message,
            Fail::Threw(error) => format!("structural edit threw at {at}: {error}"),
        })?;
    }

    Ok(())
}

enum Fail {
    Gate(String),
    Threw(XmlError),
}

impl From<XmlError> for Fail {
    fn from(error: XmlError) -> Self {
        Self::Threw(error)
    }
}

fn structural(doc: &XmlDoc, bytes: &[u8], e: usize, at: &str) -> Result<(), Fail> {
    let els = doc.elements();
    let n = els.len();
    let (dup, ni) = doc.duplicated_element(e)?;
    if dup.elements().len() != n + doc.subtree_size(e) || dup.elements()[ni].name != els[e].name {
        return Err(Fail::Gate(format!("duplicate: wrong tree at {at}")));
    }

    let back = dup.removed_element(ni)?.save();
    if back != bytes {
        return Err(Fail::Gate(format!("duplicate + remove differs at {at}{}", diff_at(&back, bytes))));
    }

    if let Some(parent) = els[e].parent
        && els[parent].children.last() != Some(&e)
    {
        match doc.moved_element(e, 1) {
            Ok(moved) => {
                if moved.elements().len() != n {
                    return Err(Fail::Gate(format!("move changed element count at {at}")));
                }

                let back = moved.moved_element(e, 1)?.save();
                if back != bytes {
                    return Err(Fail::Gate(format!("move + move back differs at {at}{}", diff_at(&back, bytes))));
                }
            }

            // Inline elements can't be swapped by whole lines.
            Err(XmlError::Edit(message)) if message.contains("adjacent lines") => {}
            Err(error) => return Err(error.into()),
        }
    }

    match doc.with_child(e, "we_new") {
        Ok((with_child, ci)) => {
            let child = &with_child.elements()[ci];
            if with_child.elements().len() != n + 1 || child.name != "we_new" || child.parent != Some(e) {
                return Err(Fail::Gate(format!("add child: wrong tree at {at}")));
            }

            if !els[e].children.is_empty() {
                let back = with_child.removed_element(ci)?.save();
                if back != bytes {
                    return Err(Fail::Gate(format!("add child + remove differs at {at}{}", diff_at(&back, bytes))));
                }
            }
        }

        // `<property>1</property>`.
        Err(XmlError::Edit(message)) if message.contains("text content") => {}
        Err(error) => return Err(error.into()),
    }

    let with_attr = doc.with_attr_added(e, "we_x", "a&b")?;
    if with_attr.get(e, "we_x").as_deref() != Some("a&b") {
        return Err(Fail::Gate(format!("add attribute failed at {at}")));
    }

    let back = with_attr.without_attr(e, els[e].attrs.len())?.save();
    if back != bytes {
        return Err(Fail::Gate(format!("add attribute + remove differs at {at}{}", diff_at(&back, bytes))));
    }

    if !els[e].attrs.is_empty() {
        let removed = doc.without_attr(e, 0)?;
        if removed.elements()[e].attrs.len() + 1 != els[e].attrs.len() || removed.find_attr(e, &els[e].attrs[0].name).is_some() {
            return Err(Fail::Gate(format!("remove attribute failed at {at}")));
        }
    }

    Ok(())
}

fn collect(path: &Path, files: &mut Vec<PathBuf>) {
    if path.is_file() {
        files.push(path.to_path_buf());
        return;
    }

    let Ok(entries) = std::fs::read_dir(path) else { return };
    let mut entries = entries.flatten().map(|entry| entry.path()).collect::<Vec<_>>();
    entries.sort();
    for entry in entries {
        if entry.is_dir() {
            collect(&entry, files);
        } else if let Some(ext) = entry.extension().and_then(|ext| ext.to_str())
            && ["environment", "lighting", "xml"].contains(&ext.to_ascii_lowercase().as_str())
        {
            files.push(entry);
        }
    }
}

fn main() {
    let mut files = vec![];
    for input in std::env::args().skip(1) {
        collect(Path::new(&input), &mut files);
    }

    let mut by_ext = BTreeMap::<String, (u32, u32)>::new();
    let (mut fails, mut corrupt_files) = (0, 0);
    for file in &files {
        let ext = file.extension().map(|ext| format!(".{}", ext.to_string_lossy())).unwrap_or_default();
        let result = check(file);
        let counts = by_ext.entry(ext).or_default();
        match &result {
            Ok(()) => counts.0 += 1,
            Err(error) if error.starts_with("CORRUPT") => corrupt_files += 1,
            Err(_) => {
                counts.1 += 1;
                fails += 1;
            }
        }

        if let Err(error) = result {
            println!("FAIL {}: {error}", file.display());
        }
    }

    for (ext, (ok, fail)) in &by_ext {
        println!("{ext:<14} ok {ok:>5}  fail {fail}");
    }

    println!("total {} files, {fails} failed, {corrupt_files} corrupt in source data", files.len());
    std::process::exit(if fails > 0 { 1 } else { 0 });
}
