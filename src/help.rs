use imgui::*;
use indoc::indoc;

pub fn show_help(ui: &Ui, text: &str) {
    ui.same_line();
    ui.text_disabled("?");
    if ui.is_item_hovered() {
        ui.tooltip_text(text);
    }
}

pub const PATHS_USAGE: &str = indoc! { "
    A list of ';' seperated file or directory to search. Directories are searched
    recursively. File paths specified on the command line override glob and ignore
    rules.
"};

pub const GLOBS_USAGE: &str = indoc! { "
    Include or exclude files and directories for searching that match the given
    glob. This always overrides any other ignore logic. Multiple glob flags may be
    used. Globbing rules match .gitignore globs. Precede a glob with a ! to exclude
    it. If multiple globs match a file or directory, the glob given later in the
    command line takes precedence.

    As an extension, globs support specifying alternatives: *-g ab{c,d}* is
    equivalet to *-g abc -g abd*. Empty alternatives like *-g ab{,c}* are not
    currently supported. Note that this syntax extension is also currently enabled
    in gitignore files, even though this syntax isn't supported by git itself.
    ripgrep may disable this syntax extension in gitignore files, but it will
    always remain available via the -g/--glob flag.
"};

pub const SETTINGS_SEARCH_BINARY_HELP: &str = indoc! { "
    Enabling this flag will cause ripgrep to search binary files. By default,
    ripgrep attempts to automatically skip binary files in order to improve the
    relevance of results and make the search faster.

    Binary files are heuristically detected based on whether they contain a NUL
    byte or not. By default (without this flag set), once a NUL byte is seen,
    ripgrep will stop searching the file. Usually, NUL bytes occur in the beginning
    of most binary files. If a NUL byte occurs after a match, then ripgrep will
    still stop searching the rest of the file, but a warning will be printed.

"};

pub const SETTINGS_EDITOR_HELP: &str = indoc! { "
    Command line to use when using F4 which can be interpolated with:
    - {file} Path to the file
    - {line} Line of the result
"};
