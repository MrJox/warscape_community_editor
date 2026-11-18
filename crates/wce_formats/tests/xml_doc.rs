//! Tests of the lossless XML layer on small documents. The corpus gates are the examples `xml_roundtrip` and `xml_splice_check`.

use wce_formats::xml_doc::{decode_entities, encode_entities};
use wce_formats::{TextEncoding, XmlDoc, XmlError, decode_text, encode_text};

const SAMPLE: &str = "<?xml version='1.0'?>\r\n<root a='1' b=\"x &amp; y\">\r\n\t<!-- note -->\r\n\t<child k='v'/>\r\n\t<child k='w'>\r\n\t\t<leaf/>\r\n\t</child>\r\n\t<text>1</text>\r\n</root>\r\n";

fn doc(text: &str) -> XmlDoc {
    XmlDoc::parse(text.as_bytes().to_vec()).unwrap()
}

fn text(doc: &XmlDoc) -> String {
    String::from_utf8(doc.save()).unwrap()
}

#[test]
fn encodings_round_trip() {
    let utf16 = [&[0xFF, 0xFE][..], &"<a b='é😀'/>".encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<_>>()].concat();
    let (decoded, encoding) = decode_text(&utf16).unwrap();
    assert_eq!(encoding, TextEncoding::Utf16LeBom);
    assert_eq!(decoded, "<a b='é😀'/>".as_bytes());
    assert_eq!(encode_text(&decoded, encoding), utf16);

    // A lone surrogate survives as WTF-8.
    let lone = [0xFF, 0xFE, 0x00, 0xD8, b'a', 0];
    let (decoded, encoding) = decode_text(&lone).unwrap();
    assert_eq!(encode_text(&decoded, encoding), lone);

    let bom = b"\xEF\xBB\xBF<a/>";
    let (decoded, encoding) = decode_text(bom).unwrap();
    assert_eq!((decoded.as_slice(), encoding), (&b"<a/>"[..], TextEncoding::Utf8Bom));
    assert_eq!(encode_text(&decoded, encoding), bom);

    // Not UTF-8: kept as it is.
    let latin1 = b"<a b='\xE9'/>";
    let doc = XmlDoc::parse(latin1.to_vec()).unwrap();
    assert_eq!(doc.save(), latin1);
    assert_eq!(doc.attr_raw(0, 0), b"\xE9");

    assert!(matches!(XmlDoc::parse(vec![0xFF, 0xFE, b'<']), Err(XmlError::OddUtf16(_))));
}

#[test]
fn parse_reads_the_tree_and_saves_unchanged() {
    let d = doc(SAMPLE);
    assert_eq!(d.save(), SAMPLE.as_bytes());
    assert_eq!(d.roots(), &[0]);
    let names = d.elements().iter().map(|el| el.name.as_str()).collect::<Vec<_>>();
    assert_eq!(names, ["root", "child", "child", "leaf", "text"]);
    assert_eq!(d.elements()[0].children, [1, 2, 4]);
    assert_eq!(d.elements()[3].parent, Some(2));
    assert!(d.elements()[1].self_closing);
    assert_eq!(d.path_of(3), "root/child/leaf");
    assert_eq!(d.subtree_size(0), 5);
    assert_eq!(d.get(0, "b").as_deref(), Some("x & y"));
    assert_eq!(d.elements()[0].attrs[1].quote, b'"');

    // Unbalanced end tags close everything opened after their element.
    let d = doc("<a><b><c></a>");
    assert_eq!(d.elements().iter().map(|el| el.end_off).collect::<Vec<_>>(), [13, 13, 13]);

    let error = XmlDoc::parse(b"<a></b>".to_vec()).unwrap_err().to_string();
    assert_eq!(error, "xml: end tag </b> without start at offset 3");
    assert!(XmlDoc::parse(b"<a>".to_vec()).unwrap_err().to_string().contains("unclosed <a>"));
}

#[test]
fn parallel_parse_matches_single() {
    let mut big = String::from("<root>\n");
    for i in 0..5000 {
        big.push_str(&format!("  <item id='{i}'>\n    <v x=\"{i}\"/>\n    <!-- c -->\n  </item>\n"));
    }

    big.push_str("</root>\n<second/>\n");
    let one = XmlDoc::parse_with(big.clone().into_bytes(), usize::MAX).unwrap();
    let many = XmlDoc::parse_with(big.into_bytes(), 1).unwrap();
    assert_eq!(one.elements(), many.elements());
    assert_eq!(one.roots(), many.roots());
}

#[test]
fn attribute_edits_and_revert() {
    let mut d = doc(SAMPLE);
    d.set(1, "k", "a'b<");
    assert_eq!(d.attr_raw(1, 0), b"a&apos;b&lt;");
    assert!(d.attr_modified(1, 0));
    d.set(1, "new", "n");
    d.set(4, "new", "m");
    assert_eq!(text(&d), SAMPLE.replace("<child k='v'/>", "<child k='a&apos;b&lt;' new='n'/>").replace("<text>", "<text new='m'>"));
    assert_eq!(d.get(1, "new").as_deref(), Some("n"));

    d.set(1, "k", "v");
    assert!(!d.attr_modified(1, 0));
    d.revert_all();
    assert!(!d.modified());
    assert_eq!(d.save(), SAMPLE.as_bytes());

    assert_eq!(decode_entities(b"&amp;&lt;&gt;&quot;&apos;&x;&"), b"&<>\"'&x;&");
    assert_eq!(encode_entities(b"a&<'\"", b'"'), b"a&amp;&lt;'&quot;");
}

#[test]
fn structural_edits() {
    let d = doc(SAMPLE);
    let removed = d.removed_element(1).unwrap();
    assert_eq!(text(&removed), SAMPLE.replace("\t<child k='v'/>\r\n", ""));
    assert_eq!(removed.structural_edits(), 1);
    assert_eq!(removed.origin_bytes(), SAMPLE.as_bytes());

    let (dup, copy) = d.duplicated_element(2).unwrap();
    assert_eq!(copy, 4);
    assert_eq!(dup.elements()[copy].name, "child");
    assert_eq!(dup.removed_element(copy).unwrap().save(), SAMPLE.as_bytes());

    let moved = d.moved_element(1, 1).unwrap();
    assert_eq!(moved.elements()[1].attrs[0].value_len, 1);
    assert_eq!(moved.get(1, "k").as_deref(), Some("w"));
    assert_eq!(moved.moved_element(1, 1).unwrap().save(), SAMPLE.as_bytes());
    assert!(matches!(d.moved_element(1, -1), Err(XmlError::Edit(_))));

    let (with_child, child) = d.with_child(1, "new").unwrap();
    assert_eq!(text(&with_child), SAMPLE.replace("<child k='v'/>", "<child k='v'>\r\n\t\t<new/>\r\n\t</child>"));
    assert_eq!(with_child.elements()[child].parent, Some(1));
    let (with_child, child) = d.with_child(2, "new").unwrap();
    assert_eq!(text(&with_child), SAMPLE.replace("\t\t<leaf/>\r\n", "\t\t<leaf/>\r\n\t\t<new/>\r\n"));
    assert_eq!(with_child.removed_element(child).unwrap().save(), SAMPLE.as_bytes());
    assert!(d.with_child(4, "new").unwrap_err().to_string().contains("text content"));
    assert!(d.with_child(0, "1bad").is_err());

    let with_attr = d.with_attr_added(3, "x", "a&b").unwrap();
    assert_eq!(with_attr.get(3, "x").as_deref(), Some("a&b"));
    assert_eq!(with_attr.without_attr(3, 0).unwrap().save(), SAMPLE.as_bytes());
    assert_eq!(text(&d.without_attr(0, 0).unwrap()), SAMPLE.replace(" a='1'", ""));
}

#[test]
fn inplace_edits_match_functional_and_undo() {
    let mut fast = doc(SAMPLE);
    let original = fast.clone();
    let mark = fast.journal_mark();
    let copy = fast.duplicate_element_inplace(2).unwrap();
    let (slow, slow_copy) = original.duplicated_element(2).unwrap();
    assert_eq!(copy, slow_copy);
    assert_eq!(fast.original_text(), slow.original_text());
    assert_eq!(fast.elements(), slow.elements());

    fast.set(copy, "k", "changed");
    fast.set(copy, "extra", "e");
    fast.bake_pending().unwrap();
    fast.remove_element_inplace(1).unwrap();
    let slow = XmlDoc::parse(slow.save()).unwrap();
    let mut slow_edit = slow.clone();
    slow_edit.set(copy, "k", "changed");
    slow_edit.set(copy, "extra", "e");
    let slow = XmlDoc::parse(slow_edit.save()).unwrap().removed_element(1).unwrap();
    assert_eq!(fast.original_text(), slow.original_text());
    assert_eq!(fast.elements(), slow.elements());
    assert_eq!(fast.roots(), slow.roots());

    let record = fast.journal_since(mark);
    fast.journal_forget(mark);
    for splice in record.iter().rev() {
        fast.unapply_splice(splice).unwrap();
    }

    assert_eq!(fast.original_text(), original.original_text());
    assert_eq!(fast.elements(), original.elements());
    assert_eq!(fast.structural_edits(), 0);
    for splice in &record {
        fast.apply_splice(splice).unwrap();
    }

    assert_eq!(fast.original_text(), slow.original_text());
    assert_eq!(fast.elements(), slow.elements());
    assert!(fast.splice(1000, 0, b"").is_err());
}

#[test]
fn many_attribute_edits_bake_as_one_splice_that_undoes_and_redoes() {
    let mut text = String::from("<root>\n");
    for i in 0..200 {
        text += &format!("  <item a='{i}' b=\"x{i}\"/>\n");
    }
    text += "</root>\n";
    let mut doc = XmlDoc::parse(text.clone().into_bytes()).unwrap();
    let mark = doc.journal_mark();
    let items = doc.elements()[0].children.clone();
    for (k, &e) in items.iter().enumerate() {
        doc.set(e, "a", &format!("{}", k * 10));
        if k % 3 == 0 {
            doc.set(e, "c", "new");
        }
    }

    let edited = doc.save();
    doc.bake_pending().unwrap();
    assert_eq!(doc.save(), edited, "the same text baked");
    let record = doc.journal_since(mark);
    assert_eq!(record.len(), 1, "one splice");
    assert_eq!(doc.get(items[3], "c").as_deref(), Some("new"), "the added attributes read back");
    assert_eq!(doc.get(items[199], "a").as_deref(), Some("1990"));

    doc.unapply_splice(&record[0]).unwrap();
    assert_eq!(doc.save(), text.as_bytes(), "undo gives the text back");
    assert_eq!(doc.get(items[5], "a").as_deref(), Some("5"));
    doc.apply_splice(&record[0]).unwrap();
    assert_eq!(doc.save(), edited, "redo");
}

#[test]
fn an_added_attribute_is_taken_away_and_the_others_added_after_it_stay() {
    let text = b"<a x='1'>\n  <b y=\"2\"/>\n</a>\n".to_vec();
    let mut doc = XmlDoc::parse(text.clone()).unwrap();
    doc.set(0, "p", "added first");
    doc.set(0, "q", "added second");
    assert!(!doc.remove_added(0, "x"), "the file's own is not taken away");
    assert!(doc.remove_added(0, "p"));
    assert_eq!((doc.get(0, "p"), doc.get(0, "q").as_deref()), (None, Some("added second")));
    assert!(doc.remove_added(0, "q"));
    assert_eq!(doc.save(), text, "the text as it was");
}

#[test]
fn a_text_read_over_its_file_is_modified_and_saves_as_read() {
    let copy = SAMPLE.replace("k='v'", "k='copy'");
    let d = XmlDoc::parse_over(copy.clone().into_bytes(), SAMPLE.as_bytes()).unwrap();
    assert!(d.modified(), "an autosave copy over its file is an edit of it");
    assert_eq!(text(&d), copy);
    assert_eq!(d.origin_bytes(), SAMPLE.as_bytes(), "going back to the file reads the file");
}

#[test]
fn many_removals_across_containers_are_one_splice_that_undoes_and_redoes() {
    let mut text = String::from("<?xml version='1.0'?>\r\n<root>\r\n");
    for l in 0..4 {
        text += &format!("  <layer n='{l}'>\r\n    <children>\r\n");
        for i in 0..30 {
            if i % 7 == 3 {
                text += &format!("      <group n='{l}.{i}'>\r\n        <tree k='{i}'/>\r\n      </group>\r\n");
            } else {
                text += &format!("      <tree k='{l}.{i}' a=\"x\"/>\r\n");
            }
        }
        text += "    </children>\r\n  </layer>\r\n";
    }
    text += "</root>\r\n";
    let original = doc(&text);

    // Every third element below the layers that is no `children`, groups among them (with what they hold), none inside another.
    let els = original.elements();
    let picked = (0..els.len())
        .filter(|&e| matches!(els[e].name.as_str(), "tree" | "group"))
        .filter(|&e| els[e].parent.is_some_and(|p| els[p].name == "children"))
        .step_by(3)
        .collect::<Vec<_>>();
    assert!(picked.len() > 20);

    let mut one_by_one = original.clone();
    for &e in picked.iter().rev() {
        one_by_one.remove_element_inplace(e).unwrap();
    }

    let mut fast = original.clone();
    let mark = fast.journal_mark();
    fast.remove_elements_inplace(&picked).unwrap();
    let record = fast.journal_since(mark);
    fast.journal_forget(mark);
    assert_eq!(record.len(), 1, "one splice");
    assert_eq!(fast.original_text(), one_by_one.original_text());
    let reread = XmlDoc::parse(fast.save()).unwrap();
    assert_eq!(fast.elements(), reread.elements(), "the table patched as the text reads");
    assert_eq!(fast.roots(), reread.roots());

    fast.unapply_splice(&record[0]).unwrap();
    assert_eq!(fast.original_text(), original.original_text());
    assert_eq!(fast.elements(), original.elements(), "undo patches the table back");
    fast.apply_splice(&record[0]).unwrap();
    assert_eq!(fast.elements(), reread.elements(), "redo");
}

#[test]
fn inner_text_edits_change_only_the_text() {
    let d = doc("<v>\r\n\t<p name='a' min='0'/>\r\n\t<p name='b' min='1'>k,1;0,0</p>\r\n</v>\r\n");
    assert_eq!(d.inner_text(1).unwrap(), b"");
    assert_eq!(d.inner_text(2).unwrap(), b"k,1;0,0");
    assert!(d.inner_text(0).is_err());

    // A self-closing element gets its end tag; an attribute edit pending before stays in the text.
    let mut d2 = d.clone();
    d2.set(1, "min", "2");
    let e = d2.with_inner_text(1, b"c,1;0,1").unwrap();
    assert_eq!(text(&e), "<v>\r\n\t<p name='a' min='2'>c,1;0,1</p>\r\n\t<p name='b' min='1'>k,1;0,0</p>\r\n</v>\r\n");
    assert_eq!(e.inner_text(1).unwrap(), b"c,1;0,1");

    let e = d.with_inner_text(2, b"k,1;0,0.5").unwrap();
    assert_eq!(text(&e), "<v>\r\n\t<p name='a' min='0'/>\r\n\t<p name='b' min='1'>k,1;0,0.5</p>\r\n</v>\r\n");
    assert!(e.structural_edits() > 0);

    // The same text again is no edit.
    assert_eq!(text(&d.with_inner_text(2, b"k,1;0,0").unwrap()), text(&d));
}

#[test]
fn a_child_added_as_text_takes_its_siblings_line_layout() {
    let d = doc("<v>\r\n\t<e>\r\n\t\t<p name='a'/>\r\n\t</e>\r\n</v>\r\n");
    let (e, i) = d.with_child_text(1, b"<p name='b' value='1.00'/>").unwrap();
    assert_eq!(text(&e), "<v>\r\n\t<e>\r\n\t\t<p name='a'/>\r\n\t\t<p name='b' value='1.00'/>\r\n\t</e>\r\n</v>\r\n");
    assert_eq!(e.get(i, "name").as_deref(), Some("b"));
    assert!(d.with_child_text(1, b"<p/><q/>").is_err());
    assert!(d.with_child_text(1, b"text").is_err());
}
