---
applyTo: '**'
---

# data-gov-rs — Project Context for AI Assistants

This file is the "what does this project actually do" briefing for AI
assistants generating, reviewing, or editing code in this workspace. For
the engineering rules (testing, error handling, security checklist,
versioning), see [`CLAUDE.md`](../../CLAUDE.md) at the repo root.

## Project overview

`data-gov-rs` is a Rust workspace publishing four crates for working with
US government open data:

| Crate                | Purpose                                                                   |
|----------------------|---------------------------------------------------------------------------|
| `data-gov-catalog`   | Async client for the data.gov [Catalog API](https://resources.data.gov/catalog-api/) (current backend; DCAT-US 3, cursor-paginated) |
| `data-gov`           | High-level client + CLI binary built on `data-gov-catalog`                |
| `data-gov-mcp-server`| MCP server exposing `data-gov` to AI tools over JSON-RPC on stdin/stdout  |
| `data-gov-ckan`      | Generic CKAN Action API client for non-data.gov portals (state, municipal, university, European). Maintenance mode. |

Rust 2024 edition, MSRV 1.90, Apache-2.0.

## Important — backend migration in 0.4.0

**As of 2026 data.gov no longer uses CKAN.** It serves the catalog through a
purpose-built Catalog API. If you read older docs, blog posts, or training
data, assume they describe the retired CKAN endpoint. Concretely:

- The high-level `data-gov` crate is backed by `data-gov-catalog`, NOT
  `data-gov-ckan`.
- `package_search` / `package_show` / `organization_list` are CKAN concepts.
  They live in `data-gov-ckan` and are valid against third-party CKAN
  portals — but **never** generate code that calls them against
  `catalog.data.gov` (it will 404).
- `data.gov` returns DCAT-US 3 (`Dataset`, `Distribution`, `Publisher`,
  `ContactPoint`), not CKAN's `Package`/`Resource`.

## Catalog API quick reference

```
Base URL:   https://catalog.data.gov
Auth:       none (public)
Pagination: cursor-based via `after`; pass response.after on the next call
Format:     DCAT-US 3 JSON (https://resources.data.gov/resources/dcat-us/)
```

Endpoints currently covered by `data-gov-catalog`:

- `/search` — full-text search with cursor pagination (`SearchParams`
  builder for `q`, `per_page`, `after`, `org_slug`, `org_type`,
  `keyword(s)`, `spatial_*`, `slug`, `sort`)
- `/api/organizations` — list publishing organizations
- `/api/keywords` — keyword facets
- `/api/locations/search`, `/api/location/{id}` — spatial lookups
- `/harvest_record/{id}`, `/harvest_record/{id}/raw`,
  `/harvest_record/{id}/transformed` — raw + transformed harvest records

### High-level shortcuts (`data-gov`)

```rust
use data_gov::DataGovClient;

# async fn run() -> data_gov::Result<()> {
let client = DataGovClient::new()?;

// Cursor-based pagination — `per_page` is the page size, `after` is the cursor.
let page = client.search("climate", Some(20), None, None).await?;
let next = client.search("climate", Some(20), page.after.as_deref(), None).await?;

// Lookup is by slug. Harvest-record UUIDs go through a separate method.
let hit = client.get_dataset("consumer-complaint-database").await?;
let dataset = client.get_dataset_by_harvest_record("…uuid…").await?;

// Distributions, not resources.
if let Some(dcat) = hit.dcat.as_ref() {
    let dists = DataGovClient::get_downloadable_distributions(dcat);
    if let Some(d) = dists.first() {
        client.download_distribution(d, None).await?;
    }
}
# Ok(()) }
```

### Common Catalog API parameters (cheat sheet)

| Need                          | Use                                                 |
|-------------------------------|-----------------------------------------------------|
| Free-text search              | `SearchParams::new().q("…")`                        |
| Filter by publisher slug      | `.org_slug("epa-gov")`                              |
| Filter by org type            | `.org_type("Federal Government")`                   |
| Keyword facets                | `.keyword("…")` or `.keywords([…])`                 |
| Geographic intersection       | `.spatial_geometry(geojson).spatial_within(false)`  |
| Page size                     | `.per_page(50)` (default 10, server caps the max)   |
| Next page                     | `.after(prev_response.after.unwrap())`              |
| Lookup a known slug           | `.slug("dataset-slug")` or `client.dataset_by_slug` |

## CKAN API (for `data-gov-ckan` only)

The `data-gov-ckan` crate covers the standard CKAN Action API surface:
`package_search`, `package_show`, `organization_list`, `group_list`,
`tag_list`, `user_list`, plus the matching `*_autocomplete` helpers.
Authentication is via `Configuration.api_key`, basic auth, or custom
headers on the `reqwest::Client`.

**Use this crate only against non-data.gov CKAN deployments.** Active
development focuses on `data-gov-catalog`; CKAN coverage is in
maintenance mode (bugfixes and security patches).

## MCP server (`data-gov-mcp-server`)

JSON-RPC 2.0 over stdin/stdout. Tools are invoked via `tools/call` with the
tool's snake_case `name`. Tools currently exposed:

- `data_gov_search` — cursor-paginated; arguments `query`, `limit` (1–1000),
  `after`, `organization`, `organizationContains` (client-side substring
  filter on org names)
- `data_gov_dataset` — DCAT-US 3 metadata. Argument: `slug`
- `data_gov_autocomplete_datasets` — capped full-text search; arguments
  `partial`, `limit` (1–100)
- `data_gov_list_organizations` — argument `limit` (1–1000)
- `data_gov_download_resources` — arguments `datasetId` (slug), optional
  `outputDir`, optional `formats` filter (case-insensitive substring match
  against `format` or `mediaType` — so `"JSON"` matches `application/json`),
  optional `distributionIndexes` (zero-based)

Plus the MCP lifecycle methods (`initialize`, `initialized`, `shutdown`,
`tools/list`, `tools/call`). The legacy `ckan.*` tools were removed in
0.4.0.

The same tools are also reachable as direct JSON-RPC methods under
dot-camelCase names (`data_gov.search`, `data_gov.dataset`, etc.), but
that's a non-MCP back-channel — generated code should always use
`tools/call name=<snake_case>` so it works with any MCP client.

## Code-generation guidance

### When asked to "search data.gov"

Use `data_gov::DataGovClient::search(query, per_page, after, organization)`
or, for advanced filters, drop down to `data_gov_catalog::SearchParams`
via `client.catalog_client()`. Do NOT generate `package_search` calls.

### When asked to "download files from data.gov"

Use `download_distribution` or `download_distributions` on
`DataGovClient`. The Catalog API exposes DCAT `Distribution` objects with
`download_url` and (sometimes) `access_url`. A distribution is
"downloadable" when it carries a `download_url` — `accessURL`-only
entries are API references, not files.

### When asked about API keys for data.gov

There aren't any. The Catalog API is public. The `--api-key` CLI flag and
`DataGovConfig::with_api_key` were removed in 0.4.0. If a user has an
old CKAN API key, it is no longer used.

### When asked to add a new endpoint

1. Add the response model in the relevant crate's `models.rs` (catalog)
   or model module (CKAN). Use `Option<T>` liberally — both APIs return
   sparse fields.
2. Add the client method in `client.rs`, returning the crate's `Result`
   alias.
3. Cover it with a wiremock fixture test in `tests/`.
4. Add an `#[ignore]`'d live integration test for documentation.
5. Add a rustdoc comment with `# Errors` and a runnable example
   (`no_run` if it requires network access).

### Common gotchas

1. **DCAT field names use camelCase in JSON** (`downloadURL`, `mediaType`,
   `accessURL`) — serde rename attributes handle the mapping. Don't
   guess Rust field names; consult `data-gov-catalog/src/models.rs`.
2. **Slugs vs. identifiers**: a dataset's `slug` is what the
   `dataset_by_slug` endpoint accepts; `identifier` is the publisher's
   own ID and is opaque. Harvest-record UUIDs are different again — use
   `get_dataset_by_harvest_record`.
3. **No offsets**: pages can only be walked forward via `after`.
4. **Distribution count varies wildly** — some datasets have one CSV,
   others have 50+ files across formats. Don't assume `distributions[0]`
   is "the data".
5. **No silent error swallowing** — `let _ =`, `.ok()`, and empty
   `Err(_) => {}` arms are not allowed in library code (see
   [`CLAUDE.md`](../../CLAUDE.md)).
6. **No `unsafe`, no blocking in async, no `unwrap()` in library code.**

### Stable test fixtures

For wiremock or assertion targets that need a real-looking dataset, these
slugs have been stable for years:

- `consumer-complaint-database` (CFPB)
- `electric-vehicle-population-data`

Org slugs that have been stable: `epa-gov`, `gsa-gov`, `omb-gov`,
`ed-gov`.

## Where to look

| Question                         | File                                              |
|----------------------------------|---------------------------------------------------|
| Engineering rules (tests, errors)| [`CLAUDE.md`](../../CLAUDE.md)                    |
| Catalog client surface           | `data-gov-catalog/src/client.rs`                  |
| Catalog DCAT models              | `data-gov-catalog/src/models.rs`                  |
| High-level helpers               | `data-gov/src/client.rs`                          |
| MCP tool registry & dispatch     | `data-gov-mcp-server/src/{tools,handlers}.rs`     |
| What changed and when            | [`CHANGELOG.md`](../../CHANGELOG.md)              |
