pub const DATA_GOV_BASE_URL: &str = "https://catalog.data.gov/api/3";

pub use data_gov_ckan as ckan;
pub mod config;

// pub trait CKANResponse: serde::de::DeserializeOwned {}

// // Extension trait for automatic conversion
// pub trait IntoCKANResponse {
//     fn into_ckan<T>(self) -> T
//     where
//         T: CKANResponse;
// }

// impl IntoCKANResponse for serde_json::Value {
//     fn into_ckan<T>(self) -> T
//     where
//         T: CKANResponse,
//     {
//         serde_json::from_value::<T>(self)
//             .expect("Failed to convert Value to target struct")
//     }
// }


// #[derive(serde::Deserialize, Debug)]
// pub struct PackageSearchResult {
//     pub help: String,
//     pub success: bool,
//     pub result: PackageSearchResultDetail,
// }

// impl CKANResponse for PackageSearchResult {}

// #[derive(serde::Deserialize, Debug)]
// pub struct PackageSearchResultDetail {
//     pub count: u32,
//     pub sort: Option<String>,
//     pub results: Vec<PackageSearchResultItem>,
//     // pub facets: Option<serde_json::Value>,
//     // pub search_facets: Option<serde_json::Value>,
// }

// #[derive(serde::Deserialize, Debug)]
// pub struct PackageSearchResultItem {
//     pub display_name: Option<String>,
//     pub id: String,
//     pub name: String,
//     pub state: String,
//     pub vocabulary_id: Option<String>,
// }

// impl PackageSearchResultItem {
    
//     // Metadata contains resource URLs and more
//     pub fn to_metadata_url(&self) -> String {
//         format!("https://catalog.data.gov/harvest/object/{}", self.id)
//     }
// }