//! MCP server entry point — struct definition, construction, and run loop.

use data_gov::config::ConfigResolver;
use data_gov::{DataGovClient, OperatingMode};
use serde_json::Value;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{
    self, AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, BufWriter,
};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore, mpsc, oneshot};

use crate::tools::find_tool_spec_by_method;
use crate::types::{Request, RequestIdKind, Response, ServerError, classify_request_id};

/// The MCP notification that asks the server to stop working on a request.
pub(crate) const CANCELLED_NOTIFICATION: &str = "notifications/cancelled";

/// How long one request may run before the server abandons it.
///
/// Applies to every method except a tool the registry marks
/// [`crate::tools::WallClockBound::Exempt`] - today only
/// `data_gov.downloadResources`. What is left answers from memory or from a
/// single catalog call, each of which carries its own 30-second bound, so a
/// request still running after this has gone wrong rather than gone slowly.
/// Abandoning it is what stops a hung upstream holding a request, and its slot
/// in the cancellation registry, open forever.
///
/// The figure is deliberately far above anything a bounded method should need,
/// because it is not operator tunable and the cost of setting it too low is a
/// request killed while it was working. It is not a download budget: a
/// transfer's honest runtime is the size of a file over the width of a link,
/// which no constant here can predict, so downloads are exempt and are bounded
/// instead by reqwest's `read_timeout` on the download client - which restarts
/// on every frame that arrives - and by `notifications/cancelled`.
pub(crate) const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(900);

/// Responses buffered between the dispatch tasks and the writer.
const RESPONSE_QUEUE_DEPTH: usize = 64;

/// How many requests may be in dispatch at once.
///
/// Reading a line is cheap and serving it is not, so without a bound the read
/// loop queues work faster than the server can finish it. The response channel
/// throttles the *writing*, but only once the work is already done, which is
/// why it does not limit the work: pipelining `tools/list` into a server whose
/// stdout nobody drained reached 27.5 GB of resident memory in 30 seconds and
/// was still climbing.
///
/// The number is deliberately generous. A small bound would reintroduce the
/// head-of-line blocking #65 exists to remove - one slow download would stop
/// the requests behind it - and that is only a risk when the bound is near the
/// number of requests a client really has outstanding. Real MCP clients keep a
/// handful in flight, so 256 is two orders of magnitude clear of normal use and
/// is only ever reached by a client that is flooding.
///
/// What it trades: at saturation the loop stops reading, so a
/// `notifications/cancelled` queued behind the request that saturated it waits
/// too. That is inherent to backpressure on a single ordered stream - the
/// cancellation cannot be read without reading the line in front of it - and
/// [`DEFAULT_REQUEST_TIMEOUT`] is the backstop that frees the slot of every
/// request it bounds. A download is deliberately not one of them: its slot is
/// given up when the transfer finishes, when its own stall bound fires, or on
/// a cancellation, never on elapsed time.
const MAX_CONCURRENT_DISPATCHES: usize = 256;

/// The largest message the server will accept, in bytes, newline included.
///
/// A message past this is refused rather than buffered, because buffering one
/// the server has already decided not to serve is the whole cost: before this
/// cap a 1 GiB line peaked at 1046 MiB of resident memory on its way to a
/// `-32700`, and a 256 MiB line that was a *valid* request peaked at 4.09x its
/// own size, because the raw buffer, the parsed `Value`, the cloned id, and the
/// serialised response all exist at once.
///
/// Sized against the largest legitimate message, which is a `tools/call` for
/// `data_gov.downloadResources`: a dataset slug at the catalog's 90-character
/// truncation cap, an `outputDir` bounded by the platform's path limit, a short
/// `formats` list, and one integer of `distributionIndexes` per distribution.
/// Even 10,000 distributions with every index spelled out is about 60 KB of
/// JSON, so 1 MiB leaves more than an order of magnitude of headroom over a
/// payload far larger than data.gov serves.
const MAX_LINE_BYTES: usize = 1024 * 1024;

/// The requests currently being served, keyed by request id.
type InFlight = Arc<Mutex<InFlightRequests>>;

/// One request's claim on a request id.
struct Registration {
    /// Tells one occupant of a key from the next.
    ///
    /// The key alone does not identify whose entry this is: a cancellation
    /// frees an id, and a retry under that id - a normal thing for a client to
    /// do - claims the same key. Without the token, a task that finishes just
    /// after both of those removes the retry's entry instead of its own.
    token: u64,
    /// Resolved to ask the dispatch task to stop.
    cancel: oneshot::Sender<()>,
}

/// What one read of the transport produced.
enum ReadLine {
    /// A whole message, in the caller's buffer.
    Line,
    /// A message past [`MAX_LINE_BYTES`]. Nothing was kept, and the rest of the
    /// line was consumed so it cannot be read as the next message.
    TooLong,
    /// The transport reached end of input.
    Eof,
}

/// The cancellation registry for one session.
#[derive(Default)]
struct InFlightRequests {
    entries: HashMap<String, Registration>,
    /// Handed out in order and never reused within a session, so a token
    /// identifies one occupancy of a key rather than the key itself.
    next_token: u64,
}

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
    /// A second hold point for the same method, in the completion epilogue.
    /// `None` leaves the epilogue unheld, which is what every other test wants.
    pub epilogue: Option<EpilogueGate>,
}

/// A hold point between one request's handler resolving and its task giving up
/// its slot in the cancellation registry.
///
/// Nothing in that window awaits anything a test can reach, so from the wire
/// the two events read as one step. A cancellation and a retry can only be
/// placed inside the window if something holds it open, which is what this is.
#[cfg(test)]
pub(crate) struct EpilogueGate {
    /// Signalled by the server when a task arrives at the hold point.
    pub arrived: std::sync::Arc<tokio::sync::Notify>,
    /// Awaited there. One waiter is released per `notify_one`.
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

    /// Build a new server from the process environment and `config.toml`.
    ///
    /// # Errors
    ///
    /// [`ServerError::DataGov`] if `config.toml` exists but cannot be read or
    /// parsed, or if any layer supplied a value that cannot work. Both fail
    /// startup rather than producing a client that misbehaves later.
    fn new() -> Result<Self, ServerError> {
        Self::from_resolver(ConfigResolver::from_process()?.with_mode(OperatingMode::CommandLine))
    }

    /// Build a server from an already-assembled configuration resolver.
    ///
    /// The server has no command-line flags, so the chain it sees is
    /// environment, then `config.toml`, then the built-in default - the same
    /// chain the CLI resolves, minus the layer that does not exist here
    /// (#116). The host's environment therefore still wins over anything the
    /// user persisted in a file, which is what keeps an operator who
    /// configures the host in charge of it.
    ///
    /// Taking the resolver rather than reading the process is what makes
    /// configuration testable: a test supplies explicit environment pairs and
    /// an explicit parsed file, and touches neither the real environment nor
    /// the filesystem.
    ///
    /// # Errors
    ///
    /// [`ServerError::DataGov`] if resolution fails - a non-numeric count, a
    /// zero concurrency limit or timeout, an empty user agent or download
    /// directory, or a base URL that is not `http` or `https`. The message
    /// names the setting and the layer the value came from.
    pub(crate) fn from_resolver(resolver: ConfigResolver) -> Result<Self, ServerError> {
        let resolved = resolver.resolve()?;

        // stdout is the JSON-RPC channel and nothing but framed responses may
        // appear on it, so a warning goes to stderr. The host shows stderr in
        // its logs; a line on stdout would corrupt the protocol stream.
        for warning in resolved.warnings() {
            eprintln!("data-gov-mcp-server: warning: {warning}");
        }

        let config = resolved.into_config();
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

    /// The resolved download directory. Test-only accessor for #116.
    #[cfg(test)]
    pub(crate) fn download_dir_for_test(&self) -> std::path::PathBuf {
        self.data_gov.config().get_base_download_dir()
    }

    /// The resolved catalog base URL. Test-only accessor for #116.
    #[cfg(test)]
    pub(crate) fn portal_base_url_for_test(&self) -> &str {
        &self.portal_base_url
    }

    /// The resolved user agent. Test-only accessor for #116.
    #[cfg(test)]
    pub(crate) fn user_agent_for_test(&self) -> Option<String> {
        self.data_gov.config().catalog_config.user_agent.clone()
    }

    /// The resolved download concurrency limit. Test-only accessor for #116.
    #[cfg(test)]
    pub(crate) fn max_concurrent_downloads_for_test(&self) -> usize {
        self.data_gov.config().max_concurrent_downloads
    }

    /// The resolved per-download timeout. Test-only accessor for #116.
    #[cfg(test)]
    pub(crate) fn download_timeout_secs_for_test(&self) -> u64 {
        self.data_gov.config().download_timeout_secs
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
        let slots = Arc::new(Semaphore::new(MAX_CONCURRENT_DISPATCHES));
        let mut reader = BufReader::new(reader);
        let mut raw = Vec::new();

        loop {
            raw.clear();
            // Bytes, not lines. `Lines::next_line` fails the whole stream on a
            // single byte that is not UTF-8, so one stray byte ended the
            // session for every well-formed request behind it. Reading raw
            // keeps the failure local to the line it arrived on. Only EOF and
            // a transport failure end the loop.
            match read_capped_line(&mut reader, &mut raw).await? {
                ReadLine::Eof => break,
                ReadLine::Line => {}
                ReadLine::TooLong => {
                    tracing::warn!("a line exceeded {MAX_LINE_BYTES} bytes and was discarded");
                    // -32600 rather than -32700: the server did not fail to
                    // parse this message, it declined to receive it, and what
                    // the client has to do about it is send a smaller one. The
                    // id is null because nothing was kept to recover one from.
                    let error = ServerError::InvalidRequest(format!(
                        "a message may not exceed {MAX_LINE_BYTES} bytes; the line was \
                         discarded unread"
                    ));
                    send_response(&responses, Response::error(None, error)).await;
                    continue;
                }
            }

            // Strictly, not lossily. Replacing an undecodable byte with U+FFFD
            // repairs it into a *different* message: a bad byte inside a JSON
            // string yields a slug the client never sent, and the tool then
            // runs on it. A line that cannot be decoded cannot be parsed, so
            // it is a parse error - and the session continues, which is the
            // part that matters.
            let line = match std::str::from_utf8(&raw) {
                Ok(line) => line,
                Err(err) => {
                    tracing::warn!("undecodable line: {err}");
                    let error = ServerError::Parse(format!("line is not valid UTF-8: {err}"));
                    send_response(&responses, Response::error(None, error)).await;
                    continue;
                }
            };

            let trimmed = line.trim();
            if !trimmed.is_empty() {
                self.accept(trimmed, &responses, &in_flight, &slots).await;
            }

            // A closed output means the client is gone. Reading on would keep
            // accepting work - downloads that write to disk among it - that
            // nobody can be told the outcome of.
            if responses.is_closed() {
                tracing::warn!("the response transport is closed; ending the session");
                break;
            }
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
    /// line and the next. The one thing that deliberately does wait here is a
    /// free dispatch slot - see [`MAX_CONCURRENT_DISPATCHES`].
    async fn accept(
        self: &Arc<Self>,
        line: &str,
        responses: &mpsc::Sender<Response>,
        in_flight: &InFlight,
        slots: &Arc<Semaphore>,
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
                self.accept_notification(message, responses, in_flight, slots)
                    .await;
                return;
            }
            // An array, a number, a string, a boolean or null. None of these is
            // a Request object, so none can be a Notification, and JSON-RPC
            // answers each with -32600 and a null id. Treating them as
            // notifications leaves a client that still sends batches - which
            // MCP 2025-06-18 removed - waiting forever on a stderr line it
            // cannot see.
            RequestIdKind::NotAnObject => {
                tracing::warn!("message is not a JSON object");
                let error = ServerError::InvalidRequest(
                    "a JSON-RPC message must be an object; batching is not supported".to_string(),
                );
                send_response(responses, Response::error(None, error)).await;
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

        // Backpressure, and the reason it sits on the reading task rather than
        // inside the spawned one: waiting here is what stops the next line
        // being read. Taken before the id is claimed, so the registry only
        // ever holds work that has somewhere to run.
        let slot = acquire_slot(slots).await;

        // Registered before the task is spawned, so a cancellation arriving on
        // the very next line always finds it. The task gives the entry up when
        // it ends, whether it finished or was cancelled.
        //
        // A second request under an id already in flight is refused rather
        // than allowed to take the slot. MCP forbids reusing a request id
        // within a session, and letting the newcomer overwrite the entry loses
        // both requests: dropping the displaced sender cancels the first, and
        // the first then deregisters the key the second is now living under,
        // cancelling that one too. Neither is ever answered.
        let (cancel, cancelled) = oneshot::channel();
        let key = cancellation_key(&id);
        let Some(token) = register(in_flight, key.clone(), cancel).await else {
            tracing::warn!(request_id = %id, "request id is already in flight");
            let error = ServerError::InvalidRequest(format!(
                "request id {id} is already in flight; a request id must not be reused \
                 within a session"
            ));
            send_response(responses, Response::error(Some(id), error)).await;
            return;
        };

        let server = Arc::clone(self);
        let responses = responses.clone();
        let registry = Arc::clone(in_flight);
        #[cfg(test)]
        let gated_method = request.method.clone();
        tokio::spawn(async move {
            // Held until this task ends, response written and all, so the slot
            // covers the whole cost of the request rather than only its
            // dispatch.
            let _slot = slot;
            tokio::select! {
                biased;
                // Dropping the handler future is what "stop processing the
                // cancelled request" means here, and no response is sent -
                // which is what the spec asks for.
                //
                // Deliberately no deregistration on this arm. `cancel_request`
                // is the only thing that can resolve this channel, and it takes
                // the entry out before it signals. Removing again would be a
                // no-op at best; at worst the id has already been claimed by a
                // later request - a client that cancels and retries under the
                // same id does exactly that - and this would take that request
                // out of the registry, drop its sender, and cancel it too.
                _ = cancelled => {}
                response = server.handle_request(Some(id), request) => {
                    #[cfg(test)]
                    server.hold_epilogue(&gated_method).await;
                    // Compare-and-remove, never remove-by-key. This task can
                    // reach here after a cancellation has already freed its id
                    // and a retry has claimed the same key: `select!` keeps the
                    // branch it did not take alive while this body runs, so the
                    // cancellation is delivered to a receiver that has already
                    // lost, and the entry it removed was this task's. Removing
                    // by key here would take the retry's entry, drop its
                    // sender, and cancel a request nobody will ever answer.
                    deregister(&registry, &key, token).await;
                    send_response(&responses, response).await;
                }
            }
        });
    }

    /// Stop a completing task in its epilogue, where a test asked for it.
    ///
    /// `method` is the envelope method, so a test that wants this holds a
    /// request that names the tool directly rather than wrapping it in
    /// `tools/call`.
    #[cfg(test)]
    async fn hold_epilogue(&self, method: &str) {
        if let Some(gate) = self.test_gate.as_ref()
            && gate.method == method
            && let Some(epilogue) = gate.epilogue.as_ref()
        {
            epilogue.arrived.notify_one();
            epilogue.release.notified().await;
        }
    }

    /// Act on a JSON object that carries no `id`.
    ///
    /// It is a Notification only if it is otherwise a valid Request object.
    /// When it is not - no `method`, or a `method` that is not a string - it is
    /// simply an invalid Request, and JSON-RPC answers that with -32600 and a
    /// null id rather than ignoring it.
    async fn accept_notification(
        self: &Arc<Self>,
        message: Value,
        responses: &mpsc::Sender<Response>,
        in_flight: &InFlight,
        slots: &Arc<Semaphore>,
    ) {
        let request: Request = match serde_json::from_value(message) {
            Ok(request) => request,
            Err(err) => {
                tracing::warn!("not a valid Request object: {err}");
                let error = ServerError::InvalidRequest(err.to_string());
                send_response(responses, Response::error(None, error)).await;
                return;
            }
        };

        // The envelope rule is not weaker for a notification. It is owed no
        // response either way, but a message the server has just declared
        // invalid must not have side effects - and cancelling somebody's
        // in-flight request is a side effect.
        if request.jsonrpc.as_deref() != Some("2.0") {
            tracing::warn!(
                method = %request.method,
                "notification ignored: the jsonrpc member must be \"2.0\""
            );
            return;
        }

        let Request { method, params, .. } = request;

        // Handled on the reading task rather than spawned: a cancellation is
        // worth exactly as much as it is prompt, and it is the one message that
        // must never queue behind anything.
        if method == CANCELLED_NOTIFICATION {
            cancel_request(params, in_flight).await;
            return;
        }

        if !notification_may_dispatch(&method) {
            tracing::warn!(
                method = %method,
                "refusing to run a tool from a message that owes no answer"
            );
            return;
        }

        // A notification is dispatched work like any other, so it takes a slot
        // too. It is taken after the cancellation branch above, which is what
        // keeps a cancellation off the bound entirely.
        let slot = acquire_slot(slots).await;

        let server = Arc::clone(self);
        tokio::spawn(async move {
            let _slot = slot;
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

/// Wait for a free dispatch slot.
///
/// # Panics
///
/// Never in practice. `acquire_owned` fails only on a closed semaphore, and
/// this one is created by [`DataGovMcpServer::serve`], cloned into the tasks it
/// spawns, and closed by nothing.
async fn acquire_slot(slots: &Arc<Semaphore>) -> OwnedSemaphorePermit {
    match Arc::clone(slots).acquire_owned().await {
        Ok(slot) => slot,
        Err(_closed) => unreachable!("the dispatch limiter is never closed"),
    }
}

/// Read one newline-framed message, refusing anything past [`MAX_LINE_BYTES`].
///
/// An over-long line is discarded as it arrives rather than buffered and then
/// rejected, so the memory it costs is bounded by the read buffer and not by
/// what the client chose to send. The rest of the line is consumed either way,
/// so its tail is never read as the next message.
///
/// The cap counts the newline, so a message of exactly [`MAX_LINE_BYTES`]
/// bytes including its terminator is accepted and one byte more is not.
async fn read_capped_line<R>(reader: &mut R, raw: &mut Vec<u8>) -> io::Result<ReadLine>
where
    R: AsyncBufRead + Unpin,
{
    let mut discarding = false;
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            // End of input. A final line with no newline still counts as one.
            return Ok(if discarding {
                ReadLine::TooLong
            } else if raw.is_empty() {
                ReadLine::Eof
            } else {
                ReadLine::Line
            });
        }

        let newline = available.iter().position(|byte| *byte == b'\n');
        let taken = newline.map_or(available.len(), |at| at + 1);
        if !discarding {
            if raw.len() + taken > MAX_LINE_BYTES {
                // Give the bytes back at the moment the line is refused. What
                // has been read is already paid for; what follows must not be.
                raw.clear();
                raw.shrink_to_fit();
                discarding = true;
            } else {
                raw.extend_from_slice(&available[..taken]);
            }
        }
        reader.consume(taken);

        if newline.is_some() {
            return Ok(if discarding {
                ReadLine::TooLong
            } else {
                ReadLine::Line
            });
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

/// Whether a notification naming `method` may be dispatched.
///
/// MCP defines no notification that invokes a tool, and this server must not
/// invent one. A tool run from a notification is uncontrollable in three ways
/// at once: it has no id, so `notifications/cancelled` cannot reach it; it owes
/// no response, so nothing reports the files it wrote or the errors it hit; and
/// it holds no sender, so the read loop reaching end of input tears the runtime
/// down while it is still writing. That is the "silence reads as success"
/// failure, on a tool whose job is writing to the filesystem.
pub(crate) fn notification_may_dispatch(method: &str) -> bool {
    method != "tools/call" && find_tool_spec_by_method(method).is_none()
}

/// The registry key for a request id.
///
/// Serialized rather than stringified, so the number `1` and the string `"1"`
/// stay distinct - JSON-RPC compares ids by value and by type.
fn cancellation_key(id: &Value) -> String {
    id.to_string()
}

/// Claim `key` for a request, returning the token that identifies the claim.
///
/// Returns `None` when the key is already occupied, which is a request id
/// being reused while its first use is still in flight.
async fn register(in_flight: &InFlight, key: String, cancel: oneshot::Sender<()>) -> Option<u64> {
    let mut registry = in_flight.lock().await;
    let token = registry.next_token;
    match registry.entries.entry(key) {
        Entry::Occupied(_) => return None,
        Entry::Vacant(slot) => {
            slot.insert(Registration { token, cancel });
        }
    }
    // A u64 counter cannot run out inside a session: one id per nanosecond
    // would take five centuries.
    registry.next_token = token + 1;
    Some(token)
}

/// Give `key` up, but only while `token` still holds it.
///
/// The comparison is the whole point. A key can be freed by a cancellation and
/// claimed again by a retry between a task resolving and that task reaching
/// here, so "the key is present" does not mean "the key is mine".
async fn deregister(in_flight: &InFlight, key: &str, token: u64) {
    let mut registry = in_flight.lock().await;
    if registry
        .entries
        .get(key)
        .is_some_and(|held| held.token == token)
    {
        registry.entries.remove(key);
    }
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

    let held = in_flight
        .lock()
        .await
        .entries
        .remove(&cancellation_key(requested));
    match held {
        Some(Registration { cancel, .. }) => {
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
