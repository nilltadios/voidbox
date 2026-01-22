//! Run command implementation

use crate::manifest::{AppManifest, PermissionConfig, parse_manifest_file};
use crate::runtime::{
    setup_container_namespaces, setup_user_namespace, spawn_container_init, start_host_bridge,
};
use crate::settings::{load_overrides, merge_permissions};
use crate::storage::paths;
use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::{ForkResult, fork};
use std::path::Path;
use std::fs;
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

    #[error("Bridge error: {0}")]
    BridgeError(#[from] crate::runtime::BridgeError),
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
        if paths::app_layer_dir(app_name).exists() {
            fs::create_dir_all(&rootfs)?;
        } else {
            return Err(RunError::NotInstalled(app_name.to_string()));
        }
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

    // If native_mode, we need to fork BEFORE namespace setup
    // Parent stays on host to run the bridge, child enters namespaces
    if permissions.native_mode {
        run_with_host_bridge(&rootfs, &cmd, &cmd_args, &permissions)?;
    } else {
        run_in_container(&rootfs, &cmd, &cmd_args, &permissions)?;
    }

    Ok(())
}

/// Run app without host bridge (standard container mode)
fn run_in_container(
    rootfs: &Path,
    cmd: &str,
    args: &[String],
    permissions: &PermissionConfig,
) -> Result<(), RunError> {
    // Setup namespaces
    setup_user_namespace(permissions.native_mode)?;
    setup_container_namespaces()?;

    // Spawn container init process with permissions
    let self_exe = std::env::current_exe()?;
    let status = spawn_container_init(&self_exe, rootfs, cmd, args, permissions)?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}

/// Run app with host bridge for native mode
/// Forks: parent runs bridge, child runs container
fn run_with_host_bridge(
    rootfs: &Path,
    cmd: &str,
    args: &[String],
    permissions: &PermissionConfig,
) -> Result<(), RunError> {
    // Start the host bridge BEFORE forking so it's available
    let bridge_handle = start_host_bridge()?;
    let bridge_port = bridge_handle.port();

    // Fork: parent stays on host for bridge, child enters namespaces
    match unsafe { fork() } {
        Ok(ForkResult::Parent { child }) => {
            // Parent: wait for child (the container) to exit
            // Keep bridge_handle alive - it runs in a background thread
            let _bridge = bridge_handle;
            loop {
                match waitpid(child, None) {
                    Ok(WaitStatus::Exited(_, code)) => {
                        std::process::exit(code);
                    }
                    Ok(WaitStatus::Signaled(_, sig, _)) => {
                        // Child killed by signal
                        std::process::exit(128 + sig as i32);
                    }
                    Ok(_) => continue, // Other status, keep waiting
                    Err(nix::errno::Errno::ECHILD) => break, // No more children
                    Err(e) => {
                        eprintln!("[voidbox] Wait error: {}", e);
                        break;
                    }
                }
            }
            Ok(())
        }
        Ok(ForkResult::Child) => {
            // Child: setup namespaces and run container
            // Set the bridge port for the container to use
            unsafe {
                std::env::set_var("VOIDBOX_BRIDGE_PORT", bridge_port.to_string());
                std::env::set_var("VOIDBOX_BRIDGE_TOKEN", bridge_handle.token());
            }

            setup_user_namespace(permissions.native_mode)?;
            setup_container_namespaces()?;

            let self_exe = std::env::current_exe()?;
            let status = spawn_container_init(&self_exe, rootfs, cmd, args, permissions)?;

            std::process::exit(status.code().unwrap_or(1));
        }
        Err(e) => Err(RunError::Failed(format!("Fork failed: {}", e))),
    }
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
    let cmd = resolve_binary_symlink(rootfs, binary_name)
        .unwrap_or_else(|| format!("/usr/bin/{}", binary_name));

    let mut cmd_args: Vec<String> = manifest.binary.args.clone();

    // Add URL if specified (for browsers)
    if let Some(u) = url {
        cmd_args.push(u.to_string());
    }

    Ok((cmd, cmd_args))
}

fn resolve_binary_symlink(rootfs: &Path, binary_name: &str) -> Option<String> {
    let symlink_path = rootfs.join("usr/bin").join(binary_name);
    if std::fs::symlink_metadata(&symlink_path).is_ok() {
        if let Ok(target) = std::fs::read_link(&symlink_path) {
            return Some(target.to_string_lossy().into_owned());
        }
    }

    let app_dir = rootfs.parent()?;
    let layer_symlink = app_dir.join("layer/usr/bin").join(binary_name);
    if std::fs::symlink_metadata(&layer_symlink).is_ok() {
        if let Ok(target) = std::fs::read_link(&layer_symlink) {
            return Some(target.to_string_lossy().into_owned());
        }
    }

    None
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
