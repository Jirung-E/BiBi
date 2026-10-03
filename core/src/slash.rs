/// Only a single leading slash and a command-shaped first token are commands.
/// Absolute paths and //escaped text remain ordinary input.
pub fn parse(text: &str) -> Option<(&str, &str)> {
    let text = text.trim().strip_prefix('/')?;
    let (name, args) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_alphanumeric() || "_-:.@".contains(c))
    {
        return None;
    }
    Some((name, args.trim_start()))
}
#[cfg(test)]
mod tests {
    use super::parse;
    #[test]
    fn commands_preserve_arguments_and_do_not_capture_paths() {
        assert_eq!(
            parse("/review   --branch feature/a"),
            Some(("review", "--branch feature/a"))
        );
        for text in ["/tmp/project", "C:\\project", "//compact", "ordinary"] {
            assert_eq!(parse(text), None);
        }
    }
}
