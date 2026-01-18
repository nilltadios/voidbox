//! Process execution in container

use crate::manifest::PermissionConfig;
use nix::unistd::execvp;
use std::ffi::CString;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ExecError {
    #[error("Failed to execute: {0}")]
    ExecFailed(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Execute a command, replacing the current process
pub fn exec_replace(cmd: &str, args: &[String]) -> Result<(), ExecError> {
    let c_cmd = CString::new(cmd.to_string())
        .map_err(|e| ExecError::ExecFailed(format!("invalid command string: {}", e)))?;

    let c_args: Vec<CString> = std::iter::once(c_cmd.clone())
        .chain(args.iter().map(|a| CString::new(a.as_str()).unwrap()))
        .collect();

    execvp(&c_cmd, &c_args).map_err(|e| ExecError::ExecFailed(format!("{}: {}", cmd, e)))?;

    Ok(())
}

/// Spawn a child process for container initialization
pub fn spawn_container_init(
    self_exe: &Path,
    rootfs: &Path,
    cmd: &str,
    args: &[String],
    permissions: &PermissionConfig,
) -> Result<std::process::ExitStatus, ExecError> {
    // Serialize permissions to JSON for passing via command line
    let permissions_json = serde_json::to_string(permissions)
        .map_err(|e| ExecError::ExecFailed(format!("failed to serialize permissions: {}", e)))?;

    let mut child = Command::new(self_exe)
        .arg("internal-init")
        .arg(rootfs)
        .arg(cmd)
        .arg("--permissions")
        .arg(&permissions_json)
        .arg("--")
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;

    Ok(child.wait()?)
}

/// Start dbus daemon inside container
pub fn start_dbus() -> Result<(), ExecError> {
    fs::create_dir_all("/run/dbus").ok();
    fs::create_dir_all("/var/run/dbus").ok();

    if Path::new("/usr/bin/dbus-daemon").exists() {
        Command::new("/usr/bin/dbus-daemon")
            .args(["--system", "--fork", "--nopidfile"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .ok();
    }

    Ok(())
}

/// Initialize container environment and execute command
pub fn init_and_exec(
    rootfs: &Path,
    cmd: &str,
    args: &[String],
    permissions: &PermissionConfig,
) -> Result<(), ExecError> {
    use super::mount::{pivot_to_container, setup_container_env, setup_container_mounts};

    setup_container_mounts(rootfs, permissions)
        .map_err(|e| ExecError::ExecFailed(format!("mount setup: {}", e)))?;

    pivot_to_container(rootfs).map_err(|e| ExecError::ExecFailed(format!("pivot_root: {}", e)))?;

    setup_container_env();
    start_dbus()?;

    exec_replace(cmd, args)
}
