# data-gov

High-level Rust client and CLI for [data.gov](https://data.gov). It wraps the low-level [`data-gov-ckan`](../data-gov-ckan/) crate with download helpers, an interactive REPL, and ergonomic configuration.

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
data-gov = "0.1.1"
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

- 🔍 Search data.gov with optional organization / format filters
- 📦 Retrieve dataset metadata and enumerate downloadable resources
- ⬇️ Download individual resources or entire datasets with progress bars
- 🏛️ List organisations and query autocomplete endpoints
- 🖥️ Interactive REPL with colour-aware output and shebang-friendly scripts

## Library quick start

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

## CLI overview

```
data-gov search "climate change" 5
data-gov show electric-vehicle-population-data
data-gov download electric-vehicle-population-data 0
data-gov list organizations
```

Key defaults:

- **Interactive mode:** `data-gov` launches a REPL that stores downloads under `~/Downloads/<dataset>/`
- **Non-interactive mode:** Commands run directly in your current directory (`./<dataset>/`)
- Override download location with `--download-dir`, toggle colours with `--color`, and silence progress bars via `NO_PROGRESS=1`

### Command reference

| Command | Purpose |
| ------- | ------- |
| `search <query> [limit]` | Full-text search with optional result cap |
| `show <dataset_id>` | Inspect dataset details and resources |
| `download <dataset_id> [index]` | Download all resources or a specific resource by index |
| `list organizations` | List publishing organisations |
| `setdir <path>` | Change the active download directory (REPL only) |
| `info` | Display current configuration |
| `help`, `quit` | Help and exit commands |

### Automation

The REPL accepts stdin, so shebang scripts work out of the box:

```bash
#!/usr/bin/env data-gov
# Simple automation example
search climate 3
download consumer-complaint-database 0
quit
```

See [`../examples/scripting`](../examples/scripting) for ready-made scripts such as `download-epa-climate.sh` and `list-orgs.sh`.

## Configuration

```rust
use data_gov::{DataGovClient, DataGovConfig, OperatingMode};

let config = DataGovConfig::new()
    .with_mode(OperatingMode::CommandLine)
    .with_download_dir("./data")
    .with_api_key("your-api-key")
    .with_max_concurrent_downloads(5)
    .with_progress(true);

let client = DataGovClient::with_config(config)?;
```

Configuration covers the underlying CKAN settings, download directory logic, concurrency, progress output, and colour preferences.

## Development

```bash
cd data-gov-rs
cargo test -p data-gov
cargo run -p data-gov --example demo
```

The crate re-exports `data-gov-ckan` as `data_gov::ckan`, making the lower-level client available when you need direct CKAN access.

## Contributing & license

- Fork, branch, add tests, run `cargo test`, open a PR
- Licensed under [Apache 2.0](../LICENSE)

> ⚠️ **AI-assisted code:** Significant portions of this crate were generated
> with AI tooling. While the library behaves well in ad-hoc testing, it still
> needs careful human review and polish before production use.