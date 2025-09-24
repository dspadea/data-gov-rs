pub struct Config {
    pub ckan_base_url: String,
    pub api_key: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ckan_base_url: "https://catalog.data.gov".to_string(),
            api_key: None,
        }
    }
}
