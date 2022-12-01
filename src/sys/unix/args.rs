// @Cleanup: This is totally broken, but will do for now...
pub fn parse_args(cmdline: &str) -> Vec<String> {
    if cmdline.is_empty() {
        return vec![];
    }
    cmdline.split(' ').map(|fragment| String::from(fragment)).collect()
}
