#![windows_subsystem = "windows"]

mod app;
mod args;
mod clipboard;
mod editor;
mod help;
mod hotkeys;
mod search;
mod settings;
mod support;
mod sys;
mod stb_image;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path of the workspace in which to start the program. Default is current workspace.
    #[arg(long)]
    workspace: Option<String>,

    /// Default paths to search in. Default is workspace.
    #[arg(long)]
    paths: Option<String>,

    /// Default patterns used to filter the file names. Default is workspace.
    #[arg(long)]
    patterns: Option<String>,

    /// Path to the config file to use.
    #[arg(short, long)]
    config: Option<String>,
}

fn main() {
    let args = Args::parse();

    if let Some(workspace) = &args.workspace {
        if let Err(err) = std::env::set_current_dir(std::path::Path::new(workspace)) {
            eprintln!(
                "Failed to change to workspace '{}', err: {:?}",
                workspace, err
            );
        }
    }

    let system = support::init("Search");
    let app = app::init(args.paths, args.patterns, args.config);
    system.main_loop(app);
}
