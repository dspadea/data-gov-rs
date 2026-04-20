use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use is_terminal::IsTerminal;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

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
    multi: MultiProgress,
    bars: Mutex<HashMap<String, ProgressBar>>,
}

impl CliStatusReporter {
    pub fn new(color_helper: ColorHelper) -> Self {
        let show_progress = std::env::var("NO_PROGRESS").is_err();
        let fancy_progress = show_progress
            && std::io::stdout().is_terminal()
            && std::env::var("FORCE_SIMPLE_PROGRESS").is_err();

        // When not in fancy mode, hide the MultiProgress so it doesn't eat
        // output.  We still create it so the code path is uniform.
        let multi = MultiProgress::new();
        if !fancy_progress {
            multi.set_draw_target(indicatif::ProgressDrawTarget::hidden());
        }

        Self {
            color_helper,
            show_progress,
            fancy_progress,
            multi,
            bars: Mutex::new(HashMap::new()),
        }
    }

    fn display_name(resource_name: &Option<String>, output_path: &Path) -> String {
        if let Some(name) = resource_name {
            return name.clone();
        }
        output_path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "file".to_string())
    }

    fn bar_key(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    /// Lock the progress bar map, recovering from poison if another thread panicked.
    fn lock_bars(&self) -> MutexGuard<'_, HashMap<String, ProgressBar>> {
        self.bars.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn make_bar(&self, total_bytes: Option<u64>, name: &str) -> ProgressBar {
        let pb = match total_bytes {
            Some(total) => self.multi.add(ProgressBar::new(total)),
            None => self.multi.add(ProgressBar::new_spinner()),
        };

        let template = if total_bytes.is_some() {
            "{msg} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})"
        } else {
            "{msg} {spinner:.cyan} {bytes} ({bytes_per_sec})"
        };

        pb.set_style(
            ProgressStyle::default_bar()
                .template(template)
                .unwrap_or_else(|_| ProgressStyle::default_bar())
                .progress_chars("█▉▊▋▌▍▎▏ "),
        );
        pb.set_message(name.to_string());
        pb
    }
}

impl StatusReporter for CliStatusReporter {
    fn on_download_batch(&self, event: &DownloadBatch) {
        if !self.show_progress {
            return;
        }

        let msg = match &event.dataset_name {
            Some(dataset) => format!(
                "{} {} resources for '{}'...",
                color_cyan("Downloading"),
                event.resource_count,
                dataset
            ),
            None => format!(
                "{} {} resources...",
                color_cyan("Downloading"),
                event.resource_count
            ),
        };

        if self.fancy_progress {
            if let Err(e) = self.multi.println(&msg) {
                eprintln!("{msg} (progress display error: {e})");
            }
        } else {
            println!("{msg}");
        }
    }

    fn on_download_started(&self, event: &DownloadStarted) {
        if !self.show_progress {
            return;
        }

        let name = Self::display_name(&event.resource_name, &event.output_path);

        if self.fancy_progress {
            let pb = self.make_bar(event.total_bytes, &name);
            let key = Self::bar_key(&event.output_path);
            self.lock_bars().insert(key, pb);
        } else if let Some(total) = event.total_bytes {
            println!("Downloading {} ({} bytes)...", name, total);
        } else {
            println!("Downloading {} ...", name);
        }
    }

    fn on_download_progress(&self, event: &DownloadProgress) {
        if !self.fancy_progress {
            return;
        }

        let key = Self::bar_key(&event.output_path);
        if let Some(pb) = self.lock_bars().get(&key) {
            if let Some(total) = event.total_bytes {
                pb.set_length(total);
            }
            pb.set_position(event.downloaded_bytes);
        }
    }

    fn on_download_finished(&self, event: &DownloadFinished) {
        let name = Self::display_name(&event.resource_name, &event.output_path);

        if self.fancy_progress {
            let key = Self::bar_key(&event.output_path);
            if let Some(pb) = self.lock_bars().remove(&key) {
                pb.finish_with_message(format!("{} {}", self.color_helper.green("✓"), name));
                return;
            }
        }

        if self.show_progress {
            println!("{} {}", self.color_helper.green("✓ Downloaded"), name);
        }
    }

    fn on_download_failed(&self, event: &DownloadFailed) {
        let display = event
            .resource_name
            .as_deref()
            .or_else(|| {
                event
                    .output_path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
            })
            .unwrap_or("file");

        if self.fancy_progress {
            if let Some(path) = &event.output_path {
                let key = Self::bar_key(path);
                if let Some(pb) = self.lock_bars().remove(&key) {
                    pb.abandon_with_message(format!(
                        "{} {}: {}",
                        color_red_bold("✗"),
                        display,
                        event.error
                    ));
                    return;
                }
            }
            // No bar found — fall through to plain print
            let msg = format!("{} {} ({})", color_red_bold("✗"), display, event.error);
            if let Err(e) = self.multi.println(&msg) {
                eprintln!("{msg} (progress display error: {e})");
            }
        } else {
            println!(
                "{} {} ({})",
                color_red_bold("Failed:"),
                display,
                event.error
            );
        }
    }
}
