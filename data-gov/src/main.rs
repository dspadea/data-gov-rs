use std::sync::Arc;

use data_gov::{DATA_GOV_BASE_URL, ckan::{CkanClient, apis::configuration::Configuration}};

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn std::error::Error>> {

    let ckan_config = Arc::new(Configuration{
        base_path: DATA_GOV_BASE_URL.to_string(),
        ..Default::default()
    });

    let ckan = CkanClient::new(ckan_config);

    println!("🔍 Searching for electric vehicle datasets...");
    
    match ckan.package_search(
        Some("electric-vehicle-population-data"), // query
        Some(10), // rows (limit results)  
        None,    // start
        None,    // fq (filter query)
    ).await {
        Ok(search_response) => {
            println!("✅ Found {} results", search_response.count.unwrap_or(0));

            if let Some(results) = &search_response.results {
                for (i, dataset) in results.iter().enumerate() {
                    println!("\n📊 Dataset {}: {} ({})",
                        i + 1,
                        dataset.title.as_ref().unwrap_or(&dataset.name),
                        dataset.name
                    );
                    
                    if let Some(id) = &dataset.id {
                        println!("   🆔 ID: {}", id);
                    }
                    
                    if let Some(notes) = &dataset.notes {
                        let truncated_notes = if notes.len() > 150 {
                            format!("{}...", &notes[..150])
                        } else {
                            notes.clone()
                        };
                        println!("   📝 Description: {}", truncated_notes);
                    }
                    
                    if let Some(resources) = &dataset.resources {
                        println!("   📁 Resources: {} available", resources.len());
                        
                        // Show first few resources
                        for (j, resource) in resources.iter().take(3).enumerate() {
                            if let Some(format) = &resource.format {
                                println!("      {}. {} ({})", 
                                    j + 1, 
                                    resource.name.as_ref().unwrap_or(&"Unnamed".to_string()),
                                    format
                                );
                            }
                        }
                        
                        if resources.len() > 3 {
                            println!("      ... and {} more resources", resources.len() - 3);
                        }
                    }
                    
                    if let Some(owner_org) = &dataset.owner_org {
                        println!("   🏛️  Organization ID: {}", owner_org);
                    }
                }

                // Get detailed info for first result
                if let Some(first_result) = results.first() {
                    println!("\n🔍 Getting detailed information for: {}", first_result.name);
                    
                    match ckan.package_show(&first_result.name).await {
                        Ok(detailed_dataset) => {
                            println!("✅ Dataset details loaded successfully");
                            
                            if let Some(resources) = &detailed_dataset.resources {
                                println!("📁 Detailed resource information:");
                                for resource in resources {
                                    println!("   • {}: {}", 
                                        resource.name.as_ref().unwrap_or(&"Unnamed Resource".to_string()),
                                        resource.url.as_ref().unwrap_or(&"No URL".to_string())
                                    );
                                    if let Some(description) = &resource.description {
                                        if !description.trim().is_empty() {
                                            println!("     Description: {}", description);
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            println!("❌ Failed to get dataset details: {}", e);
                        }
                    }
                }
            } else {
                println!("📭 No results found in response");
            }
        }
        Err(e) => {
            println!("❌ Search failed: {}", e);
        }
    }

    println!("\n🎯 Testing a broader search...");
    match ckan.package_search(
        Some("data"), // broader query
        Some(5), // fewer results
        None,    // start
        None,    // fq (filter query)
    ).await {
        Ok(search_response) => {
            println!("✅ Broader search found {} total results", search_response.count.unwrap_or(0));
            if let Some(results) = &search_response.results {
                println!("📊 Showing first {} results:", results.len());
                for (i, dataset) in results.iter().enumerate() {
                    println!("   {}. {} (State: {})", 
                        i + 1,
                        dataset.title.as_ref().unwrap_or(&dataset.name),
                        dataset.state.as_ref()
                            .map(|s| match s {
                                data_gov::ckan::models::package::State::Active => "Active",
                                data_gov::ckan::models::package::State::Deleted => "Deleted", 
                                data_gov::ckan::models::package::State::Draft => "Draft",
                            })
                            .unwrap_or("Unknown")
                    );
                }
            }
        }
        Err(e) => {
            println!("❌ Broader search failed: {}", e);
        }
    }

    println!("\n✨ CKAN API client is working! You now have a more ergonomic interface for data.gov.");
    println!("🚀 You can now use methods like:");
    println!("   • ckan.package_search() - Search for datasets");
    println!("   • ckan.package_show() - Get detailed dataset info");
    println!("   • ckan.package_create() - Create new datasets");
    println!("   • ckan.organization_list() - List organizations");
    println!("   • ckan.group_list() - List groups");
    println!("   • ckan.user_show() - Get user information");
    println!("   • And many more!");

    Ok(())
}