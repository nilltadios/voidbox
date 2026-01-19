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

    let mut command = Command::new(self_exe);
    command
        .arg("internal-init")
        .arg(rootfs)
        .arg(cmd)
        .arg("--permissions")
        .arg(&permissions_json)
        .arg("--")
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    // VOIDBOX_BRIDGE_PORT is set by run.rs/shell.rs before calling this
    // and will be inherited by the spawned child

    let mut child = command.spawn()?;

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
    use super::mount::{
        pivot_to_container, setup_container_env, setup_container_mounts, setup_host_bridge_shims,
        setup_user_identity,
    };
    use nix::sys::wait::{WaitStatus, waitpid};
    use nix::unistd::Pid;

    setup_container_mounts(rootfs, permissions)
        .map_err(|e| ExecError::ExecFailed(format!("mount setup: {}", e)))?;

    // Setup user identity masquerade (makes whoami return host username)
    if permissions.native_mode {
        setup_user_identity(rootfs)
            .map_err(|e| ExecError::ExecFailed(format!("user identity setup: {}", e)))?;
    }

    pivot_to_container(rootfs, permissions)
        .map_err(|e| ExecError::ExecFailed(format!("pivot_root: {}", e)))?;

    setup_container_env(permissions);

    // Setup host bridge shims (sudo, host-exec) if bridge port is available
    if let Ok(port_str) = std::env::var("VOIDBOX_BRIDGE_PORT") {
        if let Ok(port) = port_str.parse::<u16>() {
            let token = std::env::var("VOIDBOX_BRIDGE_TOKEN").unwrap_or_default();
            if let Err(e) = setup_host_bridge_shims(port, &token) {
                eprintln!(
                    "[voidbox] Warning: Failed to setup host bridge shims: {}",
                    e
                );
            }
        }
    }

    // Only start dbus in non-native mode; native_mode uses host's D-Bus
    if !permissions.native_mode {
        start_dbus()?;
    }

    // Become a subreaper so orphaned child processes are reparented to us
    // This is crucial for apps like VSCode where the launcher script exits
    // after spawning the actual Electron process
    #[cfg(target_os = "linux")]
    unsafe {
        libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0);
    }

    // Spawn app as child process
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| ExecError::ExecFailed(format!("{}: {}", cmd, e)))?;

    // Wait for direct child first
    let status = child
        .wait()
        .map_err(|e| ExecError::ExecFailed(format!("wait: {}", e)))?;
    let exit_code = status.code().unwrap_or(1);

    // Keep reaping orphaned children until none remain
    // This handles apps that spawn processes and exit (like VSCode's launcher)
    loop {
        match waitpid(Pid::from_raw(-1), None) {
            Ok(WaitStatus::Exited(_, _)) | Ok(WaitStatus::Signaled(_, _, _)) => continue,
            Ok(_) => continue,
            Err(nix::errno::Errno::ECHILD) => break, // No more children
            Err(_) => break,
        }
    }

    std::process::exit(exit_code);
}
