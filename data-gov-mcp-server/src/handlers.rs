//! Method dispatch and handler logic for MCP server requests.

use data_gov::DataGovClient;
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
                let result = self.data_gov.get_dataset(&params.id).await?;
                Ok(serde_json::to_value(result).map_err(ServerError::Serialization)?)
            }
            "data_gov.autocompleteDatasets" => {
                let params: AutocompleteParams = parse_required_params(method, params)?;
                let result = self
                    .data_gov
                    .autocomplete_datasets(&params.partial, params.limit)
                    .await?;
                Ok(serde_json::to_value(result).map_err(ServerError::Serialization)?)
            }
            "data_gov.listOrganizations" => {
                let params: ListOrganizationsParams = parse_optional_params(method, params)?;
                let result = self.data_gov.list_organizations(params.limit).await?;
                Ok(serde_json::to_value(result).map_err(ServerError::Serialization)?)
            }
            "data_gov.downloadResources" => self.handle_download_resources(method, params).await,
            "ckan.packageSearch" => {
                let params: PackageSearchParams = parse_optional_params(method, params)?;
                let result = self
                    .ckan
                    .package_search(
                        params.query.as_deref(),
                        params.rows,
                        params.start,
                        params.filter.as_deref(),
                    )
                    .await?;
                Ok(serde_json::to_value(result).map_err(ServerError::Serialization)?)
            }
            "ckan.packageShow" => {
                let params: DatasetParams = parse_required_params(method, params)?;
                let result = self.ckan.package_show(&params.id).await?;
                Ok(serde_json::to_value(result).map_err(ServerError::Serialization)?)
            }
            "ckan.organizationList" => {
                let params: OrganizationListParams = parse_optional_params(method, params)?;
                let result = self
                    .ckan
                    .organization_list(params.sort.as_deref(), params.limit, params.offset)
                    .await?;
                Ok(serde_json::to_value(result).map_err(ServerError::Serialization)?)
            }
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
        let mut result = self
            .data_gov
            .search(
                &params.query,
                params.limit,
                params.offset,
                params.organization.as_deref(),
                params.format.as_deref(),
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
            if let Some(results) = result.results.as_mut() {
                results.retain(|package| Self::matches_organization_filter(package, &filter));
            }
            result.count = Some(
                result
                    .results
                    .as_ref()
                    .map(|packages| packages.len() as i32)
                    .unwrap_or(0),
            );
        }

        let summaries = result
            .results
            .as_ref()
            .map(|packages| {
                packages
                    .iter()
                    .map(|package| self.to_dataset_summary(package))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let mut value = serde_json::to_value(&result).map_err(ServerError::Serialization)?;
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
        let dataset = self.data_gov.get_dataset(&params.dataset_id).await?;

        let mut missing_resource_ids: Vec<String> = Vec::new();
        let mut unavailable_formats: Vec<String> = Vec::new();

        if params
            .resource_ids
            .as_ref()
            .is_some_and(|ids| ids.is_empty())
        {
            return Err(ServerError::InvalidParams(format!(
                "{method}: resourceIds cannot be empty"
            )));
        }

        let mut resources = DataGovClient::get_downloadable_resources(&dataset);

        if let Some(resource_ids) = params.resource_ids.as_ref() {
            let available_ids: HashSet<String> = resources
                .iter()
                .filter_map(|resource| {
                    resource
                        .id
                        .as_ref()
                        .map(|uuid| uuid.to_string().to_ascii_lowercase())
                })
                .collect();

            let mut id_filter = HashSet::with_capacity(resource_ids.len());
            for id in resource_ids {
                let trimmed = id.trim();
                let normalized = trimmed.to_ascii_lowercase();
                if !available_ids.contains(&normalized) {
                    missing_resource_ids.push(trimmed.to_string());
                }
                id_filter.insert(normalized);
            }

            resources.retain(|resource| {
                resource
                    .id
                    .as_ref()
                    .is_some_and(|uuid| id_filter.contains(&uuid.to_string().to_ascii_lowercase()))
            });
        }

        if let Some(formats) = params.formats.as_ref() {
            let available_formats: HashSet<String> = resources
                .iter()
                .filter_map(|resource| resource.format.as_ref().map(|fmt| fmt.to_ascii_lowercase()))
                .collect();

            let mut format_filter = HashSet::with_capacity(formats.len());
            for fmt in formats {
                let trimmed = fmt.trim();
                let normalized = trimmed.to_ascii_lowercase();
                if !available_formats.contains(&normalized) {
                    unavailable_formats.push(trimmed.to_string());
                }
                format_filter.insert(normalized);
            }

            resources.retain(|resource| {
                resource
                    .format
                    .as_ref()
                    .is_some_and(|fmt| format_filter.contains(&fmt.to_ascii_lowercase()))
            });
        }

        if resources.is_empty() {
            let mut message = format!("{method}: no matching downloadable resources");
            if !missing_resource_ids.is_empty() {
                message.push_str(&format!(
                    "; missing resourceIds: {}",
                    missing_resource_ids.join(", ")
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

        // Sanitize dataset name to prevent path traversal attacks
        let safe_dataset_slug = data_gov::util::sanitize_path_component(&dataset.name);

        let resolved_output_dir = resolve_output_dir(
            params.output_dir.as_deref(),
            use_dataset_subdir,
            &safe_dataset_slug,
        )?;

        let output_dir = resolved_output_dir
            .unwrap_or_else(|| self.data_gov.download_dir().join(&safe_dataset_slug));

        let download_results = self
            .data_gov
            .download_resources(&resources, Some(output_dir.as_path()))
            .await;

        let mut downloads = Vec::with_capacity(resources.len());
        let mut success_count = 0usize;
        let mut error_count = 0usize;

        for (resource, result) in resources.iter().zip(download_results) {
            let resource_id = resource.id.as_ref().map(|id| id.to_string());
            match result {
                Ok(path) => {
                    success_count += 1;
                    downloads.push(json!({
                        "resourceId": resource_id,
                        "name": resource.name,
                        "format": resource.format,
                        "url": resource.url,
                        "status": "success",
                        "path": path.to_string_lossy(),
                    }));
                }
                Err(err) => {
                    error_count += 1;
                    downloads.push(json!({
                        "resourceId": resource_id,
                        "name": resource.name,
                        "format": resource.format,
                        "url": resource.url,
                        "status": "error",
                        "error": err.to_string(),
                    }));
                }
            }
        }

        let mut summary = json!({
            "dataset": {
                "id": dataset.id.as_ref().map(|id| id.to_string()),
                "name": &dataset.name,
                "title": &dataset.title,
            },
            "downloadDirectory": output_dir.to_string_lossy(),
            "downloadCount": downloads.len(),
            "successfulCount": success_count,
            "failedCount": error_count,
            "hasErrors": error_count > 0,
            "downloads": downloads,
        });

        if !missing_resource_ids.is_empty() {
            let values = missing_resource_ids
                .into_iter()
                .map(Value::String)
                .collect::<Vec<_>>();
            if let Some(obj) = summary.as_object_mut() {
                obj.insert("missingResourceIds".to_string(), Value::Array(values));
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

    /// Check whether a package matches an organization-contains filter.
    fn matches_organization_filter(package: &data_gov_ckan::models::Package, needle: &str) -> bool {
        let org_match = package.organization.as_ref().is_some_and(|org| {
            org.name.to_ascii_lowercase().contains(needle)
                || org
                    .title
                    .as_deref()
                    .is_some_and(|title| title.to_ascii_lowercase().contains(needle))
        });

        let owner_match = package
            .owner_org
            .as_deref()
            .is_some_and(|owner| owner.to_ascii_lowercase().contains(needle));

        let author_match = package
            .author
            .as_deref()
            .is_some_and(|author| author.to_ascii_lowercase().contains(needle));

        let maintainer_match = package
            .maintainer
            .as_deref()
            .is_some_and(|m| m.to_ascii_lowercase().contains(needle));

        org_match || owner_match || author_match || maintainer_match
    }

    /// Build a compact `DatasetSummary` from a full CKAN package.
    pub(crate) fn to_dataset_summary(
        &self,
        package: &data_gov_ckan::models::Package,
    ) -> DatasetSummary {
        let title = package
            .title
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string())
            .unwrap_or_else(|| package.name.clone());

        let organization_slug = package
            .organization
            .as_ref()
            .map(|org| org.name.clone())
            .or_else(|| package.owner_org.clone());

        let organization = package
            .organization
            .as_ref()
            .and_then(|org| org.title.clone())
            .or_else(|| organization_slug.clone());

        let mut formats: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        if let Some(resources) = package.resources.as_ref() {
            for resource in resources {
                if let Some(format) = resource.format.as_ref() {
                    let trimmed = format.trim();
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
            id: package.id.as_ref().map(|id| id.to_string()),
            name: package.name.clone(),
            title,
            organization,
            organization_slug,
            description: package.notes.clone(),
            dataset_url: self.dataset_url(&package.name),
            formats,
        }
    }

    /// Build the portal URL for a dataset.
    pub(crate) fn dataset_url(&self, dataset_name: &str) -> String {
        format!(
            "{}/dataset/{}",
            self.portal_base_url.trim_end_matches('/'),
            dataset_name
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
                    "error message should name the '..' component; got: {msg}"
                );
            }
            other => panic!("expected InvalidParams, got: {other:?}"),
        }
    }

    #[test]
    fn resolve_output_dir_rejects_embedded_parent_traversal() {
        // An absolute-looking prefix does not make '..' safe — filesystem
        // resolution could still escape upward via the parent segment.
        let err = resolve_output_dir(Some("/tmp/ok/../escape"), false, "slug")
            .expect_err("embedded '..' must be rejected");
        assert!(matches!(err, ServerError::InvalidParams(_)));
    }

    #[test]
    fn resolve_output_dir_rejects_windows_style_parent_traversal() {
        // The substring check is OS-agnostic; backslash separators still match.
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
        assert!(
            resolved.is_absolute(),
            "relative input should become absolute, got {resolved:?}"
        );
        assert!(resolved.ends_with("mydir"));
    }
}
