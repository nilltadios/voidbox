//! Mount operations for container setup

use crate::manifest::PermissionConfig;
use nix::mount::{MntFlags, MsFlags, mount, umount2};
use nix::unistd::{chdir, pivot_root, sethostname};
use std::fs;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MountError {
    #[error("Mount failed: {0}")]
    MountFailed(String),

    #[error("Pivot root failed: {0}")]
    PivotFailed(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Bind mount configuration
pub struct BindMount {
    pub source: String,
    pub target: String,
    pub readonly: bool,
    pub required: bool,
}

impl BindMount {
    pub fn new(source: &str, target: &str, readonly: bool) -> Self {
        Self {
            source: source.to_string(),
            target: target.to_string(),
            readonly,
            required: true,
        }
    }

    pub fn optional(source: &str, target: &str, readonly: bool) -> Self {
        Self {
            source: source.to_string(),
            target: target.to_string(),
            readonly,
            required: false,
        }
    }
}

/// Get bind mounts based on permissions
pub fn get_bind_mounts(permissions: &PermissionConfig) -> Vec<BindMount> {
    let mut mounts = vec![
        // Essential system mounts
        BindMount::new("/sys", "sys", true),
        BindMount::new("/dev", "dev", false),
        BindMount::new("/tmp", "tmp", false),
    ];

    // XDG_RUNTIME_DIR for audio/Wayland
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        let relative = runtime_dir.trim_start_matches('/');
        mounts.push(BindMount::new(&runtime_dir, relative, false));
    }

    // Home folder mount
    if permissions.home {
        if let Ok(home) = std::env::var("HOME") {
            if let Some(user) = std::env::var("USER").ok() {
                let container_home = format!("home/{}", user);
                mounts.push(BindMount::new(&home, &container_home, false));
            }
        }
    }

    // Font mount
    if permissions.fonts {
        mounts.push(BindMount::optional(
            "/usr/share/fonts",
            "usr/share/fonts",
            true,
        ));
        mounts.push(BindMount::optional(
            "/usr/local/share/fonts",
            "usr/local/share/fonts",
            true,
        ));
    }

    // Theme mount
    if permissions.themes {
        mounts.push(BindMount::optional(
            "/usr/share/themes",
            "usr/share/themes",
            true,
        ));
        mounts.push(BindMount::optional(
            "/usr/share/icons",
            "usr/share/icons",
            true,
        ));
        mounts.push(BindMount::optional(
            "/usr/share/pixmaps",
            "usr/share/pixmaps",
            true,
        ));

        // Also mount GTK/Qt config
        if let Ok(home) = std::env::var("HOME") {
            mounts.push(BindMount::optional(
                &format!("{}/.config/gtk-3.0", home),
                "root/.config/gtk-3.0",
                true,
            ));
            mounts.push(BindMount::optional(
                &format!("{}/.config/gtk-4.0", home),
                "root/.config/gtk-4.0",
                true,
            ));
        }
    }

    // Developer mode - mount host binaries
    if permissions.dev_mode {
        mounts.push(BindMount::optional("/usr/bin", "host/bin", true));
        mounts.push(BindMount::optional(
            "/usr/local/bin",
            "host/local/bin",
            true,
        ));

        // Python packages
        if let Ok(home) = std::env::var("HOME") {
            mounts.push(BindMount::optional(
                &format!("{}/.local/lib", home),
                "host/python-lib",
                true,
            ));
        }

        // Node.js global packages
        if let Ok(home) = std::env::var("HOME") {
            mounts.push(BindMount::optional(
                &format!("{}/.npm", home),
                "host/npm",
                true,
            ));
            mounts.push(BindMount::optional(
                &format!("{}/.nvm", home),
                "host/nvm",
                true,
            ));
        }

        // Cargo/Rust
        if let Ok(home) = std::env::var("HOME") {
            mounts.push(BindMount::optional(
                &format!("{}/.cargo", home),
                "host/cargo",
                true,
            ));
            mounts.push(BindMount::optional(
                &format!("{}/.rustup", home),
                "host/rustup",
                true,
            ));
        }
    }

    mounts
}

/// Setup container filesystem with bind mounts
pub fn setup_container_mounts(
    rootfs: &Path,
    permissions: &PermissionConfig,
) -> Result<(), MountError> {
    // Make root private
    mount(
        None::<&str>,
        "/",
        None::<&str>,
        MsFlags::MS_PRIVATE | MsFlags::MS_REC,
        None::<&str>,
    )
    .map_err(|e| MountError::MountFailed(format!("make root private: {}", e)))?;

    // Bind mount rootfs to itself
    mount(
        Some(rootfs),
        rootfs,
        None::<&str>,
        MsFlags::MS_BIND | MsFlags::MS_REC,
        None::<&str>,
    )
    .map_err(|e| MountError::MountFailed(format!("bind rootfs: {}", e)))?;

    chdir(rootfs).map_err(|e| MountError::MountFailed(format!("chdir to rootfs: {}", e)))?;

    // Apply bind mounts
    for bind_mount in get_bind_mounts(permissions) {
        let source = Path::new(&bind_mount.source);
        let target = rootfs.join(&bind_mount.target);

        if !source.exists() {
            if bind_mount.required {
                return Err(MountError::MountFailed(format!(
                    "required mount source missing: {}",
                    bind_mount.source
                )));
            }
            continue;
        }

        // Create target directory
        if source.is_dir() {
            fs::create_dir_all(&target)?;
        } else if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut flags = MsFlags::MS_BIND | MsFlags::MS_REC;
        if bind_mount.readonly {
            flags |= MsFlags::MS_RDONLY;
        }

        if let Err(e) = mount(Some(source), &target, None::<&str>, flags, None::<&str>) {
            if bind_mount.required {
                return Err(MountError::MountFailed(format!(
                    "bind {} -> {}: {}",
                    bind_mount.source, bind_mount.target, e
                )));
            }
            // Optional mounts can fail silently
        }
    }

    Ok(())
}

/// Perform pivot_root to switch to container filesystem
pub fn pivot_to_container(rootfs: &Path) -> Result<(), MountError> {
    let old_root = rootfs.join("old_root");
    fs::create_dir_all(&old_root)?;

    pivot_root(".", "old_root")
        .map_err(|e| MountError::PivotFailed(format!("pivot_root: {}", e)))?;

    chdir("/").map_err(|e| MountError::PivotFailed(format!("chdir /: {}", e)))?;

    // Mount proc
    if !Path::new("/proc").exists() {
        fs::create_dir("/proc")?;
    }
    mount(
        Some("proc"),
        "/proc",
        Some("proc"),
        MsFlags::empty(),
        None::<&str>,
    )
    .map_err(|e| MountError::MountFailed(format!("mount /proc: {}", e)))?;

    // Cleanup old root
    umount2("/old_root", MntFlags::MNT_DETACH)
        .map_err(|e| MountError::MountFailed(format!("umount old_root: {}", e)))?;
    fs::remove_dir("/old_root")?;

    // Set hostname
    sethostname(crate::CONTAINER_HOSTNAME)
        .map_err(|e| MountError::MountFailed(format!("sethostname: {}", e)))?;

    Ok(())
}

/// Setup environment variables for container
pub fn setup_container_env() {
    unsafe {
        std::env::set_var(
            "PATH",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/host/bin:/host/local/bin",
        );

        // Set HOME based on whether we mounted user's home
        if let Ok(user) = std::env::var("USER") {
            let home_path = format!("/home/{}", user);
            if Path::new(&home_path).exists() {
                std::env::set_var("HOME", &home_path);
            } else {
                std::env::set_var("HOME", "/root");
            }
        } else {
            std::env::set_var("HOME", "/root");
        }

        // XDG runtime
        if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
            let relative = runtime_dir.trim_start_matches('/');
            std::env::set_var("XDG_RUNTIME_DIR", format!("/{}", relative));
            std::env::set_var("PULSE_SERVER", format!("unix:/{}/pulse/native", relative));
        }

        // X11/Wayland display - DISPLAY is inherited from parent, just ensure it's set
        // The /tmp/.X11-unix socket is already mounted via /tmp bind mount
        if std::env::var("DISPLAY").is_err() {
            // Default to :0 if not set
            std::env::set_var("DISPLAY", ":0");
        }

        // Wayland socket (if using Wayland)
        if let Ok(wayland_display) = std::env::var("WAYLAND_DISPLAY") {
            std::env::set_var("WAYLAND_DISPLAY", wayland_display);
        }
    }
}
