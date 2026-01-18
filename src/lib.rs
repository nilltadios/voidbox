//! Voidbox - Universal Linux App Platform
//!
//! A portable, isolated application environment using Linux user namespaces.

pub mod cli;
pub mod desktop;
pub mod gui;
pub mod manifest;
pub mod runtime;
pub mod settings;
pub mod storage;

pub use manifest::AppManifest;
pub use storage::paths;

/// Application version from Cargo.toml
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Application name
pub const APP_NAME: &str = "voidbox";

/// Default registry URL
pub const DEFAULT_REGISTRY: &str = "https://voidbox.dev";

/// GitHub owner for self-update
pub const SELF_UPDATE_OWNER: &str = "nilltadios";

/// GitHub repo for self-update
pub const SELF_UPDATE_REPO: &str = "voidbox";

/// Container hostname
pub const CONTAINER_HOSTNAME: &str = "voidbox";

/// Ubuntu releases URL for fetching base images
pub const UBUNTU_RELEASES_URL: &str = "https://cdimage.ubuntu.com/ubuntu-base/releases/";
