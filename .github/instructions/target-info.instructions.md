---
applyTo: '**'
---

# data-gov-rs Project Context and AI Guidelines

This file provides essential context about data.gov, CKAN APIs, and this project's architecture that AI assistants should understand when generating code, reviewing changes, or answering questions.

## Project Overview

### What This Project Does
- **data-gov-rs** is a Rust workspace for interacting with US government open data APIs
- **Primary focus**: data.gov (the US government's open data portal) which runs on CKAN
- **Architecture**: Cargo workspace with multiple crates:
  - `data-gov-ckan`: Complete CKAN API client (main crate)
  - `data-gov`: Higher-level utilities (planned)

### Key Design Philosophy
- **Type safety over convenience**: Use strong Rust types for API responses
- **Async-first**: All network operations use async/await with tokio
- **Real-world tested**: Integration tests against actual data.gov APIs
- **Government-friendly**: Respects rate limits, handles authentication properly
- **Idiomatic Rust**: Follows Rust conventions, not just OpenAPI generation patterns

## CKAN Knowledge Base

### What is CKAN?
- **CKAN** (Comprehensive Knowledge Archive Network) is open-source data portal software
- **data.gov** runs on CKAN, as do many other government data portals worldwide
- **REST API** with JSON responses, following consistent patterns
- **Terminology**: "packages" = datasets, "resources" = files/data within datasets

### CKAN API Structure
```
Base URL: https://catalog.data.gov/api/3/action/
Authentication: Optional API key via header, URL param, or POST body
Response format: {"success": bool, "result": {...}, "help": "..."}
```

### Key CKAN Concepts for Code Generation

#### 1. **Packages (Datasets)**
- **package_search**: Search datasets with complex filtering
- **package_show**: Get detailed dataset information by ID/name
- **package_list**: List all dataset names (simple)
- Fields: `name` (ID), `title`, `notes` (description), `tags`, `resources`, `organization`

#### 2. **Resources (Files)**
- Files/data within a dataset
- Fields: `url`, `format`, `name`, `description`, `size`
- Common formats: CSV, JSON, XML, PDF, API endpoints

#### 3. **Organizations & Groups**  
- **Organizations**: Government agencies that publish data (e.g., "epa-gov", "gsa-gov")
- **Groups**: Thematic categorizations (e.g., "climate", "health")
- Both have: `name`, `title`, `description`, `image_url`

#### 4. **Search & Discovery**
- **Full-text search**: Query across titles, descriptions, tags
- **Faceted search**: Filter by organization, format, tags, etc.
- **Autocomplete**: For datasets, orgs, groups, tags, users
- **Pagination**: `rows` (limit) and `start` (offset)

### Common CKAN API Patterns

#### Authentication (All Optional)
```rust
// API Key (recommended for higher rate limits)
Configuration {
    api_key: Some(ApiKey { key: "xxx".to_string(), prefix: None }),
    // ...
}

// Basic Auth (some CKAN instances)
Configuration {
    basic_auth: Some(("username".to_string(), Some("password".to_string()))),
    // ...
}
```

#### Error Handling Patterns
```rust
// CKAN API responses always have this structure
{"success": false, "error": {"__type": "Not Found Error", "message": "..."}}

// Map to appropriate Rust error types
CkanError::NotFound { message, error_type }
CkanError::ValidationError { message, details }
CkanError::NetworkError(reqwest::Error)
```

#### Search Patterns
```rust
// Basic text search
client.package_search(Some("climate"), Some(10), Some(0), None).await?

// Advanced filtering with fq parameter  
let filter = json!({"organization": "epa-gov", "res_format": "CSV"});
client.package_search(Some("water"), Some(20), Some(0), Some(filter)).await?
```

## Data.gov Specifics

### Known Stable Datasets (for testing)
- `"consumer-complaint-database"`: CFPB complaints, regularly updated
- `"federal-employee-salaries"`: OPM salary data, annual updates
- `"lobbying-disclosure-contributions"`: Senate lobbying data

### Common Organizations
- `"gsa-gov"`: General Services Administration
- `"epa-gov"`: Environmental Protection Agency  
- `"omb-gov"`: Office of Management and Budget
- `"ed-gov"`: Department of Education

### API Endpoints Available on data.gov
```
✅ /api/3/action/package_search - Dataset search
✅ /api/3/action/package_show - Dataset details
✅ /api/3/action/organization_list - List agencies
✅ /api/3/action/group_list - List topic groups
✅ /api/util/dataset/autocomplete - Dataset name autocomplete
✅ /api/util/organization/autocomplete - Agency autocomplete
❌ User management endpoints (not public)
❌ Dataset creation/editing (requires auth + permissions)
```

### Rate Limiting & Best Practices
- No official rate limits published, but be respectful
- Use pagination rather than requesting everything at once
- Cache results when possible to reduce API calls
- Handle network timeouts gracefully (government servers can be slow)

## Code Generation Guidelines

### Rust Patterns to Follow

#### 1. **Error Handling**
```rust
// Always use Result types for fallible operations
pub async fn package_show(&self, id: &str) -> Result<Package, CkanError> {
    // Implementation with proper error mapping
}

// Provide rich error context
#[derive(Debug, thiserror::Error)]
pub enum CkanError {
    #[error("Dataset not found: {message}")]
    NotFound { message: String, error_type: String },
    // ...
}
```

#### 2. **Configuration Patterns**
```rust
// Provide sensible defaults for data.gov
impl Configuration {
    pub fn new_data_gov() -> Result<Configuration, CkanError> {
        Ok(Configuration {
            base_path: "https://catalog.data.gov/api/3".to_string(),
            user_agent: Some("data-gov-rs/3.0".to_string()),
            client: reqwest::Client::new(),
            // ... other defaults
        })
    }
}
```

#### 3. **API Method Patterns**
```rust
// Use Option<T> for optional parameters, not magic values
pub async fn package_search(
    &self,
    q: Option<&str>,           // Search query
    rows: Option<u32>,         // Number of results (default: 10)
    start: Option<u32>,        // Offset (default: 0)  
    fq: Option<serde_json::Value>, // Advanced filters
) -> Result<PackageSearchResult, CkanError>

// Provide convenience methods
pub async fn search_by_organization(&self, org: &str) -> Result<PackageSearchResult, CkanError> {
    let fq = json!({"organization": org});
    self.package_search(None, None, None, Some(fq)).await
}
```

#### 4. **Type Safety**
```rust
// Use strong types, not stringly-typed APIs
pub struct Package {
    pub name: String,              // Required - dataset ID
    pub title: Option<String>,     // Optional - human readable name
    pub notes: Option<String>,     // Optional - description
    pub tags: Option<Vec<Tag>>,    // Optional - list of tags
    pub resources: Option<Vec<Resource>>, // Optional - files/data
    // ... other fields with appropriate Option<T>
}

// Derive appropriate traits
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Tag {
    pub name: String,
    pub display_name: Option<String>,
}
```

### Testing Guidelines

#### Integration Test Patterns
```rust
#[tokio::test]
async fn test_package_search() -> Result<(), CkanError> {
    let client = CkanClient::new_data_gov(None)?;
    
    // Use stable, known datasets for testing
    let result = client.package_search(Some("climate"), Some(5), Some(0), None).await?;
    
    // Test structure, not specific content (data changes)
    assert!(result.count > 0);
    assert!(result.results.is_some());
    
    Ok(())
}

// Test error conditions
#[tokio::test]  
async fn test_package_not_found() {
    let client = CkanClient::new_data_gov(None).unwrap();
    let result = client.package_show("definitely-does-not-exist").await;
    
    match result {
        Err(CkanError::NotFound { .. }) => {}, // Expected
        other => panic!("Expected NotFound error, got: {:?}", other),
    }
}
```

### Documentation Patterns

#### Rustdoc Examples
```rust
/// Search for datasets on data.gov
///
/// # Arguments
/// * `q` - Search query string (searches title, description, tags)
/// * `rows` - Maximum number of results to return (default: 10, max: 1000)
/// * `start` - Number of results to skip for pagination (default: 0)
/// * `fq` - Advanced filter query as JSON object
///
/// # Examples
///
/// Basic search:
/// ```rust
/// # use data_gov_ckan::CkanClient;
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = CkanClient::new_data_gov(None)?;
/// let results = client.package_search(Some("climate"), Some(10), Some(0), None).await?;
/// println!("Found {} datasets", results.count);
/// # Ok(())
/// # }
/// ```
///
/// Search with filters:
/// ```rust,no_run
/// # use data_gov_ckan::CkanClient;
/// # use serde_json::json;
/// # #[tokio::main]  
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = CkanClient::new_data_gov(None)?;
/// let filter = json!({"organization": "epa-gov", "res_format": "CSV"});
/// let results = client.package_search(Some("water"), None, None, Some(filter)).await?;
/// # Ok(())
/// # }
/// ```
pub async fn package_search(/* ... */) -> Result<PackageSearchResult, CkanError>
```

### Common Gotchas to Avoid

1. **Don't assume all fields are present** - CKAN data is messy, use Option<T> liberally
2. **Don't hardcode URLs** - Allow configuration for different CKAN instances  
3. **Don't ignore rate limits** - Provide configurable delays between requests
4. **Don't log sensitive data** - API keys or PII that might be in responses
5. **Don't make breaking changes lightly** - Government developers need stability

### Performance Considerations

- **Reuse HTTP clients** - Don't create new reqwest::Client for each request
- **Use connection pooling** - reqwest does this automatically  
- **Implement reasonable timeouts** - Government servers can be slow
- **Cache when appropriate** - Organization lists don't change often
- **Paginate large results** - Don't fetch thousands of records at once

## When Adding New Features

### For New CKAN API Endpoints
1. Check CKAN documentation: https://docs.ckan.org/en/latest/api/
2. Test manually against data.gov first
3. Add appropriate Rust models in `models/` directory
4. Implement client method with proper error handling
5. Add unit and integration tests
6. Update documentation with real examples

### For Data.gov Specific Features  
1. Verify the feature works on catalog.data.gov
2. Consider how it works on other CKAN instances
3. Handle data.gov quirks gracefully
4. Test against known stable datasets
5. Document any data.gov-specific behavior

This context should guide all AI-generated code to be production-ready, idiomatic Rust that works well with real government data APIs.