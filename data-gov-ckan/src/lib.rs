#![allow(unused_imports)]
#![allow(clippy::too_many_arguments)]

extern crate serde_repr;
extern crate serde;
extern crate serde_json;
extern crate url;

pub mod models;
pub mod client;

// Re-export the ergonomic client and configuration for easy access
pub use client::{CkanClient, CkanError, Configuration, ApiKey, BasicAuth};
