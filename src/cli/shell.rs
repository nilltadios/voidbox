//! Shell command implementation

use crate::manifest::parse_manifest_file;
use crate::runtime::{setup_container_namespaces, setup_user_namespace, spawn_container_init};
use crate::storage::paths;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ShellError {
    #[error("App not installed: {0}")]
    NotInstalled(String),

    #[error("Manifest error: {0}")]
    ManifestError(#[from] crate::manifest::ManifestError),

    #[error("Namespace error: {0}")]
    NamespaceError(#[from] crate::runtime::NamespaceError),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Shell failed: {0}")]
    Failed(String),
}

/// Open a shell in an app's container
pub fn shell(app_name: &str, dev_mode: bool) -> Result<(), ShellError> {
    let manifest_path = paths::manifest_path(app_name);
    if !manifest_path.exists() {
        return Err(ShellError::NotInstalled(app_name.to_string()));
    }

    let rootfs = paths::app_rootfs_dir(app_name);
    if !rootfs.exists() {
        return Err(ShellError::NotInstalled(app_name.to_string()));
    }

    // Load manifest for permissions
    let manifest = parse_manifest_file(&manifest_path)?;
    let mut permissions = manifest.permissions.clone();

    // Always enable dev_mode for shell access (or if explicitly requested)
    permissions.dev_mode = dev_mode || true;

    println!("[voidbox] Opening shell in {} container...", app_name);
    println!("[voidbox] Type 'exit' to leave the container.");
    println!();

    // Setup namespaces
    setup_user_namespace(permissions.native_mode)?;
    setup_container_namespaces()?;

    // Spawn shell with permissions
    let self_exe = std::env::current_exe()?;
    let shell = "/bin/bash".to_string();
    let args: Vec<String> = vec![];

    let status = spawn_container_init(&self_exe, &rootfs, &shell, &args, &permissions)
        .map_err(|e| ShellError::Failed(e.to_string()))?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}
