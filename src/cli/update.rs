//! Update command implementation

use crate::cli::install::install_app;
use crate::manifest::{InstalledApp, SourceConfig, parse_manifest_file};
use crate::storage::paths;
use serde::Deserialize;
use std::fs;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum UpdateError {
    #[error("App not installed: {0}")]
    NotInstalled(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Install error: {0}")]
    InstallError(#[from] crate::cli::InstallError),

    #[error("Manifest error: {0}")]
    ManifestError(#[from] crate::manifest::ManifestError),

    #[error("Update failed: {0}")]
    Failed(String),
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
}

/// Get latest version from GitHub
fn get_latest_github_version(owner: &str, repo: &str) -> Result<String, UpdateError> {
    let api_url = format!(
        "https://api.github.com/repos/{}/{}/releases/latest",
        owner, repo
    );

    let mut resp = ureq::get(&api_url)
        .header("User-Agent", crate::APP_NAME)
        .call()
        .map_err(|e| UpdateError::Failed(format!("GitHub API error: {}", e)))?;

    let body = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| UpdateError::Failed(format!("Failed to read response: {}", e)))?;

    let release: GitHubRelease = serde_json::from_str(&body)
        .map_err(|e| UpdateError::Failed(format!("Failed to parse GitHub response: {}", e)))?;

    Ok(release.tag_name.trim_start_matches('v').to_string())
}

/// Get installed version of an app
fn get_installed_version(app_name: &str) -> Option<String> {
    let db_path = paths::database_path();
    if !db_path.exists() {
        return None;
    }

    let content = fs::read_to_string(&db_path).ok()?;
    let apps: Vec<InstalledApp> = serde_json::from_str(&content).ok()?;

    apps.into_iter()
        .find(|a| a.name == app_name)
        .and_then(|a| a.version)
}

/// Compare versions (returns true if latest > installed)
fn is_newer_version(installed: &str, latest: &str) -> bool {
    let parse_version = |s: &str| -> Vec<u32> {
        s.split(|c: char| !c.is_ascii_digit())
            .filter_map(|p| p.parse().ok())
            .collect()
    };

    let installed_parts = parse_version(installed);
    let latest_parts = parse_version(latest);

    latest_parts > installed_parts
}

/// Update a specific app
pub fn update_app(app_name: &str, force: bool) -> Result<(), UpdateError> {
    let manifest_path = paths::manifest_path(app_name);

    if !manifest_path.exists() {
        return Err(UpdateError::NotInstalled(app_name.to_string()));
    }

    // Load manifest to check source
    let manifest = parse_manifest_file(&manifest_path)?;
    let display_name = &manifest.app.display_name;

    // Get installed version
    let installed_version = get_installed_version(app_name);

    // Check for updates based on source type
    let latest_version = match &manifest.source {
        SourceConfig::Github { owner, repo, .. } => {
            Some(get_latest_github_version(owner, repo)?)
        }
        SourceConfig::Direct { .. } => None, // Can't check version for direct URLs
        SourceConfig::Local { .. } => None,  // Local sources don't have remote versions
    };

    // Compare versions
    if !force {
        if let (Some(installed), Some(latest)) = (&installed_version, &latest_version) {
            if !is_newer_version(installed, latest) {
                println!(
                    "[voidbox] {} is up to date (v{})",
                    display_name, installed
                );
                return Ok(());
            }
            println!(
                "[voidbox] {} update available: v{} -> v{}",
                display_name, installed, latest
            );
        } else if installed_version.is_some() && latest_version.is_none() {
            println!(
                "[voidbox] {} - cannot check for updates (non-GitHub source)",
                display_name
            );
            return Ok(());
        }
    }

    println!("[voidbox] Updating {}...", display_name);

    // Reinstall the app (force=true to overwrite)
    install_app(manifest_path.to_str().unwrap(), true)?;

    Ok(())
}

/// Update all installed apps
pub fn update_all(force: bool) -> Result<(), UpdateError> {
    let db_path = paths::database_path();

    if !db_path.exists() {
        println!("[voidbox] No apps installed.");
        return Ok(());
    }

    let content = fs::read_to_string(&db_path)?;
    let apps: Vec<InstalledApp> = serde_json::from_str(&content)
        .map_err(|e| UpdateError::Failed(format!("Failed to parse database: {}", e)))?;

    if apps.is_empty() {
        println!("[voidbox] No apps installed.");
        return Ok(());
    }

    println!("[voidbox] Checking {} app(s) for updates...", apps.len());

    let mut updated = 0;
    let mut up_to_date = 0;
    let mut failed = 0;

    for app in &apps {
        match update_app(&app.name, force) {
            Ok(()) => {
                // Check if it was actually updated or already up to date
                // by looking at the output (the function prints its status)
                updated += 1;
            }
            Err(UpdateError::Failed(msg)) if msg.contains("up to date") => {
                up_to_date += 1;
            }
            Err(e) => {
                println!("[voidbox] Failed to update {}: {}", app.name, e);
                failed += 1;
            }
        }
    }

    println!("[voidbox] Update check complete!");
    if updated > 0 || up_to_date > 0 || failed > 0 {
        if failed > 0 {
            println!("  {} failed", failed);
        }
    }

    Ok(())
}

/// Self-update voidbox
pub fn self_update(force: bool) -> Result<(), UpdateError> {
    println!("[voidbox] Checking for updates...");
    println!("  Installed: v{}", crate::VERSION);

    let status = self_update::backends::github::Update::configure()
        .repo_owner(crate::SELF_UPDATE_OWNER)
        .repo_name(crate::SELF_UPDATE_REPO)
        .bin_name(crate::APP_NAME)
        .identifier(crate::APP_NAME)
        .current_version(crate::VERSION)
        .build()
        .map_err(|e| UpdateError::Failed(format!("Failed to configure update: {}", e)))?;

    let latest = status
        .get_latest_release()
        .map_err(|e| UpdateError::Failed(format!("Failed to check for updates: {}", e)))?;

    let latest_version = latest.version.trim_start_matches('v');
    println!("  Latest:    v{}", latest_version);

    let current = semver::Version::parse(crate::VERSION).ok();
    let latest_parsed = semver::Version::parse(latest_version).ok();

    let is_newer = match (&current, &latest_parsed) {
        (Some(c), Some(l)) => l > c,
        _ => latest_version != crate::VERSION,
    };

    if !force && !is_newer {
        println!("[voidbox] Already running latest version.");
        return Ok(());
    }

    println!("[voidbox] Updating to v{}...", latest_version);

    self_update::backends::github::Update::configure()
        .repo_owner(crate::SELF_UPDATE_OWNER)
        .repo_name(crate::SELF_UPDATE_REPO)
        .bin_name(crate::APP_NAME)
        .identifier(crate::APP_NAME)
        .current_version(crate::VERSION)
        .build()
        .map_err(|e| UpdateError::Failed(format!("Failed to configure update: {}", e)))?
        .update()
        .map_err(|e| UpdateError::Failed(format!("Update failed: {}", e)))?;

    println!(
        "[voidbox] Updated to v{}! Please restart voidbox.",
        latest_version
    );

    Ok(())
}
