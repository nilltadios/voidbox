//! Info command implementation

use crate::manifest::{InstalledApp, parse_manifest_file};
use crate::storage::paths;
use std::fs;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum InfoError {
    #[error("App not installed: {0}")]
    NotInstalled(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Manifest error: {0}")]
    ManifestError(#[from] crate::manifest::ManifestError),
}

/// Show info about voidbox itself
pub fn show_voidbox_info() -> Result<(), InfoError> {
    println!("voidbox v{}", crate::VERSION);
    println!("Universal Linux App Platform");
    println!();
    println!("Data directory: {}", paths::data_dir().display());
    println!("Install path:   {}", paths::install_path().display());
    println!();

    // Count installed apps
    let db_path = paths::database_path();
    if db_path.exists() {
        let content = fs::read_to_string(&db_path)?;
        let apps: Vec<InstalledApp> = serde_json::from_str(&content).unwrap_or_default();
        println!("Installed apps: {}", apps.len());
    } else {
        println!("Installed apps: 0");
    }

    println!();

    // Check for self-updates
    print!("Checking for updates... ");
    match check_latest_version() {
        Ok(latest) => {
            if latest == crate::VERSION {
                println!("Up to date (v{})", latest);
            } else {
                println!("Update available: v{}", latest);
            }
        }
        Err(e) => println!("Failed ({})", e),
    }

    Ok(())
}

/// Show info about a specific app
pub fn show_app_info(app_name: &str) -> Result<(), InfoError> {
    let manifest_path = paths::manifest_path(app_name);
    if !manifest_path.exists() {
        return Err(InfoError::NotInstalled(app_name.to_string()));
    }

    let manifest = parse_manifest_file(&manifest_path)?;
    let rootfs = paths::app_rootfs_dir(app_name);

    println!("{}", manifest.app.display_name);
    println!("{}", "=".repeat(manifest.app.display_name.len()));
    println!();
    println!("Name:        {}", manifest.app.name);
    println!("Description: {}", manifest.app.description);

    if let Some(version) = &manifest.app.version {
        println!("Version:     {}", version);
    }

    if let Some(license) = &manifest.app.license {
        println!("License:     {}", license);
    }

    println!();
    println!(
        "Rootfs:      {} ({})",
        rootfs.display(),
        if rootfs.exists() { "exists" } else { "missing" }
    );
    println!("Manifest:    {}", manifest_path.display());

    // Show permissions
    println!();
    println!("Permissions:");
    let perms = &manifest.permissions;
    println!("  Network:    {}", if perms.network { "yes" } else { "no" });
    println!("  Audio:      {}", if perms.audio { "yes" } else { "no" });
    println!(
        "  Microphone: {}",
        if perms.microphone { "yes" } else { "no" }
    );
    println!("  GPU:        {}", if perms.gpu { "yes" } else { "no" });
    println!("  Camera:     {}", if perms.camera { "yes" } else { "no" });
    println!("  Home:       {}", if perms.home { "yes" } else { "no" });
    println!(
        "  Dev Mode:   {}",
        if perms.dev_mode { "yes" } else { "no" }
    );

    Ok(())
}

fn check_latest_version() -> Result<String, String> {
    let status = self_update::backends::github::Update::configure()
        .repo_owner(crate::SELF_UPDATE_OWNER)
        .repo_name(crate::SELF_UPDATE_REPO)
        .bin_name(crate::APP_NAME)
        .current_version(crate::VERSION)
        .build()
        .map_err(|e| e.to_string())?;

    let latest = status.get_latest_release().map_err(|e| e.to_string())?;

    Ok(latest.version.trim_start_matches('v').to_string())
}
