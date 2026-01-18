//! Remove command implementation

use crate::desktop::{remove_app_wrapper, remove_desktop_entry, remove_icon};
use crate::manifest::InstalledApp;
use crate::settings::remove_overrides;
use crate::storage::paths;
use std::fs;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RemoveError {
    #[error("App not installed: {0}")]
    NotInstalled(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Remove failed: {0}")]
    Failed(String),
}

/// Remove an installed app
pub fn remove_app(app_name: &str, purge: bool) -> Result<(), RemoveError> {
    let app_dir = paths::app_dir(app_name);
    let manifest_path = paths::manifest_path(app_name);

    if !app_dir.exists() && !manifest_path.exists() {
        return Err(RemoveError::NotInstalled(app_name.to_string()));
    }

    println!("[voidbox] Removing {}...", app_name);

    // Remove desktop entry
    if let Err(e) = remove_desktop_entry(app_name) {
        println!("[voidbox] Warning: Could not remove desktop entry: {}", e);
    }

    // Remove wrapper script
    if let Err(e) = remove_app_wrapper(app_name) {
        println!("[voidbox] Warning: Could not remove wrapper script: {}", e);
    }

    // Remove icon
    if let Err(e) = remove_icon(app_name) {
        println!("[voidbox] Warning: Could not remove icon: {}", e);
    }

    // Remove manifest
    if manifest_path.exists() {
        fs::remove_file(&manifest_path)?;
    }

    // Remove settings
    if let Err(e) = remove_overrides(app_name) {
        println!("[voidbox] Warning: Could not remove settings: {}", e);
    }

    if purge {
        // Remove entire app directory (including data)
        if app_dir.exists() {
            println!("[voidbox] Removing app data (this may take a moment)...");
            fs::remove_dir_all(&app_dir)?;
        }
    } else {
        // Just remove rootfs but keep any app data
        let rootfs = paths::app_rootfs_dir(app_name);
        if rootfs.exists() {
            println!("[voidbox] Removing rootfs...");
            fs::remove_dir_all(&rootfs)?;
        }
        println!("[voidbox] Note: App data kept in {}", app_dir.display());
        println!("[voidbox] Use --purge to remove everything.");
    }

    // Update installed apps database
    remove_from_database(app_name)?;

    println!("[voidbox] {} removed successfully!", app_name);

    Ok(())
}

fn remove_from_database(app_name: &str) -> Result<(), RemoveError> {
    let db_path = paths::database_path();

    if !db_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&db_path)?;
    let mut apps: Vec<InstalledApp> = serde_json::from_str(&content)
        .map_err(|e| RemoveError::Failed(format!("Failed to parse database: {}", e)))?;

    apps.retain(|a| a.name != app_name);

    let content = serde_json::to_string_pretty(&apps)
        .map_err(|e| RemoveError::Failed(format!("Failed to serialize: {}", e)))?;
    fs::write(&db_path, content)?;

    Ok(())
}
