//! The Audio tool's panels: Audio Banks (every bank, and the player), Audio Bank (one bank's events, dialogue events, music, media
//! and objects as readable trees), Audio Search (names across every bank) and Audio Object (the selected object's fields).

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use wce_audio::decode::{Pcm, decode_wem};
use wce_audio::edit::EditSet;
use wce_audio::encode::{DEFAULT_QUALITY, QUALITIES, encode_wem};
use wce_audio::import::read_sound;
use wce_audio::library::{MediaRef, PlayNode, SoundLibrary, action_name};
use wce_audio::music::{group_kind, item_tree};
use wce_audio::names::NameSource;
use wce_audio::references::{self, Reference, ReferenceIndex};
use wce_audio_format::bank::kind_name;
use wce_audio_format::hirc::{MusicPlaylist, NodeBase, Object};
use wce_editor::editor::Editor;
use wce_editor::gui::{self, v2};
use wce_editor::tool::Tool;
use wce_editor_core::command_registry::{CommandRegistry, CommandResult};
use wce_vfs::files::Files;

use crate::document::AudioDocument;
use crate::player::Player;
use wce_editor_core::document::{Document, SaveTarget};

mod game_data;
mod player_view;

/// What a click asked for; done after the panel is drawn.
enum Request {
    Play(MediaRef, String),
    PlayAny(Arc<PlayNode>),
    /// Play what an object plays (resolved on demand).
    PlayObject(u32),
    Select(u32),
    /// Play a media file from a point (0 to 1).
    PlayFrom(MediaRef, String, f32),
    /// Stops the player (a row's Stop).
    Stop,
    /// Replace a media file's audio with a sound file the user picks.
    Replace(u32),
    /// Add a sound file the user picks to a random/sequence container.
    AddVariation(u32),
    RemoveVariation(u32, u32),
    SetProperty(u32, u8, Option<f32>),
    Delete(u32),
    /// (bank, name, target): a new event playing the target.
    NewEvent(usize, String, u32),
    /// A row of the animation triggers firing this event in battle.
    AddTrigger(String),
    /// (playlist, the leaf item to add after): pick a sound file.
    AddMusic(u32, u32),
    ReplaceSegment(u32),
    /// (playlist, leaf item).
    RemoveEntry(u32, u32),
    /// (playlist, item, weight, loop count).
    SetItem(u32, u32, u32, i16),
    /// Write a loose file (path, bytes, what for the status line).
    WriteLoose(String, Vec<u8>, String),
    /// Name an object (a label kept in the project's names.txt); an empty name removes the label.
    Rename(u32, String),
    /// (container, mode, random mode, avoid repeat, a child's weight).
    SetContainer(u32, u8, u8, u16, Option<(u32, i32)>),
    /// (music switch, a path of states, what it plays).
    SetMusicPath(u32, Vec<u32>, u32),
    /// (music switch or playlist, rule index, the rule).
    SetRule(u32, usize, wce_audio_format::hirc::TransitionRule),
    /// (music object, its stingers).
    SetStingers(u32, Vec<wce_audio_format::hirc::Stinger>),
    /// (action, property, value): its delay (14) or fade time (15).
    SetActionProperty(u32, u8, Option<f32>),
    /// (music switch, a path, the playlist to copy): pick a sound file, a new playlist of it plays for the path.
    PathMusic(u32, Vec<u32>, u32),
    Save,
    Revert,
    Undo,
    Redo,
}

/// What an encoded import goes to.
#[derive(Clone)]
enum ImportTarget {
    Replace(u32),
    Add(u32),
    /// A playlist and the leaf item the new music goes after.
    AddMusic(u32, u32),
    ReplaceSegment(u32),
    /// (music switch, a path, the playlist copied).
    PathMusic(u32, Vec<u32>, u32),
}

/// A bank's lists, made once.
#[derive(Default)]
struct BankLists {
    events: Vec<(String, u32)>,
    dialogue: Vec<(String, u32)>,
    music: Vec<(String, u32)>,
    /// The bank's top containers (whose parent is not a container of this bank: a unit voice bank's switch, played through
    /// another bank's), labelled by the switch values choosing them.
    containers: Vec<(String, u32)>,
    /// Every sound object (streamed ones too), labelled by where it sits; its media.
    sounds: Vec<(String, u32, MediaRef)>,
    all: Vec<(String, u32, u8)>,
}

#[derive(Default)]
pub struct AudioTool {
    library: Option<Arc<SoundLibrary>>,
    loading: Option<Receiver<SoundLibrary>>,
    mount_id: u64,
    files: Option<Arc<dyn Files>>,
    bank_filter: String,
    bank: Option<usize>,
    list_filter: String,
    lists: HashMap<usize, BankLists>,
    /// The bank whose first non-empty tab was brought forward.
    tab_bank: Option<usize>,
    /// The browsers' Play / Stop buttons.
    play: player_view::PlayButtons,
    /// The Audio Object's name field: (object, text).
    rename: (u32, String),
    /// A music switch path's "play instead" field: (path index, text).
    path_target: (usize, String),
    /// A new stinger's (trigger, segment) fields.
    new_stinger: (String, String),
    search: String,
    searched: String,
    results: Vec<(String, u32, u8, usize)>,
    selected: Option<u32>,
    trees: HashMap<u32, Arc<PlayNode>>,
    decoding: Option<(String, Option<MediaRef>, Receiver<Result<Pcm, String>>)>,
    player: Player,
    rng: u64,
    /// The tree node opened by `audio_expand`, and whether the list still has to scroll to it.
    expand: Option<u32>,
    scroll_to_expanded: bool,
    /// What the script commands asked for, applied once the banks are read: (bank, search, select, expand).
    pending: (Option<String>, Option<String>, Option<String>, Option<String>),
    /// Unsaved edits: the Audio edits document's session (crate::document), copied when its history moves; the history's version
    /// seen, and whether it can undo and redo.
    edits: EditSet,
    doc_version: Option<u64>,
    can_undo_redo: (bool, bool),
    /// The import quality (Wwise's 0 to 10), `None` for the default.
    quality: Option<u8>,
    /// A sound file being encoded on a worker: its name, the result, and what it goes to.
    encoding: Option<(String, Receiver<Result<Vec<u8>, String>>, ImportTarget)>,
    /// The game's references (built on a worker after the banks).
    references: Option<Arc<ReferenceIndex>>,
    references_loading: Option<Receiver<(ReferenceIndex, game_data::GameData)>>,
    /// The game's other sound data (animations, ambience, DB tables), read with the references.
    game_data: Option<Arc<game_data::GameData>>,
    data_view: game_data::View,
    /// The Audio Bank window's dock node: the game data panel docks beside it the first time.
    bank_dock: gui::ImGuiID,
    /// `audio_animation`: an animation to show once the game data is read.
    pending_animation: Option<String>,
    /// `audio_show`: a panel to bring to the front.
    pending_panel: Option<String>,
    /// `audio_volume`: "<object> <dB>", an edit made from a script.
    pending_volume: Option<String>,
    /// Where to start the sound being decoded (a click on a waveform), 0 to 1.
    pending_seek: Option<f32>,
    /// Waveforms of the sounds shown (Audio Object, the trees), by media ID; the one being decoded; the ones shown last frame
    /// without a waveform yet, the first decoded next.
    previews: HashMap<u32, Arc<player_view::Preview>>,
    preview_loading: Option<(u32, Receiver<player_view::Preview>)>,
    preview_wanted: Vec<MediaRef>,
    /// The New event field, and the object a Delete waits to be confirmed for.
    new_event_name: String,
    confirm_delete: Option<u32>,
}

/// The Audio tool's document (its edit session), when it has one.
fn audio_doc(e: &mut Editor) -> Option<&mut AudioDocument> {
    let id = e.docs.of_tool("audio")?;
    e.docs.find_as_mut::<AudioDocument>(id)
}

/// The project's file of names given in the editor (one per line).
fn names_file(e: &Editor) -> std::path::PathBuf {
    e.projects_dir().join("audio").join("names.txt")
}

impl AudioTool {
    fn poll(&mut self, e: &mut Editor) {
        self.sync_document(e);
        // The player's end and loop, whatever panel is shown.
        self.player.tick();
        self.play.active = self.player.playing() || self.decoding.is_some();
        if let Some(files) = e.files()
            && files.mount_id() != self.mount_id
            && self.loading.is_none()
        {
            self.mount_id = files.mount_id();
            self.files = Some(files.clone());
            let names_file = names_file(e);
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let mut lib = SoundLibrary::load(files.as_ref());
                // Names given in the editor (new events), kept in the project.
                for line in std::fs::read_to_string(&names_file).unwrap_or_default().lines() {
                    match crate::document::parse_label(line) {
                        Some((id, name)) => lib.names.add_id(id, name, NameSource::Label),
                        None => lib.names.add(line, NameSource::Label),
                    }
                }
                let _ = tx.send(lib);
            });
            self.loading = Some(rx);
        }

        if let Some(rx) = &self.loading
            && let Ok(lib) = rx.try_recv()
        {
            for (path, why) in &lib.failures {
                e.status(&format!("{path}: {why}"), true);
            }
            let lib = Arc::new(lib);
            self.library = Some(lib.clone());
            self.loading = None;
            self.lists.clear();
            self.trees.clear();
            self.results.clear();
            self.searched.clear();
            if let Some(files) = self.files.clone() {
                let (tx, rx) = channel();
                std::thread::spawn(move || {
                    let _ = tx.send((ReferenceIndex::build(files.as_ref(), &lib), game_data::GameData::read(files.as_ref())));
                });
                self.references_loading = Some(rx);
            }
        }

        if let Some(rx) = &self.references_loading
            && let Ok((index, data)) = rx.try_recv()
        {
            self.references = Some(Arc::new(index));
            self.game_data = Some(Arc::new(data));
            self.references_loading = None;
        }

        if let Some(lib) = self.library.clone() {
            self.apply_pending(&lib, e);
        }

        if let Some((media, rx)) = &self.preview_loading
            && let Ok(preview) = rx.try_recv()
        {
            self.previews.insert(*media, Arc::new(preview));
            self.preview_loading = None;
        }
        self.start_preview();

        if let Some((name, rx, target)) = &self.encoding
            && let Ok(result) = rx.try_recv()
        {
            let (name, target) = (name.clone(), target.clone());
            self.encoding = None;
            match result {
                Ok(wem) => match target {
                    ImportTarget::Replace(media) => self.edit(e, &format!("replaced #{media} with {name}"), |edits, lib, files| {
                        edits
                            .replace_audio(lib, files, media, &wem)
                            .map(|id| if id == media { String::new() } else { format!(" (now streamed as #{id})") })
                    }),
                    ImportTarget::Add(container) => self.edit(e, &format!("added {name}"), |edits, lib, _| {
                        edits.add_variation(lib, container, &wem).map(|id| format!(" as #{id}"))
                    }),
                    ImportTarget::AddMusic(playlist, after) => self.edit(e, &format!("added the music {name}"), |edits, lib, _| {
                        edits.add_playlist_entry(lib, playlist, after, &wem).map(|id| format!(" as segment #{id}"))
                    }),
                    ImportTarget::ReplaceSegment(segment) => {
                        self.edit(e, &format!("replaced the music of #{segment} with {name}"), |edits, lib, _| {
                            edits.replace_segment_audio(lib, segment, &wem).map(|_| String::new())
                        })
                    }
                    ImportTarget::PathMusic(switch, keys, like) => {
                        self.edit(e, &format!("made a playlist of {name} for a music path"), |edits, lib, _| {
                            let playlist = edits.new_playlist_like(lib, like, &wem)?;
                            edits.set_music_path(lib, switch, &keys, playlist)?;
                            Ok(format!(" (#{playlist})"))
                        })
                    }
                },
                Err(why) => e.status(&format!("Audio: cannot import {name}: {why}"), true),
            }
        }

        if let Some((label, media, rx)) = &self.decoding
            && let Ok(result) = rx.try_recv()
        {
            let (label, media) = (label.clone(), media.clone());
            self.decoding = None;
            let seek = self.pending_seek.take();
            match result.and_then(|pcm| self.player.play(Arc::new(pcm), &label, media)) {
                Ok(()) => {
                    if let Some(at) = seek {
                        self.player.seek(at);
                    }
                }
                Err(why) => e.status(&format!("cannot play {label}: {why}"), true),
            }
        }
    }

    /// An object by name (any letter case) or `#id`, searching every bank.
    fn find(lib: &SoundLibrary, token: &str) -> Option<(u32, usize)> {
        let token = token.trim();
        let by_id = token.trim_start_matches('#').parse::<u32>().ok();
        let lower = token.to_lowercase();
        lib.banks.iter().enumerate().find_map(|(b, entry)| {
            entry.bank.objects().iter().find_map(|o| {
                let named = lib.names.get(o.id).is_some_and(|n| n.to_lowercase() == lower);
                (by_id == Some(o.id) || named).then_some((o.id, b))
            })
        })
    }

    fn apply_pending(&mut self, lib: &SoundLibrary, e: &mut Editor) {
        if let Some(name) = self.pending.0.take() {
            match lib.banks.iter().position(|b| b.name.eq_ignore_ascii_case(name.trim_end_matches(".bnk"))) {
                Some(i) => self.bank = Some(i),
                None => e.status(&format!("no bank {name}"), true),
            }
        }
        if let Some(text) = self.pending.1.take() {
            self.search = text;
        }
        // An expand first, so a select queued with it (both waiting for the library) keeps the selection.
        for (token, expand) in [(self.pending.3.take(), true), (self.pending.2.take(), false)] {
            let Some(token) = token else { continue };
            match Self::find(lib, &token) {
                Some((id, bank)) => {
                    self.selected = Some(id);
                    self.bank = Some(bank);
                    if expand {
                        self.expand = Some(id);
                        self.scroll_to_expanded = true;
                    }
                }
                None => e.status(&format!("no sound object {token}"), true),
            }
        }
    }

    fn random(&mut self) -> u64 {
        if self.rng == 0 {
            self.rng = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64) | 1;
        }
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    /// A sound the node can play: random containers and switches pick at random, the rest take their first child that plays.
    fn pick<'a>(&mut self, node: &'a PlayNode) -> Option<&'a PlayNode> {
        if node.media.is_some() {
            return Some(node);
        }

        let n = node.children.len();
        if n == 0 {
            return None;
        }

        let start = if matches!(node.kind, 5 | 6) { (self.random() % n as u64) as usize } else { 0 };
        (0..n).find_map(|k| self.pick(&node.children[(start + k) % n]))
    }

    fn handle(&mut self, e: &mut Editor, requests: Vec<Request>) {
        for request in requests {
            match request {
                Request::Play(media, label) => self.start_decode(e, &media, &label),
                Request::PlayAny(node) => match self.pick(&node).and_then(|leaf| leaf.media.clone().map(|m| (m, leaf.label.clone()))) {
                    Some((media, leaf)) => self.start_decode(e, &media, &format!("{} ({leaf})", node.label)),
                    None => e.status(&format!("{} plays no sound", node.label), true),
                },
                Request::Select(id) => self.selected = Some(id),
                Request::PlayObject(id) => {
                    if let Some(tree) = self.tree(id) {
                        self.handle(e, vec![Request::PlayAny(tree)]);
                    }
                }
                Request::Stop => {
                    self.decoding = None;
                    self.player.stop();
                }
                Request::PlayFrom(media, label, at) => {
                    self.pending_seek = Some(at);
                    self.start_decode(e, &media, &label);
                }
                Request::Replace(media) => self.import(e, ImportTarget::Replace(media)),
                Request::AddVariation(container) => self.import(e, ImportTarget::Add(container)),
                Request::AddMusic(playlist, after) => self.import(e, ImportTarget::AddMusic(playlist, after)),
                Request::PathMusic(switch, keys, like) => self.import(e, ImportTarget::PathMusic(switch, keys, like)),
                Request::ReplaceSegment(segment) => self.import(e, ImportTarget::ReplaceSegment(segment)),
                Request::RemoveEntry(playlist, item) => self.edit(e, "removed a music entry", |edits, lib, _| {
                    edits.remove_playlist_entry(lib, playlist, item).map(|()| String::new())
                }),
                Request::SetItem(playlist, item, weight, loop_count) => self.edit(e, "set a music entry", |edits, lib, _| {
                    edits.set_playlist_item(lib, playlist, item, weight, loop_count).map(|()| String::new())
                }),
                Request::RemoveVariation(container, child) => self
                    .edit(e, "removed a variation", |edits, lib, _| edits.remove_variation(lib, container, child).map(|()| String::new())),
                Request::SetProperty(id, property, value) => {
                    self.edit(e, "set a property", |edits, lib, _| edits.set_property(lib, id, property, value).map(|()| String::new()))
                }
                Request::Delete(id) => {
                    self.edit(e, "deleted", |edits, lib, _| edits.delete_object(lib, id).map(|gone| format!(" {} object(s)", gone.len())));
                    if self.selected == Some(id) {
                        self.selected = None;
                    }
                }
                Request::NewEvent(bank, name, target) => {
                    let mut made = None;
                    self.edit(e, &format!("made the event {name}"), |edits, lib, _| {
                        edits.new_event(lib, bank, &name, target).map(|id| {
                            made = Some(id);
                            format!(" (#{id})")
                        })
                    });
                    if let Some(id) = made {
                        self.selected = Some(id);
                        self.new_event_name.clear();
                    }
                }
                Request::AddTrigger(name) => self.edit(e, &format!("added an animation trigger for {name}"), |edits, _, files| {
                    edits
                        .add_trigger(files, &format!("wce_{name}"), &name, "", 0)
                        .map(|row| format!(" (row {row}: a .snd.meta frame fires it with this row)"))
                }),
                Request::WriteLoose(path, bytes, what) => self.edit(e, &what, |edits, _, _| {
                    edits.loose.insert(path, bytes);
                    Ok(String::new())
                }),
                Request::Rename(id, name) => self.edit(e, &format!("named #{id} {name}"), |edits, _, _| {
                    if name.trim().is_empty() {
                        edits.names.remove(&id);
                    } else {
                        edits.names.insert(id, name.trim().to_owned());
                    }
                    Ok(String::new())
                }),
                Request::SetContainer(id, mode, random_mode, avoid, weight) => self.edit(e, "set a container", |edits, lib, _| {
                    edits.set_container(lib, id, mode, random_mode, avoid, weight).map(|()| String::new())
                }),
                Request::SetMusicPath(id, keys, target) => self.edit(e, "set what a music path plays", |edits, lib, _| {
                    edits.set_music_path(lib, id, &keys, target).map(|()| String::new())
                }),
                Request::SetRule(id, index, rule) => self.edit(e, "set a music transition", |edits, lib, _| {
                    edits.set_transition_rule(lib, id, index, rule).map(|()| String::new())
                }),
                Request::SetStingers(id, stingers) => {
                    self.edit(e, "set the stingers", |edits, lib, _| edits.set_stingers(lib, id, stingers).map(|()| String::new()))
                }
                Request::SetActionProperty(id, property, value) => self
                    .edit(e, "set an action", |edits, lib, _| edits.set_action_property(lib, id, property, value).map(|()| String::new())),
                Request::Save => self.save(e),
                Request::Revert => {
                    if let Some(d) = audio_doc(e)
                        && d.revert().is_ok()
                    {
                        e.status("Audio: the unsaved edits are dropped", false);
                    }
                    self.sync_document(e);
                }
                Request::Undo => {
                    if let Some(d) = audio_doc(e) {
                        d.history_mut().undo();
                    }
                    self.sync_document(e);
                }
                Request::Redo => {
                    if let Some(d) = audio_doc(e) {
                        d.history_mut().redo();
                    }
                    self.sync_document(e);
                }
            }
        }
    }

    /// Applies an edit with an undo step; reports its result.
    fn edit(
        &mut self,
        e: &mut Editor,
        what: &str,
        apply: impl FnOnce(&mut EditSet, &SoundLibrary, &dyn Files) -> wce_audio::Result<String>,
    ) {
        let (Some(lib), Some(files)) = (self.library.clone(), self.files.clone()) else { return };
        let mut next = self.edits.clone();
        match apply(&mut next, &lib, files.as_ref()) {
            Ok(note) => {
                // An undo step of the Audio edits document (made with the first edit).
                if audio_doc(e).is_none() {
                    let doc = AudioDocument::new(lib.clone(), e.loose_dir(), names_file(e));
                    if let Err(err) = e.replace_document("audio", Box::new(doc)) {
                        e.status(&format!("Audio: the edits have no document: {err}"), true);
                    }
                }
                match audio_doc(e) {
                    Some(d) => d.push(what, next),
                    None => self.edits = next,
                }
                self.sync_document(e);
                e.status(&format!("Audio: {what}{note} (not saved yet)"), false);
            }
            Err(err) => e.status(&format!("Audio: cannot {}: {err}", what.trim_start_matches("removed ").trim_start_matches("set ")), true),
        }
    }

    /// Asks for a sound file and encodes it on a worker.
    fn import(&mut self, e: &mut Editor, target: ImportTarget) {
        let filters = [("Sound files".to_owned(), "*.wav *.ogg *.wem".to_owned()), ("All files".to_owned(), "*.*".to_owned())];
        let Some(path) = wce_vfs::steam_detect::file_dialog(false, std::path::Path::new(""), &filters) else { return };
        let quality = self.quality.unwrap_or(DEFAULT_QUALITY);
        let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let result = std::fs::read(&path)
                .map_err(|err| err.to_string())
                .and_then(|bytes| read_sound(&bytes).map_err(|err| err.to_string()))
                .and_then(|pcm| encode_wem(&pcm, quality).map_err(|err| err.to_string()));
            let _ = tx.send(result);
        });
        e.status(&format!("Audio: encoding {name}..."), false);
        self.encoding = Some((name, rx, target));
    }

    /// Writes the edited banks and sound files into the project's loose folder (read before the packs).
    fn save(&mut self, e: &mut Editor) {
        let result = audio_doc(e).map(|d| d.save(&SaveTarget { path: std::path::PathBuf::new() }));
        match result {
            Some(Err(err)) => e.status(&format!("Audio: {err}"), true),
            Some(Ok(())) => {}
            None => e.status("Audio: nothing to save", false),
        }
        self.sync_document(e);
    }

    /// Follows the Audio edits document: its session when its history moved (an edit, Undo / Redo here or in the editor's Edit
    /// menu and History panel, Revert, Save), and the files a Save wrote (the editor reads them again).
    fn sync_document(&mut self, e: &mut Editor) {
        let Some(d) = audio_doc(e) else {
            self.can_undo_redo = (false, false);
            return;
        };
        let version = d.history().version();
        self.can_undo_redo = (d.history().can_undo(), d.history().can_redo());
        let saved = d.take_saved();
        if self.doc_version != Some(version) {
            self.edits = d.edits().clone();
            self.doc_version = Some(version);
            self.forget_previews();
        }
        if let Some(written) = saved {
            for path in &written {
                e.loose_changed(path);
            }
            e.status(&format!("Audio: saved {} files into the project's loose folder", written.len()), false);
        }
    }

    /// The bytes of a media file as edited (an unsaved replace or addition first).
    fn media_bytes(&self, media: &MediaRef) -> Option<Vec<u8>> {
        let (lib, files) = (self.library.as_ref()?, self.files.as_ref()?);
        let id = media.id();
        if let Some(bytes) = self.edits.loose.get(&format!("audio/{id}.wem")) {
            return Some(bytes.clone());
        }
        if let Some(bytes) = self.edits.banks.values().find_map(|b| b.media(id)) {
            return Some(bytes.to_vec());
        }
        lib.media_bytes(media, files.as_ref())
    }

    fn start_decode(&mut self, e: &mut Editor, media: &MediaRef, label: &str) {
        let Some(bytes) = self.media_bytes(media) else {
            e.status(&format!("{label}: the media #{} is in no bank and no pack", media.id()), true);
            return;
        };

        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(decode_wem(&bytes).map_err(|err| err.to_string()));
        });
        self.decoding = Some((label.to_owned(), Some(media.clone()), rx));
    }

    fn tree(&mut self, id: u32) -> Option<Arc<PlayNode>> {
        if let Some(t) = self.trees.get(&id) {
            return Some(t.clone());
        }

        let (lib, files) = (self.library.as_ref()?, self.files.as_ref()?);
        // Unsaved edits show at once.
        let edits = &self.edits;
        let tree = Arc::new(lib.resolve_edited(id, files.as_ref(), &|o| edits.edited_object(lib, o)));
        self.trees.insert(id, tree.clone());
        Some(tree)
    }

    fn lists(&mut self, bank: usize) -> Option<&BankLists> {
        let lib = self.library.clone()?;
        let entry = lib.banks.get(bank)?;
        let files = self.files.clone()?;
        Some(self.lists.entry(bank).or_insert_with(|| {
            let mut l = BankLists::default();
            // This bank's containers, by ID, as this bank has them (a unit voice bank holds partial copies of the culture and
            // actor switches above its own, with only its branch).
            let here: HashMap<u32, &Object> = entry
                .bank
                .objects()
                .iter()
                .zip(&entry.typed)
                .filter(|(o, _)| matches!(o.kind, 5 | 6 | 9))
                .filter_map(|(o, t)| Some((o.id, t.as_ref()?)))
                .collect();
            for (i, o) in entry.bank.objects().iter().enumerate() {
                let label = lib.label(o.id, o.kind);
                let typed = entry.typed[i].as_ref();
                match o.kind {
                    4 => l.events.push((label.clone(), o.id)),
                    15 => l.dialogue.push((label.clone(), o.id)),
                    12 | 13 if !has_music_parent(typed) => l.music.push((label.clone(), o.id)),
                    5 | 6 | 9 => {
                        let parent = typed.and_then(Object::node).map_or(0, |n| n.parent_id);
                        if !here.contains_key(&parent) {
                            // Down through copies holding a single container of this bank to the bank's own.
                            let mut top = o.id;
                            while let Some(&[only]) = here.get(&top).map(|t| t.children())
                                && here.contains_key(&only)
                            {
                                top = only;
                            }
                            let kind = lib.object(top).map_or(o.kind, |(_, r, _)| r.kind);
                            let what = match here.get(&top) {
                                Some(Object::Switch(s)) => {
                                    format!("Switch on {} ({} values)", lib.label(s.group_id, 0), s.assignments.len())
                                }
                                Some(t) => format!("{} ({} children)", lib.label(top, kind), t.children().len()),
                                None => lib.label(top, kind),
                            };
                            let place = place_of(&lib, top);
                            l.containers.push((if place.is_empty() { what } else { format!("{place}:  {what}") }, top));
                        }
                    }
                    2 => {
                        if let Some(Object::Sound(s)) = typed {
                            let place = place_of(&lib, o.id);
                            let label = if place.is_empty() { label.clone() } else { format!("{place}  {label}") };
                            l.sounds.push((label, o.id, lib.media_of(&s.source, files.as_ref())));
                        }
                    }
                    _ => {}
                }
                l.all.push((label, o.id, o.kind));
            }
            for v in [&mut l.events, &mut l.dialogue, &mut l.music, &mut l.containers] {
                v.sort_by_key(|(n, _)| n.to_lowercase());
            }
            l.sounds.sort_by_key(|(n, _, _)| n.to_lowercase());
            l
        }))
    }

    // ------------------------------------------------------------------------------------------------------------------ panels

    fn banks_panel(&mut self, requests: &mut Vec<Request>) {
        gui::set_next_window_size(v2(320.0, 500.0), gui::COND_FIRST_USE_EVER);
        if !gui::begin("Audio Banks", None, 0) {
            gui::end();
            return;
        }

        // Edits: what is unsaved, and the import quality.
        if let Some((name, _, _)) = &self.encoding {
            gui::text_disabled(&format!("Encoding {name}..."));
        }
        let (banks, sounds) = (self.edits.banks.len(), self.edits.loose.len());
        if banks + sounds > 0 {
            gui::text(&format!("Unsaved: {banks} bank(s), {sounds} sound file(s)"));
            if gui::small_button("Save") {
                requests.push(Request::Save);
            }
            if gui::is_item_hovered() {
                gui::set_tooltip("Writes the edited banks and sound files into the project's loose folder, read before the game's packs.");
            }
            gui::same_line();
            if gui::small_button("Revert") {
                requests.push(Request::Revert);
            }
        }
        if self.can_undo_redo.0 {
            gui::same_line();
            if gui::small_button("Undo") {
                requests.push(Request::Undo);
            }
        }
        if self.can_undo_redo.1 {
            gui::same_line();
            if gui::small_button("Redo") {
                requests.push(Request::Redo);
            }
        }
        let labels: Vec<String> = QUALITIES
            .iter()
            .map(|q| format!("Import quality {q}{}", if *q == DEFAULT_QUALITY { " (Wwise's default)" } else { "" }))
            .collect();
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        let mut current = QUALITIES.iter().position(|q| *q == self.quality.unwrap_or(DEFAULT_QUALITY)).unwrap_or(0) as i32;
        gui::set_next_item_width(-1.0);
        if gui::combo("##quality", &mut current, &labels) {
            self.quality = QUALITIES.get(current as usize).copied();
        }
        if gui::is_item_hovered() {
            gui::set_tooltip("Vorbis quality of imported sounds, as Wwise's (vanilla mostly uses 0 to 4). Higher is larger and clearer.");
        }
        gui::separator();

        let Some(lib) = self.library.clone() else {
            gui::text_disabled(if self.loading.is_some() { "Reading the game's sound banks..." } else { "No game mounted." });
            gui::end();
            return;
        };

        gui::input_text("##banks", Some("Filter banks"), &mut self.bank_filter, 128, 0);
        let filter = self.bank_filter.to_lowercase();
        let shown: Vec<usize> = (0..lib.banks.len()).filter(|&i| filter.is_empty() || lib.banks[i].name.contains(&filter)).collect();
        if gui::begin_child("bank list", v2(0.0, 0.0), 0, 0) {
            gui::clipped(shown.len(), |row| {
                let i = shown[row];
                let b = &lib.banks[i];
                let (objects, media) = (b.bank.objects().len(), b.bank.media_index().len());
                let what = match (objects, media) {
                    (0, m) => format!("{m} sounds"),
                    (o, 0) => format!("{o} objects"),
                    (o, m) => format!("{o} objects, {m} sounds"),
                };
                if gui::selectable(&format!("{}##{i}", b.name), self.bank == Some(i), 0) {
                    self.bank = Some(i);
                    self.list_filter.clear();
                }
                if gui::is_item_hovered() {
                    gui::set_tooltip(&format!("{}\nbank version {}, {what}", b.path, b.version));
                }
            });
        }
        gui::end_child();
        gui::end();
    }

    fn bank_panel(&mut self, requests: &mut Vec<Request>) {
        gui::set_next_window_size(v2(600.0, 600.0), gui::COND_FIRST_USE_EVER);
        if !gui::begin("Audio Bank", None, 0) {
            gui::end();
            return;
        }
        self.bank_dock = gui::window_dock_id();

        let (Some(lib), Some(bank)) = (self.library.clone(), self.bank) else {
            gui::text_disabled("Pick a bank in Audio Banks, or search for a sound in Audio Search.");
            gui::end();
            return;
        };

        let entry = &lib.banks[bank];
        gui::text(&entry.name);
        gui::same_line();
        gui::text_disabled(&format!("{}  (version {})", entry.path, entry.version));
        gui::input_text("##list", Some("Filter by name"), &mut self.list_filter, 128, 0);
        let filter = self.list_filter.to_lowercase();
        let Some(lists) = self.lists(bank) else {
            gui::end();
            return;
        };
        // A tab shows when the bank has something for it (Events and All always).
        let totals = [lists.events.len(), lists.dialogue.len(), lists.music.len(), lists.containers.len(), lists.sounds.len()];
        let pick = |v: &Vec<(String, u32)>| -> Vec<(String, u32)> {
            v.iter().filter(|(n, _)| filter.is_empty() || n.to_lowercase().contains(&filter)).cloned().collect()
        };
        let (events, dialogue, music) = (pick(&lists.events), pick(&lists.dialogue), pick(&lists.music));
        let all: Vec<(String, u32, u8)> =
            lists.all.iter().filter(|(n, _, _)| filter.is_empty() || n.to_lowercase().contains(&filter)).cloned().collect();

        let containers = pick(&lists.containers);
        let sounds: Vec<(String, u32, MediaRef)> =
            lists.sounds.iter().filter(|(n, _, _)| filter.is_empty() || n.to_lowercase().contains(&filter)).cloned().collect();
        // A bank just shown opens on its first tab with something in it (a unit voice bank has only containers and sounds).
        let first = if self.tab_bank == Some(bank) {
            None
        } else {
            self.tab_bank = Some(bank);
            [events.len(), dialogue.len(), music.len(), containers.len(), sounds.len()].iter().position(|n| *n > 0)
        };
        let flag = |i: usize| if first == Some(i) { gui::TAB_SET_SELECTED } else { 0 };

        if gui::begin_tab_bar("bank tabs") {
            for (i, (tab, items)) in
                [("Events", &events), ("Dialogue", &dialogue), ("Music", &music), ("Containers", &containers)].into_iter().enumerate()
            {
                if i > 0 && totals[i] == 0 {
                    continue;
                }
                if gui::begin_tab_item(&format!("{tab} ({})###{tab}", items.len()), flag(i)) {
                    if gui::begin_child(tab, v2(0.0, 0.0), 0, 0) {
                        for (label, id) in items {
                            self.event_row(label, *id, requests);
                        }
                    }
                    gui::end_child();
                    gui::end_tab_item();
                }
            }

            if totals[4] > 0 && gui::begin_tab_item(&format!("Sounds ({})###sounds", sounds.len()), flag(4)) {
                if gui::begin_child("sounds", v2(0.0, 0.0), 0, 0) {
                    gui::clipped(sounds.len(), |row| {
                        let (label, id, media) = &sounds[row];
                        if self.play.button(&format!("snd{id}"), requests) {
                            requests.push(Request::Play(media.clone(), label.clone()));
                        }
                        gui::same_line();
                        if gui::selectable(&format!("{label}##snd{id}"), self.selected == Some(*id), 0) {
                            requests.push(Request::Select(*id));
                        }
                    });
                }
                gui::end_child();
                gui::end_tab_item();
            }
            let media = entry.bank.media_index();
            if !media.is_empty() && gui::begin_tab_item(&format!("Media ({})###media", media.len()), 0) {
                if gui::begin_child("media", v2(0.0, 0.0), 0, 0) {
                    gui::clipped(media.len(), |row| {
                        let m = media[row];
                        if self.play.button(&format!("m{}", m.id), requests) {
                            requests.push(Request::Play(MediaRef::InBank { bank, id: m.id }, format!("#{}", m.id)));
                        }
                        gui::same_line();
                        gui::text(&format!("#{}  {:.1} KB", m.id, f64::from(m.size) / 1024.0));
                    });
                }
                gui::end_child();
                gui::end_tab_item();
            }

            if gui::begin_tab_item(&format!("All ({})###all", all.len()), 0) {
                if gui::begin_child("all", v2(0.0, 0.0), 0, 0) {
                    gui::clipped(all.len(), |row| {
                        let (label, id, kind) = &all[row];
                        let text = format!("{label}##o{id}");
                        if gui::selectable(&text, self.selected == Some(*id), 0) {
                            requests.push(Request::Select(*id));
                        }
                        gui::same_line();
                        gui::text_disabled(kind_name(*kind));
                    });
                }
                gui::end_child();
                gui::end_tab_item();
            }
            gui::end_tab_bar();
        }

        gui::end();
    }

    /// An event (or dialogue event, music container): a Play button and a tree node that opens on what it plays.
    fn event_row(&mut self, label: &str, id: u32, requests: &mut Vec<Request>) {
        if self.play.button(&format!("p{id}"), requests)
            && let Some(tree) = self.tree(id)
        {
            requests.push(Request::PlayAny(tree));
        }
        if gui::is_item_hovered() && !self.play.active {
            gui::set_tooltip("Plays one of the sounds it can play (random and switch containers pick one at random).");
        }
        gui::same_line();
        if self.expand == Some(id) {
            gui::set_next_item_open(true, gui::COND_ALWAYS);
        }
        let flags = gui::TREE_OPEN_ON_ARROW | gui::TREE_SPAN_AVAIL_WIDTH | if self.selected == Some(id) { gui::TREE_SELECTED } else { 0 };
        let open = gui::tree_node_ex_id(&format!("t{id}"), flags, label);
        if self.expand == Some(id) && self.scroll_to_expanded {
            gui::set_scroll_here_y(0.1);
            self.scroll_to_expanded = false;
        }
        if gui::is_item_clicked() && !gui::is_item_toggled_open() {
            requests.push(Request::Select(id));
        }
        if open {
            if let Some(tree) = self.tree(id) {
                for c in &tree.children {
                    self.play_node(c, &mut vec![id], requests);
                }
            }
            gui::tree_pop();
        }
    }

    fn play_node(&mut self, node: &PlayNode, path: &mut Vec<u32>, requests: &mut Vec<Request>) {
        let lib = self.library.clone();
        let mut label = node.label.clone();
        if node.kind != 0 && node.kind != 3 && node.label.contains('#') {
            label = describe(node, lib.as_deref());
        }

        path.push(node.id);
        let key = path.iter().map(u32::to_string).collect::<Vec<_>>().join(".");
        if let Some(media) = &node.media {
            if self.play.button(&format!("s{key}"), requests) {
                requests.push(Request::Play(media.clone(), node.label.clone()));
            }
            gui::same_line();
            let where_ = match media {
                MediaRef::InBank { bank, id } => format!("#{id} in {}", lib.as_ref().map_or("?", |l| l.banks[*bank].name.as_str())),
                MediaRef::Loose { path, .. } => path.clone(),
                MediaRef::Missing { id } => format!("#{id}: in no bank and no pack"),
            };
            label = format!("{label}  [{where_}]");
        }

        let leaf = node.children.is_empty();
        let flags = gui::TREE_OPEN_ON_ARROW
            | gui::TREE_SPAN_AVAIL_WIDTH
            | if leaf { gui::TREE_LEAF | gui::TREE_NO_PUSH_ON_OPEN } else { 0 }
            | if self.selected == Some(node.id) && node.kind != 0 { gui::TREE_SELECTED } else { 0 };
        // An event opened by `audio_expand` shows its first levels open, down to the sounds of most events.
        if path.len() <= 4 && self.expand.is_some() && self.expand == path.first().copied() {
            gui::set_next_item_open(true, gui::COND_APPEARING);
        }
        let open = gui::tree_node_ex_id(&key, flags, &label);
        if node.kind != 0 && gui::is_item_clicked() && !gui::is_item_toggled_open() {
            requests.push(Request::Select(node.id));
        }
        if open && !leaf {
            for c in &node.children {
                self.play_node(c, path, requests);
            }
            gui::tree_pop();
        }
        path.pop();
    }

    fn search_panel(&mut self, requests: &mut Vec<Request>) {
        gui::set_next_window_size(v2(360.0, 500.0), gui::COND_FIRST_USE_EVER);
        if !gui::begin("Audio Search", None, 0) {
            gui::end();
            return;
        }

        let Some(lib) = self.library.clone() else {
            gui::text_disabled("Reading the game's sound banks...");
            gui::end();
            return;
        };

        gui::input_text("##search", Some("A name or an ID, in every bank"), &mut self.search, 256, 0);
        let query = self.search.trim().to_lowercase();
        if query != self.searched {
            self.searched = query.clone();
            self.results.clear();
            if query.len() >= 2 {
                let id_query = query.trim_start_matches('#').parse::<u32>().ok();
                'banks: for (b, entry) in lib.banks.iter().enumerate() {
                    for o in entry.bank.objects() {
                        let name = lib.names.get(o.id);
                        let hit = id_query == Some(o.id) || name.is_some_and(|n| n.to_lowercase().contains(&query));
                        if hit && !self.results.iter().any(|r| r.1 == o.id) {
                            self.results.push((lib.label(o.id, o.kind), o.id, o.kind, b));
                            if self.results.len() >= 2000 {
                                break 'banks;
                            }
                        }
                    }
                }
                self.results.sort_by_key(|r| r.0.to_lowercase());
            }
        }

        gui::text_disabled(&match self.results.len() {
            0 if query.len() < 2 => "Type at least two letters.".to_owned(),
            n => format!("{n} found"),
        });
        if gui::begin_child("results", v2(0.0, 0.0), 0, 0) {
            let results = &self.results;
            let mut chosen = None;
            gui::clipped(results.len(), |row| {
                let (label, id, kind, bank) = &results[row];
                if gui::selectable(&format!("{label}##r{id}"), self.selected == Some(*id), 0) {
                    chosen = Some((*id, *bank));
                }
                gui::same_line();
                gui::text_disabled(&format!("{}, {}", kind_name(*kind), lib.banks[*bank].name));
            });
            if let Some((id, bank)) = chosen {
                self.bank = Some(bank);
                requests.push(Request::Select(id));
            }
        }
        gui::end_child();
        gui::end();
    }

    fn object_panel(&mut self, requests: &mut Vec<Request>) {
        gui::set_next_window_size(v2(380.0, 500.0), gui::COND_FIRST_USE_EVER);
        if !gui::begin("Audio Object", None, 0) {
            gui::end();
            return;
        }

        let (Some(lib), Some(id)) = (self.library.clone(), self.selected) else {
            gui::text_disabled("Select an event, container or sound.");
            gui::end();
            return;
        };
        // The object as edited (an unsaved edit shows here at once; a new object exists only in the edits).
        let edited = self.edits.object(&lib, id);
        let (kind, typed) = match (lib.object(id), &edited) {
            (Some((_, raw, typed)), _) => (raw.kind, typed),
            (None, Some(o)) => (o.kind(), None),
            (None, None) => {
                gui::text_disabled(&format!("#{id} is in no bank."));
                gui::end();
                return;
            }
        };
        let is_new = lib.object(id).is_none();

        gui::text(&self.edits.names.get(&id).cloned().unwrap_or_else(|| lib.label(id, kind)));
        gui::text_disabled(&format!("{}  #{id}", kind_name(kind)));
        // Your own name for it (kept in the project's names.txt, read with the banks).
        if self.rename.0 != id {
            self.rename = (id, self.edits.names.get(&id).cloned().unwrap_or_default());
        }
        gui::set_next_item_width(220.0);
        if gui::input_text("##rename", Some("your name for it (Enter)"), &mut self.rename.1, 128, gui::INPUT_ENTER_RETURNS_TRUE) {
            requests.push(Request::Rename(id, self.rename.1.clone()));
        }
        if gui::is_item_hovered() {
            gui::set_tooltip("A label of yours, shown everywhere instead of its ID; kept in the project's audio/names.txt on Save.");
        }
        if is_new {
            gui::text_colored([1.0, 0.8, 0.3, 1.0], "New, not saved");
        } else if let Some(source) = lib.names.source(id) {
            gui::text_disabled(&format!("Name from {}", source.describe()));
        }
        let banks: Vec<&str> = lib.banks_of(id).iter().map(|b| lib.banks[*b].name.as_str()).collect();
        if !banks.is_empty() {
            gui::text_wrapped(&format!("In {}: {}", if banks.len() == 1 { "bank" } else { "banks" }, banks.join(", ")));
        }
        if gui::button("Play")
            && let Some(tree) = self.tree(id)
        {
            requests.push(Request::PlayAny(tree));
        }
        gui::separator();

        if !is_new && edited.is_some() && self.edits.banks.keys().any(|b| lib.banks_of(id).contains(b)) {
            gui::text_colored([1.0, 0.8, 0.3, 1.0], "Edited, not saved");
        }
        match edited.as_ref().or(typed) {
            None => gui::text_disabled("Not read field by field (this bank version or kind is kept as bytes)."),
            Some(object) => {
                fields(id, object, &lib, requests);
                if let Object::Sound(_) = object {
                    gui::text_disabled("Its waveform, levels and player: Inspector.");
                }
                edit_fields(id, object, requests);
                if let Object::MusicSwitch(switch) = object {
                    music_switch_view(id, switch, &lib, &mut self.path_target, requests);
                }
                let (rules, stingers) = match object {
                    Object::MusicSwitch(m) => (Some(&m.rules), Some(&m.base.stingers)),
                    Object::MusicPlaylist(p) => (Some(&p.rules), Some(&p.base.stingers)),
                    Object::MusicSegment(s) => (None, Some(&s.base.stingers)),
                    _ => (None, None),
                };
                if let Some(rules) = rules {
                    transitions_view(id, rules, &lib, requests);
                }
                if let Some(stingers) = stingers {
                    stingers_view(id, stingers, &lib, &mut self.new_stinger, requests);
                }
                if let Object::MusicPlaylist(playlist) = object {
                    playlist_view(id, playlist, &lib, &self.edits, &mut self.play, requests);
                }
                if let Some(node) = object.node() {
                    node_fields(node, &lib, requests);
                }
                if let Object::RandomSequence(r) = object {
                    container_settings(id, r, requests);
                }
                if !object.children().is_empty() && gui::collapsing_header(&format!("Children ({})", object.children().len()), 0) {
                    for c in object.children() {
                        let kind = self.edits.object(&lib, *c).map_or_else(|| lib.object(*c).map_or(0, |(_, o, _)| o.kind), |o| o.kind());
                        if let Object::RandomSequence(r) = object {
                            if gui::small_button(&format!("Remove##rm{c}")) {
                                requests.push(Request::RemoveVariation(id, *c));
                            }
                            gui::same_line();
                            // Its weight in random play (Wwise's percentage: 50 is the default).
                            let mut weight = r.playlist.iter().find(|(x, _)| x == c).map_or(50_000, |(_, w)| *w) as f32 / 1000.0;
                            gui::set_next_item_width(80.0);
                            gui::drag_float(&format!("##w{c}"), &mut weight, 0.5, 0.0, 100.0, "weight %.0f");
                            if gui::is_item_hovered() {
                                gui::set_tooltip("How likely random play picks it, against the others' weights (50 is the default).");
                            }
                            if gui::is_item_deactivated_after_edit() {
                                let w = Some((*c, (weight * 1000.0).round() as i32));
                                requests.push(Request::SetContainer(id, r.mode, r.random_mode, r.avoid_repeat_count, w));
                            }
                            gui::same_line();
                        }
                        if gui::selectable(&format!("{}##c{c}", lib.label(*c, kind)), false, 0) {
                            requests.push(Request::Select(*c));
                        }
                    }
                }
            }
        }

        self.references_section(&lib, id, kind, is_new, requests);
        gui::end();
    }

    /// Who uses the object, Delete (confirmed, with the game's uses as warnings), New event, Add to the animation triggers.
    fn references_section(&mut self, lib: &SoundLibrary, id: u32, kind: u8, is_new: bool, requests: &mut Vec<Request>) {
        gui::separator();
        let refs = self.references.as_ref().map(|index| references::find(lib, &self.edits, index, id));
        match &refs {
            None => gui::text_disabled("Finding what uses it..."),
            Some(refs) if refs.is_empty() => {
                if kind == 4 {
                    gui::text_colored([1.0, 0.6, 0.3, 1.0], "Nothing triggers this event: no animation, UI, map or game list names it.");
                } else {
                    gui::text_disabled("Used by nothing.");
                }
            }
            Some(refs) => {
                if gui::collapsing_header(&format!("Used by ({})", refs.len()), 0) {
                    for r in refs.iter().take(200) {
                        match r {
                            Reference::Object { id: other, .. } => {
                                if gui::selectable(&format!("{}##u{other}", r.describe(lib)), false, 0) {
                                    requests.push(Request::Select(*other));
                                }
                            }
                            _ => gui::text_wrapped(&r.describe(lib)),
                        }
                    }
                }
            }
        }

        // Delete, confirmed.
        if gui::button("Delete...") {
            self.confirm_delete = Some(id);
            gui::open_popup("Delete sound object");
        }
        if gui::begin_popup_modal("Delete sound object", None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            let target = self.confirm_delete.unwrap_or(id);
            gui::text(&format!(
                "Delete {} and everything only it uses?",
                self.edits.names.get(&target).cloned().unwrap_or_else(|| lib.label(target, kind))
            ));
            let game: Vec<String> = refs.iter().flatten().filter(|r| r.from_game()).map(|r| r.describe(lib)).collect();
            if !game.is_empty() {
                gui::text_colored([1.0, 0.6, 0.3, 1.0], "The game still uses it:");
                for g in game.iter().take(12) {
                    gui::bullet_text(g);
                }
                if game.len() > 12 {
                    gui::text_disabled(&format!("...and {} more", game.len() - 12));
                }
            }
            if gui::button("Delete") {
                requests.push(Request::Delete(target));
                self.confirm_delete = None;
                gui::close_current_popup();
            }
            gui::same_line();
            if gui::button("Cancel") {
                self.confirm_delete = None;
                gui::close_current_popup();
            }
            gui::end_popup();
        }

        // A new event playing this object (sounds and containers), or an animation trigger for an event.
        if matches!(kind, 2 | 5 | 6 | 7 | 9) && !is_new {
            gui::set_next_item_width(220.0);
            gui::input_text("##newevent", Some("new_event_name"), &mut self.new_event_name, 128, 0);
            gui::same_line();
            if gui::button("New event playing this")
                && let Some(&bank) = lib.banks_of(id).first()
            {
                requests.push(Request::NewEvent(bank, self.new_event_name.clone(), id));
            }
        }
        if kind == 4 {
            let name = self.edits.names.get(&id).cloned().or_else(|| lib.names.get(id).map(str::to_owned));
            if let Some(name) = name
                && gui::button("Add to the animation triggers")
            {
                requests.push(Request::AddTrigger(name));
            }
            if gui::is_item_hovered() {
                gui::set_tooltip(
                    "Adds a row to sound_animation_triggers.csv that fires this event in battle.\nA SOUND_TRIGGER frame of an animation's .snd.meta holding that row plays it.",
                );
            }
        }
    }
}

/// Where an object sits, by the switch values above it (the nearest last): "Barbarian / Inf1 / attack_dogs".
fn place_of(lib: &SoundLibrary, id: u32) -> String {
    let mut parts = vec![];
    let mut child = id;
    for _ in 0..8 {
        let parent = lib.object(child).and_then(|(_, _, t)| t.and_then(Object::node)).map_or(0, |n| n.parent_id);
        if parent == 0 {
            break;
        }
        match lib.object(parent) {
            Some((_, _, Some(Object::Switch(s)))) => {
                if let Some((value, _)) = s.assignments.iter().find(|(_, c)| c.contains(&child)) {
                    parts.push(lib.label(*value, 0));
                }
            }
            Some((_, p, _)) if matches!(p.kind, 5 | 9) => {}
            _ => break,
        }
        child = parent;
    }
    parts.reverse();
    parts.join(" / ")
}

fn has_music_parent(object: Option<&Object>) -> bool {
    object.and_then(Object::node).is_some_and(|n| n.parent_id != 0)
}

/// A readable label for an unnamed container or sound: what it is and what it holds.
fn describe(node: &PlayNode, lib: Option<&SoundLibrary>) -> String {
    let count = node.children.len();
    let typed = lib.and_then(|l| l.object(node.id)).and_then(|(_, _, t)| t);
    match typed {
        Some(Object::RandomSequence(r)) => {
            format!("{} of {count}", if r.mode == 1 { "Random" } else { "Sequence" })
        }
        Some(Object::Switch(s)) => {
            let group = lib.map_or(format!("#{}", s.group_id), |l| l.label(s.group_id, 0));
            format!("Switch on {group} ({count} values)")
        }
        Some(Object::Blend(_)) => format!("Blend of {count}"),
        Some(Object::Sound(_)) => "Sound".to_owned(),
        _ => node.label.clone(),
    }
}

/// The fields of a typed object that are not its node parameters.
fn fields(id: u32, object: &Object, lib: &SoundLibrary, requests: &mut Vec<Request>) {
    let row = |name: &str, value: String| {
        gui::text_disabled(name);
        gui::same_line();
        gui::text(&value);
    };

    match object {
        Object::Sound(s) => {
            let stream = match s.source.stream_type {
                0 => "in the bank",
                1 => "prefetched, then streamed",
                _ => "streamed from a file",
            };
            row("Media", format!("#{} ({stream})", s.source.source_id));
            row("Size in memory", format!("{} bytes", s.source.in_memory_size));
            row("Codec", if s.source.plugin_id == 0x40001 { "Vorbis".to_owned() } else { format!("{:#x}", s.source.plugin_id) });
        }
        Object::Action(a) => {
            row("Action", format!("{} ({:#06x})", action_name(a.action_type), a.action_type));
            if let Some(parts) = a.parts() {
                match parts.specific {
                    wce_audio_format::hirc::ActionSpecific::Play { bank_id, .. } => {
                        row("Loads bank", lib.bank_by_id(bank_id).map_or_else(|| format!("#{bank_id}"), |b| lib.banks[b].name.clone()));
                    }
                    wce_audio_format::hirc::ActionSpecific::SetState { group, value }
                    | wce_audio_format::hirc::ActionSpecific::SetSwitch { group, value } => {
                        row("Sets", format!("{} to {}", lib.label(group, 0), lib.label(value, 0)));
                    }
                    wce_audio_format::hirc::ActionSpecific::Other(_) => {}
                }
                // Its delay and fade time (seconds), editable.
                for (property, label) in [(14u8, "Delay"), (15, "Fade time")] {
                    let mut v = parts.props.iter().find(|(p, _)| *p == property).map_or(0.0, |(_, v)| *v);
                    gui::set_next_item_width(140.0);
                    gui::drag_float(&format!("{label}##ap{property}"), &mut v, 0.01, 0.0, 60.0, "%.2f s");
                    if gui::is_item_deactivated_after_edit() {
                        requests.push(Request::SetActionProperty(id, property, (v > 0.0).then_some(v)));
                    }
                }
            }
            let kind = lib.object(a.target_id).map_or(0, |(_, o, _)| o.kind);
            row("Target", lib.label(a.target_id, kind));
            if gui::small_button("Select the target") {
                requests.push(Request::Select(a.target_id));
            }
        }
        Object::Event(e) => {
            for a in &e.actions {
                if let Some((_, _, Some(Object::Action(action)))) = lib.object(*a) {
                    let target = lib.label(action.target_id, lib.object(action.target_id).map_or(0, |(_, o, _)| o.kind));
                    if gui::selectable(&format!("{} {target}##a{a}", action_name(action.action_type)), false, 0) {
                        requests.push(Request::Select(*a));
                    }
                }
            }
        }
        // Its mode, shuffle, repeat avoidance and weights are edited below (container_settings, the children).
        Object::RandomSequence(r) => row("Loop count", if r.loop_count == 0 { "infinite".into() } else { r.loop_count.to_string() }),
        Object::Switch(s) => {
            row(if s.group_type == 1 { "State group" } else { "Switch group" }, lib.label(s.group_id, 0));
            row("Default", lib.label(s.default_switch, 0));
            for (value, nodes) in &s.assignments {
                gui::text_disabled(&format!("  {}: {} object(s)", lib.label(*value, 0), nodes.len()));
            }
        }
        Object::DialogueEvent(d) => {
            let groups: Vec<String> = d.arguments.iter().map(|(g, _)| lib.label(*g, 0)).collect();
            row("Decides on", groups.join(", "));
            row("Tree nodes", d.nodes.len().to_string());
        }
        Object::MusicSegment(m) => {
            row("Duration", format!("{:.1} s", m.duration / 1000.0));
            row("Tempo", format!("{} bpm, {}/{}", m.base.meter.tempo, m.base.meter.beats_per_bar, m.base.meter.beat_value));
        }
        Object::MusicTrack(t) => {
            row("Sources", t.sources.len().to_string());
            row("Clips", t.clips.len().to_string());
        }
        _ => {}
    }
}

/// A music playlist: its items as a tree (groups and how they play, segments with their length), each segment with Play, Add
/// music after it, Remove, weight and loop; then its transitions and stingers.
fn playlist_view(
    id: u32,
    playlist: &MusicPlaylist,
    lib: &SoundLibrary,
    edits: &EditSet,
    play: &mut player_view::PlayButtons,
    requests: &mut Vec<Request>,
) {
    if !gui::collapsing_header(&format!("Music entries ({})###entries", playlist.items.len()), gui::TREE_DEFAULT_OPEN) {
        return;
    }
    for (depth, item) in item_tree(&playlist.items) {
        gui::push_id(&format!("it{}", item.item_id));
        gui::set_cursor_pos_x(gui::cursor_pos_x() + 14.0 * depth as f32);
        if item.children > 0 {
            gui::text_disabled(&format!(
                "{} of {}, loop {}",
                group_kind(item.kind),
                item.children,
                if item.loop_count == 0 { "forever".to_owned() } else { item.loop_count.to_string() }
            ));
        } else {
            let segment = edits.object(lib, item.segment_id);
            let length = match &segment {
                Some(Object::MusicSegment(s)) => format!("{:.1} s", s.duration / 1000.0),
                _ => "?".into(),
            };
            if play.button(&format!("seg{}", item.item_id), requests) {
                requests.push(Request::PlayObject(item.segment_id));
            }
            gui::same_line();
            if gui::small_button("Add after...") {
                requests.push(Request::AddMusic(id, item.item_id));
            }
            if gui::is_item_hovered() {
                gui::set_tooltip("Picks a WAV, Ogg Vorbis or WEM file and adds it as a new segment after this one, in the same group.");
            }
            gui::same_line();
            if gui::small_button("Remove") {
                requests.push(Request::RemoveEntry(id, item.item_id));
            }
            gui::same_line();
            let mut weight = item.weight as f32 / 1000.0;
            gui::set_next_item_width(90.0);
            gui::drag_float("##w", &mut weight, 1.0, 0.0, 100.0, "weight %.0f %%");
            if gui::is_item_deactivated_after_edit() {
                requests.push(Request::SetItem(id, item.item_id, (weight * 1000.0).round() as u32, item.loop_count));
            }
            gui::same_line();
            // Plays (loops) this many times in a row; 0 loops forever.
            let mut loops = i32::from(item.loop_count);
            gui::set_next_item_width(70.0);
            let format = if loops == 0 { "loop forever" } else { "%d x" };
            gui::drag_int_range("##loop", &mut loops, 0.2, 0, 99, format);
            if gui::is_item_hovered() {
                gui::set_tooltip("How many times this segment plays in a row (0: loops until the music changes).");
            }
            if gui::is_item_deactivated_after_edit() {
                requests.push(Request::SetItem(id, item.item_id, item.weight, loops.clamp(0, 99) as i16));
            }
            gui::same_line();
            if gui::selectable(&format!("{} ({length})##seg", lib.label(item.segment_id, 10)), false, 0) {
                requests.push(Request::Select(item.segment_id));
            }
        }
        gui::pop_id();
    }
}

/// The edits an object offers: replace a sound's audio, add a variation to a random container, its volume and pitch.
fn edit_fields(id: u32, object: &Object, requests: &mut Vec<Request>) {
    match object {
        Object::Sound(s) => {
            if gui::button("Replace audio...") {
                requests.push(Request::Replace(s.source.source_id));
            }
            if gui::is_item_hovered() {
                gui::set_tooltip(
                    "Picks a WAV, Ogg Vorbis or WEM file and encodes it as the game's sounds are (Wwise Vorbis).\nEvery sound using this audio plays the new one.",
                );
            }
        }
        Object::RandomSequence(_) => {
            if gui::button("Add variation...") {
                requests.push(Request::AddVariation(id));
            }
            if gui::is_item_hovered() {
                gui::set_tooltip(
                    "Picks a WAV, Ogg Vorbis or WEM file and adds it as one more sound of this container\n(set up like its first sound).",
                );
            }
        }
        Object::MusicSegment(_) => {
            if gui::button("Replace music...") {
                requests.push(Request::ReplaceSegment(id));
            }
            if gui::is_item_hovered() {
                gui::set_tooltip("Picks a WAV, Ogg Vorbis or WEM file for this segment; its length and exit cue follow the new music.");
            }
        }
        _ => {}
    }

    let Some(node) = object.node() else { return };
    for (property, label, lo, hi, format) in [(0u8, "Volume", -96.0, 12.0, "%.1f dB"), (2, "Pitch", -2400.0, 2400.0, "%.0f cents")] {
        let mut value = node.props.iter().find(|(p, _)| *p == property).map_or(0.0, |(_, v)| *v);
        gui::set_next_item_width(160.0);
        gui::drag_float(&format!("{label}##p{property}"), &mut value, if property == 0 { 0.1 } else { 10.0 }, lo, hi, format);
        if gui::is_item_deactivated_after_edit() {
            requests.push(Request::SetProperty(id, property, Some(value)));
        }
    }
}

/// The node parameters: parent, bus, properties.
fn node_fields(node: &NodeBase, lib: &SoundLibrary, requests: &mut Vec<Request>) {
    if node.parent_id != 0 {
        let kind = lib.object(node.parent_id).map_or(0, |(_, o, _)| o.kind);
        if gui::selectable(&format!("Parent: {}##parent", lib.label(node.parent_id, kind)), false, 0) {
            requests.push(Request::Select(node.parent_id));
        }
    }
    if node.override_bus_id != 0 {
        gui::text_disabled(&format!("Output bus: {}", lib.label(node.override_bus_id, 8)));
    }
    for (id, value) in &node.props {
        gui::text_disabled(&format!("{}: {value}", property_name(*id)));
    }
    for (id, min, max) in &node.ranged_props {
        gui::text_disabled(&format!("{}: random {min} to {max}", property_name(*id)));
    }
}

/// Property IDs (bank version 112). Only the ones checked are named; the others show their number (docs/plans/audio.md, step 4).
const SYNCS: [&str; 10] = [
    "immediately",
    "next grid",
    "next bar",
    "next beat",
    "next cue",
    "next custom cue",
    "entry cue",
    "exit cue",
    "never",
    "last exit position",
];
const ENTRIES: [&str; 5] = ["entry cue", "same time as the source", "random cue", "random custom cue", "last exit position"];
const CURVES: [&str; 10] = ["log 3", "sine", "log 1", "inverted S", "linear", "S", "exp 1", "sine reciprocal", "exp 3", "constant"];

fn pick(label: &str, value: &mut u32, names: &[&str]) -> bool {
    let mut i = *value as i32;
    gui::set_next_item_width(130.0);
    let changed = gui::combo(label, &mut i, names);
    if changed {
        *value = i.max(0) as u32;
    }
    changed
}

/// The objects a rule names: "any" for -1, "nothing" for 0.
fn rule_ends(ids: &[u32], lib: &SoundLibrary) -> String {
    let names: Vec<String> = ids
        .iter()
        .map(|i| match *i {
            u32::MAX => "any".to_owned(),
            0 => "nothing".to_owned(),
            i => lib.label(i, lib.object(i).map_or(0, |(_, o, _)| o.kind)),
        })
        .collect();
    names.join(", ")
}

/// A music switch's or playlist's transition rules: how it leaves a source (when, fade out) and enters a destination (where,
/// fade in), and the segment played between, each editable.
fn transitions_view(id: u32, rules: &[wce_audio_format::hirc::TransitionRule], lib: &SoundLibrary, requests: &mut Vec<Request>) {
    if rules.is_empty() || !gui::collapsing_header(&format!("Transitions ({})", rules.len()), 0) {
        return;
    }
    for (i, rule) in rules.iter().enumerate() {
        gui::push_id(&format!("rule{i}"));
        gui::text_wrapped(&format!("From {} to {}", rule_ends(&rule.sources, lib), rule_ends(&rule.destinations, lib)));
        let mut r = rule.clone();
        let mut changed = false;
        gui::text_disabled("  leave at");
        gui::same_line();
        changed |= pick("##sync", &mut r.source_rule.sync, &SYNCS);
        gui::same_line();
        let mut out = r.source_rule.fade.time;
        gui::set_next_item_width(110.0);
        gui::drag_int_range("##fo", &mut out, 10.0, 0, 60_000, "fade out %d ms");
        if gui::is_item_deactivated_after_edit() {
            r.source_rule.fade.time = out;
            changed = true;
        }
        gui::same_line();
        changed |= pick("##foc", &mut r.source_rule.fade.curve, &CURVES);
        gui::text_disabled("  enter at");
        gui::same_line();
        let mut entry = u32::from(r.destination_rule.entry);
        if pick("##entry", &mut entry, &ENTRIES) {
            r.destination_rule.entry = entry as u16;
            changed = true;
        }
        gui::same_line();
        let mut fin = r.destination_rule.fade.time;
        gui::set_next_item_width(110.0);
        gui::drag_int_range("##fi", &mut fin, 10.0, 0, 60_000, "fade in %d ms");
        if gui::is_item_deactivated_after_edit() {
            r.destination_rule.fade.time = fin;
            changed = true;
        }
        gui::same_line();
        changed |= pick("##fic", &mut r.destination_rule.fade.curve, &CURVES);
        if let Some(o) = &r.transition_object {
            gui::text_disabled(&format!("  through {}", lib.label(o.segment_id, 10)));
            gui::same_line();
            if gui::small_button("Select##seg") {
                requests.push(Request::Select(o.segment_id));
            }
        }
        if changed {
            requests.push(Request::SetRule(id, i, r));
        }
        gui::separator();
        gui::pop_id();
    }
}

/// A music object's stingers (a trigger plays a segment over the music): when, how often, the segment; Remove; Add.
fn stingers_view(
    id: u32,
    stingers: &[wce_audio_format::hirc::Stinger],
    lib: &SoundLibrary,
    fields: &mut (String, String),
    requests: &mut Vec<Request>,
) {
    if !gui::collapsing_header(&format!("Stingers ({})", stingers.len()), 0) {
        return;
    }
    gui::text_wrapped("A trigger (a game event, by name) plays a segment over the music, in time with it.");
    for (i, st) in stingers.iter().enumerate() {
        gui::push_id(&format!("st{i}"));
        let mut s = *st;
        let mut changed = false;
        gui::text(&format!("{} plays {}", lib.label(s.trigger_id, 0), lib.label(s.segment_id, 10)));
        gui::text_disabled("  at");
        gui::same_line();
        changed |= pick("##at", &mut s.sync_play_at, &SYNCS);
        gui::same_line();
        let mut gap = s.dont_repeat_time;
        gui::set_next_item_width(150.0);
        gui::drag_int_range("##gap", &mut gap, 50.0, 0, 600_000, "not again for %d ms");
        if gui::is_item_deactivated_after_edit() {
            s.dont_repeat_time = gap;
            changed = true;
        }
        gui::same_line();
        if gui::small_button("Remove") {
            let mut list = stingers.to_vec();
            list.remove(i);
            requests.push(Request::SetStingers(id, list));
        } else if changed {
            let mut list = stingers.to_vec();
            list[i] = s;
            requests.push(Request::SetStingers(id, list));
        }
        gui::pop_id();
    }
    gui::set_next_item_width(160.0);
    gui::input_text("##strig", Some("trigger name"), &mut fields.0, 128, 0);
    gui::same_line();
    gui::set_next_item_width(160.0);
    gui::input_text("##sseg", Some("segment (name or #ID)"), &mut fields.1, 128, 0);
    gui::same_line();
    let token = fields.1.trim().trim_start_matches('#');
    let segment = token.parse::<u32>().ok().or_else(|| (!token.is_empty()).then(|| wce_audio_format::fnv::id(token)));
    let segment = segment.filter(|s| lib.object(*s).is_some_and(|(_, o, _)| o.kind == 10));
    gui::begin_disabled(fields.0.trim().is_empty() || segment.is_none());
    if gui::small_button("Add a stinger")
        && let Some(segment) = segment
    {
        let mut list = stingers.to_vec();
        list.push(wce_audio_format::hirc::Stinger {
            trigger_id: wce_audio_format::fnv::id(fields.0.trim()),
            segment_id: segment,
            sync_play_at: 2,
            cue_filter_hash: 0,
            dont_repeat_time: 0,
            segment_look_ahead: 0,
        });
        requests.push(Request::SetStingers(id, list));
        *fields = (String::new(), String::new());
    }
    gui::end_disabled();
}

/// A music switch's paths of states and what each plays, each one settable to another music object (a name or #ID).
fn music_switch_view(
    id: u32,
    switch: &wce_audio_format::hirc::MusicSwitch,
    lib: &SoundLibrary,
    field: &mut (usize, String),
    requests: &mut Vec<Request>,
) {
    let depth = switch.arguments.len();
    let paths = wce_audio_format::hirc::tree_paths(depth, &switch.nodes);
    let groups: Vec<String> = switch.arguments.iter().map(|(g, _)| lib.label(*g, 0)).collect();
    if !gui::collapsing_header(&format!("What it plays ({} paths of {})", paths.len(), groups.join(" / ")), 0) {
        return;
    }
    gui::text_wrapped(
        "Each path of states (any: the state does not matter) and the music it plays. Type a playlist's name or #ID to play it instead.",
    );
    for (i, (keys, target)) in paths.iter().enumerate() {
        gui::push_id(&format!("mp{i}"));
        let states: Vec<String> = keys.iter().map(|k| if *k == 0 { "any".to_owned() } else { lib.label(*k, 0) }).collect();
        let kind = lib.object(*target).map_or(0, |(_, o, _)| o.kind);
        if gui::selectable(&format!("{}  ->  {}", states.join(" / "), lib.label(*target, kind)), false, 0) {
            requests.push(Request::Select(*target));
        }
        if gui::is_item_hovered() {
            gui::set_tooltip("Selects what it plays.");
        }
        if lib.object(*target).is_some_and(|(_, o, _)| o.kind == 13) {
            gui::same_line();
            if gui::small_button("New music...") {
                requests.push(Request::PathMusic(id, keys.clone(), *target));
            }
            if gui::is_item_hovered() {
                gui::set_tooltip("Picks a sound file: a new playlist of it (set up like this one) plays for these states instead.");
            }
        }
        gui::same_line();
        if field.0 != i {
            gui::set_next_item_width(160.0);
            let mut text = String::new();
            if gui::input_text("##to", Some("play instead..."), &mut text, 128, 0) {
                *field = (i, text);
            }
        } else {
            gui::set_next_item_width(160.0);
            gui::input_text("##to", Some("play instead..."), &mut field.1, 128, 0);
            gui::same_line();
            let token = field.1.trim().trim_start_matches('#');
            let new = token.parse::<u32>().ok().filter(|t| lib.has_object(*t)).or_else(|| {
                let h = wce_audio_format::fnv::id(token);
                lib.has_object(h).then_some(h)
            });
            gui::begin_disabled(new.is_none());
            if gui::small_button("Set")
                && let Some(new) = new
            {
                requests.push(Request::SetMusicPath(id, keys.clone(), new));
                *field = (usize::MAX, String::new());
            }
            gui::end_disabled();
        }
        gui::pop_id();
    }
}

/// A random/sequence container's play settings: random or sequence, shuffle, how many recent takes to avoid.
fn container_settings(id: u32, r: &wce_audio_format::hirc::RandomSequence, requests: &mut Vec<Request>) {
    let mut random = r.mode == 1;
    if gui::checkbox("Random (else in sequence)", &mut random) {
        requests.push(Request::SetContainer(id, u8::from(random), r.random_mode, r.avoid_repeat_count, None));
    }
    if random {
        gui::same_line();
        let mut shuffle = r.random_mode == 1;
        if gui::checkbox("Shuffle", &mut shuffle) {
            requests.push(Request::SetContainer(id, r.mode, u8::from(shuffle), r.avoid_repeat_count, None));
        }
        if gui::is_item_hovered() {
            gui::set_tooltip("Plays every take once before any again.");
        }
        gui::same_line();
        let mut avoid = i32::from(r.avoid_repeat_count);
        gui::set_next_item_width(80.0);
        let most = r.children.len().saturating_sub(1) as i32;
        gui::drag_int_range("Avoid last##avoid", &mut avoid, 0.1, 0, most, "%d");
        if gui::is_item_hovered() {
            gui::set_tooltip("How many of the last takes played are not picked again.");
        }
        if gui::is_item_deactivated_after_edit() {
            requests.push(Request::SetContainer(id, r.mode, r.random_mode, avoid.clamp(0, most) as u16, None));
        }
    }
}

/// A node property's name: Wwise 2014's property IDs (bank version 112, as wwiser lists them; volume, pitch and low-pass checked
/// in the oracle's banks).
fn property_name(id: u8) -> String {
    const NAMES: [&str; 33] = [
        "Volume (dB)",
        "LFE volume (dB)",
        "Pitch (cents)",
        "Low-pass",
        "Bus volume (dB)",
        "Priority",
        "Priority distance offset",
        "Loop count",
        "Feedback volume",
        "Feedback low-pass",
        "Mute ratio",
        "Pan left-right",
        "Pan front-rear",
        "Centre %",
        "Initial delay (s)",
        "Transition time",
        "Probability",
        "Dialogue mode",
        "User aux send 0 (dB)",
        "User aux send 1 (dB)",
        "User aux send 2 (dB)",
        "User aux send 3 (dB)",
        "Game aux send (dB)",
        "Output bus volume (dB)",
        "Output bus low-pass",
        "HDR bus threshold",
        "HDR bus ratio",
        "HDR bus release time",
        "HDR bus game parameter",
        "HDR bus parameter min",
        "HDR bus parameter max",
        "HDR active range",
        "Make-up gain (dB)",
    ];
    NAMES.get(usize::from(id)).map_or_else(|| format!("Property {id}"), |n| (*n).to_owned())
}

impl Tool for AudioTool {
    fn id(&self) -> &str {
        "audio"
    }

    fn title(&self) -> &str {
        "Audio"
    }

    fn icon(&self) -> &str {
        wce_editor::icons::AUDIO_LINES
    }

    fn inspect(&mut self, e: &mut Editor, _: &mut dyn wce_editor_core::inspector::Inspector) -> bool {
        let mut requests = vec![];
        self.inspector(&mut requests);
        self.handle(e, requests);
        true
    }

    fn shows_viewport(&self) -> bool {
        false
    }

    fn default_layout(&self, dockspace: gui::ImGuiID) {
        let (left, center) = gui::dock_builder_split(dockspace, gui::DIR_LEFT, 0.22);
        let (right, center) = gui::dock_builder_split(center, gui::DIR_RIGHT, 0.30);
        // The Inspector (the player, the sound's info) above Audio Object.
        let (right, inspector) = gui::dock_builder_split(right, gui::DIR_DOWN, 0.58);
        let (bottom, center) = gui::dock_builder_split(center, gui::DIR_DOWN, 0.22);
        gui::dock_builder_dock_window("Audio Banks", left);
        gui::dock_builder_dock_window("Audio Search", left);
        gui::dock_builder_dock_window("Audio Bank", center);
        gui::dock_builder_dock_window("Audio Object", right);
        gui::dock_builder_dock_window("Audio Game Data", center);
        gui::dock_builder_dock_window("Inspector", inspector);
        gui::dock_builder_dock_window("Assets", bottom);
        gui::dock_builder_dock_window("Log", bottom);
        gui::dock_builder_dock_window("History", bottom);
    }

    fn register_commands(&self, c: &mut CommandRegistry<Editor>) {
        type Slot = fn(&mut AudioTool) -> &mut Option<String>;
        let commands: [(&str, &str, &str, Slot); 7] = [
            ("audio_bank", "audio_bank <bank name>", "Shows a bank in Audio Bank", |t| &mut t.pending.0),
            ("audio_search", "audio_search <text>", "Searches every bank's names (Audio Search)", |t| &mut t.pending.1),
            ("audio_select", "audio_select <name | id>", "Selects a sound object (Audio Object) and shows its bank", |t| &mut t.pending.2),
            ("audio_expand", "audio_expand <event name | id>", "Selects an event and opens what it plays", |t| &mut t.pending.3),
            ("audio_animation", "audio_animation <part of a .snd.meta path>", "Shows an animation's sound frames (Audio Game Data)", |t| {
                &mut t.pending_animation
            }),
            ("audio_show", "audio_show <panel title>", "Brings an Audio panel to the front (its dock tab)", |t| &mut t.pending_panel),
            ("audio_volume", "audio_volume <object id> <dB>", "Sets an object's volume (an edit of the Audio edits document)", |t| {
                &mut t.pending_volume
            }),
        ];
        for (name, usage, help, slot) in commands {
            c.add(name, usage, help, move |e, args| {
                let text = args.join(" ");
                if text.is_empty() {
                    return CommandResult::fail(format!("usage: {usage}"));
                }
                match e.with_tool::<AudioTool, _>(|t, _| *slot(t) = Some(text.clone())) {
                    Some(()) => CommandResult::success(format!("{name} {text}")).wait(2),
                    None => CommandResult::fail("the Audio tool is not built in"),
                }
            });
        }
    }

    fn draw_panels(&mut self, e: &mut Editor) {
        self.poll(e);
        let mut requests = vec![];
        if self.library.is_some()
            && let Some(text) = self.pending_volume.take()
        {
            let mut words = text.split_whitespace();
            match (
                words.next().and_then(|w| w.trim_start_matches('#').parse::<u32>().ok()),
                words.next().and_then(|w| w.parse::<f32>().ok()),
            ) {
                (Some(id), Some(db)) => requests.push(Request::SetProperty(id, 0, Some(db))),
                _ => e.status(&format!("audio_volume: \"{text}\" is not <object id> <dB>"), true),
            }
        }
        self.banks_panel(&mut requests);
        self.search_panel(&mut requests);
        self.bank_panel(&mut requests);
        self.object_panel(&mut requests);
        self.game_data_panel(&mut requests);
        if self.library.is_some()
            && let Some(panel) = self.pending_panel.take()
        {
            gui::select_dock_tab(&panel);
        }
        self.handle(e, requests);
    }

    /// The edits are the Audio edits document's: the editor asks about it as about any document.
    fn unsaved_work(&self) -> Vec<String> {
        vec![]
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
