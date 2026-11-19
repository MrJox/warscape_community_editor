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
use wce_audio_format::bank::kind_name;
use wce_audio_format::hirc::{NodeBase, Object};
use wce_editor::editor::Editor;
use wce_editor::gui::{self, v2};
use wce_editor::tool::Tool;
use wce_editor_core::command_registry::{CommandRegistry, CommandResult};
use wce_vfs::files::Files;

use crate::player::Player;

/// What a click asked for; done after the panel is drawn.
enum Request {
    Play(MediaRef, String),
    PlayAny(Arc<PlayNode>),
    Select(u32),
    Stop,
    /// Replace a media file's audio with a sound file the user picks.
    Replace(u32),
    /// Add a sound file the user picks to a random/sequence container.
    AddVariation(u32),
    RemoveVariation(u32, u32),
    SetProperty(u32, u8, Option<f32>),
    Save,
    Revert,
    Undo,
    Redo,
}

/// What an encoded import goes to.
#[derive(Clone, Copy)]
enum ImportTarget {
    Replace(u32),
    Add(u32),
}

/// The undo steps kept (each a copy of the edited banks).
const UNDO_STEPS: usize = 20;

/// A bank's lists, made once.
#[derive(Default)]
struct BankLists {
    events: Vec<(String, u32)>,
    dialogue: Vec<(String, u32)>,
    music: Vec<(String, u32)>,
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
    search: String,
    searched: String,
    results: Vec<(String, u32, u8, usize)>,
    selected: Option<u32>,
    trees: HashMap<u32, Arc<PlayNode>>,
    decoding: Option<(String, Receiver<Result<Pcm, String>>)>,
    player: Player,
    rng: u64,
    /// The tree node opened by `audio_expand`, and whether the list still has to scroll to it.
    expand: Option<u32>,
    scroll_to_expanded: bool,
    /// What the script commands asked for, applied once the banks are read: (bank, search, select, expand).
    pending: (Option<String>, Option<String>, Option<String>, Option<String>),
    /// Unsaved edits, and the undo and redo steps.
    edits: EditSet,
    undo: Vec<EditSet>,
    redo: Vec<EditSet>,
    /// The import quality (Wwise's 0 to 10), `None` for the default.
    quality: Option<u8>,
    /// A sound file being encoded on a worker: its name, the result, and what it goes to.
    encoding: Option<(String, Receiver<Result<Vec<u8>, String>>, ImportTarget)>,
}

impl AudioTool {
    fn poll(&mut self, e: &mut Editor) {
        if let Some(files) = e.files()
            && files.mount_id() != self.mount_id
            && self.loading.is_none()
        {
            self.mount_id = files.mount_id();
            self.files = Some(files.clone());
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let _ = tx.send(SoundLibrary::load(files.as_ref()));
            });
            self.loading = Some(rx);
        }

        if let Some(rx) = &self.loading
            && let Ok(lib) = rx.try_recv()
        {
            for (path, why) in &lib.failures {
                e.status(&format!("{path}: {why}"), true);
            }
            self.library = Some(Arc::new(lib));
            self.loading = None;
            self.lists.clear();
            self.trees.clear();
            self.results.clear();
            self.searched.clear();
        }

        if let Some(lib) = self.library.clone() {
            self.apply_pending(&lib, e);
        }

        if let Some((name, rx, target)) = &self.encoding
            && let Ok(result) = rx.try_recv()
        {
            let (name, target) = (name.clone(), *target);
            self.encoding = None;
            match result {
                Ok(wem) => match target {
                    ImportTarget::Replace(media) => self.edit(e, &format!("replaced #{media} with {name}"), |edits, lib, files| {
                        edits.replace_media(lib, files, media, &wem).map(|()| String::new())
                    }),
                    ImportTarget::Add(container) => self.edit(e, &format!("added {name}"), |edits, lib, _| {
                        edits.add_variation(lib, container, &wem).map(|id| format!(" as #{id}"))
                    }),
                },
                Err(why) => e.status(&format!("Audio: cannot import {name}: {why}"), true),
            }
        }

        if let Some((label, rx)) = &self.decoding
            && let Ok(result) = rx.try_recv()
        {
            let label = label.clone();
            self.decoding = None;
            match result.and_then(|pcm| self.player.play(&pcm, &label)) {
                Ok(()) => {}
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
        for (token, expand) in [(self.pending.2.take(), false), (self.pending.3.take(), true)] {
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
                Request::Stop => self.player.stop(),
                Request::Replace(media) => self.import(e, ImportTarget::Replace(media)),
                Request::AddVariation(container) => self.import(e, ImportTarget::Add(container)),
                Request::RemoveVariation(container, child) => self
                    .edit(e, "removed a variation", |edits, lib, _| edits.remove_variation(lib, container, child).map(|()| String::new())),
                Request::SetProperty(id, property, value) => {
                    self.edit(e, "set a property", |edits, lib, _| edits.set_property(lib, id, property, value).map(|()| String::new()))
                }
                Request::Save => self.save(e),
                Request::Revert => {
                    if !self.edits.is_empty() {
                        self.undo.push(std::mem::take(&mut self.edits));
                        e.status("Audio: unsaved edits reverted (Undo brings them back)", false);
                    }
                }
                Request::Undo => {
                    if let Some(previous) = self.undo.pop() {
                        self.redo.push(std::mem::replace(&mut self.edits, previous));
                    }
                }
                Request::Redo => {
                    if let Some(next) = self.redo.pop() {
                        self.undo.push(std::mem::replace(&mut self.edits, next));
                    }
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
                self.undo.push(std::mem::replace(&mut self.edits, next));
                if self.undo.len() > UNDO_STEPS {
                    self.undo.remove(0);
                }
                self.redo.clear();
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
        let Some(lib) = self.library.clone() else { return };
        let mut written = vec![];
        let files: Vec<(String, Vec<u8>)> = self
            .edits
            .banks
            .iter()
            .map(|(b, bank)| (lib.banks[*b].path.clone(), bank.write()))
            .chain(self.edits.loose.iter().map(|(p, bytes)| (p.clone(), bytes.clone())))
            .collect();
        for (path, bytes) in files {
            let disk = e.loose_path(&path);
            if let Some(dir) = disk.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            match wce_foundation::files::write_replacing(&disk, &bytes) {
                Ok(()) => written.push(path),
                Err(err) => {
                    e.status(&format!("Audio: cannot write {}: {err}", disk.display()), true);
                    return;
                }
            }
        }
        for path in &written {
            e.loose_changed(path);
        }
        self.edits = EditSet::default();
        self.undo.clear();
        self.redo.clear();
        e.status(&format!("Audio: saved {} files into the project's loose folder", written.len()), false);
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
        self.decoding = Some((label.to_owned(), rx));
    }

    fn tree(&mut self, id: u32) -> Option<Arc<PlayNode>> {
        if let Some(t) = self.trees.get(&id) {
            return Some(t.clone());
        }

        let (lib, files) = (self.library.as_ref()?, self.files.as_ref()?);
        let tree = Arc::new(lib.resolve(id, files.as_ref()));
        self.trees.insert(id, tree.clone());
        Some(tree)
    }

    fn lists(&mut self, bank: usize) -> Option<&BankLists> {
        let lib = self.library.clone()?;
        let entry = lib.banks.get(bank)?;
        Some(self.lists.entry(bank).or_insert_with(|| {
            let mut l = BankLists::default();
            for (i, o) in entry.bank.objects().iter().enumerate() {
                let label = lib.label(o.id, o.kind);
                match o.kind {
                    4 => l.events.push((label.clone(), o.id)),
                    15 => l.dialogue.push((label.clone(), o.id)),
                    12 | 13 if !has_music_parent(entry.typed[i].as_ref()) => l.music.push((label.clone(), o.id)),
                    _ => {}
                }
                l.all.push((label, o.id, o.kind));
            }
            for v in [&mut l.events, &mut l.dialogue, &mut l.music] {
                v.sort_by_key(|(n, _)| n.to_lowercase());
            }
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

        // The player.
        if let Some((label, _)) = &self.decoding {
            gui::text_disabled(&format!("Decoding {label}..."));
        } else if let Some(p) = self.player.progress() {
            if gui::small_button("Stop") {
                requests.push(Request::Stop);
            }
            gui::same_line();
            gui::progress_bar(p, -1.0, &format!("{} ({:.1} s)", self.player.label, self.player.seconds));
        } else {
            gui::text_disabled("Nothing playing.");
        }
        gui::separator();

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
        if !self.undo.is_empty() {
            gui::same_line();
            if gui::small_button("Undo") {
                requests.push(Request::Undo);
            }
        }
        if !self.redo.is_empty() {
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
        let pick = |v: &Vec<(String, u32)>| -> Vec<(String, u32)> {
            v.iter().filter(|(n, _)| filter.is_empty() || n.to_lowercase().contains(&filter)).cloned().collect()
        };
        let (events, dialogue, music) = (pick(&lists.events), pick(&lists.dialogue), pick(&lists.music));
        let all: Vec<(String, u32, u8)> =
            lists.all.iter().filter(|(n, _, _)| filter.is_empty() || n.to_lowercase().contains(&filter)).cloned().collect();

        if gui::begin_tab_bar("bank tabs") {
            for (tab, items) in [("Events", &events), ("Dialogue events", &dialogue), ("Music", &music)] {
                if gui::begin_tab_item(&format!("{tab} ({})###{tab}", items.len()), 0) {
                    if gui::begin_child(tab, v2(0.0, 0.0), 0, 0) {
                        for (label, id) in items {
                            self.event_row(label, *id, requests);
                        }
                    }
                    gui::end_child();
                    gui::end_tab_item();
                }
            }

            let media = entry.bank.media_index();
            if gui::begin_tab_item(&format!("Sounds in the bank ({})###media", media.len()), 0) {
                if gui::begin_child("media", v2(0.0, 0.0), 0, 0) {
                    gui::clipped(media.len(), |row| {
                        let m = media[row];
                        if gui::small_button(&format!("Play##m{}", m.id)) {
                            requests.push(Request::Play(MediaRef::InBank { bank, id: m.id }, format!("#{}", m.id)));
                        }
                        gui::same_line();
                        gui::text(&format!("#{}  {:.1} KB", m.id, f64::from(m.size) / 1024.0));
                    });
                }
                gui::end_child();
                gui::end_tab_item();
            }

            if gui::begin_tab_item(&format!("All objects ({})###all", all.len()), 0) {
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
        if gui::small_button(&format!("Play##p{id}"))
            && let Some(tree) = self.tree(id)
        {
            requests.push(Request::PlayAny(tree));
        }
        if gui::is_item_hovered() {
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
            if gui::small_button(&format!("Play##s{key}")) {
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
        let Some((_, raw, typed)) = lib.object(id) else {
            gui::text_disabled(&format!("#{id} is in no bank."));
            gui::end();
            return;
        };

        gui::text(&lib.label(id, raw.kind));
        gui::text_disabled(&format!("{}  #{id}  ({} bytes)", kind_name(raw.kind), raw.body.len() + 4));
        if let Some(source) = lib.names.source(id) {
            gui::text_disabled(&format!("Name from {}", source.describe()));
        }
        let banks: Vec<&str> = lib.banks_of(id).iter().map(|b| lib.banks[*b].name.as_str()).collect();
        gui::text_wrapped(&format!("In {}: {}", if banks.len() == 1 { "bank" } else { "banks" }, banks.join(", ")));
        if gui::button("Play")
            && let Some(tree) = self.tree(id)
        {
            requests.push(Request::PlayAny(tree));
        }
        gui::separator();

        // The object as edited (an unsaved edit shows here at once).
        let edited = self.edits.object(&lib, id);
        if edited.is_some() && self.edits.banks.keys().any(|b| lib.banks_of(id).contains(b)) {
            gui::text_colored([1.0, 0.8, 0.3, 1.0], "Edited, not saved");
        }
        match edited.as_ref().or(typed) {
            None => gui::text_disabled("Not read field by field (this bank version or kind is kept as bytes)."),
            Some(object) => {
                fields(object, &lib, requests);
                edit_fields(id, object, requests);
                if let Some(node) = object.node() {
                    node_fields(node, &lib, requests);
                }
                let random = matches!(object, Object::RandomSequence(_));
                if !object.children().is_empty() && gui::collapsing_header(&format!("Children ({})", object.children().len()), 0) {
                    for c in object.children() {
                        let kind = self.edits.object(&lib, *c).map_or_else(|| lib.object(*c).map_or(0, |(_, o, _)| o.kind), |o| o.kind());
                        if random {
                            if gui::small_button(&format!("Remove##rm{c}")) {
                                requests.push(Request::RemoveVariation(id, *c));
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

        gui::end();
    }
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
fn fields(object: &Object, lib: &SoundLibrary, requests: &mut Vec<Request>) {
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
        Object::RandomSequence(r) => {
            row("Mode", if r.mode == 1 { "random".into() } else { "sequence".into() });
            row("Loop count", if r.loop_count == 0 { "infinite".into() } else { r.loop_count.to_string() });
            row("Avoid repeating the last", r.avoid_repeat_count.to_string());
            for (child, weight) in &r.playlist {
                gui::text_disabled(&format!("  #{child}: weight {:.0} %", f64::from(*weight) / 1000.0));
            }
        }
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
fn property_name(id: u8) -> String {
    match id {
        0 => "Volume (dB)".into(),
        2 => "Pitch (cents)".into(),
        3 => "Low-pass".into(),
        _ => format!("Property {id}"),
    }
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

    fn shows_viewport(&self) -> bool {
        false
    }

    fn default_layout(&self, dockspace: gui::ImGuiID) {
        let (left, center) = gui::dock_builder_split(dockspace, gui::DIR_LEFT, 0.22);
        let (right, center) = gui::dock_builder_split(center, gui::DIR_RIGHT, 0.30);
        let (bottom, center) = gui::dock_builder_split(center, gui::DIR_DOWN, 0.22);
        gui::dock_builder_dock_window("Audio Banks", left);
        gui::dock_builder_dock_window("Audio Search", left);
        gui::dock_builder_dock_window("Audio Bank", center);
        gui::dock_builder_dock_window("Audio Object", right);
        gui::dock_builder_dock_window("Inspector", right);
        gui::dock_builder_dock_window("Assets", bottom);
        gui::dock_builder_dock_window("Log", bottom);
        gui::dock_builder_dock_window("History", bottom);
    }

    fn register_commands(&self, c: &mut CommandRegistry<Editor>) {
        type Slot = fn(&mut AudioTool) -> &mut Option<String>;
        let commands: [(&str, &str, &str, Slot); 4] = [
            ("audio_bank", "audio_bank <bank name>", "Shows a bank in Audio Bank", |t| &mut t.pending.0),
            ("audio_search", "audio_search <text>", "Searches every bank's names (Audio Search)", |t| &mut t.pending.1),
            ("audio_select", "audio_select <name | id>", "Selects a sound object (Audio Object) and shows its bank", |t| &mut t.pending.2),
            ("audio_expand", "audio_expand <event name | id>", "Selects an event and opens what it plays", |t| &mut t.pending.3),
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
        self.banks_panel(&mut requests);
        self.search_panel(&mut requests);
        self.bank_panel(&mut requests);
        self.object_panel(&mut requests);
        self.handle(e, requests);
    }

    fn unsaved_work(&self) -> Vec<String> {
        if self.edits.is_empty() {
            vec![]
        } else {
            vec![format!("Audio: {} bank(s) and {} sound file(s) edited, not saved", self.edits.banks.len(), self.edits.loose.len())]
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
