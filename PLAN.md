# Voidbox: Universal Linux App Platform

## Vision Statement

**"Linux apps that work like native apps, on any distro."**

A universal Linux application platform that prioritizes portability and usability over restrictive sandboxing. Apps should just work - with full hardware access, host tool integration, and offline capability - while giving users the option to restrict permissions if they choose.

---

## Table of Contents

1. [Philosophy](#philosophy)
2. [Current State (v0.7.1)](#current-state-v071)
3. [Competitor Analysis](#competitor-analysis)
4. [Target Users](#target-users)
5. [Architecture](#architecture)
6. [Implementation Phases](#implementation-phases)
7. [Technical Specifications](#technical-specifications)
8. [Design Decisions](#design-decisions)
9. [Success Metrics](#success-metrics)

---

## Philosophy

### Core Principles

| Principle | Description |
|-----------|-------------|
| **Portability First** | Works on any Linux distro with kernel 3.8+ |
| **Usability First** | Apps work out of the box, no configuration needed |
| **Open by Default** | Full hardware/filesystem access, user can opt-out |
| **No Corporate Lock-in** | Decentralized, community-driven, fully open source |
| **No Daemon** | No background services, no boot overhead |
| **No Root** | Everything runs as regular user via user namespaces |
| **Offline Capable** | Install once, works forever without internet |
| **Developer Friendly** | Host tools (pip, npm, cargo) visible inside container |

### Philosophy Comparison

| Aspect | Flatpak | Snap | AppImage | Voidbox |
|--------|---------|------|----------|---------|
| **Primary Goal** | Security | Control | Simplicity | Portability |
| **Default Permissions** | Restrictive | Restrictive | Full (no sandbox) | Full (with sandbox) |
| **User Trust** | Distrusted | Distrusted | Trusted | Trusted |
| **Dev Workflow** | Broken | Broken | Works | Works |
| **Corporate Backing** | Red Hat | Canonical | Community | Community |
| **Update Policy** | User choice | Forced | Manual | User choice |

### What We Are NOT

- NOT trying to be "more secure than Flatpak"
- NOT trying to replace system package managers
- NOT trying to sandbox everything
- NOT trying to control users

### What We ARE

- Making Linux apps portable across all distros
- Making apps "just work" without configuration
- Giving users choice, not mandates
- Keeping things simple and transparent

---

## Current State (v0.7.1)

### What Works

| Feature | Status |
|---------|--------|
| Single binary distribution | ✅ |
| No root required | ✅ |
| No daemon required | ✅ |
| Auto-install to ~/.local/bin | ✅ |
| Desktop launcher creation | ✅ |
| Shared base images (OverlayFS) | ✅ |
| .voidbox self-extracting installers | ✅ |
| Auto-update (self) | ✅ |
| Auto-update (target app) | ✅ |
| Shared dependency layers (dedupe apt) | ✅ |
| Desktop file associations (Open With) | ✅ |
| Direct source update checks (version_url) | ✅ |
| GPU passthrough | ✅ |
| Audio passthrough (PulseAudio/PipeWire) | ✅ |
| Wayland/X11 support | ✅ |
| Offline launch (if update fails) | ✅ |
| Manifest-driven installs | ✅ |
| Uninstall command | ✅ |
| Host tool integration (native_mode) | ✅ |
| Host Bridge (sudo/interactive tools) | ✅ |

### Current Architecture

```
voidbox (single binary, ~5MB)
         │
         ▼
┌─────────────────────────────────────────┐
│         Linux User Namespaces           │
│  • User (UID/GID mapping)               │
│  • Mount (isolated filesystem)          │
│  • PID (isolated processes)             │
│  • UTS (hostname)                       │
│  • IPC (inter-process communication)    │
└─────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────┐
│    ~/.local/share/voidbox/              │
│    ├── bases/ubuntu-24.04-amd64/         │
│    ├── deps/ubuntu-24.04-amd64-deps-.../ │
│    ├── apps/brave/                       │
│    │   ├── base.json                     │
│    │   ├── layer/                        │
│    │   ├── work/                         │
│    │   └── rootfs/ (overlay mount)       │
│    ├── manifests/brave.toml              │
│    └── installed.json (version info)     │
└─────────────────────────────────────────┘
```

### Current Limitations

| Limitation | Impact | Solution |
|------------|--------|----------|
| No app discovery | Users must find manifests manually | Phase 4: Registry |
| No GUI settings | Terminal only for configuration | Phase 3: Settings app |
| No bundle signing | Users must trust the source | Phase 3: Signatures |
| Limited arm64 manifests | Some apps only ship x86_64 | Phase 2: Dual-arch bundles |

---

## Competitor Analysis

### Snap

**Developer**: Canonical
**First Release**: 2014
**Technology**: Squashfs + AppArmor + snapd daemon

#### How It Works
```
Snap Store (closed source backend)
         │
         ▼
┌─────────────────────────────────────────┐
│              snapd daemon               │
│  • Always running                       │
│  • Mounts snaps as loopback devices     │
│  • Forces automatic updates             │
└─────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────┐
│            Squashfs images              │
│  /snap/<name>/<revision>/               │
│  (compressed, read-only)                │
└─────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────┐
│   AppArmor + Seccomp + Namespaces       │
│   (confinement)                         │
└─────────────────────────────────────────┘
```

#### Snap Flaws

| Flaw | Details | Severity |
|------|---------|----------|
| **Extremely slow cold start** | Firefox: 10-15 seconds vs 2s native. Squashfs decompression is single-threaded. | Critical |
| **Forced centralization** | Snap Store backend is closed source. No self-hosting, no mirrors. | Critical |
| **Mandatory auto-updates** | Cannot disable. Breaks offline use, stability, metered connections. | Critical |
| **Daemon required** | snapd always running, consumes RAM, adds boot time. | High |
| **Broken desktop integration** | Themes don't apply, fonts differ, icons missing. | High |
| **AppArmor dependency** | Full confinement only on Ubuntu. Incomplete on Debian/others. | High |
| **Mount point pollution** | Each snap = loopback device. Users see dozens of /dev/loop* in df. | Medium |
| **Research confirmed** | Academic study (2024): snaps are "bloated and outdated" on average. | Medium |

### Flatpak

**Developer**: FreeDesktop.org (Red Hat sponsored)
**First Release**: 2015
**Technology**: OSTree + Bubblewrap + Portals

#### How It Works
```
Flathub (central repository)
         │
         ▼
┌─────────────────────────────────────────┐
│              OSTree                      │
│  • Content-addressable storage          │
│  • Deduplication                        │
│  • /var/lib/flatpak/ (~5-15GB)          │
└─────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────┐
│    Runtimes (org.gnome, org.kde)        │
│    ~1-2GB each                          │
└─────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────┐
│            Bubblewrap                   │
│            (sandbox)                    │
└─────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────┐
│             Portals                     │
│  (D-Bus services for host access)       │
│  FileChooser, Notifications, etc.       │
└─────────────────────────────────────────┘
```

#### Flatpak Flaws

| Flaw | Details | Severity |
|------|---------|----------|
| **Massive disk usage** | GNOME runtime: ~1.8GB, KDE: ~2GB. Simple app needs 500MB+ runtime. Users report 11GB+ accumulated. | Critical |
| **Security theater** | Most apps request filesystem=home, defeating sandbox. Builds not reproducible. Permission escalation possible. | Critical |
| **Developer workflow broken** | Host pip/npm/cargo invisible. IDE can't see system tools. Must install everything twice. | Critical |
| **File access nightmare** | Default: can't access ~/Documents. Need Flatseal (3rd party) to fix. Portal friction. | High |
| **Theme isolation** | Can't use host GTK/Qt themes. Different font rendering. | High |
| **Wayland/X11 issues** | Blurry on KDE Wayland, cursor problems, some apps only work on one or other. | High |
| **Complex portal system** | Each feature needs separate D-Bus portal. | Medium |
| **Silent runtime breakage** | Updates can break apps with no warning. | Medium |
| **7-year-old issues unfixed** | Misleading sandbox indicators still not solved. | Medium |

### AppImage

**Developer**: Community (probonopd)
**First Release**: 2004 (as klik), 2013 (as AppImage)
**Technology**: ELF + Squashfs + FUSE

#### How It Works
```
┌─────────────────────────────────────────┐
│         AppImage File                   │
│  ┌───────────────────────────────────┐  │
│  │  ELF Header (runtime)             │  │
│  ├───────────────────────────────────┤  │
│  │  Squashfs Image                   │  │
│  │  • /usr/bin/app                   │  │
│  │  • /usr/lib/ (bundled libs)       │  │
│  │  • /app.desktop                   │  │
│  │  • /app.png                       │  │
│  └───────────────────────────────────┘  │
└─────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────┐
│              FUSE mount                 │
│         NO SANDBOX - runs as user       │
└─────────────────────────────────────────┘
```

#### AppImage Flaws

| Flaw | Details | Severity |
|------|---------|----------|
| **Zero security** | No sandbox at all. Full user permissions. "Perfect tool to distribute malware." | Critical |
| **No update mechanism** | User must manually download new versions. No notifications. | Critical |
| **libfuse2 disaster** | Built for old libfuse2, Ubuntu/Fedora use libfuse3. "Works everywhere" broken. | Critical |
| **No desktop integration** | No menu entries, no file associations, no icons. Need AppImageLauncher (3rd party). | High |
| **No verification** | No signature checking. Download from random websites. | High |
| **Portability is a lie** | glibc mismatches, library conflicts. Works on one distro ≠ works everywhere. | High |
| **Theming impossible** | GTK/Qt themes hardcoded in bundle. Can't match desktop. | Medium |
| **No central repository** | AppImageHub is unofficial, incomplete. | Medium |

### Competitor Summary

| Aspect | Snap | Flatpak | AppImage | Voidbox (Goal) |
|--------|------|---------|----------|----------------|
| Startup speed | ❌ Very slow | ⚠️ Slow | ✅ Fast | ✅ Fast |
| Disk usage | ❌ Bloated | ❌ Very bloated | ✅ Reasonable | ✅ Minimal |
| Security | ⚠️ Partial | ⚠️ Theater | ❌ None | ✅ Real (optional) |
| Updates | ❌ Forced | ⚠️ Manual | ❌ None | ✅ User choice |
| Offline | ❌ Problematic | ✅ Works | ✅ Works | ✅ Works |
| Dev workflow | ❌ Broken | ❌ Broken | ✅ Works | ✅ Works |
| Daemon | ❌ Required | ✅ None | ✅ None | ✅ None |
| Corporate control | ❌ Canonical | ⚠️ Red Hat | ✅ Community | ✅ Community |
| Desktop integration | ❌ Broken | ⚠️ Partial | ❌ None | ✅ Native |

---

## Target Users

### Primary: Developers

**Pain point**: Flatpak VSCode can't see system pip/npm/node

```
Developer installs VSCode via Flatpak
    │
    ├── pip install numpy (on host)
    │       │
    │       └── NOT visible in VSCode
    │
    ├── npm install (on host)
    │       │
    │       └── node_modules NOT accessible
    │
    └── Must install everything TWICE
        or grant filesystem=host (defeats sandbox)
```

**Voidbox solution**: Bind mount host tools, home folder accessible by default

### Secondary: Power Users

**Pain point**: Flatpak needs Flatseal to access own files

```
User opens Flatpak app
    │
    ├── Try to open ~/Documents/file.pdf
    │       │
    │       └── "Permission denied" or Portal dialog
    │
    ├── Must install Flatseal
    │
    ├── Find correct permission toggle
    │
    └── Hope it works
```

**Voidbox solution**: Home folder accessible by default, GUI to opt-out

### Tertiary: Minimalists

**Pain point**: Snap daemon always running, slow startup

```
System boot
    │
    ├── snapd daemon starts (RAM usage)
    │
    ├── Loop devices mounted for each snap
    │
    └── First app launch: 10-15 second wait
```

**Voidbox solution**: No daemon, no background services, native startup speed

### Quaternary: Offline/Air-gapped Users

**Pain point**: Snap forces updates, assumes internet

**Voidbox solution**: Update check fails gracefully, app always launches

---

## Architecture

### Proposed Architecture (Multi-App)

```
┌─────────────────────────────────────────────────────────────────┐
│                         voidbox CLI                              │
│    voidbox search | install | run | update | remove | settings   │
├─────────────────────────────────────────────────────────────────┤
│                              │                                   │
│         ┌────────────────────┼────────────────────┐              │
│         ▼                    ▼                    ▼              │
│  ┌─────────────┐     ┌─────────────┐     ┌─────────────┐        │
│  │  Registry   │     │  Manifest   │     │  Settings   │        │
│  │  (remote)   │     │   Parser    │     │   Store     │        │
│  │             │     │   (TOML)    │     │  (per-app)  │        │
│  └─────────────┘     └─────────────┘     └─────────────┘        │
│         │                    │                    │              │
│         └────────────────────┼────────────────────┘              │
│                              ▼                                   │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                    Storage Layer                          │   │
│  │  ~/.local/share/voidbox/                                  │   │
│  │  ├── bases/                                               │   │
│  │  │   ├── ubuntu-24.04/        # Shared, ~300MB            │   │
│  │  │   └── alpine-3.19/         # Shared, ~8MB              │   │
│  │  ├── apps/                                                │   │
│  │  │   ├── brave/               # App layer                 │   │
│  │  │   ├── discord/             # App layer                 │   │
│  │  │   └── firefox/             # App layer                 │   │
│  │  ├── manifests/                                           │   │
│  │  │   ├── brave.toml                                       │   │
│  │  │   └── discord.toml                                     │   │
│  │  ├── settings/                                            │   │
│  │  │   ├── brave.toml           # User overrides            │   │
│  │  │   └── discord.toml                                     │   │
│  │  └── db.sqlite                # Local state               │   │
│  └──────────────────────────────────────────────────────────┘   │
│                              │                                   │
│                              ▼                                   │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                      OverlayFS                            │   │
│  │         [base layer] + [app layer] = merged view          │   │
│  └──────────────────────────────────────────────────────────┘   │
│                              │                                   │
│                              ▼                                   │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │              Linux Namespaces (unprivileged)              │   │
│  │         User | Mount | PID | UTS | IPC | (opt: Net)       │   │
│  └──────────────────────────────────────────────────────────┘   │
│                              │                                   │
│                              ▼                                   │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                    Bind Mounts                            │   │
│  │  /dev, /sys, /tmp, $XDG_RUNTIME_DIR                       │   │
│  │  $HOME (default ON), /usr/share/themes, /usr/share/fonts  │   │
│  └──────────────────────────────────────────────────────────┘   │
│                                                                  │
└──────────────────────────────────────────────────────────────────┘
```

### Directory Structure

```
~/.local/share/voidbox/
├── bases/                          # Shared base layers
│   ├── ubuntu-24.04/               # Full Ubuntu base (~300MB)
│   │   ├── etc/
│   │   ├── usr/
│   │   └── ...
│   └── alpine-3.19/                # Minimal Alpine base (~8MB)
│       └── ...
│
├── apps/                           # Per-app layers
│   ├── brave/
│   │   ├── layer/                  # OverlayFS upper layer
│   │   │   └── opt/brave/          # Extracted app
│   │   ├── work/                   # OverlayFS work directory
│   │   └── merged/                 # Runtime mount point
│   │
│   └── discord/
│       ├── layer/
│       ├── work/
│       └── merged/
│
├── manifests/                      # App definitions
│   ├── brave.toml
│   ├── discord.toml
│   └── ...
│
├── settings/                       # User permission overrides
│   ├── brave.toml                  # e.g., camera = false
│   └── discord.toml
│
├── desktop/                        # Generated .desktop files
│   ├── brave.desktop
│   └── discord.desktop
│
├── icons/                          # Extracted app icons
│   ├── brave.png
│   └── discord.png
│
├── bin/                            # Symlinks for PATH
│   ├── brave -> voidbox run brave
│   └── discord -> voidbox run discord
│
└── db.sqlite                       # Local state database
```

### Bind Mount Configuration (Default)

```
Host Path                         Container Path              Mode
─────────────────────────────────────────────────────────────────────
/sys                            → /sys                        read-only
/dev                            → /dev                        read-write
/tmp                            → /tmp                        read-write
$XDG_RUNTIME_DIR                → $XDG_RUNTIME_DIR            read-write
$HOME                           → /home/$USER                 read-write  ← NEW
$HOME/.config                   → /home/$USER/.config         read-write  ← NEW
/usr/share/fonts                → /usr/share/fonts            read-only   ← NEW
/usr/share/themes               → /usr/share/themes           read-only   ← NEW
/usr/share/icons                → /usr/share/icons            read-only   ← NEW
/usr/bin (optional)             → /host/bin                   read-only   ← NEW (dev mode)
```

---

## Implementation Phases

### Phase 1: Multi-App Foundation

**Goal**: `voidbox install discord` works

**Duration**: Core functionality

#### 1.1 Manifest System

```toml
# ~/.local/share/voidbox/manifests/discord.toml

[app]
name = "discord"
display_name = "Discord"
description = "Voice, video & text chat"
version = "0.0.71"
license = "Proprietary"

[source]
type = "github"                     # github | direct | custom
owner = "discord"
repo = "discord"
asset_pattern = "discord-{version}.tar.gz"
asset_os = "linux"
asset_arch = "x86_64"

# OR for direct downloads:
# type = "direct"
# url = "https://discord.com/api/download?platform=linux&format=tar.gz"
# version_url = "https://discord.com/api/updates/stable?platform=linux"

[runtime]
base = "ubuntu:24.04"               # ubuntu:24.04 | alpine:3.19
arch = ["x86_64", "aarch64"]

[dependencies]
# Packages to install in container
packages = [
    "libgtk-3-0",
    "libasound2",
    "libnotify4",
    "libnss3",
    "libxss1",
    "libxtst6",
    "xdg-utils",
]

[binary]
name = "Discord"                    # Binary to search for in archive
path = "Discord"                    # Path relative to extraction root
args = []                           # Default launch arguments

[desktop]
categories = ["Network", "InstantMessaging"]
wm_class = "discord"
icon = "discord.png"                # Icon file in archive
mime_types = ["x-scheme-handler/discord"]
keywords = ["voice", "chat", "gaming"]

[permissions]
# All default to TRUE (open by default)
# Users can override in settings/discord.toml
network = true
audio = true
microphone = true
gpu = true
camera = false                      # Opt-in for privacy
home = true
downloads = true
removable_media = false             # Opt-in
```

#### 1.2 CLI Commands

```bash
# App Management
voidbox install <app>               # Install from registry or local manifest
voidbox install ./custom.toml       # Install from local file
voidbox install https://...         # Install from URL
voidbox remove <app>                # Uninstall app
voidbox remove --purge <app>        # Remove with all data

# Running Apps
voidbox run <app>                   # Launch app
voidbox run <app> -- <args>         # Launch with arguments
voidbox run <app> --url <url>       # Launch with URL (browsers)

# Updates
voidbox update                      # Update all apps
voidbox update <app>                # Update specific app
voidbox self-update                 # Update voidbox itself

# Information
voidbox list                        # List installed apps
voidbox info <app>                  # Show app details
voidbox search <query>              # Search registry

# Settings
voidbox settings <app>              # Open GUI settings (Phase 3)
voidbox settings --cli <app>        # CLI settings editor

# Advanced
voidbox shell <app>                 # Open shell in container
voidbox export <app>                # Create portable bundle (Phase 5)
voidbox logs <app>                  # View app logs
```

#### 1.3 Project Structure

```
src/
├── main.rs                 # CLI entry point (clap)
├── lib.rs                  # Library exports
│
├── cli/                    # Command handlers
│   ├── mod.rs
│   ├── install.rs
│   ├── remove.rs
│   ├── run.rs
│   ├── update.rs
│   ├── list.rs
│   ├── search.rs
│   └── settings.rs
│
├── manifest/               # Manifest parsing
│   ├── mod.rs
│   ├── parser.rs           # TOML parsing
│   ├── schema.rs           # Manifest structs
│   └── validate.rs         # Validation logic
│
├── registry/               # Remote registry
│   ├── mod.rs
│   ├── client.rs           # HTTP client
│   ├── cache.rs            # Local cache
│   └── index.rs            # Index parsing
│
├── runtime/                # Container runtime
│   ├── mod.rs
│   ├── namespace.rs        # Linux namespace setup
│   ├── mount.rs            # Bind mounts
│   ├── overlay.rs          # OverlayFS (Phase 2)
│   └── exec.rs             # Process execution
│
├── storage/                # Local storage
│   ├── mod.rs
│   ├── paths.rs            # Directory structure
│   ├── database.rs         # SQLite state
│   └── download.rs         # File downloads
│
├── desktop/                # Desktop integration
│   ├── mod.rs
│   ├── entry.rs            # .desktop file generation
│   ├── icon.rs             # Icon extraction
│   └── symlink.rs          # PATH symlinks
│
└── settings/               # Permission management
    ├── mod.rs
    ├── defaults.rs         # Default permissions
    ├── override.rs         # User overrides
    └── gui.rs              # GTK/Qt GUI (Phase 3)
```

#### 1.4 Deliverables

- [x] Manifest TOML schema and parser
- [x] `voidbox install <app>` command
- [x] `voidbox remove <app>` command
- [x] `voidbox run <app>` command
- [x] `voidbox list` command
- [x] `voidbox update` command
- [x] Desktop file generation
- [x] Symlink creation for PATH
- [x] Home folder bind mount (default ON)
- [x] Theme/font bind mounts
- [x] Native GUI Installer (egui) - *Completed ahead of schedule*
- [x] Use absolute paths in .desktop files and wrappers (GitHub Issue #1)
- [x] Detect missing PATH and warn user with instructions (GitHub Issue #1)
- [ ] aarch64 (ARM64) support - cross-compile binary + test on ARM hardware

---

### Phase 2: Shared Base Layers

**Goal**: 10 apps use 1 base = ~1.8GB instead of ~4.5GB

**Duration**: After Phase 1 stable

#### 2.1 OverlayFS Structure

```
Base Layer (read-only, shared)
┌─────────────────────────────────────────┐
│  bases/ubuntu-24.04/                    │
│  ├── bin/                               │
│  ├── etc/                               │
│  ├── lib/                               │
│  ├── usr/                               │
│  └── ...                                │
└─────────────────────────────────────────┘
              │
              │  OverlayFS mount
              ▼
┌─────────────────────────────────────────┐
│  App Layer (read-write, per-app)        │
│  apps/discord/layer/                    │
│  └── opt/discord/                       │
│      ├── Discord                        │
│      ├── resources/                     │
│      └── ...                            │
└─────────────────────────────────────────┘
              │
              │  Merged view
              ▼
┌─────────────────────────────────────────┐
│  apps/discord/merged/                   │
│  ├── bin/           (from base)         │
│  ├── etc/           (from base)         │
│  ├── opt/discord/   (from app layer)    │
│  └── usr/           (from base)         │
└─────────────────────────────────────────┘
```

#### 2.2 Base Images

| Base | Size (compressed) | Size (extracted) | Use Case |
|------|-------------------|------------------|----------|
| `ubuntu:24.04` | ~30MB | ~300MB | Full compatibility, most apps |
| `alpine:3.19` | ~3MB | ~8MB | Minimal, static binaries |
| `fedora:40` | ~50MB | ~400MB | RPM-based apps (future) |

#### 2.3 System D-Bus Integration (VPN Support)

VPN apps require privileged kernel operations (TUN/TAP, routing) that cannot run inside unprivileged user namespaces. Like Flatpak, we support VPN apps by delegating to host system services via D-Bus.

**How it works:**
```
┌─────────────────────────────────────┐
│  VPN App (sandboxed, unprivileged)  │
│  - GUI for server selection         │
│  - Downloads VPN configs            │
└────────────┬────────────────────────┘
             │ D-Bus (system bus)
             │ org.freedesktop.NetworkManager
             ▼
┌─────────────────────────────────────┐
│  Host NetworkManager (runs as root) │
│  - Creates TUN/TAP devices          │
│  - Manages routes and DNS           │
│  - Has CAP_NET_ADMIN                │
└─────────────────────────────────────┘
```

**Implementation**:
- Add `system_dbus` permission to manifest schema
- Bind mount `/run/dbus/system_bus_socket` when enabled
- Apps can talk to NetworkManager, systemd, polkit, etc.

```toml
# Example VPN app manifest
[permissions]
system_dbus = true  # Access host D-Bus system bus
```

**Host requirements**:
- NetworkManager + VPN plugins (openvpn, wireguard)
- Polkit policies allowing user to manage VPN

#### 2.4 Deliverables

- [x] OverlayFS mounting in user namespace
- [x] Base image download and extraction
- [x] Base image sharing between apps
- [x] Lazy base download (on first app install)
- [ ] Base image updates
- [ ] `system_dbus` permission for VPN/system service apps

---

### Phase 3: Settings GUI & Native Integration

**Goal**: Visual permission management + apps feel like native apps

**Duration**: After Phase 2 stable

#### 3.1 GUI Design

```
┌─────────────────────────────────────────────────────────────────┐
│  Voidbox Settings                                        [─][□][×]│
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌──────────────┐  ┌──────────────────────────────────────────┐ │
│  │ Installed    │  │                                          │ │
│  │              │  │  Discord                                 │ │
│  │ ● Brave      │  │  ──────────────────────────────────────  │ │
│  │ ● Discord  ◄─┼──│                                          │ │
│  │ ○ Firefox    │  │  Hardware Access                         │ │
│  │ ○ VSCode     │  │  ┌────────────────────────────────────┐  │ │
│  │              │  │  │ [✓] GPU Acceleration               │  │ │
│  │              │  │  │ [✓] Audio Output                   │  │ │
│  │              │  │  │ [✓] Microphone                     │  │ │
│  │              │  │  │ [ ] Camera                         │  │ │
│  │              │  │  │ [✓] Game Controllers               │  │ │
│  └──────────────┘  │  └────────────────────────────────────┘  │ │
│                    │                                          │ │
│                    │  File Access                             │ │
│                    │  ┌────────────────────────────────────┐  │ │
│                    │  │ [✓] Home Folder                    │  │ │
│                    │  │ [✓] Downloads                      │  │ │
│                    │  │ [✓] Documents                      │  │ │
│                    │  │ [ ] External Drives                │  │ │
│                    │  └────────────────────────────────────┘  │ │
│                    │                                          │ │
│                    │  Network                                 │ │
│                    │  ┌────────────────────────────────────┐  │ │
│                    │  │ [✓] Internet Access                │  │ │
│                    │  │ [✓] Local Network                  │  │ │
│                    │  └────────────────────────────────────┘  │ │
│                    │                                          │ │
│                    │  ┌────────────────┐ ┌────────────────┐   │ │
│                    │  │ Reset Defaults │ │     Apply      │   │ │
│                    │  └────────────────┘ └────────────────┘   │ │
│                    │                                          │ │
│                    └──────────────────────────────────────────┘ │
│                                                                  │
└──────────────────────────────────────────────────────────────────┘
```

#### 3.2 Technology Options

| Option | Pros | Cons |
|--------|------|------|
| GTK4 (gtk4-rs) | Native GNOME look, good Rust bindings | Large dependency |
| Qt (qmetaobject-rs) | Native KDE look | Complex bindings |
| egui | Pure Rust, tiny, works everywhere | Non-native look |
| Tauri/WebView | HTML/CSS flexibility | Web overhead |

**Recommendation**: egui for v1 (simple, no dependencies), GTK4 later

#### 3.3 Native Desktop Integration

Make voidbox apps indistinguishable from native apps:

```bash
# "Open With" integration - register as handler for file types
xdg-mime default voidbox-gimp.desktop image/png
xdg-mime default voidbox-brave.desktop text/html

# Default browser/app registration
xdg-settings set default-web-browser voidbox-brave.desktop

# URL scheme handlers (discord://, vscode://, etc.)
xdg-mime default voidbox-discord.desktop x-scheme-handler/discord
```

**Implementation**:
- Generate proper MimeType entries in .desktop files
- Add `voidbox set-default <app>` command for browsers/mail/etc.
- Support `x-scheme-handler/*` for URL protocols
- Integrate with xdg-utils for system-wide registration

#### 3.4 Deliverables

- [ ] Settings GUI application
- [ ] Per-app permission toggles
- [ ] Reset to defaults button
- [ ] Launch from `voidbox settings <app>`
- [ ] System tray integration (optional)
- [ ] "Open With" support (register as handler for mime-types)
- [ ] Default browser/app registration (xdg-settings integration)
- [ ] URL scheme handlers (discord://, vscode://, etc.)
- [ ] `voidbox set-default <app>` command

---

### Phase 4: Registry

**Goal**: `voidbox search browser` returns community apps

**Duration**: After Phase 3 stable

#### 4.1 Registry Structure

```
https://voidbox.dev/
├── registry.json               # Main index
├── manifests/
│   ├── brave.toml
│   ├── discord.toml
│   ├── firefox.toml
│   ├── vscode.toml
│   └── ...
├── icons/
│   ├── brave.png
│   ├── discord.png
│   └── ...
└── bases/
    ├── ubuntu-24.04.tar.zst
    └── alpine-3.19.tar.zst
```

#### 4.2 Registry Index Format

```json
{
  "version": 1,
  "updated": "2025-01-18T00:00:00Z",
  "bases": {
    "ubuntu:24.04": {
      "url": "https://voidbox.dev/bases/ubuntu-24.04.tar.zst",
      "sha256": "abc123...",
      "size": 31457280
    },
    "alpine:3.19": {
      "url": "https://voidbox.dev/bases/alpine-3.19.tar.zst",
      "sha256": "def456...",
      "size": 3145728
    }
  },
  "apps": {
    "brave": {
      "display_name": "Brave Browser",
      "description": "Privacy-focused browser with ad blocking",
      "categories": ["Network", "WebBrowser"],
      "manifest": "https://voidbox.dev/manifests/brave.toml",
      "icon": "https://voidbox.dev/icons/brave.png",
      "version": "1.73.97",
      "downloads": 15420,
      "verified": true,
      "maintainer": "community"
    },
    "discord": {
      "display_name": "Discord",
      "description": "Voice, video & text chat",
      "categories": ["Network", "InstantMessaging"],
      "manifest": "https://voidbox.dev/manifests/discord.toml",
      "icon": "https://voidbox.dev/icons/discord.png",
      "version": "0.0.71",
      "downloads": 28350,
      "verified": true,
      "maintainer": "community"
    }
  }
}
```

#### 4.3 Decentralization

```bash
# Users can add multiple registries
voidbox registry add https://mycompany.com/voidbox/
voidbox registry add https://gaming-apps.org/voidbox/

# List configured registries
voidbox registry list
# 1. https://voidbox.dev/ (default)
# 2. https://mycompany.com/voidbox/
# 3. https://gaming-apps.org/voidbox/

# Remove a registry
voidbox registry remove mycompany

# Search across all registries
voidbox search browser
# [voidbox.dev] brave - Privacy-focused browser
# [voidbox.dev] firefox - Mozilla Firefox
# [gaming-apps.org] chromium - Gaming-optimized Chromium
```

#### 4.4 Deliverables

- [ ] Registry client (HTTP + JSON parsing)
- [ ] Local registry cache
- [ ] `voidbox search` command
- [ ] Multiple registry support
- [ ] Registry signature verification (optional)
- [ ] Static site generator for hosting registry

---

### Phase 5: Advanced Features

**Goal**: Polish and power-user features

**Duration**: Ongoing after Phase 4

#### 5.1 Delta Updates

```
Current: Download full app on every update (~150MB)
Goal: Download only changed files (~5-10MB)

Implementation:
1. Generate file manifest with hashes on build
2. Compare local vs remote manifest
3. Download only changed files
4. Apply via rsync-like algorithm or bsdiff
```

#### 5.2 Offline Bundles

```bash
# Export app as portable single file
voidbox export discord
# Creates: discord-0.0.71-x86_64.voidbox (~250MB)

# Import on another machine (no internet needed)
voidbox import discord-0.0.71-x86_64.voidbox
```

Bundle format:
```
discord-0.0.71-x86_64.voidbox
├── manifest.toml
├── base.tar.zst          # Or reference to shared base
├── app.tar.zst
└── signature             # Optional GPG signature
```

#### 5.3 Reproducible Builds

```bash
# Verify app matches published manifest
voidbox verify discord

# Output:
# ✓ Manifest signature valid
# ✓ Base image hash matches: abc123...
# ✓ App layer hash matches: def456...
# ✓ All files verified
```

#### 5.4 Developer Mode

```bash
# Mount host /usr/bin for access to system tools
voidbox run --dev-mode vscode

# Inside container:
# - /host/bin/python -> host Python
# - /host/bin/node -> host Node.js
# - /host/bin/cargo -> host Cargo
```

#### 5.5 Profiles

```bash
# Create restricted profile
voidbox profile create paranoid --no-network --no-camera --no-mic

# Apply to app
voidbox settings discord --profile paranoid

# Or per-launch
voidbox run --profile paranoid discord
```

#### 5.6 Deliverables

- [ ] Delta update system
- [ ] `voidbox export` command
- [ ] `voidbox import` command
- [ ] Build verification
- [ ] Developer mode bind mounts
- [ ] Profile system

---

## Technical Specifications

### System Requirements

| Requirement | Minimum | Recommended |
|-------------|---------|-------------|
| Kernel | 3.8+ (user namespaces) | 5.0+ |
| Architecture | x86_64 | x86_64, aarch64 |
| Storage | 500MB (one app) | 2GB+ |
| RAM | 512MB | 2GB+ |
| Network | Required for install | Not required for run |

### Dependencies

**Runtime dependencies**: None (statically linked)

**Build dependencies**:
- Rust 1.85+ (2024 edition)
- OpenSSL (for HTTPS)

### Kernel Features Used

| Feature | Kernel Version | Purpose |
|---------|---------------|---------|
| User namespaces | 3.8+ | Unprivileged containers |
| Mount namespaces | 2.6.24+ | Filesystem isolation |
| PID namespaces | 2.6.24+ | Process isolation |
| UTS namespaces | 2.6.19+ | Hostname isolation |
| IPC namespaces | 2.6.19+ | IPC isolation |
| OverlayFS | 3.18+ | Layered filesystem (Phase 2) |

### File Formats

| Format | Purpose |
|--------|---------|
| TOML | Manifests, settings, configuration |
| JSON | Registry index, API responses |
| SQLite | Local state database |
| tar.zst | Base images, app archives |
| PNG | App icons |

### Network Endpoints

| Endpoint | Purpose |
|----------|---------|
| `https://voidbox.dev/registry.json` | App registry |
| `https://voidbox.dev/manifests/*.toml` | App manifests |
| `https://voidbox.dev/bases/*.tar.zst` | Base images |
| `https://api.github.com/repos/*/releases` | App version checking |

---

## Design Decisions

### Why User Namespaces (not Docker/Podman)?

| Aspect | User Namespaces | Docker/Podman |
|--------|----------------|---------------|
| Root required | No | Yes (or rootless mode complexity) |
| Daemon required | No | Yes |
| Startup overhead | ~10ms | ~100-500ms |
| Complexity | Low | High |
| Portability | Works on stock kernel | Needs setup |

### Why OverlayFS (not OSTree)?

| Aspect | OverlayFS | OSTree |
|--------|-----------|--------|
| Complexity | Simple, kernel-native | Complex, userspace |
| Dependencies | None | libostree |
| Learning curve | Low | High |
| Deduplication | Per-mount | Per-file (better) |
| Our scale | Good enough | Overkill |

**Decision**: OverlayFS for simplicity. OSTree is better for large-scale deployments but adds complexity we don't need.

### Why TOML (not YAML/JSON)?

| Aspect | TOML | YAML | JSON |
|--------|------|------|------|
| Human readable | Excellent | Good | Poor |
| Comments | Yes | Yes | No |
| Complexity | Low | High (gotchas) | Low |
| Rust support | Excellent (serde) | Good | Excellent |

### Why Open by Default (not Restrictive)?

| Philosophy | Flatpak/Snap | Voidbox |
|------------|--------------|---------|
| Assumption | User is a threat | User knows what they want |
| Default | Deny all | Allow all |
| Result | Apps broken, need Flatseal | Apps work, can restrict |

**Rationale**: Our goal is portability, not security theater. Users who want restrictions can opt-in.

### Why No Network Namespace by Default?

| Consideration | Decision |
|---------------|----------|
| Breaks many apps | Network required for most apps |
| DNS complexity | Must set up virtual network |
| Little benefit | Most apps need internet anyway |

**Decision**: Network namespace OFF by default. Opt-in for paranoid users via settings.

---

## Success Metrics

### Phase 1 Success

- [x] 5+ apps packaged and working
- [x] Install/run/update cycle works reliably
- [x] Home folder access works by default
- [x] Host pip/npm visible in dev containers (via native_mode)
- [x] Zero configuration needed for basic use

### Phase 2 Success

- [ ] 10 apps share 1 base layer
- [ ] Disk usage 60% lower than current
- [ ] No performance regression

### Phase 3 Success

- [ ] GUI settings app works on GNOME/KDE/others
- [ ] Non-technical users can manage permissions
- [ ] No terminal needed for basic usage
- [ ] Apps can be set as default handlers (browser, etc.)
- [ ] "Open With" works for registered mime-types

### Phase 4 Success

- [ ] Registry live at voidbox.dev
- [ ] 20+ community-contributed manifests
- [ ] Search returns relevant results

### Long-term Success

- [ ] 100+ apps in registry
- [ ] Mentioned in distro documentation
- [ ] Community actively contributing manifests
- [ ] Not "just another dev trying to solve the same issue"

---

## FAQ

### Is this trying to replace Flatpak/Snap?

No. We're targeting users who find Flatpak too restrictive and Snap too invasive. We're not trying to win the "most secure" contest - we're trying to make apps just work.

### Why should developers package for this?

They shouldn't have to. Our manifests point to existing release assets (GitHub releases, official downloads). Developers don't need to do anything.

### How is this different from just running apps in a chroot?

- User namespaces: no root required
- Proper isolation: PID, IPC, UTS namespaces
- Desktop integration: .desktop files, icons
- Update mechanism: automatic updates
- Shared bases: disk efficiency

### What about ARM64?

Planned for Phase 1. The architecture supports it; we just need to:
- Build ARM64 binaries (cross-compile with `cargo build --target aarch64-unknown-linux-gnu`)
- Provide ARM64 base images
- Test on real hardware (Raspberry Pi, Apple Silicon via Linux VM, cloud ARM instances)

### What about Wayland vs X11?

Both work. We inherit the host's display server via `$XDG_RUNTIME_DIR` and `$DISPLAY`/`$WAYLAND_DISPLAY`.

### What about audio?

PulseAudio and PipeWire both work via `$XDG_RUNTIME_DIR/pulse/native` or PipeWire socket.

### What about VPN apps?

VPN apps (NordVPN, ProtonVPN, Mullvad, etc.) require privileged kernel operations (TUN/TAP devices, routing tables) that cannot run inside unprivileged containers. However, like Flatpak, we support VPN apps by delegating to the host's NetworkManager via D-Bus.

**How it works:**
- VPN app runs sandboxed (GUI, config download, server selection)
- App talks to host NetworkManager via D-Bus system bus
- NetworkManager (running as root on host) creates TUN devices and manages routing
- Requires `system_dbus = true` permission in manifest

**Requirements:**
- Host must have NetworkManager + VPN plugins installed
- User must be authorized via polkit to manage VPN connections

**Alternative:** If you just want VPN protection for Voidbox apps, run the VPN on your host system. Since Voidbox shares the host network by default, all containerized apps automatically use the VPN tunnel.

---

## Contributing

### Manifest Contributions

1. Create a TOML manifest following the schema
2. Test with `voidbox install ./myapp.toml`
3. Submit PR to voidbox-manifests repository

### Code Contributions

1. Fork the repository
2. Create a feature branch
3. Submit PR with tests

### Registry Hosting

Anyone can host a registry. Just serve:
- `registry.json` - index file
- `manifests/*.toml` - app manifests
- `bases/*.tar.zst` - base images (optional, can reference main registry)

---

## License

MIT

---

## Changelog

### v0.7.1
- **Feature:** Shared dependency layers via `dependencies.shared`
- **Feature:** Desktop file associations via `MimeType` (Open With)
- **Feature:** Direct source update checks via `version_url`
- **Fix:** Bundle installs record resolved versions
- **Fix:** Purge uninstall handles permissioned overlay workdirs
- **Docs:** GUI installer notes internet requirement for installs

### v0.7.0
- **Feature:** Shared base images via OverlayFS with per-app layers
- **Feature:** Self-extracting `.voidbox` installers (CLI + GUI)
- **Fix:** Desktop launchers and wrappers use absolute voidbox path
- **Fix:** Robust binary resolution for overlay installs

### v0.6.3
- **Security:** Implemented shared-secret authentication for host bridge
- **Feature:** Host bridge support for interactive `sudo` and host commands
- **Fix:** Preserved system users in container `/etc/passwd` (fixes apt/dbus)
- **Fix:** Hardened shim scripts against zombie processes

### v0.6.0
- Added `native_mode` permission for seamless host integration (access host /usr, /lib, tools)
- Fixed binary execution path resolution for native applications
- Improved DNS resolution in container environments
- Updated manifest schema to support archive types and explicit binary paths

### v0.5.0
- Refactored for easy forking via `src/app.rs`
- Support for multiple archive types (Zip, TarGz, TarXz)
- Updated README with forking instructions

### v0.4.1
- Fixed self-update asset naming

### v0.4.0
- Added uninstall command
- Updated README

### v0.3.0
- Added self-update feature
- Added auto-installation on first run

### v0.2.2
- Fixed update failing with "File exists" on broken symlinks

---

*Last updated: 2025-01-18*
