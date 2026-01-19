//! Shell command implementation

use crate::manifest::{PermissionConfig, parse_manifest_file};
use crate::runtime::{
    setup_container_namespaces, setup_user_namespace, spawn_container_init, start_host_bridge,
};
use crate::storage::paths;
use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::{ForkResult, fork};
use std::path::Path;
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

    #[error("Bridge error: {0}")]
    BridgeError(#[from] crate::runtime::BridgeError),
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

    let shell = "/bin/bash".to_string();
    let args: Vec<String> = vec![];

    // If native_mode, use host bridge
    if permissions.native_mode {
        shell_with_host_bridge(&rootfs, &shell, &args, &permissions)?;
    } else {
        shell_in_container(&rootfs, &shell, &args, &permissions)?;
    }

    Ok(())
}

/// Shell without host bridge (standard container mode)
fn shell_in_container(
    rootfs: &Path,
    shell: &str,
    args: &[String],
    permissions: &PermissionConfig,
) -> Result<(), ShellError> {
    setup_user_namespace(permissions.native_mode)?;
    setup_container_namespaces()?;

    let self_exe = std::env::current_exe()?;
    let status = spawn_container_init(&self_exe, rootfs, shell, args, permissions)
        .map_err(|e| ShellError::Failed(e.to_string()))?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}

/// Shell with host bridge for native mode
fn shell_with_host_bridge(
    rootfs: &Path,
    shell: &str,
    args: &[String],
    permissions: &PermissionConfig,
) -> Result<(), ShellError> {
    // Start the host bridge BEFORE forking
    let bridge_handle = start_host_bridge()?;
    let bridge_port = bridge_handle.port();

    match unsafe { fork() } {
        Ok(ForkResult::Parent { child }) => {
            let _bridge = bridge_handle;
            loop {
                match waitpid(child, None) {
                    Ok(WaitStatus::Exited(_, code)) => {
                        std::process::exit(code);
                    }
                    Ok(WaitStatus::Signaled(_, sig, _)) => {
                        std::process::exit(128 + sig as i32);
                    }
                    Ok(_) => continue,
                    Err(nix::errno::Errno::ECHILD) => break,
                    Err(e) => {
                        eprintln!("[voidbox] Wait error: {}", e);
                        break;
                    }
                }
            }
            Ok(())
        }
        Ok(ForkResult::Child) => {
            unsafe {
                std::env::set_var("VOIDBOX_BRIDGE_PORT", bridge_port.to_string());
                std::env::set_var("VOIDBOX_BRIDGE_TOKEN", bridge_handle.token());
            }

            setup_user_namespace(permissions.native_mode)?;
            setup_container_namespaces()?;

            let self_exe = std::env::current_exe()?;
            let status = spawn_container_init(&self_exe, rootfs, shell, args, permissions)
                .map_err(|e| ShellError::Failed(e.to_string()))?;

            std::process::exit(status.code().unwrap_or(1));
        }
        Err(e) => Err(ShellError::Failed(format!("Fork failed: {}", e))),
    }
}
