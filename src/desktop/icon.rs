//! Icon extraction and management

use crate::storage::paths;
use std::fs;
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
pub fn extract_icon(app_name: &str, icon_filename: Option<&str>) -> Result<(), IconError> {
    let app_rootfs = paths::app_rootfs_dir(app_name);
    let icon_dest = paths::app_icon_path(app_name);

    if let Some(parent) = icon_dest.parent() {
        fs::create_dir_all(parent)?;
    }

    // If specific filename provided, look for it
    if let Some(filename) = icon_filename {
        for entry in WalkDir::new(&app_rootfs).max_depth(5) {
            if let Ok(entry) = entry {
                if entry.file_name().to_string_lossy() == filename {
                    fs::copy(entry.path(), &icon_dest)?;
                    return Ok(());
                }
            }
        }
    }

    // Otherwise search for common icon patterns
    let patterns = [
        format!("{}.png", app_name),
        format!("{}.svg", app_name),
        "icon.png".to_string(),
        "icon.svg".to_string(),
        "logo.png".to_string(),
        "product_logo_128.png".to_string(),
        "app.png".to_string(),
    ];

    for entry in WalkDir::new(&app_rootfs).max_depth(5) {
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
