use serde::{Deserialize, Serialize};

/// Represents an extra key-value pair in CKAN datasets
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Extra {
    /// Name of the extra field, chosen by whoever published the dataset.
    #[serde(rename = "key")]
    pub key: String,
    /// The value.
    ///
    /// Untyped because `extras` is CKAN's escape hatch for whatever a portal
    /// wants to attach: publishers send strings, numbers, booleans, and nested
    /// objects through the same field.
    #[serde(rename = "value")]
    pub value: serde_json::Value,
}

impl Extra {
    /// Create an [`Extra`] pairing `key` with `value`.
    pub fn new(key: String, value: serde_json::Value) -> Extra {
        Extra { key, value }
    }
}
