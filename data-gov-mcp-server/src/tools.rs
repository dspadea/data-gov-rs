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

/// Individual content item within a [`ToolResponse`].
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
            description: "Search datasets on data.gov. Pagination is cursor-based: the response \
                          carries an `after` field when more results are available; pass it back \
                          as `after` on the next call to advance. The response also contains a \
                          `summaries` array with key dataset metadata.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Full-text query. Can be empty to filter only by organization.",
                        "default": ""
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 1000,
                        "description": "Page size."
                    },
                    "after": {
                        "type": "string",
                        "description": "Opaque pagination cursor returned as `after` on the previous page."
                    },
                    "organization": {
                        "type": "string",
                        "description": "Filter results by organization slug (e.g. 'nasa', 'epa-gov')."
                    },
                    "organizationContains": {
                        "type": "string",
                        "description": "Case-insensitive substring filter applied client-side to organization slug, name, and publisher."
                    }
                },
                "additionalProperties": false
            }),
        },
        ToolSpec {
            tool_name: "data_gov_dataset",
            method_name: "data_gov.dataset",
            description: "Fetch a dataset by its data.gov slug (e.g. 'meteorite-landings'). \
                          Slugs appear in search results as the `slug` field and in dataset URLs.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "slug": {
                        "type": "string",
                        "description": "Dataset slug. Use the slug from search results or the dataset URL — do not construct or guess this value."
                    }
                },
                "required": ["slug"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            tool_name: "data_gov_autocomplete_datasets",
            method_name: "data_gov.autocompleteDatasets",
            description: "Return dataset titles matching a partial query. Implemented as a \
                          capped full-text search.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "partial": {"type": "string", "description": "Partial dataset title or keyword."},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 100, "description": "Maximum suggestions to return."}
                },
                "required": ["partial"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            tool_name: "data_gov_list_organizations",
            method_name: "data_gov.listOrganizations",
            description: "List publishing organizations on data.gov.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "limit": {"type": "integer", "minimum": 1, "maximum": 1000, "description": "Maximum number of organizations to return."}
                },
                "additionalProperties": false
            }),
        },
        ToolSpec {
            tool_name: "data_gov_download_resources",
            method_name: "data_gov.downloadResources",
            description: "Download one or more DCAT distributions for a dataset to the local \
                          filesystem. By default, files are saved into a subdirectory named \
                          after the dataset slug inside the output directory. Distributions \
                          without a `downloadURL` (API-only access URLs) are skipped. You can \
                          limit to specific distributions by zero-based index within the \
                          downloadable list (see `data_gov.dataset` output).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "datasetId": {
                        "type": "string",
                        "description": "Dataset slug. Use the slug from search results or the dataset URL — do not construct or guess this value."
                    },
                    "distributionIndexes": {
                        "type": "array",
                        "items": {"type": "integer", "minimum": 0},
                        "description": "Optional zero-based indexes into the downloadable distributions list. If omitted, all downloadable distributions matching the format filter are downloaded."
                    },
                    "formats": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Optional list of distribution formats to include (e.g. CSV, JSON). Case-insensitive, matched against both `format` and `mediaType`."
                    },
                    "outputDir": {
                        "type": "string",
                        "description": "Optional directory to save files. Relative paths resolve against the current working directory. Defaults to the configured download directory."
                    },
                    "datasetSubdirectory": {
                        "type": "boolean",
                        "description": "Whether to create a dataset-named subdirectory inside the output directory.",
                        "default": true
                    }
                },
                "required": ["datasetId"],
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
        assert_eq!(TOOL_SPECS.len(), 5);
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
        let spec = find_tool_spec("data_gov_search").unwrap();
        assert_eq!(spec.method_name, "data_gov.search");
    }

    #[test]
    fn find_tool_spec_unknown_name_returns_none() {
        assert!(find_tool_spec("nonexistent_tool").is_none());
    }

    #[test]
    fn find_tool_spec_by_method_known() {
        let spec = find_tool_spec_by_method("data_gov.search").unwrap();
        assert_eq!(spec.tool_name, "data_gov_search");
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
