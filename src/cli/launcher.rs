//! App launcher mode - handles void_* binary invocations
//!
//! When the binary is invoked as "void_brave", "void_discord", etc.,
//! this module handles installing and running the corresponding app.
//!
//! The binary self-installs: running void_brave will install voidbox
//! to ~/.local/bin/voidbox and create the void_brave symlink automatically.

use crate::cli;
use crate::gui;
use crate::manifest::parse_manifest;
use crate::storage::paths;
use std::fs;
use std::os::unix::fs::symlink;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LauncherError {
    #[error("Unknown app: {0}")]
    UnknownApp(String),

    #[error("Manifest error: {0}")]
    ManifestError(#[from] crate::manifest::ManifestError),

    #[error("Install error: {0}")]
    InstallError(#[from] crate::cli::InstallError),

    #[error("Run error: {0}")]
    RunError(#[from] crate::cli::RunError),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// List of all embedded apps - used to create symlinks
pub const EMBEDDED_APPS: &[&str] = &["brave", "discord", "vscode"];

/// Embedded manifests for known apps
/// Add new apps here with their manifest content
fn get_embedded_manifest(app_name: &str) -> Option<&'static str> {
    match app_name {
        "brave" => Some(include_str!("../../examples/manifests/brave.toml")),
        "discord" => Some(include_str!("../../examples/manifests/discord.toml")),
        "vscode" => Some(include_str!("../../examples/manifests/vscode.toml")),
        _ => None,
    }
}

/// Install voidbox runtime and create app launcher symlinks
fn ensure_runtime_installed(app_name: &str, gui_mode: bool) -> Result<(), LauncherError> {
    let voidbox_path = paths::install_path();
    let current_exe = std::env::current_exe()?;

    // Check if voidbox is installed
    let voidbox_installed = voidbox_path.exists();

    // Check if our app symlink exists
    let symlink_path = paths::bin_dir().join(format!("void_{}", app_name));
    let symlink_exists = symlink_path.exists();

    if voidbox_installed && symlink_exists {
        return Ok(());
    }

    // Ensure bin directory exists
    fs::create_dir_all(paths::bin_dir())?;

    // Install voidbox if not present
    if !voidbox_installed {
        if !gui_mode {
            println!("[voidbox] Installing voidbox to {}...", voidbox_path.display());
        }
        fs::copy(&current_exe, &voidbox_path)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&voidbox_path, fs::Permissions::from_mode(0o755))?;
        }
    }

    // Create symlink for this app if not present
    if !symlink_exists {
        if !gui_mode {
            println!("[voidbox] Creating {} symlink...", symlink_path.display());
        }
        // Remove broken symlink if it exists
        let _ = fs::remove_file(&symlink_path);
        symlink(&voidbox_path, &symlink_path)?;
    }

    Ok(())
}

/// Extract app name from binary name (e.g., "void_brave" -> "brave")
pub fn extract_app_name(binary_name: &str) -> Option<String> {
    // Handle full paths - get just the filename
    let name = std::path::Path::new(binary_name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(binary_name);

    // Check if it starts with "void_"
    if name.starts_with("void_") {
        Some(name.strip_prefix("void_").unwrap().to_string())
    } else {
        None
    }
}

/// Check if we should run in launcher mode based on argv[0]
///
/// Returns None if:
/// - Binary name doesn't start with "void_"
/// - There are arguments that indicate a voidbox subcommand (internal-init, run, install, etc.)
pub fn should_run_as_launcher() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    if args.is_empty() {
        return None;
    }

    // Don't run as launcher if there are subcommand arguments
    // This prevents infinite loops when spawn_container_init calls us back
    if args.len() > 1 {
        let subcommands = [
            "internal-init",
            "install",
            "remove",
            "run",
            "update",
            "list",
            "info",
            "shell",
            "search",
            "settings",
            "self-update",
            "--help",
            "-h",
            "--version",
            "-V",
        ];
        if subcommands.contains(&args[1].as_str()) {
            return None;
        }
    }

    extract_app_name(&args[0])
}

/// Run in app launcher mode
pub fn run_launcher(app_name: &str) -> Result<(), LauncherError> {
    // Get embedded manifest or error
    let manifest_content = get_embedded_manifest(app_name)
        .ok_or_else(|| LauncherError::UnknownApp(app_name.to_string()))?;

    // Parse the manifest
    let manifest = parse_manifest(manifest_content)?;
    let display_name = &manifest.app.display_name;

    // Check if we're in GUI mode
    let gui_mode = gui::is_gui_mode();

    // Ensure voidbox runtime is installed and symlinks exist
    ensure_runtime_installed(app_name, gui_mode)?;

    // Ensure data directories exist
    paths::ensure_dirs()?;

    // Check if app is installed
    let manifest_path = paths::manifest_path(app_name);
    let app_installed = manifest_path.exists() && paths::app_rootfs_dir(app_name).exists();

    if !app_installed {
        // App not installed - install it
        if gui_mode {
            let should_install = gui::ask_yes_no(
                &format!("Install {}", display_name),
                &format!(
                    "{} is not installed.\n\n\
                    This will download and install {}.\n\
                    It may take a few minutes.\n\n\
                    Install now?",
                    display_name, display_name
                ),
            );
            if !should_install {
                return Ok(());
            }

            let progress = gui::ProgressDialog::new(
                &format!("Installing {}", display_name),
                &format!(
                    "Downloading and installing {}...\nThis may take a few minutes.",
                    display_name
                ),
            );

            // Write manifest and install
            std::fs::write(&manifest_path, manifest_content)?;
            match cli::install_app_from_manifest(&manifest, false) {
                Ok(()) => {
                    drop(progress);
                    gui::notify(
                        "Installation Complete",
                        &format!("{} has been installed!", display_name),
                    );
                }
                Err(e) => {
                    drop(progress);
                    // Clean up partial install
                    let _ = std::fs::remove_file(&manifest_path);
                    gui::show_error(
                        "Installation Failed",
                        &format!("Failed to install {}:\n\n{}", display_name, e),
                    );
                    return Err(e.into());
                }
            }
        } else {
            println!("[voidbox] Installing {}...", display_name);
            std::fs::write(&manifest_path, manifest_content)?;
            cli::install_app_from_manifest(&manifest, false)?;
            println!("[voidbox] {} installed.", display_name);
        }
    }

    // Run the app
    if !gui_mode {
        println!("[voidbox] Starting {}...", display_name);
    }

    // Get command line args to pass through (skip argv[0])
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Run the app directly using our own run logic
    // This avoids the need to spawn a separate process
    cli::run_app(app_name, &args, None, false)?;

    Ok(())
}
