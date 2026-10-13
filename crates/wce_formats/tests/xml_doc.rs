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
