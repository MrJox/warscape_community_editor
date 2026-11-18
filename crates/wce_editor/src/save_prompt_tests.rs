//! Real modal frames without a native window, using a software D3D device solely
//! to construct the shell. No game assets, user settings or renderer are used.
use super::*;
use dear_imgui_sys as sys;
use std::{
    any::Any,
    cell::Cell,
    rc::Rc,
    sync::{Mutex, MutexGuard},
};
use wce_d3d11::*;
use wce_editor_core::{
    autosave::AutosaveMeta,
    command::{CommandStack, History},
    document::{DocumentCore, SaveTarget},
};
use wce_foundation::settings::Paths;
use wce_scene::render_scene::LayerId;

static UI: Mutex<()> = Mutex::new(());

struct SaveDoc {
    core: DocumentCore,
    history: CommandStack<()>,
    path: PathBuf,
    dirty: bool,
    outside: bool,
    fail: bool,
    saves: Rc<Cell<usize>>,
}
impl SaveDoc {
    fn new(path: PathBuf, outside: bool) -> Self {
        Self { core: DocumentCore::new(), history: CommandStack::new(()), path, dirty: true, outside, fail: false, saves: Rc::default() }
    }
}
impl Document for SaveDoc {
    fn core(&self) -> &DocumentCore {
        &self.core
    }
    fn core_mut(&mut self) -> &mut DocumentCore {
        &mut self.core
    }
    fn history(&self) -> &dyn History {
        &self.history
    }
    fn history_mut(&mut self) -> &mut dyn History {
        &mut self.history
    }
    fn title(&self) -> String {
        "save fixture".into()
    }
    fn kind(&self) -> String {
        "save fixture".into()
    }
    fn modified(&self) -> bool {
        self.dirty
    }
    fn can_save(&self) -> bool {
        true
    }
    fn save_destination(&self) -> Option<PathBuf> {
        Some(self.path.clone())
    }
    fn save_changes(&self, _: &SaveTarget) -> Result<Option<String>, String> {
        Ok(self.outside.then(|| "TEd changed the original".into()))
    }
    fn save(&mut self, _: &SaveTarget) -> Result<(), String> {
        if self.fail {
            return Err("injected save failure".into());
        }
        self.saves.set(self.saves.get() + 1);
        self.dirty = false;
        self.outside = false;
        Ok(())
    }
    fn autosave_meta(&self) -> Option<AutosaveMeta> {
        Some(AutosaveMeta { title: self.title(), kind: self.kind(), target: self.path.clone(), ..Default::default() })
    }
    fn autosave(&mut self, dir: &Path) -> Result<(), String> {
        std::fs::write(dir.join("draft.txt"), b"unsaved in-memory edits").map_err(|e| e.to_string())
    }
    fn publish(&mut self, _: &mut RenderScene, _: LayerId) {}
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

struct Fixture {
    editor: Option<Editor>,
    context: *mut sys::ImGuiContext,
    root: PathBuf,
    _guard: MutexGuard<'static, ()>,
}
impl Fixture {
    fn new() -> Self {
        let guard = UI.lock().unwrap_or_else(|e| e.into_inner());
        let root = std::env::temp_dir().join(format!(
            "wce-save-prompts-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let mut settings = Settings::load(Paths::in_dir(&root.join("settings")));
        settings.game_mut().projects = Some(root.join("projects"));
        settings.autosave.enabled = false;
        let (mut device, mut immediate) = (None, None);
        // SAFETY: valid output pointers; WARP needs no window or physical GPU.
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_WARP,
                HMODULE::default(),
                0,
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION as u32,
                Some(&mut device),
                None,
                Some(&mut immediate),
            )
            .ok()
            .unwrap();
        }
        let editor = Editor::with_settings(&device.unwrap(), &immediate.unwrap(), settings);
        // SAFETY: serialized test owns the current context until Drop. No platform backend is attached.
        let context = unsafe {
            let context = sys::igCreateContext(std::ptr::null_mut());
            let io = &mut *sys::igGetIO_Nil();
            io.DisplaySize = gui::v2(800.0, 600.0);
            io.DeltaTime = 1.0 / 60.0;
            io.IniFilename = std::ptr::null();
            io.LogFilename = std::ptr::null();
            io.BackendFlags |= sys::ImGuiBackendFlags_RendererHasTextures as i32;
            context
        };
        Self { editor: Some(editor), context, root, _guard: guard }
    }
    fn editor(&mut self) -> &mut Editor {
        self.editor.as_mut().unwrap()
    }
    fn add(&mut self, outside: bool) -> (DocumentId, Rc<Cell<usize>>) {
        let index = self.editor().docs.order().len();
        let doc = SaveDoc::new(self.root.join(format!("document-{index}")), outside);
        let saves = doc.saves.clone();
        let e = self.editor();
        let id = e.docs.add(&mut e.scene, Box::new(doc), &format!("tool-{index}")).unwrap();
        (id, saves)
    }
    fn prompt(&mut self, kind: PromptKind, id: DocumentId, opened: Rc<Cell<bool>>) {
        match kind {
            PromptKind::Close => self.editor().ask_close(id),
            PromptKind::Quit => self.editor().ask_quit(),
            PromptKind::Replace => {
                assert!(
                    !self
                        .editor()
                        .clear_to_replace(
                            "tool-0",
                            Box::new(move |e| {
                                opened.set(true);
                                e.close_document(id);
                                Ok("replacement opened".into())
                            })
                        )
                        .unwrap()
                );
            }
            PromptKind::None => {}
        }
    }
    fn frame(&mut self) {
        // SAFETY: this fixture owns a current context, with display size and font-texture support set.
        unsafe {
            sys::igNewFrame();
        }
        self.editor().draw_prompt();
        self.editor().draw_changed_save_prompt();
        // SAFETY: finishes exactly the frame above; nothing is submitted to a GPU.
        unsafe {
            sys::igRender();
        }
    }
    fn click_close(&mut self, name: &str) {
        self.frame();
        self.frame();
        // SAFETY: the named window was just drawn; IO events are consumed by the following frames.
        let (x, y) = unsafe {
            let window = sys::igFindWindowByName(gui::cs(name).as_ptr());
            assert!(!window.is_null());
            let style = &*sys::igGetStyle();
            let font = (*self.context).FontSize;
            ((*window).Pos.x + (*window).Size.x - style.FramePadding.x - font * 0.5, (*window).Pos.y + style.FramePadding.y + font * 0.5)
        };
        self.click_at(x, y);
    }
    fn click_overwrite(&mut self, name: &str) {
        self.frame();
        self.frame();
        // SAFETY: the current modal starts with one text line, then its Overwrite button.
        // Read its live content origin and font metrics rather than assuming window coordinates.
        let (x, y) = unsafe {
            let window = sys::igFindWindowByName(gui::cs(name).as_ptr());
            assert!(!window.is_null());
            let style = &*sys::igGetStyle();
            let font = (*self.context).FontSize;
            (
                (*window).DC.CursorStartPos.x + style.FramePadding.x + 1.0,
                (*window).DC.CursorStartPos.y + font + style.ItemSpacing.y + style.FramePadding.y + font * 0.5,
            )
        };
        self.click_at(x, y);
    }
    fn click_at(&mut self, x: f32, y: f32) {
        // SAFETY: the fixture's current IO, with no native input backend.
        unsafe {
            sys::ImGuiIO_AddMousePosEvent(sys::igGetIO_Nil(), x, y);
        }
        self.frame();
        // SAFETY: feed a left-button press, then its release, on the modal's real button.
        unsafe {
            sys::ImGuiIO_AddMouseButtonEvent(sys::igGetIO_Nil(), 0, true);
        }
        self.frame();
        // SAFETY: balances the synthetic button press above.
        unsafe {
            sys::ImGuiIO_AddMouseButtonEvent(sys::igGetIO_Nil(), 0, false);
        }
        self.frame();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.editor.take());
        // SAFETY: destroys only the context created and exclusively owned by this fixture.
        unsafe {
            sys::igDestroyContext(self.context);
        }
        let root = self.root.canonicalize().unwrap();
        assert!(
            root.starts_with(std::env::temp_dir().canonicalize().unwrap())
                && root.file_name().unwrap().to_string_lossy().starts_with("wce-save-prompts-")
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn disk_change_overwrite_resumes_close_replace_and_quit_in_one_modal() {
    for kind in [PromptKind::Close, PromptKind::Replace, PromptKind::Quit] {
        let mut f = Fixture::new();
        let (id, saves) = f.add(true);
        let opened = Rc::new(Cell::new(false));
        f.prompt(kind, id, opened.clone());
        assert!(f.editor().answer_prompt(Answer::Save).is_err());
        for _ in 0..3 {
            f.frame();
            // SAFETY: this test owns the current context and only reads its popup/window bookkeeping.
            unsafe {
                assert_eq!((*f.context).OpenPopupStack.Size, 1);
                let sibling = sys::igFindWindowByName(c"Files changed on disk".as_ptr());
                assert!(sibling.is_null() || (*sibling).LastFrameActive != sys::igGetFrameCount());
            }
        }
        f.click_overwrite("Unsaved changes");
        assert_eq!(f.editor().prompt_kind(), PromptKind::None);
        assert!(f.editor().docs.pending_changed_save().is_none());
        assert_eq!(saves.get(), 1);
        assert_eq!(opened.get(), kind == PromptKind::Replace);
        assert_eq!(f.editor().wants_quit(), kind == PromptKind::Quit);
        assert_eq!(f.editor().docs.find(id).is_none(), kind != PromptKind::Quit);
    }
}

#[test]
fn modal_x_cancels_standalone_and_pending_close_replace_quit_saves() {
    for kind in [PromptKind::None, PromptKind::Close, PromptKind::Replace, PromptKind::Quit] {
        let mut f = Fixture::new();
        let (id, saves) = f.add(true);
        let opened = Rc::new(Cell::new(false));
        f.prompt(kind, id, opened.clone());
        if kind == PromptKind::None {
            assert!(f.editor().docs.save_one(id).is_err());
        } else {
            assert!(f.editor().answer_prompt(Answer::Save).is_err());
        }
        f.click_close(if kind == PromptKind::None { "Files changed on disk" } else { "Unsaved changes" });
        assert!(f.editor().docs.pending_changed_save().is_none(), "X must cancel {kind:?}");
        assert_eq!(f.editor().prompt_kind(), PromptKind::None);
        assert!(f.editor().docs.find(id).unwrap().modified());
        assert!(!f.editor().wants_quit());
        assert!(!opened.get());
        assert_eq!(saves.get(), 0);
        f.frame();
        // SAFETY: no popup should have been reopened in the fixture's frame.
        unsafe {
            assert_eq!((*f.context).OpenPopupStack.Size, 0);
        }
    }
}

#[test]
fn pending_quit_continues_through_multiple_conflicts_without_resaving() {
    let mut f = Fixture::new();
    let (_, first_saves) = f.add(true);
    let (second, second_saves) = f.add(true);
    f.editor().ask_quit();
    assert!(f.editor().answer_prompt(Answer::Save).is_err());
    assert!(f.editor().answer_changed_save(true).is_err());
    assert_eq!(f.editor().docs.pending_changed_save().unwrap().id, second);
    assert_eq!(f.editor().prompt_kind(), PromptKind::Quit);
    assert!(!f.editor().wants_quit());
    f.frame();
    f.editor().answer_changed_save(true).unwrap();
    assert!(f.editor().wants_quit());
    assert_eq!(first_saves.get(), 1);
    assert_eq!(second_saves.get(), 1);
}

#[test]
fn cancelled_or_failed_overwrite_preserves_the_document_and_pending_action() {
    for fail in [false, true] {
        let mut f = Fixture::new();
        let (id, saves) = f.add(true);
        f.editor().ask_close(id);
        assert!(f.editor().answer_prompt(Answer::Save).is_err());
        if fail {
            f.editor().docs.find_as_mut::<SaveDoc>(id).unwrap().fail = true;
            assert!(f.editor().answer_changed_save(true).is_err());
            assert_eq!(f.editor().prompt_kind(), PromptKind::Close);
        } else {
            f.editor().answer_changed_save(false).unwrap();
            assert_eq!(f.editor().prompt_kind(), PromptKind::None);
        }
        assert!(f.editor().docs.pending_changed_save().is_none());
        assert!(f.editor().docs.find(id).unwrap().modified());
        assert_eq!(saves.get(), 0);
        assert!(!f.editor().wants_quit());
    }
}

#[test]
fn emergency_save_keeps_recovery_of_the_conflict_and_saves_later_documents() {
    let mut f = Fixture::new();
    let (blocked, blocked_saves) = f.add(true);
    let (later, later_saves) = f.add(false);
    let (all, messages) = f.editor().save_all_documents();
    assert!(!all);
    assert!(f.editor().docs.pending_changed_save().is_none());
    assert!(f.editor().docs.find(blocked).unwrap().modified());
    assert!(!f.editor().docs.find(later).unwrap().modified());
    assert_eq!(blocked_saves.get(), 0);
    assert_eq!(later_saves.get(), 1);
    assert!(messages.iter().any(|s| s.contains("Recovery copies preserved: 1")));
    let recovery = f.editor().autosave_root();
    drop(f.editor.take()); // A device-loss exit leaves the session on disk and releases its lock.
    let found = wce_editor_core::autosave::stale(&recovery);
    assert_eq!(found.len(), 1);
    assert_eq!(std::fs::read(found[0].copy_dir(0).unwrap().join("draft.txt")).unwrap(), b"unsaved in-memory edits");
}
