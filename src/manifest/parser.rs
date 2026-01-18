//! Manifest parsing functions

use super::schema::AppManifest;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ManifestError {
    #[error("Failed to read manifest file: {0}")]
    ReadError(#[from] std::io::Error),

    #[error("Failed to parse manifest TOML: {0}")]
    ParseError(#[from] toml::de::Error),

    #[error("Invalid manifest: {0}")]
    ValidationError(String),

    #[error("Manifest not found: {0}")]
    NotFound(String),
}

/// Parse a manifest from a TOML file
pub fn parse_manifest_file(path: &Path) -> Result<AppManifest, ManifestError> {
    let content = std::fs::read_to_string(path)?;
    parse_manifest_str(&content)
}

/// Parse a manifest from a TOML string
pub fn parse_manifest_str(content: &str) -> Result<AppManifest, ManifestError> {
    let manifest: AppManifest = toml::from_str(content)?;
    Ok(manifest)
}

/// Parse a manifest from a TOML string (alias for convenience)
pub fn parse_manifest(content: &str) -> Result<AppManifest, ManifestError> {
    parse_manifest_str(content)
}

/// Parse a manifest from a URL
pub fn parse_manifest_url(url: &str) -> Result<AppManifest, ManifestError> {
    let mut resp = ureq::get(url)
        .header("User-Agent", crate::APP_NAME)
        .call()
        .map_err(|e| ManifestError::ValidationError(format!("HTTP error: {}", e)))?;

    let content = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| ManifestError::ValidationError(format!("Failed to read response: {}", e)))?;

    parse_manifest_str(&content)
}
