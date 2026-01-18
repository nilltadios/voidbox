//! Manifest schema definitions

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Complete app manifest structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppManifest {
    pub app: AppInfo,
    pub source: SourceConfig,
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub dependencies: DependencyConfig,
    pub binary: BinaryConfig,
    #[serde(default)]
    pub desktop: DesktopConfig,
    #[serde(default)]
    pub permissions: PermissionConfig,
}

/// Basic app information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInfo {
    pub name: String,
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
}

/// Source configuration for downloading the app
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SourceConfig {
    /// GitHub releases
    Github {
        owner: String,
        repo: String,
        #[serde(default)]
        asset_pattern: Option<String>,
        #[serde(default = "default_linux")]
        asset_os: String,
        #[serde(default = "default_arch")]
        asset_arch: String,
        #[serde(default)]
        asset_extension: Option<String>,
    },
    /// Direct download URL
    Direct {
        url: String,
        #[serde(default)]
        version_url: Option<String>,
    },
    /// Local file path (for testing)
    Local { path: PathBuf },
}

fn default_linux() -> String {
    "linux".to_string()
}

fn default_arch() -> String {
    "amd64".to_string()
}

/// Runtime configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    #[serde(default = "default_base")]
    pub base: String,
    #[serde(default)]
    pub arch: Vec<String>,
}

fn default_base() -> String {
    "ubuntu:24.04".to_string()
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            base: default_base(),
            arch: vec!["x86_64".to_string()],
        }
    }
}

/// Dependency configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DependencyConfig {
    #[serde(default)]
    pub packages: Vec<String>,
}

/// Binary configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryConfig {
    pub name: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub install_dir: Option<String>,
}

/// Desktop entry configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DesktopConfig {
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub wm_class: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub mime_types: Vec<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
}

/// Permission configuration - all default to true (open by default)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionConfig {
    #[serde(default = "default_true")]
    pub network: bool,
    #[serde(default = "default_true")]
    pub audio: bool,
    #[serde(default = "default_true")]
    pub microphone: bool,
    #[serde(default = "default_true")]
    pub gpu: bool,
    #[serde(default)]
    pub camera: bool,
    #[serde(default = "default_true")]
    pub home: bool,
    #[serde(default = "default_true")]
    pub downloads: bool,
    #[serde(default)]
    pub removable_media: bool,
    #[serde(default)]
    pub dev_mode: bool,
    #[serde(default = "default_true")]
    pub fonts: bool,
    #[serde(default = "default_true")]
    pub themes: bool,
}

fn default_true() -> bool {
    true
}

impl Default for PermissionConfig {
    fn default() -> Self {
        Self {
            network: true,
            audio: true,
            microphone: true,
            gpu: true,
            camera: false,
            home: true,
            downloads: true,
            removable_media: false,
            dev_mode: false,
            fonts: true,
            themes: true,
        }
    }
}

/// Archive type for the app distribution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ArchiveType {
    #[default]
    Zip,
    TarGz,
    TarXz,
    TarZst,
}

impl ArchiveType {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "zip" => Some(Self::Zip),
            "tar.gz" | "tgz" => Some(Self::TarGz),
            "tar.xz" | "txz" => Some(Self::TarXz),
            "tar.zst" | "tzst" => Some(Self::TarZst),
            _ => None,
        }
    }

    pub fn extension(&self) -> &'static str {
        match self {
            Self::Zip => ".zip",
            Self::TarGz => ".tar.gz",
            Self::TarXz => ".tar.xz",
            Self::TarZst => ".tar.zst",
        }
    }
}

/// Installed app information (stored in db)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstalledApp {
    pub name: String,
    pub display_name: String,
    pub version: Option<String>,
    pub base_version: Option<String>,
    pub installed_date: Option<String>,
    pub manifest_path: Option<PathBuf>,
}
