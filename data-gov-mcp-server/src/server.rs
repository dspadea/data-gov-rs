//! MCP server entry point — struct definition, construction, and run loop.

use data_gov::{DataGovClient, DataGovConfig, OperatingMode};
use data_gov_ckan::{ApiKey as CkanApiKey, CkanClient, Configuration as CkanConfiguration};
use serde_json::json;
use std::env;
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};

use crate::types::{Request, Response, ServerError};

/// Supported JSON-RPC methods advertised in the ready message.
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

/// The data.gov MCP server.
///
/// Reads JSON-RPC requests from stdin and writes responses to stdout.
pub struct DataGovMcpServer {
    pub(crate) data_gov: DataGovClient,
    pub(crate) ckan: CkanClient,
    pub(crate) portal_base_url: String,
}

/// Extract the portal base URL from a CKAN API base URL.
fn derive_portal_base_url(api_base: &str) -> String {
    let trimmed = api_base.trim_end_matches('/');
    if let Some(prefix) = trimmed.strip_suffix("/api/3") {
        prefix.to_string()
    } else if let Some(prefix) = trimmed.strip_suffix("/api") {
        prefix.to_string()
    } else {
        trimmed.to_string()
    }
}

impl DataGovMcpServer {
    /// Create and run the server (convenience entry point).
    pub async fn bootstrap() -> Result<(), ServerError> {
        let server = Self::new()?;
        server.run().await
    }

    /// Build a new server from environment variables.
    fn new() -> Result<Self, ServerError> {
        let api_key = env::var("DATA_GOV_API_KEY").ok();
        let base_url = env::var("DATA_GOV_BASE_URL").ok();
        let user_agent = env::var("DATA_GOV_USER_AGENT").ok();

        // Configure high-level data-gov client
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
        let portal_base_url = derive_portal_base_url(&ckan_configuration.base_path);
        let ckan = CkanClient::new(Arc::new(ckan_configuration));

        Ok(Self {
            data_gov,
            ckan,
            portal_base_url,
        })
    }

    /// Main run loop: read JSON-RPC lines from stdin, dispatch, write responses.
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

    /// Emit the server-ready announcement.
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

    /// Serialize and write a single response line.
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

    /// Validate the request and dispatch to the handler.
    async fn handle_request(&self, request: Request) -> Response {
        // JSON-RPC 2.0 requires jsonrpc field to be exactly "2.0" when present
        if let Some(ref version) = request.jsonrpc
            && version != "2.0"
        {
            return Response::error(
                request.id,
                ServerError::InvalidRequest(format!(
                    "invalid jsonrpc version: expected \"2.0\", got \"{version}\""
                )),
            );
        }

        match self.dispatch(&request.method, request.params).await {
            Ok(result) => Response::success(request.id, result),
            Err(err) => Response::error(request.id, err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_portal_base_url_strips_api_3() {
        assert_eq!(
            derive_portal_base_url("https://catalog.data.gov/api/3"),
            "https://catalog.data.gov"
        );
    }

    #[test]
    fn derive_portal_base_url_strips_api_3_with_trailing_slash() {
        assert_eq!(
            derive_portal_base_url("https://catalog.data.gov/api/3/"),
            "https://catalog.data.gov"
        );
    }

    #[test]
    fn derive_portal_base_url_strips_api_only() {
        assert_eq!(
            derive_portal_base_url("https://example.com/api"),
            "https://example.com"
        );
    }

    #[test]
    fn derive_portal_base_url_no_api_suffix() {
        assert_eq!(
            derive_portal_base_url("https://example.com/custom"),
            "https://example.com/custom"
        );
    }

    #[test]
    fn derive_portal_base_url_empty_string() {
        assert_eq!(derive_portal_base_url(""), "");
    }
}
