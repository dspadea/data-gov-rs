//! MCP server entry point — struct definition, construction, and run loop.

use data_gov::{DataGovClient, DataGovConfig, OperatingMode};
use serde_json::Value;
use std::env;
use tokio::io::{
    self, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, BufWriter,
};

use crate::types::{Request, RequestIdKind, Response, ServerError, classify_request_id};

/// A hold point a test can place in front of one method.
///
/// The concurrency tests need a request that provably has not finished, and a
/// sleep only makes that probable. Holding the dispatch on a signal the test
/// controls means "the second request was answered first" can be asserted
/// without measuring anything.
#[cfg(test)]
pub(crate) struct TestGate {
    /// The method this gate holds. Others pass straight through.
    pub method: &'static str,
    /// Released by the test. Until then the matching dispatch does not return.
    pub release: std::sync::Arc<tokio::sync::Notify>,
}

/// The data.gov MCP server.
///
/// Reads JSON-RPC requests from stdin and writes responses to stdout.
pub struct DataGovMcpServer {
    pub(crate) data_gov: DataGovClient,
    pub(crate) portal_base_url: String,
    #[cfg(test)]
    pub(crate) test_gate: Option<TestGate>,
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
            #[cfg(test)]
            test_gate: None,
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
        let mut reader = BufReader::new(reader);
        let mut writer = BufWriter::new(writer);
        let mut raw = Vec::new();

        loop {
            raw.clear();
            // Bytes, not lines. `Lines::next_line` fails the whole stream on a
            // single byte that is not UTF-8, so one stray byte would end the
            // session for every well-formed request behind it. Decoding
            // lossily turns that byte into a message this server can answer
            // and move on from. Only EOF and a transport failure end the loop.
            if reader.read_until(b'\n', &mut raw).await? == 0 {
                break;
            }

            let line = String::from_utf8_lossy(&raw);
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Some(response) = self.answer(trimmed).await {
                self.write_response(&mut writer, &response).await?;
            }
        }

        Ok(())
    }

    /// Turn one received line into the response owed to the client.
    ///
    /// Returns `None` for a notification: JSON-RPC 2.0 says the receiver "MUST
    /// NOT send a response" to one, and that holds however malformed it is.
    async fn answer(&self, line: &str) -> Option<Response> {
        // Deserialize to a `Value` before a `Request`. It is what makes an
        // absent `id` distinguishable from an explicit `"id": null`, and what
        // keeps the client's id recoverable when the rest of the object is not
        // a valid Request.
        let message: Value = match serde_json::from_str(line) {
            Ok(message) => message,
            Err(err) => {
                tracing::warn!("unparseable message: {err}");
                // Nothing can be recovered from text that is not JSON, so
                // JSON-RPC 2.0 requires a null id on this response.
                return Some(Response::error(None, ServerError::Json(err)));
            }
        };

        let id = match classify_request_id(&message) {
            RequestIdKind::Absent => {
                self.dispatch_notification(message).await;
                return None;
            }
            RequestIdKind::Invalid => {
                tracing::warn!("request id is not a string or an integer");
                return Some(Response::error(
                    None,
                    ServerError::InvalidRequest(
                        "request id must be a string or an integer; MCP forbids a null id"
                            .to_string(),
                    ),
                ));
            }
            RequestIdKind::Valid(id) => id,
        };

        // The message parsed as JSON, so a failure here is -32600 (not a valid
        // Request object), never -32700, and the id above is still echoable.
        let request: Request = match serde_json::from_value(message) {
            Ok(request) => request,
            Err(err) => {
                tracing::warn!("invalid request: {err}");
                return Some(Response::error(
                    Some(id),
                    ServerError::InvalidRequest(err.to_string()),
                ));
            }
        };

        Some(self.handle_request(Some(id), request).await)
    }

    /// Run a notification for its side effects and swallow its outcome.
    ///
    /// The client is not owed a response, so a failure here can only be
    /// logged.
    async fn dispatch_notification(&self, message: Value) {
        let request: Request = match serde_json::from_value(message) {
            Ok(request) => request,
            Err(err) => {
                tracing::warn!("invalid notification: {err}");
                return;
            }
        };

        let method = request.method.clone();
        if let Err(err) = self.dispatch(&request.method, request.params).await {
            tracing::debug!(method = %method, "notification not handled: {err}");
        }
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

    /// Validate the request envelope and dispatch to the handler.
    async fn handle_request(&self, id: Option<Value>, request: Request) -> Response {
        // JSON-RPC 2.0, section 4: the `jsonrpc` member "MUST be exactly
        // \"2.0\"". An absent member fails that as surely as a wrong one, and
        // rejecting it here is the client's earliest signal that it built the
        // message against a different protocol.
        match request.jsonrpc.as_deref() {
            Some("2.0") => {}
            Some(version) => {
                return Response::error(
                    id,
                    ServerError::InvalidRequest(format!(
                        "invalid jsonrpc version: expected \"2.0\", got \"{version}\""
                    )),
                );
            }
            None => {
                return Response::error(
                    id,
                    ServerError::InvalidRequest(
                        "missing jsonrpc member: JSON-RPC 2.0 requires \"jsonrpc\": \"2.0\""
                            .to_string(),
                    ),
                );
            }
        }

        match self.dispatch(&request.method, request.params).await {
            Ok(result) => Response::success(id, result),
            Err(err) => Response::error(id, err),
        }
    }
}
