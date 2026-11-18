//! Structural edits of a vfx file that keep it whole: an emitter is duplicated with its render stack (so the copy draws), deleted with
//! its place in the stacks; references and chains likewise. Each is one undo entry.

use wce_editor_core::document::Document;
use wce_weather::xml_file_document::{Op, XmlFileDocument};

use crate::model::{VfxIndex, unique_name};

/// Runs the steps of one edit, then joins their history entries into one named `label`.
fn as_one(d: &mut XmlFileDocument, label: &str, f: impl FnOnce(&mut XmlFileDocument) -> Result<String, String>) -> Result<String, String> {
    let before = d.history().position();
    let out = f(d);
    let n = d.history().position().saturating_sub(before);
    d.join_last(n, label);
    out
}

fn duplicate(d: &mut XmlFileDocument, elem: usize) -> Result<usize, String> {
    d.structural(Op::Duplicate, elem, 0, "", "")?.ok_or_else(|| "the copy was not made".to_owned())
}

fn remove(d: &mut XmlFileDocument, elem: usize) -> Result<(), String> {
    d.structural(Op::Remove, elem, 0, "", "").map(|_| ())
}

/// A copy of an emitter (`<id>_copy`) right after it, and of each render stack that draws it, drawing the copy alone. Returns the
/// copy's id.
///
/// # Errors
///
/// No such emitter, or an edit failed.
pub fn duplicate_emitter(d: &mut XmlFileDocument, id: &str) -> Result<String, String> {
    as_one(d, &format!("Duplicate emitter {id}"), |d| {
        let ix = VfxIndex::new(d.xml());
        let em = ix.emitter(id).ok_or("no such emitter")?;
        let taken = ix.emitters.iter().map(|e| e.id.as_str()).collect::<Vec<_>>();
        let new_id = unique_name(id, &taken);
        let copy = duplicate(d, em.elem)?;
        d.set_named(copy, "id", &new_id, 0)?;

        // Its stacks, one at a time (each edit renumbers the elements after it).
        let stacks = ix.stacks_of(id).map(|s| s.id.clone()).collect::<Vec<_>>();
        let stack_ids = ix.stacks.iter().map(|s| s.id.clone()).collect::<Vec<_>>();
        let mut taken_stacks = stack_ids.iter().map(String::as_str).collect::<Vec<_>>();
        let mut new_stacks = vec![];
        for stack in &stacks {
            let new_stack = unique_name(stack, &taken_stacks);
            new_stacks.push(new_stack);
            taken_stacks = stack_ids.iter().map(String::as_str).chain(new_stacks.iter().map(String::as_str)).collect();
            let ix = VfxIndex::new(d.xml());
            let Some(s) = ix.stacks.iter().find(|s| s.id == *stack) else { continue };
            let s_copy = duplicate(d, s.elem)?;
            d.set_named(s_copy, "id", new_stacks.last().map_or("", String::as_str), 0)?;

            // In the copy: the new emitter, and no other.
            loop {
                let ix = VfxIndex::new(d.xml());
                let Some(sc) = ix.stacks.iter().find(|s| Some(&s.id) == new_stacks.last()) else { break };
                match sc.emitters.iter().find(|(_, e)| e != id && e != &new_id) {
                    Some(&(other, _)) => remove(d, other)?,
                    None => {
                        if let Some(&(own, _)) = sc.emitters.iter().find(|(_, e)| e == id) {
                            d.set_named(own, "id", &new_id, 0)?;
                        }

                        break;
                    }
                }
            }
        }

        Ok(new_id)
    })
}

/// Deletes an emitter and takes it out of the render stacks; a stack left without emitters goes too.
///
/// # Errors
///
/// No such emitter, or an edit failed.
pub fn delete_emitter(d: &mut XmlFileDocument, id: &str) -> Result<String, String> {
    as_one(d, &format!("Delete emitter {id}"), |d| {
        loop {
            let ix = VfxIndex::new(d.xml());
            let stack = ix.stacks_of(id).next().cloned();
            match stack {
                Some(s) if s.emitters.len() <= 1 => remove(d, s.elem)?,
                Some(s) => {
                    let Some(&(elem, _)) = s.emitters.iter().find(|(_, e)| e == id) else { break };
                    remove(d, elem)?;
                }

                None => break,
            }
        }

        let ix = VfxIndex::new(d.xml());
        let em = ix.emitter(id).ok_or("no such emitter")?;
        remove(d, em.elem)?;
        Ok(format!("deleted emitter {id}"))
    })
}

/// A copy of a behaviour chain (`<id>_copy`) right after it, used by nothing yet. Returns its id.
///
/// # Errors
///
/// No such chain, or an edit failed.
pub fn duplicate_chain(d: &mut XmlFileDocument, id: &str) -> Result<String, String> {
    as_one(d, &format!("Duplicate behaviour chain {id}"), |d| {
        let ix = VfxIndex::new(d.xml());
        let c = ix.chain(id).ok_or("no such behaviour chain")?;
        let taken = ix.chains.iter().map(|c| c.id.as_str()).collect::<Vec<_>>();
        let new_id = unique_name(id, &taken);
        let copy = duplicate(d, c.elem)?;
        d.set_named(copy, "id", &new_id, 0)?;
        Ok(new_id)
    })
}

/// Deletes a behaviour chain no render stack uses.
///
/// # Errors
///
/// No such chain, a stack uses it, or the edit failed.
pub fn delete_chain(d: &mut XmlFileDocument, id: &str) -> Result<String, String> {
    let ix = VfxIndex::new(d.xml());
    let c = ix.chain(id).ok_or("no such behaviour chain")?;
    let users = ix.users_of_chain(id);
    if !users.is_empty() {
        return Err(format!("{id} draws {}: delete or change those first", users.join(", ")));
    }

    remove(d, c.elem)?;
    Ok(format!("deleted behaviour chain {id}"))
}

/// A copy of a reference (`<inst_name>_copy`) right after it. Returns its name.
///
/// # Errors
///
/// No such reference, or an edit failed.
pub fn duplicate_reference(d: &mut XmlFileDocument, inst: &str) -> Result<String, String> {
    as_one(d, &format!("Duplicate reference {inst}"), |d| {
        let ix = VfxIndex::new(d.xml());
        let r = ix.reference(inst).ok_or("no such reference")?;
        let taken = ix.refs.iter().map(|r| r.inst_name.as_str()).collect::<Vec<_>>();
        let new_name = unique_name(inst, &taken);
        let copy = duplicate(d, r.elem)?;
        d.set_named(copy, "inst_name", &new_name, 0)?;
        Ok(new_name)
    })
}

/// Deletes a reference.
///
/// # Errors
///
/// No such reference, or the edit failed.
pub fn delete_reference(d: &mut XmlFileDocument, inst: &str) -> Result<String, String> {
    let ix = VfxIndex::new(d.xml());
    let r = ix.reference(inst).ok_or("no such reference")?;
    remove(d, r.elem)?;
    Ok(format!("deleted reference {inst}"))
}
