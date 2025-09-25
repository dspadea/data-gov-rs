#![allow(unused_imports)]
#![allow(clippy::too_many_arguments)]

extern crate serde;
extern crate serde_json;
extern crate serde_repr;
extern crate url;

pub mod client;
pub mod models;

// Re-export the ergonomic client and configuration for easy access
pub use client::{ApiKey, BasicAuth, CkanClient, CkanError, Configuration};
