//! Desktop entry (.desktop file) generation

use crate::manifest::AppManifest;
use crate::storage::paths;
use std::fs;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DesktopError {
    #[error("Failed to create desktop entry: {0}")]
    CreateError(#[from] std::io::Error),
}

/// Generate a .desktop file for an app
pub fn create_desktop_entry(manifest: &AppManifest) -> Result<(), DesktopError> {
    let desktop_path = paths::app_desktop_path(&manifest.app.name);

    if let Some(parent) = desktop_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let icon_path = paths::app_icon_path(&manifest.app.name);
    let icon_value = if icon_path.exists() {
        icon_path.to_string_lossy().to_string()
    } else {
        // Fallback to generic icon
        "application-x-executable".to_string()
    };

    let categories = if manifest.desktop.categories.is_empty() {
        "Application;".to_string()
    } else {
        format!("{};", manifest.desktop.categories.join(";"))
    };

    let wm_class = manifest
        .desktop
        .wm_class
        .clone()
        .unwrap_or_else(|| manifest.app.name.clone());

    let keywords = if manifest.desktop.keywords.is_empty() {
        String::new()
    } else {
        format!("Keywords={}\n", manifest.desktop.keywords.join(";"))
    };

    let mime_types = if manifest.desktop.mime_types.is_empty() {
        String::new()
    } else {
        format!("MimeType={}\n", manifest.desktop.mime_types.join(";"))
    };

    let exec_path = paths::voidbox_exe_path();
    let exec_value = exec_path.to_string_lossy();

    let content = format!(
        r#"[Desktop Entry]
Name={}
Comment={}
Exec={} run {}
Icon={}
Terminal=false
Type=Application
Categories={}
StartupWMClass={}
{}{}
"#,
        manifest.app.display_name,
        manifest.app.description,
        exec_value,
        manifest.app.name,
        icon_value,
        categories,
        wm_class,
        keywords,
        mime_types,
    );

    fs::write(&desktop_path, content)?;

    Ok(())
}

/// Remove a .desktop file for an app
pub fn remove_desktop_entry(app_name: &str) -> Result<(), DesktopError> {
    let desktop_path = paths::app_desktop_path(app_name);
    if desktop_path.exists() {
        fs::remove_file(desktop_path)?;
    }
    Ok(())
}

/// Update desktop database
pub fn update_desktop_database() {
    // This is optional - triggers desktop environment to refresh
    if let Some(dir) = paths::desktop_dir().to_str().map(String::from) {
        let _ = std::process::Command::new("update-desktop-database")
            .arg(dir)
            .output();
    }
}
