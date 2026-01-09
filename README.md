# Brave Box (Void Runner)

A single-binary, portable, and isolated Brave Browser environment for Linux.

## Features

*   **Single Binary**: No external dependencies (no Docker, Podman, or Flatpak required).
*   **Portable**: Works on Fedora, Ubuntu, Debian, Arch, and more.
*   **Isolated**: Runs in a dedicated User/Mount/PID namespace.
*   **Hardware Accelerated**: Full GPU and Audio (Pipewire/PulseAudio) passthrough.
*   **Auto-Updating**: Automatically checks and updates Brave on launch.
*   **Self-Healing**: Automatically rebuilds the container if corrupted.

## Usage

Download the binary `brave_box` and run it:

```bash
chmod +x brave_box
./brave_box
```

Or just double-click it in your file manager.

## Building from Source

Requirements: Rust (cargo).

```bash
cargo build --release
cp target/release/void_runner brave_box
```

## How it Works

The binary contains a bootstrap logic that:
1.  Downloads a minimal Ubuntu 22.04 RootFS.
2.  Sets up a user namespace (mapping your user to root).
3.  Installs Brave and dependencies inside the container.
4.  Bind-mounts host hardware interfaces (/dev/dri, /dev/snd, Wayland sockets).
5.  Launches the browser.
