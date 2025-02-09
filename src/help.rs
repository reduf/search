use imgui::*;
use indoc::indoc;

pub fn show_help(ui: &Ui, text: &str) {
    ui.same_line();
    ui.text_disabled("?");
    if ui.is_item_hovered() {
        ui.tooltip_text(text);
    }
}

pub const PATHS_USAGE: &str = indoc! {"
    A list of ';' separated file or directory to search. Directories are searched
    recursively. File paths specified on the command line override glob and ignore
    rules.
"};

pub const GLOBS_USAGE: &str = indoc! {"
    Include or exclude files and directories for searching that match the given
    glob. This always overrides any other ignore logic. Multiple glob flags may be
    used. Globbing rules match .gitignore globs. Precede a glob with a ! to exclude
    it. If multiple globs match a file or directory, the glob given later in the
    command line takes precedence.

    As an extension, globs support specifying alternatives: *-g ab{c,d}* is
    equivalent to *-g abc -g abd*. Empty alternatives like *-g ab{,c}* are not
    currently supported.
"};

pub const SETTINGS_SEARCH_BINARY_HELP: &str = indoc! {"
    Enabling this flag will cause \"search\" to search binary files. By default,
    \"search\" attempts to automatically skip binary files in order to improve the
    relevance of results and make the search faster.

    Binary files are heuristically detected based on whether they contain a NUL
    byte or not. By default (without this flag set), once a NUL byte is seen,
    \"search\" will stop searching the file. Usually, NUL bytes occur in the beginning
    of most binary files.
"};

pub const SETTINGS_SEARCH_HIDDEN_HELP: &str = indoc! {"
    Search hidden files and directories. By default, hidden files and directories
    are skipped.

    A file or directory is considered hidden if its base name starts with a dot
    character ('.'). On operating systems which support a `hidden` file attribute,
    like Windows, files with this attribute are also considered hidden.
"};

pub const SETTINGS_INCREMENTAL_SEARCH_HELP: &str = indoc! {"
    Enabling this flag causes search to start every time the text input is updated.
    Disabling this flag causes search to only be started interactively, triggered
    with enter or by clicking the search button.
"};

pub const SETTINGS_EDITOR_HELP: &str = indoc! {"
    Editor use when double clicking or using F4. The 'System' config will try
    to use the system defined editor, and the custom allows you to specify a
    command line which can be interpolated with:
    - {file} Path to the file
    - {line} Line of the result
"};

pub const SETTINGS_ONLY_SHOW_FILENAME_HELP: &str = indoc! {"
    Only show the filename in the path column. Hovering the row will show the
    full path of the file.
"};
