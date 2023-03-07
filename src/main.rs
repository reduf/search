// #![windows_subsystem = "windows"]

mod app;
mod args;
mod clipboard;
mod editor;
mod help;
mod hotkeys;
mod search;
mod settings;
mod support;

fn main() {
    let system = support::init("Search");
    let app = app::init();
    system.main_loop(app);
}
