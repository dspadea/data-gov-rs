# data-gov-catalog

Async Rust client for the data.gov [Catalog API](https://resources.data.gov/catalog-api/).
Returns [DCAT-US 3](https://resources.data.gov/resources/dcat-us/) metadata,
cursor-paginated, no API key required.

[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](../LICENSE)

> **The current data.gov backend.** As of 2026 data.gov retired its CKAN
> Action API and replaced it with this purpose-built Catalog API. If you
> were previously calling `package_search` / `package_show` against
> `catalog.data.gov`, you want this crate (or the higher-level
> [`data-gov`](../data-gov/) wrapper). The legacy
> [`data-gov-ckan`](../data-gov-ckan/) crate is retained for non-data.gov
> CKAN portals.

## Requirements

- Rust **1.90+** (Rust 2024 edition)
- A Tokio runtime

## Add to your project

```toml
[dependencies]
data-gov-catalog = "0.4"
tokio = { version = "1", features = ["full"] }
```

Working inside this repository? `data-gov-catalog = { path = "../data-gov-catalog" }`.

## Highlights

- 🔍 Cursor-paginated full-text search with org / type / keyword / spatial filters
- 🧾 DCAT-US 3 typed models (`Dataset`, `Distribution`, `Publisher`, `ContactPoint`)
- 🏛️ Organizations, keywords, locations, and harvest-record endpoints
- ⚙️ Async via `reqwest` + `tokio`; configurable TLS backend
- 🧪 Wiremock-based unit tests + opt-in live integration tests

## Quick start

```rust
use data_gov_catalog::{CatalogClient, Configuration, SearchParams};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = CatalogClient::new(Arc::new(Configuration::default()));

    let page = client
        .search(SearchParams::new().q("climate").per_page(10))
        .await?;

    for hit in page.results.iter().take(3) {
        let title = hit.title.as_deref().unwrap_or("(untitled)");
        let slug  = hit.slug.as_deref().unwrap_or("(no-slug)");
        println!("• {title} ({slug})");
    }

    Ok(())
}
```

### Cursor-based pagination

The Catalog API has no random-access offset. The `SearchResponse` carries
an `after` cursor when more pages exist; pass it back unchanged on the
next call to advance one page:

```rust
# use data_gov_catalog::{CatalogClient, Configuration, SearchParams};
# use std::sync::Arc;
# async fn run(client: &CatalogClient) -> Result<(), Box<dyn std::error::Error>> {
let page1 = client.search(SearchParams::new().q("climate").per_page(20)).await?;

if let Some(cursor) = page1.after {
    let page2 = client
        .search(SearchParams::new().q("climate").per_page(20).after(cursor))
        .await?;
    // …
}
# Ok(()) }
```

### Filtering

```rust
use data_gov_catalog::SearchParams;

// Datasets published by EPA, sorted by harvest date.
let params = SearchParams::new()
    .org_slug("epa-gov")
    .sort("last_harvested_date")
    .per_page(50);

// Federal-agency datasets tagged "air-quality".
let params = SearchParams::new()
    .org_type("Federal Government")
    .keyword("air-quality");

// Spatial: datasets whose footprint intersects a GeoJSON geometry.
let params = SearchParams::new()
    .spatial_geometry(serde_json::json!({
        "type": "Point",
        "coordinates": [-77.0369, 38.9072]
    }))
    .spatial_within(false);
```

### Single-dataset lookup

```rust
# use data_gov_catalog::CatalogClient;
# async fn run(client: &CatalogClient) -> Result<(), Box<dyn std::error::Error>> {
let hit = client.dataset_by_slug("electric-vehicle-population-data").await?;
if let Some(hit) = hit {
    if let Some(dataset) = hit.dcat {
        for dist in &dataset.distribution {
            println!("{:?} — {:?}", dist.title, dist.download_url);
        }
    }
}
# Ok(()) }
```

## API surface

| Method                        | Endpoint                              | Returns                       |
|-------------------------------|---------------------------------------|-------------------------------|
| `search(params)`              | `GET /search`                         | `SearchResponse`              |
| `dataset_by_slug(slug)`       | `GET /search?slug=…&per_page=1`       | `Option<SearchHit>`           |
| `organizations()`             | `GET /api/organizations`              | `OrganizationsResponse`       |
| `keywords(size, min_count)`   | `GET /api/keywords`                   | `KeywordsResponse`            |
| `locations_search(q, size)`   | `GET /api/locations/search`           | `LocationsResponse`           |
| `location_geometry(id)`       | `GET /api/location/{id}`              | `serde_json::Value` (GeoJSON) |
| `harvest_record(id)`          | `GET /harvest_record/{id}`            | `HarvestRecord`               |
| `harvest_record_raw(id)`      | `GET /harvest_record/{id}/raw`        | `serde_json::Value`           |
| `harvest_record_transformed(id)` | `GET /harvest_record/{id}/transformed` | `Dataset` (DCAT-US 3)     |

Errors are surfaced through [`CatalogError`]:

- `RequestError` — network, DNS, TLS, or HTTP-protocol failure
- `ParseError` — response body was not valid JSON for the expected shape
- `ApiError { status, message }` — the server returned a non-2xx status

## Configuration

```rust
use data_gov_catalog::{CatalogClient, Configuration};
use std::sync::Arc;
use std::time::Duration;

let http = reqwest::Client::builder()
    .timeout(Duration::from_secs(30))
    .build()?;

let config = Configuration {
    base_path: "https://catalog.data.gov".to_string(),
    user_agent: Some("my-app/1.0".to_string()),
    client: http,
};

let client = CatalogClient::new(Arc::new(config));
# Ok::<(), Box<dyn std::error::Error>>(())
```

`CatalogClient` holds an `Arc<Configuration>` and is cheap to clone — share
one instance across tasks rather than building a new client per request.

## Cargo features

| Feature       | Default | Effect                                |
|---------------|---------|---------------------------------------|
| `native-tls`  | yes     | Use the platform TLS stack (`reqwest/native-tls`). |
| `rustls-tls`  | no      | Use rustls instead (`reqwest/rustls`).             |

To use rustls:

```toml
[dependencies]
data-gov-catalog = { version = "0.4", default-features = false, features = ["rustls-tls"] }
```

## Development

```bash
git clone https://github.com/dspadea/data-gov-rs.git
cd data-gov-rs

cargo test -p data-gov-catalog                                # unit + wiremock fixture tests
cargo test -p data-gov-catalog --test integration_tests -- --ignored  # live data.gov
```

The wiremock fixture tests live in `tests/client_tests.rs` and use captured
responses under `tests/fixtures/`. The live-network suite in
`tests/integration_tests.rs` is `#[ignore]`'d so it stays out of the
default `cargo test` run.

## Higher-level wrappers

If you want download helpers, a CLI, or an MCP server on top of this
client, look at the sibling crates:

- [`data-gov`](../data-gov/) — high-level client + `data-gov` CLI
- [`data-gov-mcp-server`](../data-gov-mcp-server/) — MCP tools for AI agents

## Disclaimer & license

This is an independent project and is not affiliated with data.gov or any
government agency. For authoritative information, refer to the official
[data.gov](https://www.data.gov/) portal.

Licensed under the [Apache License 2.0](../LICENSE).
