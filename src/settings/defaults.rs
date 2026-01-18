//! Default settings and permission management

use crate::manifest::PermissionConfig;

/// Get default permissions (all open by default)
pub fn default_permissions() -> PermissionConfig {
    PermissionConfig::default()
}

/// Merge manifest permissions with user overrides
pub fn merge_permissions(
    manifest: &PermissionConfig,
    overrides: Option<&PermissionConfig>,
) -> PermissionConfig {
    match overrides {
        Some(ov) => PermissionConfig {
            network: ov.network,
            audio: ov.audio,
            microphone: ov.microphone,
            gpu: ov.gpu,
            camera: ov.camera,
            home: ov.home,
            downloads: ov.downloads,
            removable_media: ov.removable_media,
            dev_mode: ov.dev_mode,
            fonts: ov.fonts,
            themes: ov.themes,
        },
        None => manifest.clone(),
    }
}
