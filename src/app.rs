use glium::glutin::{
    event::{DeviceEvent, ElementState, Event, KeyboardInput, VirtualKeyCode, WindowEvent},
    window::Window,
};
use imgui::*;
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    process::{Child, Command},
    rc::Rc,
    sync::mpsc::TryRecvError,
    time::Duration,
};

use rfd::FileDialog;

use crate::{editor::*, help::*, hotkeys::*, search::*, settings::*};

pub struct App {
    default_paths: String,
    default_patterns: String,

    settings: SettingsWindow,
    hotkeys: HotkeysWindow,
    commands: VecDeque<Command>,
    drag_files: Vec<String>,
    tabs: Vec<SearchTab>,
    selected_tab: usize,
    set_selected_tab: Option<usize>,
    pending_command: Option<Child>,
    shift_pressed: bool,
    ctrl_pressed: bool,
    alt_pressed: bool,
    super_pressed: bool,
}

pub struct UiSearchEntry {
    pub path: Rc<String>,
    pub lines: Vec<SearchResultLine>,
}

impl UiSearchEntry {
    fn new(path: Rc<String>, entry: SearchResultEntry) -> Self {
        return Self {
            path,
            lines: entry.lines,
        };
    }
}

pub struct SearchTab {
    config: SearchConfig,
    results: Vec<UiSearchEntry>,
    pending_search: Option<PendingSearch>,
    file_searched: usize,
    file_searched_with_results: usize,
    search_duration: Duration,
    last_focused_id: Option<(usize, usize)>,
    last_selected_id: Option<(usize, usize)>,
    error_message: Option<String>,
    focus_query_input: bool,
}

impl SearchTab {
    pub fn from_context(context: String, patterns: String) -> Self {
        Self {
            config: SearchConfig::with_paths_and_patterns(context, patterns),
            ..Self::default()
        }
    }

    pub fn default() -> Self {
        Self {
            config: SearchConfig::default(),
            results: Vec::new(),
            pending_search: None,
            file_searched: 0,
            file_searched_with_results: 0,
            search_duration: Duration::from_secs(0),
            last_focused_id: None,
            last_selected_id: None,
            error_message: None,
            focus_query_input: true,
        }
    }

    pub fn clone_for_tab(&self) -> Self {
        Self {
            config: self.config.clone(),
            ..Self::default()
        }
    }

    fn cancel_search(&mut self, clear_results: bool) {
        if let Some(pending) = self.pending_search.as_mut() {
            pending.signal_stop();
            self.search_duration = pending.elapsed();
        }

        self.pending_search = None;

        if clear_results {
            self.results.clear();
            self.file_searched = 0;
            self.search_duration = Duration::from_secs(0);
            self.file_searched_with_results = 0;
            self.last_focused_id = None;
            self.last_selected_id = None;
            self.error_message = None;
        }
    }

    fn save_results(results: &mut Vec<UiSearchEntry>, result: SearchResult) {
        if let Ok(path) = result.path.into_os_string().into_string() {
            let path = Rc::new(path);
            for entry in result.entries.into_iter() {
                let path = Rc::clone(&path);
                results.push(UiSearchEntry::new(path, entry));
            }
        } else {
            println!("Failed to convert the path in a UTF-8 string");
        }
    }

    fn update_pending_search(&mut self) {
        let mut is_done = false;
        if let Some(pending) = self.pending_search.as_mut() {
            loop {
                match pending.try_recv() {
                    Ok(result) => {
                        self.file_searched += 1;
                        if !result.entries.is_empty() {
                            self.file_searched_with_results += 1;
                            Self::save_results(&mut self.results, result);
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        is_done = true;
                        self.search_duration = pending.elapsed();
                        break;
                    }
                }
            }
        }

        if is_done {
            self.pending_search = None;
        }
    }

    fn is_searching(&self) -> bool {
        self.pending_search.is_some()
    }

    fn search_duration(&self) -> Duration {
        if let Some(pending) = &self.pending_search {
            pending.elapsed()
        } else {
            self.search_duration
        }
    }
}

pub fn init(paths: Option<String>, patterns: Option<String>, config: Option<String>) -> App {
    return App::new(paths, patterns, config);
}

impl App {
    fn new(paths: Option<String>, patterns: Option<String>, config: Option<String>) -> Self {
        let settings = if let Some(config) = config {
            SettingsWindow::load_from_file(PathBuf::from(config))
        } else {
            SettingsWindow::open_setting()
        };

        let default_paths = paths.unwrap_or_else(Self::cwd);
        let default_patterns = patterns.unwrap_or_default();

        let tabs = vec![SearchTab::from_context(
            default_paths.clone(),
            default_patterns.clone(),
        )];

        return Self {
            default_paths,
            default_patterns,
            settings,
            hotkeys: HotkeysWindow::new(),
            commands: VecDeque::new(),
            drag_files: Vec::new(),
            tabs,
            selected_tab: 0,
            set_selected_tab: None,
            pending_command: None,
            shift_pressed: false,
            ctrl_pressed: false,
            alt_pressed: false,
            super_pressed: false,
        };
    }

    fn default_search_tab(&self) -> SearchTab {
        return SearchTab::from_context(self.default_paths.clone(), self.default_patterns.clone());
    }

    fn handle_key_modifier(&mut self, key: VirtualKeyCode, down: bool) -> bool {
        if key == VirtualKeyCode::LShift || key == VirtualKeyCode::RShift {
            self.shift_pressed = down;
        } else if key == VirtualKeyCode::LControl || key == VirtualKeyCode::RControl {
            self.ctrl_pressed = down;
        } else if key == VirtualKeyCode::LAlt || key == VirtualKeyCode::RAlt {
            self.alt_pressed = down;
        } else if key == VirtualKeyCode::LWin || key == VirtualKeyCode::RWin {
            self.super_pressed = down;
        } else {
            return false;
        }

        return true;
    }

    pub fn handle_event<T>(&mut self, window: &Window, event: &Event<T>) -> bool {
        match *event {
            Event::WindowEvent {
                window_id,
                ref event,
            } if window_id == window.id() => self.handle_window_event(event),
            // Track key release events outside our window. If we don't do this,
            // we might never see the release event if some other window gets focus.
            Event::DeviceEvent {
                event:
                    DeviceEvent::Key(KeyboardInput {
                        state: ElementState::Released,
                        virtual_keycode: Some(key),
                        ..
                    }),
                ..
            } => {
                self.handle_key_modifier(key, false);
                return false;
            }
            _ => false,
        }
    }

    fn handle_window_event(&mut self, event: &WindowEvent) -> bool {
        match *event {
            WindowEvent::ModifiersChanged(modifiers) => {
                // We need to track modifiers separately because some system like macOS, will
                // not reliably send modifier states during certain events like ScreenCapture.
                // Gotta let the people show off their pretty imgui widgets!
                self.shift_pressed = modifiers.shift();
                self.ctrl_pressed = modifiers.ctrl();
                self.alt_pressed = modifiers.alt();
                self.super_pressed = modifiers.logo();
                return false;
            }
            WindowEvent::KeyboardInput {
                input:
                    KeyboardInput {
                        virtual_keycode: Some(key),
                        state,
                        ..
                    },
                ..
            } => {
                let pressed = state == ElementState::Pressed;
                if self.handle_key_modifier(key, pressed) {
                    return false;
                }

                // Process keys
                return self.handle_key_event(key, state);
            }
            _ => return false,
        }
    }

    fn handle_key_event(&mut self, key: VirtualKeyCode, state: ElementState) -> bool {
        let key_ctrl = self.ctrl_pressed;
        let key_shift = self.shift_pressed;

        if key == VirtualKeyCode::T && key_ctrl {
            if state == ElementState::Pressed {
                if key_shift {
                    let new_tab = if let Some(tab) = self.tabs.get_mut(self.selected_tab) {
                        tab.clone_for_tab()
                    } else {
                        self.default_search_tab()
                    };
                    self.tabs.push(new_tab);
                } else {
                    self.tabs.push(self.default_search_tab());
                }
            }

            return true;
        }

        // Rotate left with "PageUp".
        if key == VirtualKeyCode::PageUp && key_ctrl {
            if state == ElementState::Released {
                let new_id = if self.selected_tab == 0 {
                    self.tabs.len() - 1
                } else {
                    self.selected_tab - 1
                };
                self.set_selected_tab = Some(new_id);
            }
            return true;
        }

        // Detect the right that select the tab to the right.
        if key == VirtualKeyCode::PageDown && key_ctrl {
            if state == ElementState::Released {
                let new_id = (self.selected_tab + 1) % self.tabs.len();
                self.set_selected_tab = Some(new_id);
            }
            return true;
        }

        // Rotate left or right with with "Tab".
        if key == VirtualKeyCode::Tab && key_ctrl {
            if state == ElementState::Released {
                if key_shift {
                    let new_id = if self.selected_tab == 0 {
                        self.tabs.len() - 1
                    } else {
                        self.selected_tab - 1
                    };
                    self.set_selected_tab = Some(new_id);
                } else {
                    let new_id = (self.selected_tab + 1) % self.tabs.len();
                    self.set_selected_tab = Some(new_id);
                }
            }
            return true;
        }

        // Detect the hotkey that select the tab to the right.
        if key == VirtualKeyCode::W && key_ctrl {
            if state == ElementState::Released && !self.tabs.is_empty() {
                self.tabs.drain(self.selected_tab..(self.selected_tab + 1));
                let modul = std::cmp::max(self.tabs.len(), 1);
                self.selected_tab %= modul;
            }
            return true;
        }

        // Cancel search if there is a search pending.
        if key == VirtualKeyCode::Escape {
            if state == ElementState::Released {
                if let Some(tab) = self.tabs.get_mut(self.selected_tab) {
                    tab.cancel_search(false);
                }
            }
            return true;
        }

        // Open selected element in the editor.
        if key == VirtualKeyCode::F4 {
            if state == ElementState::Pressed {
                if let Some(tab) = self.tabs.get_mut(self.selected_tab) {
                    if !self.settings.settings.editor_path().is_empty() {
                        if let Some((row_id, line_id)) = tab.last_focused_id {
                            let command = build_command(
                                self.settings.settings.editor_path(),
                                tab.results[row_id].path.as_ref().clone(),
                                tab.results[row_id].lines[line_id].line_number as usize,
                            );

                            if let Ok(command) = command {
                                self.commands.push_back(command);
                            } else {
                                println!(
                                    "Invalid editor '{}'",
                                    self.settings.settings.editor_path()
                                );
                            }
                        }
                    } else {
                        let error = String::from("Editor not configured");
                        println!("{}", error);
                        tab.error_message = Some(error);
                    }
                }
            }
            return true;
        }

        // Toggle the hotkey window.
        if key == VirtualKeyCode::F1 {
            if state == ElementState::Pressed {
                self.hotkeys.toggle_open();
            }
            return true;
        }

        // Focus the search window.
        if key == VirtualKeyCode::F && key_ctrl {
            if state == ElementState::Pressed {
                if let Some(tab) = self.tabs.get_mut(self.selected_tab) {
                    tab.focus_query_input = true;
                }
            }
            return true;
        }

        return false;
    }

    fn cwd() -> String {
        std::env::current_dir()
            .map(|path| {
                path.into_os_string()
                    .into_string()
                    .unwrap_or_else(|_| String::from("./"))
            })
            .unwrap_or_else(|_| String::from("./"))
    }

    fn search_parallel(tab: &mut SearchTab, settings: &Settings) {
        tab.cancel_search(true);

        let non_existing_paths: Vec<String> = tab
            .config
            .paths()
            .into_iter()
            .filter(|path| !path.exists())
            .map(|path| path.to_string_lossy().into_owned())
            .collect();

        if !non_existing_paths.is_empty() {
            let error = format!("Can't open {}", non_existing_paths.join(", "));
            println!("{}", error);
            tab.error_message = Some(error);
        }

        if let Ok(pending) = crate::search::spawn_search(
            &tab.config,
            settings.search_binary,
            settings.number_of_threads as usize,
        ) {
            tab.pending_search = Some(pending);
        }
    }

    fn draw_menu(&mut self, ui: &Ui, keep_running: &mut bool) {
        if let Some(menu) = ui.begin_menu("File") {
            if ui.menu_item_config("New Tab").shortcut("CTRL+T").build() {
                self.tabs.push(self.default_search_tab());
            }

            ui.menu_item_config("Open...").shortcut("CTRL+O").build();
            ui.separator();
            if ui.menu_item_config("Quit").shortcut("CTRL+Q").build() {
                *keep_running = false;
            }
            menu.end();
        }

        if let Some(menu) = ui.begin_menu("Edit") {
            ui.menu_item_config("Undo").shortcut("CTRL+Z").build();
            ui.menu_item_config("Redo").shortcut("CTRL+Y").build();
            ui.separator();
            menu.end();
        }

        if let Some(menu) = ui.begin_menu("Preferences") {
            if ui.menu_item_config("Settings").build() {
                self.settings.open(true);
            }
            menu.end();
        }

        if let Some(menu) = ui.begin_menu("Help") {
            let version_homepage_text = concat!(
                "Version: ",
                env!("CARGO_PKG_VERSION"),
                "\nHomepage: ",
                env!("CARGO_PKG_HOMEPAGE")
            );
            ui.text(version_homepage_text);
            ui.separator();
            if ui.menu_item_config("Hotkeys").shortcut("F1").build() {
                self.hotkeys.toggle_open();
            }
            menu.end();
        }
    }

    fn draw_text_from_cow(ui: &Ui, color: Option<[f32; 4]>, text: std::borrow::Cow<'_, str>) {
        use std::borrow::Cow;
        let _style = color.map(|color| ui.push_style_color(StyleColor::Text, color));
        match text {
            Cow::Borrowed(text) => ui.text(text),
            Cow::Owned(text) => ui.text(text),
        }
    }

    fn draw_line_with_matches(ui: &Ui, line: &SearchResultLine) {
        if line.is_matched() {
            const COLOR_RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
            let mut printed = 0;
            for (start, end) in line.matches.iter().copied() {
                Self::draw_text_from_cow(
                    ui,
                    None,
                    String::from_utf8_lossy(&line.bytes[printed..start]),
                );
                ui.same_line_with_spacing(0.0, 0.0);
                Self::draw_text_from_cow(
                    ui,
                    Some(COLOR_RED),
                    String::from_utf8_lossy(&line.bytes[start..end]),
                );
                ui.same_line_with_spacing(0.0, 0.0);
                printed = end;
            }
            Self::draw_text_from_cow(ui, None, String::from_utf8_lossy(&line.bytes[printed..]));
        } else {
            Self::draw_text_from_cow(ui, None, String::from_utf8_lossy(&line.bytes));
        }
    }

    fn draw_selectable_path(
        &mut self,
        ui: &Ui,
        tab: &mut SearchTab,
        row_id: usize,
        full_path: Rc<String>,
        label: &str,
        line_id: usize,
        line: &mut SearchResultLine,
    ) {
        let _stack = ui.push_id_usize(line_id);

        if ui
            .selectable_config(label)
            .span_all_columns(true)
            .selected(tab.last_selected_id == Some((row_id, line_id)))
            .allow_double_click(true)
            .build()
        {
            if ui.is_mouse_double_clicked(MouseButton::Left) {
                let command = build_command(
                    self.settings.settings.editor_path(),
                    full_path.as_ref().clone(),
                    line.line_number as usize,
                );

                if let Ok(command) = command {
                    self.commands.push_back(command);
                } else {
                    println!(
                        "Invalid editor '{}'",
                        self.settings.settings.editor_path()
                    );
                }
            } else {
                tab.last_selected_id = Some((row_id, line_id));
            }
        }

        if ui.is_item_focused() {
            tab.last_focused_id = Some((row_id, line_id));
        }

        if ui.is_item_hovered() {
            if self.settings.settings.only_show_filename {
                ui.tooltip_text(full_path.as_ref());
            }

            if ui.is_mouse_clicked(MouseButton::Right) {
                ui.open_popup("row-context");
            }

            if ui.is_mouse_down(MouseButton::Left) && ui.is_mouse_dragging(MouseButton::Left)
            {
                self.drag_files
                    .push(full_path.as_ref().clone());
            }
        }

        if let Some(_) = ui.begin_popup("row-context") {
            if ui.menu_item_config("Open").shortcut("F4").build() {
                let command = build_command(
                    self.settings.settings.editor_path(),
                    full_path.as_ref().clone(),
                    line.line_number as usize,
                );

                if let Ok(command) = command {
                    self.commands.push_back(command);
                } else {
                    println!(
                        "Invalid editor '{}'",
                        self.settings.settings.editor_path()
                    );
                }
            }

            if ui.menu_item_config("Copy Full Path").build() {
                ui.set_clipboard_text(full_path.as_ref());
            }
        }
    }

    fn draw_result_line(&mut self, ui: &Ui, tab: &mut SearchTab, row_id: usize) {
        let _stack = ui.push_id_usize(row_id);

        let full_path = Rc::clone(&tab.results[row_id].path);
        let drawn_path = if self.settings.settings.only_show_filename {
            let result_file_path = full_path.as_ref();
            Path::new(result_file_path)
                .file_name()
                .map(|filename| filename.to_str().unwrap_or(result_file_path))
                .unwrap_or(result_file_path)
        } else {
            full_path.as_ref()
        };

        let mut lines = std::mem::take(&mut tab.results[row_id].lines);

        ui.table_next_column();
        for (idx, line) in lines.iter_mut().enumerate() {
            if !line.is_matched() {
                ui.new_line();
            } else {
                self.draw_selectable_path(ui, tab, row_id, Rc::clone(&full_path), drawn_path, idx, line);
            }
        }

        ui.table_next_column();
        for line in lines.iter() {
            ui.text(format!("{}", line.line_number));
        }

        ui.table_next_column();
        for line in lines.iter() {
            Self::draw_line_with_matches(ui, line);
        }

        tab.results[row_id].lines = lines;
    }

    fn draw_results(&mut self, ui: &Ui, tab: &mut SearchTab) {
        let clip = ListClipper::new(tab.results.len() as i32);
        let mut tok = clip.begin(ui);

        let mut flags = TableFlags::REORDERABLE | TableFlags::SCROLL_X;

        // @Enhancement: This refresh even if no new search happen.
        if tab.config.queries.get(0).map(|query| query.extra_context != 0).unwrap_or(false) {
            flags |= TableFlags::ROW_BG;
        }

        if let Some(_) = ui.begin_table_with_flags("table-headers", 3, flags) {
            ui.table_setup_column("File");
            ui.table_setup_column("Line");
            ui.table_setup_column("Text");
            ui.table_headers_row();

            while tok.step() {
                for row_num in tok.display_start()..tok.display_end() {
                    let row_id = row_num as usize;
                    self.draw_result_line(ui, tab, row_id);
                }
            }
        }
    }

    fn draw_tab(&mut self, ui: &Ui, tab_id: usize, mut tab: SearchTab) {
        tab.update_pending_search();

        let mut flags = TabItemFlags::empty();
        if self.set_selected_tab == Some(tab_id) {
            flags |= TabItemFlags::SET_SELECTED;
            self.set_selected_tab = None;
        }

        flags |= TabItemFlags::TRAILING;

        let label = format!("{}###{}", tab.config.paths, tab_id);
        let mut keep_open = true;
        TabItem::new(label)
            .opened(&mut keep_open)
            .flags(flags)
            .build(ui, || {
                // If we enter this block, we are in the selected tab.
                self.selected_tab = tab_id;

                let mut search = false;
                if let Some(_t) =
                    ui.begin_table_with_flags("Basic-Table", 2, TableFlags::SIZING_FIXED_FIT)
                {
                    // ui.text("Search:");

                    ui.table_setup_column_with(TableColumnSetup {
                        name: "##labels",
                        flags: TableColumnFlags::WIDTH_FIXED,
                        init_width_or_weight: 0.0,
                        user_id: Id::default(),
                    });
                    ui.table_setup_column_with(TableColumnSetup {
                        name: "##widgets",
                        flags: TableColumnFlags::WIDTH_STRETCH,
                        init_width_or_weight: 0.0,
                        user_id: Id::default(),
                    });
                    ui.table_next_row();

                    ui.table_next_column();
                    ui.text("Paths:");
                    ui.table_next_column();
                    let mut paths_edited = ui
                        .input_text("##paths", &mut tab.config.paths)
                        .enter_returns_true(true)
                        .build();

                    show_help(ui, crate::help::PATHS_USAGE);

                    ui.same_line();
                    if ui.button("...") {
                        let maybe_folders = FileDialog::new().set_directory("/").pick_folders();

                        match maybe_folders {
                            Some(folders) => {
                                for f in folders.iter() {
                                    match tab.config.paths.chars().last() {
                                        None | Some(';') => (),
                                        _ => tab.config.paths.push(';'),
                                    };

                                    tab.config
                                        .paths
                                        .push_str(&f.as_path().display().to_string());
                                }
                                paths_edited = true;
                            }
                            None => (),
                        }
                    }

                    if paths_edited {
                        // Keep the focus in the search input making it easier to iterate.
                        ui.set_keyboard_focus_here_with_offset(FocusedWidget::Previous);
                        search = true;
                    }

                    ui.table_next_column();
                    ui.text("Patterns:");
                    ui.table_next_column();
                    if ui
                        .input_text("##globs", &mut tab.config.globs)
                        .enter_returns_true(!self.settings.settings.incremental_search)
                        .hint("*.txt *.cpp")
                        .build()
                    {
                        search = true;
                        // Keep the focus in the search input making it easier to iterate.
                        ui.set_keyboard_focus_here_with_offset(FocusedWidget::Previous);
                    }
                    show_help(ui, crate::help::GLOBS_USAGE);

                    let queries = std::mem::take(&mut tab.config.queries);
                    for (idx, mut query) in queries.into_iter().enumerate() {
                        // Dropping this value pop the id from IMGUI stack.
                        let _stack = ui.push_id_usize(idx);

                        ui.table_next_column();
                        ui.text("Text:");

                        // How can we calculate that dynamically such that the button fits in the window?
                        ui.table_next_column();
                        let _w = ui.push_item_width(450.0);

                        if tab.focus_query_input {
                            ui.set_keyboard_focus_here_with_offset(FocusedWidget::Next);
                            tab.focus_query_input = false;
                        }

                        if ui
                            .input_text("##search", &mut query.query)
                            .hint("(press enter to search)")
                            .enter_returns_true(!self.settings.settings.incremental_search)
                            .build()
                        {
                            search = true;

                            // Keep the focus in the search input making it easier to iterate.
                            ui.set_keyboard_focus_here_with_offset(FocusedWidget::Previous);
                        }

                        ui.same_line();
                        ui.checkbox("Regex", &mut query.regex_syntax);
                        ui.same_line();
                        ui.checkbox("Ignore case", &mut query.ignore_case);
                        ui.same_line();
                        ui.checkbox("Invert match", &mut query.invert_match);
                        if ui.is_item_hovered() {
                            ui.tooltip_text("Show lines that do not match the given patterns.");
                        }

                        ui.same_line();
                        ui.set_next_item_width(80.0);
                        let mut extra_context_value = query.extra_context as i32;
                        if ui.input_int("Context", &mut extra_context_value).build() {
                            match extra_context_value.try_into() {
                                Ok(value) => query.extra_context = value,
                                Err(_) => {
                                    tab.error_message = Some(String::from("Context value should be positive"));
                                }
                            }
                        }
                        if ui.is_item_hovered() {
                            ui.tooltip_text("Show additional lines before and after each match.");
                        }
                        ui.same_line();

                        tab.config.queries.push(query);
                    }
                }

                // We always have at least 1 query line, so if they were all removed, re-create a default one.
                if tab.config.queries.is_empty() {
                    tab.config.queries.push(SearchQuery::new());
                }

                if ui.button("Search") {
                    search = true;
                }

                ui.same_line();
                let color = ui.push_style_color(StyleColor::Button, [1.0, 0.0, 0.0, 1.0]);
                if ui.button("Cancel") {
                    tab.cancel_search(false);
                }
                color.end();

                if let Some(error_message) = &tab.error_message {
                    ui.same_line();

                    let yellow = [1.0, 0.875, 0.0, 1.0];
                    let cursor_pos = ui.cursor_pos();
                    ui.get_window_draw_list().add_rect_filled_multicolor(
                        cursor_pos,
                        [
                            ui.content_region_max()[0],
                            cursor_pos[1] + ui.text_line_height_with_spacing(),
                        ],
                        yellow,
                        yellow,
                        yellow,
                        yellow,
                    );

                    ui.text_colored([0.0, 0.0, 0.0, 1.0], error_message);
                }

                if search {
                    Self::search_parallel(&mut tab, &self.settings.settings);
                }

                // @Enhancement: Shouldn't calculate that every frame.
                let height_seperator = unsafe { ui.style() }.item_spacing[1];
                let footer_height = height_seperator + ui.frame_height();

                ui.separator();
                ui.child_window("##results")
                    .size([0.0, -footer_height])
                    .build(|| self.draw_results(ui, &mut tab));

                ui.separator();
                let duration = tab.search_duration();
                let footer_text = format!(
                    "{} result(s) in {} file(s) ({} file(s) searched)      Duration: {}.{} secs",
                    tab.results.len(),
                    tab.file_searched_with_results,
                    tab.file_searched,
                    duration.as_secs(),
                    duration.subsec_millis(),
                );

                ui.text(footer_text);

                // @Enhancement: This is wasteful
                let searching_text = if tab.is_searching() {
                    "Searching..."
                } else {
                    "Done..."
                };

                let searching_text_width = ui.calc_text_size(searching_text)[0];
                let window_width = ui.window_content_region_max()[0];
                ui.same_line_with_pos(window_width - searching_text_width);
                ui.text(searching_text);
            }); // build end

        if keep_open {
            self.tabs.push(tab);
        }
    }

    pub fn update(&mut self, keep_running: &mut bool, ui: &Ui) {
        let window_size = ui.io().display_size;

        self.settings.draw_settings(ui);
        self.hotkeys.draw_hotkeys_help(ui);

        let window = ui
            .window("Search##main")
            .position([0.0, 0.0], Condition::FirstUseEver)
            .size(window_size, Condition::Always)
            .movable(false)
            .resizable(false)
            .collapsible(false)
            .title_bar(false)
            .bring_to_front_on_focus(false)
            .menu_bar(true);

        window.build(|| {
            if let Some(mut child) = self.pending_command.take() {
                if let Ok(None) = child.try_wait() {
                    self.pending_command = Some(child);
                }
            };

            while self.pending_command.is_none() {
                if let Some(mut command) = self.commands.pop_front() {
                    if let Ok(child) = command.spawn() {
                        self.pending_command = Some(child);
                    } else {
                        println!(
                            "Failed to start editor '{:?}' with args '{:?}'",
                            command.get_program(),
                            command.get_args()
                        );
                    }
                } else {
                    break;
                }
            }

            if let Some(_) = ui.begin_menu_bar() {
                self.draw_menu(ui, keep_running);
            }

            let tab_flags = TabBarFlags::REORDERABLE | TabBarFlags::AUTO_SELECT_NEW_TABS;
            TabBar::new("##tabs").flags(tab_flags).build(ui, || {
                let tabs = std::mem::take(&mut self.tabs);
                for (tab_id, tab) in tabs.into_iter().enumerate() {
                    let _stack = ui.push_id_usize(tab_id);
                    self.draw_tab(ui, tab_id, tab);
                }
            });
        });
    }

    pub fn process_drag_drop(&mut self, io: &mut Io) {
        if !self.drag_files.is_empty() {
            let files = std::mem::take(&mut self.drag_files);
            let files: Vec<&str> = files.iter().map(|file| file.as_str()).collect();

            crate::sys::enter_drag_drop(files.as_slice());

            // It's unclear whether we actually have to do that, but it seems so.
            // Overall, we never receive the event telling us the mouse button was down, so imgui
            // keeps thinking the button is clicked.
            io.add_mouse_button_event(MouseButton::Left, false);
        }
    }
}
