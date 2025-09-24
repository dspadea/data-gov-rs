# data-gov-ckan

A Rust library for interacting with the CKAN API, specifically designed for accessing data.gov and other CKAN-powered open data portals.

[![Crates.io](https://img.shields.io/crates/v/data-gov-ckan)](https://crates.io/crates/data-gov-ckan)
[![Documentation](https://docs.rs/data-gov-ckan/badge.svg)](https://docs.rs/data-gov-ckan)
[![License: AGPL v3](https://img.shields.io/badge/License-AGPL%20v3-blue.svg)](LICENSE)

## Features

- **Complete CKAN API Coverage**: Access datasets, organizations, groups, users, and more
- **Type-Safe**: Full Rust type system support with serde serialization
- **Async/Await**: Built on tokio and reqwest for efficient async operations  
- **Data.gov Optimized**: Specifically tested and optimized for the US government's data.gov portal
- **Error Handling**: Comprehensive error types with detailed error information
- **Flexible Authentication**: Support for API keys, basic auth, and unauthenticated access
- **Search & Discovery**: Powerful search functionality with filtering and pagination
- **Autocomplete**: Built-in support for dataset, organization, and tag autocomplete

## Quick Start

Add this to your `Cargo.toml`:

```toml
[dependencies]
data-gov-ckan = "3.0.0"
tokio = { version = "1.0", features = ["full"] }
```

### Basic Usage

```rust
use data_gov_ckan::{CkanClient, Configuration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a client for data.gov
    let client = CkanClient::new_data_gov(None)?;
    
    // Search for climate-related datasets
    let results = client.package_search(Some("climate"), Some(10), Some(0), None).await?;
    
    println!("Found {} datasets", results.count);
    
    if let Some(datasets) = results.results {
        for dataset in datasets.iter().take(3) {
            println!("- {} ({})", dataset.title.as_deref().unwrap_or("Untitled"), dataset.name);
        }
    }
    
    Ok(())
}
```

### With Custom Configuration

```rust
use data_gov_ckan::{CkanClient, Configuration, ApiKey};

#[tokio::main] 
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Configuration {
        base_path: "https://demo.ckan.org/api/3".to_string(),
        api_key: Some(ApiKey {
            prefix: None,
            key: "your-api-key".to_string(),
        }),
        ..Configuration::default()
    };
    
    let client = CkanClient::new(config)?;
    
    // Now you can make authenticated requests
    let dataset = client.package_show("example-dataset").await?;
    println!("Dataset: {}", dataset.title.as_deref().unwrap_or("Untitled"));
    
    Ok(())
}
```

### Search with Filters

```rust
use data_gov_ckan::{CkanClient, Configuration};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = CkanClient::new_data_gov(None)?;
    
    // Search with filters
    let filter_query = json!({
        "organization": "gsa-gov",
        "res_format": "CSV"
    });
    
    let results = client.package_search(
        Some("budget"),        // query
        Some(20),             // rows
        Some(0),              // start
        Some(filter_query)    // filter query
    ).await?;
    
    println!("Found {} CSV datasets from GSA about budget", results.count);
    
    Ok(())
}
```

## API Reference

### Core Client Methods

- **`package_search()`** - Search for datasets with filtering and pagination
- **`package_show()`** - Get detailed information about a specific dataset
- **`organization_list()`** - List all organizations
- **`group_list()`** - List all groups  
- **`tag_list()`** - List all tags

### Autocomplete Methods

- **`dataset_autocomplete()`** - Autocomplete dataset names
- **`organization_autocomplete()`** - Autocomplete organization names
- **`group_autocomplete()`** - Autocomplete group names
- **`tag_autocomplete()`** - Autocomplete tag names
- **`user_autocomplete()`** - Autocomplete user names

### Error Handling

The library provides detailed error information through the `CkanError` enum:

```rust
use data_gov_ckan::{CkanClient, CkanError};

match client.package_show("nonexistent").await {
    Ok(dataset) => println!("Found: {}", dataset.name),
    Err(CkanError::NotFound { message, .. }) => {
        println!("Dataset not found: {}", message);
    }
    Err(CkanError::NetworkError(e)) => {
        println!("Network error: {}", e);
    }
    Err(e) => {
        println!("Other error: {}", e);
    }
}
```

## Development

### Prerequisites

- Rust 1.70 or later
- Git

### Building

```bash
git clone https://github.com/your-username/data-gov-rs.git
cd data-gov-rs/data-gov-ckan
cargo build
```

### Running Tests

```bash
# Run unit tests
cargo test --lib

# Run integration tests (requires network access)
cargo test --test integration_tests

# Run all tests
cargo test
```

### Running Examples

```bash
# Basic search example
cargo run --example debug_search

# Raw response example
cargo run --example raw_response
```

### Code Generation

This library was initially generated from the CKAN OpenAPI specification but has been significantly refactored for better Rust idioms:

```bash
# Regenerate API models (if needed)
../codegen-api.sh
```

### Testing Against Real APIs

The integration tests run against the real data.gov API. To run them:

```bash
cargo test --test integration_tests -- --nocapture
```

Note: These tests may be slower and could fail if the API is unavailable.

## Authentication

### API Keys

For write operations or higher rate limits, you'll need an API key from your CKAN instance:

```rust
use data_gov_ckan::{Configuration, ApiKey};

let config = Configuration {
    api_key: Some(ApiKey {
        prefix: None,  // Some APIs require "Bearer" or other prefix
        key: "your-api-key-here".to_string(),
    }),
    ..Configuration::default()
};
```

### Basic Authentication

Some CKAN instances support basic authentication:

```rust
use data_gov_ckan::{Configuration, BasicAuth};

let config = Configuration {
    basic_auth: Some(("username".to_string(), Some("password".to_string()))),
    ..Configuration::default()
};
```

## Performance Tips

1. **Reuse Clients**: Create one client and reuse it for multiple requests
2. **Pagination**: Use pagination for large result sets rather than fetching everything at once
3. **Filtering**: Use specific filters to reduce the amount of data transferred
4. **Async**: Take advantage of the async API for concurrent operations

```rust
// Good: Reuse client for multiple requests
let client = CkanClient::new_data_gov(None)?;

let (search_results, orgs) = tokio::try_join!(
    client.package_search(Some("climate"), Some(10), Some(0), None),
    client.organization_list()
)?;
```

## License

This project is licensed under the AGPL v3.0 License - see the [LICENSE](../LICENSE) file for details.

## Acknowledgments

- Built on the CKAN API specification
- Uses [reqwest](https://github.com/seanmonstar/reqwest) for HTTP client functionality
- Data models generated from OpenAPI specification

## Changelog

### v3.0.0
- Major refactoring for idiomatic Rust code
- Moved configuration types to client module
- Simplified import paths
- Improved error handling
- Added comprehensive tests and documentation

### v2.x
- Generated OpenAPI client (deprecated)

## Support

For questions, issues, or contributions:

- [GitHub Issues](https://github.com/your-username/data-gov-rs/issues)
- [Documentation](https://docs.rs/data-gov-ckan)