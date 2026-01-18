// =============================================================================
// APP CONFIGURATION - Edit this file to customize for your application
// =============================================================================
//
// To fork void_runner for a different app (Discord, Firefox, etc.):
// 1. Edit the constants below
// 2. Update DEPENDENCIES for your app's requirements
// 3. Update Cargo.toml package name/version if desired
// 4. Build and release!

// -----------------------------------------------------------------------------
// App Identity
// -----------------------------------------------------------------------------

/// Internal name used for binary, data directory, etc.
pub const APP_NAME: &str = "void_runner";

/// Display name shown in desktop launcher and messages
pub const APP_DISPLAY_NAME: &str = "Void Runner";

/// Short description for CLI help and desktop file
pub const APP_DESCRIPTION: &str = "Portable Isolated Brave Browser";

/// Name of the target application being containerized
pub const TARGET_APP_NAME: &str = "Brave";

// -----------------------------------------------------------------------------
// GitHub Releases - Target Application
// -----------------------------------------------------------------------------

/// GitHub API URL for fetching target app releases
/// Set to None if using a custom download source
pub const RELEASES_API: Option<&str> = Some("https://api.github.com/repos/brave/brave-browser/releases/latest");

/// For matching release assets - customize these for your app
pub const ASSET_OS_PATTERN: &str = "linux";
pub const ASSET_ARCH_PATTERN: &str = "amd64";
pub const ASSET_EXTENSION: &str = ".zip";

// -----------------------------------------------------------------------------
// GitHub Releases - Self Update
// -----------------------------------------------------------------------------

/// GitHub owner for self-update releases
pub const SELF_UPDATE_OWNER: &str = "nilltadios";

/// GitHub repo for self-update releases
pub const SELF_UPDATE_REPO: &str = "brave_box";

// -----------------------------------------------------------------------------
// Target Application
// -----------------------------------------------------------------------------

/// Binary name to search for in extracted archive
pub const TARGET_BINARY_NAME: &str = "brave";

/// Default arguments when launching the target app
pub const DEFAULT_LAUNCH_ARGS: &[&str] = &[
    "--no-sandbox",
    "--disable-dev-shm-usage",
    "--test-type",
];

/// Installation directory inside the container (under /opt/)
pub const TARGET_INSTALL_DIR: &str = "brave";

/// Icon filename inside the extracted app directory
pub const TARGET_ICON_FILENAME: &str = "product_logo_128.png";

// -----------------------------------------------------------------------------
// Desktop Entry
// -----------------------------------------------------------------------------

/// Desktop entry categories (semicolon-separated)
pub const DESKTOP_CATEGORIES: &str = "Network;WebBrowser;";

/// WM_CLASS for window matching
pub const DESKTOP_WM_CLASS: &str = "brave-browser";

/// Fallback icon name if app icon not found
pub const DESKTOP_FALLBACK_ICON: &str = "web-browser";

// -----------------------------------------------------------------------------
// Container Hostname
// -----------------------------------------------------------------------------

/// Hostname set inside the container
pub const CONTAINER_HOSTNAME: &str = "void-runner";

// -----------------------------------------------------------------------------
// Dependencies
// -----------------------------------------------------------------------------

/// Ubuntu/Debian packages required by the target application
/// These are installed via apt-get in the container
pub const DEPENDENCIES: &str = r#"
    curl unzip \
    libnss3 libatk1.0-0t64 libatk-bridge2.0-0t64 \
    libcups2t64 libdrm2 libxkbcommon0 libxcomposite1 libxdamage1 libxfixes3 \
    libxrandr2 libgbm1 libpango-1.0-0 libcairo2 libasound2t64 libx11-xcb1 \
    libx11-6 libxcb1 libdbus-1-3 libglib2.0-0t64 libgtk-3-0t64 libgl1-mesa-dri \
    mesa-vulkan-drivers libegl1 libgles2 libpulse0 \
    libasound2-plugins fonts-liberation dconf-gsettings-backend
"#;

// -----------------------------------------------------------------------------
// Archive Handling
// -----------------------------------------------------------------------------

/// Type of archive the target app is distributed as
#[derive(Clone, Copy, PartialEq)]
#[allow(dead_code)]  // Variants available for forked apps
pub enum ArchiveType {
    Zip,
    TarGz,
    TarXz,
}

/// Archive type for the target application
pub const TARGET_ARCHIVE_TYPE: ArchiveType = ArchiveType::Zip;

// -----------------------------------------------------------------------------
// Custom Download Logic (Optional)
// -----------------------------------------------------------------------------
// If your app doesn't use GitHub releases, implement custom fetch logic here.
// Set RELEASES_API to None and uncomment/implement these functions.

/*
pub fn fetch_custom_release() -> Result<(String, String), Box<dyn std::error::Error>> {
    // Return (version, download_url)
    // Example for Firefox:
    // let version = fetch_firefox_version()?;
    // let url = format!("https://download.mozilla.org/?product=firefox-{}-SSL&os=linux64&lang=en-US", version);
    // Ok((version, url))
    unimplemented!("Implement custom download logic")
}
*/
