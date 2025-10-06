use indicatif::{ProgressBar, ProgressStyle};
use is_terminal::IsTerminal;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use data_gov::ui::{
    DownloadBatch, DownloadFailed, DownloadFinished, DownloadProgress, DownloadStarted,
    StatusReporter,
};

use super::colors::ColorHelper;
use super::{color_cyan, color_red_bold};

pub struct CliStatusReporter {
    color_helper: ColorHelper,
    show_progress: bool,
    fancy_progress: bool,
    progress: Mutex<HashMap<String, ProgressBar>>,
    print_lock: Mutex<()>,
}

impl CliStatusReporter {
    pub fn new(color_helper: ColorHelper) -> Self {
        let show_progress = std::env::var("NO_PROGRESS").is_err();
        let fancy_progress = show_progress
            && std::io::stdout().is_terminal()
            && std::env::var("FORCE_SIMPLE_PROGRESS").is_err();

        Self {
            color_helper,
            show_progress,
            fancy_progress,
            progress: Mutex::new(HashMap::new()),
            print_lock: Mutex::new(()),
        }
    }

    fn display_name(&self, resource_name: &Option<String>, output_path: &Path) -> String {
        if let Some(name) = resource_name {
            return name.clone();
        }

        output_path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "file".to_string())
    }

    fn progress_key(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }
}

impl StatusReporter for CliStatusReporter {
    fn on_download_batch(&self, event: &DownloadBatch) {
        if !self.show_progress {
            return;
        }

        let _guard = self.print_lock.lock().unwrap();
        match &event.dataset_name {
            Some(dataset) => println!(
                "{} {} resources for '{}'...",
                color_cyan("Downloading"),
                event.resource_count,
                dataset
            ),
            None => println!(
                "{} {} resources...",
                color_cyan("Downloading"),
                event.resource_count
            ),
        }
    }

    fn on_download_started(&self, event: &DownloadStarted) {
        if !self.show_progress {
            return;
        }

        let name = self.display_name(&event.resource_name, &event.output_path);

        if self.fancy_progress {
            let pb = match event.total_bytes {
                Some(total) => ProgressBar::new(total),
                None => ProgressBar::new_spinner(),
            };

            let template = if event.total_bytes.is_some() {
                "{msg} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})"
            } else {
                "{msg} [{spinner:.cyan/blue}] {bytes} ({bytes_per_sec})"
            };

            pb.set_style(
                ProgressStyle::default_bar()
                    .template(template)
                    .unwrap_or_else(|_| ProgressStyle::default_bar())
                    .progress_chars("█▉▊▋▌▍▎▏ "),
            );
            pb.set_message(format!("Downloading {}", name));

            let key = Self::progress_key(&event.output_path);
            self.progress.lock().unwrap().insert(key, pb);
        } else {
            let _guard = self.print_lock.lock().unwrap();
            if let Some(total) = event.total_bytes {
                println!("Downloading {} ({} bytes)...", name, total);
            } else {
                println!("Downloading {} ...", name);
            }
        }
    }

    fn on_download_progress(&self, event: &DownloadProgress) {
        if !self.fancy_progress {
            return;
        }

        let key = Self::progress_key(&event.output_path);
        if let Some(pb) = self.progress.lock().unwrap().get(&key) {
            if let Some(total) = event.total_bytes {
                pb.set_length(total);
            }
            pb.set_position(event.downloaded_bytes);
        }
    }

    fn on_download_finished(&self, event: &DownloadFinished) {
        let name = self.display_name(&event.resource_name, &event.output_path);

        if self.fancy_progress {
            let key = Self::progress_key(&event.output_path);
            if let Some(pb) = self.progress.lock().unwrap().remove(&key) {
                pb.finish_with_message(format!("Downloaded {}", name));
                return;
            }
        }

        if self.show_progress {
            let _guard = self.print_lock.lock().unwrap();
            println!("{} {}", self.color_helper.green("✓ Downloaded"), name);
        }
    }

    fn on_download_failed(&self, event: &DownloadFailed) {
        let name = event
            .output_path
            .as_ref()
            .map(|p| Self::progress_key(p.as_path()))
            .unwrap_or_else(|| {
                event
                    .resource_name
                    .clone()
                    .unwrap_or_else(|| "file".to_string())
            });

        if self.fancy_progress
            && let Some(path) = &event.output_path {
                let key = Self::progress_key(path);
                if let Some(pb) = self.progress.lock().unwrap().remove(&key) {
                    pb.abandon_with_message(format!("Failed {}: {}", name, event.error));
                    return;
                }
            }

        let _guard = self.print_lock.lock().unwrap();
        println!("{} {} ({})", color_red_bold("Failed:"), name, event.error);
    }
}
