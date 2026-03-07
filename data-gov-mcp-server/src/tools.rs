//! MCP tool specifications, descriptors, and lookup functions.

use serde::Serialize;
use serde_json::{Value, json};
use std::sync::LazyLock;

/// Definition of a single MCP tool linking its public name to a server method.
#[derive(Debug, Serialize)]
pub(crate) struct ToolSpec {
    pub tool_name: &'static str,
    pub method_name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
}

/// Result payload for `tools/list`.
#[derive(Debug, Serialize)]
pub(crate) struct ListToolsResult {
    pub tools: Vec<ToolDescriptor>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "nextCursor")]
    pub next_cursor: Option<String>,
}

/// Single tool entry in a `tools/list` response.
#[derive(Debug, Serialize)]
pub(crate) struct ToolDescriptor {
    pub name: &'static str,
    pub description: &'static str,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

/// Wrapper for tool invocation results.
#[derive(Debug, Serialize)]
pub(crate) struct ToolResponse {
    pub content: Vec<ToolContent>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "isError")]
    pub is_error: Option<bool>,
}

impl ToolResponse {
    /// Build a response containing both pretty-printed text and raw JSON.
    pub fn from_value(value: Value) -> Self {
        let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
        Self {
            content: vec![
                ToolContent::Text { text },
                ToolContent::Json { json: value },
            ],
            is_error: None,
        }
    }
}

/// Individual content item within a `ToolResponse`.
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub(crate) enum ToolContent {
    /// Raw JSON payload.
    #[serde(rename = "json")]
    Json { json: Value },
    /// Human-readable text representation.
    #[serde(rename = "text")]
    Text { text: String },
}

/// Build a list of `ToolDescriptor` values from the static tool specs.
pub(crate) fn tool_descriptors() -> Vec<ToolDescriptor> {
    TOOL_SPECS
        .iter()
        .map(|spec| ToolDescriptor {
            name: spec.tool_name,
            description: spec.description,
            input_schema: spec.input_schema.clone(),
        })
        .collect()
}

/// Look up a tool spec by its public tool name.
pub(crate) fn find_tool_spec(name: &str) -> Option<&'static ToolSpec> {
    TOOL_SPECS.iter().find(|spec| spec.tool_name == name)
}

/// Look up a tool spec by its internal method name.
pub(crate) fn find_tool_spec_by_method(method: &str) -> Option<&'static ToolSpec> {
    TOOL_SPECS.iter().find(|spec| spec.method_name == method)
}

/// All registered tool specifications, lazily initialized.
pub(crate) static TOOL_SPECS: LazyLock<Vec<ToolSpec>> = LazyLock::new(|| {
    vec![
        ToolSpec {
            tool_name: "data_gov_search",
            method_name: "data_gov.search",
            description: "Search datasets on data.gov with optional filters. The query parameter accepts Solr-style search syntax including wildcards (*), phrase matching, and boolean operators (AND, OR, NOT). If you only want to filter by organization or format without a text query, you can omit the query parameter or pass an empty string. The response contains the raw CKAN package_search payload plus a `summaries` array with key dataset metadata.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Full-text search query (supports Solr syntax: wildcards, phrases, boolean operators). Examples: 'climat*', \"\\\"air quality\\\"\", 'climate AND (temperature OR precipitation)'. Optional - can be empty to search by filters only.", "default": ""},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 1000, "description": "Maximum number of results"},
                    "offset": {"type": "integer", "minimum": 0, "description": "Result offset for pagination"},
                    "organization": {"type": "string", "description": "Filter results to a specific organization (e.g., 'sec-gov', 'nasa-gov')"},
                    "format": {"type": "string", "description": "Filter results by resource format e.g. CSV"},
                    "organizationContains": {"type": "string", "description": "Case-insensitive substring filter applied to organization slug, organization title, author, or maintainer (e.g., 'NASA')."}
                },
                "additionalProperties": false
            }),
        },
        ToolSpec {
            tool_name: "data_gov_dataset",
            method_name: "data_gov.dataset",
            description: "Fetch detailed metadata for a dataset by name or ID",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "Dataset identifier or name"}
                },
                "required": ["id"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            tool_name: "data_gov_autocomplete_datasets",
            method_name: "data_gov.autocompleteDatasets",
            description: "Autocomplete dataset names based on a partial query",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "partial": {"type": "string", "description": "Partial dataset name"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 100, "description": "Maximum suggestions to return"}
                },
                "required": ["partial"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            tool_name: "data_gov_list_organizations",
            method_name: "data_gov.listOrganizations",
            description: "List publishing organizations (agencies) on data.gov",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "limit": {"type": "integer", "minimum": 1, "maximum": 1000, "description": "Maximum number of organizations to return"}
                },
                "additionalProperties": false
            }),
        },
        ToolSpec {
            tool_name: "data_gov_download_resources",
            method_name: "data_gov.downloadResources",
            description: "Download one or more dataset resources to the local filesystem",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "datasetId": {"type": "string", "description": "Dataset identifier or name"},
                    "resourceIds": {"type": "array", "items": {"type": "string"}, "description": "Optional list of resource IDs to download"},
                    "formats": {"type": "array", "items": {"type": "string"}, "description": "Optional list of resource formats to include (e.g. CSV, JSON)"},
                    "outputDir": {"type": "string", "description": "Optional directory to save files. Relative paths resolve against the current working directory."},
                    "datasetSubdirectory": {"type": "boolean", "description": "If true, create a dataset-named subdirectory inside the output directory."}
                },
                "required": ["datasetId"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            tool_name: "ckan_package_search",
            method_name: "ckan.packageSearch",
            description: "Perform a low-level CKAN package_search request with full Solr query syntax support. Use the filter parameter for advanced Solr queries like 'organization:nasa-gov', 'res_format:CSV', or complex queries with AND/OR/NOT operators.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": ["string", "null"], "description": "Full-text search query (supports Solr syntax). Examples: 'budget*', \"national parks\""},
                    "rows": {"type": ["integer", "null"], "minimum": 1, "maximum": 1000, "description": "Number of rows to return"},
                    "start": {"type": ["integer", "null"], "minimum": 0, "description": "Offset into result set"},
                    "filter": {"type": ["string", "null"], "description": "Filter query in Solr/CKAN syntax (e.g., 'organization:sec-gov', 'res_format:CSV AND tags:healthcare'). Supports boolean operators, ranges, and fielded queries."}
                },
                "additionalProperties": false
            }),
        },
        ToolSpec {
            tool_name: "ckan_package_show",
            method_name: "ckan.packageShow",
            description: "Retrieve detailed metadata for a dataset using CKAN",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "Dataset identifier or name"}
                },
                "required": ["id"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            tool_name: "ckan_organization_list",
            method_name: "ckan.organizationList",
            description: "List CKAN organizations with optional sorting and pagination",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sort": {"type": ["string", "null"], "description": "Sort expression e.g. name asc"},
                    "limit": {"type": ["integer", "null"], "minimum": 1, "maximum": 1000, "description": "Maximum organizations to return"},
                    "offset": {"type": ["integer", "null"], "minimum": 0, "description": "Offset for pagination"}
                },
                "additionalProperties": false
            }),
        },
    ]
});

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn tool_specs_has_expected_count() {
        assert_eq!(TOOL_SPECS.len(), 8);
    }

    #[test]
    fn tool_descriptors_match_tool_specs() {
        let descriptors = tool_descriptors();
        assert_eq!(TOOL_SPECS.len(), descriptors.len());

        for (spec, desc) in TOOL_SPECS.iter().zip(descriptors.iter()) {
            assert_eq!(spec.tool_name, desc.name);
            assert_eq!(spec.description, desc.description);
        }
    }

    #[test]
    fn all_tool_specs_have_valid_input_schema() {
        for spec in TOOL_SPECS.iter() {
            let schema = &spec.input_schema;
            assert_eq!(
                schema["type"], "object",
                "tool {} should have object schema",
                spec.tool_name
            );
            assert!(
                schema["properties"].is_object(),
                "tool {} should have properties",
                spec.tool_name
            );
        }
    }

    #[test]
    fn tool_names_are_unique() {
        let names: HashSet<&str> = TOOL_SPECS.iter().map(|s| s.tool_name).collect();
        assert_eq!(names.len(), TOOL_SPECS.len(), "tool names should be unique");
    }

    #[test]
    fn method_names_are_unique() {
        let methods: HashSet<&str> = TOOL_SPECS.iter().map(|s| s.method_name).collect();
        assert_eq!(
            methods.len(),
            TOOL_SPECS.len(),
            "method names should be unique"
        );
    }

    #[test]
    fn find_tool_spec_by_known_name() {
        let spec = find_tool_spec("data_gov_search");
        assert!(spec.is_some());
        let spec = spec.unwrap();
        assert_eq!(spec.method_name, "data_gov.search");
    }

    #[test]
    fn find_tool_spec_unknown_name_returns_none() {
        assert!(find_tool_spec("nonexistent_tool").is_none());
    }

    #[test]
    fn find_tool_spec_by_method_known() {
        let spec = find_tool_spec_by_method("ckan.packageSearch");
        assert!(spec.is_some());
        assert_eq!(spec.unwrap().tool_name, "ckan_package_search");
    }

    #[test]
    fn find_tool_spec_by_method_unknown_returns_none() {
        assert!(find_tool_spec_by_method("nonexistent.method").is_none());
    }

    #[test]
    fn tool_response_from_value_has_text_and_json() {
        let val = json!({"count": 5});
        let resp = ToolResponse::from_value(val.clone());
        assert_eq!(resp.content.len(), 2);
        assert!(resp.is_error.is_none());

        match &resp.content[0] {
            ToolContent::Text { text } => {
                assert!(text.contains("\"count\": 5"));
            }
            other => panic!("expected Text, got: {:?}", other),
        }
        match &resp.content[1] {
            ToolContent::Json { json } => {
                assert_eq!(*json, val);
            }
            other => panic!("expected Json, got: {:?}", other),
        }
    }
}
