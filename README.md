# data-gov-rs

A collection of Rust crates for working with US government open data, with a focus on data.gov and CKAN-powered data portals.

[![License: AGPL v3](https://img.shields.io/badge/License-AGPL%20v3-blue.svg)](LICENSE)

## Overview

This workspace provides Rust libraries for accessing government open data programmatically. The primary focus is on data.gov, the US government's open data portal, which runs on CKAN (Comprehensive Knowledge Archive Network).

**Current crates:**
- [`data-gov-ckan`](./data-gov-ckan/) - CKAN API client for data.gov and other CKAN instances

**Planned crates:**
- `data-gov` - Higher-level utilities and cross-agency data tools

## Getting Started

Add the CKAN client to your project:

```toml
[dependencies]
data-gov-ckan = "3.0.0"
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

## Development

This is a Cargo workspace. To work with it:

```bash
# Clone the repository
git clone https://github.com/your-username/data-gov-rs.git
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
├── data-gov/                # Future: higher-level utilities  
├── Cargo.toml               # Workspace configuration
└── README.md                # This file
```

## Contributing

Contributions are welcome! Please:

1. Fork the repository
2. Create a feature branch
3. Add tests for new functionality
4. Ensure all tests pass with `cargo test`
5. Submit a pull request

## License

This project is licensed under the AGPL v3.0 License - see the [LICENSE](LICENSE) file for details.

## Resources

- [data.gov](https://www.data.gov/) - US Government's open data portal
- [CKAN Documentation](https://docs.ckan.org/) - CKAN API documentation
- [data.gov Developer Hub](https://www.data.gov/developers/) - API guides and resources