//! MCP server entry point — struct definition, construction, and run loop.

use data_gov::{DataGovClient, DataGovConfig, OperatingMode};
use std::env;
use tokio::io::{
    self, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, BufWriter,
};

use crate::types::{Request, Response, ServerError};

/// The data.gov MCP server.
///
/// Reads JSON-RPC requests from stdin and writes responses to stdout.
pub struct DataGovMcpServer {
    pub(crate) data_gov: DataGovClient,
    pub(crate) portal_base_url: String,
}

impl DataGovMcpServer {
    /// Create and run the server (convenience entry point).
    pub async fn bootstrap() -> Result<(), ServerError> {
        let server = Self::new()?;
        server.run().await
    }

    /// Build a new server from environment variables.
    fn new() -> Result<Self, ServerError> {
        let base_url = env::var("DATA_GOV_BASE_URL").ok();
        let user_agent = env::var("DATA_GOV_USER_AGENT").ok();

        let mut config = DataGovConfig::new().with_mode(OperatingMode::CommandLine);
        if let Some(url) = base_url {
            config = config.with_base_url(url);
        }
        if let Some(ua) = user_agent {
            config = config.with_user_agent(ua);
        }
        let portal_base_url = config.catalog_config.base_path.clone();
        let data_gov = DataGovClient::with_config(config)?;

        Ok(Self {
            data_gov,
            portal_base_url,
        })
    }

    /// Main run loop: read JSON-RPC lines from stdin, dispatch, write responses.
    async fn run(self) -> Result<(), ServerError> {
        // Nothing is written to stdout until a request arrives. stdout is the
        // protocol stream, and MCP expects the server to stay silent until it
        // answers `initialize`; lifecycle chatter goes to stderr via tracing.
        tracing::info!("data-gov MCP server ready");

        self.serve(io::stdin(), io::stdout()).await
    }

    /// Serve the JSON-RPC line protocol over an arbitrary reader and writer.
    ///
    /// [`run`](Self::run) supplies stdin and stdout; tests supply in-memory
    /// pipes, so every protocol edge is reachable without a real process.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::Io`] when the transport itself fails. A malformed
    /// message is answered on the wire, not returned here.
    pub(crate) async fn serve<R, W>(&self, reader: R, writer: W) -> Result<(), ServerError>
    where
        R: AsyncRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        let reader = BufReader::new(reader);
        let mut writer = BufWriter::new(writer);

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

            // Per JSON-RPC 2.0: a request without an `id` is a notification, and
            // the server MUST NOT reply. Still dispatch for side effects.
            let is_notification = request.id.is_none();
            let response = self.handle_request(request).await;
            if !is_notification {
                self.write_response(&mut writer, &response).await?;
            }
        }

        Ok(())
    }

    /// Serialize and write a single response line.
    async fn write_response<W>(
        &self,
        writer: &mut BufWriter<W>,
        response: &Response,
    ) -> Result<(), ServerError>
    where
        W: AsyncWrite + Unpin,
    {
        let payload = serde_json::to_string(response).map_err(ServerError::Serialization)?;
        writer.write_all(payload.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;
        Ok(())
    }

    /// Validate the request and dispatch to the handler.
    async fn handle_request(&self, request: Request) -> Response {
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
