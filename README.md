# data-gov-rs

Rust tooling for exploring U.S. government open data. This workspace bundles two companion crates:

- [`data-gov-ckan`](./data-gov-ckan/) – an async, type-safe CKAN client suitable for any CKAN portal
- [`data-gov`](./data-gov/) – a higher-level client and CLI tailored to data.gov workflows

[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)

## Requirements

- Rust **1.85+** (the workspace uses the Rust 2024 edition)
- Cargo and git
- Optional: a data.gov API key for higher rate limits

```bash
rustup toolchain install stable
rustup default stable
rustc --version  # should be 1.85 or newer
```

## Install the CLI

```bash
git clone https://github.com/dspadea/data-gov-rs.git
cd data-gov-rs/data-gov
cargo install --path .
```

Common commands:

- `data-gov search "climate change" 10`
- `data-gov show consumer-complaint-database`
- `data-gov download consumer-complaint-database 0`
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

Add the crate via a git dependency until it is published on crates.io:

```toml
[dependencies]
data-gov = { git = "https://github.com/dspadea/data-gov-rs", package = "data-gov" }
tokio = { version = "1", features = ["full"] }
```

```rust
use data_gov::DataGovClient;

#[tokio::main]
async fn main() -> data_gov::Result<()> {
    let client = DataGovClient::new()?;
    let results = client.search("climate change", Some(10), None, None, None).await?;
    println!("Found {} datasets", results.count.unwrap_or(0));

    let dataset = client.get_dataset("consumer-complaint-database").await?;
    println!("Dataset: {}", dataset.title.as_deref().unwrap_or(&dataset.name));

    let resources = DataGovClient::get_downloadable_resources(&dataset);
    if let Some(resource) = resources.first() {
        let path = client.download_resource(resource, None).await?;
        println!("Downloaded to {path:?}");
    }

    Ok(())
}
```

### Low-level CKAN client (`data-gov-ckan`)

```toml
[dependencies]
data-gov-ckan = "0.1.0"
tokio = { version = "1", features = ["full"] }
```

```rust
use data_gov_ckan::{CkanClient, Configuration};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = CkanClient::new(Arc::new(Configuration::default()));
    let results = client.package_search(Some("climate"), Some(10), Some(0), None).await?;
    println!("Found {} datasets", results.count.unwrap_or(0));

    if let Some(datasets) = results.results {
        for dataset in datasets.iter().take(3) {
            let title = dataset.title.as_deref().unwrap_or(&dataset.name);
            println!("• {title}");
        }
    }

    Ok(())
}
```

## Development

```bash
git clone https://github.com/dspadea/data-gov-rs.git
cd data-gov-rs

cargo build         # compile all crates
cargo test          # run unit & integration tests
cargo run -p data-gov-ckan --example debug_search
cargo run -p data-gov --example demo
```

Workspace layout:

```
data-gov-rs/
├── data-gov-ckan/      # CKAN client crate
├── data-gov/           # High-level client + CLI
├── examples/           # Shell automation samples
└── Cargo.toml          # Workspace manifest
```

## Disclaimer & license

This is an independent project and is not affiliated with data.gov or any government agency. For authoritative information, refer to the official [data.gov](https://www.data.gov/) portal.

Licensed under the [Apache License 2.0](LICENSE).