# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

VoidBox is a Universal Linux App Platform written in Rust that runs applications in unprivileged containers using Linux user namespaces (not Docker). Single binary (~5MB), requires no root, ~10ms startup time. Current version: v0.6.0, requires Rust 1.85+ (2024 edition).

## Build Commands

```bash
cargo build --release          # Optimized release build (stripped, LTO)
cargo clippy -- -W clippy::all # Lint before committing
cargo fmt                      # Format code
cargo test                     # Run tests
RUST_BACKTRACE=1 cargo run -- run <app>  # Debug container execution
```

## Architecture

```
src/
├── main.rs          # Entry point: CLI mode, launcher mode (argv[0]), or GUI installer
├── cli/             # Command handlers (install, run, remove, update, list, shell)
│   ├── install.rs   # Downloads base image, extracts app, creates .desktop
│   ├── run.rs       # Loads manifest, sets up namespaces, executes binary
│   └── launcher.rs  # Detects void_<app> invocation via argv[0]
├── manifest/        # TOML manifest parsing
│   └── schema.rs    # AppManifest, SourceConfig, PermissionConfig structs
├── runtime/         # Container execution engine
│   ├── namespace.rs # Linux namespace setup (user, mount, PID, UTS, IPC)
│   ├── mount.rs     # Bind mounts, pivot_root, environment variables
│   └── exec.rs      # Process execution inside container
├── storage/         # ~/.local/share/voidbox/ management
├── desktop/         # .desktop file and icon handling
├── settings/        # User permission overrides
└── gui/             # egui-based installer
```

## Key Execution Flow

**Install**: Parse manifest → Download Ubuntu base rootfs → Extract app → Generate .desktop → Create PATH symlink

**Run**: Load manifest + user overrides → setup_user_namespace() → setup_container_namespaces() → spawn_container_init() → pivot_root to app rootfs → bind mount devices/home/XDG → exec binary

The container init process (internal-init) runs after fork in new namespaces. VSCode/apps become PID 1 in their container.

## Data Directories

```
~/.local/share/voidbox/
├── apps/<name>/rootfs/   # Isolated app filesystems
├── manifests/            # Saved TOML manifests
├── settings/             # User permission overrides
└── icons/, desktop/      # System integration
```

## Manifest Format

Apps define: `[app]` metadata, `[source]` download URL, `[runtime]` base image, `[dependencies]` apt packages, `[binary]` executable/args, `[permissions]` access controls.

Permissions default to ALLOW (network, audio, gpu, home, fonts, themes = true). Key options: `native_mode` mounts host /usr /lib /etc for tool access; `dev_mode` exposes host binaries.

## Key Design Decisions

- User namespaces over Docker: no root, no daemon, faster startup
- TOML manifests over YAML: better comments, excellent serde support
- Open by default: apps work out of box, users can restrict later
- No network namespace: most apps need internet anyway
- Native GPU/audio passthrough via bind mounts to /dev/dri, XDG_RUNTIME_DIR

## Adding Features

- **New CLI command**: Create `src/cli/newcmd.rs`, export in `mod.rs`, add to `Commands` enum in main.rs
- **New manifest field**: Update `src/manifest/schema.rs` with serde attributes
- **New permission**: Add to `PermissionConfig`, handle in `runtime/mount.rs`

## Error Handling

Each module has custom error types (InstallError, RunError, etc.) with thiserror. Use `?` operator for propagation.
