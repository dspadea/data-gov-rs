# Data.gov Rust Client

A comprehensive Rust client library and interactive REPL for exploring and downloading data from [data.gov](https://data.gov), the U.S. government's open data portal.

## Features

- 🔍 **Search & Discovery**: Search for datasets with advanced filtering
- 📦 **Dataset Management**: Get detailed information about datasets and resources
- ⬇️ **File Downloads**: Download resources with progress tracking and concurrent downloads
- 🏛️ **Organization Browsing**: Explore government agencies and their data
- 🖥️ **Interactive REPL**: Command-line interface for exploring data.gov
- 🚀 **Async/Await**: Built on modern async Rust for high performance
- ⚡ **Progress Tracking**: Visual progress bars for downloads
- 🛡️ **Error Handling**: Comprehensive error types and handling

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
data-gov = { path = "../data-gov" }  # Will be published to crates.io
```

## Quick Start

### Library Usage

```rust
use data_gov::DataGovClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a client
    let client = DataGovClient::new()?;
    
    // Search for datasets
    let results = client.search("climate change", Some(10), None, None, None).await?;
    println!("Found {} datasets", results.count.unwrap_or(0));
    
    // Get detailed dataset information
    let dataset = client.get_dataset("consumer-complaint-database").await?;
    println!("Dataset: {}", dataset.title.unwrap_or(dataset.name.clone()));
    
    // Find downloadable resources
    let resources = DataGovClient::get_downloadable_resources(&dataset);
    println!("Found {} downloadable files", resources.len());
    
    // Download a resource
    if let Some(resource) = resources.first() {
        let path = client.download_resource(resource, None).await?;
        println!("Downloaded to: {:?}", path);
    }
    
    Ok(())
}
```

### Interactive REPL

Run the interactive REPL for exploring data.gov:

```bash
# Basic usage
cargo run --bin data-gov-repl

# With custom download directory
cargo run --bin data-gov-repl -- --download-dir ./my-downloads

# With API key for higher rate limits
cargo run --bin data-gov-repl -- --api-key YOUR_API_KEY
```

## REPL Commands

The interactive REPL provides these commands:

| Command | Description | Example |
|---------|-------------|---------|
| `search <query> [limit]` | Search for datasets | `search climate data 20` |
| `show <dataset_id>` | Show detailed dataset info | `show consumer-complaint-database` |
| `download <dataset_id> [index]` | Download dataset resources | `download my-dataset 0` |
| `list organizations` | List government agencies | `list orgs` |
| `setdir <path>` | Set download directory | `setdir ./downloads` |
| `info` | Show session information | `info` |
| `help` | Show help message | `help` |
| `quit` | Exit the REPL | `quit` |

### REPL Examples

```
🇺🇸 Data.gov Interactive Explorer
Type 'help' for available commands, 'quit' to exit

data.gov> search electric vehicles
Searching for 'electric vehicles'...

Found 42 results:

 1. electric-vehicle-population-data Electric Vehicle Population Data
   This dataset shows the Battery Electric Vehicles (BEVs) and Plug-in Hybrid Electric Vehicles...

 2. alternative-fuel-stations Alternative Fuel Stations
   Find alternative fuel stations in the United States...

data.gov> show electric-vehicle-population-data
Fetching dataset 'electric-vehicle-population-data'...

📦 Dataset Details
Name: electric-vehicle-population-data
Title: Electric Vehicle Population Data

Description: 
This dataset shows the Battery Electric Vehicles (BEVs) and Plug-in Hybrid Electric Vehicles (PHEVs) that are currently registered through Washington State Department of Licensing (DOL).

License: Public Domain

📁 1 downloadable resources:
  0. Electric_Vehicle_Population_Data [CSV] 

💡 Use 'download electric-vehicle-population-data' to download all resources
💡 Use 'download electric-vehicle-population-data 0' to download a specific resource

data.gov> download electric-vehicle-population-data 0
Fetching dataset 'electric-vehicle-population-data'...
Downloading resource 0...
Downloading Electric_Vehicle_Population_Data [████████████████████████████████████████] 2.1MB/2.1MB (3.2MB/s, 0s)
Success! Downloaded to: ./Electric_Vehicle_Population_Data.csv

data.gov> list organizations
Fetching organizations...

Government organizations:
 1. agency-for-international-development
 2. department-of-agriculture
 3. department-of-commerce
 4. department-of-defense
 5. department-of-education
```

## Configuration

### Basic Configuration

```rust
use data_gov::{DataGovClient, DataGovConfig};

let config = DataGovConfig::new()
    .with_download_dir("./my-downloads")
    .with_api_key("your-api-key")
    .with_user_agent("my-app/1.0")
    .with_max_concurrent_downloads(5)
    .with_progress(true);

let client = DataGovClient::with_config(config)?;
```

### Available Configuration Options

- **Download Directory**: Where to save downloaded files
- **API Key**: For higher rate limits (optional)
- **User Agent**: Custom user agent string
- **Max Concurrent Downloads**: Number of simultaneous downloads
- **Progress Bars**: Enable/disable download progress display
- **Download Timeout**: Timeout for individual downloads

## API Reference

### DataGovClient

The main client for interacting with data.gov:

#### Search Methods

- `search(query, limit, offset, organization, format)` - Search for datasets
- `get_dataset(dataset_id)` - Get detailed dataset information
- `autocomplete_datasets(partial, limit)` - Get dataset name suggestions
- `autocomplete_organizations(partial, limit)` - Get organization suggestions

#### Organization Methods

- `list_organizations(limit)` - List government organizations

#### Resource Methods

- `get_downloadable_resources(package)` - Find downloadable files in a dataset
- `download_resource(resource, output_path)` - Download a single resource
- `download_resources(resources, output_dir)` - Download multiple resources concurrently

#### Utility Methods

- `validate_download_dir()` - Check if download directory is writable
- `download_dir()` - Get current download directory
- `ckan_client()` - Access underlying CKAN client

### Error Handling

The library uses a comprehensive error type:

```rust
use data_gov::{DataGovClient, DataGovError};

match client.search("test", None, None, None, None).await {
    Ok(results) => println!("Found {} results", results.count.unwrap_or(0)),
    Err(DataGovError::CkanError(e)) => println!("API error: {}", e),
    Err(DataGovError::HttpError(e)) => println!("Network error: {}", e),
    Err(DataGovError::DownloadError { message }) => println!("Download failed: {}", message),
    Err(e) => println!("Other error: {}", e),
}
```

## Architecture

This crate is built on top of the `data-gov-ckan` crate, which provides low-level CKAN API access. The `data-gov` crate adds:

- **Higher-level abstractions** for common workflows
- **File download capabilities** with progress tracking
- **Concurrent download management**
- **Interactive REPL** for exploration
- **Rich error handling** and validation
- **Configuration management**

## Examples

See the [`examples/`](examples/) directory for more examples:

- `demo.rs` - Basic API usage demonstration

## Contributing

1. Fork the repository
2. Create a feature branch
3. Add tests for new functionality
4. Ensure all tests pass
5. Submit a pull request

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Related Projects

- [`data-gov-ckan`](../data-gov-ckan/) - Low-level CKAN API client
- [CKAN](https://ckan.org/) - The open source data management system powering data.gov

## Acknowledgments

- Built for the U.S. government's [data.gov](https://data.gov) platform
- Uses the CKAN API for data access
- Inspired by the need for better programmatic access to government data