//! MCP server entry point — struct definition, construction, and run loop.

use data_gov::{DataGovClient, DataGovConfig, OperatingMode};
use serde_json::Value;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{
    self, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, BufWriter,
};
use tokio::sync::{Mutex, mpsc, oneshot};

use crate::types::{Request, RequestIdKind, Response, ServerError, classify_request_id};

/// The MCP notification that asks the server to stop working on a request.
pub(crate) const CANCELLED_NOTIFICATION: &str = "notifications/cancelled";

/// How long one request may run before the server abandons it.
///
/// Downloads set the floor: `data_gov.downloadResources` allows 300 seconds
/// per file with three in flight, so a large dataset legitimately takes
/// minutes. This is the outer bound that stops a hung upstream holding a
/// request, and its slot in the cancellation registry, open forever.
pub(crate) const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(900);

/// Responses buffered between the dispatch tasks and the writer.
const RESPONSE_QUEUE_DEPTH: usize = 64;

/// Cancellation handles for the requests currently being served, keyed by
/// request id.
type InFlight = Arc<Mutex<HashMap<String, oneshot::Sender<()>>>>;

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
    /// How long one request may run before the server abandons it.
    pub(crate) request_timeout: Duration,
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
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
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

        Arc::new(self).serve(io::stdin(), io::stdout()).await
    }

    /// Serve the JSON-RPC line protocol over an arbitrary reader and writer.
    ///
    /// Each request is dispatched on its own task, so a download that holds a
    /// request open for minutes does not stop the next line being read. Every
    /// response travels through one channel to one writer task, which is what
    /// keeps concurrent answers from interleaving on the way out.
    ///
    /// [`run`](Self::run) supplies stdin and stdout; tests supply in-memory
    /// pipes, so every protocol edge is reachable without a real process.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::Io`] when the transport itself fails. A malformed
    /// message is answered on the wire, not returned here.
    pub(crate) async fn serve<R, W>(
        self: Arc<Self>,
        reader: R,
        writer: W,
    ) -> Result<(), ServerError>
    where
        R: AsyncRead + Unpin,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (responses, queue) = mpsc::channel::<Response>(RESPONSE_QUEUE_DEPTH);
        let writer_task = tokio::spawn(write_responses(writer, queue));

        let in_flight: InFlight = Arc::default();
        let mut reader = BufReader::new(reader);
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

            self.accept(trimmed, &responses, &in_flight).await;
        }

        // Every dispatch task holds a clone of the sender, so the writer keeps
        // draining until the last of them finishes. Work already accepted is
        // still answered after the client closes its end.
        drop(responses);

        match writer_task.await {
            Ok(result) => result,
            Err(err) => Err(ServerError::Io(std::io::Error::other(format!(
                "the response writer task failed: {err}"
            )))),
        }
    }

    /// Take one received line and either answer it, dispatch it, or act on it.
    ///
    /// Requests are spawned; only the cheap envelope decisions and cancellation
    /// happen on the reading task, so nothing that can take time is between one
    /// line and the next.
    async fn accept(
        self: &Arc<Self>,
        line: &str,
        responses: &mpsc::Sender<Response>,
        in_flight: &InFlight,
    ) {
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
                send_response(responses, Response::error(None, ServerError::Json(err))).await;
                return;
            }
        };

        let id = match classify_request_id(&message) {
            RequestIdKind::Absent => {
                self.accept_notification(message, in_flight).await;
                return;
            }
            RequestIdKind::Invalid => {
                tracing::warn!("request id is not a string or an integer");
                let error = ServerError::InvalidRequest(
                    "request id must be a string or an integer; MCP forbids a null id".to_string(),
                );
                send_response(responses, Response::error(None, error)).await;
                return;
            }
            RequestIdKind::Valid(id) => id,
        };

        // The message parsed as JSON, so a failure here is -32600 (not a valid
        // Request object), never -32700, and the id above is still echoable.
        let request: Request = match serde_json::from_value(message) {
            Ok(request) => request,
            Err(err) => {
                tracing::warn!("invalid request: {err}");
                let error = ServerError::InvalidRequest(err.to_string());
                send_response(responses, Response::error(Some(id), error)).await;
                return;
            }
        };

        // Registered before the task is spawned, so a cancellation arriving on
        // the very next line always finds it. The task removes its own entry
        // when it ends, whether it finished or was cancelled.
        //
        // A second request under an id already in flight is refused rather
        // than allowed to take the slot. MCP forbids reusing a request id
        // within a session, and letting the newcomer overwrite the entry loses
        // both requests: dropping the displaced sender cancels the first, and
        // the first then deregisters the key the second is now living under,
        // cancelling that one too. Neither is ever answered.
        let (cancel, cancelled) = oneshot::channel();
        let key = cancellation_key(&id);
        let already_in_flight = {
            let mut registry = in_flight.lock().await;
            match registry.entry(key.clone()) {
                Entry::Occupied(_) => true,
                Entry::Vacant(slot) => {
                    slot.insert(cancel);
                    false
                }
            }
        };

        if already_in_flight {
            tracing::warn!(request_id = %id, "request id is already in flight");
            let error = ServerError::InvalidRequest(format!(
                "request id {id} is already in flight; a request id must not be reused \
                 within a session"
            ));
            send_response(responses, Response::error(Some(id), error)).await;
            return;
        }

        let server = Arc::clone(self);
        let responses = responses.clone();
        let registry = Arc::clone(in_flight);
        tokio::spawn(async move {
            tokio::select! {
                biased;
                // Dropping the handler future is what "stop processing the
                // cancelled request" means here, and no response is sent -
                // which is what the spec asks for.
                _ = cancelled => {}
                response = server.handle_request(Some(id), request) => {
                    send_response(&responses, response).await;
                }
            }
            registry.lock().await.remove(&key);
        });
    }

    /// Act on a notification. The client is never sent a response to one.
    async fn accept_notification(self: &Arc<Self>, message: Value, in_flight: &InFlight) {
        let request: Request = match serde_json::from_value(message) {
            Ok(request) => request,
            Err(err) => {
                tracing::warn!("invalid notification: {err}");
                return;
            }
        };

        let Request { method, params, .. } = request;

        // Handled on the reading task rather than spawned: a cancellation is
        // worth exactly as much as it is prompt, and it is the one message that
        // must never queue behind anything.
        if method == CANCELLED_NOTIFICATION {
            cancel_request(params, in_flight).await;
            return;
        }

        let server = Arc::clone(self);
        tokio::spawn(async move {
            if let Err(err) = server.dispatch(&method, params).await {
                tracing::debug!(method = %method, "notification not handled: {err}");
            }
        });
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

/// Drain the response queue onto the transport, one whole line at a time.
///
/// Dispatch runs concurrently; this does not. One task owning the writer is
/// what stops two answers interleaving mid-line on the way out.
async fn write_responses<W>(
    writer: W,
    mut queue: mpsc::Receiver<Response>,
) -> Result<(), ServerError>
where
    W: AsyncWrite + Unpin,
{
    let mut writer = BufWriter::new(writer);
    while let Some(response) = queue.recv().await {
        let payload = serde_json::to_string(&response).map_err(ServerError::Serialization)?;
        writer.write_all(payload.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;
    }
    Ok(())
}

/// Hand one response to the writer.
///
/// The queue only closes when the writer has gone, which means the transport
/// is already finished; there is nowhere left to report that, so it is logged.
async fn send_response(responses: &mpsc::Sender<Response>, response: Response) {
    if responses.send(response).await.is_err() {
        tracing::warn!("a response was dropped: the transport is closed");
    }
}

/// The registry key for a request id.
///
/// Serialized rather than stringified, so the number `1` and the string `"1"`
/// stay distinct - JSON-RPC compares ids by value and by type.
fn cancellation_key(id: &Value) -> String {
    id.to_string()
}

/// Honour `notifications/cancelled`.
///
/// MCP asks receivers to "stop processing the cancelled request", "free
/// associated resources", and "not send a response for the cancelled request".
/// Dropping the handler future does all three.
///
/// An unknown id, an already-finished request, and a malformed notification are
/// all ignored, which the spec explicitly permits: the notification racing the
/// response it wants to prevent is normal, and both parties have to tolerate
/// it. The rule that `initialize` must not be cancelled binds the client, so it
/// is not enforced here.
async fn cancel_request(params: Option<Value>, in_flight: &InFlight) {
    let Some(requested) = params.as_ref().and_then(|params| params.get("requestId")) else {
        tracing::warn!("{CANCELLED_NOTIFICATION} carried no requestId; ignoring");
        return;
    };

    let reason = params
        .as_ref()
        .and_then(|params| params.get("reason"))
        .and_then(Value::as_str)
        .unwrap_or("no reason given");

    match in_flight.lock().await.remove(&cancellation_key(requested)) {
        Some(cancel) => {
            tracing::info!(request_id = %requested, "cancelling request: {reason}");
            if cancel.send(()).is_err() {
                tracing::debug!(
                    request_id = %requested,
                    "the request finished before its cancellation arrived"
                );
            }
        }
        None => tracing::debug!(
            request_id = %requested,
            "cancellation names a request that is unknown or already finished; ignoring"
        ),
    }
}
