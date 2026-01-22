//! Remove command implementation

use crate::desktop::{remove_app_wrapper, remove_desktop_entry, remove_icon};
use crate::manifest::InstalledApp;
use crate::settings::remove_overrides;
use crate::storage::{paths, read_base_info_for_rootfs, remove_dir_all_force};
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
    let deps_id = app_deps_id(app_name);

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
            remove_dir_all_force(&app_dir)?;
        }
    } else {
        // Just remove rootfs but keep any app data
        let rootfs = paths::app_rootfs_dir(app_name);
        if rootfs.exists() {
            println!("[voidbox] Removing rootfs...");
            remove_dir_all_force(&rootfs)?;
        }
        println!("[voidbox] Note: App data kept in {}", app_dir.display());
        println!("[voidbox] Use --purge to remove everything.");
    }

    // Update installed apps database
    remove_from_database(app_name)?;

    if purge {
        if let Some(deps_id) = deps_id.as_deref() {
            remove_unused_deps_layer(deps_id, app_name)?;
        }
    }

    println!("[voidbox] {} removed successfully!", app_name);

    Ok(())
}

fn app_deps_id(app_name: &str) -> Option<String> {
    let rootfs = paths::app_rootfs_dir(app_name);
    match read_base_info_for_rootfs(&rootfs) {
        Ok(Some(info)) => info.deps_id,
        Ok(None) => None,
        Err(e) => {
            println!(
                "[voidbox] Warning: Could not read base info for {}: {}",
                app_name, e
            );
            None
        }
    }
}

fn remove_unused_deps_layer(deps_id: &str, removed_app: &str) -> Result<(), RemoveError> {
    let apps_dir = paths::apps_dir();
    if !apps_dir.exists() {
        return Ok(());
    }

    let mut can_prune = true;

    for entry in fs::read_dir(&apps_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let app_name = entry.file_name().to_string_lossy().to_string();
        if app_name == removed_app {
            continue;
        }
        let rootfs = paths::app_rootfs_dir(&app_name);
        match read_base_info_for_rootfs(&rootfs) {
            Ok(Some(info)) => {
                if info.deps_id.as_deref() == Some(deps_id) {
                    return Ok(());
                }
            }
            Ok(None) => {}
            Err(e) => {
                println!(
                    "[voidbox] Warning: Could not read base info for {}: {}",
                    app_name, e
                );
                can_prune = false;
            }
        }
    }

    if !can_prune {
        return Ok(());
    }

    let deps_dir = paths::deps_dir().join(deps_id);
    if deps_dir.exists() {
        println!("[voidbox] Removing unused shared dependencies...");
        remove_dir_all_force(&deps_dir)?;
    }

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
