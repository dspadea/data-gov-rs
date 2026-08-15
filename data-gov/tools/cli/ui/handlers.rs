use std::collections::HashSet;
use std::path::{Path, PathBuf};

use data_gov::catalog::models::Distribution;
use data_gov::util::{join_inside, sanitize_path_component};
use data_gov::{DataGovClient, OperatingMode};
use tokio::runtime::Runtime;

use super::commands::{ListingCursor, ReplCommand, SessionContext};
use super::display::{print_cli_help, print_package_details};
use super::output::{errln, outln};
use super::{
    color_blue, color_blue_bold, color_bold, color_cyan, color_dimmed, color_green,
    color_green_bold, color_red, color_red_err, color_yellow, color_yellow_bold,
};

/// Resolve a dataset slug from the command or fall back to session context.
fn resolve_dataset<'a>(
    explicit: &'a Option<String>,
    ctx: &'a SessionContext,
) -> Result<&'a str, &'static str> {
    match explicit.as_deref() {
        // `.` is an alias for "current dataset" — like in any unix shell.
        Some(".") => ctx
            .dataset
            .as_deref()
            .ok_or("'.' refers to the current dataset, but no dataset is selected"),
        Some(slug) => Ok(slug),
        None => ctx
            .dataset
            .as_deref()
            .ok_or("no dataset specified and none selected (use: cd /<slug>)"),
    }
}

/// Execute a command (shared between REPL and CLI modes).
///
/// The `ctx` is updated in place by `select` commands. Other commands read
/// from it to fill in omitted arguments.
pub fn execute_command(
    client: &DataGovClient,
    rt: &Runtime,
    command: ReplCommand,
    ctx: &mut SessionContext,
) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        ReplCommand::Search { query, limit } => {
            handle_search(client, rt, &query, limit, ctx)?;
        }

        ReplCommand::Show { dataset_id } => {
            let slug = resolve_dataset(&dataset_id, ctx)?;
            handle_show(client, rt, slug)?;
        }

        ReplCommand::Download { args } => {
            handle_download(client, rt, &args, ctx)?;
        }

        ReplCommand::List { what } => {
            handle_list(client, rt, ctx, what.as_deref())?;
        }

        ReplCommand::Next => {
            handle_next(client, rt, ctx)?;
        }

        ReplCommand::Select { path } => {
            handle_select(client, rt, ctx, &path)?;
        }

        ReplCommand::Info => {
            handle_info(client, ctx);
        }

        ReplCommand::SetDir { .. } => {
            return Err("lcd is only available in interactive REPL mode".into());
        }

        ReplCommand::Help => {
            print_cli_help();
        }

        ReplCommand::Quit => {
            // Not applicable in CLI mode
        }
    }

    Ok(())
}

/// Handle select/cd command.
fn handle_select(
    client: &DataGovClient,
    rt: &Runtime,
    ctx: &mut SessionContext,
    path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Single-segment paths (absolute `/foo`, or relative `foo` from root) are
    // ambiguous in data.gov's flat slug namespace — `foo` could be an
    // organization OR a dataset. The string-only `apply_navigate` always
    // treats them as orgs; here we do the actual catalog lookup to
    // disambiguate.
    if let Some(slug) = ambiguous_single_segment(ctx, path) {
        return resolve_single_segment_cd(client, rt, ctx, slug);
    }

    // For everything else, parse locally to a candidate context, then verify
    // the candidate exists in the catalog before adopting it. Validating
    // before applying means a failed `cd` leaves the user where they were.
    let mut candidate = ctx.clone();
    candidate.apply_navigate(path)?;
    validate_candidate_exists(client, rt, &candidate)?;
    candidate.last_listing = None;
    *ctx = candidate;
    print_select_result(ctx);
    Ok(())
}

/// If `path` is a single segment whose semantics are ambiguous between org
/// and dataset, return that segment. Trailing slashes are tolerated.
///
/// The two ambiguous cases:
/// - `/<seg>` — absolute, single segment
/// - `<seg>` — relative, when no org is currently set (so it would otherwise
///   be parsed as an org by [`SessionContext::apply_relative`]).
fn ambiguous_single_segment<'a>(ctx: &SessionContext, path: &'a str) -> Option<&'a str> {
    if let Some(rest) = path.strip_prefix('/') {
        let inner = rest.trim_end_matches('/');
        if inner.is_empty() || inner.contains('/') {
            return None;
        }
        return Some(inner);
    }
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() || trimmed == ".." || trimmed == "." || trimmed.contains('/') {
        return None;
    }
    if ctx.org.is_some() {
        // At org level, a relative single segment is unambiguously a dataset.
        return None;
    }
    Some(trimmed)
}

/// Resolve a single-segment `cd` against the live catalog: try as an org
/// first (cheap — one bulk call), fall back to a dataset slug lookup. If
/// the segment matches a dataset, populate the org context from the
/// dataset's publishing organization so the prompt and downstream commands
/// have a complete location.
fn resolve_single_segment_cd(
    client: &DataGovClient,
    rt: &Runtime,
    ctx: &mut SessionContext,
    slug: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let orgs = rt.block_on(client.list_organizations(None))?;
    if orgs.iter().any(|s| s == slug) {
        ctx.org = Some(slug.to_string());
        ctx.dataset = None;
        ctx.last_listing = None;
        print_select_result(ctx);
        return Ok(());
    }

    match rt.block_on(client.get_dataset(slug)) {
        Ok(hit) => {
            ctx.org = hit.organization.as_ref().and_then(|o| o.slug.clone());
            ctx.dataset = Some(slug.to_string());
            ctx.last_listing = None;
            print_select_result(ctx);
            Ok(())
        }
        Err(_) => Err(format!(
            "'{slug}' matches no organization or dataset (run `ls` to see what's at the current level)"
        )
        .into()),
    }
}

/// Verify that the candidate context names entities that actually exist.
/// `dataset_by_slug` already verifies the slug matches (so we can trust
/// `Ok` here means it exists); the org check is a single membership test
/// against the bulk organizations list.
fn validate_candidate_exists(
    client: &DataGovClient,
    rt: &Runtime,
    candidate: &SessionContext,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(slug) = candidate.dataset.as_deref() {
        let hit = rt
            .block_on(client.get_dataset(slug))
            .map_err(|_| format!("dataset '{slug}' not found"))?;

        if let Some(expected_org) = candidate.org.as_deref() {
            let actual_org = hit.organization.as_ref().and_then(|o| o.slug.as_deref());
            if let Some(actual) = actual_org
                && actual != expected_org
            {
                return Err(format!(
                    "dataset '{slug}' belongs to organization '{actual}', not '{expected_org}'"
                )
                .into());
            }
        }
        return Ok(());
    }

    if let Some(org) = candidate.org.as_deref() {
        let orgs = rt.block_on(client.list_organizations(None))?;
        if !orgs.iter().any(|o| o == org) {
            return Err(format!("organization '{org}' not found").into());
        }
    }

    Ok(())
}

fn print_select_result(ctx: &SessionContext) {
    let label = ctx.prompt_label();
    if label.is_empty() {
        outln!("{} Selection cleared", color_green_bold("OK"));
    } else {
        outln!(
            "{} Active context: {}",
            color_green_bold("OK"),
            color_yellow_bold(&label)
        );
    }
}

/// Default page size for `search` and `ls` listings when the user
/// doesn't specify one. Each command's underlying API tops out around
/// 1000 results per page; 50 is small enough to fit comfortably in a
/// terminal viewport while still being useful for scripting.
const DEFAULT_PAGE_SIZE: i32 = 50;

/// Handle search command. Renders all returned hits (no artificial
/// display cap), and stashes the next-page cursor on the session
/// context so a subsequent `next` can advance.
fn handle_search(
    client: &DataGovClient,
    rt: &Runtime,
    query: &str,
    limit: Option<i32>,
    ctx: &mut SessionContext,
) -> Result<(), Box<dyn std::error::Error>> {
    let org = ctx.org.clone();
    let effective_limit = limit.unwrap_or(DEFAULT_PAGE_SIZE);

    // `limit` is `Some` only when `ReplCommand::from_parts` consumed a
    // trailing numeric token as the limit (#54) — the *only* way it can
    // be `Some` at all. That rule is unambiguous and intentional, but
    // silent: "search route 66" and "search area 51" quietly become a
    // one-word search with a page-size limit, and nothing before this
    // said so. Surface the decision every time it fires, live-confirmed
    // cases included, so a query that looks like it ends in a number
    // doesn't vanish without explanation.
    if let Some(applied_limit) = limit {
        outln!(
            "{} {}",
            color_dimmed("Note:"),
            color_dimmed(&trailing_limit_notice(query, applied_limit))
        );
    }

    if let Some(org_name) = org.as_deref() {
        outln!(
            "{} '{}' in org {}...",
            color_cyan("Searching for"),
            query,
            color_yellow(org_name)
        );
    } else {
        outln!("{} '{}'...", color_cyan("Searching for"), query);
    }

    let page = rt.block_on(client.search(query, Some(effective_limit), None, org.as_deref()))?;
    print_search_hits(&page.results);
    summarize_listing(
        page.results.len(),
        page.after.as_deref(),
        "results",
        &client.config().mode,
    );

    ctx.last_listing = page.after.map(|after| ListingCursor::SearchResults {
        query: query.to_string(),
        organization: org,
        after,
        page_size: effective_limit,
    });

    Ok(())
}

/// Build the advisory line shown when `ReplCommand::from_parts` consumed a
/// trailing numeric token as the result limit, so a query that reads as
/// ending in a number (`route 66`, `area 51`, `catch 22`) doesn't silently
/// become a shorter, numbered search with no explanation. Names both the
/// query actually searched and the limit applied, and points at the
/// quoting escape hatch that searches the phrase literally.
fn trailing_limit_notice(query: &str, limit: i32) -> String {
    format!(
        "trailing number treated as limit -- searching \"{query}\" with limit {limit} \
         (quote the whole phrase to search literally, e.g. \"{query} {limit}\")"
    )
}

/// Render search hits in a compact list with an optional truncated
/// description. Caller decides how many to show — there's no hard
/// display cap.
fn print_search_hits(hits: &[data_gov::catalog::models::SearchHit]) {
    for hit in hits {
        let slug = hit.slug.as_deref().unwrap_or("(no-slug)");
        outln!(
            "{} {}",
            color_yellow_bold(slug),
            color_dimmed(hit.title.as_deref().unwrap_or(""))
        );

        if let Some(description) = &hit.description {
            let truncated = if description.chars().count() > 100 {
                let s: String = description.chars().take(100).collect();
                format!("{s}...")
            } else {
                description.clone()
            };
            outln!("   {}", color_dimmed(&truncated));
        }
    }
}

/// Build the `Found N <unit>` line, with a `next` hint appended when more
/// pages are available *and* the hint would actually work. `next` only
/// exists inside the REPL, so the hint is suppressed in one-shot CLI mode
/// rather than advertising a command that cannot work there.
fn listing_summary_line(
    count: usize,
    after: Option<&str>,
    unit: &str,
    mode: &OperatingMode,
) -> String {
    let base = format!("{} {} {}", color_green_bold("Found"), count, unit);
    if after.is_some() && matches!(mode, OperatingMode::Interactive) {
        format!("{base} (type 'next' for more)")
    } else {
        base
    }
}

/// Print the standard `Found N <unit>` line, with a `next` hint when more
/// pages are available (REPL only — see [`listing_summary_line`]).
fn summarize_listing(count: usize, after: Option<&str>, unit: &str, mode: &OperatingMode) {
    outln!("\n{}", listing_summary_line(count, after, unit, mode));
}

/// Advance the most recent paginated listing by one page. Errors clearly
/// when nothing has been listed yet (or when the previous listing was
/// already on its last page).
///
/// The cursor is cloned rather than taken, and `ctx.last_listing` is only
/// overwritten once the request has succeeded — a transient network error
/// must leave the session exactly where it was, not silently reset the
/// listing back to page 1.
fn handle_next(
    client: &DataGovClient,
    rt: &Runtime,
    ctx: &mut SessionContext,
) -> Result<(), Box<dyn std::error::Error>> {
    let cursor = ctx.last_listing.clone().ok_or(
        "nothing to continue — run a `search` or `ls` that reports 'more available' first",
    )?;

    match cursor {
        ListingCursor::OrgDatasets {
            org,
            after,
            page_size,
        } => {
            let page = rt.block_on(client.search(
                "",
                Some(page_size),
                Some(after.as_str()),
                Some(org.as_str()),
            ))?;
            print_dataset_hits(&page.results);
            summarize_listing(
                page.results.len(),
                page.after.as_deref(),
                "more datasets",
                &client.config().mode,
            );
            ctx.last_listing = page.after.map(|after| ListingCursor::OrgDatasets {
                org,
                after,
                page_size,
            });
        }
        ListingCursor::SearchResults {
            query,
            organization,
            after,
            page_size,
        } => {
            let page = rt.block_on(client.search(
                &query,
                Some(page_size),
                Some(after.as_str()),
                organization.as_deref(),
            ))?;
            print_search_hits(&page.results);
            summarize_listing(
                page.results.len(),
                page.after.as_deref(),
                "more results",
                &client.config().mode,
            );
            ctx.last_listing = page.after.map(|after| ListingCursor::SearchResults {
                query,
                organization,
                after,
                page_size,
            });
        }
    }
    Ok(())
}

/// Handle show command.
fn handle_show(
    client: &DataGovClient,
    rt: &Runtime,
    dataset_slug: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    outln!("{} dataset '{}'...", color_cyan("Fetching"), dataset_slug);

    let hit = rt.block_on(client.get_dataset(dataset_slug))?;
    print_package_details(&hit);

    Ok(())
}

/// Collect the downloadable distributions from a fetched dataset hit, with a
/// helpful error if the hit has no DCAT record attached.
fn downloadable_for(
    hit: &data_gov::catalog::models::SearchHit,
) -> Result<Vec<Distribution>, Box<dyn std::error::Error>> {
    let dcat = hit
        .dcat
        .as_ref()
        .ok_or("dataset is missing DCAT metadata; cannot determine distributions")?;
    Ok(DataGovClient::get_downloadable_distributions(dcat))
}

/// Handle download command.
///
/// Interpretation depends on session context:
/// - **Active dataset**: all args are distribution selectors (index or title).
/// - **No active dataset**: first arg is the dataset slug, rest are selectors.
/// - **No args + active dataset**: download all distributions.
/// - **No args + no active dataset**: error.
///
/// Each selector that doesn't match a distribution is reported as an error.
fn handle_download(
    client: &DataGovClient,
    rt: &Runtime,
    args: &[String],
    ctx: &SessionContext,
) -> Result<(), Box<dyn std::error::Error>> {
    let (dataset_slug, selectors) = if ctx.dataset.is_some() {
        let id = ctx.dataset.as_deref().unwrap();
        (id, args)
    } else if let Some(first) = args.first() {
        // Guard: a numeric first arg with no dataset in context is almost
        // always a user mistake — they meant `download <index>` after
        // selecting a dataset, but no dataset is selected. Without this
        // guard the digit would be sent to the catalog as a "slug" and we
        // would download whatever the API returned for it (data.gov
        // silently ignores unmatched slugs and returns the top result).
        if first.chars().all(|c| c.is_ascii_digit()) {
            return Err(format!(
                "no dataset selected — to download by index, first navigate into a dataset (e.g. `cd /<slug>`); '{first}' is not a valid dataset slug"
            )
            .into());
        }
        (first.as_str(), &args[1..])
    } else {
        return Err("no dataset specified and none selected (use: select /org/dataset)".into());
    };

    outln!("{} dataset '{}'...", color_cyan("Fetching"), dataset_slug);

    let hit = rt.block_on(client.get_dataset(dataset_slug))?;
    let distributions = downloadable_for(&hit)?;

    if distributions.is_empty() {
        outln!(
            "{} No downloadable distributions found in this dataset.",
            color_yellow_bold("Warning:")
        );
        return Ok(());
    }

    let dataset_dir = dataset_download_dir(&client.download_dir(), dataset_slug)?;

    if selectors.is_empty() {
        let results =
            rt.block_on(client.download_distributions(&distributions, Some(&dataset_dir)));
        print_download_summary(&results)?;
    } else {
        download_selected(client, rt, selectors, &distributions, &dataset_dir)?;
    }

    Ok(())
}

/// Name the per-dataset subdirectory of `base` that a download lands in.
///
/// The slug reaches this as a command argument or as catalog metadata, neither
/// of which is ours. The reduction is what makes the join safe and the check is
/// what makes it checked, so a change to either one on its own cannot move a
/// download out of the directory the user chose.
fn dataset_download_dir(
    base: &std::path::Path,
    dataset_slug: &str,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let safe_dataset_slug = sanitize_path_component(dataset_slug);
    Ok(join_inside(base, &safe_dataset_slug)?)
}

/// Resolve selectors and download matching distributions.
///
/// Each selector is either a numeric index or a title (case-insensitive
/// substring). Unmatched selectors are reported but don't stop other
/// selectors from resolving.
///
/// All resolved matches are downloaded in a single call to
/// [`DataGovClient::download_distributions`], the batch API, rather than
/// one [`DataGovClient::download_distribution`] call per match. The batch
/// API indexes each filename by its position in the batch specifically to
/// disambiguate distributions that share a title (the data.gov default
/// "Comma Separated Values File" is extremely common); calling the
/// single-item API per match — the previous behavior — gave every same-
/// titled match the same output path, so each download silently
/// overwrote the last and the CLI reported all of them as successful (#52).
///
/// Returns `Err` if any selector failed to match a distribution, or any
/// matched distribution failed to download: a partial result must never
/// be reported as a whole one (see AGENTS.md).
fn download_selected(
    client: &DataGovClient,
    rt: &Runtime,
    selectors: &[String],
    distributions: &[Distribution],
    dataset_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut matched: Vec<Distribution> = Vec::new();
    let mut labels: Vec<String> = Vec::new();
    let mut unmatched_selectors = 0usize;

    for selector in selectors {
        if let Ok(index) = selector.parse::<usize>() {
            if index >= distributions.len() {
                errln!(
                    "  {} '{}': index out of range (0-{})",
                    color_red_err("✗"),
                    selector,
                    distributions.len().saturating_sub(1)
                );
                unmatched_selectors += 1;
                continue;
            }
            matched.push(distributions[index].clone());
            labels.push(selector.clone());
        } else {
            let sel_lower = selector.to_lowercase();
            let hits: Vec<&Distribution> = distributions
                .iter()
                .filter(|d| {
                    d.title
                        .as_ref()
                        .is_some_and(|t| t.to_lowercase().contains(&sel_lower))
                })
                .collect();

            if hits.is_empty() {
                errln!(
                    "  {} '{}': no matching distribution",
                    color_red_err("✗"),
                    selector
                );
                print_available_distributions(distributions);
                unmatched_selectors += 1;
                continue;
            }

            for distribution in hits {
                labels.push(
                    distribution
                        .title
                        .clone()
                        .unwrap_or_else(|| "untitled".to_string()),
                );
                matched.push(distribution.clone());
            }
        }
    }

    // Distinct paths actually written, not just a count of `Ok` results —
    // the reported success count must reflect files that landed on disk,
    // not download attempts that happened to return success.
    let mut success_paths: HashSet<PathBuf> = HashSet::new();
    let mut download_errors = 0usize;

    if !matched.is_empty() {
        let results = rt.block_on(client.download_distributions(&matched, Some(dataset_dir)));
        for (label, result) in labels.iter().zip(results.iter()) {
            match result {
                Ok(path) => {
                    success_paths.insert(path.clone());
                    outln!(
                        "  {} {}: {}",
                        color_green("✓"),
                        color_yellow(label),
                        color_blue(&path.display().to_string())
                    );
                }
                Err(e) => {
                    download_errors += 1;
                    errln!(
                        "  {} {}: {}",
                        color_red_err("✗"),
                        label,
                        color_red_err(&e.to_string())
                    );
                }
            }
        }
    }

    let success_count = success_paths.len();
    let error_count = unmatched_selectors + download_errors;

    if success_count + error_count > 1 {
        outln!(
            "\n{} {} downloaded, {} errors",
            color_bold("Summary:"),
            color_green(&success_count.to_string()),
            color_red(&error_count.to_string())
        );
    }

    if error_count > 0 {
        return Err(format!(
            "{error_count} of {} selector(s) failed to resolve or download",
            selectors.len()
        )
        .into());
    }

    Ok(())
}

/// Print available distributions to help the user find what they want.
/// Written to stderr: it only ever prints alongside an error line, as
/// context for diagnosing that error.
fn print_available_distributions(distributions: &[Distribution]) {
    errln!("    Available distributions:");
    for (i, d) in distributions.iter().enumerate() {
        let title = d.title.as_deref().unwrap_or("(untitled)");
        let format = d
            .format
            .as_deref()
            .or(d.media_type.as_deref())
            .unwrap_or("?");
        errln!("      {i} {title} [{format}]");
    }
}

/// Print download summary for bulk downloads (no selectors).
///
/// Returns `Err` when at least one distribution failed to download, so the
/// top-level CLI handler exits non-zero instead of reporting a partial
/// result as a full success (#68).
fn print_download_summary(
    results: &[Result<PathBuf, data_gov::DataGovError>],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut success_count = 0;
    let mut error_count = 0;

    for (i, result) in results.iter().enumerate() {
        match result {
            Ok(path) => {
                success_count += 1;
                outln!(
                    "  {} Distribution {}: {}",
                    color_green("✓"),
                    i,
                    color_blue(&path.display().to_string())
                );
            }
            Err(e) => {
                error_count += 1;
                errln!(
                    "  {} Distribution {}: {}",
                    color_red_err("✗"),
                    i,
                    color_red_err(&e.to_string())
                );
            }
        }
    }

    outln!(
        "\n{} {} downloaded, {} errors",
        color_bold("Summary:"),
        color_green(&success_count.to_string()),
        color_red(&error_count.to_string())
    );

    if error_count > 0 {
        return Err(format!("{error_count} of {} download(s) failed", results.len()).into());
    }

    Ok(())
}

/// Handle list command. Behavior depends on the explicit subject and the
/// current session context:
///
/// - `ls organizations` (or `ls orgs`) — list all organizations regardless
///   of context.
/// - `ls` at root — same as `ls organizations`.
/// - `ls` at `/<org>` — list that org's datasets.
/// - `ls` at `/<org>/<dataset>` (or `//<dataset>`) — list distributions of
///   the current dataset.
fn handle_list(
    client: &DataGovClient,
    rt: &Runtime,
    ctx: &mut SessionContext,
    what: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(subject) = what {
        match subject.to_lowercase().as_str() {
            "organizations" | "orgs" => {
                ctx.last_listing = None;
                return list_organizations(client, rt);
            }
            other => {
                return Err(
                    format!("unknown list type '{other}' (available: organizations)").into(),
                );
            }
        }
    }

    match (&ctx.org, &ctx.dataset) {
        (_, Some(slug)) => {
            // Distributions aren't paginated; the dataset record carries them all.
            let slug = slug.clone();
            ctx.last_listing = None;
            list_dataset_distributions(client, rt, &slug)
        }
        (Some(org), None) => {
            let org = org.clone();
            list_org_datasets(client, rt, ctx, &org)
        }
        (None, None) => {
            ctx.last_listing = None;
            list_organizations(client, rt)
        }
    }
}

fn list_organizations(
    client: &DataGovClient,
    rt: &Runtime,
) -> Result<(), Box<dyn std::error::Error>> {
    outln!("{} organizations...", color_cyan("Fetching"));
    // The org list comes back as a single bulk response (~60-70 orgs);
    // there's no API-level pagination, so show them all.
    let orgs = rt.block_on(client.list_organizations(None))?;
    outln!("\n{} organizations:", color_green_bold("Government"));
    for (i, org) in orgs.iter().enumerate() {
        outln!(
            "{}. {}",
            color_blue_bold(&format!("{:2}", i + 1)),
            color_yellow(org)
        );
    }
    Ok(())
}

fn list_org_datasets(
    client: &DataGovClient,
    rt: &Runtime,
    ctx: &mut SessionContext,
    org: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    outln!("{} datasets in '{}'...", color_cyan("Fetching"), org);
    let page = rt.block_on(client.search("", Some(DEFAULT_PAGE_SIZE), None, Some(org)))?;
    if page.results.is_empty() {
        ctx.last_listing = None;
        outln!(
            "{} No datasets found in '{}'.",
            color_yellow_bold("Note:"),
            org
        );
        return Ok(());
    }
    print_dataset_hits(&page.results);
    summarize_listing(
        page.results.len(),
        page.after.as_deref(),
        "datasets",
        &client.config().mode,
    );

    ctx.last_listing = page.after.map(|after| ListingCursor::OrgDatasets {
        org: org.to_string(),
        after,
        page_size: DEFAULT_PAGE_SIZE,
    });
    Ok(())
}

/// Render datasets as `<slug> — <title>` with a tiny "[N files, modified
/// YYYY-MM-DD]" tail when those fields are populated. Distribution count
/// and last-harvested date come from the search response, so no extra
/// network call is needed.
fn print_dataset_hits(hits: &[data_gov::catalog::models::SearchHit]) {
    for hit in hits {
        let slug = hit.slug.as_deref().unwrap_or("(no-slug)");
        let title = hit.title.as_deref().unwrap_or("");

        let mut tail_parts: Vec<String> = Vec::new();
        let dist_count = hit.distribution_titles.len();
        if dist_count > 0 {
            let unit = if dist_count == 1 { "file" } else { "files" };
            tail_parts.push(format!("{dist_count} {unit}"));
        }
        if let Some(date) = hit.last_harvested_date.as_deref() {
            // Display just the date portion if it's an ISO timestamp.
            let short = date.split('T').next().unwrap_or(date);
            tail_parts.push(format!("modified {short}"));
        }
        let tail = if tail_parts.is_empty() {
            String::new()
        } else {
            format!("  [{}]", tail_parts.join(", "))
        };

        outln!(
            "{} {}{}",
            color_yellow_bold(slug),
            color_dimmed(title),
            color_dimmed(&tail),
        );
    }
}

fn list_dataset_distributions(
    client: &DataGovClient,
    rt: &Runtime,
    slug: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    outln!("{} distributions of '{}'...", color_cyan("Fetching"), slug);
    let hit = rt.block_on(client.get_dataset(slug))?;
    let distributions = downloadable_for(&hit)?;
    if distributions.is_empty() {
        outln!(
            "{} No downloadable distributions in '{}'.",
            color_yellow_bold("Note:"),
            slug
        );
        return Ok(());
    }
    outln!(
        "\n{} {} distributions:",
        color_green_bold("Found"),
        distributions.len()
    );
    // Distributions are zero-indexed because `download N` is zero-indexed
    // (and the `show` output already displays them that way). Don't tempt
    // anyone to type `download 1` after seeing `1.` and getting the second
    // distribution instead of the first.
    for (i, dist) in distributions.iter().enumerate() {
        let title = dist.title.as_deref().unwrap_or("(untitled)");
        let format = dist
            .format
            .as_deref()
            .or(dist.media_type.as_deref())
            .unwrap_or("?");
        outln!(
            "{}. {} [{}]",
            color_blue_bold(&format!("{:2}", i)),
            color_yellow(title),
            color_dimmed(format)
        );
    }
    Ok(())
}

/// Handle info command.
fn handle_info(client: &DataGovClient, ctx: &SessionContext) {
    outln!("\n{}", color_blue_bold("📊 Client Information"));
    let label = ctx.prompt_label();
    if !label.is_empty() {
        outln!("Active context:    {}", color_yellow_bold(&label));
    }
    if let Some(org) = &ctx.org {
        outln!("Active org:        {}", color_yellow(org));
    }
    if let Some(ds) = &ctx.dataset {
        outln!("Active dataset:    {}", color_yellow(ds));
    }
    outln!(
        "Download directory: {}",
        color_blue(&client.download_dir().display().to_string())
    );
    outln!(
        "Catalog endpoint:  {}",
        color_blue(&client.config().catalog_config.base_path)
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use data_gov::DataGovConfig;
    use std::path::{Path, PathBuf};
    use wiremock::matchers::{method, path, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn dataset_download_dir_names_a_subdirectory_of_the_download_directory() {
        let dir = dataset_download_dir(Path::new("/tmp/downloads"), "climate-data")
            .expect("an ordinary slug names a subdirectory");
        assert_eq!(dir, PathBuf::from("/tmp/downloads/climate-data"));
    }

    /// A slug that reduces to nothing would otherwise make the download
    /// directory itself the destination, which is not the per-dataset
    /// directory the command promised. This is the case the reduction cannot
    /// answer on its own, so it is the check that has to.
    #[test]
    fn dataset_download_dir_refuses_a_slug_that_reduces_to_nothing() {
        for slug in [".", "", "!@#$%", "\u{202e}"] {
            let outcome = dataset_download_dir(Path::new("/tmp/downloads"), slug);
            assert!(
                outcome.is_err(),
                "slug {slug:?} reduces to nothing and must be refused, got: {outcome:?}"
            );
        }
    }

    /// Every one of these carries something usable once the separators and the
    /// traversals are reduced away, so each must yield a directory - and that
    /// directory must be a direct child of the download directory.
    ///
    /// Asserting the success rather than tolerating a refusal is the point.
    /// `.!.` and `..!..` are the shapes a reduction that collapses before it
    /// filters turns back into `..`; if that regressed, the join would refuse
    /// them and a test that merely skipped a refusal would stay green.
    #[test]
    fn dataset_download_dir_keeps_a_reducible_slug_inside_the_download_directory() {
        for slug in [
            "..",
            "../escaped",
            "/etc/cron.d",
            "sub/dir",
            ".!.",
            "..!..",
            "C:\\Windows\\evil",
            "ordinary-slug",
        ] {
            let dir =
                dataset_download_dir(Path::new("/tmp/downloads"), slug).unwrap_or_else(|err| {
                    panic!(
                        "slug {slug:?} reduces to a usable name and must yield a directory: {err}"
                    )
                });
            assert_eq!(
                dir.parent(),
                Some(Path::new("/tmp/downloads")),
                "slug {slug:?} resolved to {dir:?}, which is not directly inside /tmp/downloads"
            );
        }
    }

    /// A client that never makes a network call at construction time, for
    /// tests that only need to reach an error path before any I/O happens.
    fn test_client() -> DataGovClient {
        DataGovClient::with_config(DataGovConfig::default()).expect("test client must build")
    }

    fn dist(title: &str) -> Distribution {
        dist_with_url(title, &format!("https://example.com/{title}"))
    }

    fn dist_with_url(title: &str, url: &str) -> Distribution {
        Distribution {
            type_hint: None,
            title: Some(title.to_string()),
            description: None,
            download_url: Some(url.to_string()),
            access_url: None,
            media_type: None,
            format: None,
            license: None,
            described_by: None,
            described_by_type: None,
        }
    }

    #[test]
    fn title_matching_is_case_insensitive() {
        let distributions = [dist("Data.CSV"), dist("report.json"), dist("ARCHIVE.CSV")];

        let needle = "csv".to_lowercase();
        let matches: Vec<_> = distributions
            .iter()
            .enumerate()
            .filter(|(_, d)| {
                d.title
                    .as_ref()
                    .is_some_and(|t| t.to_lowercase().contains(&needle))
            })
            .collect();

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].0, 0);
        assert_eq!(matches[1].0, 2);
    }

    #[test]
    fn title_matching_partial() {
        let distributions = [
            dist("complaints-2023.csv"),
            dist("data.json"),
            dist("complaints-2024.csv"),
        ];

        let needle = "complaint".to_string();
        let matches: Vec<_> = distributions
            .iter()
            .enumerate()
            .filter(|(_, d)| {
                d.title
                    .as_ref()
                    .is_some_and(|t| t.to_lowercase().contains(&needle))
            })
            .collect();

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].0, 0);
        assert_eq!(matches[1].0, 2);
    }

    #[test]
    fn title_matching_no_matches() {
        let distributions = [dist("data.csv"), dist("report.json")];

        let needle = "pdf".to_string();
        let matches: Vec<_> = distributions
            .iter()
            .filter(|d| {
                d.title
                    .as_ref()
                    .is_some_and(|t| t.to_lowercase().contains(&needle))
            })
            .collect();

        assert!(matches.is_empty());
    }

    // --- #68: failure paths exit non-zero instead of printing "Error:"
    // to stdout and returning Ok ---

    #[test]
    fn setdir_in_cli_mode_returns_err_instead_of_succeeding() {
        // SetDir is intercepted before execute_command in the REPL
        // (repl.rs handles it directly), so this arm only fires in
        // one-shot CLI mode, where "lcd" doesn't make sense. It must fail
        // loudly: the top-level CLI handler only calls exit(1) when this
        // returns Err, and a `set -e` script sees exit 0 as success.
        let client = test_client();
        let rt = Runtime::new().expect("runtime");
        let mut ctx = SessionContext::default();

        let result = execute_command(
            &client,
            &rt,
            ReplCommand::SetDir {
                path: std::path::PathBuf::from("/tmp"),
            },
            &mut ctx,
        );

        let Err(error) = result else {
            panic!("lcd in CLI mode must return Err, not silently succeed");
        };
        // `is_err()` alone is satisfied by *any* error, including one from
        // a completely different code path (e.g. a client construction
        // failure) that would make this test pass for the wrong reason.
        // Pin the actual message so the test names the real defect.
        let message = error.to_string();
        assert!(
            message.contains("lcd") && message.contains("REPL"),
            "expected the 'lcd is only available in interactive REPL mode' \
             message, got: {message}"
        );
    }

    #[test]
    fn handle_list_unknown_subject_returns_err() {
        let client = test_client();
        let rt = Runtime::new().expect("runtime");
        let mut ctx = SessionContext::default();

        let result = execute_command(
            &client,
            &rt,
            ReplCommand::List {
                what: Some("bogus".to_string()),
            },
            &mut ctx,
        );

        let Err(error) = result else {
            panic!("an unknown `ls` subject must return Err, not print and return Ok");
        };
        // Pin the message, not just Err-ness: `is_err()` alone would also
        // pass for an unrelated failure (a network error, a panic caught
        // upstream as an Err, ...) that has nothing to do with "bogus"
        // being an unrecognized subject.
        let message = error.to_string();
        assert!(
            message.contains("bogus"),
            "error should name the rejected subject: {message}"
        );
        assert!(
            message.contains("organizations"),
            "error should name the one subject that IS available: {message}"
        );
    }

    // --- #69.2: "cd ." is a local no-op, not a wasted lookup ---

    #[test]
    fn ambiguous_single_segment_excludes_dot() {
        // "." must not be routed through resolve_single_segment_cd (which
        // costs a full organization listing, then a dataset lookup, before
        // failing) — it's the current-directory no-op, handled locally by
        // SessionContext::apply_navigate.
        let ctx = SessionContext::default();
        assert_eq!(ambiguous_single_segment(&ctx, "."), None);
    }

    // --- #58.4: the 'next' hint only appears where 'next' works ---

    #[test]
    fn listing_summary_hints_next_in_interactive_mode_when_more_pages_exist() {
        let line = listing_summary_line(50, Some("cursor"), "results", &OperatingMode::Interactive);
        assert!(line.contains("next"), "line was: {line}");
    }

    #[test]
    fn listing_summary_suppresses_next_hint_outside_repl() {
        let line = listing_summary_line(50, Some("cursor"), "results", &OperatingMode::CommandLine);
        assert!(!line.contains("next"), "line was: {line}");
    }

    #[test]
    fn listing_summary_omits_hint_when_no_more_pages() {
        let line = listing_summary_line(50, None, "results", &OperatingMode::Interactive);
        assert!(!line.contains("next"), "line was: {line}");
    }

    // --- #54: the trailing-numeric-token-as-limit rule is silent by
    // default, so surface what it decided ---

    #[test]
    fn trailing_limit_notice_names_the_query_actually_searched_and_the_limit_applied() {
        // "search route 66" -> query "route", limit 66. A user who typed
        // the whole phrase must see, unambiguously, that they got neither.
        let notice = trailing_limit_notice("route", 66);
        assert!(
            notice.contains("\"route\""),
            "notice must name the query actually searched: {notice}"
        );
        assert!(
            notice.contains("66"),
            "notice must name the limit applied: {notice}"
        );
    }

    #[test]
    fn trailing_limit_notice_points_at_the_quoting_escape_hatch() {
        let notice = trailing_limit_notice("area", 51);
        assert!(
            notice.contains("\"area 51\""),
            "notice must show the quoted form that searches literally: {notice}"
        );
    }

    // --- #58.3: a failed 'next' leaves the cursor unchanged ---

    #[test]
    fn handle_next_preserves_cursor_on_request_failure() {
        // One runtime, used first to stand up the mock server, then reused
        // (sequentially, not nested) by handle_next's own block_on calls.
        let rt = Runtime::new().expect("runtime");
        let server = rt.block_on(async {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/search"))
                .respond_with(ResponseTemplate::new(500))
                .mount(&server)
                .await;
            server
        });

        let config = DataGovConfig::default().with_base_url(server.uri());
        let client = DataGovClient::with_config(config).expect("client");

        let mut ctx = SessionContext {
            org: None,
            dataset: None,
            last_listing: Some(ListingCursor::SearchResults {
                query: "climate".to_string(),
                organization: None,
                after: "cursor-1".to_string(),
                page_size: 50,
            }),
        };

        let result = handle_next(&client, &rt, &mut ctx);

        assert!(
            result.is_err(),
            "the mocked search endpoint always 500s, so handle_next must return Err"
        );
        match &ctx.last_listing {
            Some(ListingCursor::SearchResults { after, .. }) => {
                assert_eq!(
                    after, "cursor-1",
                    "a failed request must not advance or clear the stored cursor"
                );
            }
            other => panic!("expected the original SearchResults cursor to survive, got {other:?}"),
        }
    }

    // --- #52: same-titled distributions land on distinct paths ---

    #[test]
    fn download_selected_disambiguates_same_titled_distributions() {
        let rt = Runtime::new().expect("runtime");
        let server = rt.block_on(async {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path_regex(r"^/files/.*"))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(b"content".to_vec()))
                .mount(&server)
                .await;
            server
        });

        let tmp = tempfile::TempDir::new().expect("tempdir");
        // wiremock binds loopback, which downloads refuse by default (#51).
        // This is the opt-in that exists for a mirror on your own network.
        let config = DataGovConfig::default()
            .with_mode(OperatingMode::Interactive)
            .with_download_dir(tmp.path().to_path_buf())
            .with_private_network_downloads(true);
        let client = DataGovClient::with_config(config).expect("client");

        // Two distinct distributions sharing the data.gov-default title,
        // pointing at two different URLs — the exact #52 reproduction.
        let same_title = "Comma Separated Values File";
        let distributions = vec![
            dist_with_url(same_title, &format!("{}/files/1.csv", server.uri())),
            dist_with_url(same_title, &format!("{}/files/2.csv", server.uri())),
        ];

        let selectors = vec!["comma".to_string()];
        let result = download_selected(&client, &rt, &selectors, &distributions, tmp.path());

        assert!(
            result.is_ok(),
            "both matches download successfully: {result:?}"
        );

        let mut paths: Vec<_> = std::fs::read_dir(tmp.path())
            .expect("read dir")
            .map(|e| e.expect("dir entry").path())
            .collect();
        paths.sort();

        assert_eq!(
            paths.len(),
            2,
            "two same-titled distributions must land on two distinct paths on disk, found: {paths:?}"
        );
    }

    #[test]
    fn download_selected_returns_ok_when_everything_succeeds() {
        let rt = Runtime::new().expect("runtime");
        let server = rt.block_on(async {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path_regex(r"^/files/.*"))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(b"ok".to_vec()))
                .mount(&server)
                .await;
            server
        });

        let tmp = tempfile::TempDir::new().expect("tempdir");
        // wiremock binds loopback, which downloads refuse by default (#51).
        // This is the opt-in that exists for a mirror on your own network.
        let config = DataGovConfig::default()
            .with_download_dir(tmp.path().to_path_buf())
            .with_private_network_downloads(true);
        let client = DataGovClient::with_config(config).expect("client");

        let distributions = vec![dist_with_url(
            "one",
            &format!("{}/files/1.csv", server.uri()),
        )];
        let selectors = vec!["one".to_string()];

        let result = download_selected(&client, &rt, &selectors, &distributions, tmp.path());
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn download_selected_returns_err_when_a_selector_matches_nothing() {
        // No network involved: the selector never matches, so this must
        // fail before any download is attempted.
        let rt = Runtime::new().expect("runtime");
        let tmp = tempfile::TempDir::new().expect("tempdir");
        // wiremock binds loopback, which downloads refuse by default (#51).
        // This is the opt-in that exists for a mirror on your own network.
        let config = DataGovConfig::default()
            .with_download_dir(tmp.path().to_path_buf())
            .with_private_network_downloads(true);
        let client = DataGovClient::with_config(config).expect("client");

        let distributions = vec![dist("report.csv")];
        let selectors = vec!["does-not-exist".to_string()];

        let result = download_selected(&client, &rt, &selectors, &distributions, tmp.path());
        assert!(
            result.is_err(),
            "an unmatched selector must fail the whole command, not report partial success"
        );
    }

    #[test]
    fn download_selected_returns_err_on_partial_failure() {
        // One selector resolves and downloads fine; a second names an
        // out-of-range index. The overall command must still fail — a
        // partial result is never reported as a whole one.
        let rt = Runtime::new().expect("runtime");
        let server = rt.block_on(async {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path_regex(r"^/files/.*"))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(b"ok".to_vec()))
                .mount(&server)
                .await;
            server
        });

        let tmp = tempfile::TempDir::new().expect("tempdir");
        // wiremock binds loopback, which downloads refuse by default (#51).
        // This is the opt-in that exists for a mirror on your own network.
        let config = DataGovConfig::default()
            .with_download_dir(tmp.path().to_path_buf())
            .with_private_network_downloads(true);
        let client = DataGovClient::with_config(config).expect("client");

        let distributions = vec![dist_with_url(
            "one",
            &format!("{}/files/1.csv", server.uri()),
        )];
        let selectors = vec!["0".to_string(), "99".to_string()];

        let result = download_selected(&client, &rt, &selectors, &distributions, tmp.path());
        assert!(
            result.is_err(),
            "one out-of-range selector must fail the command even though the other succeeded"
        );
    }

    // --- #68: print_download_summary (the no-selectors bulk path) ---

    #[test]
    fn print_download_summary_returns_err_when_any_download_failed() {
        let results: Vec<Result<PathBuf, data_gov::DataGovError>> = vec![
            Ok(PathBuf::from("/tmp/ok.csv")),
            Err(data_gov::DataGovError::download_error("boom")),
        ];
        assert!(print_download_summary(&results).is_err());
    }

    #[test]
    fn print_download_summary_returns_ok_when_all_succeeded() {
        let results: Vec<Result<PathBuf, data_gov::DataGovError>> = vec![
            Ok(PathBuf::from("/tmp/a.csv")),
            Ok(PathBuf::from("/tmp/b.csv")),
        ];
        assert!(print_download_summary(&results).is_ok());
    }
}
