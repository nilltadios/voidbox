//! Symlink management for PATH integration

use crate::storage::paths;
use std::fs;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SymlinkError {
    #[error("Failed to create symlink: {0}")]
    CreateError(#[from] std::io::Error),
}

/// Create a wrapper script for an app in ~/.local/bin
pub fn create_app_wrapper(app_name: &str) -> Result<(), SymlinkError> {
    let wrapper_path = paths::bin_dir().join(app_name);

    if let Some(parent) = wrapper_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let voidbox_path = paths::voidbox_exe_path();
    let voidbox_exec = voidbox_path.to_string_lossy();

    // Create a shell script that invokes voidbox
    let script = format!(
        r#"#!/bin/sh
exec {} run {} -- "$@"
"#,
        voidbox_exec,
        app_name
    );

    fs::write(&wrapper_path, script)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&wrapper_path, fs::Permissions::from_mode(0o755))?;
    }

    Ok(())
}

/// Remove wrapper script for an app
pub fn remove_app_wrapper(app_name: &str) -> Result<(), SymlinkError> {
    let wrapper_path = paths::bin_dir().join(app_name);
    if wrapper_path.exists() {
        fs::remove_file(wrapper_path)?;
    }
    Ok(())
}

/// Install voidbox binary to ~/.local/bin
pub fn install_self() -> Result<(), SymlinkError> {
    let current_exe = std::env::current_exe()?;
    let install_path = paths::install_path();

    if let Some(parent) = install_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Don't reinstall if already at the target location
    if current_exe == install_path {
        return Ok(());
    }

    println!(
        "[{}] Installing to {}...",
        crate::APP_NAME,
        install_path.display()
    );
    fs::copy(&current_exe, &install_path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&install_path, fs::Permissions::from_mode(0o755))?;
    }

    Ok(())
}

/// Check if voidbox is installed
pub fn is_installed() -> bool {
    paths::install_path().exists()
}
