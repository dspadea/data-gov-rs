use data_gov::{DataGovClient, DataGovConfig, OperatingMode};
use data_gov_ckan::{
    ApiKey as CkanApiKey, CkanClient, CkanError, Configuration as CkanConfiguration,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};

const METHODS: &[&str] = &[
    "initialize",
    "initialized",
    "shutdown",
    "tools/list",
    "data_gov.search",
    "data_gov.dataset",
    "data_gov.autocompleteDatasets",
    "data_gov.listOrganizations",
    "data_gov.downloadResources",
    "ckan.packageSearch",
    "ckan.packageShow",
    "ckan.organizationList",
];

pub struct DataGovMcpServer {
    data_gov: DataGovClient,
    ckan: CkanClient,
}

impl DataGovMcpServer {
    pub async fn bootstrap() -> Result<(), ServerError> {
        let server = Self::new()?;
        server.run().await
    }

    fn new() -> Result<Self, ServerError> {
        let api_key = env::var("DATA_GOV_API_KEY").ok();
        let base_url = env::var("DATA_GOV_BASE_URL").ok();
        let user_agent = env::var("DATA_GOV_USER_AGENT").ok();

        // Configure high level data-gov client
        let mut config = DataGovConfig::new().with_mode(OperatingMode::CommandLine);
        if let Some(ref key) = api_key {
            config = config.with_api_key(key.clone());
        }
        if let Some(ref ua) = user_agent {
            config = config.with_user_agent(ua.clone());
        }
        let data_gov = DataGovClient::with_config(config)?;

        // Configure low-level CKAN client
        let mut ckan_configuration = CkanConfiguration::default();
        if let Some(url) = base_url {
            ckan_configuration.base_path = url;
        }
        if let Some(ua) = user_agent {
            ckan_configuration.user_agent = Some(ua);
        }
        if let Some(key) = api_key {
            ckan_configuration.api_key = Some(CkanApiKey { prefix: None, key });
        }
        let ckan = CkanClient::new(Arc::new(ckan_configuration));

        Ok(Self { data_gov, ckan })
    }

    async fn run(self) -> Result<(), ServerError> {
        let stdin = io::stdin();
        let stdout = io::stdout();

        let reader = BufReader::new(stdin);
        let mut writer = BufWriter::new(stdout);

        self.send_ready(&mut writer).await?;

        let mut lines = reader.lines();

        while let Some(line) = lines.next_line().await? {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let request = match serde_json::from_str::<Request>(trimmed) {
                Ok(request) => request,
                Err(err) => {
                    tracing::warn!("invalid request: {err}");
                    let response =
                        Response::error(None, ServerError::InvalidRequest(err.to_string()));
                    self.write_response(&mut writer, &response).await?;
                    continue;
                }
            };

            let response = self.handle_request(request).await;
            self.write_response(&mut writer, &response).await?;
        }

        Ok(())
    }

    async fn send_ready(&self, writer: &mut BufWriter<io::Stdout>) -> Result<(), ServerError> {
        let ready = json!({
            "jsonrpc": "2.0",
            "id": null,
            "result": {
                "server": "data-gov-mcp-server",
                "version": env!("CARGO_PKG_VERSION"),
                "methods": METHODS,
            }
        });

        let payload = serde_json::to_string(&ready).map_err(ServerError::Serialization)?;
        writer.write_all(payload.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;
        tracing::info!("data-gov MCP server ready");
        Ok(())
    }

    async fn write_response(
        &self,
        writer: &mut BufWriter<io::Stdout>,
        response: &Response,
    ) -> Result<(), ServerError> {
        let payload = serde_json::to_string(response).map_err(ServerError::Serialization)?;
        writer.write_all(payload.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;
        Ok(())
    }

    async fn handle_request(&self, request: Request) -> Response {
        match self.dispatch(&request.method, request.params).await {
            Ok(result) => Response::success(request.id, result),
            Err(err) => Response::error(request.id, err),
        }
    }

    async fn dispatch(&self, method: &str, params: Option<Value>) -> Result<Value, ServerError> {
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
            "data_gov.search" => {
                let params: SearchParams = parse_required_params(method, params)?;
                let result = self
                    .data_gov
                    .search(
                        &params.query,
                        params.limit,
                        params.offset,
                        params.organization.as_deref(),
                        params.format.as_deref(),
                    )
                    .await?;
                Ok(serde_json::to_value(result).map_err(ServerError::Serialization)?)
            }
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
            "data_gov.downloadResources" => {
                let params: DownloadResourcesParams = parse_required_params(method, params)?;
                let dataset = self.data_gov.get_dataset(&params.dataset_id).await?;

                let dataset_slug = dataset.name.clone();
                let dataset_title = dataset.title.clone();
                let dataset_id = dataset.id.as_ref().map(|id| id.to_string());

                let mut missing_resource_ids: Vec<String> = Vec::new();
                let mut unavailable_formats: Vec<String> = Vec::new();

                if params
                    .resource_ids
                    .as_ref()
                    .map(|ids| ids.is_empty())
                    .unwrap_or(false)
                {
                    return Err(ServerError::InvalidParams(
                        "data_gov.downloadResources: resourceIds cannot be empty".to_string(),
                    ));
                }

                let mut resources = DataGovClient::get_downloadable_resources(&dataset);

                if let Some(resource_ids) = params.resource_ids.as_ref() {
                    let normalized: Vec<(String, String)> = resource_ids
                        .iter()
                        .map(|id| {
                            let trimmed = id.trim().to_string();
                            let normalized = trimmed.to_ascii_lowercase();
                            (trimmed, normalized)
                        })
                        .collect();

                    let available_ids: HashSet<String> = resources
                        .iter()
                        .filter_map(|resource| {
                            resource
                                .id
                                .as_ref()
                                .map(|uuid| uuid.to_string().to_ascii_lowercase())
                        })
                        .collect();

                    for (original, normalized) in &normalized {
                        if !available_ids.contains(normalized) {
                            missing_resource_ids.push(original.clone());
                        }
                    }

                    let id_filter: HashSet<String> = normalized
                        .into_iter()
                        .map(|(_, normalized)| normalized)
                        .collect();

                    resources.retain(|resource| {
                        resource
                            .id
                            .as_ref()
                            .map(|uuid| id_filter.contains(&uuid.to_string().to_ascii_lowercase()))
                            .unwrap_or(false)
                    });
                }

                if let Some(formats) = params.formats.as_ref() {
                    let normalized: Vec<(String, String)> = formats
                        .iter()
                        .map(|fmt| {
                            let trimmed = fmt.trim().to_string();
                            let normalized = trimmed.to_ascii_lowercase();
                            (trimmed, normalized)
                        })
                        .collect();

                    let available_formats: HashSet<String> = resources
                        .iter()
                        .filter_map(|resource| {
                            resource.format.as_ref().map(|fmt| fmt.to_ascii_lowercase())
                        })
                        .collect();

                    for (original, normalized) in &normalized {
                        if !available_formats.contains(normalized) {
                            unavailable_formats.push(original.clone());
                        }
                    }

                    let format_filter: HashSet<String> = normalized
                        .into_iter()
                        .map(|(_, normalized)| normalized)
                        .collect();

                    resources.retain(|resource| {
                        resource
                            .format
                            .as_ref()
                            .map(|fmt| format_filter.contains(&fmt.to_ascii_lowercase()))
                            .unwrap_or(false)
                    });
                }

                if resources.is_empty() {
                    let mut message =
                        "data_gov.downloadResources: no matching downloadable resources"
                            .to_string();
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

                let use_dataset_subdir = params.dataset_subdirectory.unwrap_or(false);

                let resolved_output_dir = if let Some(dir) = params.output_dir.as_ref() {
                    let mut path = PathBuf::from(dir);
                    if !path.is_absolute() {
                        path = std::env::current_dir().map_err(ServerError::Io)?.join(path);
                    }
                    if use_dataset_subdir {
                        path = path.join(&dataset_slug);
                    }
                    Some(path)
                } else {
                    None
                };

                let target_dir = resolved_output_dir
                    .clone()
                    .unwrap_or_else(|| self.data_gov.download_dir().join(&dataset_slug));

                let selected_resources = resources;

                let download_results = if let Some(dir) = resolved_output_dir.as_ref() {
                    self.data_gov
                        .download_resources(&selected_resources, Some(dir.as_path()))
                        .await
                } else {
                    self.data_gov
                        .download_dataset_resources(&selected_resources, &dataset_slug)
                        .await
                };

                let mut downloads = Vec::with_capacity(selected_resources.len());
                let mut success_count = 0usize;
                let mut error_count = 0usize;

                for (resource, result) in
                    selected_resources.iter().zip(download_results.into_iter())
                {
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
                        "id": dataset_id,
                        "name": dataset_slug,
                        "title": dataset_title,
                    },
                    "downloadDirectory": target_dir.to_string_lossy(),
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
}

#[derive(Debug, Deserialize)]
struct Request {
    #[serde(default)]
    _jsonrpc: Option<String>,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
struct Response {
    jsonrpc: &'static str,
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ResponseError>,
}

impl Response {
    fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: Option<Value>, error: ServerError) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(ResponseError::from(error)),
        }
    }
}

#[derive(Debug, Serialize)]
struct ResponseError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
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

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("unknown method: {0}")]
    InvalidMethod(String),
    #[error("invalid parameters: {0}")]
    InvalidParams(String),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    DataGov(#[from] data_gov::DataGovError),
    #[error(transparent)]
    Ckan(#[from] CkanError),
    #[error("serialization error: {0}")]
    Serialization(serde_json::Error),
}

type ServerResult<T> = Result<T, ServerError>;

fn parse_required_params<T>(method: &str, params: Option<Value>) -> ServerResult<T>
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

fn parse_optional_params<T>(method: &str, params: Option<Value>) -> ServerResult<T>
where
    T: DeserializeOwned + Default,
{
    match params {
        Some(value) => serde_json::from_value(value)
            .map_err(|err| ServerError::InvalidParams(format!("{method}: {err}"))),
        None => Ok(T::default()),
    }
}

#[derive(Debug, Deserialize)]
struct SearchParams {
    query: String,
    #[serde(default)]
    limit: Option<i32>,
    #[serde(default)]
    offset: Option<i32>,
    #[serde(default)]
    organization: Option<String>,
    #[serde(default, rename = "format")]
    format: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DatasetParams {
    id: String,
}

#[derive(Debug, Deserialize)]
struct AutocompleteParams {
    partial: String,
    #[serde(default)]
    limit: Option<i32>,
}

#[derive(Debug, Default, Deserialize)]
struct InitializeParams {
    #[serde(default, rename = "clientInfo")]
    client_info: Option<ClientInfo>,
}

#[derive(Debug, Deserialize)]
struct ClientInfo {
    name: String,
    #[serde(default)]
    version: Option<String>,
}

#[derive(Debug, Serialize)]
struct InitializeResult {
    #[serde(rename = "serverInfo")]
    server_info: ServerInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    capabilities: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "clientInfo")]
    client_info: Option<ClientInfoSummary>,
}

impl InitializeResult {
    fn new(client_info: Option<ClientInfo>) -> Self {
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

#[derive(Debug, Serialize)]
struct ServerInfo {
    name: &'static str,
    version: &'static str,
}

#[derive(Debug, Serialize)]
struct ClientInfoSummary {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DownloadResourcesParams {
    #[serde(rename = "datasetId")]
    dataset_id: String,
    #[serde(default, rename = "resourceIds")]
    resource_ids: Option<Vec<String>>,
    #[serde(default)]
    formats: Option<Vec<String>>,
    #[serde(default, rename = "outputDir")]
    output_dir: Option<String>,
    #[serde(default, rename = "datasetSubdirectory")]
    dataset_subdirectory: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct ListOrganizationsParams {
    #[serde(default)]
    limit: Option<i32>,
}

#[derive(Debug, Default, Deserialize)]
struct PackageSearchParams {
    #[serde(default, rename = "query")]
    query: Option<String>,
    #[serde(default)]
    rows: Option<i32>,
    #[serde(default)]
    start: Option<i32>,
    #[serde(default, rename = "filter")]
    filter: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct OrganizationListParams {
    #[serde(default)]
    sort: Option<String>,
    #[serde(default)]
    limit: Option<i32>,
    #[serde(default)]
    offset: Option<i32>,
}

#[derive(Debug, Default, Deserialize)]
struct ListToolsParams {
    #[serde(default, rename = "cursor")]
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CallToolParams {
    name: String,
    #[serde(default)]
    arguments: Option<Value>,
}

#[derive(Debug, Serialize)]
struct ToolSpec {
    tool_name: &'static str,
    method_name: &'static str,
    description: &'static str,
    input_schema: Value,
}

#[derive(Debug, Serialize)]
struct ListToolsResult {
    tools: Vec<ToolDescriptor>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "nextCursor")]
    next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
struct ToolDescriptor {
    name: &'static str,
    description: &'static str,
    #[serde(rename = "inputSchema")]
    input_schema: Value,
}

#[derive(Debug, Serialize)]
struct ToolResponse {
    content: Vec<ToolContent>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "isError")]
    is_error: Option<bool>,
}

impl ToolResponse {
    fn from_value(value: Value) -> Self {
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

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum ToolContent {
    #[serde(rename = "json")]
    Json { json: Value },
    #[serde(rename = "text")]
    Text { text: String },
}

fn tool_descriptors() -> Vec<ToolDescriptor> {
    tool_specs()
        .into_iter()
        .map(|spec| ToolDescriptor {
            name: spec.tool_name,
            description: spec.description,
            input_schema: spec.input_schema,
        })
        .collect()
}

fn find_tool_spec(name: &str) -> Option<ToolSpec> {
    tool_specs().into_iter().find(|spec| spec.tool_name == name)
}

fn find_tool_spec_by_method(method: &str) -> Option<ToolSpec> {
    tool_specs()
        .into_iter()
        .find(|spec| spec.method_name == method)
}

fn tool_specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            tool_name: "data_gov_search",
            method_name: "data_gov.search",
            description: "Search datasets on data.gov with optional filters",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Full-text search query"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 1000, "description": "Maximum number of results"},
                    "offset": {"type": "integer", "minimum": 0, "description": "Result offset for pagination"},
                    "organization": {"type": "string", "description": "Filter results to a specific organization"},
                    "format": {"type": "string", "description": "Filter results by resource format e.g. CSV"}
                },
                "required": ["query"],
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
            description: "Perform a low-level CKAN package_search request",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": ["string", "null"], "description": "Full-text search query"},
                    "rows": {"type": ["integer", "null"], "minimum": 1, "maximum": 1000, "description": "Number of rows to return"},
                    "start": {"type": ["integer", "null"], "minimum": 0, "description": "Offset into result set"},
                    "filter": {"type": ["string", "null"], "description": "Filter query in CKAN syntax"}
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
}
