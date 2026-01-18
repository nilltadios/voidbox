# Void Runner

A single-binary, portable, and isolated Brave Browser environment for Linux.

## Features

- **Single Binary**: No external dependencies (no Docker, Podman, or Flatpak required)
- **Auto-Install**: First run installs to `~/.local/bin` and creates a desktop launcher
- **Auto-Update**: Automatically updates both Brave and void_runner itself on launch
- **Portable**: Works on Fedora, Ubuntu, Debian, Arch, and more
- **Isolated**: Runs in a dedicated User/Mount/PID namespace
- **Hardware Accelerated**: Full GPU and Audio (Pipewire/PulseAudio) passthrough

## Installation

Download `void_runner`, make it executable, and run it:

```bash
chmod +x void_runner
./void_runner
```

On first run, it will:
1. Install itself to `~/.local/bin/void_runner`
2. Create a desktop launcher (appears in your app menu)
3. Download and set up an isolated Brave browser environment
4. Launch Brave

After installation, just run `void_runner` from anywhere or click the app launcher.

## Commands

```
void_runner              # Launch Brave (default)
void_runner run          # Launch Brave
void_runner run --url    # Launch Brave with a specific URL
void_runner update       # Update Brave browser
void_runner self-update  # Update void_runner itself
void_runner uninstall    # Remove void_runner (keeps browser data)
void_runner uninstall --purge  # Remove everything
void_runner info         # Show version and update status
```

## Building from Source

Requirements: Rust 1.85+ (uses Rust 2024 edition)

```bash
cargo build --release
./target/release/void_runner
```

## How it Works

1. Downloads a minimal Ubuntu base rootfs
2. Sets up Linux namespaces (user, mount, PID, UTS, IPC)
3. Installs Brave and dependencies inside the isolated environment
4. Bind-mounts host hardware interfaces (GPU, audio, Wayland/X11)
5. Launches Brave in the isolated container

## Uninstalling

```bash
void_runner uninstall          # Removes binary and launcher, keeps browser data
void_runner uninstall --purge  # Removes everything including browser data
```

## Forking for Other Apps

This project is designed to be easily forked for other applications like Discord, Firefox, VSCode, etc.

All app-specific configuration is in **`src/app.rs`**. To fork:

1. **Fork this repo**
2. **Edit `src/app.rs`** - Change these values:
   - `APP_NAME` - Binary name and data directory
   - `APP_DISPLAY_NAME` - Name shown in app launcher
   - `TARGET_APP_NAME` - The app you're containerizing
   - `RELEASES_API` - GitHub releases API URL for your app
   - `ASSET_*` patterns - How to find the right download asset
   - `TARGET_BINARY_NAME` - The executable name inside the archive
   - `TARGET_INSTALL_DIR` - Where to extract in /opt/
   - `TARGET_ARCHIVE_TYPE` - Zip, TarGz, or TarXz
   - `DEPENDENCIES` - Ubuntu packages your app needs
   - `DEFAULT_LAUNCH_ARGS` - Command-line args for launching
   - Desktop entry fields (categories, WM class, icon)
   - Self-update GitHub owner/repo

3. **Update `Cargo.toml`** - Change package name/version
4. **Build and release!**

Example: To fork for Firefox, you'd change:
```rust
pub const APP_NAME: &str = "firefox_box";
pub const TARGET_APP_NAME: &str = "Firefox";
pub const RELEASES_API: Option<&str> = None;  // Firefox uses direct downloads
pub const TARGET_BINARY_NAME: &str = "firefox";
// ... etc
```

For apps not on GitHub releases, set `RELEASES_API` to `None` and implement custom download logic.

## License

MIT
