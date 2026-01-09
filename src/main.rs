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
const BRAVE_RELEASES_API: &str = "https://api.github.com/repos/brave/brave-browser/releases/latest";
const UBUNTU_BASE_URL: &str = "http://cdimage.ubuntu.com/ubuntu-base/releases/22.04/release/ubuntu-base-22.04.2-base-amd64.tar.gz";

#[derive(Parser)]
#[command(name = "void_runner")]
#[command(version = VERSION)]
#[command(about = "Portable Isolated Brave Browser - No container runtime needed", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Launch Brave or a command inside the box
    Run {
        /// URL to open (if running default brave)
        #[arg(long)]
        url: Option<String>,

        /// Command to run (overrides default brave)
        #[arg(trailing_var_arg = true)]
        cmd: Vec<String>,

        /// Force rebuild of environment
        #[arg(long)]
        rebuild: bool,
    },
    /// Update Brave browser to latest version
    Update {
        /// Force update even if already on latest
        #[arg(long)]
        force: bool,
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
    brave_version: Option<String>,
    installed_date: Option<String>,
}

fn get_data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("void_runner")
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

fn fetch_latest_brave_release() -> Result<(String, String), Box<dyn std::error::Error>> {
    let resp = ureq::get(BRAVE_RELEASES_API)
        .set("User-Agent", "void_runner")
        .call()?;

    let release: GitHubRelease = resp.into_json()?;
    let version = release.tag_name.trim_start_matches('v').to_string();

    // Find linux amd64 zip
    for asset in release.assets {
        if asset.name.contains("linux") && asset.name.contains("amd64") && asset.name.ends_with(".zip") {
            return Ok((version, asset.browser_download_url));
        }
    }

    Err("No Linux amd64 zip found in release".into())
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

    match command {
        Commands::Run { url, cmd, rebuild } => {
            let rootfs = data_dir.join("rootfs");

            if rebuild && rootfs.exists() {
                println!("[void_runner] Rebuild requested. Removing old rootfs...");
                fs::remove_dir_all(&rootfs)?;
            }

            // Check if installation is complete (brave symlink exists)
            let brave_link = rootfs.join("usr/bin/brave");
            // Only enforce build check if we are running default brave
            let is_default_run = cmd.is_empty();
            
            let needs_build = if is_default_run {
                // Check if rootfs exists and the symlink exists (do NOT follow it, as it points to /opt inside container)
                !rootfs.exists() || fs::symlink_metadata(&brave_link).is_err()
            } else {
                !rootfs.exists()
            };

            if needs_build && rootfs.exists() && is_default_run {
                // Incomplete install - remove and rebuild
                // Only do this if we expected to run Brave but it's missing
                println!("[void_runner] Incomplete installation detected (missing {:?}). Rebuilding...", brave_link);
                fs::remove_dir_all(&rootfs)?;
            }

            if needs_build {
                let is_tty = unsafe { libc::isatty(1) == 1 };

                if !is_tty {
                    // Spawn terminal for first-run setup
                    // ... (existing terminal spawning code) ...
                }

                println!("[void_runner] Building isolated environment...");
                build_environment(&data_dir, &rootfs, &std::env::current_exe()?)?;
            } else {
                // Check for updates on launch (if not building)
                // We run this in a non-blocking way or quick check
                if let Ok(info) = std::fs::read_to_string(data_dir.join("installed.json")) {
                    if let Ok(installed) = serde_json::from_str::<InstalledInfo>(&info) {
                        // Only check if it's been more than 24 hours or if we just want to be safe
                        // For responsiveness, we'll spawn a background thread/process or just check quickly
                        // Here we do a blocking check but print nicely.
                        println!("[void_runner] Checking for updates...");
                        if let Ok((latest, url)) = fetch_latest_brave_release() {
                            if installed.brave_version.as_deref() != Some(&latest) {
                                println!("[void_runner] Update available: v{} -> v{}", installed.brave_version.as_deref().unwrap_or("?"), latest);
                                println!("[void_runner] Auto-updating...");
                                if let Err(e) = update_brave(&rootfs, &url, &latest) {
                                    println!("[void_runner] Update failed: {}", e);
                                } else {
                                    let mut new_info = installed;
                                    new_info.brave_version = Some(latest.clone());
                                    new_info.installed_date = Some(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
                                    save_installed_info(&data_dir, &new_info);
                                    println!("[void_runner] Updated to v{}", latest);
                                }
                            }
                        }
                    }
                }
            }

            // Determine command to run
            let (run_cmd, run_args) = if cmd.is_empty() {
                let mut args = vec![
                    "--no-sandbox".to_string(),
                    "--disable-dev-shm-usage".to_string(),
                    "--test-type".to_string(),
                ];
                if let Some(u) = url {
                    args.push(u);
                }
                ("/usr/bin/brave".to_string(), args)
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
            println!("[void_runner] Checking for Brave updates...");

            let info = load_installed_info(&data_dir);
            let (latest_version, download_url) = fetch_latest_brave_release()?;

            println!("  Installed: {}", info.brave_version.as_deref().unwrap_or("unknown"));
            println!("  Latest:    {}", latest_version);

            let needs_update = force || info.brave_version.as_deref() != Some(&latest_version);

            if !needs_update {
                println!("[void_runner] Already running latest version.");
                return Ok(());
            }

            println!("[void_runner] Updating Brave to v{}...", latest_version);

            let rootfs = data_dir.join("rootfs");
            if !rootfs.exists() {
                println!("[void_runner] No installation found. Run 'void_runner' first to install.");
                return Ok(());
            }

            // Download and install new Brave
            update_brave(&rootfs, &download_url, &latest_version)?;

            // Save new version info
            let mut new_info = info;
            new_info.brave_version = Some(latest_version.clone());
            new_info.installed_date = Some(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
            save_installed_info(&data_dir, &new_info);

            println!("[void_runner] Update complete! Brave v{} installed.", latest_version);
        }

        Commands::Info => {
            println!("void_runner v{}", VERSION);
            println!("Portable Isolated Brave Browser");
            println!();

            let info = load_installed_info(&data_dir);
            let rootfs = data_dir.join("rootfs");

            println!("Data directory: {}", data_dir.display());
            println!("Rootfs exists:  {}", rootfs.exists());

            if let Some(v) = &info.brave_version {
                println!("Brave version:  {}", v);
            }
            if let Some(d) = &info.installed_date {
                println!("Installed:      {}", d);
            }

            // Check for updates
            println!();
            print!("Checking for updates... ");
            match fetch_latest_brave_release() {
                Ok((latest, _)) => {
                    if info.brave_version.as_deref() == Some(&latest) {
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

fn update_brave(rootfs: &Path, download_url: &str, version: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("  Downloading Brave v{}...", version);

    let resp = ureq::get(download_url)
        .set("User-Agent", "void_runner")
        .call()?;

    let zip_path = rootfs.join("brave_update.zip");
    let mut out = fs::File::create(&zip_path)?;
    let mut reader = resp.into_reader();
    std::io::copy(&mut reader, &mut out)?;
    drop(out);

    println!("  Extracting...");

    // Remove old brave
    let brave_dir = rootfs.join("opt/brave");
    if brave_dir.exists() {
        fs::remove_dir_all(&brave_dir)?;
    }
    fs::create_dir_all(&brave_dir)?;

    // Extract new
    let file = fs::File::open(&zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = match file.enclosed_name() {
            Some(path) => brave_dir.join(path),
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

    fs::remove_file(zip_path)?;

    // Update symlink
    let mut binary_path = PathBuf::new();
    for entry in WalkDir::new(&brave_dir) {
        let entry = entry?;
        if entry.file_name() == "brave" && entry.path().is_file() {
            binary_path = entry.path().to_path_buf();
            break;
        }
    }

    if binary_path.as_os_str().is_empty() {
        return Err("Brave binary not found".into());
    }

    let relative_path = binary_path.strip_prefix(rootfs)?;
    let container_path = Path::new("/").join(relative_path);

    let link_path = rootfs.join("usr/bin/brave");
    if link_path.exists() {
        fs::remove_file(&link_path)?;
    }
    std::os::unix::fs::symlink(container_path, link_path)?;

    Ok(())
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

    // 1. Fetch latest Brave version
    update_progress(2, "Fetching latest Brave version...", &mut gui_stdin);
    let (brave_version, brave_url) = fetch_latest_brave_release()?;

    // 2. Download Ubuntu Base
    update_progress(5, "Downloading Ubuntu Base...", &mut gui_stdin);

    let resp = ureq::get(UBUNTU_BASE_URL).call()?;
    let len = resp.header("Content-Length").and_then(|s| s.parse::<u64>().ok()).unwrap_or(28_000_000);

    let mut reader = resp.into_reader();
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
            update_progress(pct, "Downloading Ubuntu Base...", &mut gui_stdin);
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

    let setup_script = r#"#!/bin/bash
export DEBIAN_FRONTEND=noninteractive
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

mkdir -p /tmp /etc/apt/apt.conf.d
echo 'APT::Sandbox::User "root";' > /etc/apt/apt.conf.d/sandbox

cat > /etc/apt/sources.list << 'EOF'
deb [trusted=yes] http://archive.ubuntu.com/ubuntu/ jammy main restricted universe multiverse
deb [trusted=yes] http://archive.ubuntu.com/ubuntu/ jammy-updates main restricted universe multiverse
deb [trusted=yes] http://archive.ubuntu.com/ubuntu/ jammy-security main restricted universe multiverse
EOF

apt-get update -qq
apt-get install -y -qq --no-install-recommends \
    ca-certificates curl unzip libnss3 libatk1.0-0 libatk-bridge2.0-0 \
    libcups2 libdrm2 libxkbcommon0 libxcomposite1 libxdamage1 libxfixes3 \
    libxrandr2 libgbm1 libpango-1.0-0 libcairo2 libasound2 libx11-xcb1 \
    libx11-6 libxcb1 libdbus-1-3 libglib2.0-0 libgtk-3-0 libgl1-mesa-dri \
    libgl1-mesa-glx mesa-vulkan-drivers libegl1 libgles2 libpulse0 \
    libasound2-plugins fonts-liberation > /dev/null 2>&1

apt-get clean
rm -rf /var/lib/apt/lists/*
"#;

    let setup_path = rootfs.join("setup.sh");
    fs::write(&setup_path, setup_script).map_err(|e| format!("Failed to write setup.sh: {}", e))?;
    fs::set_permissions(&setup_path, std::os::unix::fs::PermissionsExt::from_mode(0o755))?;

    // Enable stderr for debugging
    let status_res = Command::new(self_exe)
        .args(["run", "--", "/setup.sh"])
        .stdout(Stdio::inherit()) 
        .stderr(Stdio::inherit())
        .status();

    match status_res {
        Ok(status) => {
            if !status.success() {
                update_progress(0, "Error: Dependency install failed", &mut gui_stdin);
                return Err(format!("Dependency installation failed (exit code: {:?})", status.code()).into());
            }
        },
        Err(e) => {
             return Err(format!("Failed to spawn child process {:?}: {}", self_exe, e).into());
        }
    }
    fs::remove_file(&setup_path)?;

    // 5. Download Brave
    update_progress(70, &format!("Downloading Brave v{}...", brave_version), &mut gui_stdin);

    let resp = ureq::get(&brave_url)
        .set("User-Agent", "void_runner")
        .call()?;
    let len = resp.header("Content-Length").and_then(|s| s.parse::<u64>().ok()).unwrap_or(150_000_000);

    let mut reader = resp.into_reader();
    let zip_path = rootfs.join("brave.zip");
    let mut out = fs::File::create(&zip_path)?;

    let mut downloaded = 0u64;
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 { break; }
        out.write_all(&buffer[..n])?;
        downloaded += n as u64;

        if downloaded % 2_000_000 < 8192 {
            let pct = 70 + (downloaded * 18 / len);
            update_progress(pct, &format!("Downloading Brave v{}...", brave_version), &mut gui_stdin);
        }
    }
    drop(out);

    update_progress(90, "Installing Brave...", &mut gui_stdin);
    let file = fs::File::open(&zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    let target_dir = rootfs.join("opt/brave");
    fs::create_dir_all(&target_dir)?;

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

    fs::remove_file(zip_path)?;

    // Symlink
    update_progress(95, "Finalizing...", &mut gui_stdin);

    let mut binary_path = PathBuf::new();
    for entry in WalkDir::new(&target_dir) {
        let entry = entry?;
        if entry.file_name() == "brave" && entry.path().is_file() {
            binary_path = entry.path().to_path_buf();
            break;
        }
    }

    if binary_path.as_os_str().is_empty() {
        return Err("Brave binary not found in zip".into());
    }

    let relative_path = binary_path.strip_prefix(rootfs)?;
    let container_path = Path::new("/").join(relative_path);

    fs::create_dir_all(rootfs.join("usr/bin"))?;
    let link_path = rootfs.join("usr/bin/brave");
    if link_path.exists() {
        fs::remove_file(&link_path)?;
    }
    std::os::unix::fs::symlink(container_path, link_path)?;

    // Save version info
    let info = InstalledInfo {
        brave_version: Some(brave_version),
        installed_date: Some(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()),
    };
    save_installed_info(data_dir, &info);

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

    sethostname("void-runner")?;

    unsafe {
        std::env::set_var("PATH", "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin");
        std::env::set_var("HOME", "/root");
    }

    Ok(())
}
