//! JSON-RPC request/response types and MCP parameter structs.

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

/// Reject `limit` values outside the inclusive `[min, max]` range advertised
/// by the tool's input schema.
///
/// The schema is informational on the wire; we still need to enforce it before
/// dispatching to upstream APIs that would otherwise return their own
/// (uglier) 4xx errors and burn a network round-trip.
pub(crate) fn validate_limit(
    method: &str,
    limit: Option<i32>,
    min: i32,
    max: i32,
) -> ServerResult<()> {
    if let Some(value) = limit
        && !(min..=max).contains(&value)
    {
        return Err(ServerError::InvalidParams(format!(
            "{method}: limit must be between {min} and {max}, got {value}"
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// MCP parameter and result structs
// ---------------------------------------------------------------------------

/// Parameters for `data_gov.search`.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct SearchParams {
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub limit: Option<i32>,
    #[serde(default)]
    pub after: Option<String>,
    #[serde(default)]
    pub organization: Option<String>,
    #[serde(default, rename = "organizationContains")]
    pub organization_contains: Option<String>,
}

/// Compact dataset summary returned in search results.
#[derive(Debug, Serialize)]
pub(crate) struct DatasetSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    pub slug: String,
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

/// Parameters for `data_gov.dataset`.
#[derive(Debug, Deserialize)]
pub(crate) struct DatasetParams {
    /// data.gov dataset slug (e.g., `electric-vehicle-population-data`).
    pub slug: String,
}

/// Parameters for `data_gov.autocompleteDatasets`.
#[derive(Debug, Deserialize)]
pub(crate) struct AutocompleteParams {
    pub partial: String,
    #[serde(default)]
    pub limit: Option<i32>,
}

/// MCP protocol versions this server can speak, oldest first.
///
/// Version identifiers are dates marking the last backwards-incompatible
/// change, not a sequence — a client asking for one we do not list gets
/// [`LATEST_PROTOCOL_VERSION`] instead.
pub(crate) const SUPPORTED_PROTOCOL_VERSIONS: &[&str] =
    &["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"];

/// The newest protocol version this server supports.
///
/// Returned when the client requests a version we do not recognise, or omits
/// the field entirely.
pub(crate) const LATEST_PROTOCOL_VERSION: &str = "2025-11-25";

/// Resolve the protocol version for a session.
///
/// Per the MCP lifecycle spec: if the server supports the requested version it
/// MUST reply with that same version; otherwise it MUST reply with another
/// version it supports, which SHOULD be the latest.
pub(crate) fn negotiate_protocol_version(requested: Option<&str>) -> &'static str {
    requested
        .and_then(|want| {
            SUPPORTED_PROTOCOL_VERSIONS
                .iter()
                .find(|supported| **supported == want)
                .copied()
        })
        .unwrap_or(LATEST_PROTOCOL_VERSION)
}

/// Parameters for `initialize`.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct InitializeParams {
    /// Protocol version the client wants to speak. Absent for older clients.
    #[serde(default, rename = "protocolVersion")]
    pub protocol_version: Option<String>,
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
    /// Negotiated protocol version. Required by the MCP schema — a client that
    /// validates the result will abort the handshake without it.
    #[serde(rename = "protocolVersion")]
    pub protocol_version: &'static str,
    #[serde(rename = "serverInfo")]
    pub server_info: ServerInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "clientInfo")]
    pub client_info: Option<ClientInfoSummary>,
}

impl InitializeResult {
    /// Build an initialize result, negotiating the protocol version and
    /// echoing back client info if provided.
    pub fn new(requested_version: Option<&str>, client_info: Option<ClientInfo>) -> Self {
        let client_info = client_info.map(|info| ClientInfoSummary {
            name: info.name,
            version: info.version,
        });

        Self {
            protocol_version: negotiate_protocol_version(requested_version),
            server_info: ServerInfo {
                name: "data-gov-mcp-server",
                version: env!("CARGO_PKG_VERSION"),
            },
            // `listChanged` is the schema's name for this. Our tool list is
            // static, so it is false: we never emit notifications/tools/list_changed.
            capabilities: Some(json!({
                "tools": {
                    "listChanged": false
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
    #[serde(default, rename = "distributionIndexes")]
    pub distribution_indexes: Option<Vec<usize>>,
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

    /// Revisions the MCP specification has actually published, oldest first.
    ///
    /// Deliberately literal. Deriving this from [`SUPPORTED_PROTOCOL_VERSIONS`]
    /// would make every assertion below agree with whatever that constant happens
    /// to say, so a revision quietly dropped — or invented — would stay green.
    const PUBLISHED_MCP_REVISIONS: [&str; 4] =
        ["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"];

    /// The revision marked *current* at modelcontextprotocol.io/specification/versioning.
    /// Bumping this is a deliberate act of adopting a new spec, not a side effect.
    const CURRENT_MCP_REVISION: &str = "2025-11-25";

    #[test]
    fn parse_required_params_succeeds_with_valid_json() {
        let params = Some(json!({"slug": "my-dataset"}));
        let result: ServerResult<DatasetParams> = parse_required_params("test_method", params);
        let parsed = result.expect("should succeed");
        assert_eq!(parsed.slug, "my-dataset");
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
            other => panic!("expected InvalidParams, got: {other:?}"),
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
    fn validate_limit_accepts_none() {
        validate_limit("m", None, 1, 1000).expect("None should pass");
    }

    #[test]
    fn validate_limit_accepts_value_in_range() {
        validate_limit("m", Some(1), 1, 1000).expect("min should pass");
        validate_limit("m", Some(1000), 1, 1000).expect("max should pass");
        validate_limit("m", Some(50), 1, 1000).expect("middle should pass");
    }

    #[test]
    fn validate_limit_rejects_below_min() {
        let err = validate_limit("m", Some(0), 1, 1000).expect_err("below min should fail");
        match err {
            ServerError::InvalidParams(msg) => {
                assert!(msg.contains("between 1 and 1000"), "got: {msg}");
                assert!(msg.contains("got 0"), "got: {msg}");
            }
            other => panic!("expected InvalidParams, got: {other:?}"),
        }
    }

    #[test]
    fn validate_limit_rejects_above_max() {
        let err = validate_limit("m", Some(1500), 1, 1000).expect_err("above max should fail");
        assert!(matches!(err, ServerError::InvalidParams(_)));
    }

    #[test]
    fn validate_limit_rejects_negative() {
        let err = validate_limit("m", Some(-1), 1, 1000).expect_err("negative should fail");
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

    #[test]
    fn search_params_all_fields() {
        let val = json!({
            "query": "climate",
            "limit": 10,
            "after": "cursor-xyz",
            "organization": "epa-gov",
            "organizationContains": "NASA"
        });
        let params: SearchParams = serde_json::from_value(val).expect("should parse");
        assert_eq!(params.query, "climate");
        assert_eq!(params.limit, Some(10));
        assert_eq!(params.after.as_deref(), Some("cursor-xyz"));
        assert_eq!(params.organization.as_deref(), Some("epa-gov"));
        assert_eq!(params.organization_contains.as_deref(), Some("NASA"));
    }

    #[test]
    fn search_params_defaults() {
        let val = json!({});
        let params: SearchParams = serde_json::from_value(val).expect("should parse");
        assert_eq!(params.query, "");
        assert!(params.limit.is_none());
        assert!(params.after.is_none());
        assert!(params.organization.is_none());
    }

    #[test]
    fn dataset_summary_skips_empty_formats() {
        let summary = DatasetSummary {
            identifier: None,
            slug: "test".to_string(),
            title: "Test".to_string(),
            organization: None,
            organization_slug: None,
            description: None,
            dataset_url: "https://example.com/dataset/test".to_string(),
            formats: vec![],
        };
        let json = serde_json::to_value(&summary).expect("should serialize");
        let obj = json.as_object().unwrap();
        assert!(!obj.contains_key("formats"));
        assert!(!obj.contains_key("identifier"));
        assert!(!obj.contains_key("organization"));
    }

    #[test]
    fn dataset_summary_includes_non_empty_formats() {
        let summary = DatasetSummary {
            identifier: Some("abc".to_string()),
            slug: "test".to_string(),
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
        assert!(obj.contains_key("identifier"));
        assert!(obj.contains_key("organization"));
        assert!(obj.contains_key("organizationSlug"));
        assert_eq!(obj["datasetUrl"], "https://example.com/dataset/test");
    }

    #[test]
    fn initialize_result_without_client_info() {
        let result = InitializeResult::new(None, None);
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
        let result = InitializeResult::new(None, Some(info));
        let ci = result.client_info.expect("should have client_info");
        assert_eq!(ci.name, "test-client");
        assert_eq!(ci.version.as_deref(), Some("1.0"));
    }

    /// MCP 2025-11-25, Version Negotiation: "If the server supports the
    /// requested protocol version, it MUST respond with the same version."
    ///
    /// The revision list is literal on purpose. Iterating
    /// `SUPPORTED_PROTOCOL_VERSIONS` would only prove `find(x in L) == x`,
    /// which holds for any contents of `L` — including a list that has never
    /// heard of the current revision.
    #[test]
    fn negotiate_echoes_every_published_revision() {
        for revision in PUBLISHED_MCP_REVISIONS {
            assert!(
                SUPPORTED_PROTOCOL_VERSIONS.contains(&revision),
                "{revision} is a published MCP revision but is no longer advertised: \
                 {SUPPORTED_PROTOCOL_VERSIONS:?}"
            );
            assert_eq!(negotiate_protocol_version(Some(revision)), revision);
        }
        for advertised in SUPPORTED_PROTOCOL_VERSIONS {
            assert!(
                PUBLISHED_MCP_REVISIONS.contains(advertised),
                "{advertised} is not a published MCP revision"
            );
        }
        assert_eq!(LATEST_PROTOCOL_VERSION, CURRENT_MCP_REVISION);
    }

    /// "Otherwise, the server MUST respond with another protocol version it
    /// supports. This SHOULD be the latest version supported by the server."
    /// The MUST is membership in the advertised set; the SHOULD is recency.
    #[test]
    fn negotiate_never_answers_with_an_unsupported_version() {
        for requested in [
            Some("1999-01-01"),
            Some("2025-11-24"),  // one day off a real revision
            Some("2025-11-25 "), // trailing whitespace is a different identifier
            Some("1.0.0"),       // the spec's own error example
            Some("latest"),
            Some(""),
            None,
        ] {
            let answered = negotiate_protocol_version(requested);
            assert!(
                SUPPORTED_PROTOCOL_VERSIONS.contains(&answered),
                "negotiating {requested:?} answered {answered:?}, which this server cannot speak"
            );
            assert_eq!(
                answered, CURRENT_MCP_REVISION,
                "fallback SHOULD be the current revision"
            );
        }
    }

    /// The fallback clause is about which revision is *newest*, not which array
    /// slot it occupies. `.last()` is positional: a newest-first list with
    /// `LATEST` pointing at the oldest revision passes that while downgrading
    /// every client. `YYYY-MM-DD` makes lexicographic max chronological, and
    /// the shape check below is what licenses using `max()`.
    #[test]
    fn latest_protocol_version_is_the_newest_supported() {
        let newest = SUPPORTED_PROTOCOL_VERSIONS
            .iter()
            .copied()
            .max()
            .expect("a server must support at least one revision");
        assert_eq!(LATEST_PROTOCOL_VERSION, newest);
        assert_eq!(
            LATEST_PROTOCOL_VERSION, CURRENT_MCP_REVISION,
            "server advertises {LATEST_PROTOCOL_VERSION} as its latest, but the current \
             published MCP revision is {CURRENT_MCP_REVISION}"
        );

        let mut sorted = SUPPORTED_PROTOCOL_VERSIONS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted.as_slice(),
            SUPPORTED_PROTOCOL_VERSIONS,
            "SUPPORTED_PROTOCOL_VERSIONS must be sorted oldest-first with no duplicates"
        );

        for v in SUPPORTED_PROTOCOL_VERSIONS {
            let b = v.as_bytes();
            assert!(
                b.len() == 10
                    && b[4] == b'-'
                    && b[7] == b'-'
                    && b.iter().filter(|c| c.is_ascii_digit()).count() == 8,
                "MCP revisions are YYYY-MM-DD date strings, got {v:?}"
            );
        }
    }

    /// `InitializeResult` requires protocolVersion, capabilities and serverInfo;
    /// serverInfo requires name and version. Probing every advertised revision
    /// rather than one is what kills a constructor that never reads its
    /// argument — with a single probe, a hardcoded constant is
    /// indistinguishable from working negotiation.
    #[test]
    fn initialize_result_serializes_protocol_version_and_spec_capabilities() {
        for requested in SUPPORTED_PROTOCOL_VERSIONS {
            let v = serde_json::to_value(InitializeResult::new(Some(requested), None))
                .expect("serializes");
            assert_eq!(
                v["protocolVersion"].as_str(),
                Some(*requested),
                "a supported version must be echoed verbatim, got: {v}"
            );
        }

        for unsupported in ["1999-01-01", "", "latest", "2.0"] {
            let v = serde_json::to_value(InitializeResult::new(Some(unsupported), None))
                .expect("serializes");
            let answered = v["protocolVersion"]
                .as_str()
                .expect("a string protocolVersion");
            assert!(
                SUPPORTED_PROTOCOL_VERSIONS.contains(&answered),
                "answered {answered:?} for unsupported {unsupported:?}"
            );
            assert_eq!(answered, CURRENT_MCP_REVISION);
        }

        let v = serde_json::to_value(InitializeResult::new(None, None)).expect("serializes");
        assert_eq!(v["protocolVersion"].as_str(), Some(CURRENT_MCP_REVISION));
        assert_eq!(v["capabilities"]["tools"]["listChanged"], json!(false));
        assert!(
            v["capabilities"]["tools"].get("list").is_none(),
            "`list` is not a key of the tools capability"
        );
        assert!(
            v.get("server_info").is_none(),
            "result keys are camelCase, got: {v}"
        );
        assert_eq!(v["serverInfo"]["name"], "data-gov-mcp-server");
        assert!(
            v["serverInfo"]["version"]
                .as_str()
                .is_some_and(|s| !s.is_empty()),
            "serverInfo.version must be a non-empty string, got: {v}"
        );
    }
}
