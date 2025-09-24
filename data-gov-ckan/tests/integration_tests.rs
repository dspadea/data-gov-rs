//! Comprehensive integration tests for the CKAN client against the real data.gov API.
//! 
//! These tests validate that our client works correctly with the live data.gov
//! CKAN API and provide examples of real-world usage patterns.

use data_gov_ckan::{CkanClient, CkanError, apis::configuration::Configuration};
use std::sync::Arc;

/// Helper function to create a test client configured for data.gov
fn create_test_client() -> CkanClient {
    let config = Arc::new(Configuration {
        base_path: "https://catalog.data.gov/api/3".to_string(),
        user_agent: Some("data-gov-ckan-client-test/1.0".to_string()),
        client: reqwest::Client::new(),
        basic_auth: None,
        oauth_access_token: None,
        bearer_access_token: None,
        api_key: None,
    });
    
    CkanClient::new(config)
}

#[tokio::test]
async fn test_basic_search() {
    let client = create_test_client();
    
    // Search for datasets containing "climate"
    let result = client.package_search(
        Some("climate"), // query
        Some(5),         // limit to 5 results
        Some(0),         // start at beginning
        None,            // no additional filters
    ).await.expect("Search should succeed");
    
    // Verify the response structure
    assert!(result.count.unwrap_or(0) > 0, "Should find climate-related datasets");
    
    if let Some(ref results) = result.results {
        assert!(results.len() <= 5, "Should respect row limit");
        
        // Check first result has expected fields
        if let Some(first_dataset) = results.first() {
            assert!(first_dataset.title.is_some() || !first_dataset.name.is_empty(), 
                    "Dataset should have title or name");
        }
    }
}

#[tokio::test]
async fn test_filtered_search() {
    let client = create_test_client();
    
    // Search with organization filter
    let result = client.package_search(
        Some("data"),
        Some(3),
        Some(0),
        Some("res_format:CSV"), // Only CSV resources
    ).await.expect("Filtered search should succeed");
    
    assert!(result.count.unwrap_or(0) > 0, "Should find CSV datasets");
}

#[tokio::test]
async fn test_pagination() {
    let client = create_test_client();
    
    // Get first page
    let first_page = client.package_search(
        Some("government"),
        Some(2),
        Some(0),
        None,
    ).await.expect("First page should succeed");
    
    // Get second page  
    let second_page = client.package_search(
        Some("government"),
        Some(2),
        Some(2),
        None,
    ).await.expect("Second page should succeed");
    
    // Verify pagination works
    if let (Some(first_results), Some(second_results)) = (&first_page.results, &second_page.results) {
        assert!(first_results.len() <= 2, "First page should have ≤2 results");
        assert!(second_results.len() > 0, "Second page should have results");
        
        // Results should be different (if we have enough total results)
        if first_results.len() > 0 && second_results.len() > 0 {
            assert_ne!(first_results[0].id, second_results[0].id, 
                       "Different pages should have different results");
        }
    }
}

#[tokio::test] 
async fn test_package_show() {
    let client = create_test_client();
    
    // First, find a dataset to show
    let search = client.package_search(Some("energy"), Some(1), Some(0), None)
        .await.expect("Search should succeed");
    
    if let Some(results) = search.results {
        if let Some(first_result) = results.first() {
            if let Some(ref dataset_id) = first_result.id {
                // Now get the full dataset details
                let package = client.package_show(&dataset_id.to_string())
                    .await.expect("Package show should succeed");
                
                // Verify we got a valid package with basic fields
                assert!(!package.name.is_empty(), "Package should have a name");
                assert!(package.title.is_some(), "Package should have a title");
            }
        }
    }
}

#[tokio::test]
async fn test_package_show_by_name() {
    let client = create_test_client();
    
    // Try to get a well-known dataset by name (this may fail if the dataset doesn't exist)
    match client.package_show("federal-student-aid-data-center").await {
        Ok(package) => {
            // Verify we got a valid package
            assert!(!package.name.is_empty(), "Package should have a name");
        },
        Err(CkanError::ApiError { status: 404, .. }) => {
            // Dataset not found is acceptable for this test
            println!("Test dataset not found - this is expected");
        },
        Err(e) => {
            panic!("Unexpected error: {}", e);
        }
    }
}

#[tokio::test]
async fn test_organization_list() {
    let client = create_test_client();
    
    let result = client.organization_list(
        Some("name"),  // sort by name  
        Some(10),      // limit to 10
        None,          // no offset
    ).await.expect("Organization list should succeed");
    
    assert!(!result.is_empty(), "Should have organizations");
    assert!(result.len() <= 10, "Should respect limit");
}

#[tokio::test]
async fn test_group_list() {
    let client = create_test_client();
    
    let result = client.group_list(
        Some("name"),  // sort by name
        Some(5),       // limit to 5 
        None,          // no offset
    ).await.expect("Group list should succeed");
    
    assert!(result.len() <= 5, "Should respect limit");
}

#[tokio::test]
async fn test_dataset_autocomplete() {
    let client = create_test_client();
    
    let result = client.dataset_autocomplete(
        Some("energy"), // search for datasets with "energy"
        Some(5),        // limit to 5 suggestions
    ).await.expect("Dataset autocomplete should succeed");
    
    // Check that we got some results  
    println!("Found {} dataset suggestions", result.len());
    
    // Verify all results contain "energy" in name or title
    for dataset in &result {
        let name = dataset.name.as_deref().unwrap_or("");
        let title = dataset.title.as_deref().unwrap_or("");
        let contains_energy = name.to_lowercase().contains("energy") || 
                             title.to_lowercase().contains("energy");
        assert!(contains_energy, "Dataset '{}' or title '{}' should contain 'energy'", name, title);
    }
}

#[tokio::test]
async fn test_tag_autocomplete() {
    let client = create_test_client();
    
    let result = client.tag_autocomplete(
        Some("health"), // search for tags with "health"
        Some(3),        // limit to 3 suggestions
        None,           // no vocabulary filter
    ).await.expect("Tag autocomplete should succeed");
    
    println!("Found {} tag suggestions", result.len());
    
    // Verify all results contain "health"
    for tag in &result {
        assert!(tag.to_lowercase().contains("health"), "Tag '{}' should contain 'health'", tag);
    }
}

#[tokio::test]
async fn test_user_autocomplete() {
    let client = create_test_client();
    
    // User autocomplete likely requires authentication on data.gov
    match client.user_autocomplete(
        Some("admin"),  // search for users with "admin" 
        Some(2),        // limit to 2
        None,           // don't ignore self (not applicable for anonymous)
    ).await {
        Ok(result) => {
            println!("Found {} user suggestions", result.len());
        },
        Err(CkanError::ApiError { status: 403, .. }) => {
            // 403 Forbidden is expected for user_autocomplete without authentication
            println!("User autocomplete requires authentication (403 Forbidden) - this is expected");
        },
        Err(e) => panic!("Unexpected error: {:?}", e),
    }
}

#[tokio::test] 
async fn test_group_autocomplete() {
    let client = create_test_client();
    
    let result = client.group_autocomplete(
        Some("science"), // search for groups with "science"
        Some(3),         // limit to 3
    ).await.expect("Group autocomplete should succeed");
    
    // Check results
    for group in result {
        assert!(group.name.as_ref().map_or(false, |n| !n.is_empty()), "Group should have a name");
    }
}

#[tokio::test]
async fn test_organization_autocomplete() {
    let client = create_test_client();
    
    let result = client.organization_autocomplete(
        Some("department"), // search for orgs with "department"
        Some(4),            // limit to 4
    ).await.expect("Organization autocomplete should succeed");
    
    // Check results 
    for org in result {
        assert!(org.name.as_ref().map_or(false, |n| !n.is_empty()), "Organization should have a name");
    }
}

#[tokio::test]
async fn test_resource_format_autocomplete() {
    let client = create_test_client();
    
    let result = client.resource_format_autocomplete(
        Some("csv"), // search for formats with "csv"
        Some(3),     // limit to 3
    ).await.expect("Resource format autocomplete should succeed");
    
    println!("Found {} format suggestions", result.len());
    
    // Verify all results contain "csv"
    for format in &result {
        assert!(format.to_lowercase().contains("csv"), "Format '{}' should contain 'csv'", format);
    }
}

#[tokio::test]
async fn test_error_handling() {
    let client = create_test_client();
    
    // Try to get a dataset that definitely doesn't exist
    match client.package_show("this-dataset-definitely-does-not-exist-12345").await {
        Ok(_) => panic!("Should have returned an error for non-existent dataset"),
        Err(CkanError::ApiError { status, .. }) => {
            assert_eq!(status, 404, "Should return 404 for non-existent dataset");
        },
        Err(e) => {
            // Other errors are also acceptable (e.g., network errors)
            println!("Got expected error: {}", e);
        }
    }
}

#[tokio::test]
async fn test_large_result_handling() {
    let client = create_test_client();
    
    // Test with a large limit (but not too large to avoid timeouts)
    let result = client.package_search(
        Some("data"), // broad search term
        Some(100),    // larger limit
        Some(0), 
        None,
    ).await.expect("Large result search should succeed");
    
    if let Some(count) = result.count {
        assert!(count >= 0, "Count should be non-negative");
    }
    
    if let Some(results) = result.results {
        assert!(results.len() <= 100, "Server should limit results appropriately");
    }
}

#[tokio::test] 
async fn test_concurrent_requests() {
    let client = create_test_client();
    
    // Make several concurrent requests
    let futures = vec![
        client.package_search(Some("energy"), Some(5), Some(0), None),
        client.package_search(Some("health"), Some(5), Some(0), None),
        client.package_search(Some("education"), Some(5), Some(0), None),
    ];
    
    // Wait for all to complete
    let results = futures::future::try_join_all(futures).await
        .expect("All concurrent requests should succeed");
    
    // All should have succeeded
    assert_eq!(results.len(), 3, "All requests should complete");
    
    for result in results {
        // Each result should have some basic validity
        assert!(result.count.is_some(), "Each result should have a count");
    }
}

#[cfg(test)]
mod performance_tests {
    use super::*;
    use std::time::Instant;
    
    #[tokio::test]
    async fn test_search_performance() {
        let client = create_test_client();
        
        let start = Instant::now();
        let _result = client.package_search(Some("data"), Some(10), Some(0), None)
            .await.expect("Search should succeed");
        let duration = start.elapsed();
        
        // API should respond within reasonable time (10 seconds is generous)
        assert!(duration.as_secs() < 10, "Search should complete within 10 seconds");
        println!("Search took {:?}", duration);
    }
}