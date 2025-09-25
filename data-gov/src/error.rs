use data_gov_ckan::CkanError;
use thiserror::Error;

/// Errors that can occur when using the Data.gov client
#[derive(Error, Debug)]
pub enum DataGovError {
    /// Error from the underlying CKAN API
    #[error("CKAN API error: {0}")]
    CkanError(#[from] CkanError),

    /// HTTP request error
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),

    /// File I/O error
    #[error("File operation failed: {0}")]
    IoError(#[from] std::io::Error),

    /// Invalid URL error
    #[error("Invalid URL: {0}")]
    UrlError(#[from] url::ParseError),

    /// Resource not found
    #[error("Resource not found: {message}")]
    ResourceNotFound { message: String },

    /// Download failed
    #[error("Download failed: {message}")]
    DownloadError { message: String },

    /// Invalid resource format
    #[error("Invalid resource format: expected {expected}, got {actual}")]
    InvalidFormat { expected: String, actual: String },

    /// Configuration error
    #[error("Configuration error: {message}")]
    ConfigError { message: String },

    /// Validation error
    #[error("Validation error: {message}")]
    ValidationError { message: String },

    /// Generic error with custom message
    #[error("{message}")]
    Other { message: String },
}

impl DataGovError {
    /// Create a new resource not found error
    pub fn resource_not_found<S: Into<String>>(message: S) -> Self {
        Self::ResourceNotFound {
            message: message.into(),
        }
    }

    /// Create a new download error
    pub fn download_error<S: Into<String>>(message: S) -> Self {
        Self::DownloadError {
            message: message.into(),
        }
    }

    /// Create a new configuration error
    pub fn config_error<S: Into<String>>(message: S) -> Self {
        Self::ConfigError {
            message: message.into(),
        }
    }

    /// Create a new validation error
    pub fn validation_error<S: Into<String>>(message: S) -> Self {
        Self::ValidationError {
            message: message.into(),
        }
    }

    /// Create a generic error with custom message
    pub fn other<S: Into<String>>(message: S) -> Self {
        Self::Other {
            message: message.into(),
        }
    }
}

/// Type alias for Results using DataGovError
pub type Result<T> = std::result::Result<T, DataGovError>;
