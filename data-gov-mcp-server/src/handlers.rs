//! Method dispatch and handler logic for MCP server requests.

use data_gov::DataGovClient;
use data_gov::catalog::models::{Distribution, SearchHit};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use tokio::time::timeout;

use crate::server::DataGovMcpServer;
use crate::tools::{
    ListToolsResult, ToolResponse, WallClockBound, find_tool_spec, find_tool_spec_by_method,
    tool_descriptors, wall_clock_bound,
};
use crate::types::*;

impl DataGovMcpServer {
    /// Route a JSON-RPC method call to the appropriate handler.
    ///
    /// `tools/call` requests are unwrapped and re-dispatched to the underlying
    /// method. Direct method calls that correspond to a registered tool are
    /// wrapped in a `ToolResponse` automatically.
    pub(crate) async fn dispatch(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, ServerError> {
        if method == "tools/call" {
            let params: CallToolParams = parse_required_params(method, params)?;
            let spec = find_tool_spec(&params.name)
                .ok_or_else(|| ServerError::InvalidMethod(params.name.clone()))?;

            return self.run_tool(spec.method_name, params.arguments).await;
        }

        if find_tool_spec_by_method(method).is_some() {
            return self.run_tool(method, params).await;
        }

        self.invoke_method(method, params).await
    }

    /// Run a tool and wrap the outcome in a `ToolResponse`.
    ///
    /// A tool that ran and failed comes back as a result with `isError: true`,
    /// so the model can read the reason. A protocol fault it cannot act on -
    /// an unknown tool, arguments that fail the schema - propagates as a
    /// JSON-RPC error.
    async fn run_tool(&self, method: &str, params: Option<Value>) -> Result<Value, ServerError> {
        let response = match self.invoke_method(method, params).await {
            Ok(value) => ToolResponse::from_value(value),
            Err(ServerError::ToolFailedWith { message, payload }) => {
                tracing::warn!(method = %method, "tool reported failure: {message}");
                ToolResponse::execution_error_with(message, *payload)
            }
            Err(err) if err.is_tool_execution_failure() => {
                tracing::warn!(method = %method, "tool execution failed: {err}");
                ToolResponse::execution_error(err.to_string())
            }
            Err(err) => return Err(err),
        };
        serde_json::to_value(response).map_err(ServerError::Serialization)
    }

    /// Execute a single method under the per-request timeout.
    ///
    /// The timeout is the outer bound on one request. Without it a hung
    /// upstream holds a request, and its slot in the cancellation registry,
    /// for as long as the session lasts. It sits here, around the resolved
    /// method, so the message names the tool that was abandoned rather than
    /// the `tools/call` envelope it arrived in.
    ///
    /// A timed-out tool becomes a result with `isError: true`, like any other
    /// execution failure, because a model can act on it; a timed-out protocol
    /// method has no tool result to travel in and stays a JSON-RPC error.
    /// [`ServerError::is_tool_execution_failure`] makes that split.
    ///
    /// A tool the registry marks [`WallClockBound::Exempt`] runs without it.
    /// Elapsed time says nothing true about a transfer, so a budget that fits
    /// one link kills a healthy download on a slower one; what still stops
    /// such a tool is its own stall bounds - for a download, reqwest's
    /// `read_timeout` and the bound on the pre-flight name lookup, which runs
    /// outside reqwest - and `notifications/cancelled`. The
    /// answer is read from the tool registry rather than matched on the method
    /// name here, so a tool added later has to declare it - see
    /// [`crate::tools::ToolSpec::wall_clock`].
    async fn invoke_method(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, ServerError> {
        if wall_clock_bound(method) == WallClockBound::Exempt {
            return self.run_method(method, params).await;
        }

        match timeout(self.request_timeout, self.run_method(method, params)).await {
            Ok(result) => result,
            Err(_elapsed) => {
                let budget = self.request_timeout;
                tracing::warn!(method = %method, "request abandoned after {budget:?}");
                Err(ServerError::Timeout(format!(
                    "{method}: the server gave up after {budget:?}"
                )))
            }
        }
    }

    /// Execute a single method and return the result as a JSON `Value`.
    async fn run_method(&self, method: &str, params: Option<Value>) -> Result<Value, ServerError> {
        #[cfg(test)]
        if let Some(gate) = self.test_gate.as_ref()
            && gate.method == method
        {
            gate.release.notified().await;
        }

        match method {
            "initialize" => {
                let params: InitializeParams = parse_optional_params(method, params)?;
                let result =
                    InitializeResult::new(params.protocol_version.as_deref(), params.client_info);
                Ok(serde_json::to_value(result).map_err(ServerError::Serialization)?)
            }
            // The client's readiness notification. MCP names it
            // notifications/initialized in every supported revision back to
            // 2024-11-05 - never the bare "initialized" this arm used to
            // answer, which is not an MCP method and could never be sent by a
            // conformant client. It arrives as a notification with no id, so
            // this value is normally never observed; a JSON object rather
            // than `Value::Null` is still correct, because MCP types a
            // result as an object and a non-conformant client that sends
            // this with an id deserves a valid one.
            //
            // "shutdown" is gone for the same reason, not merged into this
            // arm: MCP defines no JSON-RPC method by that name in any
            // revision. A stdio session ends when the client closes stdin
            // (see Lifecycle > Shutdown); nothing is exchanged on the wire.
            // Both bare names are what LSP - which MCP's own spec cites as
            // an influence - calls its own lifecycle methods, and that is
            // almost certainly how they ended up here.
            //
            // https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle
            "notifications/initialized" => Ok(json!({})),
            // MCP ping: "The receiver MUST respond promptly with an empty
            // response." A keepalive answered with -32601 reads to the client
            // as a dead connection.
            "ping" => Ok(json!({})),
            "tools/list" => {
                let params: ListToolsParams = parse_optional_params(method, params)?;
                let _ = params.cursor;
                let result = ListToolsResult {
                    tools: tool_descriptors(),
                    next_cursor: None,
                };
                Ok(serde_json::to_value(result).map_err(ServerError::Serialization)?)
            }
            "data_gov.search" => self.handle_search(method, params).await,
            "data_gov.dataset" => {
                let params: DatasetParams = parse_required_params(method, params)?;
                let result = self.data_gov.get_dataset(&params.slug).await?;
                Ok(serde_json::to_value(result).map_err(ServerError::Serialization)?)
            }
            "data_gov.autocompleteDatasets" => {
                let params: AutocompleteParams = parse_required_params(method, params)?;
                validate_limit(method, params.limit, 1, 100)?;
                let result = self
                    .data_gov
                    .autocomplete_datasets(&params.partial, params.limit)
                    .await?;
                // A named object, not the bare Vec. MCP defines structuredContent
                // as a JSON object, and a key leaves room to add fields later
                // without breaking consumers.
                Ok(json!({ "datasets": result }))
            }
            "data_gov.listOrganizations" => {
                let params: ListOrganizationsParams = parse_optional_params(method, params)?;
                validate_limit(method, params.limit, 1, 1000)?;
                let result = self.data_gov.list_organizations(params.limit).await?;
                Ok(json!({ "organizations": result }))
            }
            "data_gov.downloadResources" => self.handle_download_resources(method, params).await,
            other => Err(ServerError::InvalidMethod(other.to_string())),
        }
    }

    /// Handle `data_gov.search` with optional organization-contains filtering.
    async fn handle_search(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, ServerError> {
        // Optional, not required: the advertised `inputSchema` lists no
        // required properties, so a call that omits `arguments` entirely is a
        // valid call and every field falls back to its default.
        let params: SearchParams = parse_optional_params(method, params)?;
        validate_limit(method, params.limit, 1, 1000)?;
        let mut page = self
            .data_gov
            .search(
                &params.query,
                params.limit,
                params.after.as_deref(),
                params.organization.as_deref(),
            )
            .await?;

        if let Some(filter) = params.organization_contains.as_ref().and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_ascii_lowercase())
            }
        }) {
            page.results
                .retain(|hit| Self::matches_organization_filter(hit, &filter));
        }

        let summaries: Vec<_> = page
            .results
            .iter()
            .map(|hit| self.to_dataset_summary(hit))
            .collect();

        let mut value = serde_json::to_value(&page).map_err(ServerError::Serialization)?;
        if let Value::Object(ref mut map) = value {
            map.insert(
                "summaries".to_string(),
                serde_json::to_value(&summaries).map_err(ServerError::Serialization)?,
            );
        }

        Ok(value)
    }

    /// Handle `data_gov.downloadResources` — filter, resolve output dir, download.
    async fn handle_download_resources(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, ServerError> {
        let params: DownloadResourcesParams = parse_required_params(method, params)?;

        if params
            .distribution_indexes
            .as_ref()
            .is_some_and(|ids| ids.is_empty())
        {
            return Err(ServerError::InvalidParams(format!(
                "{method}: distributionIndexes cannot be empty"
            )));
        }

        let hit = self.data_gov.get_dataset(&params.dataset_id).await?;
        // What follows describes the dataset the catalog returned, not the
        // request, so it reaches the client as a tool result with
        // `isError: true` rather than as a parameter error the model would try
        // to fix by changing arguments that were never wrong.
        let slug = hit.slug.clone().ok_or_else(|| {
            ServerError::ToolFailed(format!(
                "{method}: dataset returned without a slug; cannot derive download subdirectory"
            ))
        })?;

        let dcat = hit.dcat.as_ref().ok_or_else(|| {
            ServerError::ToolFailed(format!(
                "{method}: dataset has no DCAT metadata; cannot enumerate distributions"
            ))
        })?;

        let all_downloadable = DataGovClient::get_downloadable_distributions(dcat);

        let mut out_of_range: Vec<usize> = Vec::new();
        let mut unavailable_formats: Vec<String> = Vec::new();

        let mut distributions: Vec<Distribution> =
            if let Some(indexes) = params.distribution_indexes.as_ref() {
                let mut picked = Vec::with_capacity(indexes.len());
                let mut seen = HashSet::new();
                for &idx in indexes {
                    if !seen.insert(idx) {
                        continue;
                    }
                    match all_downloadable.get(idx) {
                        Some(dist) => picked.push(dist.clone()),
                        None => out_of_range.push(idx),
                    }
                }
                picked
            } else {
                all_downloadable.clone()
            };

        if let Some(formats) = params.formats.as_ref() {
            // Match user filters as case-insensitive substrings of either
            // `format` or `mediaType`. DCAT-US 3 distributions usually leave
            // `format` empty and populate `mediaType` with a full MIME type
            // (e.g., "application/json"), so users typing "JSON" should still
            // match. Empty filter strings are dropped — they would otherwise
            // match every distribution.
            let filters = normalized_format_filters(formats);

            // A filter list that reduces to nothing matches every
            // distribution, so it means the same as no filter at all: an
            // empty array, or one holding only blanks. Retaining on it
            // instead cleared the list, and the caller was told nothing
            // matched without being told which format to try, because there
            // was no format to name.
            if !filters.is_empty() {
                let distribution_matches = |d: &Distribution, filter: &str| -> bool {
                    d.format
                        .as_deref()
                        .is_some_and(|f| f.to_ascii_lowercase().contains(filter))
                        || d.media_type
                            .as_deref()
                            .is_some_and(|m| m.to_ascii_lowercase().contains(filter))
                };

                for (raw, normalized) in &filters {
                    if !distributions
                        .iter()
                        .any(|d| distribution_matches(d, normalized))
                    {
                        unavailable_formats.push(raw.clone());
                    }
                }

                distributions.retain(|d| {
                    filters
                        .iter()
                        .any(|(_, normalized)| distribution_matches(d, normalized))
                });
            }
        }

        if distributions.is_empty() {
            let mut message = format!("{method}: no matching downloadable distributions");
            if !out_of_range.is_empty() {
                let as_strings: Vec<String> = out_of_range.iter().map(|i| i.to_string()).collect();
                message.push_str(&format!(
                    "; out-of-range distributionIndexes: {}",
                    as_strings.join(", ")
                ));
            }
            if !unavailable_formats.is_empty() {
                message.push_str(&format!(
                    "; unavailable formats: {}",
                    unavailable_formats.join(", ")
                ));
            }
            return Err(ServerError::ToolFailed(message));
        }

        if params.output_dir.is_none() {
            self.data_gov.validate_download_dir().await?;
        }

        let use_dataset_subdir = params.dataset_subdirectory.unwrap_or(true);
        let safe_dataset_slug = data_gov::util::sanitize_path_component(&slug);

        let output_dir = resolve_output_dir(
            params.output_dir.as_deref(),
            use_dataset_subdir,
            &safe_dataset_slug,
            &self.data_gov.download_dir(),
        )?;

        let download_results = self
            .data_gov
            .download_distributions(&distributions, Some(output_dir.as_path()))
            .await;

        let mut downloads = Vec::with_capacity(distributions.len());
        let mut success_count = 0usize;
        let mut error_count = 0usize;

        for (distribution, result) in distributions.iter().zip(download_results) {
            match result {
                Ok(path) => {
                    success_count += 1;
                    downloads.push(json!({
                        "title": distribution.title,
                        "format": distribution.format,
                        "mediaType": distribution.media_type,
                        "url": distribution.download_url,
                        "status": "success",
                        "path": path.to_string_lossy(),
                    }));
                }
                Err(err) => {
                    error_count += 1;
                    downloads.push(json!({
                        "title": distribution.title,
                        "format": distribution.format,
                        "mediaType": distribution.media_type,
                        "url": distribution.download_url,
                        "status": "error",
                        "error": err.to_string(),
                    }));
                }
            }
        }

        let mut summary = json!({
            "dataset": {
                "slug": slug,
                "title": hit.title,
                "identifier": hit.identifier,
            },
            "downloadDirectory": output_dir.to_string_lossy(),
            "downloadCount": downloads.len(),
            "successfulCount": success_count,
            "failedCount": error_count,
            "hasErrors": error_count > 0,
            "downloads": downloads,
        });

        if !out_of_range.is_empty() {
            let values = out_of_range
                .into_iter()
                .map(|i| Value::from(i as u64))
                .collect::<Vec<_>>();
            if let Some(obj) = summary.as_object_mut() {
                obj.insert(
                    "outOfRangeDistributionIndexes".to_string(),
                    Value::Array(values),
                );
            }
        }

        if !unavailable_formats.is_empty() {
            let values = unavailable_formats
                .into_iter()
                .map(Value::String)
                .collect::<Vec<_>>();
            if let Some(obj) = summary.as_object_mut() {
                obj.insert("unavailableFormats".to_string(), Value::Array(values));
            }
        }

        // A call that downloaded none of the files it was asked for did not
        // succeed, and the flag has to say so: an agent acts on the result
        // without reading the counts beside it.
        //
        // A partial result deliberately stays a success. `hasErrors`,
        // `failedCount` and a per-file `status` name exactly which files did
        // not arrive, so nothing is hidden - and flagging the whole call as an
        // error would misreport the files that did.
        if success_count == 0 && error_count > 0 {
            return Err(ServerError::ToolFailedWith {
                message: format!(
                    "{method}: all {error_count} downloads failed; target directory {}. \
                     See `downloads` for the reason each one gave.",
                    output_dir.to_string_lossy()
                ),
                payload: Box::new(summary),
            });
        }

        Ok(summary)
    }

    /// Check whether a search hit matches an organization-contains filter.
    fn matches_organization_filter(hit: &SearchHit, needle: &str) -> bool {
        let org_slug_match = hit
            .organization
            .as_ref()
            .and_then(|o| o.slug.as_deref())
            .is_some_and(|slug| slug.to_ascii_lowercase().contains(needle));

        let org_name_match = hit
            .organization
            .as_ref()
            .and_then(|o| o.name.as_deref())
            .is_some_and(|name| name.to_ascii_lowercase().contains(needle));

        let publisher_match = hit
            .publisher
            .as_deref()
            .is_some_and(|p| p.to_ascii_lowercase().contains(needle));

        org_slug_match || org_name_match || publisher_match
    }

    /// Build a compact [`DatasetSummary`] from a full search hit.
    pub(crate) fn to_dataset_summary(&self, hit: &SearchHit) -> DatasetSummary {
        let slug = hit.slug.clone().unwrap_or_default();
        let title = hit
            .title
            .as_ref()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| slug.clone());

        let organization_slug = hit.organization.as_ref().and_then(|o| o.slug.clone());
        let organization = hit
            .organization
            .as_ref()
            .and_then(|o| o.name.clone())
            .or_else(|| organization_slug.clone())
            .or_else(|| hit.publisher.clone());

        let mut formats: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        if let Some(dcat) = hit.dcat.as_ref() {
            for dist in &dcat.distribution {
                let raw = dist.format.as_deref().or(dist.media_type.as_deref());
                if let Some(raw) = raw {
                    let trimmed = raw.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let key = trimmed.to_ascii_lowercase();
                    if seen.insert(key) {
                        formats.push(trimmed.to_string());
                    }
                }
            }
        }

        DatasetSummary {
            identifier: hit.identifier.clone(),
            slug: slug.clone(),
            title,
            organization,
            organization_slug,
            description: hit.description.clone(),
            dataset_url: self.dataset_url(&slug),
            formats,
        }
    }

    /// Build the portal URL for a dataset.
    pub(crate) fn dataset_url(&self, slug: &str) -> String {
        format!(
            "{}/dataset/{slug}",
            self.portal_base_url.trim_end_matches('/'),
        )
    }
}

/// Pair each requested format with the lowercase form used for matching.
///
/// Blank entries are dropped: they would match every distribution. The raw and
/// normalized forms travel together in one vector so the report and the filter
/// cannot disagree about which string a filter came from. Building them as two
/// vectors and zipping them looks equivalent and is not - the moment a blank is
/// dropped the indexes stop lining up, and the zip both mislabels the survivors
/// and stops at the shorter vector.
fn normalized_format_filters(formats: &[String]) -> Vec<(String, String)> {
    formats
        .iter()
        .map(|raw| {
            let trimmed = raw.trim();
            (trimmed.to_string(), trimmed.to_ascii_lowercase())
        })
        .filter(|(trimmed, _)| !trimmed.is_empty())
        .collect()
}

/// Resolve the directory the downloaded files land in.
///
/// - Uses `default_base` when the client requested no directory.
/// - Rejects any requested path containing `..` components.
/// - Anchors a relative requested path to the current working directory.
/// - Appends `safe_dataset_slug` when `use_dataset_subdir` is true, and refuses
///   when the result would not name a directory directly inside the base.
///
/// The subdirectory flag applies on both branches. It is advertised and
/// defaulted, so it has to mean the same thing whether or not `outputDir` was
/// given; deciding it only on the requested branch left it inert across half
/// its input space.
///
/// The slug is joined on through one checked call, so both branches are guarded
/// on the same terms: the configured download directory is no more entitled to
/// be escaped than a caller-supplied one.
///
/// `safe_dataset_slug` is expected to have already been run through
/// [`data_gov::util::sanitize_path_component`], but the join is checked
/// regardless: the slug comes from the catalog, which is untrusted input, and
/// a guard that assumes what a reduction can produce is a guard that stops
/// working the day the reduction changes.
///
/// # Errors
///
/// Returns [`ServerError::InvalidParams`] when `requested` carries a `..`
/// component, or when `safe_dataset_slug` does not name a directory directly
/// inside the base, and [`ServerError::Io`] when a relative `requested` path
/// cannot be anchored to the current directory.
pub(crate) fn resolve_output_dir(
    requested: Option<&str>,
    use_dataset_subdir: bool,
    safe_dataset_slug: &str,
    default_base: &Path,
) -> Result<PathBuf, ServerError> {
    let base = match requested {
        None => default_base.to_path_buf(),
        Some(dir) => {
            if names_a_parent_directory(dir) {
                return Err(ServerError::InvalidParams(
                    "output_dir must not contain '..' path components".to_string(),
                ));
            }

            let path = PathBuf::from(dir);
            if path.is_absolute() {
                path
            } else {
                std::env::current_dir().map_err(ServerError::Io)?.join(path)
            }
        }
    };

    if use_dataset_subdir {
        return dataset_subdirectory(&base, safe_dataset_slug);
    }
    Ok(base)
}

/// Name the per-dataset subdirectory of `base`, refusing anything that leaves it.
///
/// # Errors
///
/// Returns [`ServerError::InvalidParams`] when `safe_dataset_slug` does not
/// name a directory directly inside `base`.
fn dataset_subdirectory(base: &Path, safe_dataset_slug: &str) -> Result<PathBuf, ServerError> {
    data_gov::util::join_inside(base, safe_dataset_slug).map_err(|err| {
        ServerError::InvalidParams(format!(
            "dataset slug does not name a directory inside the chosen download directory: {err}"
        ))
    })
}

/// True when `dir` carries a parent-directory step.
///
/// Reads path components rather than searching for the two characters, so a
/// name that merely contains dots - `/data/v1..v2/exports` - is the ordinary
/// directory it looks like and not a traversal. This is the same reading
/// [`data_gov::util::join_inside`] takes of the component it joins on.
///
/// Backslash-separated segments are read too. On a host whose separator is
/// `/`, `..\\escape` is one ordinary component and would otherwise pass, and
/// the caller of an MCP server is free to be a Windows client.
fn names_a_parent_directory(dir: &str) -> bool {
    dir.split('\\').any(|segment| {
        Path::new(segment)
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Stand-in for the configured download directory.
    const DEFAULT_BASE: &str = "/base/downloads";

    fn default_base() -> &'static Path {
        Path::new(DEFAULT_BASE)
    }

    fn distribution(fields: Value) -> Distribution {
        serde_json::from_value(fields).expect("a Distribution")
    }

    #[test]
    fn resolve_output_dir_rejects_leading_parent_traversal() {
        let err = resolve_output_dir(Some("../etc/passwd"), true, "slug", default_base())
            .expect_err("parent traversal must be rejected");
        match err {
            ServerError::InvalidParams(msg) => {
                assert!(
                    msg.contains(".."),
                    "error should name the '..' component; got: {msg}"
                );
            }
            other => panic!("expected InvalidParams, got: {other:?}"),
        }
    }

    #[test]
    fn resolve_output_dir_rejects_embedded_parent_traversal() {
        let err = resolve_output_dir(Some("/tmp/ok/../escape"), false, "slug", default_base())
            .expect_err("embedded '..' must be rejected");
        assert!(matches!(err, ServerError::InvalidParams(_)));
    }

    #[test]
    fn resolve_output_dir_rejects_windows_style_parent_traversal() {
        let err = resolve_output_dir(
            Some("C:\\Users\\me\\..\\other"),
            false,
            "slug",
            default_base(),
        )
        .expect_err("'..' inside backslash path must be rejected");
        assert!(matches!(err, ServerError::InvalidParams(_)));
    }

    /// Dots in a directory name are not a parent-directory step.
    /// `/data/v1..v2/exports` names an ordinary directory, and refusing it on
    /// the substring `..` both blocks a legitimate path and tells the caller
    /// something about it that is not true.
    #[test]
    fn resolve_output_dir_accepts_a_directory_name_that_contains_dots() {
        for dir in [
            "/data/v1..v2/exports",
            "/srv/archive..2024",
            "/tmp/..hidden",
        ] {
            let resolved = resolve_output_dir(Some(dir), false, "slug", default_base())
                .unwrap_or_else(|err| panic!("{dir} has no `..` component, got: {err:?}"));
            assert_eq!(resolved, PathBuf::from(dir));
        }
    }

    /// The other half: dots in a name buy a real traversal nothing. The check
    /// reads path components, so a `..` step beside such a name is still
    /// refused - under either separator.
    #[test]
    fn resolve_output_dir_rejects_traversal_beside_a_name_that_contains_dots() {
        for dir in ["/data/v1..v2/../escape", "/srv/a..b/..", "..\\v1..v2"] {
            let outcome = resolve_output_dir(Some(dir), false, "slug", default_base());
            assert!(
                matches!(outcome, Err(ServerError::InvalidParams(_))),
                "{dir} leaves its parent and must be refused, got: {outcome:?}"
            );
        }
    }

    /// The `..` check covers the string the caller supplied. The slug is joined
    /// on afterwards and comes from the catalog, which is untrusted input, so
    /// the join needs its own check rather than an assumption about what the
    /// reduction can produce.
    #[test]
    fn resolve_output_dir_rejects_a_slug_that_leaves_the_chosen_directory() {
        for slug in ["..", "../escaped", "/etc/cron.d", "sub/dir"] {
            let outcome = resolve_output_dir(Some("/tmp/downloads"), true, slug, default_base());
            match outcome {
                Err(ServerError::InvalidParams(_)) => {}
                Err(other) => panic!("expected InvalidParams for slug {slug:?}, got: {other:?}"),
                Ok(path) => panic!(
                    "slug {slug:?} resolved to {path:?}, which is not inside the chosen directory"
                ),
            }
        }
    }

    /// A slug that reduces to nothing would silently make the chosen directory
    /// itself the destination, which is not the directory the caller asked for.
    #[test]
    fn resolve_output_dir_rejects_a_slug_that_reduces_to_nothing() {
        let outcome = resolve_output_dir(Some("/tmp/downloads"), true, "", default_base());
        assert!(
            matches!(outcome, Err(ServerError::InvalidParams(_))),
            "an empty slug must be refused, got: {outcome:?}"
        );
    }

    #[test]
    fn resolve_output_dir_appends_the_slug_to_the_configured_default() {
        let resolved = resolve_output_dir(None, true, "climate-data", Path::new("/tmp/downloads"))
            .expect("the configured default takes the dataset subdirectory");
        assert_eq!(resolved, PathBuf::from("/tmp/downloads/climate-data"));
    }

    /// The default directory is checked on the same terms as one the caller
    /// named. Without this, an escaping slug lands in the parent of the user's
    /// Downloads directory instead.
    #[test]
    fn resolve_output_dir_refuses_a_slug_that_leaves_the_configured_default() {
        for slug in ["..", "../escaped", "/etc/cron.d", "sub/dir", ""] {
            let outcome = resolve_output_dir(None, true, slug, Path::new("/tmp/downloads"));
            match outcome {
                Err(ServerError::InvalidParams(_)) => {}
                Err(other) => panic!("expected InvalidParams for slug {slug:?}, got: {other:?}"),
                Ok(path) => panic!(
                    "slug {slug:?} resolved to {path:?}, which is not inside the default directory"
                ),
            }
        }
    }

    #[test]
    fn resolve_output_dir_prefers_the_directory_the_caller_named() {
        let resolved = resolve_output_dir(
            Some("/tmp/chosen"),
            true,
            "climate-data",
            Path::new("/tmp/downloads"),
        )
        .expect("a caller-supplied directory wins over the configured default");
        assert_eq!(resolved, PathBuf::from("/tmp/chosen/climate-data"));
    }

    #[test]
    fn resolve_output_dir_anchors_relative_path_to_cwd() {
        let resolved = resolve_output_dir(Some("mydir"), false, "slug", default_base())
            .expect("should succeed");
        assert!(resolved.is_absolute());
        assert!(resolved.ends_with("mydir"));
    }

    /// `datasetSubdirectory` is advertised with a default, so it has to mean
    /// the same thing across its whole input space. All four combinations, in
    /// one table: with `outputDir` omitted the flag was previously decided
    /// before it was ever read, and the `(None, false)` row is the one that
    /// could not pass.
    #[test]
    fn resolve_output_dir_honours_the_subdirectory_flag_on_both_branches() {
        let cases = [
            (
                Some("/tmp/downloads"),
                true,
                PathBuf::from("/tmp/downloads/climate-data"),
            ),
            (
                Some("/tmp/downloads"),
                false,
                PathBuf::from("/tmp/downloads"),
            ),
            (None, true, PathBuf::from("/base/downloads/climate-data")),
            (None, false, PathBuf::from("/base/downloads")),
        ];

        for (requested, use_dataset_subdir, expected) in cases {
            let resolved = resolve_output_dir(
                requested,
                use_dataset_subdir,
                "climate-data",
                default_base(),
            )
            .expect("should succeed");
            assert_eq!(
                resolved, expected,
                "outputDir {requested:?} with datasetSubdirectory {use_dataset_subdir}"
            );
        }
    }

    #[test]
    fn normalized_format_filters_drops_blanks_and_keeps_the_raw_form_paired() {
        let formats = vec![
            "  ".to_string(),
            "XLSX".to_string(),
            "".to_string(),
            " CSV ".to_string(),
        ];

        let filters = normalized_format_filters(&formats);

        assert_eq!(
            filters,
            vec![
                ("XLSX".to_string(), "xlsx".to_string()),
                ("CSV".to_string(), "csv".to_string()),
            ],
            "a blank filter would match every distribution, so it is dropped - \
             and dropping it must not shift the remaining pairs"
        );
    }

    /// The defect this replaced: `formats.iter().zip(normalized.iter())` paired
    /// index 0 of the raw list with index 0 of the filtered list. With one
    /// blank at the front, every pair is off by one and the last is lost.
    #[test]
    fn normalized_format_filters_never_pairs_a_filter_with_another_formats_label() {
        let formats = vec!["  ".to_string(), "XLSX".to_string()];

        let filters = normalized_format_filters(&formats);

        assert_eq!(filters.len(), 1, "one usable filter: {filters:?}");
        let (raw, normalized) = &filters[0];
        assert_eq!(raw, "XLSX", "the label must name the format the user typed");
        assert_eq!(normalized, "xlsx");
    }

    #[test]
    fn normalized_format_filters_is_empty_when_every_entry_is_blank() {
        let formats = vec!["".to_string(), "   ".to_string(), "\t".to_string()];
        assert!(normalized_format_filters(&formats).is_empty());
    }

    /// A `Distribution` deserialized from the wire shape, so the field names
    /// under test are the ones the Catalog API actually sends.
    #[test]
    fn a_distribution_carries_the_format_fields_the_filter_reads() {
        let dist = distribution(json!({
            "@type": "dcat:Distribution",
            "downloadURL": "https://example.com/f.csv",
            "mediaType": "text/csv"
        }));
        assert_eq!(dist.media_type.as_deref(), Some("text/csv"));
        assert!(
            dist.format.is_none(),
            "DCAT-US 3 usually leaves `format` empty, which is why the filter \
             also reads mediaType"
        );
    }
}
