//! Progress reporting for long-running downloads.
//!
//! Downloads are the one operation in this crate slow enough that a caller
//! needs to show progress, and the crate cannot know how: a terminal wants a
//! progress bar, an MCP server wants structured events, a test wants a
//! recording. So the client emits events and a consumer decides what they
//! mean.
//!
//! Implement [`StatusReporter`] and hand it to
//! [`DataGovConfig::with_status_reporter`](crate::DataGovConfig::with_status_reporter).
//! Every method has a default that does nothing, so an implementation only
//! overrides the events it cares about, and a new event added here does not
//! break an existing implementation.
//!
//! With no reporter configured, downloads run silently.
//!
//! The events fire in order: [`DownloadBatch`] once for a batch, then
//! [`DownloadStarted`], zero or more [`DownloadProgress`], and exactly one
//! of [`DownloadFinished`] or [`DownloadFailed`] per file.
//!
//! # Examples
//!
//! ```
//! use data_gov::ui::{DownloadFinished, StatusReporter};
//!
//! struct CountFinished(std::sync::atomic::AtomicUsize);
//!
//! impl StatusReporter for CountFinished {
//!     fn on_download_finished(&self, _event: &DownloadFinished) {
//!         self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
//!     }
//! }
//! ```

use std::path::PathBuf;

/// A batch of downloads is about to start.
///
/// Emitted once per call that downloads more than one file, before any
/// [`DownloadStarted`].
#[derive(Debug, Clone)]
pub struct DownloadBatch {
    /// How many files the batch will attempt.
    pub resource_count: usize,
    /// Title of the dataset the batch belongs to, where one is known.
    pub dataset_name: Option<String>,
}

/// One file's transfer has begun.
#[derive(Debug, Clone)]
pub struct DownloadStarted {
    /// Title of the distribution being fetched, where the metadata carries
    /// one.
    pub resource_name: Option<String>,
    /// Title of the dataset it belongs to, where one is known.
    pub dataset_name: Option<String>,
    /// URL being fetched.
    pub url: String,
    /// Final path the file will occupy once the transfer completes.
    ///
    /// The transfer writes to a temporary path and renames onto this one at
    /// the end, so this path holds either nothing or a whole file - never a
    /// partial download.
    pub output_path: PathBuf,
    /// Size the server declared, where it sent a `Content-Length`.
    ///
    /// `None` means the size is unknown, not that the file is empty, so a
    /// reporter should show an indeterminate indicator rather than a bar.
    pub total_bytes: Option<u64>,
}

/// A file's transfer has advanced.
///
/// Emitted repeatedly as bytes arrive. A reporter that does per-event work
/// should expect this to fire often.
#[derive(Debug, Clone)]
pub struct DownloadProgress {
    /// Title of the distribution being fetched, where the metadata carries
    /// one.
    pub resource_name: Option<String>,
    /// Title of the dataset it belongs to, where one is known.
    pub dataset_name: Option<String>,
    /// Final path the file will occupy, matching the
    /// [`DownloadStarted::output_path`] this progress belongs to.
    pub output_path: PathBuf,
    /// Bytes received so far.
    pub downloaded_bytes: u64,
    /// Size the server declared, or `None` where it sent no
    /// `Content-Length`.
    pub total_bytes: Option<u64>,
}

/// A file arrived whole and is now at its final path.
#[derive(Debug, Clone)]
pub struct DownloadFinished {
    /// Title of the distribution that was fetched, where the metadata
    /// carries one.
    pub resource_name: Option<String>,
    /// Title of the dataset it belongs to, where one is known.
    pub dataset_name: Option<String>,
    /// Path the completed file now occupies.
    pub output_path: PathBuf,
}

/// A file's transfer did not complete.
///
/// Any partial data has already been discarded, so nothing is left at
/// [`Self::output_path`] from this attempt.
#[derive(Debug, Clone)]
pub struct DownloadFailed {
    /// Title of the distribution that was being fetched, where the metadata
    /// carries one.
    pub resource_name: Option<String>,
    /// Title of the dataset it belongs to, where one is known.
    pub dataset_name: Option<String>,
    /// Path the file would have occupied, where the failure happened late
    /// enough for one to have been chosen.
    pub output_path: Option<PathBuf>,
    /// What went wrong, rendered for a person to read.
    pub error: String,
}

/// Receives download progress events.
///
/// Every method does nothing by default, so implement only the events you
/// need. Implementations are called from async tasks and may be called
/// concurrently for different files, which is why `Send + Sync` is required.
///
/// A method that blocks stalls the transfer that called it, so do the
/// minimum here and send anything expensive elsewhere.
pub trait StatusReporter: Send + Sync {
    /// A batch of downloads is about to start.
    fn on_download_batch(&self, _event: &DownloadBatch) {}
    /// One file's transfer has begun.
    fn on_download_started(&self, _event: &DownloadStarted) {}
    /// A file's transfer has advanced. Fires repeatedly.
    fn on_download_progress(&self, _event: &DownloadProgress) {}
    /// A file arrived whole and is at its final path.
    fn on_download_finished(&self, _event: &DownloadFinished) {}
    /// A file's transfer did not complete.
    fn on_download_failed(&self, _event: &DownloadFailed) {}
}
