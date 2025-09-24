use serde::{Deserialize, Serialize};

/// Represents an extra key-value pair in CKAN datasets
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Extra {
    #[serde(rename = "key")]
    pub key: String,
    #[serde(rename = "value")]
    pub value: serde_json::Value,
}

impl Extra {
    pub fn new(key: String, value: serde_json::Value) -> Extra {
        Extra { key, value }
    }
}