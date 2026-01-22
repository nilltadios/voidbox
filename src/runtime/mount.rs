//! Mount operations for container setup

use crate::manifest::PermissionConfig;
use crate::storage::{paths, read_base_info_for_rootfs};
use nix::mount::{MntFlags, MsFlags, mount, umount2};
use nix::unistd::{chdir, pivot_root, sethostname};
use std::fs;
use std::io::{Read, Write};
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

    // Native mode - mount host's /usr, /lib, /etc for full compatibility
    if permissions.native_mode {
        // /run for DNS and other runtime data (must be before XDG_RUNTIME_DIR)
        mounts.push(BindMount::optional("/run", "run", true));

        // XDG_RUNTIME_DIR for audio/Wayland (RW over /run)
        if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
            let relative = runtime_dir.trim_start_matches('/');
            mounts.push(BindMount::new(&runtime_dir, relative, false));
        }

        // Mount entire host userspace (read-only for safety)
        mounts.push(BindMount::optional("/usr", "usr", true));
        mounts.push(BindMount::optional("/lib", "lib", true));
        mounts.push(BindMount::optional("/lib64", "lib64", true));
        mounts.push(BindMount::optional("/etc", "etc", true));
        mounts.push(BindMount::optional("/bin", "bin", true));
        mounts.push(BindMount::optional("/sbin", "sbin", true));
        // /var for various tools
        mounts.push(BindMount::optional("/var", "var", true));
        // Mount home writable
        if let Ok(home) = std::env::var("HOME") {
            if let Some(user) = std::env::var("USER").ok() {
                let container_home = format!("home/{}", user);
                mounts.push(BindMount::new(&home, &container_home, false));
            }
        }
        return mounts;
    }

    // XDG_RUNTIME_DIR for audio/Wayland (standard mode)
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

    // Developer mode - mount host binaries and tools
    if permissions.dev_mode {
        mounts.push(BindMount::optional("/usr/bin", "host/bin", true));
        mounts.push(BindMount::optional(
            "/usr/local/bin",
            "host/local/bin",
            true,
        ));
        // Node global modules (for gemini, claude symlinks)
        mounts.push(BindMount::optional(
            "/usr/local/lib",
            "host/local/lib",
            true,
        ));

        if let Ok(home) = std::env::var("HOME") {
            // User's local bin (pip, gemini, claude, etc.)
            mounts.push(BindMount::optional(
                &format!("{}/.local/bin", home),
                &format!("host/user/bin"),
                true,
            ));

            // Python packages and pyenv
            mounts.push(BindMount::optional(
                &format!("{}/.local/lib", home),
                "host/python-lib",
                true,
            ));
            mounts.push(BindMount::optional(
                &format!("{}/.pyenv", home),
                &format!("{}.pyenv", home.trim_start_matches('/')),
                true,
            ));

            // Node.js global packages
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
            // Also mount nvm to same path for shebang compatibility
            mounts.push(BindMount::optional(
                &format!("{}/.nvm", home),
                &format!("{}.nvm", home.trim_start_matches('/')),
                true,
            ));

            // Cargo/Rust
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
            // Mount cargo bin to same path for shebang compatibility
            mounts.push(BindMount::optional(
                &format!("{}/.cargo/bin", home),
                &format!("{}.cargo/bin", home.trim_start_matches('/')),
                true,
            ));
        }
    }

    mounts
}

fn try_mount_overlay(rootfs: &Path) -> Result<bool, MountError> {
    let Some(info) = read_base_info_for_rootfs(rootfs)
        .map_err(|e| MountError::MountFailed(format!("base info: {}", e)))?
    else {
        return Ok(false);
    };

    let base_dir = paths::base_dir(&info.base, &info.arch);
    if !base_dir.exists() {
        return Err(MountError::MountFailed(format!(
            "base image missing: {}",
            base_dir.display()
        )));
    }

    let app_dir = rootfs.parent().ok_or_else(|| {
        MountError::MountFailed(format!("invalid rootfs path: {}", rootfs.display()))
    })?;
    let layer_dir = app_dir.join("layer");
    let work_dir = app_dir.join("work");

    fs::create_dir_all(&layer_dir)?;
    fs::create_dir_all(&work_dir)?;

    let mut lowerdir = base_dir.display().to_string();

    if let Some(deps_id) = &info.deps_id {
        let deps_rootfs = paths::deps_rootfs_dir(deps_id);
        let deps_layer = paths::deps_layer_dir(deps_id);
        let deps_work = paths::deps_work_dir(deps_id);

        fs::create_dir_all(&deps_rootfs)?;
        fs::create_dir_all(&deps_layer)?;
        fs::create_dir_all(&deps_work)?;

        let deps_marker = deps_rootfs.join("etc/os-release");
        if !deps_marker.exists() {
            let base_lower = base_dir.display().to_string();
            if let Err(err) = mount_overlay_with_fallback(
                &deps_rootfs,
                &base_lower,
                &deps_layer,
                &deps_work,
            ) {
                eprintln!(
                    "[voidbox] Warning: deps overlay mount failed: {}",
                    err
                );
            }
        }

        if deps_marker.exists() {
            lowerdir = deps_rootfs.display().to_string();
        } else {
            lowerdir = format!("{}:{}", deps_layer.display(), base_dir.display());
        }
    }

    mount_overlay_with_fallback(rootfs, &lowerdir, &layer_dir, &work_dir)?;

    Ok(true)
}

fn mount_overlay_with_fallback(
    target: &Path,
    lowerdir: &str,
    upperdir: &Path,
    workdir: &Path,
) -> Result<(), MountError> {
    let base_opts = format!(
        "lowerdir={},upperdir={},workdir={}",
        lowerdir,
        upperdir.display(),
        workdir.display()
    );
    let opts_with_xattr = format!("{},userxattr", base_opts);

    if mount(
        Some("overlay"),
        target,
        Some("overlay"),
        MsFlags::empty(),
        Some(opts_with_xattr.as_str()),
    )
    .is_ok()
    {
        return Ok(());
    }

    mount(
        Some("overlay"),
        target,
        Some("overlay"),
        MsFlags::empty(),
        Some(base_opts.as_str()),
    )
    .map_err(|e| MountError::MountFailed(format!("overlay mount failed: {}", e)))
}

/// Generate synthetic /etc/passwd content that preserves system users but maps UID 0 to host username
fn generate_passwd_content(rootfs: &Path) -> Result<String, std::io::Error> {
    let mut content = String::new();
    let etc_passwd = rootfs.join("etc/passwd");

    if etc_passwd.exists() {
        let mut file = fs::File::open(&etc_passwd)?;
        file.read_to_string(&mut content)?;
    }

    let username = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
    let home = std::env::var("HOME").unwrap_or_else(|_| format!("/home/{}", username));

    let mut new_content = String::new();

    // Filter out existing root entry if present, keep others
    for line in content.lines() {
        if line.starts_with("root:") {
            continue;
        }
        new_content.push_str(line);
        new_content.push('\n');
    }

    // Map UID 0 to the host username so whoami returns the correct name
    // Format: name:password:uid:gid:gecos:home:shell
    new_content.push_str(&format!(
        "{}:x:0:0:{}:/{}:/bin/bash\n",
        username,
        username,
        home.trim_start_matches('/')
    ));

    Ok(new_content)
}

/// Generate synthetic /etc/group content
fn generate_group_content(rootfs: &Path) -> Result<String, std::io::Error> {
    let mut content = String::new();
    let etc_group = rootfs.join("etc/group");

    if etc_group.exists() {
        let mut file = fs::File::open(&etc_group)?;
        file.read_to_string(&mut content)?;
    }

    let username = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
    let mut new_content = String::new();

    // Filter out existing root group
    for line in content.lines() {
        if line.starts_with("root:") {
            continue;
        }
        new_content.push_str(line);
        new_content.push('\n');
    }

    // Map GID 0 to a group named after the user
    new_content.push_str(&format!("{}:x:0:{}\n", username, username));

    Ok(new_content)
}

/// Setup synthetic passwd/group files in container for native feel
pub fn setup_user_identity(rootfs: &Path) -> Result<(), MountError> {
    let username = std::env::var("USER").unwrap_or_else(|_| "user".to_string());

    // Create .voidbox directory for our synthetic files
    let voidbox_dir = rootfs.join(".voidbox");
    fs::create_dir_all(&voidbox_dir)?;

    // Write synthetic passwd
    let passwd_path = voidbox_dir.join("passwd");
    let mut passwd_file = fs::File::create(&passwd_path)?;
    passwd_file.write_all(generate_passwd_content(rootfs)?.as_bytes())?;

    // Write synthetic group
    let group_path = voidbox_dir.join("group");
    let mut group_file = fs::File::create(&group_path)?;
    group_file.write_all(generate_group_content(rootfs)?.as_bytes())?;

    // Bind mount over /etc/passwd and /etc/group
    let etc_passwd = rootfs.join("etc/passwd");
    let etc_group = rootfs.join("etc/group");

    // Ensure /etc exists
    fs::create_dir_all(rootfs.join("etc"))?;

    // Create empty target files if they don't exist (for bind mount)
    if !etc_passwd.exists() {
        fs::File::create(&etc_passwd)?;
    }
    if !etc_group.exists() {
        fs::File::create(&etc_group)?;
    }

    // Bind mount synthetic files
    mount(
        Some(&passwd_path),
        &etc_passwd,
        None::<&str>,
        MsFlags::MS_BIND,
        None::<&str>,
    )
    .map_err(|e| MountError::MountFailed(format!("bind passwd: {}", e)))?;

    mount(
        Some(&group_path),
        &etc_group,
        None::<&str>,
        MsFlags::MS_BIND,
        None::<&str>,
    )
    .map_err(|e| MountError::MountFailed(format!("bind group: {}", e)))?;

    eprintln!(
        "[voidbox] User identity: {} (native feel enabled)",
        username
    );

    Ok(())
}

/// Setup container filesystem with bind mounts
pub fn setup_container_mounts(
    rootfs: &Path,
    permissions: &PermissionConfig,
) -> Result<(), MountError> {
    fs::create_dir_all(rootfs)?;

    // Make root private
    mount(
        None::<&str>,
        "/",
        None::<&str>,
        MsFlags::MS_PRIVATE | MsFlags::MS_REC,
        None::<&str>,
    )
    .map_err(|e| MountError::MountFailed(format!("make root private: {}", e)))?;

    // Try to mount overlay (shared base + per-app layer)
    if !try_mount_overlay(rootfs)? {
        // Fallback: bind mount rootfs to itself (legacy mode)
        mount(
            Some(rootfs),
            rootfs,
            None::<&str>,
            MsFlags::MS_BIND | MsFlags::MS_REC,
            None::<&str>,
        )
        .map_err(|e| MountError::MountFailed(format!("bind rootfs: {}", e)))?;
    }

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
pub fn pivot_to_container(rootfs: &Path, permissions: &PermissionConfig) -> Result<(), MountError> {
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

    // Set hostname - skip in native mode to preserve host hostname
    if !permissions.native_mode {
        sethostname(crate::CONTAINER_HOSTNAME)
            .map_err(|e| MountError::MountFailed(format!("sethostname: {}", e)))?;
    }

    Ok(())
}

/// Setup the sudo shim and other host bridge scripts in the container
/// This must be called AFTER pivot_root when we're inside the container
pub fn setup_host_bridge_shims(port: u16, token: &str) -> Result<(), MountError> {
    // Create /.voidbox/bin for our shims
    let shim_dir = Path::new("/.voidbox/bin");
    fs::create_dir_all(shim_dir)?;

    // Create the sudo shim script with full interactive PTY support
    let sudo_shim = format!(
        r#"#!/bin/bash
# VoidBox sudo shim - bridges to host for privileged operations with full PTY
# Port: {}

PORT={}
TOKEN="{}"
CMD="$*"

# Connect to host bridge
exec 3<>/dev/tcp/127.0.0.1/$PORT 2>/dev/null
if [ $? -ne 0 ]; then
    echo "voidbox: Cannot connect to host bridge on port $PORT" >&2
    exit 1
fi

# Cleanup on exit (kills background cat)
trap "kill \$stdin_pid 2>/dev/null; exec 3<&-" EXIT

# Send authentication token
echo "$TOKEN" >&3

# Send the command
echo "SUDO $CMD" >&3

# Forward stdin to socket in background
cat <&0 >&3 2>/dev/null &
stdin_pid=$!

# Forward socket to stdout (this blocks until connection closes)
cat <&3 2>/dev/null

exit 0
"#,
        port, port, token
    );

    let sudo_path = shim_dir.join("sudo");
    let mut sudo_file = fs::File::create(&sudo_path)?;
    sudo_file.write_all(sudo_shim.as_bytes())?;

    // Make executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&sudo_path, fs::Permissions::from_mode(0o755))?;
    }

    // Also create a host-exec shim for running arbitrary commands on host
    let host_exec_shim = format!(
        r#"#!/bin/bash
# VoidBox host-exec - run commands on the host system with full PTY
PORT={}
TOKEN="{}"
CMD="$*"

exec 3<>/dev/tcp/127.0.0.1/$PORT 2>/dev/null
if [ $? -ne 0 ]; then
    echo "voidbox: Cannot connect to host bridge" >&2
    exit 1
fi

trap "kill \$stdin_pid 2>/dev/null; exec 3<&-" EXIT

echo "$TOKEN" >&3
echo "EXEC $CMD" >&3

cat <&0 >&3 2>/dev/null &
stdin_pid=$!

cat <&3 2>/dev/null

exit 0
"#,
        port, token
    );

    let host_exec_path = shim_dir.join("host-exec");
    let mut host_exec_file = fs::File::create(&host_exec_path)?;
    host_exec_file.write_all(host_exec_shim.as_bytes())?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&host_exec_path, fs::Permissions::from_mode(0o755))?;
    }

    Ok(())
}

/// Setup environment variables for container
pub fn setup_container_env(permissions: &PermissionConfig) {
    unsafe {
        // In native mode, preserve the host PATH but prepend our shim directory
        if permissions.native_mode {
            let current_path = std::env::var("PATH").unwrap_or_else(|_| {
                "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".to_string()
            });
            // Prepend /.voidbox/bin so our sudo shim takes precedence
            std::env::set_var("PATH", format!("/.voidbox/bin:{}", current_path));
        } else {
            std::env::set_var(
                "PATH",
                "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/host/bin:/host/local/bin:/host/user/bin",
            );
        }

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

        // D-Bus session bus (for file dialogs via xdg-desktop-portal, notifications, etc.)
        if let Ok(dbus_addr) = std::env::var("DBUS_SESSION_BUS_ADDRESS") {
            std::env::set_var("DBUS_SESSION_BUS_ADDRESS", dbus_addr);
        }
    }
}
