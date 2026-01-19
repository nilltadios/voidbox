//! Linux namespace setup

use nix::sched::{CloneFlags, unshare};
use nix::unistd::{getgid, getuid};
use std::fs;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum NamespaceError {
    #[error("Failed to unshare namespace: {0}")]
    UnshareError(String),

    #[error("Failed to write UID/GID map: {0}")]
    MappingError(#[from] std::io::Error),
}

/// Setup user namespace with UID/GID mapping
pub fn setup_user_namespace(_native_mode: bool) -> Result<(), NamespaceError> {
    let uid = getuid();
    let gid = getgid();

    // Create user namespace first
    unshare(CloneFlags::CLONE_NEWUSER)
        .map_err(|e| NamespaceError::UnshareError(format!("CLONE_NEWUSER: {}", e)))?;

    // Map root (uid 0) inside to real user outside
    // This gives us CAP_SYS_ADMIN inside the namespace for mount operations
    // Note: Files owned by the real user will appear as "nobody" inside,
    // but the process can still access them since it maps to the same uid.
    let uid_map = format!("0 {} 1", uid);
    let gid_map = format!("0 {} 1", gid);

    fs::write("/proc/self/uid_map", &uid_map)?;
    fs::write("/proc/self/setgroups", "deny")?;
    fs::write("/proc/self/gid_map", &gid_map)?;

    Ok(())
}

/// Setup remaining namespaces (mount, PID, UTS, IPC)
pub fn setup_container_namespaces() -> Result<(), NamespaceError> {
    unshare(
        CloneFlags::CLONE_NEWNS
            | CloneFlags::CLONE_NEWUTS
            | CloneFlags::CLONE_NEWIPC
            | CloneFlags::CLONE_NEWPID,
    )
    .map_err(|e| NamespaceError::UnshareError(format!("container namespaces: {}", e)))?;

    Ok(())
}
