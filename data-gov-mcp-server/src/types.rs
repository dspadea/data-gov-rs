//! JSON-RPC request/response types and MCP parameter structs.

use data_gov_ckan::CkanError;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use thiserror::Error;

/// Incoming JSON-RPC request.
#[derive(Debug, Deserialize)]
pub(crate) struct Request {
    #[serde(default)]
    pub jsonrpc: Option<String>,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

/// Outgoing JSON-RPC response.
#[derive(Debug, Serialize)]
pub(crate) struct Response {
    jsonrpc: &'static str,
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ResponseError>,
}

impl Response {
    /// Build a success response.
    pub fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Build an error response.
    pub fn error(id: Option<Value>, error: ServerError) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(ResponseError::from(error)),
        }
    }
}

/// JSON-RPC error payload.
#[derive(Debug, Serialize)]
pub(crate) struct ResponseError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl From<ServerError> for ResponseError {
    fn from(err: ServerError) -> Self {
        match err {
            ServerError::InvalidRequest(message) => Self {
                code: -32600,
                message,
                data: None,
            },
            ServerError::InvalidMethod(method) => Self {
                code: -32601,
                message: format!("Unknown method: {method}"),
                data: None,
            },
            ServerError::InvalidParams(message) => Self {
                code: -32602,
                message,
                data: None,
            },
            ServerError::Json(err) => Self {
                code: -32700,
                message: err.to_string(),
                data: None,
            },
            ServerError::Io(err) => Self {
                code: -32020,
                message: err.to_string(),
                data: None,
            },
            ServerError::DataGov(err) => Self {
                code: -32010,
                message: err.to_string(),
                data: None,
            },
            ServerError::Ckan(err) => Self {
                code: -32011,
                message: err.to_string(),
                data: None,
            },
            ServerError::Serialization(err) => Self {
                code: -32603,
                message: err.to_string(),
                data: None,
            },
        }
    }
}

/// Server-side errors mapped to JSON-RPC error codes.
#[derive(Debug, Error)]
pub enum ServerError {
    /// The request was malformed.
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    /// The requested method does not exist.
    #[error("unknown method: {0}")]
    InvalidMethod(String),
    /// The parameters are invalid for the requested method.
    #[error("invalid parameters: {0}")]
    InvalidParams(String),
    /// JSON parse error.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// I/O error.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// High-level data-gov client error.
    #[error(transparent)]
    DataGov(#[from] data_gov::DataGovError),
    /// Low-level CKAN client error.
    #[error(transparent)]
    Ckan(#[from] CkanError),
    /// Serialization error (distinct from parse errors).
    #[error("serialization error: {0}")]
    Serialization(serde_json::Error),
}

/// Convenience alias used throughout the server.
pub(crate) type ServerResult<T> = Result<T, ServerError>;

/// Deserialize required params from a JSON-RPC request, returning an error if missing.
pub(crate) fn parse_required_params<T>(method: &str, params: Option<Value>) -> ServerResult<T>
where
    T: DeserializeOwned,
{
    match params {
        Some(value) => serde_json::from_value(value)
            .map_err(|err| ServerError::InvalidParams(format!("{method}: {err}"))),
        None => Err(ServerError::InvalidParams(format!(
            "{method}: missing parameters"
        ))),
    }
}

/// Deserialize optional params, falling back to `T::default()` when absent.
pub(crate) fn parse_optional_params<T>(method: &str, params: Option<Value>) -> ServerResult<T>
where
    T: DeserializeOwned + Default,
{
    match params {
        Some(value) => serde_json::from_value(value)
            .map_err(|err| ServerError::InvalidParams(format!("{method}: {err}"))),
        None => Ok(T::default()),
    }
}

// ---------------------------------------------------------------------------
// MCP parameter and result structs
// ---------------------------------------------------------------------------

/// Parameters for `data_gov.search`.
#[derive(Debug, Deserialize)]
pub(crate) struct SearchParams {
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub limit: Option<i32>,
    #[serde(default)]
    pub offset: Option<i32>,
    #[serde(default)]
    pub organization: Option<String>,
    #[serde(default, rename = "format")]
    pub format: Option<String>,
    #[serde(default, rename = "organizationContains")]
    pub organization_contains: Option<String>,
}

/// Compact dataset summary returned in search results.
#[derive(Debug, Serialize)]
pub(crate) struct DatasetSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "organizationSlug")]
    pub organization_slug: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "datasetUrl")]
    pub dataset_url: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub formats: Vec<String>,
}

/// Parameters for `data_gov.dataset` and `ckan.packageShow`.
#[derive(Debug, Deserialize)]
pub(crate) struct DatasetParams {
    pub id: String,
}

/// Parameters for `data_gov.autocompleteDatasets`.
#[derive(Debug, Deserialize)]
pub(crate) struct AutocompleteParams {
    pub partial: String,
    #[serde(default)]
    pub limit: Option<i32>,
}

/// Parameters for `initialize`.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct InitializeParams {
    #[serde(default, rename = "clientInfo")]
    pub client_info: Option<ClientInfo>,
}

/// Client information sent during initialization.
#[derive(Debug, Deserialize)]
pub(crate) struct ClientInfo {
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
}

/// Result of the `initialize` handshake.
#[derive(Debug, Serialize)]
pub(crate) struct InitializeResult {
    #[serde(rename = "serverInfo")]
    pub server_info: ServerInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "clientInfo")]
    pub client_info: Option<ClientInfoSummary>,
}

impl InitializeResult {
    /// Build an initialize result, echoing back client info if provided.
    pub fn new(client_info: Option<ClientInfo>) -> Self {
        let client_info = client_info.map(|info| ClientInfoSummary {
            name: info.name,
            version: info.version,
        });

        Self {
            server_info: ServerInfo {
                name: "data-gov-mcp-server",
                version: env!("CARGO_PKG_VERSION"),
            },
            capabilities: Some(json!({
                "tools": {
                    "list": true
                }
            })),
            client_info,
        }
    }
}

/// Server identity sent during initialization.
#[derive(Debug, Serialize)]
pub(crate) struct ServerInfo {
    pub name: &'static str,
    pub version: &'static str,
}

/// Echo of client info in the initialize response.
#[derive(Debug, Serialize)]
pub(crate) struct ClientInfoSummary {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Parameters for `data_gov.downloadResources`.
#[derive(Debug, Deserialize)]
pub(crate) struct DownloadResourcesParams {
    #[serde(rename = "datasetId")]
    pub dataset_id: String,
    #[serde(default, rename = "resourceIds")]
    pub resource_ids: Option<Vec<String>>,
    #[serde(default)]
    pub formats: Option<Vec<String>>,
    #[serde(default, rename = "outputDir")]
    pub output_dir: Option<String>,
    #[serde(default, rename = "datasetSubdirectory")]
    pub dataset_subdirectory: Option<bool>,
}

/// Parameters for `data_gov.listOrganizations`.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct ListOrganizationsParams {
    #[serde(default)]
    pub limit: Option<i32>,
}

/// Parameters for `ckan.packageSearch`.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct PackageSearchParams {
    #[serde(default, rename = "query")]
    pub query: Option<String>,
    #[serde(default)]
    pub rows: Option<i32>,
    #[serde(default)]
    pub start: Option<i32>,
    #[serde(default, rename = "filter")]
    pub filter: Option<String>,
}

/// Parameters for `ckan.organizationList`.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct OrganizationListParams {
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default)]
    pub limit: Option<i32>,
    #[serde(default)]
    pub offset: Option<i32>,
}

/// Parameters for `tools/list`.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct ListToolsParams {
    #[serde(default, rename = "cursor")]
    pub cursor: Option<String>,
}

/// Parameters for `tools/call`.
#[derive(Debug, Deserialize)]
pub(crate) struct CallToolParams {
    pub name: String,
    #[serde(default)]
    pub arguments: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -----------------------------------------------------------------------
    // parse_required_params / parse_optional_params
    // -----------------------------------------------------------------------

    #[test]
    fn parse_required_params_succeeds_with_valid_json() {
        let params = Some(json!({"id": "my-dataset"}));
        let result: ServerResult<DatasetParams> = parse_required_params("test_method", params);
        let parsed = result.expect("should succeed");
        assert_eq!(parsed.id, "my-dataset");
    }

    #[test]
    fn parse_required_params_fails_when_none() {
        let result: ServerResult<DatasetParams> = parse_required_params("test_method", None);
        let err = result.expect_err("should fail");
        match err {
            ServerError::InvalidParams(msg) => {
                assert!(msg.contains("test_method"));
                assert!(msg.contains("missing parameters"));
            }
            other => panic!("expected InvalidParams, got: {:?}", other),
        }
    }

    #[test]
    fn parse_required_params_fails_with_wrong_shape() {
        let params = Some(json!({"wrong_field": 42}));
        let result: ServerResult<DatasetParams> = parse_required_params("test_method", params);
        let err = result.expect_err("should fail");
        assert!(matches!(err, ServerError::InvalidParams(_)));
    }

    #[test]
    fn parse_optional_params_returns_default_when_none() {
        let result: ServerResult<ListOrganizationsParams> =
            parse_optional_params("test_method", None);
        let parsed = result.expect("should succeed");
        assert!(parsed.limit.is_none());
    }

    #[test]
    fn parse_optional_params_parses_provided_value() {
        let params = Some(json!({"limit": 25}));
        let result: ServerResult<ListOrganizationsParams> =
            parse_optional_params("test_method", params);
        let parsed = result.expect("should succeed");
        assert_eq!(parsed.limit, Some(25));
    }

    // -----------------------------------------------------------------------
    // Response construction
    // -----------------------------------------------------------------------

    #[test]
    fn response_success_has_correct_structure() {
        let resp = Response::success(Some(json!(1)), json!({"data": "test"}));
        assert_eq!(resp.jsonrpc, "2.0");
        assert_eq!(resp.id, Some(json!(1)));
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
    }

    #[test]
    fn response_error_has_correct_structure() {
        let resp = Response::error(
            Some(json!(2)),
            ServerError::InvalidMethod("foo".to_string()),
        );
        assert_eq!(resp.jsonrpc, "2.0");
        assert_eq!(resp.id, Some(json!(2)));
        assert!(resp.result.is_none());
        let error = resp.error.expect("should have error");
        assert_eq!(error.code, -32601);
        assert!(error.message.contains("foo"));
    }

    #[test]
    fn response_success_serializes_without_error_field() {
        let resp = Response::success(Some(json!(1)), json!("ok"));
        let json_str = serde_json::to_string(&resp).expect("should serialize");
        assert!(!json_str.contains("\"error\""));
    }

    #[test]
    fn response_error_serializes_without_result_field() {
        let resp = Response::error(None, ServerError::InvalidRequest("bad".into()));
        let json_str = serde_json::to_string(&resp).expect("should serialize");
        assert!(!json_str.contains("\"result\""));
    }

    // -----------------------------------------------------------------------
    // ResponseError::from(ServerError)  — JSON-RPC error codes
    // -----------------------------------------------------------------------

    #[test]
    fn error_code_invalid_request() {
        let err = ResponseError::from(ServerError::InvalidRequest("bad".into()));
        assert_eq!(err.code, -32600);
    }

    #[test]
    fn error_code_invalid_method() {
        let err = ResponseError::from(ServerError::InvalidMethod("foo".into()));
        assert_eq!(err.code, -32601);
        assert!(err.message.contains("foo"));
    }

    #[test]
    fn error_code_invalid_params() {
        let err = ResponseError::from(ServerError::InvalidParams("missing x".into()));
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn error_code_json_parse() {
        let serde_err = serde_json::from_str::<Value>("not json").unwrap_err();
        let err = ResponseError::from(ServerError::Json(serde_err));
        assert_eq!(err.code, -32700);
    }

    #[test]
    fn error_code_io() {
        let io_err = std::io::Error::other("disk full");
        let err = ResponseError::from(ServerError::Io(io_err));
        assert_eq!(err.code, -32020);
    }

    // -----------------------------------------------------------------------
    // Request deserialization
    // -----------------------------------------------------------------------

    #[test]
    fn request_deserializes_full_json_rpc() {
        let json_str = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#;
        let req: Request = serde_json::from_str(json_str).expect("should parse");
        assert_eq!(req.method, "tools/list");
        assert_eq!(req.id, Some(json!(1)));
        assert!(req.params.is_some());
    }

    #[test]
    fn request_deserializes_minimal() {
        let json_str = r#"{"method":"initialize"}"#;
        let req: Request = serde_json::from_str(json_str).expect("should parse");
        assert_eq!(req.method, "initialize");
        assert!(req.id.is_none());
        assert!(req.params.is_none());
    }

    #[test]
    fn request_rejects_missing_method() {
        let json_str = r#"{"jsonrpc":"2.0","id":1}"#;
        let result = serde_json::from_str::<Request>(json_str);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // SearchParams deserialization
    // -----------------------------------------------------------------------

    #[test]
    fn search_params_all_fields() {
        let val = json!({
            "query": "climate",
            "limit": 10,
            "offset": 20,
            "organization": "epa-gov",
            "format": "CSV",
            "organizationContains": "NASA"
        });
        let params: SearchParams = serde_json::from_value(val).expect("should parse");
        assert_eq!(params.query, "climate");
        assert_eq!(params.limit, Some(10));
        assert_eq!(params.offset, Some(20));
        assert_eq!(params.organization.as_deref(), Some("epa-gov"));
        assert_eq!(params.format.as_deref(), Some("CSV"));
        assert_eq!(params.organization_contains.as_deref(), Some("NASA"));
    }

    #[test]
    fn search_params_defaults() {
        let val = json!({});
        let params: SearchParams = serde_json::from_value(val).expect("should parse");
        assert_eq!(params.query, "");
        assert!(params.limit.is_none());
        assert!(params.organization.is_none());
    }

    // -----------------------------------------------------------------------
    // DatasetSummary serialization
    // -----------------------------------------------------------------------

    #[test]
    fn dataset_summary_skips_empty_formats() {
        let summary = DatasetSummary {
            id: None,
            name: "test".to_string(),
            title: "Test".to_string(),
            organization: None,
            organization_slug: None,
            description: None,
            dataset_url: "https://example.com/dataset/test".to_string(),
            formats: vec![],
        };
        let json = serde_json::to_value(&summary).expect("should serialize");
        assert!(!json.as_object().unwrap().contains_key("formats"));
        assert!(!json.as_object().unwrap().contains_key("id"));
        assert!(!json.as_object().unwrap().contains_key("organization"));
    }

    #[test]
    fn dataset_summary_includes_non_empty_formats() {
        let summary = DatasetSummary {
            id: Some("abc".to_string()),
            name: "test".to_string(),
            title: "Test".to_string(),
            organization: Some("EPA".to_string()),
            organization_slug: Some("epa-gov".to_string()),
            description: Some("A dataset".to_string()),
            dataset_url: "https://example.com/dataset/test".to_string(),
            formats: vec!["CSV".to_string(), "JSON".to_string()],
        };
        let json = serde_json::to_value(&summary).expect("should serialize");
        let obj = json.as_object().unwrap();
        assert!(obj.contains_key("formats"));
        assert!(obj.contains_key("id"));
        assert!(obj.contains_key("organization"));
        assert!(obj.contains_key("organizationSlug"));
        assert_eq!(obj["datasetUrl"], "https://example.com/dataset/test");
    }

    // -----------------------------------------------------------------------
    // InitializeResult
    // -----------------------------------------------------------------------

    #[test]
    fn initialize_result_without_client_info() {
        let result = InitializeResult::new(None);
        assert_eq!(result.server_info.name, "data-gov-mcp-server");
        assert!(result.client_info.is_none());
        assert!(result.capabilities.is_some());
    }

    #[test]
    fn initialize_result_with_client_info() {
        let info = ClientInfo {
            name: "test-client".to_string(),
            version: Some("1.0".to_string()),
        };
        let result = InitializeResult::new(Some(info));
        let ci = result.client_info.expect("should have client_info");
        assert_eq!(ci.name, "test-client");
        assert_eq!(ci.version.as_deref(), Some("1.0"));
    }
}
