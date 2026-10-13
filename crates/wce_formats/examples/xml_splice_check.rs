//! Gate of the in-place structural edits, as the C++ `xml_splice_check`: they give the same text and the same element table as the
//! functional edits that read the document again, and their records undo and redo exactly. Also checks that reading a text in pieces
//! gives the table of reading it in one go.
//!
//!   cargo run --release -p wce_formats --example xml_splice_check -- <file.xml> [rounds]

use wce_formats::{XmlDoc, XmlSplice};

fn same_table(a: &XmlDoc, b: &XmlDoc) -> Result<(), String> {
    if a.elements().len() != b.elements().len() {
        return Err(format!("element count {} vs {}", a.elements().len(), b.elements().len()));
    }

    if a.roots() != b.roots() {
        return Err("roots".to_owned());
    }

    match a.elements().iter().zip(b.elements()).position(|(x, y)| x != y) {
        Some(i) => Err(format!("element {i} <{}>", a.elements()[i].name)),
        None => Ok(()),
    }
}

/// A small deterministic generator (xorshift64*).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> usize {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) as usize
    }
}

fn run(bytes: &[u8], rounds: usize, failed: &mut u32) -> Result<(), wce_formats::XmlError> {
    let mut check = |ok: bool, what: &str| {
        if !ok {
            println!("FAIL: {what}");
            *failed += 1;
        }
    };

    let one = XmlDoc::parse_with(bytes.to_vec(), usize::MAX)?;
    let many = XmlDoc::parse_with(bytes.to_vec(), 1)?;
    let why = same_table(&one, &many).err().unwrap_or_default();
    check(why.is_empty(), &format!("parallel parse: {why}"));
    println!("parallel parse: {} elements", many.elements().len());

    let mut fast = XmlDoc::parse(bytes.to_vec())?;
    let mut slow = fast.clone();
    let original = fast.clone();
    let mut rng = Rng(12345);
    let mut records: Vec<Vec<XmlSplice>> = vec![];
    for round in 0..rounds {
        let Some(e) = (0..50).map(|_| rng.next() % fast.elements().len()).find(|&e| fast.elements()[e].parent.is_some()) else { break };
        let remove = rng.next().is_multiple_of(3);
        fast.bake_pending()?;
        let mark = fast.journal_mark();
        if remove {
            fast.remove_element_inplace(e)?;
            slow = slow.removed_element(e)?;
        } else {
            let copy_fast = fast.duplicate_element_inplace(e)?;
            let (doc, copy_slow) = slow.duplicated_element(e)?;
            slow = doc;
            check(copy_fast == copy_slow, "the copy gets the same index");
        }

        records.push(fast.journal_since(mark));
        fast.journal_forget(mark);

        // A value edit after the structural one, inside the same record.
        if rng.next().is_multiple_of(3) {
            let t = rng.next() % fast.elements().len();
            if let Some(name) = fast.elements()[t].attrs.first().map(|at| at.name.clone()) {
                let mark = fast.journal_mark();
                fast.set(t, &name, &format!("later{round}"));
                slow.set(t, &name, &format!("later{round}"));
                fast.bake_pending()?;
                records.last_mut().into_iter().for_each(|record| record.extend(fast.journal_since(mark)));
                fast.journal_forget(mark);
                slow = XmlDoc::parse(slow.save())?;
            }
        }

        if fast.original_text() != slow.original_text() {
            check(false, "same text as the functional edit");
            println!("round {round} ({} element {e})", if remove { "remove" } else { "duplicate" });
            break;
        }

        if let Err(why) = same_table(&fast, &slow) {
            check(false, &why);
            println!("round {round}: table differs: {why}");
            break;
        }
    }

    println!("{} structural edits applied", records.len());
    for record in records.iter().rev() {
        for splice in record.iter().rev() {
            fast.unapply_splice(splice)?;
        }
    }

    check(fast.original_text() == original.original_text(), "undoing every record gives the original text");
    let why = same_table(&fast, &original).err().unwrap_or_default();
    check(why.is_empty(), &format!("undone table: {why}"));
    check(fast.structural_edits() == 0, "no structural edits remain after undoing them all");
    for splice in records.iter().flatten() {
        fast.apply_splice(splice)?;
    }

    check(fast.original_text() == slow.original_text(), "redoing every record gives the edited text again");
    let why = same_table(&fast, &slow).err().unwrap_or_default();
    check(why.is_empty(), &format!("redone table: {why}"));
    Ok(())
}

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let Some(path) = args.get(1) else {
        println!("usage: xml_splice_check <file.xml> [rounds]");
        std::process::exit(2);
    };

    let rounds = args.get(2).and_then(|rounds| rounds.parse().ok()).unwrap_or(40);
    let bytes = std::fs::read(path).unwrap_or_else(|error| panic!("cannot read {path}: {error}"));
    let mut failed = 0;
    if let Err(error) = run(&bytes, rounds, &mut failed) {
        println!("threw: {error}");
        failed += 1;
    }

    println!("{}", if failed > 0 { "FAILED" } else { "ok" });
    std::process::exit(if failed > 0 { 1 } else { 0 });
}
