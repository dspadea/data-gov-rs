//! Method dispatch and handler logic for MCP server requests.

use data_gov::DataGovClient;
use data_gov::catalog::models::{Distribution, SearchHit};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::PathBuf;

use crate::server::DataGovMcpServer;
use crate::tools::{
    ListToolsResult, ToolResponse, find_tool_spec, find_tool_spec_by_method, tool_descriptors,
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

            let value = self
                .invoke_method(spec.method_name, params.arguments)
                .await?;
            let response = ToolResponse::from_value(value);
            return serde_json::to_value(response).map_err(ServerError::Serialization);
        }

        if find_tool_spec_by_method(method).is_some() {
            let value = self.invoke_method(method, params).await?;
            let response = ToolResponse::from_value(value);
            return serde_json::to_value(response).map_err(ServerError::Serialization);
        }

        self.invoke_method(method, params).await
    }

    /// Execute a single method and return the result as a JSON `Value`.
    async fn invoke_method(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, ServerError> {
        match method {
            "initialize" => {
                let params: InitializeParams = parse_optional_params(method, params)?;
                let result = InitializeResult::new(params.client_info);
                Ok(serde_json::to_value(result).map_err(ServerError::Serialization)?)
            }
            "initialized" => Ok(Value::Null),
            "shutdown" => Ok(Value::Null),
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
                Ok(serde_json::to_value(result).map_err(ServerError::Serialization)?)
            }
            "data_gov.listOrganizations" => {
                let params: ListOrganizationsParams = parse_optional_params(method, params)?;
                validate_limit(method, params.limit, 1, 1000)?;
                let result = self.data_gov.list_organizations(params.limit).await?;
                Ok(serde_json::to_value(result).map_err(ServerError::Serialization)?)
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
        let params: SearchParams = parse_required_params(method, params)?;
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
        let slug = hit.slug.clone().ok_or_else(|| {
            ServerError::InvalidParams(format!(
                "{method}: dataset returned without a slug; cannot derive download subdirectory"
            ))
        })?;

        let dcat = hit.dcat.as_ref().ok_or_else(|| {
            ServerError::InvalidParams(format!(
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
            let normalized_filters: Vec<String> = formats
                .iter()
                .map(|f| f.trim().to_ascii_lowercase())
                .filter(|f| !f.is_empty())
                .collect();

            let distribution_matches = |d: &Distribution, filter: &str| -> bool {
                d.format
                    .as_deref()
                    .is_some_and(|f| f.to_ascii_lowercase().contains(filter))
                    || d.media_type
                        .as_deref()
                        .is_some_and(|m| m.to_ascii_lowercase().contains(filter))
            };

            for (raw, normalized) in formats.iter().zip(normalized_filters.iter()) {
                if !distributions
                    .iter()
                    .any(|d| distribution_matches(d, normalized))
                {
                    unavailable_formats.push(raw.trim().to_string());
                }
            }

            distributions.retain(|d| {
                normalized_filters
                    .iter()
                    .any(|filter| distribution_matches(d, filter))
            });
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
            return Err(ServerError::InvalidParams(message));
        }

        if params.output_dir.is_none() {
            self.data_gov.validate_download_dir().await?;
        }

        let use_dataset_subdir = params.dataset_subdirectory.unwrap_or(true);
        let safe_dataset_slug = data_gov::util::sanitize_path_component(&slug);

        let resolved_output_dir = resolve_output_dir(
            params.output_dir.as_deref(),
            use_dataset_subdir,
            &safe_dataset_slug,
        )?;

        let output_dir = resolved_output_dir
            .unwrap_or_else(|| self.data_gov.download_dir().join(&safe_dataset_slug));

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

/// Resolve a client-requested download directory into an absolute path.
///
/// - Returns `Ok(None)` when no directory was requested (caller picks a
///   default).
/// - Rejects any path containing `..` components with
///   [`ServerError::InvalidParams`].
/// - Anchors relative paths to the current working directory.
/// - Appends `safe_dataset_slug` when `use_dataset_subdir` is true.
///
/// `safe_dataset_slug` is expected to have already been run through
/// [`data_gov::util::sanitize_path_component`].
pub(crate) fn resolve_output_dir(
    requested: Option<&str>,
    use_dataset_subdir: bool,
    safe_dataset_slug: &str,
) -> Result<Option<PathBuf>, ServerError> {
    let Some(dir) = requested else {
        return Ok(None);
    };

    if dir.contains("..") {
        return Err(ServerError::InvalidParams(
            "output_dir must not contain '..' path components".to_string(),
        ));
    }

    let mut path = PathBuf::from(dir);
    if !path.is_absolute() {
        path = std::env::current_dir().map_err(ServerError::Io)?.join(path);
    }
    if use_dataset_subdir {
        path = path.join(safe_dataset_slug);
    }
    Ok(Some(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_output_dir_returns_none_when_no_dir_requested() {
        let resolved = resolve_output_dir(None, true, "slug").expect("should succeed");
        assert!(resolved.is_none());
    }

    #[test]
    fn resolve_output_dir_rejects_leading_parent_traversal() {
        let err = resolve_output_dir(Some("../etc/passwd"), true, "slug")
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
        let err = resolve_output_dir(Some("/tmp/ok/../escape"), false, "slug")
            .expect_err("embedded '..' must be rejected");
        assert!(matches!(err, ServerError::InvalidParams(_)));
    }

    #[test]
    fn resolve_output_dir_rejects_windows_style_parent_traversal() {
        let err = resolve_output_dir(Some("C:\\Users\\me\\..\\other"), false, "slug")
            .expect_err("'..' inside backslash path must be rejected");
        assert!(matches!(err, ServerError::InvalidParams(_)));
    }

    #[test]
    fn resolve_output_dir_appends_dataset_slug_when_enabled() {
        let resolved = resolve_output_dir(Some("/tmp/downloads"), true, "climate-data")
            .expect("should succeed")
            .expect("should produce path");
        assert_eq!(resolved, PathBuf::from("/tmp/downloads/climate-data"));
    }

    #[test]
    fn resolve_output_dir_omits_dataset_slug_when_disabled() {
        let resolved = resolve_output_dir(Some("/tmp/downloads"), false, "climate-data")
            .expect("should succeed")
            .expect("should produce path");
        assert_eq!(resolved, PathBuf::from("/tmp/downloads"));
    }

    #[test]
    fn resolve_output_dir_anchors_relative_path_to_cwd() {
        let resolved = resolve_output_dir(Some("mydir"), false, "slug")
            .expect("should succeed")
            .expect("should produce path");
        assert!(resolved.is_absolute());
        assert!(resolved.ends_with("mydir"));
    }
}
