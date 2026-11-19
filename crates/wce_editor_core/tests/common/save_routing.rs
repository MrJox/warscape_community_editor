use super::*;

fn documents(changed: &[bool]) -> (DocumentManager, Vec<u32>) {
    let mut manager = DocumentManager::default();
    let mut scene = RenderScene::default();
    let ids = changed
        .iter()
        .enumerate()
        .map(|(i, &changed_on_disk)| {
            let mut doc = NumberDoc::new(1);
            doc.set(0, i as f32 + 1.0, 0);
            doc.changed_on_disk = changed_on_disk;
            manager.add(&mut scene, Box::new(doc), &format!("tool-{i}")).unwrap()
        })
        .collect();
    (manager, ids)
}

#[test]
fn emergency_save_attempts_every_document_without_an_unanswerable_prompt() {
    let (mut manager, ids) = documents(&[false, true, false, false]);
    manager.find_as_mut::<NumberDoc>(ids[2]).unwrap().save_error = Some("disk full".into());
    assert!(manager.save_one(ids[1]).is_err());
    assert!(manager.pending_changed_save().is_some());
    let mut messages = vec![];
    assert!(!manager.save_all_without_prompt(&mut messages));
    assert_eq!(messages.len(), 4);
    assert!(messages.iter().any(|s| s.contains("files changed outside")));
    assert!(messages.iter().any(|s| s.contains("disk full")));
    assert!(manager.pending_changed_save().is_none());
    for (index, &id) in ids.iter().enumerate() {
        let doc = manager.find_as::<NumberDoc>(id).unwrap();
        let failed = index == 1 || index == 2;
        assert_eq!(doc.modified(), failed);
        assert_eq!(doc.saves, i32::from(!failed));
        assert_eq!(doc.values(), &[index as f32 + 1.0]);
    }
}

#[test]
fn save_all_resumes_after_each_overwrite_without_saving_completed_documents_again() {
    let (mut manager, ids) = documents(&[true, true, false]);
    assert!(!manager.save_all(&mut vec![]));
    assert_eq!(manager.pending_changed_save().unwrap().id, ids[0]);
    assert!(manager.answer_changed_save(true).is_err());
    assert_eq!(manager.pending_changed_save().unwrap().id, ids[1]);
    manager.answer_changed_save(true).unwrap();
    assert!(manager.pending_changed_save().is_none());
    for id in ids {
        let doc = manager.find_as::<NumberDoc>(id).unwrap();
        assert_eq!(doc.saves, 1);
        assert!(!doc.modified());
    }
}

#[test]
fn cancelling_save_all_drops_its_continuation_and_keeps_remaining_edits() {
    let (mut manager, ids) = documents(&[true, false]);
    assert!(!manager.save_all(&mut vec![]));
    manager.answer_changed_save(false).unwrap();
    assert!(manager.pending_changed_save().is_none());
    assert!(manager.save_one(ids[0]).is_err());
    manager.answer_changed_save(true).unwrap();
    assert_eq!(manager.find_as::<NumberDoc>(ids[0]).unwrap().saves, 1);
    let later = manager.find_as::<NumberDoc>(ids[1]).unwrap();
    assert_eq!(later.saves, 0);
    assert!(later.modified());
}

#[test]
fn save_all_keeps_earlier_failures_and_continues_after_a_failed_overwrite() {
    for overwrite_fails in [false, true] {
        let (mut manager, ids) = documents(&[false, true, false]);
        manager.find_as_mut::<NumberDoc>(ids[0]).unwrap().save_error = Some("first disk full".into());
        assert!(!manager.save_all(&mut vec![]));
        if overwrite_fails {
            manager.find_as_mut::<NumberDoc>(ids[1]).unwrap().save_error = Some("overwrite disk full".into());
        }
        let error = manager.answer_changed_save(true).unwrap_err();
        assert!(error.contains("first disk full"));
        assert_eq!(error.contains("overwrite disk full"), overwrite_fails);
        assert!(manager.pending_changed_save().is_none());
        for id in ids {
            assert_eq!(manager.find_as::<NumberDoc>(id).unwrap().save_attempts, 1, "each document is attempted once");
        }
    }
}

#[test]
fn headless_save_mode_clears_stale_choices_and_attempts_the_remaining_documents() {
    let (mut manager, ids) = documents(&[true, false]);
    assert!(manager.save_one(ids[0]).is_err());
    assert!(manager.pending_changed_save().is_some());
    assert!(manager.set_save_prompts(false));
    assert!(!manager.save_all(&mut vec![]));
    assert!(manager.pending_changed_save().is_none());
    assert_eq!(manager.find_as::<NumberDoc>(ids[1]).unwrap().saves, 1);
    assert!(manager.find(ids[0]).unwrap().modified());
    assert!(!manager.set_save_prompts(true));
    assert!(manager.save_one(ids[0]).is_err());
    assert!(manager.pending_changed_save().is_some(), "interactive Save can still ask after a script");
}
