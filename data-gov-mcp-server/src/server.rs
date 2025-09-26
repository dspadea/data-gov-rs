use data_gov::{DataGovClient, DataGovConfig, OperatingMode};
use data_gov_ckan::{
    ApiKey as CkanApiKey, CkanClient, CkanError, Configuration as CkanConfiguration,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::env;
use std::sync::Arc;
use thiserror::Error;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};

const METHODS: &[&str] = &[
    "data_gov.search",
    "data_gov.dataset",
    "data_gov.autocompleteDatasets",
    "data_gov.listOrganizations",
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
        match method {
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
