use data_gov::DataGovClient;
use data_gov::catalog::models::SearchHit;

use super::{
    color_blue, color_blue_bold, color_bold, color_dimmed, color_green, color_green_bold,
    color_yellow, color_yellow_bold,
};

/// Right-pad `text` to `width` visible characters, *then* apply `colorize`.
///
/// Padding must happen before colorizing. `colored`'s ANSI escape bytes
/// count toward Rust's `{:width$}` field width, so formatting an
/// already-colored string is always short of its target column by the
/// escape length — coloring `cmd` and then padding it to 30 lands its
/// description at column 10 for a short command and 22-34 for a longer
/// one, four different left edges depending on the command's own length.
///
/// Split out from [`padded_green_bold`] so the ordering can be tested
/// directly: `color_green_bold` no-ops unless the process-global
/// `COLOR_HELPER` has been set (which only happens once, in production
/// `run()`, and can't be set from a test in another module), so a test
/// calling `padded_green_bold` itself can never observe real ANSI bytes
/// and can never catch this specific bug. A test can still supply its own
/// `colorize` closure that really colorizes.
fn pad_then_colorize(text: &str, width: usize, colorize: impl FnOnce(&str) -> String) -> String {
    colorize(&format!("{text:<width$}"))
}

fn padded_green_bold(text: &str, width: usize) -> String {
    pad_then_colorize(text, width, color_green_bold)
}

/// Print dataset details (shared between REPL and CLI modes).
pub fn print_package_details(hit: &SearchHit) {
    println!("\n{}", color_blue_bold("📦 Dataset Details"));
    if let Some(slug) = &hit.slug {
        println!("{}: {}", color_bold("Slug"), color_yellow(slug));
    }

    if let Some(title) = &hit.title {
        println!("{}: {}", color_bold("Title"), title);
    }

    if let Some(description) = &hit.description {
        println!("\n{}: ", color_bold("Description"));
        println!("{}", color_dimmed(description));
    }

    if let Some(dcat) = &hit.dcat
        && let Some(license) = &dcat.license
    {
        println!("\n{}: {}", color_bold("License"), color_green(license));
    }

    if let Some(dcat) = &hit.dcat
        && let Some(contact) = &dcat.contact_point
        && let Some(name) = &contact.fn_
    {
        println!("{}: {}", color_bold("Contact"), name);
    }

    if let Some(org) = &hit.organization
        && let Some(name) = &org.name
    {
        println!("{}: {}", color_bold("Organization"), name);
    }

    let distributions = hit
        .dcat
        .as_ref()
        .map(DataGovClient::get_downloadable_distributions)
        .unwrap_or_default();

    if !distributions.is_empty() {
        println!(
            "\n{} {} downloadable distributions:",
            color_bold("📁"),
            distributions.len()
        );

        for (i, dist) in distributions.iter().enumerate() {
            let title = dist.title.as_deref().unwrap_or("Unnamed");
            let format = dist
                .format
                .as_deref()
                .or(dist.media_type.as_deref())
                .unwrap_or("Unknown");

            println!(
                "  {}. {} {}",
                color_blue_bold(&i.to_string()),
                color_yellow(title),
                color_green(&format!("[{format}]")),
            );

            if let Some(desc) = &dist.description
                && !desc.is_empty()
            {
                let truncated = if desc.chars().count() > 80 {
                    let s: String = desc.chars().take(80).collect();
                    format!("{s}...")
                } else {
                    desc.clone()
                };
                println!("     {}", color_dimmed(&truncated));
            }
        }

        if let Some(slug) = &hit.slug {
            println!(
                "\n{} Use 'data-gov download {}' to download all distributions",
                color_bold("💡"),
                color_yellow(slug)
            );
            println!(
                "{} Use 'data-gov download {} <index|name>' to download by index or name",
                color_bold("💡"),
                color_yellow(slug)
            );
        }
    } else {
        println!(
            "\n{} No downloadable distributions found",
            color_yellow("⚠️")
        );
    }

    println!();
}

/// Print help for CLI mode.
pub fn print_cli_help() {
    println!("\n{}", color_blue_bold("📚 CLI Mode Commands"));
    println!();

    let commands = vec![
        (
            "search <query> [limit]",
            "Search for datasets (filtered by active org)",
            "search \"climate data\" 20",
        ),
        (
            "show [dataset_slug|.]",
            "Show dataset info ('.' or omitted means current dataset)",
            "show electric-vehicle-population-data",
        ),
        (
            "download [dataset] [selectors...]",
            "Download distributions (by index or title)",
            "download electric-vehicle-population-data 0",
        ),
        (
            "cd <path>",
            "Navigate to an org or dataset (validated against the catalog)",
            "cd /nasa-gov",
        ),
        (
            "ls",
            "List the contents of the current location (orgs / datasets / distributions)",
            "ls",
        ),
        ("info", "Show client and session information", "info"),
    ];

    for (cmd, desc, example) in commands {
        println!("{} {}", padded_green_bold(cmd, 30), desc);
        println!(
            "{:30} {}: data-gov {}",
            "",
            color_dimmed("Example"),
            color_blue(example)
        );
        println!();
    }

    println!("{}", color_yellow_bold("💡 Interactive Mode:"));
    println!(
        "  Run without arguments to start interactive REPL: {}",
        color_blue("data-gov")
    );
    println!();
}

/// Print help for REPL mode.
pub fn print_repl_help() {
    println!("\n{}", color_blue_bold("📚 Available Commands"));
    println!();

    let commands = vec![
        (
            "search <query> [limit]",
            "Search datasets (filtered by active org)",
            "search climate data 20",
        ),
        (
            "show [dataset_slug]",
            "Show dataset info (uses active dataset)",
            "show electric-vehicle-population-data",
        ),
        (
            "download [dataset] [selectors...]",
            "Download distributions (by index or title)",
            "download electric-vehicle-population-data 0",
        ),
        (
            "cd <path>",
            "Navigate to an org or dataset (validated against the catalog)",
            "cd /epa-gov",
        ),
        (
            "ls",
            "List orgs (at root), datasets (at /<org>), or distributions (at /<org>/<dataset>)",
            "ls",
        ),
        (
            "next",
            "Fetch the next page of the most recent search or ls (alias: 'n')",
            "next",
        ),
        (
            "lcd <path>",
            "Set local download directory",
            "lcd ./downloads",
        ),
        ("info", "Show session and client info", "info"),
        ("help", "Show this help message", "help"),
        ("quit", "Exit the REPL", "quit"),
    ];

    for (cmd, desc, example) in commands {
        println!("{} {}", padded_green_bold(cmd, 25), desc);
        println!(
            "{:25} {}: {}",
            "",
            color_dimmed("Example"),
            color_blue(example)
        );
        println!();
    }

    println!("{}", color_yellow_bold("💡 Pro Tips:"));
    println!(
        "  • Use short commands: {} for search, {} for show, {} for download",
        color_green("s"),
        color_green("d"),
        color_green("dl")
    );
    println!(
        "  • Navigate like a filesystem: {}, {}, {}",
        color_blue("cd epa-gov"),
        color_blue("cd air-quality"),
        color_blue("cd ..")
    );
    println!(
        "  • Or jump directly: {}, {}, {}",
        color_blue("cd /epa-gov/air-quality"),
        color_blue("cd /nasa-gov"),
        color_blue("cd /")
    );
    println!(
        "  • Then just: {}, {}, {}",
        color_blue("show"),
        color_blue("download 0"),
        color_blue("search pollution")
    );
    println!(
        "  • Download multiple distributions: {}",
        color_blue("download \"RDF File\" \"XML File\"")
    );
    println!(
        "  • Aliases: {} = {}, {} = {}",
        color_green("select"),
        color_green("cd"),
        color_green("setdir"),
        color_green("lcd")
    );
    println!("  • Downloads are organized by dataset slug in subdirectories");
    println!(
        "  • Create scripts with {} for automation",
        color_blue("#!/usr/bin/env data-gov")
    );
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use colored::Colorize;

    /// Strip `colored`'s SGR escape sequences (`\x1b[...m`) so a test can
    /// assert on the text a terminal would actually display.
    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\u{1b}' {
                for c2 in chars.by_ref() {
                    if c2 == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    /// `colored::control::SHOULD_COLORIZE` is process-global; serialize on
    /// this lock so concurrently-running tests can't see each other's
    /// override, and clean up afterward.
    static COLOR_OVERRIDE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn pad_then_colorize_pads_to_the_requested_visible_width_when_actually_colorized() {
        let _guard = COLOR_OVERRIDE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        colored::control::set_override(true);

        let result = pad_then_colorize("cd", 10, |s| s.to_string().green().bold().to_string());

        colored::control::unset_override();

        assert_ne!(
            result.chars().count(),
            10,
            "sanity check: the colorized string must actually carry escape bytes"
        );
        let visible = strip_ansi(&result);
        assert_eq!(
            visible.chars().count(),
            10,
            "visible (escapes stripped) text was {visible:?}, raw was {result:?} \
             — padding must happen before colorizing, not after"
        );
        assert_eq!(visible.trim_end(), "cd");
    }

    #[test]
    fn pad_then_colorize_does_not_truncate_text_longer_than_width() {
        let result = pad_then_colorize("a-very-long-command-name", 5, |s| s.to_string());
        assert!(
            result.starts_with("a-very-long-command-name"),
            "result was: {result:?}"
        );
    }

    #[test]
    fn padded_green_bold_pads_to_the_requested_width_when_uncolorized() {
        // Under `cargo test`, COLOR_HELPER is never set, so
        // color_green_bold is a pass-through — this exercises the
        // production wrapper end to end for the plain-text case; the
        // colorized case is covered above via pad_then_colorize directly.
        let s = padded_green_bold("cd", 10);
        assert_eq!(s.chars().count(), 10, "padded string was: {s:?}");
    }
}
