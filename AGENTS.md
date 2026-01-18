# AGENTS.md - Voidbox

Guidelines for AI coding agents working on this Rust codebase.

## Project Overview

Voidbox is a universal Linux app platform that provides portable, isolated application
environments using Linux user namespaces. It downloads Ubuntu base images and target
applications from manifests, then runs them in unprivileged containers with GPU/audio
passthrough. No root, no daemon, no Docker required.

## Build Commands

```bash
# Debug build
cargo build

# Release build (optimized, stripped, with LTO)
cargo build --release

# Check compilation without building
cargo check

# Run debug binary
cargo run

# Run release binary
./target/release/voidbox
```

## Lint Commands

```bash
# Run clippy linter with all warnings
cargo clippy -- -W clippy::all

# Run clippy and deny warnings (CI-style)
cargo clippy -- -D warnings

# Format check (does not modify files)
cargo fmt -- --check

# Auto-format code
cargo fmt
```

## Test Commands

```bash
# Run all tests
cargo test

# Run a single test by name
cargo test <test_name>

# Run tests with output
cargo test -- --nocapture

# Run tests in a specific module
cargo test <module_name>::
```

## Project Structure

```
src/
├── main.rs              # CLI entry point
├── lib.rs               # Library exports and constants
├── cli/                 # Command handlers
│   ├── mod.rs
│   ├── install.rs       # voidbox install
│   ├── remove.rs        # voidbox remove
│   ├── run.rs           # voidbox run
│   ├── list.rs          # voidbox list
│   ├── update.rs        # voidbox update / self-update
│   ├── shell.rs         # voidbox shell
│   └── info.rs          # voidbox info
├── manifest/            # TOML manifest parsing
│   ├── mod.rs
│   ├── schema.rs        # Manifest struct definitions
│   ├── parser.rs        # TOML parsing
│   └── validate.rs      # Validation logic
├── runtime/             # Container runtime
│   ├── mod.rs
│   ├── namespace.rs     # Linux namespace setup
│   ├── mount.rs         # Bind mounts and pivot_root
│   └── exec.rs          # Process execution
├── storage/             # Local storage management
│   ├── mod.rs
│   ├── paths.rs         # Directory paths
│   └── download.rs      # HTTP downloads
├── desktop/             # Desktop integration
│   ├── mod.rs
│   ├── entry.rs         # .desktop file generation
│   ├── icon.rs          # Icon extraction
│   └── symlink.rs       # PATH symlinks
└── settings/            # Permission management
    ├── mod.rs
    ├── defaults.rs      # Default permissions
    └── overrides.rs     # User overrides

examples/manifests/      # Example app manifests
```

## Code Style Guidelines

### Rust Edition
- Uses **Rust 2024 edition** (requires Rust 1.85+)

### Imports
- Group: `use crate::`, external crates, `std`
- Use explicit imports, avoid glob imports except for `pub use module::*` in mod.rs

### Error Handling
- Use `thiserror` for custom error types in each module
- Use `?` operator for propagation
- Provide context with `map_err()` when needed

### Naming Conventions
- **Constants**: SCREAMING_SNAKE_CASE
- **Functions**: snake_case
- **Types/Structs/Enums**: PascalCase
- **Variables**: snake_case

### Module Organization
- Each module has its own error type (e.g., `InstallError`, `RunError`)
- Use `mod.rs` to re-export public items with `pub use`
- Keep related functionality in the same module directory

### Clap CLI
- Use derive macros for CLI definition in `main.rs`
- Add doc comments (`///`) for command descriptions
- Use `#[command(hide = true)]` for internal commands

### File System Operations
- Use `PathBuf` for owned paths, `&Path` for borrowed
- All path functions are in `storage::paths`
- Use `paths::ensure_dirs()` to create required directories

### Unsafe Code
- Minimize `unsafe`; only for FFI/libc calls
- Use `unsafe { std::env::set_var(...) }` for env vars in 2024 edition

## Key Dependencies

- `clap` - CLI argument parsing (derive feature)
- `nix` - Linux system calls (namespaces, mount, pivot_root)
- `serde`/`toml` - TOML manifest parsing
- `ureq` - HTTP client
- `flate2`/`tar`/`zip` - Archive handling
- `thiserror` - Custom error types
- `self_update` - GitHub release auto-updates

## Common Tasks

### Adding a new CLI command
1. Create handler in `src/cli/` (e.g., `newcmd.rs`)
2. Add to `src/cli/mod.rs` exports
3. Add variant to `Commands` enum in `main.rs`
4. Add match arm in `main()`

### Adding a new manifest field
1. Update struct in `src/manifest/schema.rs`
2. Add serde attributes (`#[serde(default)]` etc.)
3. Use in relevant CLI handlers

### Testing container functionality
```bash
RUST_BACKTRACE=1 cargo run -- run <app>
```

### Creating an app manifest
See `examples/manifests/` for templates. Key sections:
- `[app]` - Name and metadata
- `[source]` - Download configuration
- `[dependencies]` - Ubuntu packages
- `[binary]` - Executable configuration
- `[permissions]` - Access controls
