use crate::help;
use anyhow::{anyhow, bail, Result};
use imgui::*;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use rfd::FileDialog;

#[derive(Serialize, Deserialize, Copy, Clone, PartialEq)]
pub enum StyleColor {
    Dark,
    Light,
    Classic,
}

impl Default for StyleColor {
    fn default() -> Self {
        Self::Dark
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[repr(transparent)]
pub struct BoolTrue(pub bool);

impl Default for BoolTrue {
    fn default() -> Self {
        Self(true)
    }
}

impl std::fmt::Display for BoolTrue {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::ops::Not for BoolTrue {
    type Output = bool;
    fn not(self) -> Self::Output {
        !self.0
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub number_of_threads: i32,
    #[serde(default)]
    pub follow_symlink: bool,
    #[serde(default)]
    pub search_binary: bool,
    #[serde(default)]
    pub editor_path: String,
    #[serde(default)]
    pub style_color: StyleColor,
    #[serde(default)]
    pub incremental_search: BoolTrue,
}

impl Settings {
    fn default_editor_path() -> &'static str {
        #[cfg(windows)]
        return "C:\\Windows\\notepad.exe {file}";
        #[cfg(not(windows))]
        return "nano +{line} {file}";
    }

    pub fn editor_path(&self) -> &str {
        if self.editor_path.is_empty() {
            return Self::default_editor_path();
        } else {
            return self.editor_path.as_str();
        }
    }
}

pub struct SettingsWindow {
    path: PathBuf,
    opened: bool,
    pub settings: Settings,
}

const SETTING_FILE_NAME: &str = "search.conf";

fn current_dir() -> Result<PathBuf> {
    let mut builder = std::env::current_exe().map_err(|err| {
        println!("Failed to get the executable path, error: {}", err);
        anyhow!("Failed to get the executable path")
    })?;

    // Remove the file from the path to the executable.
    builder.pop();
    return Ok(builder);
}

pub fn enumerate_setting_paths() -> Result<Vec<PathBuf>> {
    let mut builder = current_dir().map_err(|err| {
        println!("Failed to get the executable path, error: {}", err);
        anyhow!("Failed to get the executable path")
    })?;

    // Contains the list of potential settings files, in order in which they
    // should be read.
    let mut results = Vec::new();
    loop {
        builder.push(SETTING_FILE_NAME);
        if builder.is_file() {
            results.push(builder.clone());
        }

        builder.pop(); // Remove the file we added.

        // Remove the parent and check if there is anything else to check.
        if !builder.pop() {
            break;
        }
    }

    if results.is_empty() {
        bail!("Couldn't not find a root repository");
    }

    return Ok(results);
}

impl SettingsWindow {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            settings: Settings::default(),
            opened: false,
        }
    }

    fn update_style(style_color: StyleColor) {
        match style_color {
            StyleColor::Dark => unsafe { sys::igStyleColorsDark(std::ptr::null_mut()) },
            StyleColor::Light => unsafe { sys::igStyleColorsLight(std::ptr::null_mut()) },
            StyleColor::Classic => unsafe { sys::igStyleColorsClassic(std::ptr::null_mut()) },
        };
    }

    fn try_load_from_file(path: PathBuf) -> Result<Self> {
        let content = fs::read_to_string(path.as_path())?;
        let settings: Settings = serde_json::from_str(&content)?;
        Self::update_style(settings.style_color);
        Ok(Self {
            path,
            settings,
            opened: false,
        })
    }

    pub fn load_from_file(path: PathBuf) -> Self {
        // @Enhancement:
        // The clone here, shouldn't be necessary and force a memory allocation and copy.
        return Self::try_load_from_file(path.clone()).unwrap_or_else(|_| SettingsWindow::new(path));
    }

    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let content = serde_json::to_string_pretty(&self.settings)?;
        fs::write(path, content.as_bytes())?;
        Ok(())
    }

    pub fn open_setting() -> Self {
        if let Ok(paths) = enumerate_setting_paths() {
            for path in paths.into_iter() {
                if let Ok(settings) = Self::try_load_from_file(path) {
                    println!("Loaded settings from '{}'", settings.path.to_string_lossy());
                    return settings;
                }
            }
        }

        // When failing to load any settings, we just fallback to the current
        // directory.
        let mut path = current_dir().unwrap_or_else(|_| PathBuf::from(""));
        path.push(SETTING_FILE_NAME);
        return SettingsWindow::new(path);
    }

    pub fn save_results(&self) {
        println!("Saving settings to '{}'...", self.path.to_string_lossy());
        if self.save_to_file(self.path.as_path()).is_err() {
            // We could potentially create a Window with the serialized settings.
            println!(
                "Failed to save settings to '{}'",
                self.path.to_string_lossy()
            );
        }
    }

    pub fn open(&mut self, opened: bool) {
        self.opened = opened;
    }

    pub fn draw_settings(&mut self, ui: &Ui) {
        if !self.opened {
            return;
        }

        let display_size = ui.io().display_size;
        let settings_window_size = [750.0, 562.0];
        let pos_x = (display_size[0] / 2.0) - (settings_window_size[0] / 2.0);
        let pos_y = (display_size[1] / 2.0) - (settings_window_size[1] / 2.0);

        let window = ui
            .window("Settings")
            .size(settings_window_size, Condition::Appearing)
            .position([pos_x, pos_y], Condition::Appearing)
            .opened(&mut self.opened);

        window.build(|| {
            if let Some(_t) =
                ui.begin_table_with_flags("settings-layout", 2, TableFlags::SIZING_FIXED_FIT)
            {
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
                ui.text("Path: ");
                ui.table_next_column();
                let mut path_as_str = self.path.to_string_lossy().into_owned();
                ui.input_text("##path", &mut path_as_str)
                    .read_only(true)
                    .build();

                ui.table_next_column();
                ui.separator();
                ui.table_next_column();
                ui.separator();

                ui.table_next_column();
                ui.text("Style: ");
                ui.table_next_column();
                if ui.radio_button("Dark", &mut self.settings.style_color, StyleColor::Dark) {
                    Self::update_style(self.settings.style_color);
                }
                ui.same_line();
                if ui.radio_button("Light", &mut self.settings.style_color, StyleColor::Light) {
                    Self::update_style(self.settings.style_color);
                }
                ui.same_line();
                if ui.radio_button(
                    "Classic",
                    &mut self.settings.style_color,
                    StyleColor::Classic,
                ) {
                    Self::update_style(self.settings.style_color);
                }

                ui.table_next_column();
                ui.text("Number of threads: ");
                ui.table_next_column();
                ui.input_int("##threads", &mut self.settings.number_of_threads)
                    .build();

                ui.table_next_column();
                ui.text("Follow Symlinks: ");
                ui.table_next_column();
                ui.checkbox("##symlinks", &mut self.settings.follow_symlink);

                ui.table_next_column();
                ui.text("Search binary: ");
                ui.table_next_column();
                ui.checkbox("##binary", &mut self.settings.search_binary);
                help::show_help(ui, help::SETTINGS_SEARCH_BINARY_HELP);

                ui.table_next_column();
                ui.text("Incremental search: ");
                ui.table_next_column();
                ui.checkbox(
                    "##incremental-search",
                    &mut self.settings.incremental_search.0,
                );
                help::show_help(ui, help::SETTINGS_INCREMENTAL_SEARCH_HELP);

                ui.table_next_column();
                ui.text("Editor Path: ");
                ui.table_next_column();
                ui.input_text("##editor", &mut self.settings.editor_path)
                    .hint(Settings::default_editor_path())
                    .build();
                ui.same_line();
                if ui.button("...") {
                    let maybe_file = FileDialog::new()
                        .add_filter("Executable files", &["exe"])
                        .set_directory("/")
                        .pick_file();
                    match maybe_file {
                        Some(f) => {
                            self.settings.editor_path.clear();
                            self.settings.editor_path.push('"');
                            self.settings.editor_path.push_str(&f.as_path().display().to_string());
                            self.settings.editor_path.push('"');
                        },
                        None => {},
                    }
                }

                help::show_help(ui, help::SETTINGS_EDITOR_HELP);
            }
        });
    }
}

impl Drop for SettingsWindow {
    fn drop(&mut self) {
        self.save_results();
    }
}
