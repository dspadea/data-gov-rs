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

    /// Sanitize error message for external consumption.
    ///
    /// Removes filesystem paths and other potentially sensitive information.
    pub fn sanitized_message(&self) -> String {
        let msg = self.to_string();
        msg.split_whitespace()
            .map(|word| {
                if word.starts_with('/') || word.contains(":\\") || word.starts_with("./") {
                    "[path]"
                } else {
                    word
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Type alias for Results using [`DataGovError`].
pub type Result<T> = std::result::Result<T, DataGovError>;

#[cfg(test)]
mod tests {
    use super::*;

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
