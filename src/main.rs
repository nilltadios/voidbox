mod app;

use clap::{Parser, Subcommand};
use flate2::read::GzDecoder;
use nix::mount::{mount, umount2, MsFlags, MntFlags};
use nix::libc;
use nix::sched::{unshare, CloneFlags};
use nix::unistd::{pivot_root, chdir, execvp, sethostname, getuid, getgid};
use serde::Deserialize;
use std::ffi::CString;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use walkdir::WalkDir;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const UBUNTU_RELEASES_URL: &str = "https://cdimage.ubuntu.com/ubuntu-base/releases/";

#[derive(Parser)]
#[command(name = app::APP_NAME)]
#[command(version = VERSION)]
#[command(about = app::APP_DESCRIPTION, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Launch the app or a command inside the container
    Run {
        /// URL to open (if running default app)
        #[arg(long)]
        url: Option<String>,

        /// Command to run (overrides default app)
        #[arg(trailing_var_arg = true)]
        cmd: Vec<String>,

        /// Force rebuild of environment
        #[arg(long)]
        rebuild: bool,
    },
    /// Update target app to latest version
    Update {
        /// Force update even if already on latest
        #[arg(long)]
        force: bool,
    },
    /// Update void_runner itself to latest version
    SelfUpdate {
        /// Force update even if already on latest
        #[arg(long)]
        force: bool,
    },
    /// Uninstall void_runner completely
    Uninstall {
        /// Also remove browser data and rootfs
        #[arg(long)]
        purge: bool,
    },
    /// Show version and installed component info
    Info,
    /// Internal initialization (do not use manually)
    #[command(hide = true)]
    InternalInit {
        rootfs: PathBuf,
        cmd: String,

        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
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

#[derive(Deserialize, serde::Serialize, Default)]
struct InstalledInfo {
    #[serde(alias = "brave_version")]  // Backwards compatibility with old installs
    app_version: Option<String>,
    ubuntu_version: Option<String>,
    installed_date: Option<String>,
}

fn get_data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(app::APP_NAME)
}

fn get_install_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(format!(".local/bin/{}", app::APP_NAME))
}

fn get_desktop_file_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(format!("applications/{}.desktop", app::APP_NAME))
}

fn is_installed() -> bool {
    let install_path = get_install_path();
    install_path.exists()
}

fn install_self() -> Result<(), Box<dyn std::error::Error>> {
    let current_exe = std::env::current_exe()?;
    let install_path = get_install_path();
    let desktop_path = get_desktop_file_path();
    let data_dir = get_data_dir();

    // Create ~/.local/bin if it doesn't exist
    if let Some(parent) = install_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Copy binary to install path
    println!("[{}] Installing to {}...", app::APP_NAME, install_path.display());
    fs::copy(&current_exe, &install_path)?;

    // Make executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&install_path, fs::Permissions::from_mode(0o755))?;
    }

    // Create .desktop file
    if let Some(parent) = desktop_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Try to extract app icon if rootfs exists
    let icon_dst = data_dir.join(format!("{}.png", app::APP_NAME));
    if !icon_dst.exists() {
        let app_icon = data_dir.join(format!("rootfs/opt/{}/{}", app::TARGET_INSTALL_DIR, app::TARGET_ICON_FILENAME));
        if app_icon.exists() {
            let _ = fs::copy(&app_icon, &icon_dst);
        }
    }

    // Use app icon if available, otherwise use fallback icon
    let icon_value = if icon_dst.exists() {
        icon_dst.to_string_lossy().to_string()
    } else {
        app::DESKTOP_FALLBACK_ICON.to_string()
    };

    let desktop_content = format!(
r#"[Desktop Entry]
Name={}
Comment={}
Exec={}
Icon={}
Terminal=false
Type=Application
Categories={}
StartupWMClass={}
"#,
        app::APP_DISPLAY_NAME,
        app::APP_DESCRIPTION,
        app::APP_NAME,
        icon_value,
        app::DESKTOP_CATEGORIES,
        app::DESKTOP_WM_CLASS
    );

    println!("[{}] Creating desktop launcher...", app::APP_NAME);
    fs::write(&desktop_path, desktop_content)?;

    println!("[{}] Installation complete!", app::APP_NAME);
    println!("[{}] You can now run '{}' from anywhere or find it in your app launcher.", app::APP_NAME, app::APP_NAME);

    Ok(())
}

fn uninstall_self(purge: bool) -> Result<(), Box<dyn std::error::Error>> {
    let install_path = get_install_path();
    let desktop_path = get_desktop_file_path();
    let data_dir = get_data_dir();

    println!("[{}] Uninstalling...", app::APP_NAME);

    // Remove binary
    if install_path.exists() {
        fs::remove_file(&install_path)?;
        println!("  Removed {}", install_path.display());
    }

    // Remove desktop file
    if desktop_path.exists() {
        fs::remove_file(&desktop_path)?;
        println!("  Removed {}", desktop_path.display());
    }

    // Remove icon
    let icon_path = data_dir.join(format!("{}.png", app::APP_NAME));
    if icon_path.exists() {
        fs::remove_file(&icon_path)?;
        println!("  Removed {}", icon_path.display());
    }

    if purge {
        // Remove entire data directory (rootfs, config, etc.)
        if data_dir.exists() {
            println!("  Removing data directory (this may take a moment)...");
            fs::remove_dir_all(&data_dir)?;
            println!("  Removed {}", data_dir.display());
        }
    } else {
        // Just remove installed.json but keep rootfs
        let info_path = data_dir.join("installed.json");
        if info_path.exists() {
            fs::remove_file(&info_path)?;
        }
        println!();
        println!("  Note: Browser data kept at {}", data_dir.display());
        println!("  Use --purge to remove everything including browser data.");
    }

    println!();
    println!("[{}] Uninstall complete!", app::APP_NAME);

    Ok(())
}

fn load_installed_info(data_dir: &Path) -> InstalledInfo {
    let info_path = data_dir.join("installed.json");
    if info_path.exists() {
        if let Ok(content) = fs::read_to_string(&info_path) {
            if let Ok(info) = serde_json::from_str(&content) {
                return info;
            }
        }
    }
    InstalledInfo::default()
}

fn save_installed_info(data_dir: &Path, info: &InstalledInfo) {
    let info_path = data_dir.join("installed.json");
    if let Ok(content) = serde_json::to_string_pretty(info) {
        let _ = fs::write(info_path, content);
    }
}

fn fetch_latest_target_release() -> Result<(String, String), Box<dyn std::error::Error>> {
    let api_url = app::RELEASES_API.ok_or("No releases API configured")?;

    let mut resp = ureq::get(api_url)
        .header("User-Agent", app::APP_NAME)
        .call()?;

    let body = resp.body_mut().read_to_string()?;
    let release: GitHubRelease = serde_json::from_str(&body)?;
    let version = release.tag_name.trim_start_matches('v').to_string();

    // Find matching asset based on app config
    for asset in release.assets {
        if asset.name.contains(app::ASSET_OS_PATTERN)
            && asset.name.contains(app::ASSET_ARCH_PATTERN)
            && asset.name.ends_with(app::ASSET_EXTENSION)
        {
            return Ok((version, asset.browser_download_url));
        }
    }

    Err(format!(
        "No {} {} {} found in release",
        app::ASSET_OS_PATTERN,
        app::ASSET_ARCH_PATTERN,
        app::ASSET_EXTENSION
    ).into())
}

fn fetch_latest_ubuntu_base() -> Result<(String, String), Box<dyn std::error::Error>> {
    // Fetch the releases directory listing
    let mut resp = ureq::get(UBUNTU_RELEASES_URL)
        .header("User-Agent", app::APP_NAME)
        .call()?;

    let body = resp.body_mut().read_to_string()?;

    // Parse version directories from HTML (matches patterns like "25.10/" or "24.04.3/")
    let mut versions: Vec<String> = Vec::new();
    for cap in body.split("href=\"").skip(1) {
        if let Some(end) = cap.find('/') {
            let dir = &cap[..end];
            // Check if it looks like a version number (starts with digit, contains dots)
            if dir.chars().next().map(|c: char| c.is_ascii_digit()).unwrap_or(false)
               && dir.contains('.')
               && dir.chars().all(|c: char| c.is_ascii_digit() || c == '.') {
                versions.push(dir.to_string());
            }
        }
    }

    if versions.is_empty() {
        return Err("No Ubuntu versions found".into());
    }

    // Sort versions (simple string sort works for Ubuntu versions like 24.04, 25.10)
    versions.sort_by(|a, b| {
        let parse_version = |s: &str| -> Vec<u32> {
            s.split('.').filter_map(|p| p.parse().ok()).collect()
        };
        parse_version(a).cmp(&parse_version(b))
    });

    // Try versions from newest to oldest until we find one with a release
    for version in versions.iter().rev() {
        let release_url = format!("{}{}/release/", UBUNTU_RELEASES_URL, version);

        if let Ok(mut resp) = ureq::get(&release_url)
            .header("User-Agent", app::APP_NAME)
            .call()
        {
            if let Ok(body) = resp.body_mut().read_to_string() {
                // Look for ubuntu-base-*-base-amd64.tar.gz
                let pattern = format!("ubuntu-base-{}-base-amd64.tar.gz", version);
                if body.contains(&pattern) {
                    let download_url = format!("{}{}", release_url, pattern);
                    return Ok((version.clone(), download_url));
                }

                // Also try without minor version for point releases (e.g., 24.04.3 -> 24.04)
                let base_version: String = version.split('.').take(2).collect::<Vec<_>>().join(".");
                let alt_pattern = format!("ubuntu-base-{}-base-amd64.tar.gz", base_version);
                if body.contains(&alt_pattern) {
                    let download_url = format!("{}{}", release_url, alt_pattern);
                    return Ok((version.clone(), download_url));
                }
            }
        }
    }

    Err("No Ubuntu base image found".into())
}

fn get_ubuntu_codename(rootfs: &Path) -> String {
    // Read codename from extracted rootfs /etc/os-release
    let os_release = rootfs.join("etc/os-release");
    if let Ok(content) = fs::read_to_string(&os_release) {
        for line in content.lines() {
            if line.starts_with("VERSION_CODENAME=") {
                return line.trim_start_matches("VERSION_CODENAME=").trim_matches('"').to_string();
            }
        }
    }
    // Fallback to noble if we can't detect
    "noble".to_string()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let data_dir = get_data_dir();
    fs::create_dir_all(&data_dir)?;

    let command = cli.command.unwrap_or(Commands::Run {
        url: None,
        cmd: vec![],
        rebuild: false
    });

    // Self-install on first run (skip for internal-init command)
    if !matches!(command, Commands::InternalInit { .. }) && !is_installed() {
        if let Err(e) = install_self() {
            println!("[{}] Warning: Self-installation failed: {}", app::APP_NAME, e);
            println!("[{}] Continuing without installation...", app::APP_NAME);
        }
    }

    match command {
        Commands::Run { url, cmd, rebuild } => {
            // Check for self-updates first
            print!("[{}] Checking for self-updates... ", app::APP_NAME);
            match get_latest_self_version() {
                Ok(latest) => {
                    // Check if latest is actually newer using semver
                    let current = semver::Version::parse(VERSION).ok();
                    let latest_parsed = semver::Version::parse(&latest).ok();
                    let is_newer = match (&current, &latest_parsed) {
                        (Some(c), Some(l)) => l > c,
                        _ => latest != VERSION,
                    };

                    if is_newer {
                        println!("v{} available!", latest);
                        match check_self_update(false) {
                            Ok(true) => println!("[{}] Please restart to use the new version.", app::APP_NAME),
                            Ok(false) => {}
                            Err(e) => println!("[{}] Self-update failed: {}", app::APP_NAME, e),
                        }
                    } else {
                        println!("up to date.");
                    }
                }
                Err(e) => println!("failed ({})", e),
            }

            let rootfs = data_dir.join("rootfs");

            if rebuild && rootfs.exists() {
                println!("[{}] Rebuild requested. Removing old rootfs...", app::APP_NAME);
                fs::remove_dir_all(&rootfs)?;
            }

            // Check if installation is complete (target app symlink exists)
            let target_link = rootfs.join(format!("usr/bin/{}", app::TARGET_BINARY_NAME));
            // Only enforce build check if we are running default app
            let is_default_run = cmd.is_empty();

            let needs_build = if is_default_run {
                // Check if rootfs exists and the symlink exists (do NOT follow it, as it points to /opt inside container)
                !rootfs.exists() || fs::symlink_metadata(&target_link).is_err()
            } else {
                !rootfs.exists()
            };

            if needs_build && rootfs.exists() && is_default_run {
                // Incomplete install - remove and rebuild
                println!("[{}] Incomplete installation detected (missing {:?}). Rebuilding...", app::APP_NAME, target_link);
                fs::remove_dir_all(&rootfs)?;
            }

            if needs_build {
                let is_tty = unsafe { libc::isatty(1) == 1 };

                if !is_tty {
                    // Spawn terminal for first-run setup
                    // ... (existing terminal spawning code) ...
                }

                println!("[{}] Building isolated environment...", app::APP_NAME);
                build_environment(&data_dir, &rootfs, &std::env::current_exe()?)?;
            } else {
                // Check for updates on launch (if not building)
                // We run this in a non-blocking way or quick check
                if let Ok(info) = std::fs::read_to_string(data_dir.join("installed.json")) {
                    if let Ok(installed) = serde_json::from_str::<InstalledInfo>(&info) {
                        // Only check if it's been more than 24 hours or if we just want to be safe
                        // For responsiveness, we'll spawn a background thread/process or just check quickly
                        // Here we do a blocking check but print nicely.
                        println!("[{}] Checking for updates...", app::APP_NAME);
                        if let Ok((latest, url)) = fetch_latest_target_release() {
                            if installed.app_version.as_deref() != Some(&latest) {
                                println!("[{}] Update available: v{} -> v{}", app::APP_NAME, installed.app_version.as_deref().unwrap_or("?"), latest);
                                println!("[{}] Auto-updating...", app::APP_NAME);
                                if let Err(e) = update_target_app(&rootfs, &url, &latest) {
                                    println!("[{}] Update failed: {}", app::APP_NAME, e);
                                } else {
                                    let mut new_info = installed;
                                    new_info.app_version = Some(latest.clone());
                                    new_info.installed_date = Some(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
                                    save_installed_info(&data_dir, &new_info);
                                    println!("[{}] Updated to v{}", app::APP_NAME, latest);
                                }
                            }
                        }
                    }
                }
            }

            // Determine command to run
            let (run_cmd, run_args) = if cmd.is_empty() {
                let mut args: Vec<String> = app::DEFAULT_LAUNCH_ARGS.iter().map(|s| s.to_string()).collect();
                if let Some(u) = url {
                    args.push(u);
                }
                (format!("/usr/bin/{}", app::TARGET_BINARY_NAME), args)
            } else {
                (cmd[0].clone(), cmd[1..].to_vec())
            };

            // Setup Namespaces
            let uid = getuid();
            let gid = getgid();

            unshare(CloneFlags::CLONE_NEWUSER).map_err(|e| format!("Unshare user failed: {}", e))?;

            let uid_map = format!("0 {} 1", uid);
            let gid_map = format!("0 {} 1", gid);
            fs::write("/proc/self/uid_map", &uid_map)?;
            fs::write("/proc/self/setgroups", "deny")?;
            fs::write("/proc/self/gid_map", &gid_map)?;

            unshare(CloneFlags::CLONE_NEWNS | CloneFlags::CLONE_NEWUTS | CloneFlags::CLONE_NEWIPC | CloneFlags::CLONE_NEWPID)
                .map_err(|e| format!("Unshare failed: {}", e))?;

            let mut child = Command::new(std::env::current_exe()?)
                .arg("internal-init")
                .arg(&rootfs)
                .arg(&run_cmd)
                .args(&run_args)
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()?;

            let status = child.wait()?;
            if !status.success() {
                std::process::exit(status.code().unwrap_or(1));
            }
        }

        Commands::InternalInit { rootfs, cmd, args } => {
            setup_container(&rootfs)?;

            let c_cmd = CString::new(cmd.clone()).unwrap();
            let c_args: Vec<CString> = std::iter::once(c_cmd.clone())
                .chain(args.iter().map(|a| CString::new(a.as_str()).unwrap()))
                .collect();

            execvp(&c_cmd, &c_args).map_err(|e| format!("Exec failed: {} ({})", e, cmd))?;
        }

        Commands::Update { force } => {
            println!("[{}] Checking for {} updates...", app::APP_NAME, app::TARGET_APP_NAME);

            let info = load_installed_info(&data_dir);
            let (latest_version, download_url) = fetch_latest_target_release()?;

            println!("  Installed: {}", info.app_version.as_deref().unwrap_or("unknown"));
            println!("  Latest:    {}", latest_version);

            let needs_update = force || info.app_version.as_deref() != Some(&latest_version);

            if !needs_update {
                println!("[{}] Already running latest version.", app::APP_NAME);
                return Ok(());
            }

            println!("[{}] Updating {} to v{}...", app::APP_NAME, app::TARGET_APP_NAME, latest_version);

            let rootfs = data_dir.join("rootfs");
            if !rootfs.exists() {
                println!("[{}] No installation found. Run '{}' first to install.", app::APP_NAME, app::APP_NAME);
                return Ok(());
            }

            // Download and install new target app
            update_target_app(&rootfs, &download_url, &latest_version)?;

            // Save new version info
            let mut new_info = info;
            new_info.app_version = Some(latest_version.clone());
            new_info.installed_date = Some(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
            save_installed_info(&data_dir, &new_info);

            println!("[{}] Update complete! {} v{} installed.", app::APP_NAME, app::TARGET_APP_NAME, latest_version);
        }

        Commands::SelfUpdate { force } => {
            println!("[{}] Checking for {} updates...", app::APP_NAME, app::APP_NAME);
            println!("  Installed: v{}", VERSION);

            match get_latest_self_version() {
                Ok(latest) => {
                    println!("  Latest:    v{}", latest);

                    if !force && latest == VERSION {
                        println!("[{}] Already running latest version.", app::APP_NAME);
                        return Ok(());
                    }

                    match check_self_update(force) {
                        Ok(true) => println!("[{}] Self-update complete! Please restart {}.", app::APP_NAME, app::APP_NAME),
                        Ok(false) => println!("[{}] Already up to date.", app::APP_NAME),
                        Err(e) => println!("[{}] Self-update failed: {}", app::APP_NAME, e),
                    }
                }
                Err(e) => println!("[{}] Failed to check for updates: {}", app::APP_NAME, e),
            }
        }

        Commands::Uninstall { purge } => {
            if purge {
                println!("[{}] This will remove {} and ALL app data.", app::APP_NAME, app::APP_NAME);
            } else {
                println!("[{}] This will remove {} but keep app data.", app::APP_NAME, app::APP_NAME);
            }
            print!("Continue? [y/N] ");
            std::io::Write::flush(&mut std::io::stdout())?;

            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;

            if input.trim().to_lowercase() == "y" {
                uninstall_self(purge)?;
            } else {
                println!("[{}] Uninstall cancelled.", app::APP_NAME);
            }
        }

        Commands::Info => {
            println!("{} v{}", app::APP_NAME, VERSION);
            println!("{}", app::APP_DESCRIPTION);
            println!();

            let info = load_installed_info(&data_dir);
            let rootfs = data_dir.join("rootfs");

            println!("Data directory: {}", data_dir.display());
            println!("Rootfs exists:  {}", rootfs.exists());

            if let Some(v) = &info.app_version {
                println!("{} version: {}", app::TARGET_APP_NAME, v);
            }
            if let Some(v) = &info.ubuntu_version {
                println!("Ubuntu version: {}", v);
            }
            if let Some(d) = &info.installed_date {
                println!("Installed:      {}", d);
            }

            // Check for updates
            println!();
            print!("Checking for {} updates... ", app::TARGET_APP_NAME);
            match fetch_latest_target_release() {
                Ok((latest, _)) => {
                    if info.app_version.as_deref() == Some(&latest) {
                        println!("Up to date (v{})", latest);
                    } else {
                        println!("Update available: v{}", latest);
                    }
                }
                Err(e) => println!("Failed ({})", e),
            }

            print!("Checking for {} updates... ", app::APP_NAME);
            match get_latest_self_version() {
                Ok(latest) => {
                    if latest == VERSION {
                        println!("Up to date (v{})", latest);
                    } else {
                        println!("Update available: v{}", latest);
                    }
                }
                Err(e) => println!("Failed ({})", e),
            }
        }
    }

    Ok(())
}

fn update_target_app(rootfs: &Path, download_url: &str, version: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("  Downloading {} v{}...", app::TARGET_APP_NAME, version);

    let mut resp = ureq::get(download_url)
        .header("User-Agent", app::APP_NAME)
        .call()?;

    let archive_path = rootfs.join(format!("{}_update{}", app::TARGET_INSTALL_DIR, app::ASSET_EXTENSION));
    let mut out = fs::File::create(&archive_path)?;
    let mut reader = resp.body_mut().with_config().limit(500_000_000).reader();
    std::io::copy(&mut reader, &mut out)?;
    drop(out);

    println!("  Extracting...");

    // Remove old app
    let target_dir = rootfs.join(format!("opt/{}", app::TARGET_INSTALL_DIR));
    if target_dir.exists() {
        fs::remove_dir_all(&target_dir)?;
    }
    fs::create_dir_all(&target_dir)?;

    // Extract based on archive type
    match app::TARGET_ARCHIVE_TYPE {
        app::ArchiveType::Zip => {
            let file = fs::File::open(&archive_path)?;
            let mut archive = zip::ZipArchive::new(file)?;

            for i in 0..archive.len() {
                let mut file = archive.by_index(i)?;
                let outpath = match file.enclosed_name() {
                    Some(path) => target_dir.join(path),
                    None => continue,
                };

                if file.name().ends_with('/') {
                    fs::create_dir_all(&outpath)?;
                } else {
                    if let Some(p) = outpath.parent() {
                        if !p.exists() { fs::create_dir_all(p)?; }
                    }
                    let mut outfile = fs::File::create(&outpath)?;
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
        }
        app::ArchiveType::TarGz => {
            let file = fs::File::open(&archive_path)?;
            let decoder = GzDecoder::new(file);
            let mut archive = tar::Archive::new(decoder);
            archive.unpack(&target_dir)?;
        }
        app::ArchiveType::TarXz => {
            // For .tar.xz, we'd need xz2 crate - for now just error
            return Err("TarXz archive type not yet supported".into());
        }
    }

    fs::remove_file(archive_path)?;

    // Update symlink
    let mut binary_path = PathBuf::new();
    for entry in WalkDir::new(&target_dir) {
        let entry = entry?;
        if entry.file_name() == app::TARGET_BINARY_NAME && entry.path().is_file() {
            binary_path = entry.path().to_path_buf();
            break;
        }
    }

    if binary_path.as_os_str().is_empty() {
        return Err(format!("{} binary not found", app::TARGET_APP_NAME).into());
    }

    let relative_path = binary_path.strip_prefix(rootfs)?;
    let container_path = Path::new("/").join(relative_path);

    let link_path = rootfs.join(format!("usr/bin/{}", app::TARGET_BINARY_NAME));
    // Use symlink_metadata to detect broken symlinks (exists() returns false for them)
    if fs::symlink_metadata(&link_path).is_ok() {
        fs::remove_file(&link_path)?;
    }
    std::os::unix::fs::symlink(container_path, link_path)?;

    Ok(())
}

fn check_self_update(force: bool) -> Result<bool, Box<dyn std::error::Error>> {
    let status = self_update::backends::github::Update::configure()
        .repo_owner(app::SELF_UPDATE_OWNER)
        .repo_name(app::SELF_UPDATE_REPO)
        .bin_name(app::APP_NAME)
        .identifier(app::APP_NAME)  // Match exact asset name
        .current_version(VERSION)
        .build()?;

    let latest = status.get_latest_release()?;
    let latest_version = latest.version.trim_start_matches('v');

    // Parse versions for proper comparison
    let current = semver::Version::parse(VERSION).ok();
    let latest_parsed = semver::Version::parse(latest_version).ok();

    let is_newer = match (&current, &latest_parsed) {
        (Some(c), Some(l)) => l > c,
        _ => latest_version != VERSION, // Fallback to string comparison
    };

    if !force && !is_newer {
        return Ok(false);
    }

    println!("[{}] Self-update available: v{} -> v{}", app::APP_NAME, VERSION, latest_version);
    println!("[{}] Updating {}...", app::APP_NAME, app::APP_NAME);

    let status = self_update::backends::github::Update::configure()
        .repo_owner(app::SELF_UPDATE_OWNER)
        .repo_name(app::SELF_UPDATE_REPO)
        .bin_name(app::APP_NAME)
        .identifier(app::APP_NAME)  // Match exact asset name
        .current_version(VERSION)
        .build()?
        .update()?;

    println!("[{}] Updated to v{}", app::APP_NAME, status.version());
    Ok(true)
}

fn get_latest_self_version() -> Result<String, Box<dyn std::error::Error>> {
    let status = self_update::backends::github::Update::configure()
        .repo_owner(app::SELF_UPDATE_OWNER)
        .repo_name(app::SELF_UPDATE_REPO)
        .bin_name(app::APP_NAME)
        .current_version(VERSION)
        .build()?;

    let latest = status.get_latest_release()?;
    Ok(latest.version.trim_start_matches('v').to_string())
}

fn build_environment(data_dir: &Path, rootfs: &Path, self_exe: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(rootfs)?;

    let is_tty = unsafe { libc::isatty(1) == 1 };

    // GUI Progress
    let mut gui_child = None;
    let mut gui_stdin: Option<std::process::ChildStdin> = None;

    if !is_tty {
        let script = r#"
import tkinter as tk
from tkinter import ttk
import sys, threading, queue

msg_queue = queue.Queue()

def read_stdin():
    while True:
        try:
            line = sys.stdin.readline()
            if not line: break
            msg_queue.put(line.strip())
        except: break

def poll_queue():
    while not msg_queue.empty():
        msg = msg_queue.get()
        if msg.startswith("PCT:"):
            try: progress['value'] = float(msg.split(":")[1])
            except: pass
        elif msg.startswith("MSG:"):
            status_label.config(text=msg[4:])
        elif msg == "EXIT":
            root.destroy()
            return
    root.after(100, poll_queue)

root = tk.Tk()
root.title("Void Runner Setup")
root.geometry("400x150")
root.resizable(False, False)
x = (root.winfo_screenwidth() - 400) // 2
y = (root.winfo_screenheight() - 150) // 2
root.geometry(f"400x150+{x}+{y}")

frame = ttk.Frame(root, padding="20")
frame.pack(fill=tk.BOTH, expand=True)
ttk.Label(frame, text="Installing Void Runner", font=("Arial", 12, "bold")).pack(pady=(0, 10))
status_label = ttk.Label(frame, text="Initializing...", font=("Arial", 10))
status_label.pack(anchor=tk.W, pady=(0, 5))
progress = ttk.Progressbar(frame, length=300, mode='determinate')
progress.pack(fill=tk.X)

threading.Thread(target=read_stdin, daemon=True).start()
root.after(100, poll_queue)
root.mainloop()
"#;
        let script_path = std::env::temp_dir().join("void_runner_gui.py");
        if fs::write(&script_path, script).is_ok() {
            if let Ok(mut child) = Command::new("python3")
                .arg(&script_path)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                gui_stdin = child.stdin.take();
                gui_child = Some(child);
            }
        }
    }

    let update_progress = |pct: u64, msg: &str, gui: &mut Option<std::process::ChildStdin>| {
        if is_tty {
            println!("[{:3}%] {}", pct, msg);
        }
        if let Some(stdin) = gui {
            let _ = writeln!(stdin, "PCT:{}", pct);
            let _ = writeln!(stdin, "MSG:{}", msg);
        }
    };

    // 1. Fetch latest versions
    update_progress(2, "Fetching latest versions...", &mut gui_stdin);
    let (app_version, app_url) = fetch_latest_target_release()?;
    let (ubuntu_version, ubuntu_url) = fetch_latest_ubuntu_base()?;

    // 2. Download Ubuntu Base
    update_progress(5, &format!("Downloading Ubuntu {} Base...", ubuntu_version), &mut gui_stdin);

    let mut resp = ureq::get(&ubuntu_url).call()?;
    let len = resp.headers()
        .get("Content-Length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(28_000_000);

    let mut reader = resp.body_mut().with_config().limit(500_000_000).reader();
    let mut buffer = vec![0u8; 8192];
    let mut downloaded = 0u64;

    let mut temp_tar = fs::File::create(rootfs.join("ubuntu_base.tar.gz"))?;
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 { break; }
        temp_tar.write_all(&buffer[..n])?;
        downloaded += n as u64;

        if downloaded % 1_000_000 < 8192 {
            let pct = 5 + (downloaded * 20 / len);
            update_progress(pct, &format!("Downloading Ubuntu {} Base...", ubuntu_version), &mut gui_stdin);
        }
    }
    drop(temp_tar);

    update_progress(25, "Extracting Base System...", &mut gui_stdin);
    let tar_gz = fs::File::open(rootfs.join("ubuntu_base.tar.gz"))?;
    let decoder = GzDecoder::new(tar_gz);
    let mut archive = tar::Archive::new(decoder);
    archive.set_ignore_zeros(true);
    archive.unpack(rootfs)?;
    fs::remove_file(rootfs.join("ubuntu_base.tar.gz"))?;

    // 3. Setup Network
    update_progress(35, "Configuring Network...", &mut gui_stdin);
    if Path::new("/etc/resolv.conf").exists() {
        fs::create_dir_all(rootfs.join("etc"))?;
        let content = fs::read_to_string("/etc/resolv.conf").unwrap_or_else(|_| "nameserver 8.8.8.8".to_string());
        fs::write(rootfs.join("etc/resolv.conf"), content)?;
    }

    // 4. Install Dependencies
    update_progress(40, "Installing Dependencies (this takes a while)...", &mut gui_stdin);

    // Debug: Check self_exe
    if !self_exe.exists() {
        return Err(format!("Self executable not found at: {:?}", self_exe).into());
    }

    // Get Ubuntu codename from the extracted rootfs
    let codename = get_ubuntu_codename(rootfs);

    let setup_script = format!(r#"#!/bin/bash
export DEBIAN_FRONTEND=noninteractive
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

mkdir -p /tmp /run /var/run /var/run/dbus /etc/apt/apt.conf.d

# Disable APT sandboxing (we're already in a container)
echo 'APT::Sandbox::User "root";' > /etc/apt/apt.conf.d/99sandbox

# Create systemd users/groups BEFORE installing packages
# These are needed for proper package configuration
groupadd -r -g 999 systemd-journal 2>/dev/null || true
groupadd -r -g 998 systemd-network 2>/dev/null || true
groupadd -r -g 997 systemd-resolve 2>/dev/null || true
groupadd -r -g 996 systemd-timesync 2>/dev/null || true
groupadd -r -g 995 messagebus 2>/dev/null || true

useradd -r -u 998 -g systemd-network -d / -s /usr/sbin/nologin systemd-network 2>/dev/null || true
useradd -r -u 997 -g systemd-resolve -d / -s /usr/sbin/nologin systemd-resolve 2>/dev/null || true
useradd -r -u 996 -g systemd-timesync -d / -s /usr/sbin/nologin systemd-timesync 2>/dev/null || true
useradd -r -u 995 -g messagebus -d /nonexistent -s /usr/sbin/nologin messagebus 2>/dev/null || true

# Add current user to systemd-journal group
usermod -a -G systemd-journal root 2>/dev/null || true

# Use native Ubuntu sources - just ensure they're properly configured
# The base image comes with /etc/apt/sources.list.d/ubuntu.sources in DEB822 format
if [ -f /etc/apt/sources.list.d/ubuntu.sources ]; then
    # Native sources exist, just need GPG keys
    # Temporarily allow unauthenticated to bootstrap keyring
    echo 'Acquire::AllowInsecureRepositories "true";' > /etc/apt/apt.conf.d/99temp-insecure
    apt-get update -qq
    apt-get install -y --no-install-recommends ubuntu-keyring ca-certificates 2>/dev/null || true
    rm -f /etc/apt/apt.conf.d/99temp-insecure
else
    # Fallback: create sources.list if native sources don't exist
    cat > /etc/apt/sources.list << 'SOURCES'
deb http://archive.ubuntu.com/ubuntu/ {codename} main restricted universe multiverse
deb http://archive.ubuntu.com/ubuntu/ {codename}-updates main restricted universe multiverse
deb http://archive.ubuntu.com/ubuntu/ {codename}-security main restricted universe multiverse
SOURCES
fi

# Update with proper authentication now
apt-get update -qq

# Create machine-id before installing dbus
if [ ! -f /etc/machine-id ]; then
    cat /proc/sys/kernel/random/uuid | tr -d '-' > /etc/machine-id
fi
mkdir -p /var/lib/dbus
ln -sf /etc/machine-id /var/lib/dbus/machine-id 2>/dev/null || true

# Install dbus first (needed for proper package configuration)
apt-get install -y --no-install-recommends dbus dbus-user-session 2>&1 || true

# Start dbus system daemon (needed for GTK/dconf)
dbus-daemon --system --fork --nopidfile 2>/dev/null || true

# Install all dependencies
apt-get install -y --no-install-recommends {dependencies} 2>&1 || true

# Force configure all packages (some may fail due to missing systemd, that's OK)
dpkg --configure -a --force-confdef --force-confold --force-depends 2>/dev/null || true

apt-get clean
rm -rf /var/lib/apt/lists/*

echo "Setup complete!"
"#, codename = codename, dependencies = app::DEPENDENCIES.trim());

    let setup_path = rootfs.join("setup.sh");
    fs::write(&setup_path, setup_script).map_err(|e| format!("Failed to write setup.sh: {}", e))?;
    fs::set_permissions(&setup_path, std::os::unix::fs::PermissionsExt::from_mode(0o755))?;

    // Run setup script (may return non-zero due to dpkg config issues in container, that's OK)
    let status_res = Command::new(self_exe)
        .args(["run", "--", "/setup.sh"])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();

    match status_res {
        Ok(status) => {
            if !status.success() {
                // Non-zero exit is expected due to systemd/dbus config issues in container
                // The packages are still installed, just not fully configured
                println!("[{}] Note: Some packages couldn't be fully configured (expected in container)", app::APP_NAME);
            }
        },
        Err(e) => {
             return Err(format!("Failed to spawn child process {:?}: {}", self_exe, e).into());
        }
    }
    let _ = fs::remove_file(&setup_path);

    // 5. Download target app
    update_progress(70, &format!("Downloading {} v{}...", app::TARGET_APP_NAME, app_version), &mut gui_stdin);

    let mut resp = ureq::get(&app_url)
        .header("User-Agent", app::APP_NAME)
        .call()?;
    let len = resp.headers()
        .get("Content-Length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(150_000_000);

    let mut reader = resp.body_mut().with_config().limit(500_000_000).reader();
    let archive_path = rootfs.join(format!("{}{}", app::TARGET_INSTALL_DIR, app::ASSET_EXTENSION));
    let mut out = fs::File::create(&archive_path)?;

    let mut downloaded = 0u64;
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 { break; }
        out.write_all(&buffer[..n])?;
        downloaded += n as u64;

        if downloaded % 2_000_000 < 8192 {
            let pct = 70 + (downloaded * 18 / len);
            update_progress(pct, &format!("Downloading {} v{}...", app::TARGET_APP_NAME, app_version), &mut gui_stdin);
        }
    }
    drop(out);

    update_progress(90, &format!("Installing {}...", app::TARGET_APP_NAME), &mut gui_stdin);

    let target_dir = rootfs.join(format!("opt/{}", app::TARGET_INSTALL_DIR));
    fs::create_dir_all(&target_dir)?;

    // Extract based on archive type
    match app::TARGET_ARCHIVE_TYPE {
        app::ArchiveType::Zip => {
            let file = fs::File::open(&archive_path)?;
            let mut archive = zip::ZipArchive::new(file)?;

            for i in 0..archive.len() {
                let mut file = archive.by_index(i)?;
                let outpath = match file.enclosed_name() {
                    Some(path) => target_dir.join(path),
                    None => continue,
                };

                if file.name().ends_with('/') {
                    fs::create_dir_all(&outpath)?;
                } else {
                    if let Some(p) = outpath.parent() {
                        if !p.exists() { fs::create_dir_all(p)?; }
                    }
                    let mut outfile = fs::File::create(&outpath)?;
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
        }
        app::ArchiveType::TarGz => {
            let file = fs::File::open(&archive_path)?;
            let decoder = GzDecoder::new(file);
            let mut archive = tar::Archive::new(decoder);
            archive.unpack(&target_dir)?;
        }
        app::ArchiveType::TarXz => {
            return Err("TarXz archive type not yet supported".into());
        }
    }

    fs::remove_file(archive_path)?;

    // Symlink
    update_progress(95, "Finalizing...", &mut gui_stdin);

    let mut binary_path = PathBuf::new();
    for entry in WalkDir::new(&target_dir) {
        let entry = entry?;
        if entry.file_name() == app::TARGET_BINARY_NAME && entry.path().is_file() {
            binary_path = entry.path().to_path_buf();
            break;
        }
    }

    if binary_path.as_os_str().is_empty() {
        return Err(format!("{} binary not found in archive", app::TARGET_APP_NAME).into());
    }

    let relative_path = binary_path.strip_prefix(rootfs)?;
    let container_path = Path::new("/").join(relative_path);

    fs::create_dir_all(rootfs.join("usr/bin"))?;
    let link_path = rootfs.join(format!("usr/bin/{}", app::TARGET_BINARY_NAME));
    // Use symlink_metadata to detect broken symlinks (exists() returns false for them)
    if fs::symlink_metadata(&link_path).is_ok() {
        fs::remove_file(&link_path)?;
    }
    std::os::unix::fs::symlink(container_path, link_path)?;

    // Save version info
    let info = InstalledInfo {
        app_version: Some(app_version),
        ubuntu_version: Some(ubuntu_version),
        installed_date: Some(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()),
    };
    save_installed_info(data_dir, &info);

    // Extract app icon for desktop launcher
    let icon_src = target_dir.join(app::TARGET_ICON_FILENAME);
    if icon_src.exists() {
        let icon_dst = data_dir.join(format!("{}.png", app::APP_NAME));
        let _ = fs::copy(&icon_src, &icon_dst);

        // Update .desktop file with the icon if it exists
        let desktop_path = get_desktop_file_path();
        if desktop_path.exists() {
            if let Ok(content) = fs::read_to_string(&desktop_path) {
                let updated = content.replace(
                    &format!("Icon={}", app::DESKTOP_FALLBACK_ICON),
                    &format!("Icon={}", icon_dst.display())
                );
                let _ = fs::write(&desktop_path, updated);
            }
        }
    }

    update_progress(100, "Done! Launching...", &mut gui_stdin);

    if let Some(stdin) = &mut gui_stdin {
        let _ = writeln!(stdin, "EXIT");
    }
    if let Some(mut child) = gui_child {
        let _ = child.wait();
    }

    Ok(())
}

fn setup_container(rootfs: &Path) -> Result<(), Box<dyn std::error::Error>> {
    mount(None::<&str>, "/", None::<&str>, MsFlags::MS_PRIVATE | MsFlags::MS_REC, None::<&str>)?;
    mount(Some(rootfs), rootfs, None::<&str>, MsFlags::MS_BIND | MsFlags::MS_REC, None::<&str>)?;
    chdir(rootfs)?;

    // Bind mounts
    let mounts = [
        ("/sys", "sys", true),
        ("/dev", "dev", false),
        ("/tmp", "tmp", false),
    ];

    for (src, dst, readonly) in mounts {
        let target = rootfs.join(dst);
        if !target.exists() { fs::create_dir_all(&target)?; }
        let mut flags = MsFlags::MS_BIND | MsFlags::MS_REC;
        if readonly { flags |= MsFlags::MS_RDONLY; }
        mount(Some(src), &target, None::<&str>, flags, None::<&str>)?;
    }

    // XDG_RUNTIME_DIR for audio
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        let host_path = Path::new(&runtime_dir);
        if host_path.exists() {
            let relative = host_path.strip_prefix("/").unwrap_or(host_path);
            let target = rootfs.join(relative);
            fs::create_dir_all(&target)?;
            mount(Some(host_path), &target, None::<&str>, MsFlags::MS_BIND | MsFlags::MS_REC, None::<&str>)?;

            unsafe {
                std::env::set_var("XDG_RUNTIME_DIR", format!("/{}", relative.display()));
                std::env::set_var("PULSE_SERVER", format!("unix:/{}/pulse/native", relative.display()));
            }
        }
    }

    // Pivot root
    let old_root = rootfs.join("old_root");
    fs::create_dir_all(&old_root)?;
    pivot_root(".", "old_root")?;
    chdir("/")?;

    // Mount proc
    if !Path::new("/proc").exists() { fs::create_dir("/proc")?; }
    mount(Some("proc"), "/proc", Some("proc"), MsFlags::empty(), None::<&str>)?;

    // Cleanup old root
    umount2("/old_root", MntFlags::MNT_DETACH)?;
    fs::remove_dir("/old_root")?;

    sethostname(app::CONTAINER_HOSTNAME)?;

    unsafe {
        std::env::set_var("PATH", "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin");
        std::env::set_var("HOME", "/root");
    }

    // Create dbus runtime directory and start daemon
    let _ = fs::create_dir_all("/run/dbus");
    let _ = fs::create_dir_all("/var/run/dbus");

    // Start dbus-daemon if available (needed for GTK/dconf)
    if Path::new("/usr/bin/dbus-daemon").exists() {
        let _ = Command::new("/usr/bin/dbus-daemon")
            .args(["--system", "--fork", "--nopidfile"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    Ok(())
}
