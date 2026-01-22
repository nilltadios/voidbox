//! Icon extraction and management

use crate::storage::paths;
use std::fs;
use std::path::Path;
use thiserror::Error;
use walkdir::WalkDir;

#[derive(Error, Debug)]
pub enum IconError {
    #[error("Failed to extract icon: {0}")]
    ExtractError(#[from] std::io::Error),

    #[error("Icon not found: {0}")]
    NotFound(String),
}

/// Extract icon from app installation directory
pub fn extract_icon(app_name: &str, icon_path: Option<&str>) -> Result<(), IconError> {
    let layer_dir = paths::app_layer_dir(app_name);
    let rootfs_dir = paths::app_rootfs_dir(app_name);
    let app_rootfs = if layer_dir.exists() { layer_dir } else { rootfs_dir };
    let icon_dest = paths::app_icon_path(app_name);

    if let Some(parent) = icon_dest.parent() {
        fs::create_dir_all(parent)?;
    }

    // If specific path provided, try it directly first
    if let Some(path) = icon_path {
        // Try as a relative path from rootfs
        let full_path = app_rootfs.join(path);
        if full_path.exists() {
            fs::copy(&full_path, &icon_dest)?;
            return Ok(());
        }

        // Try from /opt directory (common for extracted apps)
        let opt_path = app_rootfs.join("opt").join(app_name);
        if opt_path.exists() {
            for entry in WalkDir::new(&opt_path).max_depth(10) {
                if let Ok(entry) = entry {
                    if entry.path().ends_with(path) {
                        fs::copy(entry.path(), &icon_dest)?;
                        return Ok(());
                    }
                }
            }
        }

        // Search for the filename anywhere in rootfs (deep search)
        let filename = Path::new(path).file_name().unwrap_or_default();
        for entry in WalkDir::new(&app_rootfs).max_depth(12) {
            if let Ok(entry) = entry {
                if entry.file_name() == filename {
                    fs::copy(entry.path(), &icon_dest)?;
                    return Ok(());
                }
            }
        }
    }

    // Search for common icon patterns
    let patterns = [
        format!("{}.png", app_name),
        format!("{}.svg", app_name),
        "icon.png".to_string(),
        "icon.svg".to_string(),
        "logo.png".to_string(),
        "product_logo_128.png".to_string(),
        "app.png".to_string(),
        "code.png".to_string(), // VSCode
    ];

    for entry in WalkDir::new(&app_rootfs).max_depth(12) {
        if let Ok(entry) = entry {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            for pattern in &patterns {
                if name == pattern.to_lowercase() {
                    fs::copy(entry.path(), &icon_dest)?;
                    return Ok(());
                }
            }
        }
    }

    // No icon found - not an error, just use default
    Ok(())
}

/// Remove icon for an app
pub fn remove_icon(app_name: &str) -> Result<(), IconError> {
    let icon_path = paths::app_icon_path(app_name);
    if icon_path.exists() {
        fs::remove_file(icon_path)?;
    }
    Ok(())
}
