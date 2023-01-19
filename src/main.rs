#![windows_subsystem = "windows"]

mod args;
mod clipboard;
mod editor;
mod help;
mod hotkeys;
mod search;
mod settings;
mod support;

use glium::glutin::event::VirtualKeyCode;
use imgui::*;
use std::{
    collections::VecDeque,
    process::{Child, Command},
    rc::Rc,
    sync::mpsc::TryRecvError,
    time::Duration,
};

use crate::{
    editor::*,
    help::*,
    hotkeys::*,
    search::*,
    settings::*,
};

pub struct UiSearchEntry {
    pub selected: bool,
    pub path: Rc<String>,
    pub line_number: u64,
    pub bytes: Vec<u8>,
    pub matches: Vec<(usize, usize)>,
}

impl UiSearchEntry {
    fn new(path: Rc<String>, entry: SearchResultEntry) -> Self {
        Self {
            selected: false,
            path,
            line_number: entry.line_number,
            bytes: entry.bytes,
            matches: entry.matches,
        }
    }
}

pub struct SearchTab {
    config: SearchConfig,
    results: Vec<UiSearchEntry>,
    pending_search: Option<PendingSearch>,
    file_searched: usize,
    file_searched_with_results: usize,
    search_duration: Duration,
    last_focused_row: Option<usize>,
    last_selected_row: Option<usize>,
    error_message: Option<String>,
    focus_query_input: bool,
}

impl SearchTab {
    pub fn from_context(context: String) -> Self {
        Self {
            config: SearchConfig::with_paths(context),
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
            last_focused_row: None,
            last_selected_row: None,
            error_message: None,
            focus_query_input: false,
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
            self.last_focused_row = None;
            self.last_selected_row = None;
            self.error_message = None;
        }
    }

    fn save_results(results: &mut Vec<UiSearchEntry>, result: SearchResult){
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
                    },
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

pub struct SearchTabs {
    tabs: Vec<SearchTab>,
    selected_tab: usize,
    set_selected_tab: Option<usize>,
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

    if let Ok(pending) = search::spawn_search(&tab.config, settings.search_binary, settings.number_of_threads as usize) {
        tab.pending_search = Some(pending);
    }
}

fn cwd() -> String {
    std::env::current_dir()
        .map(|path| {
            path.into_os_string()
                .into_string()
                .unwrap_or(String::from("./"))
        })
        .unwrap_or(String::from("./"))
}

fn draw_menu(
    ui: &Ui,
    keep_running: &mut bool,
    state: &mut SearchTabs,
    settings: &mut SettingsWindow,
    hotkeys: &mut HotkeysWindow
) {
    if let Some(menu) = ui.begin_menu("File") {
        if ui.menu_item_config("New Tab").shortcut("CTRL+T").build() {
            state.tabs.push(SearchTab::from_context(cwd()));
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
            settings.open(true);
        }
        menu.end();
    }

    if let Some(menu) = ui.begin_menu("Help") {
        ui.text("Version: 0.3.0\nHomepage: https://github.com/reduf/search");
        ui.separator();
        if ui.menu_item_config("Hotkeys").shortcut("F1").build() {
            hotkeys.toggle_open();
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

fn draw_result(ui: &Ui, result: &UiSearchEntry) {
    const COLOR_RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
    let mut printed = 0;
    for (start, end) in result.matches.iter().map(|&val| val) {
        draw_text_from_cow(ui, None, String::from_utf8_lossy(&result.bytes[printed..start]));
        ui.same_line_with_spacing(0.0, 0.0);
        draw_text_from_cow(ui, Some(COLOR_RED), String::from_utf8_lossy(&result.bytes[start..end]));
        ui.same_line_with_spacing(0.0, 0.0);
        printed = end;
    }
    draw_text_from_cow(ui, None, String::from_utf8_lossy(&result.bytes[printed..]));
}

fn draw_tab(
    ui: &Ui,
    state: &mut SearchTabs,
    tab_id: usize,
    mut tab: SearchTab,
    settings: &Settings,
    commands: &mut VecDeque<Command>,
) {
    tab.update_pending_search();

    let mut flags = TabItemFlags::empty();
    if state.set_selected_tab == Some(tab_id) {
        flags |= TabItemFlags::SET_SELECTED;
        state.set_selected_tab = None;
    }

    flags |= TabItemFlags::TRAILING;

    let label = format!("{}###{}", tab.config.paths, tab_id);
    let mut keep_open = true;
    TabItem::new(label).opened(&mut keep_open).flags(flags).build(ui, || {
        // If we enter this block, we are in the selected tab.
        state.selected_tab = tab_id;

        let mut search = false;
        if let Some(_t) = ui.begin_table_with_flags("Basic-Table", 2, TableFlags::SIZING_FIXED_FIT) {
            // ui.text("Search:");

            ui.table_setup_column_with(TableColumnSetup { name: "##labels", flags: TableColumnFlags::WIDTH_FIXED, init_width_or_weight: 0.0, user_id: Id::default() });
            ui.table_setup_column_with(TableColumnSetup { name: "##widgets", flags: TableColumnFlags::WIDTH_STRETCH, init_width_or_weight: 0.0, user_id: Id::default() });
            ui.table_next_row();

            ui.table_next_column();
            ui.text("Paths:");
            ui.table_next_column();
            if ui
                .input_text("##paths", &mut tab.config.paths)
                .enter_returns_true(true)
                .build()
            {
                // Keep the focus in the search input making it easier to iterate.
                ui.set_keyboard_focus_here_with_offset(FocusedWidget::Previous);
                search = true;
            }
            show_help(ui, help::PATHS_USAGE);

            ui.table_next_column();
            ui.text("Patterns:");
            ui.table_next_column();
            if ui
                .input_text("##globs", &mut tab.config.globs)
                .enter_returns_true(settings.interactive_search)
                .hint("*.txt *.cpp")
                .build()
            {
                search = true;
                // Keep the focus in the search input making it easier to iterate.
                ui.set_keyboard_focus_here_with_offset(FocusedWidget::Previous);
            }
            show_help(ui, help::GLOBS_USAGE);

            let queries = std::mem::replace(&mut tab.config.queries, vec![]);
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
                    .enter_returns_true(settings.interactive_search)
                    .build()
                {
                    search = true;

                    // Keep the focus in the search input making it easier to iterate.
                    ui.set_keyboard_focus_here_with_offset(FocusedWidget::Previous);
                }

                ui.same_line();
                ui.checkbox("Regex syntax", &mut query.regex_syntax);
                ui.same_line();
                ui.checkbox("Ignore case", &mut query.ignore_case);
                ui.same_line();
                ui.checkbox("Invert match", &mut query.invert_match);
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
                [ui.content_region_max()[0], cursor_pos[1] + ui.text_line_height_with_spacing()],
                yellow,
                yellow,
                yellow,
                yellow,
            );

            ui.text_colored([0.0, 0.0, 0.0, 1.0], error_message);
        }

        if search {
            search_parallel(&mut tab, settings);
        }

        // @Enhancement: Shouldn't calculate that every frame.
        let height_seperator = unsafe { ui.style() }.item_spacing[1];
        let footer_height = height_seperator + ui.frame_height();

        ui.separator();
        ui.child_window("##result").size([0.0, -footer_height]).build(|| {
            let clip = ListClipper::new(tab.results.len() as i32);
            let mut tok = clip.begin(ui);

            let flags = TableFlags::REORDERABLE | TableFlags::RESIZABLE | TableFlags::SIZING_FIXED_FIT;
            if let Some(_t) = ui.begin_table_with_flags("table-headers", 3, flags) {
                let avail_width = ui.content_region_avail()[0];
                ui.table_setup_column_with(TableColumnSetup { name: "File", flags: TableColumnFlags::WIDTH_FIXED, init_width_or_weight: 0.5 * avail_width, user_id: Id::default() });
                ui.table_setup_column_with(TableColumnSetup { name: "Line", flags: TableColumnFlags::WIDTH_FIXED, init_width_or_weight: 0.1 * avail_width, user_id: Id::default() });
                ui.table_setup_column_with(TableColumnSetup { name: "Text", flags: TableColumnFlags::WIDTH_STRETCH, init_width_or_weight: 0.0, user_id: Id::default() });
                ui.table_headers_row();

                while tok.step() {
                    for row_num in tok.display_start()..tok.display_end() {
                        let row_id = row_num as usize;
                        let _stack = ui.push_id_usize(row_id);

                        ui.table_next_column();
                        if ui
                            .selectable_config(tab.results[row_id].path.as_ref())
                            .span_all_columns(true)
                            .selected(tab.results[row_id].selected)
                            .allow_double_click(true)
                            .build()
                        {
                            if ui.is_mouse_double_clicked(MouseButton::Left) {
                                let command = build_command(
                                    settings.editor_path(),
                                    tab.results[row_id].path.as_ref().clone(),
                                    tab.results[row_id].line_number as usize,
                                );

                                if let Ok(command) = command {
                                    commands.push_back(command);
                                } else {
                                    println!("Invalid editor '{}'", settings.editor_path());
                                }
                            } else {
                                if let Some(last_selected_row) = tab.last_selected_row {
                                    tab.results[last_selected_row].selected = false;
                                }

                                tab.results[row_id].selected = !tab.results[row_id].selected;
                                tab.last_selected_row = Some(row_id);
                            }
                        }

                        if ui.is_item_focused() {
                            tab.last_focused_row = Some(row_id);
                        }

                        ui.table_next_column();
                        ui.text(format!("{}", tab.results[row_id].line_number));

                        ui.table_next_column();
                        draw_result(ui, &tab.results[row_id]);
                    }
                }
            }
        });

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
        state.tabs.push(tab);
    }
}

fn main() {
    let system = support::init("Search");
    let mut settings = SettingsWindow::open_setting();
    let mut hotkeys = HotkeysWindow::new();

    let mut pending_command: Option<Child> = None;
    let mut commands: VecDeque<Command> = VecDeque::new();
    let mut state = SearchTabs {
        tabs: Vec::new(),
        selected_tab: 0,
        set_selected_tab: None,
    };

    state.tabs.push(SearchTab::from_context(cwd()));

    system.main_loop(move |keep_running, ui| {
        let window_size = ui.io().display_size;

        settings.draw_settings(ui);
        hotkeys.draw_hotkeys_help(ui);

        let window = ui.window("Search##main")
            .position([0.0, 0.0], Condition::FirstUseEver)
            .size(window_size, Condition::Always)
            .movable(false)
            .resizable(false)
            .collapsible(false)
            .title_bar(false)
            .bring_to_front_on_focus(false)
            .menu_bar(true);

        window.build(|| {
            let key_ctrl = ui.io().key_ctrl;
            let key_shift = ui.io().key_shift;

            if ui.is_key_index_released(VirtualKeyCode::T as i32) && key_ctrl {
                if key_shift {
                    let new_tab = if let Some(tab) = state.tabs.get_mut(state.selected_tab) {
                        tab.clone_for_tab()
                    } else {
                        SearchTab::from_context(cwd())
                    };
                    state.tabs.push(new_tab);
                } else {
                    state.tabs.push(SearchTab::from_context(cwd()));
                }
            }

            // Detect the hotkey that select the tab to the left.
            if (key_ctrl && ui.is_key_index_released(VirtualKeyCode::PageUp as i32)) || 
               (key_ctrl && key_shift && ui.is_key_index_released(VirtualKeyCode::Tab as i32))
            {
                let new_id = if state.selected_tab == 0 {
                    state.tabs.len() - 1
                } else {
                    state.selected_tab - 1
                };

                state.set_selected_tab = Some(new_id);
            }

            // Detect the hotkey that select the tab to the right.
            if (key_ctrl && ui.is_key_index_released(VirtualKeyCode::PageDown as i32)) || 
               (key_ctrl && ui.is_key_index_released(VirtualKeyCode::Tab as i32))
            {
                let new_id = (state.selected_tab + 1) % state.tabs.len();
                state.set_selected_tab = Some(new_id);
            }

            // Detect the hotkey that select the tab to the right.
            if key_ctrl && ui.is_key_index_released(VirtualKeyCode::W as i32) {
                if !state.tabs.is_empty() {
                    state.tabs.drain(state.selected_tab..(state.selected_tab + 1));
                    let modul = std::cmp::max(state.tabs.len(), 1);
                    state.selected_tab = state.selected_tab % modul;
                }
            }

            if ui.is_key_index_released(VirtualKeyCode::Escape as i32) {
                if let Some(tab) = state.tabs.get_mut(state.selected_tab) {
                    tab.cancel_search(false);
                }
            }

            if ui.is_key_index_released(VirtualKeyCode::F4 as i32) {
                if let Some(tab) = state.tabs.get_mut(state.selected_tab) {
                    if !settings.settings.editor_path().is_empty() {
                        if let Some(last_focused_row) = tab.last_focused_row {
                            let command = build_command(
                                settings.settings.editor_path(),
                                tab.results[last_focused_row].path.as_ref().clone(),
                                tab.results[last_focused_row].line_number as usize,
                            );

                            if let Ok(command) = command {
                                commands.push_back(command);
                            } else {
                                println!("Invalid editor '{}'", settings.settings.editor_path());
                            }
                        }
                    } else {
                        let error = String::from("Editor not configured");
                        println!("{}", error);
                        tab.error_message = Some(error);
                    }
                }
            }

            if ui.is_key_index_released(VirtualKeyCode::F1 as i32) {
                hotkeys.toggle_open();
            }

            if ui.is_key_index_released(VirtualKeyCode::F as i32) {
                if key_ctrl {
                    if let Some(tab) = state.tabs.get_mut(state.selected_tab) {
                        tab.focus_query_input = true;
                    }
                }
            }

            if let Some(mut child) = pending_command.take() {
                if let Ok(None) = child.try_wait() {
                    pending_command = Some(child);
                }
            };

            while pending_command.is_none() {
                if let Some(mut command) = commands.pop_front() {
                    if let Ok(child) = command.spawn() {
                        pending_command = Some(child);
                    } else {
                        println!("Failed to start editor '{:?}' with args '{:?}'", command.get_program(), command.get_args());
                    }
                } else {
                    break;
                }
            }

            if let Some(_) = ui.begin_menu_bar() {
                draw_menu(ui, keep_running, &mut state, &mut settings, &mut hotkeys);
            }

            let tab_flags = TabBarFlags::REORDERABLE | TabBarFlags::AUTO_SELECT_NEW_TABS;
            TabBar::new("##tabs").flags(tab_flags).build(ui, || {
                let tabs = std::mem::replace(&mut state.tabs, vec![]);
                for (tab_id, tab) in tabs.into_iter().enumerate() {
                    let _stack = ui.push_id_usize(tab_id);
                    draw_tab(ui, &mut state, tab_id, tab, &settings.settings, &mut commands);
                }
            });
        });
    });
}
