# data-gov-rs

A collection of Rust crates for working with US government open data, with a focus on data.gov and CKAN-powered data portals.

[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)

## Overview

This workspace provides Rust libraries for accessing government open data programmatically. The primary focus is on data.gov, the US government's open data portal, which runs on CKAN (Comprehensive Knowledge Archive Network).

**Current crates:**
- [`data-gov-ckan`](./data-gov-ckan/) - Low-level CKAN API client for data.gov and other CKAN instances  
- [`data-gov`](./data-gov/) - High-level client library and CLI tool for exploring and downloading data

## Getting Started

### Command Line Tool

Install and use the `data-gov` CLI:

```bash
# Install from source
cd data-gov && cargo install --path .

# Search for datasets
data-gov search "climate change" 10

# Get dataset details
data-gov show consumer-complaint-database

# Download resources
data-gov download consumer-complaint-database 0

# Interactive mode
data-gov

# Use in scripts with shebang
echo '#!/usr/bin/env data-gov\nsearch climate 5\nquit' > script.sh
chmod +x script.sh && ./script.sh
```

#### TTY-Aware Behavior

The CLI automatically adapts to different environments:

**Colors:**
- **Interactive terminals**: Full color output for better readability
- **Piped/redirected output**: Plain text without ANSI codes
- **Control options**: `--color=auto|always|never`
- **NO_COLOR**: Respects the `NO_COLOR` environment variable

**Progress Indicators:**
- **Interactive terminals**: Animated progress bars with download speed and ETA
- **Non-interactive**: Simple text progress messages
- **Control options**: `NO_PROGRESS=1` to disable all progress indication
- **Force simple**: `FORCE_SIMPLE_PROGRESS=1` for basic text even in terminals

```bash
# Examples:
data-gov search "climate" 10                    # Colors in terminal
data-gov search "climate" 10 > results.txt      # No colors in file
NO_COLOR=1 data-gov search "climate" 10         # Force no colors
data-gov --color=always search "climate" 10     # Force colors

# Progress bars during downloads:
data-gov download dataset-id                    # Fancy progress in terminal  
data-gov download dataset-id > log.txt          # Simple text progress
NO_PROGRESS=1 data-gov download dataset-id      # No progress indication
```

### Library Usage

Add the CKAN client to your project:

```toml
[dependencies]
data-gov-ckan = "0.1.0"
tokio = { version = "1.0", features = ["full"] }
```

Search for datasets on data.gov:

```rust
use data_gov_ckan::CkanClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a client for data.gov
    let client = CkanClient::new_data_gov(None)?;
    
    // Search for climate datasets
    let results = client.package_search(Some("climate"), Some(10), Some(0), None).await?;
    
    println!("Found {} climate-related datasets", results.count);
    
    // Print first few results
    if let Some(datasets) = results.results {
        for dataset in datasets.iter().take(3) {
            println!("• {}", dataset.title.as_deref().unwrap_or(&dataset.name));
        }
    }
    
    Ok(())
}
```

Get details about a specific dataset:

```rust
// Get detailed information about a dataset
let dataset = client.package_show("consumer-complaint-database").await?;
println!("Dataset: {}", dataset.title.as_deref().unwrap_or("Untitled"));
println!("Resources: {}", dataset.resources.as_ref().map_or(0, |r| r.len()));
```

## What You Can Do

- **Search datasets** across all of data.gov with powerful filtering
- **Access metadata** for any dataset including descriptions, tags, and organization info
- **List organizations** and groups that publish data
- **Get resource information** including download URLs and formats
- **Use autocomplete** to discover datasets, organizations, and tags
- **Work with any CKAN instance** (not just data.gov)

## Use Cases

This library is useful for:

- **Data science projects** that need government datasets
- **Research applications** requiring reproducible data access
- **Civic technology** projects using government open data
- **Data engineering** pipelines consuming government data
- **Academic research** with government data sources
- **Government agencies** building tools on CKAN

## Crates

### data-gov-ckan

[![Crates.io](https://img.shields.io/crates/v/data-gov-ckan)](https://crates.io/crates/data-gov-ckan)
[![Documentation](https://docs.rs/data-gov-ckan/badge.svg)](https://docs.rs/data-gov-ckan)

A comprehensive, type-safe CKAN API client optimized for data.gov.

**Key features:**
- Complete API coverage (search, datasets, organizations, users, etc.)
- Built for async/await with tokio and reqwest
- Type-safe with full Rust struct definitions
- Authentication support (API keys, basic auth)
- Comprehensive error handling
- Extensive test coverage including real API integration tests

[**→ Full documentation**](./data-gov-ckan/README.md)

### data-gov

A high-level client library and command-line tool built on top of `data-gov-ckan`.

**Key features:**
- Interactive REPL for exploring data.gov
- CLI commands for scripting and automation  
- File download with progress tracking and concurrent downloads
- Higher-level search and discovery APIs
- Configuration management for download directories and API keys

**CLI Usage:**
```bash
data-gov search "energy efficiency" 5
data-gov show electric-vehicle-population-data
data-gov download my-dataset 0
```

**Library Usage:**
```rust
use data_gov::DataGovClient;

let client = DataGovClient::new()?;
let results = client.search("climate", Some(10), None, None, None).await?;
```

[**→ Full documentation**](./data-gov/README.md)

## Development

This is a Cargo workspace. To work with it:

```bash
# Clone the repository
git clone https://github.com/dspadea/data-gov-rs.git
cd data-gov-rs

# Build all crates
cargo build

# Run tests
cargo test

# Run examples
cargo run -p data-gov-ckan --example debug_search
```

### Project Structure

```
data-gov-rs/
├── data-gov-ckan/           # CKAN API client
│   ├── src/                 # Source code
│   ├── tests/               # Tests
│   ├── examples/            # Usage examples
│   └── README.md            # Detailed CKAN client docs
├── data-gov/                # High-level client and CLI tool  
├── Cargo.toml               # Workspace configuration
└── README.md                # This file
```

## Disclaimer

**Independent Project**: This is a personal hobby project and I am not affiliated with, employed by, or paid by data.gov, the US government, or any government agency in any way. While I'd certainly appreciate it if they wanted to throw some of my tax dollars back my way, this project is developed independently with best-effort support.

This project is provided as-is for educational and research purposes. For official government data access, please refer to the official [data.gov](https://www.data.gov/) website and APIs.

## Contributing

Contributions are welcome! Please:

1. Fork the repository
2. Create a feature branch
3. Add tests for new functionality
4. Ensure all tests pass with `cargo test`
5. Submit a pull request

## License

This project is licensed under the Apache 2.0 License - see the [LICENSE](LICENSE) file for details.

## Resources

- [data.gov](https://www.data.gov/) - US Government's open data portal
- [CKAN Documentation](https://docs.ckan.org/) - CKAN API documentation
- [data.gov Developer Hub](https://www.data.gov/developers/) - API guides and resources