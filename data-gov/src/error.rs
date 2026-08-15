use data_gov_catalog::CatalogError;
use thiserror::Error;

/// Errors that can occur when using the Data.gov client.
#[derive(Error, Debug)]
pub enum DataGovError {
    /// Error from the underlying Catalog API.
    #[error("Catalog API error: {0}")]
    CatalogError(#[from] CatalogError),

    /// HTTP request error.
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),

    /// File I/O error.
    #[error("File operation failed: {0}")]
    IoError(#[from] std::io::Error),

    /// Invalid URL error.
    #[error("Invalid URL: {0}")]
    UrlError(#[from] url::ParseError),

    /// Resource not found.
    #[error("Resource not found: {message}")]
    ResourceNotFound { message: String },

    /// Download failed.
    #[error("Download failed: {message}")]
    DownloadError { message: String },

    /// Invalid resource format.
    #[error("Invalid resource format: expected {expected}, got {actual}")]
    InvalidFormat { expected: String, actual: String },

    /// Configuration error.
    #[error("Configuration error: {message}")]
    ConfigError { message: String },

    /// Validation error.
    #[error("Validation error: {message}")]
    ValidationError { message: String },

    /// Generic error with custom message.
    #[error("{message}")]
    Other { message: String },
}

impl DataGovError {
    /// Create a new resource not found error.
    pub fn resource_not_found<S: Into<String>>(message: S) -> Self {
        Self::ResourceNotFound {
            message: message.into(),
        }
    }

    /// Create a new download error.
    pub fn download_error<S: Into<String>>(message: S) -> Self {
        Self::DownloadError {
            message: message.into(),
        }
    }

    /// Create a new configuration error.
    pub fn config_error<S: Into<String>>(message: S) -> Self {
        Self::ConfigError {
            message: message.into(),
        }
    }

    /// Create a new validation error.
    pub fn validation_error<S: Into<String>>(message: S) -> Self {
        Self::ValidationError {
            message: message.into(),
        }
    }

    /// Create a generic error with a custom message.
    pub fn other<S: Into<String>>(message: S) -> Self {
        Self::Other {
            message: message.into(),
        }
    }

    /// Render this error with filesystem paths replaced by `[path]`.
    ///
    /// Use it when an error crosses a trust boundary - a log shipped off
    /// the machine, a response to a remote caller, a bug report. A path
    /// discloses the account name and the directory layout of the machine
    /// the tool ran on, neither of which the recipient needs.
    ///
    /// Every whitespace-separated token that looks like a path is
    /// replaced, wherever it appears and however it is punctuated, so a
    /// quoted or bracketed path is caught along with a bare one and the
    /// punctuation around it is kept.
    ///
    /// URLs survive, because a public catalog URL is usually the one thing
    /// that makes a failure actionable and it says nothing about the
    /// machine. `file://` URLs do not survive: those are paths wearing a
    /// scheme.
    ///
    /// Prose is left alone. `read/write` and `and/or` are not paths, so a
    /// token needs more than one separator, or an anchor such as `/`,
    /// `./`, `../`, `~/` or a drive letter, or a file extension after its
    /// only separator, before it is treated as one.
    ///
    /// Runs of whitespace in the message collapse to single spaces.
    ///
    /// # Examples
    ///
    /// ```
    /// use data_gov::DataGovError;
    ///
    /// let error = DataGovError::other("could not read /home/someone/q3 data/report.csv");
    /// assert_eq!(error.sanitized_message(), "could not read [path] [path]");
    ///
    /// let error = DataGovError::other("GET https://catalog.data.gov/search failed");
    /// assert_eq!(
    ///     error.sanitized_message(),
    ///     "GET https://catalog.data.gov/search failed"
    /// );
    /// ```
    pub fn sanitized_message(&self) -> String {
        self.to_string()
            .split_whitespace()
            .map(redact_if_path)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Characters that commonly open a quoted or bracketed path in an error
/// message. A leading one of these hid the path from the original
/// `starts_with('/')` test entirely.
const LEADING_WRAPPERS: &[char] = &['"', '\'', '`', '(', '[', '{', '<'];

/// Characters that commonly close a quoted or bracketed path, or end the
/// sentence it sits in.
const TRAILING_WRAPPERS: &[char] = &[
    '"', '\'', '`', ')', ']', '}', '>', ',', ';', ':', '.', '!', '?',
];

/// Replace `token` with `[path]` if it is a path, keeping any punctuation
/// that wraps it so the message still reads as a sentence.
fn redact_if_path(token: &str) -> String {
    let unwrapped = token.trim_start_matches(LEADING_WRAPPERS);
    let core = unwrapped.trim_end_matches(TRAILING_WRAPPERS);

    if !looks_like_a_path(core) {
        return token.to_string();
    }

    let leading = &token[..token.len() - unwrapped.len()];
    let trailing = &token[token.len() - (unwrapped.len() - core.len())..];
    format!("{leading}[path]{trailing}")
}

/// Whether `token` is a filesystem path rather than prose or a URL.
///
/// The hard part is not recognising a path. It is not redacting ordinary
/// English: `read/write` and `and/or` both carry a separator, and turning
/// them into `[path]` would make the message useless while protecting
/// nothing.
fn looks_like_a_path(token: &str) -> bool {
    if !token.contains('/') && !token.contains('\\') {
        return false;
    }

    // A URL is not a filesystem path. `file://` is the exception: it
    // carries one, and it is the shape that would otherwise slip through
    // the URL exemption.
    if let Some((scheme, _)) = token.split_once("://") {
        return scheme.eq_ignore_ascii_case("file");
    }

    // Anchored forms settle it on shape alone, whatever follows.
    const ANCHORS: &[&str] = &["/", "./", "../", "~/", ".\\", "..\\", "~\\", "\\\\"];
    if ANCHORS.iter().any(|anchor| token.starts_with(anchor)) {
        return true;
    }

    // A Windows drive letter: `C:\` or `C:/`.
    let bytes = token.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
    {
        return true;
    }

    // Unanchored, so only the shape of the remainder is left to go on.
    // Two separators is a directory tree. One separator with a file
    // extension after it is a file in a directory - and it is the tail of
    // a path that contained a space, which is how the account name used to
    // escape. One separator and no extension is prose.
    let separators = token.chars().filter(|c| *c == '/' || *c == '\\').count();
    if separators >= 2 {
        return true;
    }

    let last_segment = token.rsplit(['/', '\\']).next().unwrap_or_default();
    match last_segment.split_once('.') {
        Some((stem, extension)) => {
            !stem.is_empty()
                && !extension.is_empty()
                && extension.chars().all(|c| c.is_ascii_alphanumeric())
        }
        None => false,
    }
}

/// Type alias for Results using [`DataGovError`].
pub type Result<T> = std::result::Result<T, DataGovError>;

#[cfg(test)]
mod tests {
    use super::*;

    // --- #59: `sanitized_message` claims to remove filesystem paths, and
    // nothing held it to that claim. It had no test and no caller, so the
    // documented property was a sentence rather than a behaviour. These
    // pin it. ---

    /// A stand-in for the thing that must never reach a log or a remote
    /// consumer: the account name embedded in a home directory.
    const USERNAME: &str = "someone";

    fn sanitize(message: &str) -> String {
        DataGovError::other(message).sanitized_message()
    }

    #[test]
    fn an_absolute_path_is_replaced() {
        let sanitized = sanitize("could not read /home/someone/data/report.csv today");

        assert_eq!(sanitized, "could not read [path] today");
    }

    /// The original implementation split on whitespace and replaced only
    /// the tokens that began with a separator. A path containing a space
    /// therefore lost its first half and kept the rest, which is the half
    /// that names the file.
    #[test]
    fn a_path_containing_spaces_leaks_no_component() {
        let sanitized = sanitize("could not read /home/someone/my data/report.csv today");

        assert!(
            !sanitized.contains("report.csv"),
            "the tail of a path with a space in it must not survive, got: {sanitized}"
        );
        assert!(
            !sanitized.contains(USERNAME),
            "the account name must not survive, got: {sanitized}"
        );
    }

    /// `../secrets/key.txt` begins with neither `/` nor `./`, so the
    /// original implementation passed it through untouched.
    #[test]
    fn a_parent_relative_path_is_replaced() {
        let sanitized = sanitize("could not read ../secrets/key.txt today");

        assert_eq!(sanitized, "could not read [path] today");
    }

    #[test]
    fn a_home_relative_path_is_replaced() {
        let sanitized = sanitize("could not read ~/secrets/key.txt today");

        assert_eq!(sanitized, "could not read [path] today");
    }

    #[test]
    fn a_bare_relative_path_is_replaced() {
        let sanitized = sanitize("could not read data/private/notes.txt today");

        assert_eq!(sanitized, "could not read [path] today");
    }

    #[test]
    fn a_windows_path_is_replaced() {
        let sanitized = sanitize("could not read C:\\Users\\someone\\report.csv today");

        assert_eq!(sanitized, "could not read [path] today");
    }

    #[test]
    fn a_unc_path_is_replaced() {
        let sanitized = sanitize("could not read \\\\fileserver\\share\\report.csv today");

        assert_eq!(sanitized, "could not read [path] today");
    }

    /// Error messages quote the path they are complaining about far more
    /// often than they leave it bare, and a leading quote defeated the
    /// original `starts_with('/')` test entirely.
    #[test]
    fn a_path_wrapped_in_punctuation_is_replaced_and_keeps_its_wrapper() {
        for (message, expected) in [
            (
                "failed to open \"/home/someone/report.csv\" for writing",
                "failed to open \"[path]\" for writing",
            ),
            (
                "failed to open (/home/someone/report.csv) for writing",
                "failed to open ([path]) for writing",
            ),
            (
                "failed to open /home/someone/report.csv, giving up",
                "failed to open [path], giving up",
            ),
            (
                "failed to open '/home/someone/report.csv'.",
                "failed to open '[path]'.",
            ),
        ] {
            assert_eq!(sanitize(message), expected, "input was: {message}");
        }
    }

    /// Sanitizing must not cost the reader the one thing that usually
    /// makes a download failure actionable. A public catalog URL is not a
    /// filesystem path and discloses nothing about the machine.
    #[test]
    fn a_url_survives_sanitization() {
        let sanitized = sanitize("GET https://catalog.data.gov/api/dataset/foo failed");

        assert_eq!(
            sanitized,
            "GET https://catalog.data.gov/api/dataset/foo failed"
        );
    }

    /// The URL exemption must not become a way to smuggle a path past
    /// the redaction. `file://` carries a filesystem path and is the one
    /// scheme that has to lose.
    #[test]
    fn a_file_url_is_replaced_despite_the_url_exemption() {
        let sanitized = sanitize("could not read file:///home/someone/report.csv today");

        assert_eq!(sanitized, "could not read [path] today");
    }

    /// Redacting every token with a slash in it would turn ordinary
    /// English into `[path]` and make the message useless.
    #[test]
    fn ordinary_prose_containing_a_slash_survives() {
        let sanitized = sanitize("the file is open read/write and/or locked");

        assert_eq!(sanitized, "the file is open read/write and/or locked");
    }

    #[test]
    fn a_message_with_no_path_is_returned_unchanged() {
        let sanitized = sanitize("Validation failed: per_page must be between 1 and 1000");

        assert_eq!(
            sanitized,
            "Validation failed: per_page must be between 1 and 1000"
        );
    }

    /// The property the doc comment actually promises, checked across
    /// every shape above at once rather than one example at a time.
    #[test]
    fn no_path_shape_lets_the_account_name_through() {
        let shapes = [
            "/home/someone/report.csv",
            "/home/someone/my data/report.csv",
            "../../home/someone/report.csv",
            "~/someone/report.csv",
            "home/someone/report.csv",
            "C:\\Users\\someone\\report.csv",
            "\\\\fileserver\\someone\\report.csv",
            "\"/home/someone/report.csv\"",
            "(/home/someone/report.csv)",
            "/home/someone/report.csv.",
        ];

        for shape in shapes {
            let sanitized = sanitize(&format!("could not read {shape} today"));
            assert!(
                !sanitized.contains(USERNAME),
                "{shape:?} leaked the account name as: {sanitized}"
            );
        }
    }

    /// #77: `validation_error` had no test that called it directly -- every
    /// exercise of it went through `data_gov::util`'s checks, which
    /// construct the variant rather than call the constructor by name.
    /// It has real callers today (every download-URL and path-containment
    /// check in `util.rs`), but this pins the constructor's own contract:
    /// the variant and the message it was given, not a message some other
    /// function happened to produce.
    #[test]
    fn validation_error_builds_a_validation_error_carrying_the_given_message() {
        let err = DataGovError::validation_error("destination is outside the chosen directory");
        assert!(
            matches!(err, DataGovError::ValidationError { .. }),
            "got {err:?}"
        );
        assert!(
            err.to_string()
                .contains("destination is outside the chosen directory"),
            "got: {err}"
        );
    }
}
