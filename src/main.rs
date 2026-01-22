//! Voidbox - Universal Linux App Platform
//!
//! A portable, isolated application environment using Linux user namespaces.
//!
//! This binary supports two modes:
//! 1. CLI mode: `voidbox install`, `voidbox run`, etc.
//! 2. Launcher mode: when invoked as `void_brave`, `void_discord`, etc.
//!    (uses argv[0] detection, similar to busybox)

use clap::{Parser, Subcommand};
use std::path::PathBuf;

use voidbox::bundle;
use voidbox::cli;
use voidbox::desktop::install_self;
use voidbox::gui;
use voidbox::manifest::PermissionConfig;
use voidbox::runtime::{
    init_and_exec, setup_container_namespaces, setup_user_namespace, spawn_container_init,
};
use voidbox::storage::paths;

#[derive(Parser)]
#[command(name = "voidbox")]
#[command(version = voidbox::VERSION)]
#[command(about = "Universal Linux App Platform - portable, isolated application environments")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Install an app from a manifest
    Install {
        /// Manifest source (file path, URL, or app name from registry)
        source: String,

        /// Force reinstall even if already installed
        #[arg(long, short)]
        force: bool,
    },

    /// Remove an installed app
    Remove {
        /// App name to remove
        app: String,

        /// Also remove all app data
        #[arg(long)]
        purge: bool,
    },

    /// Run an installed app
    Run {
        /// App name to run
        app: String,

        /// URL to open (for browsers)
        #[arg(long)]
        url: Option<String>,

        /// Enable developer mode (mount host tools)
        #[arg(long)]
        dev: bool,

        /// Additional arguments to pass to the app
        #[arg(last = true)]
        args: Vec<String>,
    },

    /// List installed apps
    List,

    /// Update apps
    Update {
        /// App name to update (updates all if not specified)
        app: Option<String>,

        /// Force update even if already on latest
        #[arg(long, short)]
        force: bool,
    },

    /// Update voidbox itself
    SelfUpdate {
        /// Force update even if already on latest
        #[arg(long, short)]
        force: bool,
    },

    /// Open a shell in an app's container
    Shell {
        /// App name
        app: String,

        /// Enable developer mode (mount host tools)
        #[arg(long)]
        dev: bool,
    },

    /// Show information about voidbox or a specific app
    Info {
        /// App name (shows voidbox info if not specified)
        app: Option<String>,
    },

    /// Uninstall voidbox completely
    Uninstall {
        /// Also remove all app data
        #[arg(long)]
        purge: bool,
    },

    /// Bundle commands (.voidbox installers)
    Bundle {
        #[command(subcommand)]
        command: BundleCommands,
    },

    /// Internal initialization command (do not use manually)
    #[command(hide = true)]
    InternalInit {
        rootfs: PathBuf,
        cmd: String,
        /// Serialized permissions JSON
        #[arg(long)]
        permissions: Option<String>,
        #[arg(last = true)]
        args: Vec<String>,
    },

    /// Internal run command for setup scripts (do not use manually)
    #[command(hide = true)]
    InternalRun {
        rootfs: PathBuf,
        cmd: String,
        #[arg(last = true)]
        args: Vec<String>,
    },
}

#[derive(Subcommand)]
enum BundleCommands {
    /// Create a self-extracting .voidbox installer
    Create {
        /// Manifest TOML file
        manifest: PathBuf,

        /// App archive file (zip/tar.gz)
        archive: PathBuf,

        /// Output .voidbox file
        #[arg(long, short)]
        output: Option<PathBuf>,
    },

    /// Install from a .voidbox file
    Install {
        /// Bundle file path
        bundle: PathBuf,

        /// Run the app after install
        #[arg(long)]
        run: bool,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Check if we're running as a launcher (void_brave, void_discord, etc.)
    // This uses argv[0] detection similar to busybox
    if let Some(app_name) = cli::should_run_as_launcher() {
        return run_as_launcher(&app_name);
    }

    // Check for embedded bundle or GUI installer
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 1 {
        let current_exe = std::env::current_exe().unwrap_or_default();
        let is_installed_exe = current_exe == paths::install_path();

        if !is_installed_exe {
            if let Some(info) = bundle::embedded_manifest_info()? {
                if gui::is_gui_mode() {
                    return gui_bundle_install_mode(info);
                }
                return cli_bundle_install_mode(info);
            }
        }

        // Check if we're being double-clicked (no args, not a TTY)
        if gui::is_gui_mode() {
            return gui_install_mode();
        }
    }

    let cli = Cli::parse();

    // Ensure data directories exist
    paths::ensure_dirs()?;

    let command = cli.command.unwrap_or(Commands::List);

    // Self-install on first run (skip for internal commands)
    if !matches!(
        command,
        Commands::InternalInit { .. } | Commands::InternalRun { .. }
    ) {
        if !voidbox::desktop::is_installed() {
            if let Err(e) = install_self() {
                eprintln!("[voidbox] Warning: Self-installation failed: {}", e);
            } else if !paths::is_bin_dir_in_path() {
                eprintln!(
                    "[voidbox] Warning: {} is not in your PATH.",
                    paths::bin_dir().display()
                );
                eprintln!(
                    "[voidbox] Add this to your shell config: export PATH=\"$HOME/.local/bin:$PATH\""
                );
            }
        }
    }

    match command {
        Commands::Install { source, force } => {
            cli::install_app(&source, force)?;
        }

        Commands::Remove { app, purge } => {
            cli::remove_app(&app, purge)?;
        }

        Commands::Run {
            app,
            url,
            dev,
            args,
        } => {
            cli::run_app(&app, &args, url.as_deref(), dev)?;
        }

        Commands::List => {
            cli::list_apps()?;
        }

        Commands::Update { app, force } => match app {
            Some(app_name) => cli::update_app(&app_name, force)?,
            None => cli::update_all(force)?,
        },

        Commands::SelfUpdate { force } => {
            cli::self_update(force)?;
        }

        Commands::Shell { app, dev } => {
            cli::shell(&app, dev)?;
        }

        Commands::Info { app } => match app {
            Some(app_name) => cli::show_app_info(&app_name)?,
            None => cli::show_voidbox_info()?,
        },

        Commands::Uninstall { purge } => {
            uninstall_voidbox(purge)?;
        }

        Commands::Bundle { command } => match command {
            BundleCommands::Create {
                manifest,
                archive,
                output,
            } => {
                cli::bundle_create(&manifest, &archive, output.as_deref())?;
            }
            BundleCommands::Install { bundle, run } => {
                cli::bundle_install(&bundle, run)?;
            }
        },

        Commands::InternalInit {
            rootfs,
            cmd,
            permissions,
            args,
        } => {
            // This runs inside the new namespace after fork
            // Parse permissions from JSON or use defaults
            let perms = match permissions {
                Some(json) => serde_json::from_str(&json).unwrap_or_default(),
                None => PermissionConfig::default(),
            };
            init_and_exec(&rootfs, &cmd, &args, &perms)?;
        }

        Commands::InternalRun { rootfs, cmd, args } => {
            // Setup namespaces and run command (for setup scripts)
            // Use minimal permissions - disable fonts/themes mounts so packages can install there
            let permissions = PermissionConfig {
                network: true,
                audio: false,
                microphone: false,
                gpu: false,
                camera: false,
                home: false, // Don't mount home during install
                downloads: false,
                removable_media: false,
                dev_mode: false,
                fonts: false,  // Don't mount fonts - let packages install
                themes: false, // Don't mount themes/icons - let packages install
                native_mode: false,
            };
            setup_user_namespace(permissions.native_mode)?;
            setup_container_namespaces()?;

            let self_exe = std::env::current_exe()?;
            let status = spawn_container_init(&self_exe, &rootfs, &cmd, &args, &permissions)?;

            if !status.success() {
                std::process::exit(status.code().unwrap_or(1));
            }
        }
    }

    Ok(())
}

fn uninstall_voidbox(purge: bool) -> Result<(), Box<dyn std::error::Error>> {
    use voidbox::manifest::InstalledApp;

    if purge {
        println!("[voidbox] This will remove voidbox and ALL app data.");
    } else {
        println!("[voidbox] This will remove voidbox but keep app data.");
    }
    print!("Continue? [y/N] ");
    std::io::Write::flush(&mut std::io::stdout())?;

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;

    if input.trim().to_lowercase() != "y" {
        println!("[voidbox] Uninstall cancelled.");
        return Ok(());
    }

    println!("[voidbox] Uninstalling...");

    // Remove install binary
    let install_path = paths::install_path();
    if install_path.exists() {
        std::fs::remove_file(&install_path)?;
        println!("  Removed {}", install_path.display());
    }

    // Remove app wrappers and launcher symlinks
    let mut installed_apps: Vec<String> = Vec::new();
    let db_path = paths::database_path();
    if db_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&db_path) {
            if let Ok(apps) = serde_json::from_str::<Vec<InstalledApp>>(&content) {
                installed_apps = apps.into_iter().map(|a| a.name).collect();
            }
        }
    }

    let bin_dir = paths::bin_dir();
    for app_name in installed_apps {
        let wrapper_path = bin_dir.join(&app_name);
        if wrapper_path.exists() {
            std::fs::remove_file(&wrapper_path)?;
            println!("  Removed {}", wrapper_path.display());
        }
        let launcher_path = bin_dir.join(format!("void_{}", app_name));
        if launcher_path.exists() {
            std::fs::remove_file(&launcher_path)?;
            println!("  Removed {}", launcher_path.display());
        }
    }

    if bin_dir.exists() {
        for entry in std::fs::read_dir(&bin_dir)? {
            if let Ok(entry) = entry {
                let path = entry.path();
                if let Ok(meta) = entry.file_type() {
                    if meta.is_symlink() {
                        if let Ok(target) = std::fs::read_link(&path) {
                            if target.file_name().is_some_and(|n| n == "voidbox") {
                                std::fs::remove_file(&path)?;
                                println!("  Removed {}", path.display());
                            }
                        }
                    }
                }
            }
        }
    }

    // Remove all desktop entries
    let desktop_dir = paths::desktop_dir();
    if desktop_dir.exists() {
        for entry in std::fs::read_dir(&desktop_dir)? {
            if let Ok(entry) = entry {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("voidbox-") && name.ends_with(".desktop") {
                    std::fs::remove_file(entry.path())?;
                    println!("  Removed {}", entry.path().display());
                }
            }
        }
    }

    if purge {
        // Remove entire data directory
        let data_dir = paths::data_dir();
        if data_dir.exists() {
            println!("  Removing data directory (this may take a moment)...");
            std::fs::remove_dir_all(&data_dir)?;
            println!("  Removed {}", data_dir.display());
        }
    } else {
        println!();
        println!("  Note: App data kept at {}", paths::data_dir().display());
        println!("  Use --purge to remove everything.");
    }

    println!();
    println!("[voidbox] Uninstall complete!");

    Ok(())
}

/// App launcher mode - triggered when invoked as void_<app>
fn run_as_launcher(app_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    if let Err(e) = cli::run_launcher(app_name) {
        if gui::is_gui_mode() {
            gui::show_error(
                &format!("Voidbox Error"),
                &format!("Failed to launch {}:\n\n{}", app_name, e),
            );
        } else {
            eprintln!("Error: {}", e);
        }
        std::process::exit(1);
    }
    Ok(())
}

/// GUI installation mode - triggered when double-clicking the binary
fn gui_install_mode() -> Result<(), Box<dyn std::error::Error>> {
    use voidbox::desktop;
    use voidbox::gui::{InstallType, run_installer};

    // Check if already installed
    if desktop::is_installed() {
        gui::show_info(
            "Voidbox Already Installed",
            &format!(
                "Voidbox v{} is already installed.\n\n\
                You can:\n\
                - Install apps with: voidbox install <app.toml>\n\
                - Run apps with: voidbox run <app>\n\
                - List apps with: voidbox list\n\n\
                Open a terminal to use voidbox commands.",
                voidbox::VERSION
            ),
        );
        return Ok(());
    }

    // Run the native installer
    if let Err(e) = run_installer(InstallType::SelfInstall) {
        eprintln!("GUI Error: {}", e);
        // Fallback to text mode if GUI fails (unlikely with egui)
        println!("Falling back to terminal mode...");
    }

    Ok(())
}

fn gui_bundle_install_mode(
    info: bundle::BundleManifestInfo,
) -> Result<(), Box<dyn std::error::Error>> {
    use voidbox::gui::{InstallType, run_installer};

    if let Err(e) = run_installer(InstallType::AppInstall {
        name: info.app_name,
        display_name: info.display_name,
        manifest_content: info.manifest_content,
    }) {
        eprintln!("GUI Error: {}", e);
    }

    Ok(())
}

fn cli_bundle_install_mode(
    info: bundle::BundleManifestInfo,
) -> Result<(), Box<dyn std::error::Error>> {
    use voidbox::storage::paths;

    if !voidbox::desktop::is_installed() {
        install_self()?;
        if !paths::is_bin_dir_in_path() {
            eprintln!(
                "[voidbox] Warning: {} is not in your PATH.",
                paths::bin_dir().display()
            );
            eprintln!(
                "[voidbox] Add this to your shell config: export PATH=\"$HOME/.local/bin:$PATH\""
            );
        }
    }

    println!("[voidbox] Installing {}...", info.display_name);
    let bundle_data = bundle::extract_embedded_bundle()?.ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "Bundle data missing")
    })?;
    let install_result = cli::install_app_from_bundle(
        &bundle_data.manifest_content,
        &bundle_data.archive_path,
        &bundle_data.archive_ext,
        false,
    );
    bundle_data.cleanup();
    install_result?;
    println!(
        "[voidbox] {} installed. Run with: voidbox run {}",
        info.display_name, info.app_name
    );
    Ok(())
}
