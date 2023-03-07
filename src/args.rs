use anyhow::{bail, Result};

pub fn parse_args(cmdline: &str) -> Result<Vec<String>> {
    let mut results = Vec::new();

    let mut arg_builder = String::new();
    let mut in_quote = false;

    // Usually, empty args are not saved, except if it's an empty arg_builder between quotes.
    let mut save_empty_arg = false;

    let mut it = cmdline.chars();
    while let Some(value) = it.next() {
        if value == '\\' {
            if let Some(escaped_char) = it.next() {
                let need_escape = match escaped_char {
                    '"' | '\\' | ' ' | '\t' => true,
                    _ => false,
                };

                if !need_escape {
                    arg_builder.push(value);
                }

                arg_builder.push(escaped_char);
            } else {
                bail!("Expected characters to escape");
            }
        } else if value == '"' {
            in_quote = !in_quote;
            save_empty_arg = true;
        } else if value == ' ' || value == '\t' {
            if !in_quote {
                if !arg_builder.is_empty() || save_empty_arg {
                    results.push(std::mem::replace(&mut arg_builder, String::new()));
                    save_empty_arg = false;
                }
            } else {
                arg_builder.push(value);
            }
        } else {
            arg_builder.push(value);
        }
    }

    if in_quote {
        bail!("Unclosed quote");
    }

    if !arg_builder.is_empty() {
        results.push(arg_builder);
    }

    return Ok(results);
}

#[cfg(test)]
mod tests {
    fn chk(cmdline: &str, expected: &[&'static str]) {
        let calculated = super::parse_args(cmdline).unwrap();
        assert_eq!(
            calculated.len(),
            expected.len(),
            "  calculated: `{:?}`, expected: `{:?}`\n",
            calculated,
            expected
        );
        for (calc, expec) in calculated.iter().zip(expected) {
            assert_eq!(&calc.as_str(), expec);
        }
    }

    #[test]
    fn single_words() {
        chk("EXE one_word", &["EXE", "one_word"]);
        chk("EXE a", &["EXE", "a"]);
        chk("EXE 😅", &["EXE", "😅"]);
        chk("EXE 😅🤦", &["EXE", "😅🤦"]);
    }

    #[test]
    fn official_examples() {
        chk(r#"EXE "abc" d e"#, &["EXE", "abc", "d", "e"]);
        chk(r#"EXE a\\b d"e f"g h"#, &["EXE", r"a\b", "de fg", "h"]);
        chk(r#"EXE a\\\"b c d"#, &["EXE", r#"a\"b"#, "c", "d"]);
        chk(r#"EXE a\\\\"b c" d e"#, &["EXE", r"a\\b c", "d", "e"]);
    }

    #[test]
    fn invalid_examples() {
        super::parse_args(r#"EXE \"#).unwrap_err();
        super::parse_args(r#"EXE ""#).unwrap_err();
        super::parse_args(r#"EXE "fdfsd" "" ""#).unwrap_err();
    }
}
