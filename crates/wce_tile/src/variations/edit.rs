//! Edits of a combined scene's variations (docs/plans/tile_variations.md, "Working in the editor"): which variations a layer, group
//! or folder belongs to, variation folders made and taken away, *Split for this variation*, and Tidy. Text splices on the document, as
//! the other structural edits of [`TedScene`]: element indices change, and a caller that wants an undo record brackets them with
//! `begin_edit` / `end_edit`.

use super::{FOLDER, TAGGABLE, VALUES, VARIATIONS_ATTR, attr_span, effective, list_text, own_variations, to_variation};
use crate::ted_scene::{TedScene, indent_of, line_end, line_start, reindent};

type Result<T> = std::result::Result<T, String>;

/// The indentation a level adds (TEd's, and what `TedScene` adds).
const STEP: &[u8] = b"    ";

fn bake(sc: &mut TedScene) -> Result<()> {
    sc.doc_mut().bake_pending().map_err(|e| e.to_string())
}

fn splice(sc: &mut TedScene, at: usize, len: usize, text: &[u8]) -> Result<()> {
    sc.doc_mut().splice(at, len, text).map(|_| ()).map_err(|e| e.to_string())
}

/// Whether an element can carry variations: a layer, a group, a variation folder.
#[must_use]
pub fn can_carry(sc: &TedScene, elem: usize) -> bool {
    let name = sc.type_of(elem);
    name == FOLDER || TAGGABLE.contains(&name)
}

/// Sets the variations a layer, group or variation folder belongs to; `None`: as its parent (the attribute taken away).
///
/// # Errors
///
/// The element cannot carry variations, or the attribute can't be taken away.
pub fn set_variations(sc: &mut TedScene, elem: usize, list: Option<&[String]>) -> Result<()> {
    if !can_carry(sc, elem) {
        return Err(format!("a {} cannot belong to some variations only (its layer or folder does)", sc.type_of(elem)));
    }

    match list {
        Some(l) => sc.doc_mut().set(elem, VARIATIONS_ATTR, &list_text(l)),
        None => {
            if sc.doc().find_attr(elem, VARIATIONS_ATTR).is_none() || sc.doc_mut().remove_added(elem, VARIATIONS_ATTR) {
                return Ok(());
            }

            bake(sc)?;
            let (a, b) = attr_span(sc.doc().original_text(), sc.doc(), elem, VARIATIONS_ATTR).ok_or("the attribute is not in the file")?;
            splice(sc, a, b - a, b"")?;
        }
    }

    Ok(())
}

/// The elements, sorted, when they are side by side in one layer, group or folder (the same parent).
fn siblings(sc: &TedScene, elems: &[usize]) -> Result<Vec<usize>> {
    let mut elems = elems.to_vec();
    elems.sort_unstable();
    elems.dedup();
    let els = sc.doc().elements();
    let first = *elems.first().ok_or("nothing selected")?;
    let parent = els[first].parent.filter(|&p| els[p].name == "children" || els[p].name == FOLDER).ok_or("not in a layer or a folder")?;
    if elems.iter().any(|&e| els[e].parent != Some(parent)) {
        return Err("the elements are not side by side in one layer, group or folder".to_owned());
    }

    if elems.iter().any(|&e| els[e].name == VALUES || els[e].name == super::START) {
        return Err("bookkeeping elements cannot be moved".to_owned());
    }

    Ok(elems)
}

/// Puts sibling elements (with their values per variation) into a new variation folder at the first one's place; returns the folder.
///
/// # Errors
///
/// The elements are not siblings in a layer, group or folder.
pub fn wrap_in_folder(sc: &mut TedScene, elems: &[usize], list: &[String]) -> Result<usize> {
    let elems = siblings(sc, elems)?;
    bake(sc)?;
    let doc = sc.doc();
    let (t, els) = (doc.original_text(), doc.elements());
    let nl = sc.newline();
    let lead = sc.lead(elems[0]);
    let ind = indent_of(t, els[elems[0]].start_off);
    let inner = [ind.as_slice(), STEP].concat();
    let mut text = [ind.as_slice(), b"<", FOLDER.as_bytes(), b" ", VARIATIONS_ATTR.as_bytes(), b"=\""].concat();
    text.extend_from_slice(list_text(list).as_bytes());
    text.extend_from_slice(b"\">");
    text.extend_from_slice(nl);
    let mut cuts = vec![];
    for &e in &elems {
        let (a, b) = (line_start(t, els[sc.lead(e)].start_off), line_end(t, els[e].end_off - 1));
        reindent(&t[a..b], &ind, &inner, &mut text);
        cuts.push((a, b));
    }

    text.extend_from_slice(&ind);
    text.extend_from_slice(b"</");
    text.extend_from_slice(FOLDER.as_bytes());
    text.extend_from_slice(b">");
    text.extend_from_slice(nl);

    // The cuts from the last, then the folder where the first was.
    for &(a, b) in cuts.iter().rev() {
        splice(sc, a, b - a, b"")?;
    }

    splice(sc, cuts[0].0, 0, &text)?;
    Ok(lead)
}

/// The text between a container's start and end tag lines (empty for a self-closing one).
fn contents(sc: &TedScene, elem: usize) -> &[u8] {
    let doc = sc.doc();
    let (t, el) = (doc.original_text(), &doc.elements()[elem]);
    match super::close_tag(doc, t, elem) {
        Some(close) => &t[line_end(t, el.start_end - 1).min(close)..line_start(t, close)],
        None => &[],
    }
}

/// Takes a variation folder away, its contents in its place (one level less).
///
/// # Errors
///
/// Not a variation folder.
pub fn unwrap_folder(sc: &mut TedScene, folder: usize) -> Result<()> {
    if sc.type_of(folder) != FOLDER {
        return Err(format!("{} is not a variation folder", sc.type_of(folder)));
    }

    bake(sc)?;
    let doc = sc.doc();
    let (t, el) = (doc.original_text(), &doc.elements()[folder]);
    let (a, b) = (line_start(t, el.start_off), line_end(t, el.end_off - 1));
    let ind = indent_of(t, el.start_off);
    let inner = el.children.first().map_or_else(|| [ind.as_slice(), STEP].concat(), |&c| indent_of(t, doc.elements()[c].start_off));
    let mut text = vec![];
    reindent(contents(sc, folder), &inner, &ind, &mut text);
    splice(sc, a, b - a, &text)
}

/// An element's lines as `variation` has them: its own values put in, what is not its taken out, the folders and groups it goes
/// through taken away ([`to_variation`] of them). At the element's indentation.
fn variation_text(sc: &TedScene, elem: usize, variation: &str) -> Result<Vec<u8>> {
    let doc = sc.doc();
    let (t, els) = (doc.original_text(), doc.elements());
    let nl = sc.newline();
    let (a, b) = (line_start(t, els[sc.lead(elem)].start_off), line_end(t, els[elem].end_off - 1));
    let head = format!("<project {VARIATIONS_ATTR}=\"{variation}\">");
    let text = [head.as_bytes(), nl, &t[a..b], b"</project>", nl].concat();
    let out = to_variation(&text, variation)?;
    let body = out.splitn(2, |&c| c == b'\n').nth(1).unwrap_or_default();
    let end = body.windows(10).rposition(|w| w == b"</project>").unwrap_or(body.len());
    Ok(body[..end].to_vec())
}

/// *Split for this variation*: sibling elements are copied, as `variation` has them, into a folder for it, and the originals go into a
/// folder for their parent's other variations, in one place. Returns the copies.
///
/// # Errors
///
/// Not siblings, not in the variation, or no other variation has them.
pub fn split_for(sc: &mut TedScene, elems: &[usize], variation: &str, all: &[String]) -> Result<Vec<usize>> {
    let elems = siblings(sc, elems)?;
    bake(sc)?;
    let parent = sc.doc().elements()[elems[0]].parent.ok_or("not in a layer")?;
    let set = effective(sc.doc(), parent, all);
    if !set.iter().any(|v| v == variation) {
        return Err(format!("the selection is not in variation {variation}"));
    }

    let others = set.iter().filter(|v| *v != variation).cloned().collect::<Vec<_>>();
    if others.is_empty() {
        return Err(format!("only {variation} has the selection already"));
    }

    let old_ind = indent_of(sc.doc().original_text(), sc.doc().elements()[elems[0]].start_off);
    let copies = elems.iter().map(|&e| variation_text(sc, e, variation)).collect::<Result<Vec<_>>>()?.concat();
    let f = wrap_in_folder(sc, &elems, &others)?;
    bake(sc)?;
    let nl = sc.newline();
    let t = sc.doc().original_text();
    let start = sc.doc().elements()[f].start_off;
    let ind = indent_of(t, start);
    let mut text =
        [ind.as_slice(), b"<", FOLDER.as_bytes(), b" ", VARIATIONS_ATTR.as_bytes(), b"=\"", variation.as_bytes(), b"\">", nl].concat();
    reindent(&copies, &old_ind, &[ind.as_slice(), STEP].concat(), &mut text);
    text.extend_from_slice(&[ind.as_slice(), b"</", FOLDER.as_bytes(), b">", nl].concat());
    let at = line_start(t, start);
    splice(sc, at, 0, &text)?;
    let els = sc.doc().elements();
    Ok(els[f].children.iter().copied().filter(|&c| els[c].name != VALUES).collect())
}

/// The element a [`VALUES`] line is for: the next sibling that is not one.
fn values_target(sc: &TedScene, values: usize) -> Option<usize> {
    let els = sc.doc().elements();
    let parent = els[values].parent?;
    let list = &els[parent].children;
    let at = list.iter().position(|&c| c == values)?;
    list[at + 1..].iter().copied().find(|&c| els[c].name != VALUES)
}

enum Tidy {
    Remove,
    Unwrap,
}

/// Tidy: takes away a variation folder that holds every variation of its parent (its contents in its place), one that holds none of
/// them or nothing, values per variation that no variation of their element has or that are its own anyway; joins side-by-side
/// folders with the same contents. No variation's file changes. Returns how many edits it made.
///
/// # Errors
///
/// An edit failed.
pub fn tidy(sc: &mut TedScene, all: &[String]) -> Result<usize> {
    let mut changes = 0;
    loop {
        bake(sc)?;
        let doc = sc.doc();
        let els = doc.elements();
        let mut acts = vec![];
        for e in (0..els.len()).rev() {
            let Some(parent) = els[e].parent else { continue };
            if els[e].name == FOLDER {
                let set = effective(doc, parent, all);
                let own = own_variations(doc, e).unwrap_or_else(|| all.to_vec());
                let kept = set.iter().filter(|v| own.contains(v)).count();
                if kept == 0 || els[e].children.is_empty() {
                    acts.push((e, Tidy::Remove));
                } else if kept == set.len() {
                    acts.push((e, Tidy::Unwrap));
                }
            } else if els[e].name == VALUES {
                let Some(target) = values_target(sc, e) else {
                    acts.push((e, Tidy::Remove));
                    continue;
                };

                let set = effective(doc, target, all);
                let own = own_variations(doc, e).unwrap_or_default();
                let same = els[e]
                    .attrs
                    .iter()
                    .enumerate()
                    .filter(|(_, a)| a.name != VARIATIONS_ATTR)
                    .all(|(i, a)| doc.find_attr(target, &a.name).is_some_and(|k| doc.attr_raw(target, k) == doc.attr_raw(e, i)));
                if same || !own.iter().any(|v| set.contains(v)) {
                    acts.push((e, Tidy::Remove));
                }
            }
        }

        // Side-by-side folders with the same contents: one, for both their variations.
        if acts.is_empty() {
            let mut joins = vec![];
            for p in 0..els.len() {
                let kids = &els[p].children;
                for w in kids.windows(2).rev() {
                    let (x, y) = (w[0], w[1]);
                    if els[x].name == FOLDER && els[y].name == FOLDER && contents(sc, x) == contents(sc, y) {
                        let (lx, ly) = (own_variations(doc, x).unwrap_or_default(), own_variations(doc, y).unwrap_or_default());
                        let union = all.iter().filter(|v| lx.contains(v) || ly.contains(v)).cloned().collect::<Vec<_>>();
                        joins.push((x, y, union));
                        break;
                    }
                }
            }

            if joins.is_empty() {
                return Ok(changes);
            }

            joins.sort_by_key(|j| std::cmp::Reverse(j.1));
            for (x, y, union) in joins {
                sc.doc_mut().remove_element_inplace(y).map_err(|e| e.to_string())?;
                sc.doc_mut().set(x, VARIATIONS_ATTR, &list_text(&union));
                bake(sc)?;
                changes += 1;
            }

            continue;
        }

        for (e, act) in acts {
            match act {
                Tidy::Remove => sc.doc_mut().remove_element_inplace(e).map_err(|e| e.to_string())?,
                Tidy::Unwrap => unwrap_folder(sc, e)?,
            }

            changes += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::variations::to_variation;

    const SCENE: &str = "<project version=\"17\" wce_variations=\"a,b,c\">
    <scene>
        <SCENE_GRAPH>
            <children>
                <LayerInstance name=\"one\">
                    <children>
                        <BUILDING_INSTANCE name=\"w\" position=\"vector(0,0,0)\"/>
                        <WceVariationValues wce_variations=\"b\" position=\"vector(1,5,1)\"/>
                        <BUILDING_INSTANCE name=\"x\" position=\"vector(1,2,1)\"/>
                        <UserInstanceGroup name=\"g\" position=\"vector(10,0,0)\">
                            <children>
                                <BUILDING_INSTANCE name=\"y\" position=\"vector(0,0,0)\"/>
                            </children>
                        </UserInstanceGroup>
                        <BUILDING_INSTANCE name=\"z\" position=\"vector(0,0,0)\"/>
                    </children>
                </LayerInstance>
            </children>
        </SCENE_GRAPH>
    </scene>
</project>
";

    fn all() -> Vec<String> {
        ["a", "b", "c"].map(str::to_owned).to_vec()
    }

    fn scene() -> TedScene {
        TedScene::parse(SCENE.as_bytes().to_vec()).unwrap()
    }

    fn named(sc: &TedScene, name: &str) -> usize {
        (0..sc.doc().elements().len()).find(|&e| sc.doc().get(e, "name").as_deref() == Some(name)).unwrap()
    }

    fn of(sc: &TedScene, v: &str) -> String {
        String::from_utf8(to_variation(&sc.save(), v).unwrap()).unwrap()
    }

    #[test]
    fn tags_come_and_go() {
        let mut sc = scene();
        let g = named(&sc, "g");
        set_variations(&mut sc, g, Some(&["a".to_owned()])).unwrap();
        assert!(String::from_utf8(sc.save()).unwrap().contains("position=\"vector(10,0,0)\" wce_variations=\"a\">"));
        assert!(!of(&sc, "b").contains("name=\"y\""));
        sc.begin_edit().unwrap();
        set_variations(&mut sc, g, None).unwrap();
        assert_eq!(sc.save(), SCENE.as_bytes());
        let x = named(&sc, "x");
        assert!(set_variations(&mut sc, x, None).is_err(), "an instance has its folder's");
    }

    #[test]
    fn a_folder_wraps_and_unwraps() {
        let mut sc = scene();
        let picked = [named(&sc, "x"), named(&sc, "g")];
        let f = wrap_in_folder(&mut sc, &picked, &["a".to_owned(), "c".to_owned()]).unwrap();
        assert_eq!(sc.type_of(f), FOLDER);
        let text = String::from_utf8(sc.save()).unwrap();
        assert!(text.contains(
            "                        <WceVariationFolder wce_variations=\"a,c\">
                            <WceVariationValues wce_variations=\"b\" position=\"vector(1,5,1)\"/>
                            <BUILDING_INSTANCE name=\"x\""
        ));
        assert!(!of(&sc, "b").contains("name=\"x\""));
        assert_eq!(of(&sc, "a"), of(&scene(), "a"));
        unwrap_folder(&mut sc, f).unwrap();
        assert_eq!(sc.save(), SCENE.as_bytes());
    }

    #[test]
    fn a_split_keeps_every_variation_and_tidy_undoes_it() {
        let before = all().iter().map(|v| of(&scene(), v)).collect::<Vec<_>>();
        let mut sc = scene();
        let picked = [named(&sc, "x"), named(&sc, "g")];
        let copies = split_for(&mut sc, &picked, "b", &all()).unwrap();
        assert_eq!(copies.iter().map(|&c| sc.name(c)).collect::<Vec<_>>(), ["x", "g"]);
        assert_eq!(sc.position(copies[0]), Some(wce_foundation::math::Double3::new(1.0, 5.0, 1.0)), "b's own height, in the copy");
        assert_eq!(all().iter().map(|v| of(&sc, v)).collect::<Vec<_>>(), before);

        // The copies the same as the others': tidy makes it one again (the dead values for b go, the folders join, then go).
        sc.set_position(copies[0], wce_foundation::math::Double3::new(1.0, 2.0, 1.0));
        let n = tidy(&mut sc, &all()).unwrap();
        assert!(n >= 3, "{n}");
        assert_eq!(
            String::from_utf8(sc.save()).unwrap(),
            SCENE.replace("                        <WceVariationValues wce_variations=\"b\" position=\"vector(1,5,1)\"/>\n", "")
        );
    }

    #[test]
    fn a_folder_splits_too() {
        let mut sc = scene();
        let x = [named(&sc, "x")];
        let f = wrap_in_folder(&mut sc, &x, &["a".to_owned(), "b".to_owned()]).unwrap();
        let before = all().iter().map(|v| of(&sc, v)).collect::<Vec<_>>();
        let copies = split_for(&mut sc, &[f], "b", &all()).unwrap();
        assert_eq!(copies.iter().map(|&c| sc.name(c)).collect::<Vec<_>>(), ["x"]);
        assert_eq!(all().iter().map(|v| of(&sc, v)).collect::<Vec<_>>(), before);
    }

    #[test]
    fn a_split_needs_another_variation() {
        let mut sc = scene();
        let l = named(&sc, "one");
        set_variations(&mut sc, l, Some(&["b".to_owned()])).unwrap();
        let x = [named(&sc, "x")];
        assert!(split_for(&mut sc, &x, "b", &all()).is_err());
        assert!(split_for(&mut sc, &x, "a", &all()).is_err());
    }
}
