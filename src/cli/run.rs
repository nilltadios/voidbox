//! Run command implementation

use crate::manifest::{AppManifest, PermissionConfig, parse_manifest_file};
use crate::runtime::{setup_container_namespaces, setup_user_namespace, spawn_container_init};
use crate::settings::{load_overrides, merge_permissions};
use crate::storage::paths;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RunError {
    #[error("App not installed: {0}")]
    NotInstalled(String),

    #[error("Manifest error: {0}")]
    ManifestError(#[from] crate::manifest::ManifestError),

    #[error("Namespace error: {0}")]
    NamespaceError(#[from] crate::runtime::NamespaceError),

    #[error("Exec error: {0}")]
    ExecError(#[from] crate::runtime::ExecError),

    #[error("Settings error: {0}")]
    SettingsError(#[from] crate::settings::SettingsError),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Run failed: {0}")]
    Failed(String),
}

/// Run an installed app
pub fn run_app(
    app_name: &str,
    args: &[String],
    url: Option<&str>,
    dev_mode: bool,
) -> Result<(), RunError> {
    // Check if app is installed
    let manifest_path = paths::manifest_path(app_name);
    if !manifest_path.exists() {
        return Err(RunError::NotInstalled(app_name.to_string()));
    }

    let rootfs = paths::app_rootfs_dir(app_name);
    if !rootfs.exists() {
        return Err(RunError::NotInstalled(app_name.to_string()));
    }

    // Load manifest
    let manifest = parse_manifest_file(&manifest_path)?;

    // Get permissions (manifest defaults + user overrides)
    let mut permissions = manifest.permissions.clone();
    if let Some(overrides) = load_overrides(app_name)? {
        permissions = merge_permissions(&manifest.permissions, Some(&overrides));
    }

    // Override dev_mode if specified on command line
    if dev_mode {
        permissions.dev_mode = true;
    }

    // Build command and args
    let (cmd, cmd_args) = build_command(&manifest, args, url, &rootfs)?;

    // Setup namespaces
    setup_user_namespace(permissions.native_mode)?;
    setup_container_namespaces()?;

    // Spawn container init process with permissions
    let self_exe = std::env::current_exe()?;
    let status = spawn_container_init(&self_exe, &rootfs, &cmd, &cmd_args, &permissions)?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}

/// Build the command and arguments to run
fn build_command(
    manifest: &AppManifest,
    args: &[String],
    url: Option<&str>,
    rootfs: &Path,
) -> Result<(String, Vec<String>), RunError> {
    if !args.is_empty() {
        // Custom command specified
        return Ok((args[0].clone(), args[1..].to_vec()));
    }

    // Default app command
    let binary_name = &manifest.binary.name;

    // Resolve the actual binary path by reading the symlink created during install
    // This is required for native_mode where /usr/bin is masked by the host
    let symlink_path = rootfs.join("usr/bin").join(binary_name);

    // Use symlink_metadata to check existence of the link itself, not the target
    // (since target is absolute path inside container, it won't exist on host)
    let cmd = if std::fs::symlink_metadata(&symlink_path).is_ok() {
        match std::fs::read_link(&symlink_path) {
            Ok(target) => target.to_string_lossy().into_owned(),
            Err(_) => format!("/usr/bin/{}", binary_name),
        }
    } else {
        format!("/usr/bin/{}", binary_name)
    };

    let mut cmd_args: Vec<String> = manifest.binary.args.clone();

    // Add URL if specified (for browsers)
    if let Some(u) = url {
        cmd_args.push(u.to_string());
    }

    Ok((cmd, cmd_args))
}

/// Internal init function - called after fork in new namespace
pub fn internal_init(
    rootfs: &Path,
    cmd: &str,
    args: &[String],
    permissions: &PermissionConfig,
) -> Result<(), RunError> {
    use crate::runtime::init_and_exec;

    init_and_exec(rootfs, cmd, args, permissions)?;

    Ok(())
}
