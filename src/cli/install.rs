//! Install command implementation

use crate::desktop::{create_app_wrapper, create_desktop_entry, extract_icon};
use crate::manifest::{
    AppManifest, ArchiveType, InstalledApp, SourceConfig, parse_manifest_file, parse_manifest_str,
    parse_manifest_url, validate_manifest,
};
use crate::storage::{BaseInfo, download_file, paths, write_base_info};
use flate2::read::GzDecoder;
use serde::Deserialize;
use std::fs::{self, File};
use std::path::Path;
use std::process::{Command, Stdio};
use thiserror::Error;
use walkdir::WalkDir;

#[derive(Error, Debug)]
pub enum InstallError {
    #[error("Manifest error: {0}")]
    ManifestError(#[from] crate::manifest::ManifestError),

    #[error("Download error: {0}")]
    DownloadError(#[from] crate::storage::DownloadError),

    #[error("Base info error: {0}")]
    BaseInfoError(#[from] crate::storage::BaseInfoError),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Installation failed: {0}")]
    Failed(String),

    #[error("App already installed: {0}")]
    AlreadyInstalled(String),
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

/// Install an app from a manifest source
pub fn install_app(source: &str, force: bool) -> Result<(), InstallError> {
    println!("[voidbox] Installing from {}...", source);

    // Parse manifest based on source type
    let manifest = if source.starts_with("http://") || source.starts_with("https://") {
        parse_manifest_url(source)?
    } else if Path::new(source).exists() {
        parse_manifest_file(Path::new(source))?
    } else {
        // Try to find in local manifests directory
        let manifest_path = paths::manifest_path(source);
        if manifest_path.exists() {
            parse_manifest_file(&manifest_path)?
        } else {
            // TODO: Try registry lookup
            return Err(InstallError::Failed(format!(
                "Manifest not found: {}. Try 'voidbox install ./manifest.toml' or a URL.",
                source
            )));
        }
    };

    install_app_from_manifest(&manifest, force)
}

/// Install an app from an already-parsed manifest
pub fn install_app_from_manifest(manifest: &AppManifest, force: bool) -> Result<(), InstallError> {
    validate_manifest(manifest)?;

    let app_name = &manifest.app.name;
    let app_dir = paths::app_dir(app_name);

    // Check if already installed
    if app_dir.exists() && !force {
        return Err(InstallError::AlreadyInstalled(app_name.clone()));
    }

    // Create directories
    paths::ensure_dirs()?;
    fs::create_dir_all(&app_dir)?;

    // Save manifest locally
    let manifest_path = paths::manifest_path(app_name);
    let manifest_content = toml::to_string_pretty(&manifest)
        .map_err(|e| InstallError::Failed(format!("Failed to serialize manifest: {}", e)))?;
    fs::write(&manifest_path, manifest_content)?;

    let rootfs = paths::app_rootfs_dir(app_name);
    let layer_dir = paths::app_layer_dir(app_name);
    let work_dir = paths::app_work_dir(app_name);
    let base_info_path = paths::app_base_info_path(app_name);

    let (install_root, base_version) = if rootfs.join("etc/os-release").exists()
        && !base_info_path.exists()
    {
        println!("[voidbox] Existing rootfs detected - using legacy mode.");
        fs::create_dir_all(&rootfs)?;
        (rootfs.clone(), None)
    } else {
        let arch = detect_ubuntu_arch()?;
        let base_dir = paths::base_dir(&manifest.runtime.base, &arch);
        let base_version = setup_base_image(&base_dir, &arch)?;

        write_base_info(
            app_name,
            &BaseInfo {
                base: manifest.runtime.base.clone(),
                arch: arch.clone(),
                version: base_version.clone(),
            },
        )?;

        fs::create_dir_all(&rootfs)?;
        fs::create_dir_all(&layer_dir)?;
        fs::create_dir_all(&work_dir)?;
        (layer_dir.clone(), Some(base_version))
    };

    // Install dependencies
    install_dependencies(&rootfs, &install_root, &manifest)?;

    // Download and install the app (returns actual version downloaded)
    let actual_version = install_app_binary(&install_root, &manifest)?;

    // Extract icon
    let icon_filename = manifest.desktop.icon.as_deref();
    if let Err(e) = extract_icon(app_name, icon_filename) {
        println!("[voidbox] Warning: Could not extract icon: {}", e);
    }

    // Create desktop entry
    if let Err(e) = create_desktop_entry(&manifest) {
        println!("[voidbox] Warning: Could not create desktop entry: {}", e);
    }

    // Create wrapper script
    if let Err(e) = create_app_wrapper(app_name) {
        println!("[voidbox] Warning: Could not create wrapper script: {}", e);
    }

    // Save installed app info with actual version
    save_installed_app(
        &manifest,
        actual_version.as_deref(),
        base_version.as_deref(),
    )?;

    println!(
        "[voidbox] Successfully installed {}!",
        manifest.app.display_name
    );
    println!("[voidbox] Run with: voidbox run {}", app_name);

    Ok(())
}

/// Install an app from an embedded bundle (manifest + local archive)
pub fn install_app_from_bundle(
    manifest_content: &str,
    archive_path: &Path,
    archive_ext: &str,
    force: bool,
) -> Result<(), InstallError> {
    let manifest = parse_manifest_str(manifest_content)?;
    let mut install_manifest = manifest.clone();
    install_manifest.source = SourceConfig::Local {
        path: archive_path.to_path_buf(),
        archive_type: Some(archive_ext.to_string()),
    };

    install_app_from_manifest(&install_manifest, force)?;

    // Restore original manifest content (preserves comments/source)
    let manifest_path = paths::manifest_path(&manifest.app.name);
    fs::write(&manifest_path, manifest_content)?;

    Ok(())
}

/// Setup shared base image (Ubuntu)
fn setup_base_image(base_dir: &Path, arch: &str) -> Result<String, InstallError> {
    if base_dir.exists() {
        // Check if base is already setup
        if base_dir.join("etc/os-release").exists() {
            println!("[voidbox] Base image already exists, skipping...");
            let existing_version = read_base_version(base_dir).unwrap_or_else(|| "unknown".into());
            return Ok(existing_version);
        }
        fs::remove_dir_all(base_dir)?;
    }

    fs::create_dir_all(base_dir)?;

    println!("[voidbox] Fetching Ubuntu base image...");

    // Fetch latest Ubuntu base
    let (version, url) = fetch_latest_ubuntu_base(arch)?;
    println!("[voidbox] Downloading Ubuntu {} base...", version);

    let archive_path = base_dir.join("ubuntu_base.tar.gz");
    download_file(&url, &archive_path, true)?;

    println!("[voidbox] Extracting base image...");
    let tar_gz = File::open(&archive_path)?;
    let decoder = GzDecoder::new(tar_gz);
    let mut archive = tar::Archive::new(decoder);
    archive.set_ignore_zeros(true);
    archive.unpack(base_dir)?;
    fs::remove_file(archive_path)?;

    // Setup network
    if Path::new("/etc/resolv.conf").exists() {
        fs::create_dir_all(base_dir.join("etc"))?;
        let content = fs::read_to_string("/etc/resolv.conf")
            .unwrap_or_else(|_| "nameserver 8.8.8.8".to_string());
        fs::write(base_dir.join("etc/resolv.conf"), content)?;
    }

    Ok(version)
}

/// Fetch latest Ubuntu base image URL
fn fetch_latest_ubuntu_base(arch: &str) -> Result<(String, String), InstallError> {
    let mut resp = ureq::get(crate::UBUNTU_RELEASES_URL)
        .header("User-Agent", crate::APP_NAME)
        .call()
        .map_err(|e| InstallError::Failed(format!("Failed to fetch Ubuntu releases: {}", e)))?;

    let body = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| InstallError::Failed(format!("Failed to read response: {}", e)))?;

    // Parse version directories from HTML
    let mut versions: Vec<String> = Vec::new();
    for cap in body.split("href=\"").skip(1) {
        if let Some(end) = cap.find('/') {
            let dir = &cap[..end];
            if dir
                .chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false)
                && dir.contains('.')
                && dir.chars().all(|c| c.is_ascii_digit() || c == '.')
            {
                versions.push(dir.to_string());
            }
        }
    }

    if versions.is_empty() {
        return Err(InstallError::Failed("No Ubuntu versions found".into()));
    }

    // Sort and get latest
    versions.sort_by(|a, b| {
        let parse_version =
            |s: &str| -> Vec<u32> { s.split('.').filter_map(|p| p.parse().ok()).collect() };
        parse_version(a).cmp(&parse_version(b))
    });

    // Try versions from newest to oldest
    for version in versions.iter().rev() {
        let release_url = format!("{}{}/release/", crate::UBUNTU_RELEASES_URL, version);

        if let Ok(mut resp) = ureq::get(&release_url)
            .header("User-Agent", crate::APP_NAME)
            .call()
        {
            if let Ok(body) = resp.body_mut().read_to_string() {
                let pattern = format!("ubuntu-base-{}-base-{}.tar.gz", version, arch);
                if body.contains(&pattern) {
                    let download_url = format!("{}{}", release_url, pattern);
                    return Ok((version.clone(), download_url));
                }

                // Try base version for point releases
                let base_version: String = version.split('.').take(2).collect::<Vec<_>>().join(".");
                let alt_pattern = format!("ubuntu-base-{}-base-{}.tar.gz", base_version, arch);
                if body.contains(&alt_pattern) {
                    let download_url = format!("{}{}", release_url, alt_pattern);
                    return Ok((version.clone(), download_url));
                }
            }
        }
    }

    Err(InstallError::Failed("No Ubuntu base image found".into()))
}

fn read_base_version(base_dir: &Path) -> Option<String> {
    let os_release = base_dir.join("etc/os-release");
    let content = fs::read_to_string(os_release).ok()?;
    for line in content.lines() {
        if let Some(value) = line.strip_prefix("VERSION_ID=") {
            return Some(value.trim_matches('"').to_string());
        }
    }
    None
}

fn detect_ubuntu_arch() -> Result<String, InstallError> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("amd64".to_string()),
        "aarch64" => Ok("arm64".to_string()),
        other => Err(InstallError::Failed(format!(
            "Unsupported architecture: {}",
            other
        ))),
    }
}

/// Install dependencies in the container
fn install_dependencies(
    rootfs: &Path,
    layer_dir: &Path,
    manifest: &AppManifest,
) -> Result<(), InstallError> {
    if manifest.dependencies.packages.is_empty() {
        return Ok(());
    }

    println!("[voidbox] Installing dependencies...");

    // Get Ubuntu codename
    let _codename = get_ubuntu_codename(rootfs);
    let packages = manifest.dependencies.packages.join(" ");

    let setup_script = format!(
        r#"#!/bin/bash
export DEBIAN_FRONTEND=noninteractive
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

mkdir -p /tmp /run /var/run /var/run/dbus /etc/apt/apt.conf.d

echo 'APT::Sandbox::User "root";' > /etc/apt/apt.conf.d/99sandbox

groupadd -r -g 999 systemd-journal 2>/dev/null || true
groupadd -r -g 998 systemd-network 2>/dev/null || true
groupadd -r -g 997 systemd-resolve 2>/dev/null || true
groupadd -r -g 996 systemd-timesync 2>/dev/null || true
groupadd -r -g 995 messagebus 2>/dev/null || true

useradd -r -u 998 -g systemd-network -d / -s /usr/sbin/nologin systemd-network 2>/dev/null || true
useradd -r -u 997 -g systemd-resolve -d / -s /usr/sbin/nologin systemd-resolve 2>/dev/null || true
useradd -r -u 996 -g systemd-timesync -d / -s /usr/sbin/nologin systemd-timesync 2>/dev/null || true
useradd -r -u 995 -g messagebus -d /nonexistent -s /usr/sbin/nologin messagebus 2>/dev/null || true

if [ -f /etc/apt/sources.list.d/ubuntu.sources ]; then
    echo 'Acquire::AllowInsecureRepositories "true";' > /etc/apt/apt.conf.d/99temp-insecure
    apt-get update -qq
    apt-get install -y --no-install-recommends ubuntu-keyring ca-certificates 2>/dev/null || true
    rm -f /etc/apt/apt.conf.d/99temp-insecure
fi

apt-get update -qq

if [ ! -f /etc/machine-id ]; then
    cat /proc/sys/kernel/random/uuid | tr -d '-' > /etc/machine-id
fi
mkdir -p /var/lib/dbus
ln -sf /etc/machine-id /var/lib/dbus/machine-id 2>/dev/null || true

apt-get install -y --no-install-recommends dbus dbus-user-session 2>&1 || true
dbus-daemon --system --fork --nopidfile 2>/dev/null || true

apt-get install -y --no-install-recommends {packages} 2>&1 || true

dpkg --configure -a --force-confdef --force-confold --force-depends 2>/dev/null || true

# Compile GLib schemas (required for GTK file dialogs)
if [ -d /usr/share/glib-2.0/schemas ]; then
    glib-compile-schemas /usr/share/glib-2.0/schemas 2>/dev/null || true
fi

# Update icon cache
gtk-update-icon-cache /usr/share/icons/hicolor 2>/dev/null || true

# Update MIME database
update-mime-database /usr/share/mime 2>/dev/null || true

apt-get clean
rm -rf /var/lib/apt/lists/*

echo "Setup complete!"
"#,
        packages = packages
    );

    let setup_path = layer_dir.join("setup.sh");
    fs::write(&setup_path, setup_script)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&setup_path, fs::Permissions::from_mode(0o755))?;
    }

    // Run setup script using voidbox itself
    // Note: We use the installed voidbox path, not current_exe(), because
    // this code may be called from app-specific binaries like void_brave
    let voidbox_exe = crate::storage::paths::install_path();
    let exe_to_use = if voidbox_exe.exists() {
        voidbox_exe
    } else {
        std::env::current_exe()?
    };
    let status = Command::new(&exe_to_use)
        .args(["internal-run", rootfs.to_str().unwrap(), "/setup.sh"])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();

    fs::remove_file(&setup_path).ok();

    match status {
        Ok(s) if !s.success() => {
            println!(
                "[voidbox] Note: Some packages couldn't be fully configured (expected in container)"
            );
        }
        Err(e) => {
            println!("[voidbox] Warning: Setup script failed: {}", e);
        }
        _ => {}
    }

    Ok(())
}

fn get_ubuntu_codename(rootfs: &Path) -> String {
    let os_release = rootfs.join("etc/os-release");
    if let Ok(content) = fs::read_to_string(&os_release) {
        for line in content.lines() {
            if line.starts_with("VERSION_CODENAME=") {
                return line
                    .trim_start_matches("VERSION_CODENAME=")
                    .trim_matches('"')
                    .to_string();
            }
        }
    }
    "noble".to_string()
}

/// Download and install the app binary
/// Returns the actual version downloaded (if available)
fn install_app_binary(
    install_root: &Path,
    manifest: &AppManifest,
) -> Result<Option<String>, InstallError> {
    let (version, download_url, override_ext) = match &manifest.source {
        SourceConfig::Github {
            owner,
            repo,
            asset_os,
            asset_arch,
            asset_extension,
            ..
        } => (
            fetch_github_release(
                owner,
                repo,
                asset_os,
                asset_arch,
                asset_extension.as_deref(),
            )?
            .0,
            fetch_github_release(
                owner,
                repo,
                asset_os,
                asset_arch,
                asset_extension.as_deref(),
            )?
            .1,
            None,
        ),
        SourceConfig::Direct {
            url, archive_type, ..
        } => ("latest".to_string(), url.clone(), archive_type.clone()),
        SourceConfig::Local { path, archive_type } => {
            // Install from local path
            let install_dir = manifest
                .binary
                .install_dir
                .as_deref()
                .unwrap_or(&manifest.app.name);
            let target_dir = install_root.join(format!("opt/{}", install_dir));
            fs::create_dir_all(&target_dir)?;

            if path.is_dir() {
                copy_dir_all(path, &target_dir)?;
                create_binary_symlink(install_root, manifest)?;
                return Ok(None);
            }

            let path_str = path.to_string_lossy();
            let extension = if let Some(ext) = archive_type {
                if ext.starts_with('.') {
                    ext.clone()
                } else {
                    format!(".{}", ext)
                }
            } else {
                get_extension_from_url(&path_str)
            };

            if let Some(archive_type) =
                ArchiveType::from_extension(&extension.trim_start_matches('.'))
            {
                extract_archive(archive_type, path, &target_dir)?;
                create_binary_symlink(install_root, manifest)?;
                return Ok(None);
            }

            fs::copy(path, target_dir.join(path.file_name().unwrap()))?;
            create_binary_symlink(install_root, manifest)?;
            return Ok(None);
        }
    };

    let actual_version = if version != "latest" {
        Some(version.clone())
    } else {
        None
    };

    println!(
        "[voidbox] Downloading {} v{}...",
        manifest.app.display_name, version
    );

    let install_dir = manifest
        .binary
        .install_dir
        .as_deref()
        .unwrap_or(&manifest.app.name);

    let extension = if let Some(ext) = override_ext {
        if ext.starts_with('.') {
            ext
        } else {
            format!(".{}", ext)
        }
    } else {
        get_extension_from_url(&download_url)
    };

    let archive_path = install_root.join(format!("{}_download{}", install_dir, extension));

    download_file(&download_url, &archive_path, true)?;

    println!("[voidbox] Extracting...");
    let target_dir = install_root.join(format!("opt/{}", install_dir));
    fs::create_dir_all(&target_dir)?;

    // Extract based on archive type
    let archive_type =
        ArchiveType::from_extension(&extension.trim_start_matches('.')).unwrap_or(ArchiveType::Zip);
    extract_archive(archive_type, &archive_path, &target_dir)?;

    fs::remove_file(archive_path)?;

    // Create symlink to binary
    create_binary_symlink(install_root, manifest)?;

    Ok(actual_version)
}

fn fetch_github_release(
    owner: &str,
    repo: &str,
    asset_os: &str,
    asset_arch: &str,
    asset_extension: Option<&str>,
) -> Result<(String, String), InstallError> {
    let api_url = format!(
        "https://api.github.com/repos/{}/{}/releases/latest",
        owner, repo
    );

    let mut resp = ureq::get(&api_url)
        .header("User-Agent", crate::APP_NAME)
        .call()
        .map_err(|e| InstallError::Failed(format!("GitHub API error: {}", e)))?;

    let body = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| InstallError::Failed(format!("Failed to read response: {}", e)))?;

    let release: GitHubRelease = serde_json::from_str(&body)
        .map_err(|e| InstallError::Failed(format!("Failed to parse GitHub response: {}", e)))?;

    let version = release.tag_name.trim_start_matches('v').to_string();

    // Find matching asset
    for asset in release.assets {
        let name_lower = asset.name.to_lowercase();
        if name_lower.contains(asset_os) && name_lower.contains(asset_arch) {
            if let Some(ext) = asset_extension {
                if asset.name.ends_with(ext) {
                    return Ok((version, asset.browser_download_url));
                }
            } else {
                return Ok((version, asset.browser_download_url));
            }
        }
    }

    Err(InstallError::Failed(format!(
        "No matching asset found for {} {} in {}/{}",
        asset_os, asset_arch, owner, repo
    )))
}

fn get_extension_from_url(url: &str) -> String {
    let path = url.split('?').next().unwrap_or(url);
    if path.ends_with(".tar.gz") || path.ends_with(".tgz") {
        ".tar.gz".to_string()
    } else if path.ends_with(".tar.xz") {
        ".tar.xz".to_string()
    } else if path.ends_with(".tar.zst") {
        ".tar.zst".to_string()
    } else if path.ends_with(".zip") {
        ".zip".to_string()
    } else {
        ".zip".to_string() // Default
    }
}

fn extract_archive(
    archive_type: ArchiveType,
    archive_path: &Path,
    target_dir: &Path,
) -> Result<(), InstallError> {
    match archive_type {
        ArchiveType::Zip => {
            let file = File::open(archive_path)?;
            let mut archive = zip::ZipArchive::new(file)
                .map_err(|e| InstallError::Failed(format!("Failed to open zip: {}", e)))?;

            for i in 0..archive.len() {
                let mut file = archive.by_index(i).map_err(|e| {
                    InstallError::Failed(format!("Failed to read zip entry: {}", e))
                })?;

                let outpath = match file.enclosed_name() {
                    Some(path) => target_dir.join(path),
                    None => continue,
                };

                if file.name().ends_with('/') {
                    fs::create_dir_all(&outpath)?;
                } else {
                    if let Some(p) = outpath.parent() {
                        if !p.exists() {
                            fs::create_dir_all(p)?;
                        }
                    }
                    let mut outfile = File::create(&outpath)?;
                    std::io::copy(&mut file, &mut outfile)?;
                }

                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Some(mode) = file.unix_mode() {
                        fs::set_permissions(&outpath, fs::Permissions::from_mode(mode))?;
                    }
                }
            }
            Ok(())
        }
        ArchiveType::TarGz => {
            let file = File::open(archive_path)?;
            let decoder = GzDecoder::new(file);
            let mut archive = tar::Archive::new(decoder);
            archive.unpack(target_dir)?;
            Ok(())
        }
        _ => Err(InstallError::Failed(format!(
            "Unsupported archive type: {}",
            archive_type.extension()
        ))),
    }
}

fn create_binary_symlink(install_root: &Path, manifest: &AppManifest) -> Result<(), InstallError> {
    let install_dir = manifest
        .binary
        .install_dir
        .as_deref()
        .unwrap_or(&manifest.app.name);
    let target_dir = install_root.join(format!("opt/{}", install_dir));

    // Find the binary
    let binary_name = &manifest.binary.name;
    let mut binary_path = None;

    // Priority 1: Check manifest path (suffix match for flexibility)
    if let Some(explicit_path) = &manifest.binary.path {
        for entry in WalkDir::new(&target_dir).max_depth(3) {
            if let Ok(entry) = entry {
                if entry.path().ends_with(explicit_path) && entry.path().is_file() {
                    binary_path = Some(entry.path().to_path_buf());
                    break;
                }
            }
        }
    }

    // Priority 2: Name match (existing logic)
    if binary_path.is_none() {
        for entry in WalkDir::new(&target_dir).max_depth(3) {
            if let Ok(entry) = entry {
                if entry.file_name().to_string_lossy() == binary_name.as_str()
                    && entry.path().is_file()
                {
                    binary_path = Some(entry.path().to_path_buf());
                    break;
                }
            }
        }
    }

    let binary_path = binary_path.ok_or_else(|| {
        InstallError::Failed(format!("Binary '{}' not found in archive", binary_name))
    })?;

    // Create /usr/bin symlink
    let relative_path = binary_path
        .strip_prefix(install_root)
        .map_err(|e| InstallError::Failed(format!("Path error: {}", e)))?;
    let container_path = Path::new("/").join(relative_path);

    fs::create_dir_all(install_root.join("usr/bin"))?;
    let link_path = install_root.join(format!("usr/bin/{}", binary_name));

    if fs::symlink_metadata(&link_path).is_ok() {
        fs::remove_file(&link_path)?;
    }

    std::os::unix::fs::symlink(container_path, link_path)?;

    Ok(())
}

fn copy_dir_all(src: &Path, dst: &Path) -> Result<(), InstallError> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dst.join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.join(entry.file_name()))?;
        }
    }
    Ok(())
}

fn save_installed_app(
    manifest: &AppManifest,
    actual_version: Option<&str>,
    base_version: Option<&str>,
) -> Result<(), InstallError> {
    let db_path = paths::database_path();

    let mut apps: Vec<InstalledApp> = if db_path.exists() {
        let content = fs::read_to_string(&db_path)?;
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        Vec::new()
    };

    // Remove existing entry if any
    apps.retain(|a| a.name != manifest.app.name);

    // Use actual downloaded version if available, otherwise manifest version
    let version = actual_version
        .map(|v| Some(v.to_string()))
        .unwrap_or_else(|| manifest.app.version.clone());

    // Add new entry
    apps.push(InstalledApp {
        name: manifest.app.name.clone(),
        display_name: manifest.app.display_name.clone(),
        version,
        base_version: base_version.map(|v| v.to_string()),
        installed_date: Some(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()),
        manifest_path: Some(paths::manifest_path(&manifest.app.name)),
    });

    let content = serde_json::to_string_pretty(&apps)
        .map_err(|e| InstallError::Failed(format!("Failed to serialize: {}", e)))?;
    fs::write(&db_path, content)?;

    Ok(())
}
