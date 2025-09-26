use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct DownloadBatch {
    pub resource_count: usize,
    pub dataset_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DownloadStarted {
    pub resource_name: Option<String>,
    pub dataset_name: Option<String>,
    pub url: String,
    pub output_path: PathBuf,
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct DownloadProgress {
    pub resource_name: Option<String>,
    pub dataset_name: Option<String>,
    pub output_path: PathBuf,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct DownloadFinished {
    pub resource_name: Option<String>,
    pub dataset_name: Option<String>,
    pub output_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct DownloadFailed {
    pub resource_name: Option<String>,
    pub dataset_name: Option<String>,
    pub output_path: Option<PathBuf>,
    pub error: String,
}

pub trait StatusReporter: Send + Sync {
    fn on_download_batch(&self, _event: &DownloadBatch) {}
    fn on_download_started(&self, _event: &DownloadStarted) {}
    fn on_download_progress(&self, _event: &DownloadProgress) {}
    fn on_download_finished(&self, _event: &DownloadFinished) {}
    fn on_download_failed(&self, _event: &DownloadFailed) {}
}
