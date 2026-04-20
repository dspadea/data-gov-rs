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

    #[test]
    fn test_sanitize_single_dot_preserved() {
        assert_eq!(sanitize_path_component("."), ".");
    }

    #[test]
    fn test_sanitize_hidden_file_prefix_preserved() {
        assert_eq!(sanitize_path_component(".bashrc"), ".bashrc");
    }

    #[test]
    fn test_sanitize_trailing_dot_preserved() {
        assert_eq!(sanitize_path_component("file."), "file.");
    }

    #[test]
    fn test_sanitize_three_dots_replaces_leading_pair() {
        assert_eq!(sanitize_path_component("..."), "_.");
    }

    #[test]
    fn test_sanitize_four_dots_replaces_both_pairs() {
        assert_eq!(sanitize_path_component("...."), "__");
    }

    #[test]
    fn test_sanitize_embedded_parent_traversal_replaced() {
        assert_eq!(sanitize_path_component("foo..bar"), "foo_bar");
    }

    #[test]
    fn test_sanitize_preserves_unicode_letters() {
        assert_eq!(sanitize_path_component("résumé"), "résumé");
        assert_eq!(sanitize_path_component("日本語"), "日本語");
    }

    #[test]
    fn test_sanitize_only_special_chars_returns_empty() {
        assert_eq!(sanitize_path_component("!@#$%^&*()"), "");
    }

    #[test]
    fn test_sanitize_long_input_does_not_panic() {
        let long = "a".repeat(10_000);
        let result = sanitize_path_component(&long);
        assert_eq!(result.len(), 10_000);
    }

    #[test]
    fn test_sanitize_mixed_safe_and_traversal() {
        assert_eq!(
            sanitize_path_component("safe-name../evil"),
            "safe-name__evil"
        );
    }
}
