//! goro: the engine startup and request handling shared by the SAPIs (the
//! `goro` command line with its built-in `-S` web server, and goro-httpd).

pub mod engine;
pub mod mime_types;
pub mod websapi;

/// PHP_VERSION
pub const PHP_VERSION: &str = "8.5.4";
