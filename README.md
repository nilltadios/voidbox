# Voidbox

**Universal Linux App Platform** - Portable, isolated application environments using Linux user namespaces.

## Features

- **Single Binary**: No external dependencies (no Docker, Podman, or Flatpak required)
- **Multi-App Support**: Install any app using TOML manifests
- **Shared Base Images**: OverlayFS base + per-app layers
- **Auto-Install**: First run installs to `~/.local/bin` and creates desktop launchers
- **.voidbox Installers**: Self-extracting, double-clickable single-file apps
- **Auto-Update**: Automatically updates apps and voidbox itself
- **Portable**: Works on Fedora, Ubuntu, Debian, Arch, and more
- **Isolated**: Runs in dedicated User/Mount/PID namespaces
- **Hardware Accelerated**: Full GPU and Audio (PipeWire/PulseAudio) passthrough
- **Home Folder Access**: Apps can access your home folder by default
- **Theme Integration**: Host fonts, themes, and icons are available in containers
- **Developer Mode**: Access host tools (pip, npm, cargo) inside containers

## Quick Start

```bash
# Download and run
chmod +x voidbox
./voidbox

# Install an app from a manifest
voidbox install brave.toml

# Run an installed app
voidbox run brave

# List installed apps
voidbox list
```

## .voidbox Installers (Single-File Apps)

Create a self-extracting `.voidbox` installer that works like a Linux `.exe`:

```bash
voidbox bundle create ./myapp.toml ./myapp.zip --output MyApp.voidbox
chmod +x MyApp.voidbox
./MyApp.voidbox
```

Double-clicking `MyApp.voidbox` opens a GUI installer and requires no terminal.

You can also install from an existing file:

```bash
voidbox bundle install ./MyApp.voidbox
```

## Commands

```
voidbox install <manifest>   # Install from manifest file, URL, or registry
voidbox remove <app>         # Remove an installed app
voidbox remove --purge <app> # Remove app and all data
voidbox run <app>            # Run an installed app
voidbox run <app> --url URL  # Run app with a URL (browsers)
voidbox run <app> --dev      # Run with developer mode (host tools)
voidbox list                 # List installed apps
voidbox update               # Update all apps
voidbox update <app>         # Update specific app
voidbox self-update          # Update voidbox itself
voidbox shell <app>          # Open shell in app's container
voidbox info                 # Show voidbox info
voidbox info <app>           # Show app details
voidbox uninstall            # Remove voidbox (keeps app data)
voidbox uninstall --purge    # Remove voidbox and all data
voidbox bundle create <manifest> <archive>   # Create a .voidbox installer
voidbox bundle install <bundle.voidbox>      # Install from a .voidbox file
```

## Manifest Format

Apps are defined using TOML manifests:

```toml
[app]
name = "brave"
display_name = "Brave Browser"
description = "Privacy-focused browser"

[source]
type = "github"
owner = "brave"
repo = "brave-browser"
asset_os = "linux"
asset_arch = "amd64"
asset_extension = ".zip"

[runtime]
base = "ubuntu:24.04"

[dependencies]
packages = ["libnss3", "libgtk-3-0t64", "libpulse0"]

[binary]
name = "brave"
args = ["--no-sandbox"]

[desktop]
categories = ["Network", "WebBrowser"]
wm_class = "brave-browser"

[permissions]
network = true
audio = true
gpu = true
home = true
dev_mode = false
```

See `examples/manifests/` for more examples.

## Building from Source

Requirements: Rust 1.85+ (uses Rust 2024 edition)

```bash
cargo build --release
./target/release/voidbox
```

## How it Works

1. Parses the app manifest to get download URL and dependencies
2. Downloads a shared Ubuntu base rootfs (once per base + arch)
3. Sets up Linux namespaces (user, mount, PID, UTS, IPC)
4. Creates a per-app overlay layer and installs dependencies
5. Downloads and extracts the target application into the layer
6. Bind-mounts host hardware interfaces (GPU, audio, Wayland/X11)
7. Bind-mounts home folder, fonts, themes (based on permissions)
8. Launches the app in the isolated container

## Directory Structure

```
~/.local/share/voidbox/
├── bases/                   # Shared base images
│   └── ubuntu-24.04-amd64/
├── apps/                    # Per-app installations
│   └── brave/
│       ├── base.json        # Base metadata
│       ├── layer/           # App layer (upperdir)
│       ├── work/            # Overlay workdir
│       └── rootfs/          # Overlay mountpoint
├── manifests/               # Saved app manifests
│   └── brave.toml
├── settings/                # User permission overrides
├── icons/                   # Extracted app icons
└── installed.json           # App database
```

## Permissions

All permissions default to **open** (enabled). Users can restrict permissions:

| Permission | Default | Description |
|------------|---------|-------------|
| network | true | Internet access |
| audio | true | Audio output |
| microphone | true | Audio input |
| gpu | true | GPU acceleration |
| camera | true | Webcam access |
| home | true | Home folder access |

| downloads | true | Downloads folder |
| fonts | true | Host fonts |
| themes | true | Host GTK/Qt themes |
| dev_mode | false | Access to host tools |

## License

MIT
