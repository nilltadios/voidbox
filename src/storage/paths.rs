//! Directory path management for Voidbox

use std::path::PathBuf;

/// Get the base data directory (~/.local/share/voidbox)
pub fn data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(crate::APP_NAME)
}

/// Get the bases directory (shared base images)
pub fn bases_dir() -> PathBuf {
    data_dir().join("bases")
}

/// Get the apps directory (per-app layers)
pub fn apps_dir() -> PathBuf {
    data_dir().join("apps")
}

/// Get a specific app's directory
pub fn app_dir(app_name: &str) -> PathBuf {
    apps_dir().join(app_name)
}

/// Get app's layer directory (for OverlayFS upper layer)
pub fn app_layer_dir(app_name: &str) -> PathBuf {
    app_dir(app_name).join("layer")
}

/// Get app's rootfs directory (merged view / direct install)
pub fn app_rootfs_dir(app_name: &str) -> PathBuf {
    app_dir(app_name).join("rootfs")
}

/// Get app's work directory (for OverlayFS)
pub fn app_work_dir(app_name: &str) -> PathBuf {
    app_dir(app_name).join("work")
}

/// Get the manifests directory
pub fn manifests_dir() -> PathBuf {
    data_dir().join("manifests")
}

/// Get a specific app's manifest path
pub fn manifest_path(app_name: &str) -> PathBuf {
    manifests_dir().join(format!("{}.toml", app_name))
}

/// Get the settings directory (user overrides)
pub fn settings_dir() -> PathBuf {
    data_dir().join("settings")
}

/// Get a specific app's settings path
pub fn app_settings_path(app_name: &str) -> PathBuf {
    settings_dir().join(format!("{}.toml", app_name))
}

/// Get the icons directory
pub fn icons_dir() -> PathBuf {
    data_dir().join("icons")
}

/// Get a specific app's icon path
pub fn app_icon_path(app_name: &str) -> PathBuf {
    icons_dir().join(format!("{}.png", app_name))
}

/// Get the desktop files directory
pub fn desktop_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("applications")
}

/// Get a specific app's desktop file path
pub fn app_desktop_path(app_name: &str) -> PathBuf {
    desktop_dir().join(format!("voidbox-{}.desktop", app_name))
}

/// Get the bin directory for symlinks
pub fn bin_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local/bin")
}

/// Get voidbox install path
pub fn install_path() -> PathBuf {
    bin_dir().join(crate::APP_NAME)
}

/// Get the installed apps database path
pub fn database_path() -> PathBuf {
    data_dir().join("installed.json")
}

/// Ensure all required directories exist
pub fn ensure_dirs() -> std::io::Result<()> {
    std::fs::create_dir_all(data_dir())?;
    std::fs::create_dir_all(bases_dir())?;
    std::fs::create_dir_all(apps_dir())?;
    std::fs::create_dir_all(manifests_dir())?;
    std::fs::create_dir_all(settings_dir())?;
    std::fs::create_dir_all(icons_dir())?;
    std::fs::create_dir_all(desktop_dir())?;
    std::fs::create_dir_all(bin_dir())?;
    Ok(())
}
