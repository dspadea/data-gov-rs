/// Sanitize a string for use as a single filesystem path component.
///
/// Removes path traversal sequences (`..`, `/`, `\`) and filters to
/// alphanumeric characters plus `-`, `_`, and `.`.
// Three distinct patterns (".." then two separators); collapsing into a
// single `replace` would change behavior since `..` must be handled first.
#[allow(clippy::collapsible_str_replace)]
pub fn sanitize_path_component(s: &str) -> String {
    s.replace("..", "_")
        .replace('/', "_")
        .replace('\\', "_")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_removes_path_traversal() {
        assert_eq!(
            sanitize_path_component("../../etc/passwd"),
            "____etc_passwd"
        );
    }

    #[test]
    fn test_sanitize_removes_backslash() {
        assert_eq!(sanitize_path_component("foo\\bar"), "foo_bar");
    }

    #[test]
    fn test_sanitize_preserves_safe_chars() {
        assert_eq!(
            sanitize_path_component("my-dataset_2024.csv"),
            "my-dataset_2024.csv"
        );
    }

    #[test]
    fn test_sanitize_strips_special_chars() {
        assert_eq!(sanitize_path_component("hello world!@#"), "helloworld");
    }

    #[test]
    fn test_sanitize_empty_string() {
        assert_eq!(sanitize_path_component(""), "");
    }
}
