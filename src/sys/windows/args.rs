pub fn parse_args(cmdline: &str) -> Vec<String> {
    let mut results = Vec::new();

    if cmdline.starts_with(" ") && cmdline.starts_with("\t") {
        results.push(String::new());
    }

    let mut arg = String::new();
    let mut in_quote = false;

    // Usually, empty args are not saved, except if it's an empty arg between quotes.
    let mut save_empty_arg = false;

    // We need to save the backslash count, because the amount to save depends
    // on the characters that follow the backslashes.
    let mut backslash_count = 0;

    let mut it = cmdline.chars();
    while let Some(value) = it.next() {
        if value == '\\' {
            backslash_count += 1;
        } else if value == '"' {
            for _ in 0..(backslash_count / 2) {
                arg.push('\\');
            }

            // If there is a odd number of slashes, the quote is escaped
            if (backslash_count % 2) == 1 {
                arg.push('"');
            } else {
                in_quote = !in_quote;
                save_empty_arg = true;
            }

            backslash_count = 0;
        } else {
            for _ in 0..backslash_count {
                arg.push('\\');
            }

            backslash_count = 0;

            if value == ' ' || value == '\t' {
                if !in_quote {
                    if !arg.is_empty() || save_empty_arg {
                        results.push(std::mem::replace(&mut arg, String::new()));
                        save_empty_arg = false;
                    }
                } else {
                    arg.push(value);
                }
            } else {
                arg.push(value);
            }
        }
    }

    if !arg.is_empty() {
        results.push(arg);
    }

    return results;
}

#[cfg(test)]
mod tests {
    fn chk(cmdline: &str, expected: &[&'static str]) {
        let calculated = super::parse_args(cmdline);
        assert_eq!(calculated.len(), expected.len(), "  left: `{:?}`, right: `{:?}`\n", calculated, expected);
        for (calc, expec) in calculated.iter().zip(expected) {
            assert_eq!(&calc.as_str(), expec);
        }

        // println!("Calculted: {:?}", calculated);
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
        chk(r#"EXE a\\\b d"e f"g h"#, &["EXE", r"a\\\b", "de fg", "h"]);
        chk(r#"EXE a\\\"b c d"#, &["EXE", r#"a\"b"#, "c", "d"]);
        chk(r#"EXE a\\\\"b c" d e"#, &["EXE", r"a\\b c", "d", "e"]);
    }

    #[test]
    fn whitespace_behavior() {
        chk("test test2 ", &["test", "test2"]);
        chk("test  test2 ", &["test", "test2"]);
        chk("test ", &["test"]);
    }

    #[test]
    fn genius_quotes() {
        // chk(r#"EXE "" """#, &["EXE", "", ""]);
        // chk(r#"EXE "" """"#, &["EXE", "", r#"""#]);
        // chk(
        //     r#"EXE "this is """all""" in the same argument""#,
        //     &["EXE", r#"this is "all" in the same argument"#],
        // );
        // chk(r#"EXE "a"""#, &["EXE", r#"a""#]);
        // chk(r#"EXE "a"" a"#, &["EXE", r#"a" a"#]);
        // quotes cannot be escaped in command names
        chk(r#""EXE" check"#, &["EXE", "check"]);
        chk(r#""EXE check""#, &["EXE check"]);
        chk(r#""EXE """for""" check"#, &["EXE for check"]);
        // chk(r#""EXE \"for\" check"#, &[r"EXE \for\ check"]);
        // chk(
        //     r#""EXE \" for \" check"#,
        //     &[r"EXE \", "for", r#"""#, "check"],
        // );
        chk(r#"E"X"E test"#, &["EXE", "test"]);
        chk(r#"EX""E test"#, &["EXE", "test"]);
    }

    // from https://daviddeley.com/autohotkey/parameters/parameters.htm#WINCRULESEX
    #[test]
    fn post_2008() {
        chk("EXE CallMeIshmael", &["EXE", "CallMeIshmael"]);
        chk(r#"EXE "Call Me Ishmael""#, &["EXE", "Call Me Ishmael"]);
        chk(r#"EXE Cal"l Me I"shmael"#, &["EXE", "Call Me Ishmael"]);
        chk(r#"EXE CallMe\"Ishmael"#, &["EXE", r#"CallMe"Ishmael"#]);
        chk(r#"EXE "CallMe\"Ishmael""#, &["EXE", r#"CallMe"Ishmael"#]);
        chk(r#"EXE "Call Me Ishmael\\""#, &["EXE", r"Call Me Ishmael\"]);
        chk(r#"EXE "CallMe\\\"Ishmael""#, &["EXE", r#"CallMe\"Ishmael"#]);
        chk(r#"EXE a\\\b"#, &["EXE", r"a\\\b"]);
        chk(r#"EXE "a\\\b""#, &["EXE", r"a\\\b"]);
        chk(
            r#"EXE "\"Call Me Ishmael\"""#,
            &["EXE", r#""Call Me Ishmael""#],
        );
        chk(r#"EXE "C:\TEST A\\""#, &["EXE", r"C:\TEST A\"]);
        chk(r#"EXE "\"C:\TEST A\\\"""#, &["EXE", r#""C:\TEST A\""#]);
        chk(r#"EXE "a b c"  d  e"#, &["EXE", "a b c", "d", "e"]);
        chk(r#"EXE "ab\"c"  "\\"  d"#, &["EXE", r#"ab"c"#, r"\", "d"]);
        chk(r#"EXE a\\\b d"e f"g h"#, &["EXE", r"a\\\b", "de fg", "h"]);
        chk(r#"EXE a\\\"b c d"#, &["EXE", r#"a\"b"#, "c", "d"]);
        chk(r#"EXE a\\\\"b c" d e"#, &["EXE", r"a\\b c", "d", "e"]);
        /*
        // Double Double Quotes
        chk(r#"EXE "a b c"""#, &["EXE", r#"a b c""#]);
        chk(
            r#"EXE """CallMeIshmael"""  b  c"#,
            &["EXE", r#""CallMeIshmael""#, "b", "c"],
        );
        chk(
            r#"EXE """Call Me Ishmael""""#,
            &["EXE", r#""Call Me Ishmael""#],
        );
        chk(
            r#"EXE """"Call Me Ishmael"" b c"#,
            &["EXE", r#""Call"#, "Me", "Ishmael", "b", "c"],
        );
        */
    }
}
