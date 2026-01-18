//! User permission overrides

use crate::manifest::PermissionConfig;
use crate::storage::paths;
use std::fs;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SettingsError {
    #[error("Failed to read settings: {0}")]
    ReadError(#[from] std::io::Error),

    #[error("Failed to parse settings: {0}")]
    ParseError(#[from] toml::de::Error),

    #[error("Failed to save settings: {0}")]
    SaveError(String),
}

/// Load user settings overrides for an app
pub fn load_overrides(app_name: &str) -> Result<Option<PermissionConfig>, SettingsError> {
    let settings_path = paths::app_settings_path(app_name);

    if !settings_path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(settings_path)?;
    let config: PermissionConfig = toml::from_str(&content)?;

    Ok(Some(config))
}

/// Save user settings overrides for an app
pub fn save_overrides(app_name: &str, settings: &PermissionConfig) -> Result<(), SettingsError> {
    let settings_path = paths::app_settings_path(app_name);

    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let content =
        toml::to_string_pretty(settings).map_err(|e| SettingsError::SaveError(e.to_string()))?;

    fs::write(settings_path, content)?;

    Ok(())
}

/// Remove settings overrides for an app
pub fn remove_overrides(app_name: &str) -> Result<(), SettingsError> {
    let settings_path = paths::app_settings_path(app_name);
    if settings_path.exists() {
        fs::remove_file(settings_path)?;
    }
    Ok(())
}
