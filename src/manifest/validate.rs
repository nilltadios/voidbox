//! Manifest validation

use super::ManifestError;
use super::schema::AppManifest;

/// Validate a manifest for completeness and correctness
pub fn validate_manifest(manifest: &AppManifest) -> Result<(), ManifestError> {
    // Check required fields
    if manifest.app.name.is_empty() {
        return Err(ManifestError::ValidationError(
            "app.name is required".into(),
        ));
    }

    if manifest.app.display_name.is_empty() {
        return Err(ManifestError::ValidationError(
            "app.display_name is required".into(),
        ));
    }

    if manifest.binary.name.is_empty() {
        return Err(ManifestError::ValidationError(
            "binary.name is required".into(),
        ));
    }

    // Validate app name (lowercase, alphanumeric, hyphens only)
    if !manifest
        .app
        .name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(ManifestError::ValidationError(
            "app.name must be lowercase alphanumeric with hyphens only".into(),
        ));
    }

    Ok(())
}
