// #![windows_subsystem = "windows"]

mod args;
mod clipboard;
mod editor;
mod help;
mod hotkeys;
mod search;
mod settings;
mod support;

use glium::glutin::event::VirtualKeyCode;
use ignore::{WalkBuilder, WalkState};
use imgui::*;
use std::{
    collections::{HashSet, VecDeque},
    process::Child,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
    thread,
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
    pub text: String,
    pub matches: Vec<(usize, usize)>,
}

impl UiSearchEntry {
    fn new(path: Rc<String>, entry: SearchResultEntry) -> Self {
        Self {
            selected: false,
            path,
            line_number: entry.line_number,
            text: entry.text,
            matches: entry.matches,
        }
    }
}

pub struct SearchTab {
    config: SearchConfig,
    results: Vec<UiSearchEntry>,
    pending_search: Option<SearchFuture>,
    file_searched: usize,
    file_searched_with_results: usize,
    search_duration: Duration,
    selected_rows: HashSet<usize>,
    last_selected_row: usize,
}

impl SearchTab {
    pub fn new() -> Self {
        Self::create(String::from("/"))
    }

    pub fn from_context(context: String) -> Self {
        Self::create(context)
    }

    pub fn clone_for_tab(&self) -> Self {
        Self {
            config: self.config.clone(),
            results: Vec::new(),
            pending_search: None,
            file_searched: 0,
            file_searched_with_results: 0,
            search_duration: Duration::from_secs(0),
            selected_rows: HashSet::new(),
            last_selected_row: 0,
        }
    }

    fn create(context: String) -> Self {
        SearchTab {
            config: SearchConfig::with_paths(context),
            results: Vec::new(),
            pending_search: None,
            file_searched: 0,
            file_searched_with_results: 0,
            search_duration: Duration::from_secs(0),
            selected_rows: HashSet::new(),
            last_selected_row: 0,
        }
    }

    fn cancel_search(&mut self, clear_results: bool) {
        if let Some(future) = self.pending_search.as_mut() {
            future.quit.store(true, Ordering::Relaxed);
            self.search_duration = future.start_time.elapsed();
        }

        self.pending_search = None;

        if clear_results {
            self.results.clear();
            self.file_searched = 0;
            self.search_duration = Duration::from_secs(0);
            self.file_searched_with_results = 0;
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
        if let Some(future) = self.pending_search.as_mut() {
            loop {
                match future.rx.try_recv() {
                    Ok(result) => {
                        self.file_searched += 1;
                        if !result.entries.is_empty() {
                            self.file_searched_with_results += 1;
                            Self::save_results(&mut self.results, result);
                        }
                    },
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        is_done = true;
                        self.search_duration = future.start_time.elapsed();
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
        if let Some(future) = &self.pending_search {
            future.start_time.elapsed()
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

struct SearchFuture {
    rx: mpsc::Receiver<SearchResult>,
    quit: Arc<AtomicBool>,
    start_time: Instant,
}

fn search_parallel(tab: &mut SearchTab, settings: &Settings) {
    tab.cancel_search(true);

    let (tx, rx) = mpsc::channel();
    let quit = Arc::new(AtomicBool::new(false));
    tab.pending_search = Some(SearchFuture {
        rx,
        quit: quit.clone(),
        start_time: Instant::now(),
    });

    let workers = tab.config.workers();
    if workers.is_empty() {
        // Simply erasing the matches.
        return;
    }

    let mut builder = if let Some((first, remaining)) = tab.config.paths().split_first() {
        let mut builder = WalkBuilder::new(first);
        for path in remaining {
            builder.add(path);
        }
        builder
    } else {
        println!("Can't search with no path");
        return;
    };

    builder.overrides(tab.config.overrides());

    let threads = if settings.number_of_threads == 0 {
        thread::available_parallelism().map(|value| value.get()).unwrap_or(2)
    } else {
        settings.number_of_threads as usize
    };

    let search_binary = settings.search_binary;
    let walker = builder.threads(threads).build_parallel();

    std::thread::spawn(move || {
        walker.run(|| {
            let tx = tx.clone();
            let quit = quit.clone();

            let mut workers = workers.clone();

            Box::new(move |result| {
                if quit.load(Ordering::Relaxed) {
                    return WalkState::Quit;
                }

                let entry = if let Ok(entry) = result {
                    entry
                } else {
                    return WalkState::Continue;
                };

                if let Some(file_type) = entry.file_type() {
                    if !file_type.is_file() {
                        return WalkState::Continue;
                    }
                } else {
                    return WalkState::Continue;
                };

                if let Some(result) = workers[0].search_path(entry, search_binary) {
                    return match tx.send(result) {
                        Ok(_) => WalkState::Continue,
                        Err(_) => WalkState::Quit,
                    };
                } else {
                    return WalkState::Continue;
                };
            })
        });
    });
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

fn draw_menu(ui: &Ui, keep_running: &mut bool, state: &mut SearchTabs, settings: &mut SettingsWindow) {
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
        ui.menu_item_config("Hotkeys").shortcut("F1").build();
        ui.separator();
        ui.menu_item_config("About...").build();
        menu.end();
    }
}

fn draw_result(ui: &Ui, result: &UiSearchEntry) {
    const COLOR_RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
    let mut printed = 0;
    for (start, end) in result.matches.iter().map(|&val| val) {
        ui.text(result.text.get(printed..start).unwrap_or("<invalid utf8>"));
        ui.same_line_with_spacing(0.0, 0.0);
        ui.text_colored(COLOR_RED, result.text.get(start..end).unwrap_or("<invalid utf8>"));
        ui.same_line_with_spacing(0.0, 0.0);
        printed = end;
    }
    ui.text(result.text.get(printed..).unwrap_or("<invalid utf8>"));
}

fn draw_tab(ui: &Ui, state: &mut SearchTabs, tab_id: usize, mut tab: SearchTab, settings: &Settings) {
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
                .enter_returns_true(true)
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

                // How can we calculate that dynamically such that the button fits in the window?
                ui.table_next_column();
                let _w = ui.push_item_width(450.0);
                if ui
                    .input_text("##search", &mut query.query)
                    .hint("(press enter to search)")
                    .enter_returns_true(true)
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

                let add = ui.button("+");
                ui.same_line();

                if !ui.button("-") {
                    tab.config.queries.push(query);
                }

                if add {
                    tab.config.queries.push(SearchQuery::new());
                }
            }
        }

        // We always have at least 1 query line, so if they were all removed, re-create a default one.
        if tab.config.queries.is_empty() {
            tab.config.queries.push(SearchQuery::new());
        }

        ui.same_line();
        if ui.button("Search") {
            search = true;
        }

        if search {
            search_parallel(&mut tab, settings);
        }

        ui.same_line();
        if ui.button("Cancel") {
            tab.cancel_search(false);
        }

        // @Enhancement: Shouldn't calculate that every frame.
        let height_seperator = unsafe { ui.style() }.item_spacing[1];
        let footer_height = height_seperator + ui.frame_height();

        ui.separator();
        ui.child_window("##result").size([0.0, -footer_height]).build(|| {
            let clip = ListClipper::new(tab.results.len() as i32);
            let mut tok = clip.begin(ui);

            let flags = TableFlags::REORDERABLE | TableFlags::RESIZABLE | TableFlags::SIZING_STRETCH_PROP;
            if let Some(_t) = ui.begin_table_with_flags("table-headers", 3, flags) {
                ui.table_setup_column_with(TableColumnSetup { name: "File", flags: TableColumnFlags::empty(), init_width_or_weight: 0.5, user_id: Id::default() });
                ui.table_setup_column_with(TableColumnSetup { name: "Line", flags: TableColumnFlags::empty(), init_width_or_weight: 0.1, user_id: Id::default() });
                ui.table_setup_column_with(TableColumnSetup { name: "Text", flags: TableColumnFlags::empty(), init_width_or_weight: 0.0, user_id: Id::default() });
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
                            .build()
                        {
                            if !ui.io().key_ctrl {
                                // clear selected
                                let selected_rows = std::mem::replace(&mut tab.selected_rows, HashSet::new());
                                for selected_id in selected_rows.into_iter() {
                                    if let Some(entry) = tab.results.get_mut(selected_id) {
                                        entry.selected = false;
                                    }
                                }
                            }

                            if ui.io().key_shift {
                                // Select everything in between `last_selected_row` and clicked row.
                                let first = std::cmp::min(row_id, tab.last_selected_row);
                                let last = std::cmp::max(row_id, tab.last_selected_row);
                                for idx in first..=last {
                                    if idx != row_id {
                                        if tab.results[idx].selected {
                                            tab.selected_rows.remove(&idx);
                                        } else {
                                            tab.selected_rows.insert(idx);
                                        }
                                        tab.results[idx].selected = !tab.results[idx].selected;
                                    } else {
                                        tab.results[idx].selected = true;
                                        tab.selected_rows.insert(idx);
                                    }
                                }
                                // We don't update the `last_selected_row` when shift is pressed.
                            } else {
                                if tab.results[row_id].selected {
                                    tab.selected_rows.remove(&row_id);
                                } else {
                                    tab.selected_rows.insert(row_id);
                                }

                                tab.last_selected_row = row_id;
                            }

                            tab.results[row_id].selected = !tab.results[row_id].selected;
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
    let mut commands = VecDeque::new();
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
                if let Some(tab) = state.tabs.get(state.selected_tab) {
                    if !settings.settings.editor_path.is_empty() {
                        for selected_id in &tab.selected_rows {
                            if let Some(entry) = tab.results.get(*selected_id) {
                                let command = build_command(
                                    &settings.settings.editor_path,
                                    entry.path.as_ref().clone(),
                                    entry.line_number as usize,
                                );

                                if let Ok(command) = command {
                                    commands.push_back(command);
                                } else {
                                    println!("Invalid editor '{}'", settings.settings.editor_path);
                                }
                            }
                        }
                    } else {
                        println!("Editor not configured");
                    }
                }
            }

            if ui.is_key_index_released(VirtualKeyCode::F1 as i32) {
                hotkeys.toggle_open();
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
                draw_menu(ui, keep_running, &mut state, &mut settings);
            }

            let tab_flags = TabBarFlags::REORDERABLE | TabBarFlags::AUTO_SELECT_NEW_TABS;
            TabBar::new("##tabs").flags(tab_flags).build(ui, || {
                let tabs = std::mem::replace(&mut state.tabs, vec![]);
                for (tab_id, tab) in tabs.into_iter().enumerate() {
                    let _stack = ui.push_id_usize(tab_id);
                    draw_tab(ui, &mut state, tab_id, tab, &settings.settings);
                }
            });
        });
    });
}
