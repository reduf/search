use anyhow::{bail, Result};
use crate::sys::args;
use std::{collections::HashMap, process::Command};

fn replace(argument: &str, replacements: &HashMap<String, String>) -> Result<String> {
    let mut result = String::with_capacity(argument.len());
    let mut in_open_brace = None;
    let mut it = argument.chars().enumerate();
    while let Some((idx, value)) = it.next() {
        match value {
            '\\' => {
                if let Some((_, value)) = it.next() {
                    result.push(value);
                } else {
                    bail!("Can't finish a string with a non-escaped '\\'");
                }
                continue;
            },
            '{' => {
                if in_open_brace.is_some() {
                    bail!("Can't have scoped braces");
                } else {
                    in_open_brace = Some(idx);
                }
            },
            '}' => {
                if let Some(open_pos) = in_open_brace {
                    let key = &argument[open_pos+1..idx];
                    if let Some(replacement) = replacements.get(key) {
                        result.push_str(replacement);
                    } else {
                        bail!("No replacement exist for key '{}'", key);
                    }
                    in_open_brace = None;
                } else {
                    bail!("Can't have closing brace without opening brace");
                }
            },
            _ => {
                if in_open_brace.is_none() {
                    result.push(value);
                }
            }
        }
    }

    if let Some(open_pos) = in_open_brace {
        bail!("Unclosed open brace starting at position {}", open_pos);
    }

    Ok(result)
}

pub fn build_command(editor: &str, file_path: String, line_number: usize) -> Result<Command> {
    let arguments = args::parse_args(editor);
    if let Some((editor, arguments)) = arguments.split_first() {
        let mut replacements = HashMap::new();
        replacements.insert(String::from("file"), file_path);
        replacements.insert(String::from("line"), format!("{}", line_number));

        let mut command = Command::new(editor);
        for argument in arguments.iter() {
            command.arg(replace(argument, &replacements)?);
        }

        return Ok(command);
    }

    bail!("Expected a path to a program");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_file_line_in_valid_strings() {
        let file = String::from("/home/foo/bar");
        let line = String::from("38");

        let mut replacements = HashMap::new();
        replacements.insert(String::from("file"), file.clone());
        replacements.insert(String::from("line"), line.clone());

        assert_eq!(
            replace("{file}", &replacements).unwrap(),
            file
        );
        assert_eq!(
            replace("{file}{line}", &replacements).unwrap(),
            format!("{}{}", file, line)
        );
        assert_eq!(
            replace("--line={line}", &replacements).unwrap(),
            format!("--line={}", line)
        );
        assert_eq!(
            replace("-p={file}:{line}", &replacements).unwrap(),
            format!("-p={}:{}", file, line)
        );
        assert_eq!(
            replace(r#"--json-pos=\{"line": {line}, "file": "{file}"\}"#, &replacements).unwrap(),
            format!(r#"--json-pos={{"line": {}, "file": "{}"}}"#, line, file),
        );
    }

    #[test]
    fn replace_file_in_invalid_string() {
        let mut replacements = HashMap::new();
        replacements.insert(String::from("file"), String::from("/home/"));

        replace("{file", &replacements).unwrap_err();
        replace("{{file}", &replacements).unwrap_err();
        replace("\\", &replacements).unwrap_err();
        replace("{line}", &replacements).unwrap_err();
    }

    #[test]
    fn building_command_without_editor() {
        let file = String::from("/home");
        let line = 10;
        build_command("", file.clone(), line).unwrap_err();

        let cmd = build_command("{file} {line}", file.clone(), line).unwrap();
        assert_eq!(cmd.get_program(), std::ffi::OsStr::new("{file}"));
    }

    #[test]
    fn building_valid_command_with_replacements() {
        use std::ffi::OsStr;

        let file = String::from("/home");
        let line = 10;

        let cmd = build_command("/usr/bin/editor {file} {line}", file.clone(), line).unwrap();
        assert_eq!(cmd.get_program(), OsStr::new("/usr/bin/editor"));
        let arguments: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(arguments.len(), 2);
        assert_eq!(arguments[0], OsStr::new("/home"));
        assert_eq!(arguments[1], OsStr::new("10"));

        let cmd = build_command("subl {file}:{line}", file.clone(), line).unwrap();
        assert_eq!(cmd.get_program(), OsStr::new("subl"));
        let arguments: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(arguments.len(), 1);
        assert_eq!(arguments[0], OsStr::new("/home:10"));
    }
}
