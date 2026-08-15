# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
(0.x releases may include breaking changes on minor bumps).


## [0.5.0] - Unreleased

### Breaking - data-gov-catalog

- **`SearchParams::slug` is removed** (#71). The field, the `.slug()` builder,
  and its query arm all promised exact-slug filtering that the Catalog API does
  not implement: `GET /search?slug=electric-vehicle-population-data` returns
  HTTP 200 with a full unfiltered page, so a caller got arbitrary results and no
  indication anything was wrong. The API's own OpenAPI document confirms
  `/search` has no `slug` parameter. Call `CatalogClient::dataset_by_slug`
  instead, which uses the real exact-lookup endpoint.
- **`SearchParams::spatial_filter` takes a `SpatialFilter` enum** (#77), not a
  string. The API silently ignores an invalid filter value rather than rejecting
  it - `spatial_filter=BOGUS` returns a full unfiltered page - so a bare string
  could not be validated anywhere. Replace `.spatial_filter("geospatial")` with
  `.spatial_filter(SpatialFilter::Geospatial)`. The two variants match the wire
  values in the published OpenAPI document exactly.
- **`CatalogError` has new `InvalidPathSegment` and `InvalidPerPage` variants**
  (#71, #111). Code matching the enum exhaustively must add arms.
- **`harvest_record_transformed` returns `Result<Option<Dataset>>`** (#83). A 404
  means the harvest record has no populated transform, which is the common
  answer rather than a failure: across 752 records sampled from 18
  organizations, only census and noaa populated one at all - roughly 87% of that
  sample had none. It is now `Ok(None)`, through the same helper
  `dataset_by_slug` uses. Any other non-2xx is still an error. `data-gov`'s
  `get_dataset_by_harvest_record` keeps its signature and reports a missing
  transform as `ResourceNotFound`.
- **`SearchParams::sort` takes a `SortOrder` enum** (#77), not a string. The API
  silently ignores a value it does not recognise, so a typo returned plausible
  wrong results with no error: live-probed, `sort=BOGUS_NOT_A_SORT` echoes
  `"relevance"` and returns the unfiltered baseline with HTTP 200. Replace
  `.sort("popularity")` with `.sort(SortOrder::Popularity)`. The four variants
  and their wire values come from the API's own OpenAPI document.

### Breaking - data-gov-ckan

- **Entity ids are `Option<String>`, not `Option<Uuid>`** (#63). `make_uuid` is
  only CKAN's *default* id generator: harvesters, migrations, an explicit
  `package_create`, and CKAN-compatible backends such as DKAN all store
  slug-style ids. A single record with `"id":"city-budget-2024"` failed the
  entire containing response with `UUID parsing failed`, taking every valid
  sibling down with it - which contradicts the whole reason this crate was kept
  after data.gov retired its CKAN endpoint. This is not hypothetical: data.gov.ie
  serves `"id":"central-statistics-office"` today, and the captured fixture
  proving it is in the test suite. Affects `Package`, `Resource` (`id` and
  `package_id`), `Group`, `Tag` (`id` and `vocabulary_id`), `User`, and
  `Vocabulary`. Parse to `Uuid` yourself if you need one.
- **`Resource.size` is `Option<i64>` and tolerates a string** (#62). CKAN's
  column is a `BigInteger`, and serde rejected anything above `i32::MAX` rather
  than saturating - so one multi-gigabyte file discarded a whole search page.
  Captured proof: a 2,290,761,766-byte resource on open.canada.ca. Some portals
  send `size` as text, so a numeric string and an integral float are both
  accepted; a non-numeric string such as `"523 KiB"` (real, from
  data.qld.gov.au) degrades to `None` rather than failing the record.
- **`Configuration` has no `timeout` or `connect_timeout` field** (#48). Setting
  one was a silent no-op: `client` is built once, so
  `Configuration { timeout, ..default() }` kept the old client and the new value
  sat unused. Use `Configuration::with_timeouts(connect, request)`, which builds
  the client from the values given. Defaults are 10s connect, 30s request.
- **`Configuration::default()` no longer names a live portal** (#72). It pointed
  at data.gov's retired CKAN endpoint, a confirmed 404, while the crate's own
  docs said data.gov no longer exposes CKAN. It is now
  `https://ckan.example.invalid/api/3` - reserved by RFC 2606, so it never
  resolves and fails immediately rather than 404-ing in a way that reads like a
  broken request. This crate serves any CKAN portal, so there is no correct live
  default to pick; set `base_path` to your own.

### Breaking - data-gov

- **`DataGovConfig::base_download_dir` is an `Option<PathBuf>`** (#86, #53). It
  was an eager `PathBuf`, so "the user asked for the working directory" and
  "nobody said anything" were the same state - which is exactly why
  `--download-dir` was accepted and then discarded. Wrap a struct-literal value
  in `Some(..)`; `with_download_dir` is unchanged.
- **`DataGovConfig::user_agent` is a method, not a field** (#106). The agent
  lived on both `DataGovConfig` and its nested `catalog_config`, kept in step
  only by `with_user_agent` - so a config built any other way sent metadata
  requests and downloads out under two different identities, silently. It is now
  derived from one field. Read it with `config.user_agent()`; setting it was
  already only supported through the builder.
- **`with_config` rejects a zero timeout or concurrency** (#107, #73). Both
  produced a client that could not work: `download_timeout_secs: 0` became a
  zero connect and read timeout so every download failed on the first byte, and
  `max_concurrent_downloads: 0` built a zero-permit semaphore whose `acquire()`
  never resolved - a hang with no error and no log. Both now return a
  `ConfigError` naming the field. Failing beats clamping here, because silence
  was the damage in every one of these.
- **Three unproven public items are removed** (#77):
  `DataGovClient::list_organization_records`,
  `DataGovClient::autocomplete_organizations`, and
  `DataGovConfig::get_dataset_download_dir`. Each had zero callers, zero tests,
  and no documented intent anywhere in the workspace, confirmed across four
  search axes. `get_dataset_download_dir` was independently found to be a fourth
  unchecked path join, so removing it closes that too. The four remaining items
  in that set were kept and now have tests.
- **`DataGovConfig` has a new `allow_private_network_downloads` field** (#51).
  A struct literal must add it, or switch to `..Default::default()`. It defaults
  to `false`; set it with `with_private_network_downloads(true)` if you point the
  client at a mirror on your own network. It deliberately does **not** open
  link-local: no mirror is served from the range that carries cloud instance
  metadata.
- **`get_distribution_filename` returns one sanitized path component** (#45). A
  title of `../../etc/passwd` now yields `____etc_passwd`. If you relied on the
  raw title reaching the filename, names will differ.
- **`sanitize_path_component(".")` returns `""`, not `"."`** (#45). A lone dot
  names the current directory rather than a file, so it can no longer be
  returned. Callers already handle an empty result via the `data.<ext>` fallback.

### Breaking - MCP server

- **`jsonrpc` is now required, and ids must be a string or an integer** (#93,
  #55). A request with no `jsonrpc` member was accepted and served: the version
  check existed and worked, but only fired when the member was present. An
  explicit `"id": null` was treated as a notification and never answered, so the
  client waited forever - MCP forbids a null request id outright. All of these
  now return `-32600`, echoing the client's id where one can be recovered.
- **Tool arguments reject undeclared properties** (#70). Every tool schema
  advertised `additionalProperties: false` and no parameter struct enforced it,
  so a misspelled key was dropped and the tool returned success computed from
  different arguments - `{"datasetId":"x","output_dir":"/data"}` wrote to the
  default directory and reported success. Unknown keys now yield `-32602`.
- **A request id already in flight is refused**, so a client cannot have two
  outstanding requests under one id.

- **`structuredContent` is now always a JSON object** (#60). Two tools —
  `data_gov.listOrganizations` and `data_gov.autocompleteDatasets` — returned a
  bare JSON array, which the spec does not permit: structured content "is
  returned as a JSON object". They now return `{"organizations": [...]}` and
  `{"datasets": [...]}` respectively. If you were reading the array directly,
  read the named key instead.
- **Tool results no longer contain a `{"type":"json"}` content block** (#60).
  MCP's `content` is a closed union of `text`, `image`, `audio`,
  `resource_link` and `resource`; `json` is not a member, so every tool result
  this server produced failed schema validation on a strict client. The
  machine-readable payload moves to the sibling `structuredContent` field, and
  the pretty-printed text block stays, as the spec recommends for clients that
  do not read structured output. If you were reading
  `content[1].json`, read `structuredContent` instead.
- **MCP `initialize` now returns the required `protocolVersion`** and negotiates
  it (#44). The server advertises `2024-11-05`, `2025-03-26`, `2025-06-18`, and
  `2025-11-25`: a version it supports is echoed verbatim, anything else (or an
  omitted field) gets `2025-11-25`. Previously the field was absent entirely, so
  any client validating the result against the MCP schema aborted the handshake
  and no tool was ever reachable.
- **`capabilities.tools` now uses `listChanged`, not `list`** (#44). `list` is
  not a key in the MCP schema. If you were reading `capabilities.tools.list`,
  read `capabilities.tools.listChanged` instead — it is `false`, since the tool
  list is static.
- **The unsolicited `ready` line on stdout is gone** (#29). The server previously
  wrote `{"jsonrpc":"2.0","id":null,"result":{...}}` before any request, which
  matches no MCP message shape and is not valid JSON-RPC either — a `result`
  response with a null id corresponds to no request. stdout is now silent until
  the server answers a request. If you were parsing that line for the method
  list, call `tools/list` after `initialize` instead. The same information still
  goes to stderr via `tracing` at startup.

### Changed
- **The MCP server now resolves configuration through the same chain as the
  CLI** (#116). It previously read `DATA_GOV_BASE_URL` and
  `DATA_GOV_USER_AGENT` by hand and built its configuration programmatically,
  so `config.toml` (added in this release) had no effect on the agent-facing
  front door, and `DATA_GOV_DOWNLOAD_DIR`,
  `DATA_GOV_MAX_CONCURRENT_DOWNLOADS` and `DATA_GOV_DOWNLOAD_TIMEOUT_SECS`
  were ignored outright. All five settings now resolve as
  `environment variable > config file > built-in default` - the CLI's chain
  without the flag layer the server does not have.

  **What changes for you:** if you set any of those three environment
  variables when launching the server, they now take effect where before they
  did nothing. If you keep a `config.toml`, the server now honours it. The
  environment still beats the file, so a host that sets variables is
  unaffected. `DATA_GOV_BASE_URL` and `DATA_GOV_USER_AGENT` behave exactly as
  before.

  A `config.toml` that cannot be parsed, or a value that cannot work, now
  stops the server at startup naming the setting, rather than being ignored.
  Warnings go to stderr, never stdout, which carries the JSON-RPC stream.
- **MCP responses may arrive out of order; correlate by `id`** (#65). The run
  loop awaited each handler before reading the next line, so a
  `downloadResources` call holding that await for minutes queued every later
  request behind it - including `shutdown`, which meant the process had to be
  killed. Requests now dispatch concurrently behind a single writer, so whole
  lines are still never interleaved.
- **Content-dependent MCP failures return a result, not a JSON-RPC error** (#70).
  "No matching downloadable distributions" and "dataset has no DCAT metadata"
  were `-32602 invalid parameters`, which neither describes. They now return
  `isError: true` with the reason in `content`, which is what lets a model see
  the failure and correct itself; `isError` is always present. Genuine protocol
  faults stay JSON-RPC errors.
- **The MCP read loop bounds itself** (#109). It dispatches at most 256 requests
  at once and caps an accepted line at 1 MiB. Peak memory with an undrained
  stdout falls from tens of gigabytes to a flat 25 MB - and a drained pipeline
  runs 3.2x faster, because the unbounded loop had been thrashing.
- **Downloads refuse a destination on your own network** (#51). A distribution's
  `downloadURL` comes from harvested third-party metadata, so it is untrusted
  input. Loopback, RFC1918, carrier-grade NAT, IPv6 unique-local, and link-local
  destinations are refused, as is any scheme but `http` and `https`. Checked
  before the request, on every redirect hop, and again at name resolution. Point
  the client at a mirror on your own network with
  `with_private_network_downloads(true)`; that opt-in does not open link-local.
- **`download_timeout_secs` is a stall timeout, not a deadline on the transfer**
  (#50). `reqwest`'s `timeout` runs from connect until the body finishes, so the
  300-second default cut off any transfer longer than five minutes no matter how
  healthy the connection was - a 3 GB file over a 20 Mbit link failed every time,
  identically, so it could never be fetched at all. It now bounds the connect
  phase and each read of the body. If you raised the value to fit a large
  download, you can lower it again.
- **A download writes to a temporary file and renames it into place** (#49). The
  destination now only ever holds a complete file.
- **The CKAN client actually sends the credentials you configure** (#56). The
  request builder never attached the configured API key, basic auth, bearer
  token, or user agent, so calls a caller believed were authenticated went out
  anonymous. The server answered "not authorized" or an empty result set, and
  nothing indicated the credential had never been sent. Priority is API key,
  then bearer, then basic. `Debug` on `Configuration` and `ApiKey` now redacts
  every secret.
- **`CkanError` discriminates transport from parse from business failure** (#72).
  A JSON decode failure was classified `RequestError`, documented as covering
  "connection failures, timeouts, DNS resolution issues", so the error class
  depended on which layer of the same document failed - and the rustdoc example
  teaches consumers to branch on exactly that distinction. Decode failures are
  now `ParseError`. Separately, `ApiError.message` carries CKAN's own parsed
  error text rather than the raw envelope or the fixed literal
  `"CKAN API reported failure"`.
- **The Catalog client now has timeouts** (#48). `Configuration::default()` built
  a bare `reqwest::Client`, which applies no connect timeout and no request
  timeout, so a host that accepted the connection and never sent headers hung
  the caller forever. Because the MCP run loop was serial, one stalled metadata
  request blocked every later request including `shutdown`, and the process had
  to be killed. The default is now a 10-second connect timeout and a 30-second
  request timeout, matching what this crate's README already documented. Use
  `Configuration::with_timeouts` for different bounds.
- **One TLS stack, and you can now choose which** (#47). `data-gov` declared
  `reqwest` without `default-features = false`, so reqwest's own default
  backend was always enabled on top of whatever `data-gov-catalog` and
  `data-gov-ckan` selected. Every build compiled **both** native-tls (pulling
  `openssl`) and rustls, and the `native-tls` / `rustls-tls` features the lower
  two crates expose could not actually select anything, because `data-gov`
  re-enabled the default underneath them. A default workspace build drops from
  173 crates to 156.

  `data-gov` and `data-gov-mcp-server` now expose their own `native-tls`
  (default) and `rustls-tls` features, forwarding the choice down the whole
  chain. To drop `openssl` entirely:

  ```toml
  data-gov = { version = "0.5", default-features = false, features = ["rustls-tls"] }
  ```

  If you were relying on rustls being present because reqwest's default pulled
  it in, you must now select it explicitly.
- **Inter-crate dependencies resolve from the tree you cloned** (#75). No member
  manifest declared a sibling `path`, so local resolution worked only through a
  root `[patch.crates-io]` table — and `[patch]` applies to the top-level
  workspace of the build being run, so it was never inherited by consumers. A
  git-dependency consumer silently got the published crate instead of the tree
  they cloned; three commits had already landed on `data-gov-catalog/src` while
  `data-gov` was pinned to a published version. Each edge now declares both
  `version` and `path`, and the patch table is gone.
- All dependencies refreshed to their latest semver-compatible releases.
  `rustyline` 17 → 18 is deliberately **not** included; it is a major bump
  under the whole REPL and is tracked separately.

### Security
- **Credentials in a `base_url` are masked wherever it is displayed** (#86). A
  URL carrying userinfo would otherwise reach an error message, a log line, or
  `config show`.
- **A distribution title could write anywhere on the filesystem** (#45). The DCAT
  `title` was joined onto the output directory verbatim, so `../../outside/x`
  escaped the tree and an absolute `/etc/cron.d/evil` discarded the output
  directory entirely - `Path::join` replaces the whole path on an absolute
  component. Titles come from harvested third-party agency metadata, not from
  anything the operator controls. Confirmed end to end against a mock server
  before the fix. `format` was a second injection point and is sanitized too, and
  the transfer verifies the resolved path is still inside the chosen directory as
  a backstop.
- **A harvested URL could reach cloud instance metadata** (#51). The download URL
  went straight to `reqwest` with no scheme allowlist, no destination check, and
  reqwest's default redirect policy. A compromised harvest source could point at
  `169.254.169.254`, and the fetched credentials landed on disk - with the path
  handed back to the model in the MCP case. Redirects are now followed manually
  so every hop is checked by the same async validation that resolves hostnames;
  the synchronous redirect policy could not resolve, and a configured proxy meant
  the resolver never saw the destination either. **Residual risk, stated plainly:
  a name we resolve and a proxy resolves can differ, so this closes the
  demonstrated escape but does not make a hostile proxy safe.**
- **`openssl` is now avoidable rather than unconditional** (#47). It was
  reachable only because `reqwest` pulled its default TLS backend, which is why
  the seven GHSA advisories below applied to this workspace at all. Selecting
  `rustls-tls` now leaves `cargo tree -i openssl` empty for every crate. The
  rustls path brings `aws-lc-rs` in openssl's place, which is a C library with
  the same class of build requirement — a different trade, not a free one.
- **Cleared three advisories** by refreshing the lockfile (#46):
  - `quinn-proto` 0.11.14 → 0.11.16 — RUSTSEC-2026-0185, remote memory
    exhaustion from unbounded out-of-order stream reassembly (7.5 High).
  - `openssl` 0.10.75 → 0.10.81 — seven GHSA advisories, five High, covering
    out-of-bounds writes in AES key wrap, `digest_final()` writing past the
    caller buffer, and adjacent memory leaked to the peer via PSK/cookie
    trampolines. These are absent from the RustSec database, so `cargo audit`
    alone did not report them.
  - `anyhow` 1.0.102 → 1.0.104 — RUSTSEC-2026-0190, unsoundness in
    `Error::downcast_mut()`.

### Added
- MCP `ping` and `notifications/cancelled`. Cancelling drops the in-flight
  handler and sends no response, as the spec requires.
- `util::join_inside` in `data-gov`, which joins a component onto a directory and
  refuses a result that leaves it.
- `DataGovConfig::with_private_network_downloads`.
- `SpatialFilter`, `SortOrder`, `Configuration::with_timeouts`,
  `DEFAULT_CONNECT_TIMEOUT`, and `DEFAULT_TIMEOUT` in `data-gov-catalog`, all
  re-exported from the crate root.
- `Configuration::with_timeouts` in `data-gov-ckan`, the supported way to build a
  client with specific timeouts.

### Fixed
- **One non-UTF-8 byte no longer ends the MCP session** (#55). `next_line`
  returns an error on invalid UTF-8 and it propagated to `main`, so a single
  malformed byte killed the server for every subsequent well-formed request.
- **A cancelled id that is then reused is answered** (#65). A completing request
  could deregister a retry that had claimed its freed id, dropping that retry's
  cancellation sender and leaving it with no response and no error.
- **`tools/call` with no `arguments` succeeds** where the schema requires none,
  and **`datasetSubdirectory: false` is honoured when `outputDir` is omitted**
  (#70) - the flag was inert across half its input space.
- **`unavailableFormats` names the right format** (#70). A `.filter()` dropped
  blank entries, so the vector no longer index-aligned with the one it was
  zipped against and the report paired the wrong strings.
- **An interrupted download no longer destroys the file it was replacing** (#49).
  `File::create` is create-plus-truncate, so an existing complete file was zeroed
  the moment the request succeeded, and every error path left a partial file
  under the final name with nothing to distinguish it from a whole one. A
  40 MB truncated `report.csv` looked like an ordinary CSV to every downstream
  tool.
- **The last chunk of a download is no longer lost.** The file was never flushed,
  and dropping a `tokio::fs::File` does not flush it - the transfer was still
  reported complete.
- **A body shorter than its declared `Content-Length` is a failure**, not a
  success.
- **`max_concurrent_downloads: 0` no longer hangs forever** (#73). The builder
  clamped it, but the field is public on a struct that was not
  `#[non_exhaustive]`, so a struct literal built a zero-permit semaphore whose
  `acquire()` never resolved - no error, no timeout, no log. It presented as
  "only multi-file datasets hang", because the single-distribution path skips the
  semaphore. Clamped where it is used.
- **A dropped connection reading an error body is a transport failure** (#72).
  `data-gov-ckan` read a non-2xx body with `unwrap_or_else`, so a connection that
  dropped mid-body - or the client's own timeout firing - was discarded and the
  literal `"Unknown error"` became an `ApiError`. Retry logic keyed on
  `RequestError` never fired, because a dropped connection was indistinguishable
  from a permanent CKAN-reported 500.
- **The DCAT contact name is no longer silently dropped** (#61).
  `ContactPoint::fn_` keyed on the literal JSON name `fn_`, because serde does
  not strip trailing underscores and the `rename` its own comment and rustdoc
  both claimed was never written. Real payloads send `fn`, so every contact name
  parsed as `None` and re-serializing emitted the schema-invalid key `fn_`. The
  Contact line in the CLI's dataset view was unreachable for all real data.
- **Path ids cannot retarget a request** (#71). `harvest_record`,
  `harvest_record_raw`, `harvest_record_transformed`, and `location_geometry`
  interpolated caller-supplied ids into the URL unencoded, so an id containing
  `..`, `%2e`, `#`, or `?` could redirect the GET at a different endpoint - and
  the `#` case stranded the suffix in the fragment, letting an unrelated JSON
  object deserialize into an all-`None` model with no error at all. All four now
  percent-encode the segment.

  Encoding alone is not sufficient, and this is the part worth knowing: a
  segment that is *entirely* dots cannot be carried at all. The URL standard
  looks for dot-segments after decoding and treats `%2E` as a dot, so an id of
  `..` collapses `/harvest_record/{id}/transformed` to `/transformed` no matter
  how it is escaped. `""`, `"."`, and `".."` are therefore refused before any
  request, with the new `CatalogError::InvalidPathSegment`. `dataset_by_slug`
  had the same hole and is fixed too.
- **A build with no TLS backend is rejected instead of shipped** (#47). Turning
  reqwest's defaults off made a new configuration reachable:
  `default-features = false` with neither `native-tls` nor `rustls-tls`
  compiled clean and produced a client with an HTTP-only connector. Every
  data.gov endpoint is HTTPS, so the crate built and then failed on the first
  request with `invalid URL, scheme is not http` — an error naming the scheme
  rather than the missing feature. All four crate roots now fail the build with
  a message that says which feature to enable.
- **`dataset_by_slug` now resolves every dataset that exists** (#94). It was
  implemented as a full-text search — `q=<slug>&per_page=20`, scanning the page
  for an exact match — because the `slug=` query parameter it was written
  against never existed in the API. A slug is a lossy derivation of the title
  (truncated at 90 characters mid-word, punctuation collapsed, `U.S.` flattened
  to `u-s`), so its tokens are frequently absent from the indexed text and the
  query returned nothing. Measured against live data.gov: **15% of datasets
  unresolvable on a uniform sample, 27% past cursor depth 400, ~69% of slugs at
  the 90-character cap.**

  It now uses `GET /api/dataset/{slug_or_id}`, the exact-lookup endpoint
  declared in the API's own OpenAPI document at `/openapi.json`. Same one
  request, no ranking, no prefix matching. This affects every entry point that
  takes a slug — REPL `cd`/`show`/`download`, one-shot CLI, and the
  `data_gov.dataset` and `data_gov.downloadResources` MCP tools — all of which
  previously reported "not found" for datasets they had just listed.
- **A data.gov outage is no longer reported as a missing dataset** (#94). Only a
  404 yields `Ok(None)`; every other non-2xx surfaces as
  `CatalogError::ApiError` and network failure as `CatalogError::RequestError`.
- **The slug is percent-encoded into a single path segment**, so a value
  containing `..`, `%2e` or a slash cannot redirect the request to another
  endpoint.

### Removed
- Four more unused dependencies from `data-gov-ckan` (#105): `async-trait`,
  `serde_repr`, `url`, and `uuid`. Found by the new `just check-deps` gate, which
  was added for #76 and immediately paid for itself - #76's own sweep had only
  covered `data-gov` and `data-gov-mcp-server`.
- Six declared-but-unused dependencies (#76): `serde`, `serde_json`,
  `tokio-util`, and `anyhow` from `data-gov`; `futures` and `data-gov-catalog`
  from `data-gov-mcp-server`, which reaches catalog types through the
  `data_gov::catalog` re-export. The lockfile drops from 308 to 279 crates.

### Infrastructure

- **Working docs are plain ASCII, and CI enforces it.** `CLAUDE.md` and the
  `justfile` used em-dashes and arrow glyphs throughout - characters nobody
  types by hand, in the file that documents the project's own writing standard.
  `just check-ascii` fails the gate on any non-ASCII character in those files.
  The READMEs are deliberately exempt: their emoji headings and box-drawing
  trees are presentation, and converting them would cost more than it returns.
- **Documented the commit and writing conventions in `CLAUDE.md`**, which had
  covered branches, PRs, and the tracker but never said what a commit should
  look like: conventional subjects scoped to the crate, one concern each, never
  against a failing gate, lockfile committed, and a body carrying the
  measurements the diff cannot show.

- **Fixtures now record where they came from** (#101). `tests/fixtures/MANIFEST.json`
  holds the endpoint, HTTP status, source host, and capture date for every
  fixture; `just fixtures` writes it and `fixture_parity_tests` enforces it, so a
  fixture added without provenance fails the build. A fixture that cannot be
  captured is listed under `unverified` with a reason — currently only
  `harvest_record_transformed.json`, whose endpoint 404s for every sampled record
  (#83).
- **Captured the negative responses, not only the success cases** (#101). The
  404 body from `/api/dataset/{absent}` and the empty-result body from `/search`
  are now fixtures. They are different envelopes — the empty search omits `total`
  and the cursor entirely — and tests assert both deserialize, which is what
  keeps every envelope field optional.
- **`dataset_by_slug.json` and `dataset_by_slug_truncated.json` are refreshable
  again** (#101). Both existed as captures the script never regenerated. The
  truncated one is pinned to a dataset at the 90-character slug cap.
- **Adopted the newest shared engineering standards in `CLAUDE.md`**: fixtures
  mocked from captured responses rather than hand-written bodies, with
  `data-gov-ckan`'s UUID-shaped test ids as the worked example of why; adversarial
  review run from a fresh context with distinct lenses; findings carrying the
  burden of proof, illustrated by four confidently wrong findings from this
  review; reporting the limits of what was checked; searching several independent
  axes until two rounds come up empty; deciding at the top and briefing each
  branch; and weighing candidate designs before committing.

- **Warnings are now denied by the manifest, not by a flag** (#97). A
  `[workspace.lints]` table in the root `Cargo.toml`, inherited by all four
  members, makes a plain `cargo build` fail on any compiler or clippy warning.
  Enforcement previously depended on someone passing `-D warnings`, so it held
  in CI and nowhere else.
- **Added a `justfile`, and CI now calls it** (#97). The gate commands lived in
  the workflow and in `CLAUDE.md` as two dialects that could drift apart; they
  are now one set of recipes that both a developer and the pipeline run.
  `just check` is the gate; `just` lists the rest. Contributors need `just`
  installed, which `CONTRIBUTING.md` now states.
- **The rustls-only build is gated** (#98). `--all-features` enables
  `native-tls` and `rustls` together, a combination no consumer selects, so the
  rustls-only configuration consumers do select was never compiled or tested.
  `just check-rustls` builds it, in the gate and locally.
- **Adopted the remaining shared engineering standards in `CLAUDE.md`**: strict
  types outbound and permissive types inbound, with the three deserialization
  failures in this workspace as the evidence; a normal negative answer is not a
  failure, worked through `dataset_by_slug` returning `Ok(None)` on 404;
  idempotent and resumable operations, aimed at downloads; coverage as a floor
  rather than a target; acceptance criteria as individually named tests;
  adversarial self-review of the fixes as a second pass; transport kept at the
  crate edges; no machine-specific or personal data in the repository; the
  release-branch override of the merge default, stated as a table of what is and
  is not authorised; and GitHub Issues recorded as the tracker.
- **Recorded the lessons from the 2026-07 review in `CLAUDE.md`**, each written
  from a specific failure rather than from principle: falsifiability is proven
  by mutation rather than by introspection; assertions are driven from the
  registry rather than one hand-picked instance; the machine-readable API spec
  outranks the prose docs; work branches never stack, because a stacked PR
  receives no CI and says so quietly.
- **Adopted the engineering standards from the adelie-ai repositories** that
  this project did not already cover: mechanical warnings-as-errors via a
  `[workspace.lints]` table, the observation that `--all-features` compiles a
  superset rather than exercising mutually exclusive features, an advisory scan
  that cannot pass by accident, scanning before the first build because
  `build.rs` executes at compile time, adversarial self-review of a diff before
  requesting review, earning abstractions at roughly three call sites, worktree
  and issue hygiene, and a reference list of which upstream sources are
  authoritative and which are stale.
- Documented the testing standard in `CLAUDE.md`: test conditions derive from
  the specification or real API data and never from the current implementation;
  every test must name the wrong implementation it would catch; real captured
  payloads are preferred to synthetic bodies; edge cases and backfilling are
  expected. Written up from two live examples in this codebase where a test
  asserted the buggy behaviour and so guaranteed it.
- **Captured a fresh set of Catalog API fixtures** and added
  `scripts/capture-fixtures.sh` to refresh them. Fixtures are the project's
  record of what the API actually returns; slug-addressed captures are pinned to
  a long-lived dataset so tests can assert against their contents, and a failed
  request never truncates an existing fixture.
- **Added `fixture_parity_tests.rs`** — deserializes each captured response into
  the model it should populate and asserts the fields actually arrive. This
  catches a class the wiremock tests structurally cannot: `client_tests.rs`
  proves a request was *sent* correctly, not that a response is *understood*.
  A field that silently becomes `None` because its serde name does not match the
  wire name passes every wiremock test while losing data.
- Documented the configuration and file-location policy in `CLAUDE.md`: config,
  cache, and state go in the XDG base directories via the `dirs` crate, never as
  dotfiles in `$HOME`; precedence is flag > environment > config file > default;
  secrets are never accepted as command-line arguments; secret files are `0600`
  inside a `0700` directory, with the mode set at creation rather than
  `chmod`-ed afterwards, and checked on read.

- **CI now runs 181 tests instead of 37** (#23). The gate selected `--lib`,
  which skips both binary crates (`data-gov-mcp-server` has no `lib.rs`; the
  CLI is a `[[bin]]`) and every `tests/` integration target — so all 54
  MCP-server tests, all 42 CLI tests, all 7 download tests, and all 14 catalog
  wiremock tests never ran. `clippy --all-targets` compiled them, which is why
  the gap was invisible.
- **CI now runs on `release/**` branches** (#23). Triggers were limited to
  `main`, so pull requests targeting a release branch received no checks at all.
- **Replaced the permanently-green integration job** (#23). It ran
  `cargo test --test integration_tests` in `data-gov-ckan`, where all 17 tests
  are `#[ignore]`d — reporting success having executed nothing. Network tests
  now live in an opt-in `workflow_dispatch` job that runs `-- --ignored`, so a
  data.gov outage cannot turn pull requests red.
- **Examples are compiled workspace-wide** (#25). The job built only
  `data-gov-ckan`'s examples, leaving `data-gov/examples/demo.rs` uncompiled.
- **Removed the no-op documentation-coverage step** (#25). It took its exit
  status from `jq`, which succeeds on empty input, and compared the reported
  percentage against no threshold — it could not fail. Enforcement moves to a
  `missing_docs` lint once the outstanding gaps are documented (#59).
- **Added an OSV/GHSA lockfile scan** alongside `cargo audit` (#46). The seven
  `openssl` advisories were GHSA-only and invisible to `cargo audit`.

## [0.4.0] - 2026-04-25

The Catalog API migration and the reqwest 0.13 upgrade shipped together in
0.4.0; both are recorded below.

### Breaking — Catalog API migration
- **data.gov retired its CKAN Action API.** The workspace now targets the new
  [Catalog API](https://resources.data.gov/catalog-api/) (cursor-paginated,
  DCAT-US 3 payloads, no API keys).
- **New `data-gov-catalog` crate** replaces `data-gov-ckan` as the backend
  for `data-gov` and `data-gov-mcp-server`. The CKAN crate is retained as a
  general-purpose client for other CKAN-compatible portals, but is no longer
  used by data.gov.
- **`DataGovClient::search` signature** changed: `offset`/`format` parameters
  removed; a cursor-based `after: Option<&str>` replaces offset. Returns
  `SearchResponse` (from the catalog crate) instead of CKAN's
  `PackageSearchResult`.
- **`DataGovClient::get_dataset(slug)`** now returns a `SearchHit` (not
  `Package`) and resolves strictly by slug; harvest-record UUIDs go through
  the new `get_dataset_by_harvest_record(id)`.
- **`DataGovClient::download_resources`** renamed to `download_distributions`
  and takes `&[Distribution]`. `download_resource` → `download_distribution`.
- **`DataGovClient::get_downloadable_resources`** renamed to
  `get_downloadable_distributions` and takes `&Dataset`.
- **`DataGovClient::get_resource_filename`** renamed to
  `get_distribution_filename`.
- **`DataGovClient::ckan_client()`** replaced by `catalog_client()`.
- **`DataGovConfig::with_api_key` removed** — the Catalog API is unauthenticated.
- **`data_gov::ckan` re-export** replaced by `data_gov::catalog`.
- **`DATA_GOV_BASE_URL`** constant now points at `https://catalog.data.gov`
  (was `https://catalog.data.gov/api/3`).
- **CLI `--api-key` flag removed** (Catalog API is public).
- **MCP server** drops the `ckan.packageSearch`, `ckan.packageShow`, and
  `ckan.organizationList` tools. The `data_gov.search` params lost `offset`
  and `format`; the new `after` cursor and `organizationContains` client-side
  filter remain. `data_gov.downloadResources` replaces `resourceIds` with
  `distributionIndexes`; the `formats` filter is now matched client-side
  against both `format` and `mediaType`.

### Breaking — reqwest 0.13 and client construction
- **Removed `Default` impl for `DataGovClient`** — use `DataGovClient::new()?`
  instead. The previous impl could panic if the HTTP client failed to build.
- **Upgraded reqwest from 0.12 to 0.13** across `data-gov-ckan` and `data-gov`.
  If you depend on reqwest types re-exported from these crates, you may need to
  update your own reqwest dependency.
- **`rustls-tls` feature** in `data-gov-ckan` now maps to `reqwest/rustls`
  (was `reqwest/rustls-tls`). The feature name on `data-gov-ckan` is unchanged;
  only the underlying reqwest feature differs.
- Default user-agent string now reflects the actual crate version
  (`data-gov-rs/0.4.0`) instead of the previously hardcoded `data-gov-rs/1.0`.

### Changed
- **CKAN client refactored** — extracted `call_action<T>` generic helper,
  reducing `client.rs` from ~1243 to ~771 lines and eliminating 10 copies of
  HTTP boilerplate. Uses reqwest's `.query()` instead of manual URL encoding,
  removing the `urlencoding` dependency.
- **MCP server tool specs** — converted from a function returning `Vec<ToolSpec>`
  to a `static TOOL_SPECS: LazyLock<Vec<ToolSpec>>` (allocated once).
- **JSON-RPC version validation** — the MCP server now rejects requests where
  `jsonrpc` is present but not `"2.0"`.
- **CLI version** — now uses `env!("CARGO_PKG_VERSION")` instead of hardcoded
  `"1.0"`.
- **`download-dir` CLI flag** — removed magic string default detection;
  the flag is now purely optional.
- Path sanitization logic deduplicated into `data_gov::util`.
- **MCP server modularized** — split monolithic `server.rs` (1548 lines) into
  four focused modules: `server.rs` (run loop), `types.rs` (request/response
  types and param structs), `tools.rs` (tool specs and lookup), `handlers.rs`
  (method dispatch and handler logic).

### Added — data-gov-catalog crate
- **`data-gov-catalog`** — new crate wrapping the Catalog API with typed
  models for DCAT-US 3 (`Dataset`, `Distribution`, `Publisher`,
  `ContactPoint`), search envelopes (`SearchResponse`, `SearchHit`),
  organizations, keywords, locations, and harvest records. Endpoint coverage:
  `/search` (with `SearchParams` builder), `/api/organizations`,
  `/api/keywords`, `/api/locations/search`, `/api/location/{id}`,
  `/harvest_record/{id}`, `/harvest_record/{id}/raw`,
  `/harvest_record/{id}/transformed`.

### Added — tests, tooling, and utilities
- **Comprehensive test suite** — 130+ tests across the workspace:
  - 21 wiremock-based unit tests for all CKAN client endpoints
    (`data-gov-ckan/tests/unit_tests.rs`)
  - 38 unit tests for the MCP server's pure functions, serialization, tool
    specs, and error codes (spread across `types.rs`, `tools.rs`, `server.rs`)
  - 11 fixture-based tests for the high-level `DataGovClient` using captured
    API responses (`data-gov/tests/client_tests.rs`)
  - 5 tests for path sanitization (`data-gov/src/util.rs`)
- `DataGovClient::config()` — read access to the current configuration.
- `DataGovConfig::with_base_url()` — override the CKAN API base URL (useful
  for testing with mock servers).
- `data_gov::util::sanitize_path_component()` — shared path sanitization
  function used by the CLI and MCP server.
- `CLAUDE.md` development guide covering TDD workflow, file organization,
  dependency management, and security checklist.
- `CHANGELOG.md` (this file).
- `Cargo.lock` is now committed for reproducible binary builds.

### Fixed
- **Parallel download progress bars** — replaced independent `ProgressBar`
  instances with `indicatif::MultiProgress` so concurrent downloads render
  correctly instead of overwriting each other.
- **UTF-8 string truncation panic** — `&notes[..100]` byte slicing replaced
  with `chars().take(100)` in three locations to prevent panics on multi-byte
  characters.
- **`setdir` REPL command discarded user config** — now clones the existing
  configuration instead of creating a fresh default, preserving API key,
  timeouts, and other settings.
- **`output_dir` MCP parameter path traversal** — rejects paths containing
  `..` to prevent writing outside the intended directory.
- **Download progress per-chunk cloning** — `DownloadProgress` struct is now
  constructed once before the download loop; only `downloaded_bytes` is updated
  per chunk.

### Deprecated
- **`data-gov-ckan`** crate-level docs and README now note that data.gov no
  longer uses CKAN. The crate remains published and functional for use against
  other CKAN-compatible instances (European, state, municipal, university
  portals).

### Removed
- `urlencoding` dependency from `data-gov-ckan`.
- `extern crate` declarations from `data-gov-ckan/src/lib.rs` (unnecessary
  since Rust 2018).
- Crate-level `#![allow(unused_imports)]` from `data-gov-ckan`; 17 unused
  imports cleaned up.

### Infrastructure
- Updated `actions/cache` from v3 to v4 in CI and release workflows.
- Replaced deprecated `actions/create-release@v1` with
  `softprops/action-gh-release@v2` in release workflow.

## [0.3.1] - 2025-10-25

Previous release. See git history for details.
