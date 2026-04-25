# data-gov

High-level Rust client and CLI for [data.gov](https://data.gov). It wraps the low-level [`data-gov-catalog`](../data-gov-catalog/) crate with download helpers, an interactive REPL, and ergonomic configuration.

> **2026 migration note:** data.gov retired its CKAN Action API. This crate now
> targets the purpose-built
> [Catalog API](https://resources.data.gov/catalog-api/) via `data-gov-catalog`.
> The Catalog API uses cursor-based pagination, returns DCAT-US 3 metadata, and
> is publicly accessible (no API key).

## Requirements

- Rust **1.90+** (Rust 2024 edition)
- Cargo and git

```bash
rustup toolchain install stable
rustup default stable
```

## Add to your project

Use the published crate from crates.io:

```toml
[dependencies]
data-gov = "0.4"
tokio = { version = "1", features = ["full"] }
```

Working inside this repository? You can still use a path dependency in `Cargo.toml`:

```toml
data-gov = { path = "../data-gov" }
```

Need unreleased features between tags? Swap in the git dependency form instead:

```toml
data-gov = { git = "https://github.com/dspadea/data-gov-rs", package = "data-gov" }
```

### CLI install

```bash
git clone https://github.com/dspadea/data-gov-rs.git
cd data-gov-rs/data-gov
cargo install --path .
```

The `data-gov` binary is then available on your PATH.

## Highlights

- 🔍 Search data.gov with optional organization filter
- 📦 Retrieve DCAT-US 3 dataset metadata and enumerate downloadable distributions
- ⬇️ Download individual distributions or entire datasets with progress bars
- 🏛️ List organisations and suggest dataset titles
- 🖥️ Interactive REPL with colour-aware output and shebang-friendly scripts

## Library quick start

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

## CLI overview

```
data-gov search "climate change" 5
data-gov show electric-vehicle-population-data
data-gov download electric-vehicle-population-data 0                                 # Download by index
data-gov download electric-vehicle-population-data "Comma Separated Values File"    # Download by title (quoted)
data-gov download electric-vehicle-population-data csv                               # Partial title match
data-gov list organizations
```

Key defaults:

- **Interactive mode:** `data-gov` launches a REPL that stores downloads under `~/Downloads/<dataset>/`
- **Non-interactive mode:** Commands run directly in your current directory (`./<dataset>/`)
- Override download location with `--download-dir`, toggle colours with `--color`, and silence progress bars via `NO_PROGRESS=1`

### Command reference

| Command | Purpose |
| ------- | ------- |
| `search <query> [limit]` | Full-text search with optional page size |
| `show <dataset_slug>` | Inspect dataset details and distributions |
| `download <dataset_slug> [index\|title]` | Download all distributions, or specific ones by index or title substring |
| `list organizations` | List publishing organisations |
| `setdir <path>` | Change the active download directory (REPL only) |
| `info` | Display current configuration |
| `help`, `quit` | Help and exit commands |

### Automation

The REPL accepts stdin, so shebang scripts work out of the box:

```bash
#!/usr/bin/env data-gov
# Simple automation example
search "electric vehicle" 3
show electric-vehicle-population-data
download electric-vehicle-population-data "Comma Separated Values File"    # Download by title (quoted)
quit
```

See [`../examples/scripting`](../examples/scripting) for ready-made scripts such as `download-epa-climate.sh` and `list-orgs.sh`.

### Pagination

The Catalog API uses cursor-based pagination. The search response carries an
`after` field when more pages are available; pass it back on the next call to
advance:

```rust
let page1 = client.search("climate", Some(20), None, None).await?;
let page2 = client
    .search("climate", Some(20), page1.after.as_deref(), None)
    .await?;
```

There is no random-access offset — pages can only be walked forward in order.

### Advanced filters

Use [`data_gov::catalog::CatalogClient`](https://docs.rs/data-gov-catalog) and
[`SearchParams`](https://docs.rs/data-gov-catalog) directly for keyword,
spatial, or organization-type filters not exposed on the high-level `search`.

## Configuration

```rust
use data_gov::{DataGovClient, DataGovConfig, OperatingMode};

let config = DataGovConfig::new()
    .with_mode(OperatingMode::CommandLine)
    .with_download_dir("./data")
    .with_max_concurrent_downloads(5);

let client = DataGovClient::with_config(config)?;
```

Configuration covers the underlying Catalog API settings, download directory logic, concurrency, progress output, and colour preferences.

## Development

```bash
cd data-gov-rs
cargo test -p data-gov
cargo run -p data-gov --example demo
```

The crate re-exports `data-gov-catalog` as `data_gov::catalog`, making the lower-level client available when you need direct Catalog API access.

## Contributing & license

- Fork, branch, add tests, run `cargo test`, open a PR
- Licensed under [Apache 2.0](../LICENSE)


## Disclaimer & license

This is an independent project and is not affiliated with data.gov or any government agency. For authoritative information, refer to the official [data.gov](https://www.data.gov/) portal.

Licensed under the [Apache License 2.0](LICENSE).
