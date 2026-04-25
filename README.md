# data-gov-rs

Rust tooling for exploring U.S. government open data. This workspace bundles four companion crates:

- [`data-gov-catalog`](./data-gov-catalog/) – async client for the data.gov Catalog API (DCAT-US 3, cursor-paginated search)
- [`data-gov`](./data-gov/) – higher-level client and CLI tailored to data.gov workflows
- [`data-gov-mcp-server`](./data-gov-mcp-server/) – MCP server using the `data-gov` client for AI integration
- [`data-gov-ckan`](./data-gov-ckan/) – async CKAN Action API client, **retained for use against non-data.gov CKAN portals** (see note below)

[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)

## ⚠️ Breaking change in 0.4.0 — Catalog API migration

**data.gov retired its CKAN Action API in 2026** and replaced it with a
purpose-built [Catalog API](https://resources.data.gov/catalog-api/). The
`data-gov` and `data-gov-mcp-server` crates have been rewired onto a new
`data-gov-catalog` backend; the public APIs of both have changed accordingly.

What's different about the new API:

- **DCAT-US 3 payloads** (`Dataset`, `Distribution`, `Publisher`, …) instead of
  CKAN's `Package`/`Resource` shapes.
- **Cursor-based pagination** — `search` takes an `after: Option<&str>` cursor
  in place of the old `offset`. Each `SearchResponse` carries the next cursor.
- **No API keys.** The Catalog API is unauthenticated; the `--api-key` CLI flag
  and `DataGovConfig::with_api_key` are gone.
- **Slugs, not IDs.** `get_dataset(slug)` resolves by slug; harvest-record
  UUIDs go through `get_dataset_by_harvest_record(id)`.
- **Distributions, not resources.** `download_resource(s)` →
  `download_distribution(s)`, `get_resource_filename` →
  `get_distribution_filename`.
- **MCP server** drops the `ckan.*` tools and renames parameters on
  `data_gov.search` and `data_gov.downloadResources`. See
  [CHANGELOG.md](./CHANGELOG.md) for the full diff.

### What about `data-gov-ckan`?

The `data-gov-ckan` crate is **still published and still works** against any
compliant CKAN portal — European, state, municipal, and university instances
still run CKAN. Just point `Configuration::base_path` at your target host.

That said, **active development now focuses on the Catalog API**. The CKAN
crate is in maintenance mode: bug fixes and security patches will land, but
new features are unlikely unless a contributor steps up.

## Requirements

- Rust **1.90+** (the workspace uses the Rust 2024 edition)
- Cargo and git

```bash
rustup toolchain install stable
rustup default stable
rustc --version  # should be 1.90 or newer
```

## Install the CLI

```bash
# Using cargo install
cargo install data-gov
```

Common commands:

- `data-gov search "climate change" 10`
- `data-gov show electric-vehicle-population-data`
- `data-gov download electric-vehicle-population-data 0`                              # Download by index
- `data-gov download electric-vehicle-population-data "Comma Separated Values File"`  # Download by title (quoted)
- `data-gov list organizations`

The CLI automatically adjusts colour and progress output for TTY / non-TTY environments. Tune behaviour with `--color`, `NO_COLOR`, or `NO_PROGRESS` as needed.

### Script & REPL automation

The binary doubles as an interactive REPL. You can automate workflows with shebang scripts:

```bash
#!/usr/bin/env data-gov
search climate 5
download consumer-complaint-database 0
quit
```

See [`examples/scripting`](examples/scripting/) for ready-to-run scripts.

## Library quick starts

### High-level client (`data-gov`)

```rust
use data_gov::DataGovClient;

#[tokio::main]
async fn main() -> data_gov::Result<()> {
    let client = DataGovClient::new()?;
    let page = client.search("climate change", Some(10), None, None).await?;
    println!("Found {} results on this page", page.results.len());

    let hit = client.get_dataset("consumer-complaint-database").await?;
    println!("Dataset: {}", hit.title.as_deref().unwrap_or(""));

    if let Some(dcat) = hit.dcat.as_ref() {
        let distributions = DataGovClient::get_downloadable_distributions(dcat);
        if let Some(distribution) = distributions.first() {
            let path = client.download_distribution(distribution, None).await?;
            println!("Downloaded to {path:?}");
        }
    }

    Ok(())
}
```

### Low-level Catalog API client (`data-gov-catalog`)

```toml
[dependencies]
data-gov-catalog = "0.4"
tokio = { version = "1", features = ["full"] }
```

```rust
use data_gov_catalog::{CatalogClient, Configuration, SearchParams};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = CatalogClient::new(Arc::new(Configuration::default()));

    // Cursor-based pagination: the response carries `after` when more pages exist.
    let page = client
        .search(SearchParams::new().q("climate").per_page(10))
        .await?;
    for hit in page.results.iter().take(3) {
        let title = hit.title.as_deref().unwrap_or("(untitled)");
        let slug = hit.slug.as_deref().unwrap_or("(no-slug)");
        println!("• {title} ({slug})");
    }

    if let Some(after) = page.after {
        let _next = client
            .search(SearchParams::new().q("climate").per_page(10).after(after))
            .await?;
    }

    Ok(())
}
```

### Generic CKAN client (`data-gov-ckan`)

For portals that still expose the CKAN Action API:

```toml
[dependencies]
data-gov-ckan = "0.4"
```

```rust
use data_gov_ckan::{CkanClient, Configuration};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Configuration {
        base_path: "https://your-ckan-host.example/api/3".to_string(),
        ..Configuration::default()
    };
    let client = CkanClient::new(Arc::new(config));
    let results = client.package_search(Some("climate"), Some(10), Some(0), None).await?;
    println!("Found {} datasets", results.count.unwrap_or(0));
    Ok(())
}
```

## Development

```bash
git clone https://github.com/dspadea/data-gov-rs.git
cd data-gov-rs

cargo build         # compile all crates
cargo test          # run unit & integration tests (excluding live-network tests)
cargo run -p data-gov --example demo
```

Live-network integration tests are marked `#[ignore]`. Run them explicitly:

```bash
cargo test -p data-gov-catalog --test integration_tests -- --ignored
```

Workspace layout:

```
data-gov-rs/
├── .vscode/                # VSCode config files for MCP setup in this workspace
├── data-gov-catalog/       # Catalog API client (current data.gov backend)
├── data-gov-ckan/          # Generic CKAN Action API client (non-data.gov portals)
├── data-gov-mcp-server/    # MCP server for AI integration
├── data-gov/               # High-level client + CLI
├── examples/               # Shell automation samples
└── Cargo.toml              # Workspace manifest
```

## Disclaimer & license

This is an independent project and is not affiliated with data.gov or any government agency. For authoritative information, refer to the official [data.gov](https://www.data.gov/) portal.

Licensed under the [Apache License 2.0](LICENSE).
