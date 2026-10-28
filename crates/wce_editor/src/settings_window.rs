//! The Settings window (File > Settings..., command `settings`): every setting of the file on pages, edited on a copy that OK and Apply put in
//! place and write; Cancel drops it. The appearance shows live while it is edited. Tools add their own pages (`Tool::draw_settings`).

use std::path::{Path, PathBuf};

use wce_foundation::log;
use wce_foundation::settings::{Settings, env_value};

use crate::editor::Editor;
use crate::steam_detect::{detect_game_folder, pick_folder};
use crate::{gui, icons, theme};

/// A page of the window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Page {
    #[default]
    General,
    Game,
    Appearance,
    Folders,

    /// A tool's page, by its index.
    Tool(usize),
}

#[derive(Default)]
pub(crate) struct SettingsWindow {
    /// The copy being edited; the window is open while there is one.
    pub(crate) draft: Option<Settings>,

    /// The settings as the window found them: a tool's table the window did not change keeps what the tool wrote meanwhile.
    opened: Option<Settings>,
    page: Page,

    /// Dear ImGui's style editor, to work on a theme.
    style_editor: bool,
}

/// The labels' column.
pub const LABEL_WIDTH: f32 = 150.0;

/// A row's label in the labels' column, the field after it.
pub fn label(text: &str) {
    gui::align_text_to_frame_padding();
    gui::text(text);
    gui::same_line_at(LABEL_WIDTH);
}

/// A note under a field: the environment variable that overrides it, if set.
pub fn env_note(var: &str) {
    if let Some(value) = env_value(var) {
        gui::set_cursor_pos_x(LABEL_WIDTH);
        gui::text_colored(theme::warning(), &format!("{}  ${var} overrides this: {value}", icons::TRIANGLE_ALERT));
    }
}

/// A help text under a field.
pub fn help(text: &str) {
    gui::set_cursor_pos_x(LABEL_WIDTH);
    gui::push_text_wrap_pos(0.0);
    gui::text_colored(theme::dim(), text);
    gui::pop_text_wrap_pos();
}

/// A labelled folder field with a Browse button; empty is none. True when it changed.
pub fn folder_field(label_text: &str, value: &mut Option<PathBuf>, hint: &str) -> bool {
    label(label_text);
    let mut text = value.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
    gui::set_next_item_width(-gui::frame_height_with_spacing() * 2.0 - 70.0);
    let mut changed = gui::input_text(&format!("##{label_text}"), Some(hint), &mut text, 1024, 0);
    gui::same_line();
    if gui::button(&format!("{}  Browse##{label_text}", icons::FOLDER_OPEN))
        && let Some(p) = pick_folder(label_text)
    {
        text = p.display().to_string();
        changed = true;
    }

    gui::same_line();
    gui::begin_disabled(text.is_empty());
    if gui::button(&format!("{}##clear{label_text}", icons::X)) {
        text.clear();
        changed = true;
    }

    gui::end_disabled();
    if changed {
        *value = (!text.trim().is_empty()).then(|| PathBuf::from(text.trim()));
    }

    changed
}

/// A list of folders: each one editable, movable and removable, and an Add button. True when it changed.
pub fn folder_list(id: &str, folders: &mut Vec<PathBuf>) -> bool {
    let mut changed = false;
    let mut remove = None;
    let mut swap = None;
    gui::push_id(id);
    for (i, folder) in folders.iter_mut().enumerate() {
        gui::push_id_int(i as i32);
        gui::set_cursor_pos_x(LABEL_WIDTH);
        let mut text = folder.display().to_string();
        gui::set_next_item_width(-gui::frame_height_with_spacing() * 3.0);
        if gui::input_text("##path", None, &mut text, 1024, 0) {
            *folder = PathBuf::from(text);
            changed = true;
        }

        gui::same_line();
        gui::begin_disabled(i == 0);
        if gui::button(icons::ARROW_UP) {
            swap = Some(i);
        }

        gui::end_disabled();
        gui::same_line();
        if gui::button(icons::X) {
            remove = Some(i);
        }

        gui::pop_id();
    }

    if let Some(i) = swap {
        folders.swap(i - 1, i);
        changed = true;
    }

    if let Some(i) = remove {
        folders.remove(i);
        changed = true;
    }

    gui::set_cursor_pos_x(LABEL_WIDTH);
    if gui::button(&format!("{}  Add folder...", icons::FOLDER_PLUS))
        && let Some(p) = pick_folder("Add a folder")
    {
        folders.push(p);
        changed = true;
    }

    gui::pop_id();
    changed
}

/// Shows a folder in the system's file manager.
fn open_folder(path: &Path) {
    let _ = std::fs::create_dir_all(path);
    let opener = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };

    if let Err(e) = std::process::Command::new(opener).arg(path).spawn() {
        log::error(format!("cannot open {}: {e}", path.display()));
    }
}

impl Editor {
    /// Opens the Settings window (on a fresh copy of the settings) or closes it without applying.
    pub fn show_settings_window(&mut self, open: bool) {
        self.settings_window.draft = open.then(|| self.settings_ref().clone());
        self.settings_window.opened = self.settings_window.draft.clone();
        self.mod_checks_reset();
    }

    /// Shows a page of the open window: general, game, appearance, folders or a tool's id. False when there is no such page.
    pub fn show_settings_page(&mut self, name: &str) -> bool {
        let page = match name {
            "general" => Page::General,
            "game" => Page::Game,
            "appearance" => Page::Appearance,
            "folders" => Page::Folders,
            id => match self.tool_ids().iter().position(|t| t == id) {
                Some(i) if self.with_tool_at(i, |t, _| t.has_settings()).unwrap_or(false) => Page::Tool(i),
                _ => return false,
            },
        };

        self.settings_window.page = page;
        true
    }

    pub fn settings_window_open(&self) -> bool {
        self.settings_window.draft.is_some()
    }

    /// The appearance to show: the window's while it is open, so that its changes show at once.
    pub(crate) fn shown_appearance(&self) -> &wce_foundation::settings::Appearance {
        self.settings_window.draft.as_ref().map_or(&self.settings_ref().appearance, |d| &d.appearance)
    }

    /// Puts the window's copy in place, writes it, and passes on what changed: the frame cap, the packs to mount, the tools' settings.
    pub fn apply_settings_window(&mut self) {
        let Some(mut draft) = self.settings_window.draft.clone() else { return };
        if let Some(packs) = self.mod_checks_for_game(draft.game_profile(), &draft.game_dir()) {
            draft.game_mut().mod_packs = packs;
        }

        // The tools' tables the window left alone are the live ones (a tool may have written its table while the window was open).
        let opened = self.settings_window.opened.as_ref().map(|o| o.tools.clone()).unwrap_or_default();
        for (id, live) in &self.settings_ref().tools {
            if draft.tools.get(id) == opened.get(id) {
                draft.tools.insert(id.clone(), live.clone());
            }
        }

        // The same for each game's own part of the tools' tables.
        let opened_games = self.settings_window.opened.as_ref().map(|o| o.games.clone()).unwrap_or_default();
        for (game, live) in &self.settings_ref().games {
            let was = opened_games.get(game).map(|g| &g.tools);
            let draft_game = draft.games.entry(game.clone()).or_default();
            for (id, table) in &live.tools {
                if draft_game.tools.get(id) == was.and_then(|t| t.get(id)) {
                    draft_game.tools.insert(id.clone(), table.clone());
                }
            }
        }

        let before = std::mem::replace(self.settings(), draft);
        self.save_settings();
        self.set_frame_cap_live(self.settings_ref().frame_cap());

        // Another game: the switch closes the documents and mounts it (or opens the first-run dialog).
        if before.game != self.settings_ref().game {
            let key = std::mem::replace(&mut self.settings().game, before.game.clone());
            if let Err(m) = self.switch_game(&key) {
                self.status(&m, true);
            }

            for i in 0..self.tool_count() {
                self.with_tool_at(i, |t, e| t.settings_applied(e));
            }

            return;
        }

        let now = self.settings_ref();
        let game_changed = before.game_dir() != now.game_dir();
        let files_changed = before.mod_packs() != now.mod_packs() || before.overlay_folders() != now.overlay_folders() || before.loose_dir() != now.loose_dir();
        if !self.is_mounted() {
            if self.start() {
                self.activate_all_tools();
            }
        } else if game_changed {
            self.status("The new game folder is used from the next start of the editor.", false);
        } else if files_changed {
            match self.remount_vfs() {
                Ok(()) => self.activate_all_tools(),
                Err(m) => self.status(&m, true),
            }
        }

        for i in 0..self.tool_count() {
            self.with_tool_at(i, |t, e| t.settings_applied(e));
        }

        if !game_changed {
            self.status(&format!("Settings saved to {}", self.settings_ref().file().display()), false);
        }

        // The window goes on with what was applied.
        if self.settings_window.draft.is_some() {
            self.settings_window.draft = Some(self.settings_ref().clone());
            self.settings_window.opened = self.settings_window.draft.clone();
        }
    }

    pub(crate) fn draw_settings_window(&mut self) {
        if self.settings_window.style_editor {
            gui::set_next_window_size(gui::v2(480.0, 640.0), gui::COND_FIRST_USE_EVER);
            if gui::begin("Style editor", Some(&mut self.settings_window.style_editor), gui::WINDOW_NO_DOCKING) {
                gui::show_style_editor();
            }

            gui::end();
        }

        if self.settings_window.draft.is_none() {
            return;
        }

        let vp = gui::main_viewport();
        gui::set_next_window_pos(gui::v2(vp.Pos.x + vp.Size.x * 0.5, vp.Pos.y + vp.Size.y * 0.5), gui::COND_APPEARING, gui::v2(0.5, 0.5));
        gui::set_next_window_size(gui::v2(820.0, 560.0), gui::COND_FIRST_USE_EVER);
        let mut open = true;
        let visible = gui::begin(&format!("{}  Settings###Settings", icons::SETTINGS), Some(&mut open), gui::WINDOW_NO_DOCKING | gui::WINDOW_NO_COLLAPSE);
        if visible {
            let footer = gui::frame_height_with_spacing() + gui::item_spacing().y * 2.0;
            gui::begin_child("##pages", gui::v2(180.0, -footer), gui::CHILD_BORDERS, 0);
            self.draw_settings_pages_list();
            gui::end_child();
            gui::same_line();
            gui::begin_child("##page", gui::v2(0.0, -footer), gui::CHILD_BORDERS, 0);
            self.draw_settings_page();
            gui::end_child();

            gui::separator();
            let w = gui::calc_text_width("Cancel") + gui::calc_text_width("Apply") + gui::calc_text_width("OK") + 80.0;
            gui::same_line_at(gui::window_width() - w);
            if gui::button("OK") {
                self.apply_settings_window();
                open = false;
            }

            gui::same_line();
            if gui::button("Cancel") {
                open = false;
            }

            gui::same_line();
            if gui::button("Apply") {
                self.apply_settings_window();
            }
        }

        gui::end();
        if !open {
            self.show_settings_window(false);
        }
    }

    fn draw_settings_pages_list(&mut self) {
        let mut pages = vec![
            (Page::General, format!("{}  General", icons::SETTINGS)),
            (Page::Game, format!("{}  Game", icons::HARD_DRIVE)),
            (Page::Appearance, format!("{}  Appearance", icons::PALETTE)),
        ];

        for i in 0..self.tool_count() {
            let page = self.with_tool_at(i, |t, _| t.has_settings().then(|| format!("{}  {}", t.icon(), t.title()))).flatten();
            if let Some(title) = page {
                pages.push((Page::Tool(i), title));
            }
        }

        pages.push((Page::Folders, format!("{}  Folders", icons::FOLDER)));
        for (page, title) in pages {
            if gui::selectable(&title, self.settings_window.page == page, 0) {
                self.settings_window.page = page;
            }
        }
    }

    fn draw_settings_page(&mut self) {
        match self.settings_window.page {
            Page::General => self.draw_general_page(),
            Page::Game => self.draw_game_page(),
            Page::Appearance => self.draw_appearance_page(),
            Page::Folders => self.draw_folders_page(),
            Page::Tool(i) => {
                self.with_tool_at(i, |t, e| {
                    gui::separator_text(t.title());
                    if let Some(draft) = e.settings_window.draft.as_mut() {
                        t.draw_settings(draft);
                    }
                });
            }
        }
    }

    fn draw_general_page(&mut self) {
        let Some(d) = self.settings_window.draft.as_mut() else { return };
        gui::separator_text("Performance");
        label("Frame rate cap");
        const CAPS: [(u32, &str); 5] = [(30, "30 fps"), (60, "60 fps"), (120, "120 fps"), (144, "144 fps"), (0, "Unlimited")];
        let shown = CAPS.iter().find(|c| c.0 == d.frame_cap).map_or_else(|| format!("{} fps", d.frame_cap), |c| c.1.to_owned());
        gui::set_next_item_width(200.0);
        if gui::begin_combo("##frame_cap", &shown) {
            for (fps, name) in CAPS {
                if gui::selectable(name, d.frame_cap == fps, 0) {
                    d.frame_cap = fps;
                }
            }

            gui::end_combo();
        }

        env_note("WCE_FRAME_CAP");
        help("Holds an interactive session to that many frames per second, so that a laptop GPU does not heat up and throttle itself.");
    }

    fn draw_game_page(&mut self) {
        let Some(d) = self.settings_window.draft.as_mut() else { return };
        let game = d.game_profile();
        label("Game");
        gui::set_next_item_width(300.0);
        if gui::begin_combo("##game", game.display_name) {
            for g in wce_vfs::GameProfile::ALL {
                if gui::selectable(g.display_name, g.key == game.key, 0) {
                    d.game = g.key.to_owned();
                }
            }

            gui::end_combo();
        }

        help("The game being edited. Applying another one closes the open documents and reads that game's packs.");
        let game = d.game_profile();
        gui::separator_text(game.display_name);
        let mut dir = d.game_mut().dir.clone();
        if folder_field("Game folder", &mut dir, "the folder with the game's data folder") {
            d.game_mut().dir = dir;
        }

        gui::set_cursor_pos_x(LABEL_WIDTH);
        if gui::button(&format!("{}  Detect", icons::SEARCH))
            && let Some(g) = detect_game_folder(game)
        {
            d.game_mut().dir = Some(g);
        }

        env_note("WCE_GAME");
        let default_output = crate::editor::default_output_dir_for(d.game_profile()).display().to_string();
        let mut output = d.game_mut().output_dir.clone();
        if folder_field("Output folder", &mut output, &default_output) {
            d.game_mut().output_dir = output;
        }

        env_note("WCE_OUTPUT_DIR");
        help("Save writes this game's loose files here, under their path in the packs (and reads them back as the game's).");
        let game = d.game_dir();
        if let Some(problem) = game.as_ref().and_then(|g| d.game_profile().folder_problem(g)) {
            gui::set_cursor_pos_x(LABEL_WIDTH);
            gui::text_colored(theme::error(), &format!("{}  {problem}", icons::CIRCLE_X));
        }

        gui::separator_text("Mod packs");
        help("Read on top of the game's own packs. The launcher's list of enabled mods is not used.");
        env_note("WCE_MOD_PACKS");
        gui::set_cursor_pos_x(LABEL_WIDTH);
        let game = game.map(|g| g.display().to_string()).unwrap_or_default();
        let (profile, chosen) = (d.game_profile(), d.mod_packs());
        self.mod_pack_checks_at(&game, profile, &chosen, LABEL_WIDTH);

        let Some(d) = self.settings_window.draft.as_mut() else { return };
        gui::separator_text("Assembly Kit and projects");
        label("Assembly Kits");
        folder_list("assembly_kits", &mut d.game_mut().assembly_kits);
        env_note("WCE_ASSEMBLY_KIT");
        if d.game_settings().assembly_kits.is_empty() {
            gui::set_cursor_pos_x(LABEL_WIDTH);
            let detected = d.detected_assembly_kit().map_or_else(|| "none found in the game folder".to_owned(), |k| k.display().to_string());
            gui::text_disabled(&format!("In use: {detected}"));
        }

        help("Folders with raw_data; the first one wins where several have the same tile or prefab. The tools read them; only \"Save for Assembly Kit\" writes to one (it asks which when several fit), as TEd and the kit's tools read files. With none listed, the kit in the game folder is used.");
        if !d.game_profile().features.kit_tiles {
            help(&format!("{}'s kit has no battle tiles: the tiles come from the game's packs and go into mod packs (Tile > Save to mod pack).", d.game_profile().display_name));
        }
        let default_projects = d.projects_dir().display().to_string();
        let mut projects = d.game_mut().projects.clone();
        if folder_field("Projects", &mut projects, &default_projects) {
            d.game_mut().projects = projects;
        }

        env_note("WCE_PROJECTS");
        help("Where the editor saves: assembly_kit/ holds the tiles (tile.wce) in the kit's layout, loose/ the loose files read before every pack (the UI tool's skins, layouts and pictures).");

        gui::separator_text("Loose files");
        label("Other folders");
        folder_list("overlays", &mut d.game_mut().overlay_folders);
        env_note("WCE_OVERLAY");
        help("Read before every pack, after the projects' loose files; the first one wins.");
    }

    fn draw_appearance_page(&mut self) {
        let Some(d) = self.settings_window.draft.as_mut() else { return };
        gui::separator_text("Theme");
        label("Theme");
        let shown = theme::THEMES.iter().find(|t| t.id == d.appearance.theme).map_or(d.appearance.theme.as_str(), |t| t.title);
        gui::set_next_item_width(260.0);
        if gui::begin_combo("##theme", shown) {
            for t in theme::THEMES {
                if gui::selectable(t.title, t.id == d.appearance.theme, 0) {
                    d.appearance.theme = t.id.to_owned();
                }
            }

            gui::end_combo();
        }

        label("Text size");
        gui::set_next_item_width(260.0);
        gui::slider_float("##font_size", &mut d.appearance.font_size, *theme::FONT_SIZES.start(), *theme::FONT_SIZES.end(), "%.0f pt");
        d.appearance.font_size = d.appearance.font_size.round();
        help("Changes show at once; Cancel goes back.");

        gui::separator_text("Preview");
        gui::set_cursor_pos_x(LABEL_WIDTH);
        gui::text(&format!("{}  Saved   {}  Undo   {}  Layers   {}  Visible", icons::SAVE, icons::UNDO_2, icons::LAYERS, icons::EYE));
        gui::set_cursor_pos_x(LABEL_WIDTH);
        gui::text_colored(theme::error(), "error");
        gui::same_line();
        gui::text_colored(theme::warning(), "warning");
        gui::same_line();
        gui::text_colored(theme::modified(), "modified");
        gui::same_line();
        gui::text_colored(theme::success(), "done");
        gui::same_line();
        gui::text_colored(theme::dim(), "secondary");

        gui::separator_text("Theme work");
        gui::set_cursor_pos_x(LABEL_WIDTH);
        gui::checkbox("Dear ImGui's style editor", &mut self.settings_window.style_editor);
        help("Edits the live style (not saved): to try out colours and sizes for a theme in theme.rs.");
    }

    fn draw_folders_page(&mut self) {
        let paths = self.settings_ref().paths().clone();
        gui::separator_text("Where the editor keeps its files");
        for (name, path) in [
            ("Settings", paths.config_dir().to_path_buf()), ("Your files", paths.data_dir().to_path_buf()), ("Caches", paths.cache_dir().to_path_buf()),
            ("Log", paths.log_dir().to_path_buf()),
        ] {
            label(name);
            gui::text(&path.display().to_string());
            gui::same_line();
            if gui::small_button(&format!("{}  Open##{name}", icons::FOLDER_OPEN)) {
                open_folder(&path);
            }
        }

        env_note("WCE_SETTINGS_DIR");
        env_note("WCE_CACHE_DIR");
        help("The caches (map context terrain, tile index) are rebuilt when deleted.");
    }
}
