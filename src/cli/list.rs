//! List command implementation

use crate::manifest::InstalledApp;
use crate::storage::paths;
use std::fs;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ListError {
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Failed to read database: {0}")]
    DatabaseError(String),
}

/// List all installed apps
pub fn list_apps() -> Result<(), ListError> {
    let db_path = paths::database_path();

    if !db_path.exists() {
        println!("No apps installed.");
        println!();
        println!("Install an app with: voidbox install <manifest.toml>");
        return Ok(());
    }

    let content = fs::read_to_string(&db_path)?;
    let apps: Vec<InstalledApp> =
        serde_json::from_str(&content).map_err(|e| ListError::DatabaseError(e.to_string()))?;

    if apps.is_empty() {
        println!("No apps installed.");
        println!();
        println!("Install an app with: voidbox install <manifest.toml>");
        return Ok(());
    }

    println!("Installed apps:");
    println!();

    for app in &apps {
        let version = app.version.as_deref().unwrap_or("unknown");
        let date = app.installed_date.as_deref().unwrap_or("");

        println!("  {} ({})", app.display_name, app.name);
        println!("    Version:   {}", version);
        if !date.is_empty() {
            println!("    Installed: {}", date);
        }
        println!();
    }

    println!("Run an app with: voidbox run <app-name>");

    Ok(())
}

/// Get a list of installed app names
pub fn get_installed_apps() -> Result<Vec<InstalledApp>, ListError> {
    let db_path = paths::database_path();

    if !db_path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&db_path)?;
    let apps: Vec<InstalledApp> =
        serde_json::from_str(&content).map_err(|e| ListError::DatabaseError(e.to_string()))?;

    Ok(apps)
}
