//! Update command implementation

use crate::cli::install::install_app;
use crate::manifest::{InstalledApp, SourceConfig, parse_manifest_file};
use crate::storage::{paths, download_string, BaseInfo};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
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

    #[error("Download error: {0}")]
    DownloadError(#[from] crate::storage::DownloadError),

    #[error("Update failed: {0}")]
    Failed(String),
}

#[derive(Debug, Clone, Copy)]
pub enum UpdateOutcome {
    Updated,
    UpToDate,
    Skipped,
    Unknown,
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

fn get_latest_direct_version(version_url: &str) -> Result<Option<String>, UpdateError> {
    let content = download_string(version_url)?;
    Ok(parse_version_response(&content))
}

fn parse_version_response(content: &str) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        if let Some(obj) = value.as_object() {
            for key in ["productVersion", "version", "name", "tag_name"] {
                if let Some(version) = obj.get(key).and_then(|v| v.as_str()) {
                    return Some(version.trim_start_matches('v').to_string());
                }
            }
        }

        if let Some(array) = value.as_array() {
            for item in array {
                if let Some(version) = item.as_str() {
                    return Some(version.trim_start_matches('v').to_string());
                }
                if let Some(obj) = item.as_object() {
                    for key in ["productVersion", "version", "name", "tag_name"] {
                        if let Some(version) = obj.get(key).and_then(|v| v.as_str()) {
                            return Some(version.trim_start_matches('v').to_string());
                        }
                    }
                }
            }
        }
    }

    trimmed
        .lines()
        .next()
        .map(|line| line.trim().trim_start_matches('v').to_string())
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
pub fn update_app(app_name: &str, force: bool) -> Result<UpdateOutcome, UpdateError> {
    let manifest_path = paths::manifest_path(app_name);

    if !manifest_path.exists() {
        return Err(UpdateError::NotInstalled(app_name.to_string()));
    }

    // Load manifest to check source
    let manifest = parse_manifest_file(&manifest_path)?;
    let display_name = &manifest.app.display_name;

    // Get installed version
    let installed_version = get_installed_version(app_name).or_else(|| manifest.app.version.clone());

    // Check for updates based on source type
    let latest_version = match &manifest.source {
        SourceConfig::Github { owner, repo, .. } => Some(get_latest_github_version(owner, repo)?),
        SourceConfig::Direct { version_url, .. } => match version_url.as_deref() {
            Some(url) => get_latest_direct_version(url)?,
            None => None,
        },
        SourceConfig::Local { .. } => None,
    };

    // Compare versions
    if !force {
        match &manifest.source {
            SourceConfig::Github { .. } => {
                let Some(latest) = latest_version.as_deref() else {
                    println!(
                        "[voidbox] {} - cannot check for updates right now",
                        display_name
                    );
                    return Ok(UpdateOutcome::Unknown);
                };
                let Some(installed) = installed_version.as_deref() else {
                    println!(
                        "[voidbox] {} - cannot determine installed version (use --force to update)",
                        display_name
                    );
                    return Ok(UpdateOutcome::Unknown);
                };
                if !is_newer_version(installed, latest) {
                    println!("[voidbox] {} is up to date (v{})", display_name, installed);
                    return Ok(UpdateOutcome::UpToDate);
                }
                println!(
                    "[voidbox] {} update available: v{} -> v{}",
                    display_name, installed, latest
                );
            }
            SourceConfig::Direct { version_url, .. } => match version_url {
                Some(_) => {
                    let Some(latest) = latest_version.as_deref() else {
                        println!(
                            "[voidbox] {} - cannot check for updates right now",
                            display_name
                        );
                        return Ok(UpdateOutcome::Unknown);
                    };
                    let Some(installed) = installed_version.as_deref() else {
                        println!(
                            "[voidbox] {} - cannot determine installed version (use --force to update)",
                            display_name
                        );
                        return Ok(UpdateOutcome::Unknown);
                    };
                    if !is_newer_version(installed, latest) {
                        println!("[voidbox] {} is up to date (v{})", display_name, installed);
                        return Ok(UpdateOutcome::UpToDate);
                    }
                    println!(
                        "[voidbox] {} update available: v{} -> v{}",
                        display_name, installed, latest
                    );
                }
                None => {
                    println!(
                        "[voidbox] {} - cannot check for updates (direct source)",
                        display_name
                    );
                    return Ok(UpdateOutcome::Skipped);
                }
            },
            SourceConfig::Local { .. } => {
                println!(
                    "[voidbox] {} - cannot check for updates (local source)",
                    display_name
                );
                return Ok(UpdateOutcome::Skipped);
            }
        }
    }

    println!("[voidbox] Updating {}...", display_name);

    // Reinstall the app (force=true to overwrite)
    install_app(manifest_path.to_str().unwrap(), true)?;

    Ok(UpdateOutcome::Updated)
}

/// Read base info from a base.json file
fn read_base_json(path: &Path) -> Option<BaseInfo> {
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Get all unique deps_ids from installed apps
fn get_all_deps_ids() -> Result<HashSet<String>, UpdateError> {
    let mut deps_ids = HashSet::new();
    let apps_dir = paths::apps_dir();

    if !apps_dir.exists() {
        return Ok(deps_ids);
    }

    for entry in fs::read_dir(&apps_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let base_json = entry.path().join("base.json");
            if let Some(info) = read_base_json(&base_json) {
                if let Some(deps_id) = info.deps_id {
                    deps_ids.insert(deps_id);
                }
            }
        }
    }

    Ok(deps_ids)
}

/// Upgrade system packages in a deps layer
fn upgrade_deps_layer(deps_id: &str) -> Result<(), UpdateError> {
    let deps_rootfs = paths::deps_rootfs_dir(deps_id);
    let deps_layer = paths::deps_layer_dir(deps_id);

    if !deps_rootfs.exists() {
        return Err(UpdateError::Failed(format!(
            "Deps layer not found: {}",
            deps_id
        )));
    }

    println!("[voidbox] Upgrading system packages in {}...", deps_id);

    // Create upgrade script
    let upgrade_script = r#"#!/bin/bash
export DEBIAN_FRONTEND=noninteractive
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

echo "Updating package lists..."
apt-get update -qq

echo "Upgrading packages..."
apt-get upgrade -y --no-install-recommends 2>&1

echo "Cleaning up..."
apt-get autoremove -y 2>/dev/null || true
apt-get clean
rm -rf /var/lib/apt/lists/*

echo "System packages upgraded!"
"#;

    let script_path = deps_layer.join("upgrade.sh");
    fs::write(&script_path, upgrade_script)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755))?;
    }

    // Run upgrade script using voidbox internal-run
    let voidbox_exe = paths::install_path();
    let exe_to_use = if voidbox_exe.exists() {
        voidbox_exe
    } else {
        std::env::current_exe()?
    };

    let status = Command::new(&exe_to_use)
        .args(["internal-run", deps_rootfs.to_str().unwrap(), "/upgrade.sh"])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| UpdateError::Failed(format!("Failed to run upgrade: {}", e)))?;

    // Clean up script
    fs::remove_file(&script_path).ok();

    if !status.success() {
        return Err(UpdateError::Failed(format!(
            "Upgrade failed with exit code: {:?}",
            status.code()
        )));
    }

    Ok(())
}

/// Update all installed apps and system packages
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

    // First, upgrade system packages in all shared deps layers
    let deps_ids = get_all_deps_ids()?;
    if !deps_ids.is_empty() {
        println!(
            "[voidbox] Upgrading system packages in {} shared layer(s)...",
            deps_ids.len()
        );
        for deps_id in &deps_ids {
            if let Err(e) = upgrade_deps_layer(deps_id) {
                println!("[voidbox] Warning: Failed to upgrade {}: {}", deps_id, e);
            }
        }
        println!("[voidbox] System packages upgraded.");
    }

    // Then check and update app binaries
    println!(
        "[voidbox] Checking {} app(s) for updates...",
        apps.len()
    );

    let mut updated = 0;
    let mut up_to_date = 0;
    let mut skipped = 0;
    let mut unknown = 0;
    let mut failed = 0;

    for app in &apps {
        match update_app(&app.name, force) {
            Ok(UpdateOutcome::Updated) => updated += 1,
            Ok(UpdateOutcome::UpToDate) => up_to_date += 1,
            Ok(UpdateOutcome::Skipped) => skipped += 1,
            Ok(UpdateOutcome::Unknown) => unknown += 1,
            Err(e) => {
                println!("[voidbox] Failed to update {}: {}", app.name, e);
                failed += 1;
            }
        }
    }

    println!("[voidbox] Update check complete!");
    if updated > 0 {
        println!("  {} updated", updated);
    }
    if up_to_date > 0 {
        println!("  {} up to date", up_to_date);
    }
    if skipped > 0 {
        println!("  {} skipped", skipped);
    }
    if unknown > 0 {
        println!("  {} unknown", unknown);
    }
    if failed > 0 {
        println!("  {} failed", failed);
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
